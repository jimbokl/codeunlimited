//! Deterministic usage monitor. No prompts, provider calls or quota estimates.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::parsers::{self, ScanOptions, ScanStats};
use crate::types::Request;

const WEEK: i64 = 7 * 86_400;
const SESSION_RECORD_CAP: usize = 20;
const MAX_STATE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Default, Serialize, Deserialize)]
struct Group {
    source: String,
    project: String,
    model: String,
    records: u64,
    sessions: u64,
    prompt_tokens: u64,
    cached_tokens: u64,
    output_tokens: u64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Summary {
    records: u64,
    sessions: u64,
    prompt_tokens: u64,
    cached_tokens: u64,
    output_tokens: u64,
    accounting_complete: bool,
    groups: BTreeMap<String, Group>,
}

#[derive(Serialize, Deserialize)]
struct Comparison {
    status: String,
    input_reduction_pct: Option<f64>,
    output_reduction_pct: Option<f64>,
    matched_baseline_records: u64,
    matched_current_records: u64,
    current_coverage_pct: Option<f64>,
    causal_savings_verified: bool,
    quality: String,
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    checked_at: i64,
    window_start: i64,
    observed: Summary,
    cohort: Summary,
    comparison: Comparison,
    scan_stats: Value,
    scan_duration_ms: u64,
    missing_timestamps: u64,
}

#[derive(Serialize, Deserialize)]
struct State {
    schema_version: u32,
    enabled: bool,
    activated_at: i64,
    baseline_from: i64,
    scheduler: String,
    claude_root: PathBuf,
    codex_root: PathBuf,
    roots_existed: [bool; 2],
    existing_sessions: BTreeSet<String>,
    baseline: Summary,
    baseline_scan_stats: Value,
    snapshots: Vec<Snapshot>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn key(parts: &[&str]) -> String {
    // Length-delimited JSON avoids collisions from separators inside log labels.
    format!("{:x}", Sha256::digest(serde_json::to_vec(parts).unwrap()))
}

fn session_key(r: &Request) -> String {
    key(&[r.source, &r.session])
}

fn summarize(rows: &[&Request], complete: bool) -> Summary {
    let mut out = Summary {
        accounting_complete: complete,
        ..Summary::default()
    };
    let mut sessions = BTreeSet::new();
    let mut group_sessions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    out.accounting_complete &= !parsers::counters_overflow(rows.iter().copied());
    for r in rows {
        let id = key(&[r.source, &r.project, &r.model]);
        let sid = session_key(r);
        sessions.insert(sid.clone());
        group_sessions.entry(id.clone()).or_default().insert(sid);
        let group = out.groups.entry(id).or_insert_with(|| Group {
            source: r.source.to_string(),
            project: r.project.to_string(),
            model: r.model.to_string(),
            ..Group::default()
        });
        out.records += 1;
        group.records += 1;
        out.prompt_tokens = out.prompt_tokens.saturating_add(r.prompt_total());
        group.prompt_tokens = group.prompt_tokens.saturating_add(r.prompt_total());
        out.output_tokens = out.output_tokens.saturating_add(r.out);
        group.output_tokens = group.output_tokens.saturating_add(r.out);
        out.cached_tokens = out.cached_tokens.saturating_add(r.cached_in);
        group.cached_tokens = group.cached_tokens.saturating_add(r.cached_in);
    }
    out.sessions = sessions.len() as u64;
    for (id, group) in &mut out.groups {
        group.sessions = group_sessions[id].len() as u64;
    }
    out
}

fn cohort<'a>(
    rows: &'a [Request],
    from: i64,
    to: i64,
    excluded: &BTreeSet<String>,
) -> Vec<&'a Request> {
    let mut sessions: BTreeMap<String, Vec<&Request>> = BTreeMap::new();
    for r in rows {
        sessions.entry(session_key(r)).or_default().push(r);
    }
    let mut selected = Vec::new();
    for (id, mut session) in sessions {
        if excluded.contains(&id) || session.iter().any(|r| r.ts.is_none()) {
            continue;
        }
        session.sort_by_key(|r| r.ts);
        if session[0].ts.is_some_and(|ts| ts >= from && ts <= to) {
            selected.extend(
                session
                    .into_iter()
                    .filter(|r| r.ts.is_some_and(|ts| ts <= to))
                    .take(SESSION_RECORD_CAP),
            );
        }
    }
    selected
}

