use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

fn binary() -> Command {
    Command::cargo_bin("codeunlimited").expect("binary")
}

fn python() -> &'static str {
    if cfg!(windows) {
        "python"
    } else {
        "python3"
    }
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_driver.py")
}

fn workflow(project: &Path) -> PathBuf {
    let path = project.join("workflow.md");
    fs::write(&path, "# Workflow\nPerform one bounded increment.\n").expect("workflow");
    path
}

fn init_command(project: &Path, name: &str, mode: &str, extra_provider_args: &[String]) -> Command {
    let skill = workflow(project);
    let mut command = binary();
    command
        .args(["run", "init", name, "--project"])
        .arg(project)
        .arg("--skill")
        .arg(skill)
        .args([
            "--objective",
            "Implement the fixture feature",
            "--provider",
            "command",
            "--provider-executable",
            python(),
        ]);
    for argument in [
        fixture().to_string_lossy().into_owned(),
        "--mode".into(),
        mode.into(),
        "--revision-from-prompt".into(),
    ]
    .into_iter()
    .chain(extra_provider_args.iter().cloned())
    {
        command.arg(format!("--provider-arg={argument}"));
    }
    command
}

fn initialize_git(project: &Path) {
    assert!(ProcessCommand::new("git")
        .args(["init", "-q"])
        .current_dir(project)
        .status()
        .expect("git init")
        .success());
    assert!(ProcessCommand::new("git")
        .args(["add", "."])
        .current_dir(project)
        .status()
        .expect("git add")
        .success());
    assert!(ProcessCommand::new("git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ])
        .current_dir(project)
        .status()
        .expect("git commit")
        .success());
}

fn json_output(mut command: Command) -> Value {
    let output = command.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("JSON output")
}

fn keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .expect("JSON object")
        .keys()
        .map(String::as_str)
        .collect()
}

#[test]
fn run_help_lists_the_complete_bounded_lifecycle() {
    binary()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("init"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("prompt"))
        .stdout(predicate::str::contains("step"))
        .stdout(predicate::str::contains("auto"))
        .stdout(predicate::str::contains("recover"));
}

#[test]
fn init_is_local_only_refuses_duplicates_and_prints_exact_ignore_guidance() {
    let project = TempDir::new().expect("project");
    let capture = project.path().join("provider-called");
    let extra = vec!["--capture".into(), capture.to_string_lossy().into_owned()];

    init_command(project.path(), "feature-x", "success", &extra)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Recommended .gitignore entry: .codeunlimited/runs/",
        ));
    assert!(!capture.exists(), "init must not invoke the provider");

    init_command(project.path(), "feature-x", "success", &extra)
        .assert()
        .failure()
        .stderr(predicate::str::contains("runtime[invalid_input]"));
    assert!(!capture.exists());
}

#[test]
fn status_json_is_versioned_strict_and_reports_provider_isolation() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "feature-x", "success", &[])
        .assert()
        .success();

    let mut command = binary();
    command
        .args(["run", "status", "feature-x", "--project"])
        .arg(project.path())
        .arg("--json");
    let status = json_output(command);

    assert_eq!(status["schema_version"], 1);
    assert_eq!(status["revision"], 0);
    assert_eq!(status["status"], "running");
    assert_eq!(status["provider"]["kind"], "command");
    assert_eq!(
        status["provider"]["isolation"],
        "external-process isolation"
    );
    assert_eq!(
        keys(&status),
        BTreeSet::from([
            "attempts",
            "busy",
            "dynamic_prompt_bytes",
            "prompt_bytes",
            "provider",
            "recovery_required",
            "revision",
            "run_name",
            "schema_version",
            "stable_prompt_bytes",
            "status",
            "usage",
        ])
    );
}

