//! Parsers for Claude Code (~/.claude/projects) and Codex CLI (~/.codex/sessions) logs.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::ops::AddAssign;
use std::path::{Path, PathBuf};
use std::sync::{mpsc::sync_channel, Arc};

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
    }
}

pub struct ClaudeScan {
    pub requests: Vec<Request>,
    pub stats: ScanStats,
}

pub struct CodexScan {
    pub requests: Vec<Request>,
    pub series: LimitSeries,
    pub stats: ScanStats,
}

type ClaudeRecord = (Option<String>, Request);
type ClaudeFileScan = Option<(Vec<ClaudeRecord>, ScanStats)>;

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

type CodexFileScan = Option<(Vec<Request>, LimitSeries, ScanStats, FileObservation)>;

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

#[derive(serde::Deserialize)]
struct CodexRecord<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(borrow)]
    payload: Option<CodexPayload<'a>>,
}

#[derive(Default, serde::Deserialize)]
struct CodexPayload<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
    #[serde(borrow)]
    cwd: Option<Cow<'a, str>>,
    info: Option<CodexInfo>,
    rate_limits: Option<CodexRateLimits>,
}

#[derive(serde::Deserialize)]
struct CodexInfo {
    last_token_usage: Option<CodexTokenUsage>,
}

#[derive(serde::Deserialize)]
struct CodexTokenUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    cache_write_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[derive(serde::Deserialize)]
struct CodexRateLimits {
    primary: Option<CodexRateLimit>,
}

