//! Parsers for Claude Code (~/.claude/projects) and Codex CLI (~/.codex/sessions) logs.

use std::collections::{BTreeSet, HashSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::ops::AddAssign;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;
use serde_json::Value;
use walkdir::WalkDir;

use crate::scan_index::{self, FileFingerprint, FileIndexEntry, IndexAccess};
use crate::types::{parse_ts, Request};

#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    pub project: Option<PathBuf>,
    pub since: Option<i64>,
    pub use_index: bool,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ScanStats {
    pub files_discovered: u64,
    pub files_opened: u64,
    pub files_skipped_by_date: u64,
    pub files_skipped_by_index: u64,
    pub usage_records: u64,
    pub malformed_records: u64,
    pub files_failed: u64,
    pub discovery_errors: u64,
    pub duplicate_usage_records: u64,
    pub usage_without_cumulative_counters: u64,
    pub cumulative_resets: u64,
    pub counter_overflow: bool,
}

impl ScanStats {
    /// Completeness of the recognized counters, not proof of request identity.
    pub fn complete_accounting(&self) -> bool {
        self.malformed_records == 0
            && self.files_failed == 0
            && self.discovery_errors == 0
            && !self.counter_overflow
    }
}

impl AddAssign for ScanStats {
    fn add_assign(&mut self, rhs: Self) {
        self.files_discovered = self.files_discovered.saturating_add(rhs.files_discovered);
        self.files_opened = self.files_opened.saturating_add(rhs.files_opened);
        self.files_skipped_by_date = self
            .files_skipped_by_date
            .saturating_add(rhs.files_skipped_by_date);
        self.files_skipped_by_index = self
            .files_skipped_by_index
            .saturating_add(rhs.files_skipped_by_index);
        self.usage_records = self.usage_records.saturating_add(rhs.usage_records);
        self.malformed_records = self.malformed_records.saturating_add(rhs.malformed_records);
        self.files_failed = self.files_failed.saturating_add(rhs.files_failed);
        self.discovery_errors = self.discovery_errors.saturating_add(rhs.discovery_errors);
        self.duplicate_usage_records = self
            .duplicate_usage_records
            .saturating_add(rhs.duplicate_usage_records);
        self.usage_without_cumulative_counters = self
            .usage_without_cumulative_counters
            .saturating_add(rhs.usage_without_cumulative_counters);
        self.cumulative_resets = self.cumulative_resets.saturating_add(rhs.cumulative_resets);
        if rhs.counter_overflow {
            self.counter_overflow = true;
        }
    }
}

pub struct ClaudeScan {
    pub requests: Vec<Request>,
    pub stats: ScanStats,
}

/// Validate aggregate representability before presenting or persisting totals.
pub fn counters_overflow<'a>(requests: impl IntoIterator<Item = &'a Request>) -> bool {
    requests
        .into_iter()
        .try_fold(0u64, |total, r| {
            [r.unc_in, r.cached_in, r.w5, r.w1h, r.out]
                .into_iter()
                .try_fold(total, u64::checked_add)
        })
        .is_none()
}

pub struct CodexScan {
    pub requests: Vec<Request>,
    pub series: LimitSeries,
    pub stats: ScanStats,
}

type ClaudeMessageId = (Arc<str>, Arc<str>, String);
type ClaudeRecord = (Option<ClaudeMessageId>, Option<Request>);
type ClaudeFileScan = io::Result<(Vec<ClaudeRecord>, ScanStats)>;

#[derive(Default)]
struct FileObservation {
    cwd_keys: BTreeSet<String>,
    min_ts: Option<i64>,
    max_ts: Option<i64>,
}

impl FileObservation {
    fn observe_ts(&mut self, ts: Option<i64>) {
        if let Some(ts) = ts {
            self.min_ts = Some(self.min_ts.map_or(ts, |current| current.min(ts)));
            self.max_ts = Some(self.max_ts.map_or(ts, |current| current.max(ts)));
        }
    }

