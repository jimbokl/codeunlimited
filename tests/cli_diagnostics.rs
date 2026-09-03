use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn binary() -> Command {
    Command::cargo_bin("codeunlimited").expect("binary")
}

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn doctor_fails_when_no_logs_exist() {
    let home = TempDir::new().expect("empty home");
    binary()
        .env("CLAUDE_HOME", home.path().join("claude"))
        .env("CODEX_HOME", home.path().join("codex"))
        .arg("doctor")
        .assert()
        .failure()
        .stdout(predicate::str::contains("no logs found"));
}

#[test]
fn doctor_accepts_one_healthy_available_source() {
    let home = TempDir::new().expect("empty home");
    binary()
        .env("CLAUDE_HOME", fixture("tests/fixtures/claude_home"))
        .env("CODEX_HOME", home.path().join("codex"))
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("claude: 1 files"));
}

#[test]
fn doctor_accepts_healthy_codex_when_claude_is_unavailable() {
    let home = TempDir::new().expect("empty home");
    binary()
        .env("CLAUDE_HOME", home.path().join("claude"))
        .env("CODEX_HOME", fixture("tests/fixtures/codex_home"))
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("codex : 1 files"));
}

#[test]
fn doctor_rejects_sources_with_no_usage_candidates() {
    let home = TempDir::new().expect("empty home");
    let logs = home.path().join("claude/projects/project");
    fs::create_dir_all(&logs).expect("log directory");
    fs::write(
        logs.join("session.jsonl"),
        r#"{"type":"system","message":"schema changed"}"#,
    )
    .expect("non-usage fixture");

    binary()
        .env("CLAUDE_HOME", home.path().join("claude"))
        .env("CODEX_HOME", home.path().join("codex"))
        .arg("doctor")
        .assert()
        .failure()
        .stdout(predicate::str::contains("no usage candidates recognized"));
}

#[test]
fn html_report_destination_is_rejected_before_history_changes() {
    let state = TempDir::new().expect("state home");
    let out = state.path().join("report.html");
    binary()
        .env("CLAUDE_HOME", fixture("tests/fixtures/claude_home"))
        .env("CODEX_HOME", fixture("tests/fixtures/codex_home"))
        .env("CODEUNLIMITED_HOME", state.path())
        .args(["report", "--all", "--out"])
        .arg(&out)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Markdown"));

    assert!(!out.exists());
    assert!(!state.path().join("history.jsonl").exists());
}

#[test]
fn failed_report_write_does_not_append_history() {
    let state = TempDir::new().expect("state home");
    let out = state.path().join("missing/report.md");
    binary()
        .env("CLAUDE_HOME", fixture("tests/fixtures/claude_home"))
        .env("CODEX_HOME", fixture("tests/fixtures/codex_home"))
        .env("CODEUNLIMITED_HOME", state.path())
        .args(["report", "--all", "--out"])
        .arg(&out)
        .assert()
        .failure();

    assert!(!state.path().join("history.jsonl").exists());
}

#[test]
fn corrupt_history_is_preserved_and_report_fails() {
    let state = TempDir::new().expect("state home");
    let history = state.path().join("history.jsonl");
    let out = state.path().join("summary.md");
    fs::write(&history, [0xff, 0xfe, 0xfd]).expect("corrupt history");

    binary()
        .env("CLAUDE_HOME", fixture("tests/fixtures/claude_home"))
        .env("CODEX_HOME", fixture("tests/fixtures/codex_home"))
        .env("CODEUNLIMITED_HOME", state.path())
        .args(["report", "--all", "--out"])
        .arg(&out)
        .assert()
        .failure();

    assert_eq!(
        fs::read(history).expect("history preserved"),
        [0xff, 0xfe, 0xfd]
    );
    assert!(!out.exists());
}

#[test]
fn malformed_json_history_is_preserved_and_report_fails() {
    let state = TempDir::new().expect("state home");
    let history = state.path().join("history.jsonl");
    let out = state.path().join("summary.md");
    fs::write(&history, "{not valid json}\n").expect("malformed history");

    binary()
        .env("CLAUDE_HOME", fixture("tests/fixtures/claude_home"))
        .env("CODEX_HOME", fixture("tests/fixtures/codex_home"))
        .env("CODEUNLIMITED_HOME", state.path())
        .args(["report", "--all", "--out"])
        .arg(&out)
        .assert()
        .failure();

    assert_eq!(
        fs::read_to_string(history).expect("history preserved"),
        "{not valid json}\n"
    );
    assert!(!out.exists());
}

#[test]
fn anonymized_report_does_not_expose_fixture_project_names() {
    let state = TempDir::new().expect("state home");
    let out = state.path().join("summary.md");
    binary()
        .env("CLAUDE_HOME", fixture("tests/fixtures/claude_home"))
        .env("CODEX_HOME", fixture("tests/fixtures/codex_home"))
        .env("CODEUNLIMITED_HOME", state.path())
        .args(["report", "--all", "--anonymize", "--out"])
        .arg(&out)
        .assert()
        .success();

    for report in [out.clone(), out.with_extension("html")] {
        let rendered = fs::read_to_string(report).expect("rendered report");
        assert!(!rendered.contains("C--test-proj"));
        assert!(!rendered.contains("testcx"));
        assert!(rendered.contains("proj-"));
    }
}

#[test]
fn invalid_day_windows_are_rejected_by_clap() {
    binary().args(["audit", "--days", "0"]).assert().failure();
    binary()
        .args(["audit", "--days", "36501"])
        .assert()
        .failure();
    binary().args(["compare", "--days", "0"]).assert().failure();
    binary()
        .args(["compare", "--days", "36501"])
        .assert()
        .failure();
}

#[test]
fn skill_preserves_custom_content_without_force() {
    let home = TempDir::new().expect("home");
    let skill = home.path().join(".claude/skills/codeunlimited/SKILL.md");
    fs::create_dir_all(skill.parent().expect("skill parent")).expect("skill directory");
    fs::write(&skill, "custom\n").expect("custom skill");

    binary()
        .env("HOME", home.path())
        .env_remove("USERPROFILE")
        .arg("skill")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--force"));

    assert_eq!(
        fs::read_to_string(&skill).expect("skill unchanged"),
        "custom\n"
    );

    binary()
        .env("HOME", home.path())
        .env_remove("USERPROFILE")
        .args(["skill", "--force"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(codeunlimited::safeio::backup_path(&skill)).expect("skill backup"),
        "custom\n"
    );
    assert_ne!(
        fs::read_to_string(skill).expect("installed skill"),
        "custom\n"
    );
}