fn compare(base: &Summary, current: &Summary) -> Comparison {
    let mut comparison = Comparison {
        status: "insufficient_data".into(),
        input_reduction_pct: None,
        output_reduction_pct: None,
        matched_baseline_records: 0,
        matched_current_records: 0,
        current_coverage_pct: None,
        causal_savings_verified: false,
        quality: "unknown: local usage logs do not measure task correctness".into(),
    };
    if !base.accounting_complete || !current.accounting_complete {
        comparison.status = "incomplete_accounting".into();
        return comparison;
    }
    let (mut expected_input, mut actual_input, mut expected_output, mut actual_output) =
        (0.0, 0.0, 0.0, 0.0);
    for (id, new) in &current.groups {
        let Some(old) = base.groups.get(id) else {
            continue;
        };
        if old.records == 0 || new.records == 0 || old.model == "?" || old.project == "?" {
            continue;
        }
        comparison.matched_baseline_records += old.records;
        comparison.matched_current_records += new.records;
        expected_input += old.prompt_tokens as f64 / old.records as f64 * new.records as f64;
        expected_output += old.output_tokens as f64 / old.records as f64 * new.records as f64;
        actual_input += new.prompt_tokens as f64;
        actual_output += new.output_tokens as f64;
    }
    if current.records > 0 {
        comparison.current_coverage_pct =
            Some(100.0 * comparison.matched_current_records as f64 / current.records as f64);
    }
    // Sessions can span models/projects; use the unique overall count, not a sum
    // of stratum counts. Require coverage so one tiny overlap cannot dominate.
    if comparison.matched_baseline_records >= 100
        && comparison.matched_current_records >= 100
        && base.sessions >= 3
        && current.sessions >= 3
        && comparison
            .current_coverage_pct
            .is_some_and(|pct| pct >= 80.0)
        && expected_input > 0.0
    {
        comparison.status = "observational_change".into();
        comparison.input_reduction_pct = Some(100.0 * (1.0 - actual_input / expected_input));
        comparison.output_reduction_pct =
            (expected_output > 0.0).then(|| 100.0 * (1.0 - actual_output / expected_output));
    }
    comparison
}

fn scan(claude: &Path, codex: &Path) -> (Vec<Request>, ScanStats) {
    let options = ScanOptions {
        project: None,
        since: None,
        use_index: false,
    };
    let mut first = parsers::scan_claude_at(claude, &options);
    let second = parsers::scan_codex_at(codex, &options);
    first.requests.extend(second.requests);
    first.stats += second.stats;
    first.stats.counter_overflow |= parsers::counters_overflow(&first.requests);
    (first.requests, first.stats)
}

fn existing_files(claude: &Path, codex: &Path) -> io::Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    for (source, root) in [("claude", claude), ("codex", codex)] {
        if !root.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(root) {
            let entry = entry.map_err(io::Error::other)?;
            if entry.file_type().is_file() && entry.path().extension().is_some_and(|s| s == "jsonl")
            {
                ids.insert(key(&[
                    source,
                    &parsers::monitor_file_identity(entry.path()),
                ]));
            }
            if ids.len() > 100_000 {
                return Err(invalid(
                    "too many log files for bounded monitor state (100000)",
                ));
            }
        }
    }
    Ok(ids)
}

fn state_path(dir: &Path) -> PathBuf {
    dir.join("state.json")
}

fn read(dir: &Path) -> io::Result<Option<State>> {
    let path = state_path(dir);
    if fs::symlink_metadata(&path).is_ok_and(|m| m.len() > MAX_STATE_BYTES) {
        return Err(invalid(
            "monitor state exceeds 32 MiB; preserve it and inspect manually",
        ));
    }
    let Some(text) = crate::safeio::read_optional_text(&path)? else {
        return Ok(None);
    };
    let state: State =
        serde_json::from_str(&text).map_err(|e| invalid(format!("invalid monitor state: {e}")))?;
    if state.schema_version != 1
        || !state.claude_root.is_absolute()
        || !state.codex_root.is_absolute()
        || state.snapshots.len() > 90
        || state.existing_sessions.len() > 100_000
    {
        return Err(invalid(
            "unsupported or invalid monitor state; not overwritten",
        ));
    }
    Ok(Some(state))
}

