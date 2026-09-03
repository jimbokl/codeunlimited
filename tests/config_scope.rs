use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn base_cmd(state: &Path) -> Command {
    let mut command = Command::cargo_bin("codeunlimited").expect("binary");
    command
        .env("CLAUDE_HOME", state.join("claude"))
        .env("CODEX_HOME", state.join("codex"))
        .env("CODEUNLIMITED_HOME", state.join("state"));
    command
}

#[test]
fn explicit_project_uses_its_own_thresholds() {
    let project = TempDir::new().expect("project tempdir");
    let state = TempDir::new().expect("state tempdir");
    fs::write(
        project.path().join(".codeunlimited.toml"),
        "[thresholds]\ntrivial_output_tokens = 10\n",
    )
    .expect("project config");

    let key = codeunlimited::parsers::claude_project_key(project.path());
    let logs = state.path().join("claude/projects").join(key);
    fs::create_dir_all(&logs).expect("log directory");
    fs::write(
        logs.join("session.jsonl"),
        r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-01-01T00:00:00Z","message":{"id":"m1","model":"claude-opus-5","usage":{"input_tokens":100000,"output_tokens":20}}}"#,
    )
    .expect("log fixture");

    base_cmd(state.path())
        .args(["audit", "--project"])
        .arg(project.path())
        .arg("--json")
        .assert()
        .success()
        .stdout(predicate::str::contains("Top-tier model burned").not());
}

#[test]
fn fix_all_skips_globally_ignored_registered_project() {
    let keep_parent = TempDir::new().expect("keep parent");
    let skip_parent = TempDir::new().expect("skip parent");
    let state = TempDir::new().expect("state tempdir");
    let keep = keep_parent.path().join("keep-me");
    let skip = skip_parent.path().join("skip-me");
    fs::create_dir(&keep).expect("keep project");
    fs::create_dir(&skip).expect("skip project");

    base_cmd(state.path())
        .arg("fix")
        .arg(&keep)
        .assert()
        .success();
    base_cmd(state.path())
        .arg("fix")
        .arg(&skip)
        .assert()
        .success();
    fs::write(
        state.path().join("state/config.toml"),
        "ignore_projects = [\"skip-me\"]\n",
    )
    .expect("global config");

    base_cmd(state.path())
        .args(["fix", "--all", "--apply"])
        .assert()
        .success();

    assert!(keep.join("AGENTS.md").is_file());
    assert!(!skip.join("AGENTS.md").exists());
    assert!(!skip.join("CLAUDE.md").exists());
}

#[test]
fn fix_all_skips_project_ignored_by_its_local_config() {
    let project_parent = TempDir::new().expect("project parent");
    let state = TempDir::new().expect("state tempdir");
    let project = project_parent.path().join("local-ignore");
    fs::create_dir(&project).expect("project");

    base_cmd(state.path())
        .arg("fix")
        .arg(&project)
        .assert()
        .success();
    fs::write(
        project.join(".codeunlimited.toml"),
        "ignore_projects = [\"local-ignore\"]\n",
    )
    .expect("local config");

    base_cmd(state.path())
        .args(["fix", "--all", "--apply"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Skipping ignored project"));

    assert!(!project.join("AGENTS.md").exists());
    assert!(!project.join("CLAUDE.md").exists());
}
