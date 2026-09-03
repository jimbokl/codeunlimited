//! Parsers for Claude Code (~/.claude/projects) and Codex CLI (~/.codex/sessions) logs.

use std::collections::{BTreeSet, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
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
        let msg = d.get("message").cloned().unwrap_or(Value::Null);
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
        let cc = u.get("cache_creation").cloned().unwrap_or(Value::Null);
        let w1h = u64_of(&cc, "ephemeral_1h_input_tokens");
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
        let Ok(d) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if d.get("type").and_then(Value::as_str) == Some("turn_context") {
            let payload = d.get("payload").unwrap_or(&Value::Null);
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
        let p = d.get("payload").cloned().unwrap_or(Value::Null);
        if p.get("type").and_then(Value::as_str) != Some("token_count") {
            continue;
        }
        let ts = d
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_ts);
        observation.observe_ts(ts);
        if let Some(w) = want {
            if cwd_key != w {
                continue;
            }
        }
        if since.is_some_and(|cutoff| ts.is_none_or(|value| value < cutoff)) {
            continue;
        }
        if let Some(pr) = p.get("rate_limits").and_then(|rl| rl.get("primary")) {
            let used = pr
                .get("used_percent")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let win = pr
                .get("window_minutes")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            series.push((ts.unwrap_or(0), used, win));
        }
        let Some(u) = p
            .get("info")
            .and_then(|i| i.get("last_token_usage"))
            .filter(|u| u.is_object())
        else {
            continue;
        };
        let inp = u64_of(u, "input_tokens");
        let cch = u64_of(u, "cached_input_tokens");
        out.push(Request {
            source: "codex",
            project: Arc::clone(&project),
            session: Arc::clone(&session),
            ts,
            model: Arc::clone(&model),
            unc_in: inp.saturating_sub(cch),
            cached_in: cch,
            w5: u64_of(u, "cache_write_input_tokens"),
            w1h: 0,
            out: u64_of(u, "output_tokens"),
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
    let parts: Vec<(&CodexCandidate, CodexFileScan)> = candidates
        .par_iter()
        .map(|candidate| {
            (
                candidate,
                parse_codex_file(&candidate.path, want.as_deref(), options.since),
            )
        })
        .collect();
    let mut out = Vec::new();
    let mut series: LimitSeries = Vec::new();
    for (candidate, part) in parts {
        let Some((reqs, s, file_stats, observation)) = part else {
            continue;
        };
        stats += file_stats;
        out.extend(reqs);
        series.extend(s);
        if let (IndexAccess::Enabled(index), Some(fingerprint)) =
            (&mut index, candidate.fingerprint.clone())
        {
            index
                .files
                .insert(candidate.key.clone(), observation.into_entry(fingerprint));
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