fn write(dir: &Path, state: &State) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(invalid("monitor state exceeds 32 MiB"));
    }
    crate::safeio::atomic_write(&state_path(dir), &bytes)
}

fn lock(dir: &Path) -> io::Result<File> {
    if fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(invalid("monitor directory must not be a symlink"));
    }
    let existed = dir.exists();
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !existed {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
    }
    #[cfg(not(unix))]
    let _ = existed;
    let path = dir.join("monitor.lock");
    crate::safeio::reject_symlink(&path)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    file.try_lock_exclusive().map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("monitor already running or lock unavailable: {e}"),
        )
    })?;
    Ok(file)
}

fn report(dir: &Path, state: &State) -> io::Result<()> {
    let text = if let Some(s) = state.snapshots.last() {
        let pct = s
            .comparison
            .input_reduction_pct
            .map_or_else(|| "not available".into(), |v| format!("{v:+.1}%"));
        format!("# Local usage monitor\n\nChecked (UTC epoch): {}. All locally discovered projects.\n\nObserved input: {} tokens; output: {}; cache reads: {}; retained usage records: {}.\n\nNew-session cohort (first 20 records/session): {} records across {} sessions.\n\nMatched input reduction: **{}**. Status: {}.\n\nPositive = less input per retained record; negative = more. This is an observational, source/model/project-matched comparison, not causal savings, billing or subscription quota. Task quality and the cost of external monitoring agents are unknown. Baseline may already contain efficiency interventions.\n\nCollector: {} ms; complete accounting: {}. Daily snapshots retained: {} / 90. Per-project/model counters and matching coverage are in state.json (`monitor status`).\n", s.checked_at, s.observed.prompt_tokens, s.observed.output_tokens, s.observed.cached_tokens, s.observed.records, s.cohort.records, s.cohort.sessions, pct, s.comparison.status, s.scan_duration_ms, s.cohort.accounting_complete, state.snapshots.len())
    } else {
        "# Local usage monitor\n\nBaseline saved. Waiting for a check and new sessions; no savings estimate yet. All processing is local, with no LLM calls.\n".into()
    };
    crate::safeio::atomic_write(&dir.join("report.md"), text.as_bytes())
}