    fn into_entry(self, fingerprint: FileFingerprint) -> FileIndexEntry {
        FileIndexEntry {
            fingerprint,
            cwd_keys: self.cwd_keys.into_iter().collect(),
            min_ts: self.min_ts,
            max_ts: self.max_ts,
        }
    }
}

struct CodexCandidate {
    path: PathBuf,
    key: String,
    fingerprint: Option<FileFingerprint>,
}

type CodexFileScan = io::Result<(Vec<Request>, LimitSeries, ScanStats, FileObservation)>;

struct ParsedCandidate {
    key: String,
    fingerprint: Option<FileFingerprint>,
    scan: CodexFileScan,
}

#[derive(Default)]
struct CodexAggregate {
    requests: Vec<Request>,
    series: LimitSeries,
    stats: ScanStats,
    observations: Vec<(String, FileFingerprint, FileObservation)>,
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

pub fn claude_root() -> PathBuf {
    std::env::var_os("CLAUDE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"))
        .join("projects")
}

pub fn codex_root() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".codex"))
        .join("sessions")
}

/// Claude Code stores a project's logs under a directory whose name is the
/// project cwd with every non-alphanumeric character replaced by '-'.
pub fn claude_project_key(project: &Path) -> String {
    let canon = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let s = canon.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s); // windows verbatim prefix
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn jsonl_files(base: &Path, stats: &mut ScanStats) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in WalkDir::new(base) {
        match entry {
            Ok(e)
                if e.file_type().is_file()
                    && e.path().extension().is_some_and(|x| x == "jsonl") =>
            {
                files.push(e.into_path())
            }
            Ok(_) => {}
            Err(_) => stats.discovery_errors += 1,
        }
    }
    files.sort_unstable();
    files
}

fn jsonl_files_checked(base: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(base) {
        let entry = entry.map_err(io::Error::other)?;
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "jsonl")
        {
            files.push(entry.into_path());
        }
    }
    files.sort_unstable();
    Ok(files)
}

fn u64_of(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn valid_counters(v: &Value, fields: &[&str]) -> bool {
    v.is_object()
        && fields
            .iter()
            .all(|key| v.get(key).is_none_or(|n| n.as_u64().is_some()))
}

fn has_required_counters(v: &Value) -> bool {
    ["input_tokens", "output_tokens"]
        .iter()
        .all(|key| v.get(key).and_then(Value::as_u64).is_some())
}

fn source_available(root: &Path, stats: &mut ScanStats) -> bool {
    match std::fs::metadata(root) {
        Ok(m) if m.is_dir() => true,
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        _ => {
            stats.discovery_errors += 1;
            false
        }
    }
}

fn parse_claude_file(path: &Path, since: Option<i64>) -> ClaudeFileScan {
    let project: Arc<str> = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned().into())
        .unwrap_or_else(|| Arc::from(""));
    let file_session: Arc<str> = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .into();
    let f = File::open(path)?;
    let mut stats = ScanStats {
        files_opened: 1,
        ..ScanStats::default()
    };
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        if !line.contains("\"assistant\"") {
            continue;
        }
        let Ok(d) = serde_json::from_str::<Value>(&line) else {
            stats.malformed_records += 1;
            continue;
        };
        if !d.is_object() {
            stats.malformed_records += 1;
            continue;
        }
        if d.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let msg = d.get("message").unwrap_or(&Value::Null);
        let Some(u) = msg.get("usage").filter(|u| u.is_object()) else {
            stats.malformed_records += 1;
            continue;
        };
        let model: Arc<str> = msg
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .into();
        if model.contains("<synthetic>") {
            continue;
        }
        let ts = d
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_ts);
        let session = d
            .get("sessionId")
            .and_then(Value::as_str)
            .map(Arc::from)
            .unwrap_or_else(|| Arc::clone(&file_session));
        let mid = msg
            .get("id")
            .and_then(Value::as_str)
            .map(|id| (Arc::clone(&project), Arc::clone(&session), id.to_string()));
        if since.is_some_and(|cutoff| ts.is_none_or(|value| value < cutoff)) {
            // Keep the id so global first-seen deduplication stays identical to
            // an unfiltered scan, without retaining the excluded request.
            out.push((mid, None));
            continue;
        }
        if !has_required_counters(u)
            || !valid_counters(
                u,
                &[
                    "input_tokens",
                    "output_tokens",
                    "cache_read_input_tokens",
                    "cache_creation_input_tokens",
                ],
            )
            || u.get("cache_creation").is_some_and(|cc| {
                !valid_counters(
                    cc,
                    &["ephemeral_1h_input_tokens", "ephemeral_5m_input_tokens"],
                )
            })
        {
            stats.malformed_records += 1;
            continue;
        }
        let cw = u64_of(u, "cache_creation_input_tokens");
        let cc = u.get("cache_creation").unwrap_or(&Value::Null);
        let w1h = u64_of(cc, "ephemeral_1h_input_tokens");
        let w5 = cc
            .get("ephemeral_5m_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(cw.saturating_sub(w1h));
        if w5.checked_add(w1h).is_none()
            || (u.get("cache_creation_input_tokens").is_some() && w5.checked_add(w1h) != Some(cw))
        {
            stats.malformed_records += 1;
            continue;
        }
        out.push((
            mid,
            Some(Request {
                source: "claude",
                project: Arc::clone(&project),
                session,
                ts,
                model,
                unc_in: u64_of(u, "input_tokens"),
                cached_in: u64_of(u, "cache_read_input_tokens"),
                w5,
                w1h,
                out: u64_of(u, "output_tokens"),
            }),
        ));
        stats.usage_records = stats.usage_records.saturating_add(1);
    }
    Ok((out, stats))
}