#[derive(serde::Deserialize)]
struct CodexRateLimit {
    #[serde(default)]
    used_percent: f64,
    #[serde(default)]
    window_minutes: u64,
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

fn jsonl_files(base: &Path) -> Vec<PathBuf> {
    WalkDir::new(base)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .map(|e| e.into_path())
        .collect()
}

fn u64_of(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
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
    let f = File::open(path).ok()?;
    let mut stats = ScanStats {
        files_opened: 1,
        ..ScanStats::default()
    };
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let Ok(line) = line else { continue };
        if !line.contains("\"usage\"") || !line.contains("\"assistant\"") {
            continue;
        }
        let Ok(d) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if d.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let msg = d.get("message").unwrap_or(&Value::Null);
        let Some(u) = msg.get("usage").filter(|u| u.is_object()) else {
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
        if since.is_some_and(|cutoff| ts.is_none_or(|value| value < cutoff)) {
            continue;
        }
        let mid = msg.get("id").and_then(Value::as_str).map(str::to_string);
        let cw = u64_of(u, "cache_creation_input_tokens");
        let cc = u.get("cache_creation").unwrap_or(&Value::Null);
        let w1h = u64_of(cc, "ephemeral_1h_input_tokens");
        let w5 = cc
            .get("ephemeral_5m_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(cw.saturating_sub(w1h));
        out.push((
            mid,
            Request {
                source: "claude",
                project: Arc::clone(&project),
                session: d
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(Arc::from)
                    .unwrap_or_else(|| Arc::clone(&file_session)),
                ts,
                model,
                unc_in: u64_of(u, "input_tokens"),
                cached_in: u64_of(u, "cache_read_input_tokens"),
                w5,
                w1h,
                out: u64_of(u, "output_tokens"),
            },
        ));
        stats.usage_records = stats.usage_records.saturating_add(1);
    }
    Some((out, stats))
}

pub fn scan_claude(options: &ScanOptions) -> ClaudeScan {
    let root = claude_root();
    if !root.is_dir() {
        return ClaudeScan {
            requests: vec![],
            stats: ScanStats::default(),
        };
    }
    let files: Vec<PathBuf> = match options.project.as_deref() {
        Some(p) => {
            let key = claude_project_key(p).to_lowercase();
            std::fs::read_dir(&root)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().to_lowercase() == key)
                .flat_map(|e| jsonl_files(&e.path()))
                .collect()
        }
        None => jsonl_files(&root),
    };
    let mut stats = ScanStats {
        files_discovered: files.len() as u64,
        ..ScanStats::default()
    };
    let parsed: Vec<ClaudeFileScan> = files
        .par_iter()
        .map(|f| parse_claude_file(f, options.since))
        .collect();
    // Streamed chunks repeat the same message id - keep the first occurrence.
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (chunk, file_stats) in parsed.into_iter().flatten() {
        stats += file_stats;
        for (mid, r) in chunk {
            if let Some(id) = mid {
                if !seen.insert(id) {
                    continue;
                }
            }
            out.push(r);
        }
    }
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

fn parse_codex_file(path: &Path, want: Option<&str>, since: Option<i64>) -> CodexFileScan {
    let f = File::open(path).ok()?;
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
    for line in BufReader::new(f).lines() {
        let Ok(line) = line else { continue };
        if !line.contains("\"token_count\"") && !line.contains("\"turn_context\"") {
            continue;
        }
        if let Some(cutoff) = since {
            if line.contains("\"token_count\"") && !line.contains("\"turn_context\"") {
                if let Some(ts) = leading_timestamp(&line) {
                    observation.observe_ts(Some(ts));
                    if ts < cutoff {
                        continue;
                    }
                }
            }
        }
        let Ok(record) = serde_json::from_str::<CodexRecord<'_>>(&line) else {
            continue;
        };
        let payload = record.payload.unwrap_or_default();
        if record.kind.as_deref() == Some("turn_context") {
            if let Some(value) = payload.model {
                model = Arc::from(value.as_ref());
            }
            if let Some(value) = payload.cwd {
                cwd_key = path_key(Path::new(value.as_ref()));
                project = project_label(value.as_ref()).into();
                observation.cwd_keys.insert(cwd_key.clone());
            }
            continue;
        }
        let ts = record.timestamp.as_deref().and_then(parse_ts);
        observation.observe_ts(ts);
        if since.is_some_and(|cutoff| ts.is_none_or(|value| value < cutoff)) {
            continue;
        }
        if payload.kind.as_deref() != Some("token_count") {
            continue;
        }
        if let Some(w) = want {
            if cwd_key != w {
                continue;
            }
        }
        if let Some(primary) = payload
            .rate_limits
            .as_ref()
            .and_then(|limits| limits.primary.as_ref())
        {
            series.push((
                ts.unwrap_or(0),
                primary.used_percent,
                primary.window_minutes,
            ));
        }
        let Some(usage) = payload
            .info
            .as_ref()
            .and_then(|info| info.last_token_usage.as_ref())
        else {
            continue;
        };
        out.push(Request {
            source: "codex",
            project: Arc::clone(&project),
            session: Arc::clone(&session),
            ts,
            model: Arc::clone(&model),
            unc_in: usage.input_tokens.saturating_sub(usage.cached_input_tokens),
            cached_in: usage.cached_input_tokens,
            w5: usage.cache_write_input_tokens,
            w1h: 0,
            out: usage.output_tokens,
        });
        stats.usage_records = stats.usage_records.saturating_add(1);
    }
    Some((out, series, stats, observation))
}

/// Full Codex scan: requests plus the observed rate-limit series (ts-sorted).
pub fn scan_codex(options: &ScanOptions) -> CodexScan {
    let root = codex_root();
    if !root.is_dir() {
        return CodexScan {
            requests: vec![],
            series: vec![],
            stats: ScanStats::default(),
        };
    }
    let want = options.project.as_deref().map(path_key);
    let files = jsonl_files(&root);
    let mut stats = ScanStats {
        files_discovered: files.len() as u64,
        ..ScanStats::default()
    };
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
    let queue_capacity = rayon::current_num_threads().max(1).saturating_mul(2);
    let (sender, receiver) = sync_channel::<ParsedCandidate>(queue_capacity);
    let aggregate = std::thread::scope(|scope| {
        let consumer = scope.spawn(|| {
            let mut aggregate = CodexAggregate::default();
            for parsed in receiver {
                let Some((mut reqs, mut series, file_stats, observation)) = parsed.scan else {
                    continue;
                };
                aggregate.stats += file_stats;
                aggregate.requests.append(&mut reqs);
                aggregate.series.append(&mut series);
                if let Some(fingerprint) = parsed.fingerprint {
                    aggregate
                        .observations
                        .push((parsed.key, fingerprint, observation));
                }
            }
            aggregate
        });
        candidates
            .par_iter()
            .for_each_with(sender, |sender, candidate| {
                let _ = sender.send(ParsedCandidate {
                    key: candidate.key.clone(),
                    fingerprint: candidate.fingerprint.clone(),
                    scan: parse_codex_file(&candidate.path, want.as_deref(), options.since),
                });
            });
        consumer.join().expect("Codex aggregation thread panicked")
    });
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
}