#[test]
fn prompt_is_read_only_and_step_invokes_exactly_once() {
    let project = TempDir::new().expect("project");
    let capture = project.path().join("provider-prompt");
    let extra = vec!["--capture".into(), capture.to_string_lossy().into_owned()];
    init_command(project.path(), "feature-x", "success", &extra)
        .assert()
        .success();

    binary()
        .args(["run", "prompt", "feature-x", "--project"])
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("CURRENT_STATE"));
    assert!(!capture.exists(), "prompt must not invoke the provider");

    binary()
        .args(["run", "step", "feature-x", "--project"])
        .arg(project.path())
        .arg("--json")
        .assert()
        .success();
    assert!(capture.exists(), "step must invoke the provider");
    assert_eq!(
        fs::read_dir(
            project
                .path()
                .join(".codeunlimited/runs/feature-x/attempts")
        )
        .expect("attempts")
        .count(),
        1
    );
}

#[test]
fn auto_requires_a_bound_and_never_exceeds_it() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "bounded", "success", &[])
        .assert()
        .success();

    binary()
        .args(["run", "auto", "bounded", "--project"])
        .arg(project.path())
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--steps"));

    let mut command = binary();
    command
        .args(["run", "auto", "bounded", "--project"])
        .arg(project.path())
        .args(["--steps", "3", "--json"]);
    let report = json_output(command);
    assert_eq!(report["steps"].as_array().unwrap().len(), 3);
    assert_eq!(report["steps"][2]["revision"], 3);
}

#[test]
fn auto_stops_when_a_run_becomes_terminal() {
    let project = TempDir::new().expect("project");
    init_command(
        project.path(),
        "terminal",
        "success",
        &["--outcome".into(), "complete".into()],
    )
    .arg("--allow-unverified-completion")
    .assert()
    .success();

    let mut command = binary();
    command
        .args(["run", "auto", "terminal", "--project"])
        .arg(project.path())
        .args(["--steps", "5", "--json"]);
    let report = json_output(command);
    assert_eq!(report["steps"].as_array().unwrap().len(), 1);
    assert_eq!(report["steps"][0]["status"], "complete");
}

#[test]
fn recover_requires_an_ambiguous_attempt_and_a_bounded_regular_observation() {
    let project = TempDir::new().expect("project");
    let changed = project.path().join("changed.txt");
    init_command(
        project.path(),
        "feature-x",
        "invalid",
        &["--change".into(), changed.to_string_lossy().into_owned()],
    )
    .assert()
    .success();
    initialize_git(project.path());

    let observation = project.path().join("recovery.txt");
    fs::write(&observation, "Accepted changed.txt after inspection\n").expect("observation");
    binary()
        .args(["run", "recover", "feature-x", "--project"])
        .arg(project.path())
        .arg("--observation")
        .arg(&observation)
        .assert()
        .failure()
        .stderr(predicate::str::contains("runtime[invalid_input]"));

    binary()
        .args(["run", "step", "feature-x", "--project"])
        .arg(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("runtime[recovery_required]"));
    assert!(changed.exists());

    binary()
        .args(["run", "recover", "feature-x", "--project"])
        .arg(project.path())
        .arg("--observation")
        .arg(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("runtime[invalid_input]"));

    binary()
        .args(["run", "recover", "feature-x", "--project"])
        .arg(project.path())
        .arg("--observation")
        .arg(&observation)
        .assert()
        .success();

    let mut command = binary();
    command
        .args(["run", "status", "feature-x", "--project"])
        .arg(project.path())
        .arg("--json");
    let status = json_output(command);
    assert_eq!(status["revision"], 1);
    assert_eq!(status["recovery_required"], false);
}

#[test]
fn failures_use_stable_categories_without_leaking_provider_output() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "failed", "failure", &[])
        .assert()
        .success();
    initialize_git(project.path());
    binary()
        .args(["run", "step", "failed", "--project"])
        .arg(project.path())
        .assert()
        .code(5)
        .stderr(predicate::str::contains("runtime[provider_failure]"))
        .stderr(predicate::str::contains("PRIVATE PROVIDER ERROR BODY").not());

    let project = TempDir::new().expect("project");
    init_command(project.path(), "invalid", "invalid", &[])
        .assert()
        .success();
    initialize_git(project.path());
    binary()
        .args(["run", "step", "invalid", "--project"])
        .arg(project.path())
        .assert()
        .code(7)
        .stderr(predicate::str::contains("runtime[invalid_transition]"));

    let project = TempDir::new().expect("project");
    let skill = workflow(project.path());
    binary()
        .args(["run", "init", "missing", "--project"])
        .arg(project.path())
        .arg("--skill")
        .arg(skill)
        .args([
            "--objective",
            "Test a missing provider",
            "--provider",
            "command",
            "--provider-executable",
            "definitely-not-a-codeunlimited-provider",
        ])
        .assert()
        .success();
    initialize_git(project.path());
    binary()
        .args(["run", "step", "missing", "--project"])
        .arg(project.path())
        .assert()
        .code(4)
        .stderr(predicate::str::contains("runtime[missing_provider]"));
}