pub fn scan_claude(options: &ScanOptions) -> ClaudeScan {
    let root = claude_root();
    let mut stats = ScanStats::default();
    if !source_available(&root, &mut stats) {
        return ClaudeScan {
            requests: vec![],
            stats,
        };
    }
    let files: Vec<PathBuf> = match options.project.as_deref() {
        Some(p) => {
            let key = claude_project_key(p).to_lowercase();
            let mut files = Vec::new();
            match std::fs::read_dir(&root) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(e) if e.file_name().to_string_lossy().to_lowercase() == key => {
                                files.extend(jsonl_files(&e.path(), &mut stats))
                            }
                            Ok(_) => {}
                            Err(_) => stats.discovery_errors += 1,
                        }
                    }
                }
                Err(_) => stats.discovery_errors += 1,
            }
            files.sort_unstable();
            files
        }
        None => jsonl_files(&root, &mut stats),
    };
    stats.files_discovered = files.len() as u64;
    let parsed: Vec<ClaudeFileScan> = files
        .par_iter()
        .map(|f| parse_claude_file(f, options.since))
        .collect();
    // Streamed chunks repeat the same message id - keep the first occurrence.
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for scan in parsed {
        let Ok((chunk, file_stats)) = scan else {
            stats.files_failed += 1;
            continue;
        };
        stats += file_stats;
        for (mid, request) in chunk {
            if let Some(id) = mid {
                if !seen.insert(id) {
                    stats.duplicate_usage_records += 1;
                    continue;
                }
            }
            if let Some(request) = request {
                out.push(request);
            }
        }
    }
    stats.counter_overflow |= counters_overflow(&out);
    ClaudeScan {
        requests: out,
        stats,
    }
}

pub fn iter_claude(project: Option<&Path>) -> Vec<Request> {
    scan_claude(&ScanOptions {
        project: project.map(Path::to_path_buf),
        since: None,
        use_index: false,
    })
    .requests
}

