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
