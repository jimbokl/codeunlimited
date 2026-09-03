use std::fs;
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