/// Strict Claude scan used by experiment accounting. Missing source roots are
/// empty, but discovery and file I/O failures abort the measurement.
pub fn iter_claude_checked(project: Option<&Path>) -> io::Result<Vec<Request>> {
    let root = claude_root();
    if !root.exists() {
        return Ok(Vec::new());
    }

    let files = match project {
        Some(project) => {
            let key = claude_project_key(project).to_lowercase();
            let mut files = Vec::new();
            for entry in std::fs::read_dir(&root)? {
                let entry = entry?;
                if entry.file_name().to_string_lossy().to_lowercase() == key {
                    files.extend(jsonl_files_checked(&entry.path())?);
                }
            }
            files
        }
        None => jsonl_files_checked(&root)?,
    };

    let parsed: Vec<ClaudeFileScan> = files
        .par_iter()
        .map(|path| parse_claude_file(path, None))
        .collect();
    let mut seen = HashSet::new();
    let mut requests = Vec::new();
    for scan in parsed {
        let (records, stats) = scan?;
        if !stats.complete_accounting() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "malformed Claude usage counters",
            ));
        }
        for (message_id, request) in records {
            if let Some(message_id) = message_id {
                if !seen.insert(message_id) {
                    continue;
                }
            }
            if let Some(request) = request {
                requests.push(request);
            }
        }
    }
    if counters_overflow(&requests) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude counter aggregation overflow",
        ));
    }
    Ok(requests)
}

fn normalize_path_text(raw: &str) -> String {
    let windows = raw.starts_with(r"\\?\")
        || raw.contains('\\')
        || raw.as_bytes().get(1).is_some_and(|b| *b == b':');
    let slash = if raw
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"\\?\UNC\"))
    {
        format!("//{}", raw.get(8..).unwrap_or_default().replace('\\', "/"))
    } else {
        raw.strip_prefix(r"\\?\").unwrap_or(raw).replace('\\', "/")
    };
    let absolute = slash.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in slash.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            ".." if !absolute => parts.push(part),
            ".." => {}
            _ => parts.push(part),
        }
    }
    let mut key = if absolute {
        "/".to_string()
    } else {
        String::new()
    };
    key.push_str(&parts.join("/"));
    if windows {
        key.make_ascii_lowercase();
    }
    key
}

fn path_key(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    normalize_path_text(&canonical.to_string_lossy())
}

fn project_label(cwd: &str) -> String {
    normalize_path_text(cwd)
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("?")
        .to_string()
}

/// Peak observed rate-limit usage: (used_percent, window_minutes).
pub type LimitPeak = Option<(f64, u64)>;
/// Observed rate-limit usage over time: (unix_ts, used_percent, window_minutes).
pub type LimitSeries = Vec<(i64, f64, u64)>;

pub fn peak(series: &LimitSeries) -> LimitPeak {
    series
        .iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|&(_, u, w)| (u, w))
}

#[cfg(test)]
fn leading_timestamp(line: &str) -> Option<i64> {
    const PREFIX: &str = r#"{"timestamp":""#;
    if !line.starts_with(PREFIX) || line.matches("\"timestamp\"").count() != 1 {
        return None;
    }
    let value = line.get(PREFIX.len()..)?.split('"').next()?;
    if value.contains('\\') {
        return None;
    }
    parse_ts(value)
}

const CODEX_COUNTERS: [&str; 7] = [
    "input_tokens",
    "cached_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
    "total_tokens",
    "cache_write_input_tokens",
    "cached_tokens",
];
type CodexSnapshot = [Option<u64>; 7];

fn codex_snapshot(usage: &Value) -> CodexSnapshot {
    CODEX_COUNTERS.map(|key| usage.get(key).and_then(Value::as_u64))
}