fn run_inner(action: &str, dir: &Path, no_schedule: bool, quiet: bool) -> io::Result<i32> {
    if !dir.is_absolute() {
        return Err(invalid("--state-dir must be absolute"));
    }
    if action == "status" {
        let Some(state) = read(dir)? else {
            println!("{}", json!({"enabled":false,"status":"not_initialized"}));
            return Ok(1);
        };
        println!("{}", serde_json::to_string_pretty(&json!({
            "schema_version":1, "enabled":state.enabled, "scheduler":state.scheduler,
            "activated_at":state.activated_at, "baseline":state.baseline,
            "latest":state.snapshots.last(), "snapshot_count":state.snapshots.len(),
            "report_path":dir.join("report.md"), "scheduler_health":"not_probed",
            "scope":"all locally discovered projects; retained usage records, not completed tasks"
        })).map_err(io::Error::other)?);
        return Ok(i32::from(!state.enabled));
    }
    let _lock = lock(dir)?;
    let mut existing = read(dir)?;
    if action == "enable" {
        if existing.is_none() {
            let activated_at = Utc::now().timestamp();
            let claude_root = std::env::var_os("CLAUDE_HOME")
                .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR"))
                .map(|p| PathBuf::from(p).join("projects"))
                .unwrap_or_else(parsers::claude_root);
            let codex_root = parsers::codex_root();
            if !claude_root.is_absolute() || !codex_root.is_absolute() {
                return Err(invalid("provider log directories must be absolute"));
            }
            // Include files without usage yet: an in-flight first turn must not
            // enter the new-session cohort when its response is logged later.
            let mut ids = existing_files(&claude_root, &codex_root)?;
            let (rows, stats) = scan(&claude_root, &codex_root);
            if !stats.complete_accounting() || rows.iter().any(|r| r.ts.is_none()) {
                return Err(invalid("baseline accounting incomplete or timestamps missing; repair local logs/discovery before enabling monitoring"));
            }
            let baseline = summarize(
                &cohort(&rows, activated_at - WEEK, activated_at, &BTreeSet::new()),
                true,
            );
            ids.extend(rows.iter().map(session_key));
            if ids.len() > 100_000 {
                return Err(invalid(
                    "too many sessions for bounded monitor state (100000)",
                ));
            }
            existing = Some(State {
                schema_version: 1,
                enabled: false,
                activated_at,
                baseline_from: activated_at - WEEK,
                scheduler: "external".into(),
                roots_existed: [claude_root.is_dir(), codex_root.is_dir()],
                claude_root,
                codex_root,
                existing_sessions: ids,
                baseline,
                baseline_scan_stats: json!(stats),
                snapshots: vec![],
            });
            // Persist the baseline before scheduler activation; a retry must not reset it.
            write(dir, existing.as_ref().unwrap())?;
        }
        let state = existing.as_mut().unwrap();
        if !no_schedule {
            state.scheduler = crate::monitor_schedule::install(&std::env::current_exe()?, dir)?;
        }
        state.enabled = true;
        write(dir, state)?;
        report(dir, state)?;
        println!(
            "Monitor enabled for all local projects ({}). Report: {}. No LLM/API calls.",
            state.scheduler,
            dir.join("report.md").display()
        );
        return Ok(0);
    }
    let Some(mut state) = existing else {
        return Err(invalid("monitor is not initialized; run monitor enable"));
    };
    if action == "disable" {
        state.enabled = false;
        write(dir, &state)?;
        // Stop our collector even when an edited/foreign scheduler resource
        // must be preserved. A lingering owned invocation then becomes a no-op.
        crate::monitor_schedule::remove(dir).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("collection disabled; scheduler cleanup needs attention: {e}"),
            )
        })?;
        println!("Monitor disabled. Existing reports and baseline retained.");
        return Ok(0);
    }
    if !state.enabled {
        return Ok(0);
    }
    let started = std::time::Instant::now();
    let now = Utc::now().timestamp();
    let (rows, stats) = scan(&state.claude_root, &state.codex_root);
    let missing_timestamps = rows.iter().filter(|r| r.ts.is_none()).count() as u64;
    let roots_available = (!state.roots_existed[0] || state.claude_root.is_dir())
        && (!state.roots_existed[1] || state.codex_root.is_dir());
    let complete = stats.complete_accounting() && missing_timestamps == 0 && roots_available;
    let window_start = (now - WEEK).max(state.activated_at);
    let observed_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.ts.is_some_and(|ts| ts >= window_start && ts <= now))
        .collect();
    let observed = summarize(&observed_rows, complete);
    let current = summarize(
        &cohort(&rows, window_start, now, &state.existing_sessions),
        complete,
    );
    let comparison = compare(&state.baseline, &current);
    let snapshot = Snapshot {
        checked_at: now,
        window_start,
        observed,
        cohort: current,
        comparison,
        scan_stats: json!(stats),
        scan_duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        missing_timestamps,
    };
    state
        .snapshots
        .retain(|s| s.checked_at.div_euclid(86400) != now.div_euclid(86400));
    state.snapshots.push(snapshot);
    state.snapshots.sort_by_key(|s| s.checked_at);
    if state.snapshots.len() > 90 {
        state.snapshots.drain(..state.snapshots.len() - 90);
    }
    write(dir, &state)?;
    report(dir, &state)?;
    if !complete {
        return Err(invalid(
            "incomplete accounting; report saved without a savings estimate",
        ));
    }
    if !quiet {
        println!("Local monitor updated: {}", dir.join("report.md").display());
    }
    Ok(0)
}

pub fn run(action: &str, state_dir: Option<PathBuf>, no_schedule: bool, quiet: bool) -> i32 {
    let dir = state_dir.unwrap_or_else(|| crate::registry::home_dir().join("monitor"));
    match run_inner(action, &dir, no_schedule, quiet) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Monitor: {e}");
            1
        }
    }
}
