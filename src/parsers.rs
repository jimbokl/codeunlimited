//! Parsers for Claude Code (~/.claude/projects) and Codex CLI (~/.codex/sessions) logs.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde_json::Value;
use walkdir::WalkDir;

use crate::types::{parse_ts, Request};

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

fn parse_claude_file(path: &Path) -> Vec<(Option<String>, Request)> {
    let project = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Ok(f) = File::open(path) else {
        return vec![];
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
        let model = msg
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        if model.contains("<synthetic>") {
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
                project: project.clone(),
                session: d
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| path.file_stem().and_then(|s| s.to_str()).unwrap_or("?"))
                    .to_string(),
                ts: d
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(parse_ts),
                model,
                unc_in: u64_of(u, "input_tokens"),
                cached_in: u64_of(u, "cache_read_input_tokens"),
                w5,
                w1h,
                out: u64_of(u, "output_tokens"),
            },
        ));
    }
    out
}

pub fn iter_claude(project: Option<&Path>) -> Vec<Request> {
    let root = claude_root();
    if !root.is_dir() {
        return vec![];
    }
    let files: Vec<PathBuf> = match project {
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
    let parsed: Vec<Vec<(Option<String>, Request)>> =
        files.par_iter().map(|f| parse_claude_file(f)).collect();
    // Streamed chunks repeat the same message id - keep the first occurrence.
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for chunk in parsed {
        for (mid, r) in chunk {
            if let Some(id) = mid {
                if !seen.insert(id) {
                    continue;
                }
            }
            out.push(r);
        }
    }
    out
}

fn str_field<'a>(line: &'a str, pat: &str) -> Option<&'a str> {
    let i = line.find(pat)? + pat.len();
    let rest = &line[i..];
    let j = rest.find('"')?;
    Some(&rest[..j])
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

fn parse_codex_file(path: &Path, want: Option<&str>) -> (Vec<Request>, LimitSeries) {
    let Ok(f) = File::open(path) else {
        return (vec![], vec![]);
    };
    let mut model = String::from("?");
    let mut project = String::from("?");
    let mut out = Vec::new();
    let mut series: LimitSeries = Vec::new();
    for line in BufReader::new(f).lines() {
        let Ok(line) = line else { continue };
        if model == "?" {
            if let Some(m) = str_field(&line, "\"model\":\"") {
                model = m.to_string();
            }
        }
        if project == "?" {
            if let Some(c) = str_field(&line, "\"cwd\":\"") {
                let cwd = c.replace("\\\\", "\\");
                project = cwd
                    .split(['\\', '/'])
                    .rfind(|p| !p.is_empty())
                    .unwrap_or("?")
                    .to_string();
            }
        }
        if !line.contains("\"token_count\"") {
            continue;
        }
        if let Some(w) = want {
            if !project.eq_ignore_ascii_case(w) {
                continue;
            }
        }
        let Ok(d) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let p = d.get("payload").cloned().unwrap_or(Value::Null);
        if p.get("type").and_then(Value::as_str) != Some("token_count") {
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
            let ts = d
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_ts)
                .unwrap_or(0);
            series.push((ts, used, win));
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
            project: project.clone(),
            session: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string(),
            ts: d
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_ts),
            model: model.clone(),
            unc_in: inp.saturating_sub(cch),
            cached_in: cch,
            w5: u64_of(u, "cache_write_input_tokens"),
            w1h: 0,
            out: u64_of(u, "output_tokens"),
        });
    }
    (out, series)
}

/// Full Codex scan: requests plus the observed rate-limit series (ts-sorted).
pub fn iter_codex_full(project: Option<&Path>) -> (Vec<Request>, LimitSeries) {
    let root = codex_root();
    if !root.is_dir() {
        return (vec![], vec![]);
    }
    let want = project.map(|p| {
        p.canonicalize()
            .unwrap_or_else(|_| p.to_path_buf())
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let files = jsonl_files(&root);
    let parts: Vec<(Vec<Request>, LimitSeries)> = files
        .par_iter()
        .map(|f| parse_codex_file(f, want.as_deref()))
        .collect();
    let mut out = Vec::new();
    let mut series: LimitSeries = Vec::new();
    for (reqs, s) in parts {
        out.extend(reqs);
        series.extend(s);
    }
    series.sort_unstable_by_key(|&(t, _, _)| t);
    (out, series)
}

pub fn iter_codex(project: Option<&Path>) -> Vec<Request> {
    iter_codex_full(project).0
}