fn parse_codex_file(path: &Path, want: Option<&str>, since: Option<i64>) -> CodexFileScan {
    let f = File::open(path)?;
    let mut stats = ScanStats {
        files_opened: 1,
        ..ScanStats::default()
    };
    let session: Arc<str> = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .into();
    let mut model: Arc<str> = Arc::from("?");
    let mut project: Arc<str> = Arc::from("?");
    let mut cwd_key = String::new();
    let mut out = Vec::new();
    let mut series: LimitSeries = Vec::new();
    let mut observation = FileObservation::default();
    // Track before project/date filtering: a later rate-limit event may repeat
    // usage from outside the requested window. Missing counters break the chain.
    let mut previous_usage: Option<(CodexSnapshot, CodexSnapshot)> = None;
    for line in BufReader::new(f).lines() {
        let line = line?;
        if !line.contains("\"token_count\"")
            && !line.contains("\"turn_context\"")
            && !line.contains("\"event_msg\"")
        {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            stats.malformed_records += 1;
            previous_usage = None;
            continue;
        };
        if !record.is_object() {
            stats.malformed_records += 1;
            previous_usage = None;
            continue;
        }
        let payload = record.get("payload").unwrap_or(&Value::Null);
        if record.get("type").and_then(Value::as_str) == Some("turn_context") {
            if let Some(value) = payload.get("model").and_then(Value::as_str) {
                model = Arc::from(value);
            }
            if let Some(value) = payload.get("cwd").and_then(Value::as_str) {
                cwd_key = path_key(Path::new(value));
                project = project_label(value).into();
                observation.cwd_keys.insert(cwd_key.clone());
            }
            continue;
        }
        let ts = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_ts);
        observation.observe_ts(ts);
        if payload.get("type").and_then(Value::as_str) != Some("token_count") {
            continue;
        }
        let in_scope = since.is_none_or(|cutoff| ts.is_some_and(|value| value >= cutoff))
            && want.is_none_or(|w| cwd_key == w);
        if let Some(primary) = payload
            .get("rate_limits")
            .and_then(|limits| limits.get("primary"))
            .filter(|_| in_scope)
        {
            series.push((
                ts.unwrap_or(0),
                primary
                    .get("used_percent")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
                primary
                    .get("window_minutes")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            ));
        }
        let Some(usage) = payload
            .get("info")
            .and_then(|info| info.get("last_token_usage"))
            .filter(|usage| usage.is_object())
        else {
            if payload.get("info").is_some_and(|info| !info.is_null()) {
                stats.malformed_records += 1;
                previous_usage = None;
            }
            continue;
        };
        let cumulative = payload
            .get("info")
            .and_then(|info| info.get("total_token_usage"))
            .filter(|v| !v.is_null());
        if !has_required_counters(usage)
            || !valid_counters(usage, &CODEX_COUNTERS)
            || cumulative.is_some_and(|u| {
                !valid_counters(u, &CODEX_COUNTERS)
                    || u64_of(u, "cached_input_tokens") > u64_of(u, "input_tokens")
            })
            || u64_of(usage, "cached_input_tokens") > u64_of(usage, "input_tokens")
        {
            stats.malformed_records += 1;
            previous_usage = None;
            continue;
        }
        if let Some(total) = cumulative.filter(|u| {
            u.get("input_tokens").and_then(Value::as_u64).is_some()
                && u.get("output_tokens").and_then(Value::as_u64).is_some()
        }) {
            let current = (codex_snapshot(total), codex_snapshot(usage));
            if previous_usage.as_ref() == Some(&current) {
                if in_scope {
                    stats.duplicate_usage_records += 1;
                }
                continue;
            }
            if previous_usage.as_ref().is_some_and(|old| {
                old.0
                    .iter()
                    .zip(current.0)
                    .any(|(a, b)| matches!((a, b), (Some(a), Some(b)) if b < *a))
            }) && in_scope
            {
                stats.cumulative_resets += 1;
            }
            previous_usage = Some(current);
        } else {
            previous_usage = None;
            if in_scope {
                stats.usage_without_cumulative_counters += 1;
            }
        }
        if !in_scope {
            continue;
        }
        let input_tokens = u64_of(usage, "input_tokens");
        let cached_input_tokens = u64_of(usage, "cached_input_tokens");
        out.push(Request {
            source: "codex",
            project: Arc::clone(&project),
            session: Arc::clone(&session),
            ts,
            model: Arc::clone(&model),
            unc_in: input_tokens.saturating_sub(cached_input_tokens),
            cached_in: cached_input_tokens,
            w5: u64_of(usage, "cache_write_input_tokens"),
            w1h: 0,
            out: u64_of(usage, "output_tokens"),
        });
        stats.usage_records = stats.usage_records.saturating_add(1);
    }
    Ok((out, series, stats, observation))
}

fn parse_codex_batch(
    candidates: &[CodexCandidate],
    want: Option<&str>,
    since: Option<i64>,
) -> Vec<ParsedCandidate> {
    candidates
        .par_iter()
        .map(|candidate| ParsedCandidate {
            key: candidate.key.clone(),
            fingerprint: candidate.fingerprint.clone(),
            scan: parse_codex_file(&candidate.path, want, since),
        })
        .collect()
}