#[test]
fn non_utf8_workflow_and_unicode_byte_overflow_fail_without_content_echo() {
    let project = TempDir::new().expect("project");
    let skill = project.path().join("workflow.bin");
    fs::write(&skill, [0xff, 0xfe, 0xfd]).expect("non UTF-8 workflow");
    binary()
        .args(["run", "init", "bad-workflow", "--project"])
        .arg(project.path())
        .arg("--skill")
        .arg(&skill)
        .args([
            "--objective",
            "bounded",
            "--provider",
            "command",
            "--provider-executable",
            python(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("runtime[invalid_input]"));
    assert!(!project
        .path()
        .join(".codeunlimited/runs/bad-workflow")
        .exists());

    let project = TempDir::new().expect("project");
    let skill = workflow(project.path());
    let oversized = "é".repeat(4097);
    binary()
        .args(["run", "init", "unicode", "--project"])
        .arg(project.path())
        .arg("--skill")
        .arg(skill)
        .arg("--objective")
        .arg(&oversized)
        .args(["--provider", "command", "--provider-executable", python()])
        .assert()
        .code(6)
        .stderr(predicate::str::contains("runtime[over_budget]"))
        .stderr(predicate::str::contains(&oversized).not());
}

#[test]
fn non_utf8_recovery_observation_is_rejected_without_clearing_recovery() {
    let project = TempDir::new().expect("project");
    let changed = project.path().join("changed.txt");
    init_command(
        project.path(),
        "recover-utf8",
        "invalid",
        &["--change".into(), changed.to_string_lossy().into_owned()],
    )
    .assert()
    .success();
    initialize_git(project.path());
    binary()
        .args(["run", "step", "recover-utf8", "--project"])
        .arg(project.path())
        .assert()
        .code(8);

    let observation = project.path().join("recovery.bin");
    fs::write(&observation, [0xff, 0xfe]).expect("non UTF-8 observation");
    binary()
        .args(["run", "recover", "recover-utf8", "--project"])
        .arg(project.path())
        .arg("--observation")
        .arg(&observation)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("runtime[invalid_input]"));
    assert!(project
        .path()
        .join(".codeunlimited/runs/recover-utf8/recovery.json")
        .exists());
}

#[cfg(unix)]
#[test]
fn post_provider_read_only_store_preserves_state_and_journals_the_attempt() {
    use std::os::unix::fs::PermissionsExt;

    let project = TempDir::new().expect("project");
    let capture = project.path().join("provider-ran");
    init_command(
        project.path(),
        "read-only",
        "success",
        &["--capture".into(), capture.to_string_lossy().into_owned()],
    )
    .assert()
    .success();
    initialize_git(project.path());
    let run = project.path().join(".codeunlimited/runs/read-only");
    let state = run.join("state.json");
    let before = fs::read(&state).expect("state before");
    fs::set_permissions(&run, fs::Permissions::from_mode(0o555)).expect("read-only run");

    binary()
        .args(["run", "step", "read-only", "--project"])
        .arg(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("runtime[invalid_input]"));

    fs::set_permissions(&run, fs::Permissions::from_mode(0o755)).expect("restore run");
    assert!(capture.exists(), "failure must occur after provider launch");
    assert_eq!(fs::read(state).expect("state after"), before);
    assert_eq!(
        fs::read_dir(run.join("attempts"))
            .expect("attempt journal")
            .count(),
        1,
        "every provider launch must be journaled"
    );
}
