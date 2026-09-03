use std::fs;
use std::io::Write;
use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn write_fixture(root: &Path) {
    let claude = root.join("claude/projects/project/session.jsonl");
    fs::create_dir_all(claude.parent().expect("Claude fixture parent"))
        .expect("Claude fixture directory");
    fs::write(
        claude,
        concat!(
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2000-01-01T00:00:00Z","message":{"id":"old","model":"claude-test","usage":{"input_tokens":10,"output_tokens":1}}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2099-01-01T00:00:00Z","message":{"id":"new","model":"claude-test","usage":{"input_tokens":20,"output_tokens":2}}}"#,
            "\n",
        ),
    )
    .expect("Claude fixture");

    let codex = root.join("codex/sessions/2099/01/session.jsonl");
    fs::create_dir_all(codex.parent().expect("Codex fixture parent"))
        .expect("Codex fixture directory");
    fs::write(
        codex,
        concat!(
            r#"{"type":"turn_context","payload":{"model":"gpt-test","cwd":"/work/target"}}"#,
            "\n",
            r#"{"timestamp":"2000-01-01T00:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":50,"output_tokens":5}}}}"#,
            "\n",
            r#"{"timestamp":"2099-01-01T00:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":200,"cached_input_tokens":100,"output_tokens":10}}}}"#,
            "\n",
        ),
    )
    .expect("Codex fixture");
}