/// Full Codex scan: requests plus the observed rate-limit series (ts-sorted).
pub fn scan_codex(options: &ScanOptions) -> CodexScan {
    let root = codex_root();
    let mut stats = ScanStats::default();
    if !source_available(&root, &mut stats) {
        return CodexScan {
            requests: vec![],
            series: vec![],
            stats,
        };
    }
    let want = options.project.as_deref().map(path_key);
    let files = jsonl_files(&root, &mut stats);
    stats.files_discovered = files.len() as u64;
    let mut index = if options.use_index {
        IndexAccess::load()
    } else {
        IndexAccess::Disabled
    };
    let mut discovered_keys = HashSet::new();
    let mut candidates = Vec::new();
    for path in files {
        let key = scan_index::file_key(&path);
        discovered_keys.insert(key.clone());
        let fingerprint = match &index {
            IndexAccess::Enabled(_) => scan_index::fingerprint(&path).ok(),
            IndexAccess::Disabled => None,
        };
        let cached = match (&index, &fingerprint) {
            (IndexAccess::Enabled(index), Some(fingerprint)) => index
                .files
                .get(&key)
                .filter(|entry| entry.fingerprint == *fingerprint),
            _ => None,
        };
        if let Some(entry) = cached {
            let wrong_project = want
                .as_ref()
                .is_some_and(|key| !entry.cwd_keys.iter().any(|cwd| cwd == key));
            if wrong_project {
                stats.files_skipped_by_index = stats.files_skipped_by_index.saturating_add(1);
                continue;
            }
            let entirely_old = options
                .since
                .is_some_and(|cutoff| entry.max_ts.is_some_and(|ts| ts < cutoff));
            if entirely_old {
                stats.files_skipped_by_date = stats.files_skipped_by_date.saturating_add(1);
                continue;
            }
        }
        candidates.push(CodexCandidate {
            path,
            key,
            fingerprint,
        });
    }
    let batch_size = rayon::current_num_threads().max(1).saturating_mul(2);
    let mut aggregate = CodexAggregate::default();
    for batch in candidates.chunks(batch_size) {
        for parsed in parse_codex_batch(batch, want.as_deref(), options.since) {
            let Ok((mut reqs, mut series, file_stats, observation)) = parsed.scan else {
                aggregate.stats.files_failed += 1;
                continue;
            };
            aggregate.stats += file_stats;
            aggregate.requests.append(&mut reqs);
            aggregate.series.append(&mut series);
            if let Some(fingerprint) = parsed
                .fingerprint
                .filter(|_| file_stats.complete_accounting())
            {
                aggregate
                    .observations
                    .push((parsed.key, fingerprint, observation));
            }
        }
    }
    stats += aggregate.stats;
    let mut out = aggregate.requests;
    out.shrink_to_fit();
    let mut series = aggregate.series;
    if let IndexAccess::Enabled(index) = &mut index {
        for (key, fingerprint, observation) in aggregate.observations {
            index.files.insert(key, observation.into_entry(fingerprint));
        }
    }
    if let IndexAccess::Enabled(index) = &mut index {
        index.files.retain(|key, _| discovered_keys.contains(key));
    }
    index.save();
    series.sort_unstable_by_key(|&(t, _, _)| t);
    stats.counter_overflow |= counters_overflow(&out);
    CodexScan {
        requests: out,
        series,
        stats,
    }
}

pub fn iter_codex_full(project: Option<&Path>) -> (Vec<Request>, LimitSeries) {
    let scan = scan_codex(&ScanOptions {
        project: project.map(Path::to_path_buf),
        since: None,
        use_index: false,
    });
    (scan.requests, scan.series)
}

pub fn iter_codex(project: Option<&Path>) -> Vec<Request> {
    iter_codex_full(project).0
}

