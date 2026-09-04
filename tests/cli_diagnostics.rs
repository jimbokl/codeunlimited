use std::fmt::Write as _;
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

fn write_delta_fixture(project: &Path, home: &Path, requests: u64) {
    let key = codeunlimited::parsers::claude_project_key(project);
    let log = home.join("claude/projects").join(key).join("delta.jsonl");
    fs::create_dir_all(log.parent().expect("delta fixture parent"))
        .expect("delta fixture directory");
    let mut rows = String::new();
    for index in 0..requests {
        writeln!(
            rows,
            "{{\"type\":\"assistant\",\"sessionId\":\"s\",\"timestamp\":\"2099-01-01T00:00:00Z\",\"message\":{{\"id\":\"m{index}\",\"model\":\"private-model\",\"usage\":{{\"input_tokens\":500,\"output_tokens\":1}}}}}}"
        )
        .expect("delta fixture row");
    }
    fs::write(log, rows).expect("delta fixture");
    fs::write(
        project.join(codeunlimited::deltacmd::BASELINE_FILE),
        r#"{"created_unix":0,"source":"claude","metrics":{"requests":100,"avg_prompt_per_turn":1000,"context_growth":1.0}}"#,
    )
    .expect("delta baseline");
}

#[test]
fn delta_with_zero_requests_reports_exact_insufficient_sample() {
    let project = TempDir::new().expect("project");
    let home = TempDir::new().expect("isolated homes");
    write_delta_fixture(project.path(), home.path(), 0);

    let output = binary()
        .env("CLAUDE_HOME", home.path().join("claude"))
        .env("CODEX_HOME", home.path().join("codex"))
        .arg("delta")
        .arg(project.path())
        .output()
        .expect("delta process");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("delta UTF-8");
    assert!(stdout.contains("insufficient sample (0/100 requests)"));
    assert!(!stdout.contains("VERDICT"));
}

#[test]
fn delta_with_nine_requests_reports_insufficient_sample_without_direction() {
    let project = TempDir::new().expect("project");
    let home = TempDir::new().expect("isolated homes");
    write_delta_fixture(project.path(), home.path(), 9);

    let output = binary()
        .env("CLAUDE_HOME", home.path().join("claude"))
        .env("CODEX_HOME", home.path().join("codex"))
        .arg("delta")
        .arg(project.path())
        .output()
        .expect("delta process");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("delta UTF-8");
    assert!(stdout.contains("insufficient sample (9/100 requests)"));
    assert!(!stdout.contains("VERDICT"));
    assert!(!stdout.contains("more work"));
    assert!(!stdout.contains("leaks are growing"));
}

#[test]
fn delta_with_one_hundred_requests_retains_directional_verdict() {
    let project = TempDir::new().expect("project");
    let home = TempDir::new().expect("isolated homes");
    write_delta_fixture(project.path(), home.path(), 100);

    let output = binary()
        .env("CLAUDE_HOME", home.path().join("claude"))
        .env("CODEX_HOME", home.path().join("codex"))
        .arg("delta")
        .arg(project.path())
        .output()
        .expect("delta process");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("delta UTF-8");
    assert!(stdout.contains("VERDICT: context per turn is down 50%"));
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