fn run(root: &Path, args: &[&str]) -> Value {
    let output = Command::cargo_bin("codeunlimited")
        .expect("binary")
        .env("CLAUDE_HOME", root.join("claude"))
        .env("CODEX_HOME", root.join("codex"))
        .env("CODEUNLIMITED_HOME", root.join("state"))
        .args(args)
        .output()
        .expect("audit process");
    assert!(
        output.status.success(),
        "audit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("audit JSON")
}

fn write_codex_session(path: &Path, cwd: &str, model: &str, input_tokens: u64) {
    write_codex_session_at(path, cwd, model, input_tokens, "2099-01-01T00:00:00Z");
}

fn write_codex_session_at(path: &Path, cwd: &str, model: &str, input_tokens: u64, timestamp: &str) {
    fs::create_dir_all(path.parent().expect("Codex session parent"))
        .expect("Codex session directory");
    fs::write(
        path,
        format!(
            concat!(
                "{{\"type\":\"turn_context\",\"payload\":{{\"model\":\"{}\",\"cwd\":\"{}\"}}}}\n",
                "{{\"timestamp\":\"{}\",\"type\":\"event_msg\",",
                "\"payload\":{{\"type\":\"token_count\",\"info\":{{\"last_token_usage\":",
                "{{\"input_tokens\":{},\"cached_input_tokens\":0,\"output_tokens\":1}}}}}}}}\n"
            ),
            model, cwd, timestamp, input_tokens
        ),
    )
    .expect("Codex session");
}

fn write_two_codex_sessions(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let sessions = root.join("codex/sessions/2099/01");
    let target = sessions.join("target.jsonl");
    let other = sessions.join("other.jsonl");
    write_codex_session(&target, "/work/target", "gpt-target", 111);
    write_codex_session(&other, "/work/other", "gpt-other", 222);
    (target, other)
}

fn scoped_args() -> [&'static str; 7] {
    [
        "audit",
        "--source",
        "codex",
        "--project",
        "/work/target",
        "--json",
        "--scan-stats",
    ]
}

#[test]
fn codex_days_filter_counts_only_retained_usage_records() {
    let state = TempDir::new().expect("fixture root");
    write_fixture(state.path());

    let value = run(
        state.path(),
        &[
            "audit",
            "--source",
            "codex",
            "--days",
            "1",
            "--json",
            "--scan-stats",
            "--no-index",
        ],
    );

    assert_eq!(value["sources"]["codex"]["requests"], 1);
    assert_eq!(value["scan"]["usage_records"], 1);
    assert_eq!(value["scan"]["files_discovered"], 1);
    assert_eq!(value["scan"]["files_opened"], 1);
}

#[test]
fn claude_days_filter_counts_only_retained_usage_records() {
    let state = TempDir::new().expect("fixture root");
    write_fixture(state.path());

    let value = run(
        state.path(),
        &[
            "audit",
            "--source",
            "claude",
            "--days",
            "1",
            "--json",
            "--scan-stats",
            "--no-index",
        ],
    );

    assert_eq!(value["sources"]["claude"]["requests"], 1);
    assert_eq!(value["scan"]["usage_records"], 1);
    assert_eq!(value["scan"]["files_discovered"], 1);
    assert_eq!(value["scan"]["files_opened"], 1);
}

#[test]
fn scan_stats_requires_json_output() {
    let state = TempDir::new().expect("fixture root");
    write_fixture(state.path());

    Command::cargo_bin("codeunlimited")
        .expect("binary")
        .env("CLAUDE_HOME", state.path().join("claude"))
        .env("CODEX_HOME", state.path().join("codex"))
        .args(["audit", "--scan-stats"])
        .assert()
        .failure();
}

#[test]
fn ordinary_json_keeps_the_v1_shape() {
    let state = TempDir::new().expect("fixture root");
    write_fixture(state.path());

    let value = run(state.path(), &["audit", "--source", "codex", "--json"]);

    assert!(value.get("scan").is_none());
}

#[test]
fn repeated_scope_uses_index() {
    let state = TempDir::new().expect("fixture root");
    write_two_codex_sessions(state.path());

    let args = scoped_args();
    let first = run(state.path(), &args);
    let second = run(state.path(), &args);

    assert_eq!(first["scan"]["files_opened"], 2);
    assert_eq!(second["scan"]["files_opened"], 1);
    assert_eq!(second["scan"]["files_skipped_by_index"], 1);
    assert_eq!(first["sources"], second["sources"]);

    let raw = fs::read_to_string(state.path().join("state/codex-index-v1.json"))
        .expect("Codex metadata index");
    let index: Value = serde_json::from_str(&raw).expect("index JSON");
    assert_eq!(index["schema_version"], 1);
    assert!(!raw.contains("gpt-target"));
    assert!(!raw.contains("gpt-other"));
    assert!(!raw.contains("input_tokens"));
    assert!(!raw.contains("111"));
    assert!(!raw.contains("222"));
}

#[test]
fn scoped_scan_indexes_timestamps_from_excluded_files() {
    let state = TempDir::new().expect("fixture root");
    write_two_codex_sessions(state.path());

    run(state.path(), &scoped_args());

    let raw = fs::read_to_string(state.path().join("state/codex-index-v1.json"))
        .expect("Codex metadata index");
    let index: Value = serde_json::from_str(&raw).expect("index JSON");
    let other = index["files"]
        .as_object()
        .expect("indexed files")
        .values()
        .find(|entry| {
            entry["cwd_keys"]
                .as_array()
                .is_some_and(|keys| keys.iter().any(|key| key == "/work/other"))
        })
        .expect("other project entry");
    assert_eq!(other["min_ts"], 4_070_908_800i64);
    assert_eq!(other["max_ts"], 4_070_908_800i64);
}

#[test]
fn appended_file_is_reopened_before_it_can_be_skipped_again() {
    let state = TempDir::new().expect("fixture root");
    let (_, other) = write_two_codex_sessions(state.path());
    let args = scoped_args();
    run(state.path(), &args);
    assert_eq!(run(state.path(), &args)["scan"]["files_opened"], 1);

    writeln!(
        fs::OpenOptions::new()
            .append(true)
            .open(other)
            .expect("other session"),
        r#"{{"timestamp":"2099-01-01T00:01:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":333,"cached_input_tokens":0,"output_tokens":1}}}}}}}}"#
    )
    .expect("append session");

    let changed = run(state.path(), &args);
    assert_eq!(changed["sources"]["codex"]["requests"], 1);
    assert_eq!(changed["scan"]["files_opened"], 2);
    let warm = run(state.path(), &args);
    assert_eq!(warm["scan"]["files_opened"], 1);
    assert_eq!(warm["scan"]["files_skipped_by_index"], 1);
}

#[test]
fn truncated_file_is_reopened_and_reindexed() {
    let state = TempDir::new().expect("fixture root");
    let (_, other) = write_two_codex_sessions(state.path());
    let args = scoped_args();
    run(state.path(), &args);

    fs::write(
        other,
        r#"{"type":"turn_context","payload":{"model":"gpt-other","cwd":"/work/other"}}
"#,
    )
    .expect("truncate session");

    let changed = run(state.path(), &args);
    assert_eq!(changed["sources"]["codex"]["requests"], 1);
    assert_eq!(changed["scan"]["files_opened"], 2);
    assert_eq!(run(state.path(), &args)["scan"]["files_opened"], 1);
}

#[test]
fn deleted_files_are_pruned_from_the_index() {
    let state = TempDir::new().expect("fixture root");
    let (_, other) = write_two_codex_sessions(state.path());
    let args = scoped_args();
    run(state.path(), &args);

    fs::remove_file(other).expect("delete other session");
    let after = run(state.path(), &args);
    assert_eq!(after["scan"]["files_discovered"], 1);
    assert_eq!(after["scan"]["files_opened"], 1);

    let raw = fs::read_to_string(state.path().join("state/codex-index-v1.json"))
        .expect("Codex metadata index");
    let index: Value = serde_json::from_str(&raw).expect("index JSON");
    assert_eq!(index["files"].as_object().expect("indexed files").len(), 1);
}

#[test]
fn invalid_json_index_is_rebuilt_without_changing_results() {
    let state = TempDir::new().expect("fixture root");
    write_two_codex_sessions(state.path());
    let args = scoped_args();
    let first = run(state.path(), &args);
    let index_path = state.path().join("state/codex-index-v1.json");
    fs::write(&index_path, "{not json").expect("corrupt index");

    let rebuilt = run(state.path(), &args);
    assert_eq!(rebuilt["sources"], first["sources"]);
    assert_eq!(rebuilt["scan"]["files_opened"], 2);
    let index: Value = serde_json::from_slice(&fs::read(index_path).expect("rebuilt index"))
        .expect("valid rebuilt index");
    assert_eq!(index["schema_version"], 1);
}

#[cfg(unix)]
#[test]
fn symlinked_index_disables_cache_without_replacing_the_target() {
    use std::os::unix::fs::symlink;

    let state = TempDir::new().expect("fixture root");
    write_two_codex_sessions(state.path());
    let args = scoped_args();
    run(state.path(), &args);
    let index_path = state.path().join("state/codex-index-v1.json");
    fs::remove_file(&index_path).expect("remove generated index");
    let outside = state.path().join("outside.json");
    fs::write(&outside, "keep\n").expect("outside fixture");
    symlink(&outside, &index_path).expect("symlink index");

    let uncached = run(state.path(), &args);
    assert_eq!(uncached["sources"]["codex"]["requests"], 1);
    assert_eq!(uncached["scan"]["files_opened"], 2);
    assert_eq!(
        fs::read_to_string(outside).expect("outside preserved"),
        "keep\n"
    );
    assert!(fs::symlink_metadata(index_path)
        .expect("index symlink")
        .file_type()
        .is_symlink());
}

#[test]
fn no_index_neither_reads_nor_writes_the_cache() {
    let state = TempDir::new().expect("fixture root");
    write_two_codex_sessions(state.path());
    let args = [
        "audit",
        "--source",
        "codex",
        "--project",
        "/work/target",
        "--json",
        "--scan-stats",
        "--no-index",
    ];

    let first = run(state.path(), &args);
    let second = run(state.path(), &args);
    assert_eq!(first["sources"], second["sources"]);
    assert_eq!(first["scan"]["files_opened"], 2);
    assert_eq!(second["scan"]["files_opened"], 2);
    assert!(!state.path().join("state/codex-index-v1.json").exists());
}

#[test]
fn project_skip_takes_precedence_over_date_skip() {
    let state = TempDir::new().expect("fixture root");
    let sessions = state.path().join("codex/sessions/2099/01");
    write_codex_session(
        &sessions.join("target.jsonl"),
        "/work/target",
        "gpt-target",
        111,
    );
    write_codex_session_at(
        &sessions.join("other.jsonl"),
        "/work/other",
        "gpt-other",
        222,
        "2000-01-01T00:00:00Z",
    );
    run(state.path(), &scoped_args());

    let scoped = run(
        state.path(),
        &[
            "audit",
            "--source",
            "codex",
            "--project",
            "/work/target",
            "--days",
            "1",
            "--json",
            "--scan-stats",
        ],
    );
    assert_eq!(scoped["scan"]["files_skipped_by_index"], 1);
    assert_eq!(scoped["scan"]["files_skipped_by_date"], 0);

    let unscoped = run(
        state.path(),
        &[
            "audit",
            "--source",
            "codex",
            "--days",
            "1",
            "--json",
            "--scan-stats",
        ],
    );
    assert_eq!(unscoped["scan"]["files_skipped_by_index"], 0);
    assert_eq!(unscoped["scan"]["files_skipped_by_date"], 1);
}

#[test]
fn claude_deduplicates_before_applying_the_time_cutoff() {
    let state = TempDir::new().expect("fixture root");
    let session = state.path().join("claude/projects/project/session.jsonl");
    fs::create_dir_all(session.parent().expect("Claude session parent"))
        .expect("Claude session directory");
    fs::write(
        session,
        concat!(
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2000-01-01T00:00:00Z","message":{"id":"same","model":"claude-test","usage":{"input_tokens":10,"output_tokens":1}}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2099-01-01T00:00:00Z","message":{"id":"same","model":"claude-test","usage":{"input_tokens":20,"output_tokens":2}}}"#,
            "\n",
        ),
    )
    .expect("Claude duplicate fixture");

    let value = run(
        state.path(),
        &[
            "audit",
            "--source",
            "claude",
            "--days",
            "1",
            "--json",
            "--scan-stats",
            "--no-index",
        ],
    );

    assert!(value["sources"].get("claude").is_none());
}

#[test]
fn malformed_known_codex_fields_do_not_hide_other_usage_metadata() {
    let state = TempDir::new().expect("fixture root");
    let session = state.path().join("codex/sessions/2099/01/session.jsonl");
    fs::create_dir_all(session.parent().expect("Codex session parent"))
        .expect("Codex session directory");
    fs::write(
        session,
        concat!(
            r#"{"type":"turn_context","payload":{"model":42,"cwd":"/work/target"}}"#,
            "\n",
            r#"{"timestamp":"2099-01-01T00:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":"bad","window_minutes":60}},"info":{"last_token_usage":{"input_tokens":"bad","cached_input_tokens":5,"output_tokens":2}}}}"#,
            "\n",
        ),
    )
    .expect("Codex malformed-field fixture");
    let args = scoped_args();

    let first = run(state.path(), &args);
    let warm = run(state.path(), &args);

    assert_eq!(first["sources"]["codex"]["requests"], 1);
    assert_eq!(first["sources"]["codex"]["prompt_tokens"], 5);
    assert_eq!(warm["sources"], first["sources"]);
    assert_eq!(warm["scan"]["files_opened"], 1);
}