/// Strict Codex scan used by experiment accounting. Missing source roots are
/// empty, but discovery and file I/O failures abort the measurement.
pub fn iter_codex_checked(project: Option<&Path>) -> io::Result<Vec<Request>> {
    let root = codex_root();
    if !root.exists() {
        return Ok(Vec::new());
    }
    if !root.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Codex session root is not a directory: {}", root.display()),
        ));
    }

    let want = project.map(path_key);
    let files = jsonl_files_checked(&root)?;
    let parsed: Vec<CodexFileScan> = files
        .par_iter()
        .map(|path| parse_codex_file(path, want.as_deref(), None))
        .collect();
    let mut requests = Vec::new();
    for scan in parsed {
        let (mut file_requests, _, stats, _) = scan?;
        if !stats.complete_accounting() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "malformed Codex usage counters",
            ));
        }
        requests.append(&mut file_requests);
    }
    if counters_overflow(&requests) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Codex counter aggregation overflow",
        ));
    }
    Ok(requests)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn leading_timestamp_requires_one_unescaped_first_field() {
        assert_eq!(
            leading_timestamp(r#"{"timestamp":"2099-01-01T00:00:00Z","type":"event_msg"}"#),
            Some(4_070_908_800)
        );
        assert_eq!(
            leading_timestamp(r#"{"type":"event_msg","timestamp":"2099-01-01T00:00:00Z"}"#),
            None
        );
        assert_eq!(
            leading_timestamp(
                r#"{"timestamp":"2099-01-01T00:00:00Z","timestamp":"2000-01-01T00:00:00Z"}"#
            ),
            None
        );
        assert_eq!(
            leading_timestamp(r#"{"timestamp":"2099-01-01T00:00:00\u005a"}"#),
            None
        );
    }

    #[test]
    fn codex_records_in_one_context_share_metadata_allocations() {
        let root = TempDir::new().expect("fixture root");
        let path = root.path().join("session.jsonl");
        fs::write(
            &path,
            concat!(
                r#"{"type":"turn_context","payload":{"model":"gpt-shared","cwd":"/work/shared"}}"#,
                "\n",
                r#"{"timestamp":"2099-01-01T00:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":1}}}}"#,
                "\n",
                r#"{"timestamp":"2099-01-01T00:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":20,"cached_input_tokens":0,"output_tokens":2}}}}"#,
                "\n",
            ),
        )
        .expect("fixture session");

        let (requests, _, _, _) = parse_codex_file(&path, None, None).expect("parsed file");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].session.as_ptr(), requests[1].session.as_ptr());
        assert_eq!(requests[0].project.as_ptr(), requests[1].project.as_ptr());
        assert_eq!(requests[0].model.as_ptr(), requests[1].model.as_ptr());
    }

    #[test]
    fn codex_parallel_batch_preserves_candidate_order() {
        let root = TempDir::new().expect("fixture root");
        let first = root.path().join("a-first.jsonl");
        let second = root.path().join("b-second.jsonl");
        for (path, cwd) in [(&first, "/work/first"), (&second, "/work/second")] {
            fs::write(
                path,
                format!(
                    concat!(
                        "{{\"type\":\"turn_context\",\"payload\":{{\"model\":\"gpt-test\",\"cwd\":\"{}\"}}}}\n",
                        "{{\"timestamp\":\"2099-01-01T00:00:00Z\",\"type\":\"event_msg\",",
                        "\"payload\":{{\"type\":\"token_count\",\"info\":{{\"last_token_usage\":",
                        "{{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}}}}}}}\n"
                    ),
                    cwd
                ),
            )
            .expect("fixture session");
        }
        let candidates = vec![
            CodexCandidate {
                path: first,
                key: "first".to_string(),
                fingerprint: None,
            },
            CodexCandidate {
                path: second,
                key: "second".to_string(),
                fingerprint: None,
            },
        ];

        let parsed = parse_codex_batch(&candidates, None, None);
        let sessions: Vec<_> = parsed
            .into_iter()
            .map(|candidate| candidate.scan.expect("parsed candidate"))
            .flat_map(|(requests, _, _, _)| requests)
            .map(|request| request.session.to_string())
            .collect();

        assert_eq!(sessions, ["a-first", "b-second"]);
    }
}
