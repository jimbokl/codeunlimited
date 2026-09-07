use std::collections::BTreeMap;
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

fn start(project: &Path, name: &str, mode: &str) -> Command {
    start_with_verification(project, name, mode, true)
}

fn start_with_verification(project: &Path, name: &str, mode: &str, passes: bool) -> Command {
    start_with_verification_code(
        project,
        name,
        mode,
        &format!("raise SystemExit({})", if passes { 0 } else { 1 }),
    )
}

fn start_with_verification_code(
    project: &Path,
    name: &str,
    mode: &str,
    verification_code: &str,
) -> Command {
    let mut command = binary();
    command
        .args(["run", "start", name, "--project"])
        .arg(project)
        .args([
            "--objective",
            "Complete the bounded fixture objective",
            "--verify-program",
            python(),
            "--verify-arg=-c",
            "--provider",
            "codex",
            "--provider-executable",
        ])
        .arg(fixture())
        .arg(format!("--verify-arg={verification_code}"))
        .arg("--provider-arg=--fixture-mode")
        .arg(format!("--provider-arg={mode}"));
    command
}

#[test]
fn start_help_lists_only_supported_subscription_providers() {
    binary()
        .args(["run", "start", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("possible values: claude, codex"))
        .stdout(predicate::str::contains("command").not())
        .stdout(predicate::str::contains("openai-api").not())
        .stdout(predicate::str::contains("anthropic-api").not());
}

fn json_output(mut command: Command) -> Value {
    let output = command.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("JSON output")
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, current: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).expect("read directory") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                visit(root, &path, files);
            } else if path.is_file() {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn init_git(project: &Path) {
    assert!(ProcessCommand::new("git")
        .args(["init", "-q"])
        .current_dir(project)
        .status()
        .unwrap()
        .success());
    fs::write(project.join("tracked.txt"), "fixture\n").unwrap();
    assert!(ProcessCommand::new("git")
        .args(["add", "tracked.txt"])
        .current_dir(project)
        .status()
        .unwrap()
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
        .unwrap()
        .success());
}

#[test]
fn start_rejects_invalid_inputs_before_creating_or_invoking() {
    for args in [
        vec!["--objective", "fixture"],
        vec![
            "--objective",
            "fixture",
            "--verify-program",
            "cargo",
            "--provider",
            "openai-api",
        ],
        vec![
            "--objective",
            "fixture",
            "--verify-program",
            "cargo",
            "--max-steps",
            "0",
        ],
        vec![
            "--objective",
            "fixture",
            "--verify-program",
            "cargo",
            "--max-total-tokens",
            "0",
        ],
        vec![
            "--objective",
            "fixture",
            "--verify-program",
            "cargo",
            "--provider-timeout-seconds",
            "0",
        ],
    ] {
        let project = TempDir::new().unwrap();
        binary()
            .args(["run", "start", "invalid", "--project"])
            .arg(project.path())
            .args(args)
            .assert()
            .failure();
        assert!(!project.path().join(".codeunlimited").exists());
    }
}

#[test]
fn start_completes_with_real_verification_ledger_and_private_checkpoint() {
    let project = TempDir::new().unwrap();
    init_git(project.path());
    let capture = project
        .path()
        .join(".codeunlimited/runs/complete/capture.jsonl");
    let mut command = start(project.path(), "complete", "complete");
    command
        .arg("--json")
        .arg("--provider-arg=--fixture-capture")
        .arg(format!("--provider-arg={}", capture.display()));
    let report = json_output(command);
    assert_eq!(report["run_name"], "complete");
    assert_eq!(report["steps"].as_array().unwrap().len(), 1);
    assert_eq!(report["steps"][0]["status"], "complete");
    assert_eq!(report["steps"][0]["verification_passed"], true);

    let captured: Value = serde_json::from_str(
        fs::read_to_string(&capture)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(captured["worker"], "1");
    assert!(captured["prompt"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase()
        .contains("never launch another codeunlimited run"));

    let run = project.path().join(".codeunlimited/runs/complete");
    let state: Value = serde_json::from_slice(&fs::read(run.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["status"], "complete");
    let mut ledger = binary();
    ledger
        .args(["run", "ledger", "complete", "--project"])
        .arg(project.path())
        .arg("--json");
    let ledger = json_output(ledger);
    assert_eq!(ledger["coverage"]["attempt_count"], 1);
    assert_eq!(ledger["coverage"]["total_tokens"], 127);
    assert_eq!(ledger["coverage"]["complete"], true);
    assert_eq!(fs::read_to_string(run.join(".gitignore")).unwrap(), "*\n");
    assert!(ProcessCommand::new("git")
        .args([
            "check-ignore",
            "-q",
            ".codeunlimited/runs/complete/state.json"
        ])
        .current_dir(project.path())
        .status()
        .unwrap()
        .success());
    let status = ProcessCommand::new("git")
        .args(["status", "--porcelain", "--", ".codeunlimited"])
        .current_dir(project.path())
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(status.stdout.is_empty());
}

#[test]
fn provider_receives_worker_marker_but_real_verifier_does_not() {
    let project = TempDir::new().unwrap();
    let provider_capture = project
        .path()
        .join(".codeunlimited/runs/marker/capture.jsonl");
    let verifier_capture = project
        .path()
        .join(".codeunlimited/runs/marker/verifier-marker.txt");
    let verification_code = format!(
        "import os,pathlib; pathlib.Path({:?}).write_text(os.environ.get('CODEUNLIMITED_RUNTIME_WORKER', 'absent'))",
        verifier_capture.to_string_lossy()
    );
    let mut command =
        start_with_verification_code(project.path(), "marker", "complete", &verification_code);
    command
        .arg("--provider-arg=--fixture-capture")
        .arg(format!("--provider-arg={}", provider_capture.display()))
        .assert()
        .success();

    let provider: Value = serde_json::from_str(
        fs::read_to_string(provider_capture)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(provider["worker"], "1");
    assert_eq!(fs::read_to_string(verifier_capture).unwrap(), "absent");
}

#[test]
fn duplicate_start_preserves_the_existing_run_without_worker_call() {
    let project = TempDir::new().unwrap();
    let capture = project
        .path()
        .join(".codeunlimited/runs/duplicate/capture.jsonl");
    let mut first = start(project.path(), "duplicate", "complete");
    first
        .arg("--provider-arg=--fixture-capture")
        .arg(format!("--provider-arg={}", capture.display()));
    first.assert().success();
    let run = project.path().join(".codeunlimited/runs/duplicate");
    let before = snapshot(&run);
    let calls = fs::read_to_string(&capture).unwrap();

    let mut second = start(project.path(), "duplicate", "complete");
    second
        .arg("--provider-arg=--fixture-capture")
        .arg(format!("--provider-arg={}", capture.display()))
        .assert()
        .failure();
    assert_eq!(snapshot(&run), before);
    assert_eq!(fs::read_to_string(capture).unwrap(), calls);
}

#[test]
fn start_terminates_on_blocked_failed_verification_and_soft_budget() {
    let blocked = TempDir::new().unwrap();
    start(blocked.path(), "blocked", "blocked")
        .arg("--json")
        .assert()
        .code(10)
        .stdout(predicate::str::contains("\"status\": \"blocked\""));

    let failed_check = TempDir::new().unwrap();
    let mut command =
        start_with_verification(failed_check.path(), "failed-check", "complete", false);
    command.args(["--max-steps", "2"]).assert().code(10);
    let attempts = fs::read_dir(
        failed_check
            .path()
            .join(".codeunlimited/runs/failed-check/attempts"),
    )
    .unwrap()
    .count();
    assert_eq!(attempts, 2);

    let budget = TempDir::new().unwrap();
    start(budget.path(), "budget", "continue")
        .args(["--max-total-tokens", "100"])
        .arg("--json")
        .assert()
        .code(6);
    let mut ledger = binary();
    ledger
        .args(["run", "ledger", "budget", "--project"])
        .arg(budget.path())
        .arg("--json");
    let ledger = json_output(ledger);
    assert_eq!(ledger["coverage"]["attempt_count"], 1);
    assert_eq!(ledger["cap_reached"], true);
    assert_eq!(ledger["cap_overshoot_tokens"], 27);
}

#[test]
fn start_handles_provider_failure_unknown_usage_and_ambiguous_output_finitely() {
    let failed = TempDir::new().unwrap();
    init_git(failed.path());
    start(failed.path(), "failed", "failure").assert().code(5);
    assert_eq!(
        fs::read_dir(failed.path().join(".codeunlimited/runs/failed/attempts"))
            .unwrap()
            .count(),
        1
    );

    let unknown = TempDir::new().unwrap();
    start(unknown.path(), "unknown", "unknown-usage")
        .assert()
        .code(6);
    let mut ledger = binary();
    ledger
        .args(["run", "ledger", "unknown", "--project"])
        .arg(unknown.path())
        .arg("--json");
    let ledger = json_output(ledger);
    assert_eq!(ledger["coverage"]["attempt_count"], 1);
    assert!(ledger["coverage"]["total_tokens"].is_null());

    let ambiguous = TempDir::new().unwrap();
    init_git(ambiguous.path());
    let changed = ambiguous.path().join("worker-change.txt");
    let mut command = start(ambiguous.path(), "ambiguous", "malformed");
    command
        .arg("--provider-arg=--fixture-change")
        .arg(format!("--provider-arg={}", changed.display()))
        .assert()
        .code(8);
    assert!(ambiguous
        .path()
        .join(".codeunlimited/runs/ambiguous/recovery.json")
        .is_file());
}

#[test]
fn marked_workers_cannot_dispatch_but_read_only_inspection_remains_available() {
    let project = TempDir::new().unwrap();
    start(project.path(), "existing", "complete")
        .assert()
        .success();
    let run = project.path().join(".codeunlimited/runs/existing");
    let before = snapshot(&run);

    start(project.path(), "nested", "complete")
        .env("CODEUNLIMITED_RUNTIME_WORKER", "1")
        .assert()
        .failure();
    assert!(!project.path().join(".codeunlimited/runs/nested").exists());
    for action in ["step", "auto", "cache-probe"] {
        let mut command = binary();
        command
            .args(["run", action, "existing", "--project"])
            .arg(project.path())
            .env("CODEUNLIMITED_RUNTIME_WORKER", "1");
        if action == "auto" {
            command.args(["--steps", "1"]);
        }
        command.assert().failure();
    }
    assert_eq!(snapshot(&run), before);
    for action in ["status", "ledger"] {
        binary()
            .args(["run", action, "existing", "--project"])
            .arg(project.path())
            .arg("--json")
            .env("CODEUNLIMITED_RUNTIME_WORKER", "1")
            .assert()
            .success();
    }
}
