use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{json, Value};
use tempfile::TempDir;

fn binary() -> Command {
    Command::cargo_bin("codeunlimited").expect("binary")
}

fn spawned_binary() -> ProcessCommand {
    ProcessCommand::new(env!("CARGO_BIN_EXE_codeunlimited"))
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

fn init_command(project: &Path, name: &str, mode: &str) -> Command {
    let mut command = binary();
    command
        .args(["run", "init", name, "--project"])
        .arg(project)
        .arg("--skill")
        .arg(workflow(project))
        .args([
            "--objective",
            "Exercise the durable attempt ledger",
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
    ] {
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

fn run_dir(project: &Path, name: &str) -> PathBuf {
    project.join(format!(".codeunlimited/runs/{name}"))
}

fn json_output(mut command: Command) -> Value {
    let output = command.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("JSON output")
}

fn ledger(project: &Path, name: &str) -> Value {
    let mut command = binary();
    command
        .args(["run", "ledger", name, "--project"])
        .arg(project)
        .arg("--json");
    json_output(command)
}

fn usage(
    semantics: &str,
    input: Option<u64>,
    read: Option<u64>,
    write: Option<u64>,
    output: Option<u64>,
) -> Value {
    json!({
        "input_token_semantics": semantics,
        "input_tokens": input,
        "uncached_input_tokens": Value::Null,
        "cache_read_input_tokens": read,
        "cache_write_input_tokens": write,
        "cache_write_5m_input_tokens": Value::Null,
        "cache_write_1h_input_tokens": Value::Null,
        "output_tokens": output,
    })
}

fn attempt(number: u64, outcome: &str, usage: Value) -> Value {
    json!({
        "schema_version": 1,
        "attempt": number,
        "base_revision": number - 1,
        "committed_revision": if outcome == "succeeded" { json!(number) } else { Value::Null },
        "outcome": outcome,
        "error_category": if outcome == "succeeded" { Value::Null } else { json!("fixture_failure") },
        "prompt_bytes": 200,
        "stable_prompt_bytes": 100,
        "dynamic_prompt_bytes": 100,
        "prompt_sha256": "11".repeat(32),
        "stable_prompt_sha256": "22".repeat(32),
        "workflow_sha256": "33".repeat(32),
        "provider": "command",
        "started_unix": 1_700_000_000 + number,
        "duration_ms": 10,
        "exit_code": if outcome == "succeeded" { 0 } else { 7 },
        "response_bytes": 64,
        "usage": usage,
        "state_before_sha256": "44".repeat(32),
        "state_after_sha256": if outcome == "succeeded" { json!("55".repeat(32)) } else { Value::Null },
        "before_git": {"available": true, "head": "abc", "status_sha256": "66".repeat(32)},
        "after_git": {"available": true, "head": "abc", "status_sha256": "66".repeat(32)},
        "accepted_task_ids": [],
        "configuration_sha256": "77".repeat(32),
    })
}

fn write_attempt(project: &Path, name: &str, number: u64, value: &Value) {
    let path = run_dir(project, name)
        .join("attempts")
        .join(format!("{number:08}.json"));
    fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("attempt JSON"),
    )
    .expect("attempt");
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn snapshot_regular_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, current: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).expect("read fixture directory") {
            let entry = entry.expect("directory entry");
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("metadata");
            if metadata.is_dir() {
                visit(root, &path, result);
            } else if metadata.is_file() {
                match fs::read(&path) {
                    Ok(bytes) => {
                        result.insert(path.strip_prefix(root).unwrap().to_path_buf(), bytes);
                    }
                    // The live worker holds an exclusive lock on the run lock
                    // file; Windows denies reads of exclusively locked regions
                    // (ERROR_LOCK_VIOLATION), while Unix flock stays advisory.
                    Err(error) if error.raw_os_error() == Some(33) => {}
                    Err(error) => panic!("read {}: {error}", path.display()),
                }
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn ledger_is_a_recognized_command_and_missing_run_is_a_runtime_error() {
    let project = TempDir::new().expect("project");
    binary()
        .args(["run", "ledger", "missing", "--project"])
        .arg(project.path())
        .arg("--json")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "runtime[invalid_input]: run does not exist",
        ))
        .stderr(predicate::str::contains("unrecognized subcommand").not());
}

#[test]
fn ledger_retains_successful_and_failed_attempt_usage_with_literal_totals() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "totals", "success")
        .assert()
        .success();
    write_attempt(
        project.path(),
        "totals",
        1,
        &attempt(
            1,
            "succeeded",
            usage(
                "total_includes_cache",
                Some(100),
                Some(60),
                Some(20),
                Some(10),
            ),
        ),
    );
    write_attempt(
        project.path(),
        "totals",
        2,
        &attempt(
            2,
            "provider_failed",
            usage("total_includes_cache", Some(20), Some(5), Some(5), Some(3)),
        ),
    );

    let report = ledger(project.path(), "totals");
    assert_eq!(report["coverage"]["attempt_count"], 2);
    assert_eq!(report["coverage"]["complete_attempts"], 2);
    assert_eq!(report["coverage"]["incomplete_attempts"], 0);
    assert_eq!(report["coverage"]["observed_input_tokens"], 120);
    assert_eq!(report["coverage"]["observed_output_tokens"], 13);
    assert_eq!(report["coverage"]["observed_total_tokens"], 133);
    assert_eq!(report["coverage"]["total_tokens"], 133);
    assert_eq!(report["attempts"].as_array().unwrap().len(), 2);
    assert_eq!(report["attempts"][1]["outcome"], "provider_failed");
}

#[test]
fn incomplete_usage_keeps_observed_subtotals_but_nulls_complete_total() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "partial", "success")
        .assert()
        .success();
    write_attempt(
        project.path(),
        "partial",
        1,
        &attempt(
            1,
            "succeeded",
            usage(
                "total_includes_cache",
                Some(100),
                Some(60),
                Some(20),
                Some(10),
            ),
        ),
    );
    write_attempt(
        project.path(),
        "partial",
        2,
        &attempt(
            2,
            "provider_failed",
            usage("unknown", None, None, None, None),
        ),
    );

    let report = ledger(project.path(), "partial");
    assert_eq!(report["coverage"]["complete_attempts"], 1);
    assert_eq!(report["coverage"]["incomplete_attempts"], 1);
    assert_eq!(report["coverage"]["observed_input_tokens"], 100);
    assert_eq!(report["coverage"]["observed_output_tokens"], 10);
    assert_eq!(report["coverage"]["observed_total_tokens"], 110);
    assert!(report["coverage"]["total_tokens"].is_null());
    assert_eq!(report["coverage"]["complete"], false);
}

#[test]
fn zero_attempts_are_explicitly_no_evidence() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "empty", "success")
        .assert()
        .success();

    let report = ledger(project.path(), "empty");
    assert_eq!(report["coverage"]["attempt_count"], 0);
    assert_eq!(report["coverage"]["zero_attempts"], true);
    assert_eq!(report["coverage"]["complete"], false);
    assert!(report["coverage"]["total_tokens"].is_null());
    assert_eq!(report["coverage"]["cache_probes_excluded"], true);
}

#[test]
fn each_attempt_normalizes_cache_counters_before_aggregation() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "cache", "success")
        .assert()
        .success();
    write_attempt(
        project.path(),
        "cache",
        1,
        &attempt(
            1,
            "succeeded",
            usage(
                "total_includes_cache",
                Some(100),
                Some(60),
                Some(20),
                Some(5),
            ),
        ),
    );
    let mut exclusive = attempt(
        2,
        "succeeded",
        usage("uncached_only", Some(20), Some(60), Some(20), Some(5)),
    );
    exclusive["usage"]["uncached_input_tokens"] = json!(20);
    write_attempt(project.path(), "cache", 2, &exclusive);

    let report = ledger(project.path(), "cache");
    assert_eq!(report["attempts"][0]["transported_input_tokens"], 100);
    assert_eq!(report["attempts"][1]["transported_input_tokens"], 100);
    assert_eq!(report["coverage"]["observed_input_tokens"], 200);
    assert_eq!(report["coverage"]["total_tokens"], 210);
}

#[test]
fn checked_aggregation_reports_overflow_without_saturating_a_complete_total() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "overflow", "success")
        .assert()
        .success();
    write_attempt(
        project.path(),
        "overflow",
        1,
        &attempt(
            1,
            "succeeded",
            usage("total_includes_cache", Some(u64::MAX), None, None, Some(0)),
        ),
    );
    write_attempt(
        project.path(),
        "overflow",
        2,
        &attempt(
            2,
            "succeeded",
            usage("total_includes_cache", Some(1), None, None, Some(0)),
        ),
    );

    let report = ledger(project.path(), "overflow");
    assert_eq!(report["coverage"]["overflowed"], true);
    assert!(report["coverage"]["observed_input_tokens"].is_null());
    assert!(report["coverage"]["observed_total_tokens"].is_null());
    assert!(report["coverage"]["total_tokens"].is_null());
    assert_eq!(report["coverage"]["complete"], false);
}

#[test]
fn uncached_normalization_overflow_is_reported_instead_of_looking_unavailable() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "normalize-overflow", "success")
        .assert()
        .success();
    let mut overflowing = attempt(
        1,
        "succeeded",
        usage("uncached_only", Some(u64::MAX), Some(1), Some(0), Some(0)),
    );
    overflowing["usage"]["uncached_input_tokens"] = json!(u64::MAX);
    write_attempt(project.path(), "normalize-overflow", 1, &overflowing);

    let report = ledger(project.path(), "normalize-overflow");
    assert_eq!(
        report["attempts"][0]["transported_input_tokens"],
        Value::Null
    );
    assert_eq!(report["coverage"]["overflowed"], true);
    assert_eq!(report["coverage"]["observed_input_tokens"], Value::Null);
    assert_eq!(report["coverage"]["observed_output_tokens"], 0);
    assert_eq!(report["coverage"]["total_tokens"], Value::Null);
}

#[test]
fn legacy_optional_fields_default_without_rewriting_during_inspection() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "legacy", "success")
        .assert()
        .success();
    let run = run_dir(project.path(), "legacy");
    let manifest_path = run.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest.as_object_mut().unwrap().remove("max_total_tokens");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let mut legacy_attempt = attempt(
        1,
        "succeeded",
        usage("total_includes_cache", Some(4), None, None, Some(1)),
    );
    legacy_attempt
        .as_object_mut()
        .unwrap()
        .remove("accepted_task_ids");
    legacy_attempt
        .as_object_mut()
        .unwrap()
        .remove("configuration_sha256");
    write_attempt(project.path(), "legacy", 1, &legacy_attempt);
    let before = snapshot_regular_files(&run);

    let report = ledger(project.path(), "legacy");
    assert!(report["max_total_tokens"].is_null());
    assert_eq!(report["accepted_task_count"], 0);
    assert!(report["tokens_per_accepted_task"].is_null());
    assert_eq!(snapshot_regular_files(&run), before);
}

#[test]
fn intent_is_visible_while_worker_runs_and_ledger_never_claims_complete() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "busy", "sleep")
        .arg("--provider-arg=--sleep")
        .arg("--provider-arg=1.0")
        .assert()
        .success();
    let run = run_dir(project.path(), "busy");
    let intent = run.join("attempt-intent.json");
    let mut step = spawned_binary();
    step.args(["run", "step", "busy", "--project"])
        .arg(project.path())
        .arg("--json")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = step.spawn().expect("step process");
    wait_for(&intent);

    let before = snapshot_regular_files(&run);
    let report = ledger(project.path(), "busy");
    assert_eq!(report["coverage"]["busy"], true);
    assert_eq!(report["coverage"]["pending"], true);
    assert_eq!(report["coverage"]["complete"], false);
    assert!(report["coverage"]["total_tokens"].is_null());
    assert_eq!(report["pending_attempt"]["attempt"], 1);
    let mut status = binary();
    status
        .args(["run", "status", "busy", "--project"])
        .arg(project.path())
        .arg("--json");
    let status = json_output(status);
    assert_eq!(status["busy"], true);
    assert_eq!(status["recovery_required"], true);
    assert_eq!(snapshot_regular_files(&run), before);

    assert!(child.wait().expect("step exit").success());
    assert!(!intent.exists());
    assert!(run.join("attempts/00000001.json").exists());
}

#[test]
fn interrupted_worker_requires_explicit_recovery_and_records_unknown_once() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "interrupted", "sleep")
        .arg("--provider-arg=--sleep")
        .arg("--provider-arg=0.5")
        .assert()
        .success();
    initialize_git(project.path());
    let run = run_dir(project.path(), "interrupted");
    let intent = run.join("attempt-intent.json");
    let mut step = spawned_binary();
    step.args(["run", "step", "interrupted", "--project"])
        .arg(project.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = step.spawn().expect("step process");
    wait_for(&intent);
    child.kill().expect("kill step");
    let _ = child.wait();
    // The runtime is not a process-tree supervisor. Let the bounded fixture
    // child exit before this test removes its temporary working directory.
    thread::sleep(Duration::from_millis(600));

    binary()
        .args(["run", "step", "interrupted", "--project"])
        .arg(project.path())
        .assert()
        .code(8)
        .stderr(predicate::str::contains("runtime[recovery_required]"));
    let intent_before = fs::read(&intent).expect("pending intent");
    let status_before = snapshot_regular_files(&run);
    let report = ledger(project.path(), "interrupted");
    assert_eq!(report["coverage"]["pending"], true);
    assert!(report["coverage"]["total_tokens"].is_null());
    assert_eq!(
        fs::read(&intent).unwrap(),
        intent_before,
        "ledger is read-only"
    );
    assert_eq!(
        snapshot_regular_files(&run),
        status_before,
        "ledger does not repair"
    );

    let observation = project.path().join("recovery.txt");
    fs::write(&observation, "Preserve any interrupted worker edits\n").unwrap();
    binary()
        .args(["run", "recover", "interrupted", "--project"])
        .arg(project.path())
        .arg("--observation")
        .arg(&observation)
        .assert()
        .success();

    let recovered = ledger(project.path(), "interrupted");
    assert_eq!(recovered["coverage"]["attempt_count"], 1);
    assert_eq!(recovered["coverage"]["incomplete_attempts"], 1);
    assert_eq!(recovered["attempts"][0]["error_category"], "interrupted");
    assert!(!intent.exists());
    binary()
        .args(["run", "recover", "interrupted", "--project"])
        .arg(project.path())
        .arg("--observation")
        .arg(&observation)
        .assert()
        .failure();
    assert_eq!(fs::read_dir(run.join("attempts")).unwrap().count(), 1);
}

#[test]
fn recovery_reconciles_an_already_finalized_intent_without_duplicate_charge() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "dedupe", "sleep")
        .arg("--provider-arg=--sleep")
        .arg("--provider-arg=0.2")
        .assert()
        .success();
    initialize_git(project.path());
    let run = run_dir(project.path(), "dedupe");
    let intent = run.join("attempt-intent.json");
    let mut step = spawned_binary();
    step.args(["run", "step", "dedupe", "--project"])
        .arg(project.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = step.spawn().expect("step process");
    wait_for(&intent);
    let finalized_intent = fs::read(&intent).unwrap();
    assert!(child.wait().expect("step exit").success());
    assert!(!intent.exists());
    fs::write(&intent, finalized_intent).unwrap();
    let observation = project.path().join("recovery.txt");
    fs::write(&observation, "Reconcile finalized intent\n").unwrap();

    binary()
        .args(["run", "recover", "dedupe", "--project"])
        .arg(project.path())
        .arg("--observation")
        .arg(&observation)
        .assert()
        .success();

    let report = ledger(project.path(), "dedupe");
    assert_eq!(report["coverage"]["attempt_count"], 1);
    assert_eq!(report["attempts"].as_array().unwrap().len(), 1);
    assert!(!intent.exists());
}

#[test]
fn provider_altering_the_live_intent_requires_recovery_and_is_not_silently_cleared() {
    let project = TempDir::new().expect("project");
    let intent = run_dir(project.path(), "tamper").join("attempt-intent.json");
    init_command(project.path(), "tamper", "success")
        .arg("--provider-arg=--change")
        .arg(format!("--provider-arg={}", intent.display()))
        .assert()
        .success();
    initialize_git(project.path());

    binary()
        .args(["run", "step", "tamper", "--project"])
        .arg(project.path())
        .assert()
        .code(8)
        .stderr(predicate::str::contains("runtime[recovery_required]"));

    assert_eq!(fs::read(&intent).unwrap(), b"changed\n");
    assert!(run_dir(project.path(), "tamper")
        .join("recovery.json")
        .exists());
    assert_eq!(
        fs::read_dir(run_dir(project.path(), "tamper").join("attempts"))
            .unwrap()
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn failed_attempt_record_write_leaves_the_intent_for_explicit_recovery() {
    use std::os::unix::fs::PermissionsExt;

    let project = TempDir::new().expect("project");
    init_command(project.path(), "write-failure", "success")
        .assert()
        .success();
    initialize_git(project.path());
    let run = run_dir(project.path(), "write-failure");
    let attempts = run.join("attempts");
    fs::set_permissions(&attempts, fs::Permissions::from_mode(0o555)).unwrap();

    binary()
        .args(["run", "step", "write-failure", "--project"])
        .arg(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("runtime[invalid_input]"));

    fs::set_permissions(&attempts, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(run.join("attempt-intent.json").exists());
    assert_eq!(fs::read_dir(&attempts).unwrap().count(), 0);
    binary()
        .args(["run", "step", "write-failure", "--project"])
        .arg(project.path())
        .assert()
        .code(8);
}

#[test]
fn configured_cap_allows_first_attempt_then_refuses_unknown_usage() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "capped", "success")
        .args(["--max-total-tokens", "100"])
        .assert()
        .success();
    binary()
        .args(["run", "step", "capped", "--project"])
        .arg(project.path())
        .assert()
        .success();

    binary()
        .args(["run", "step", "capped", "--project"])
        .arg(project.path())
        .assert()
        .code(6)
        .stderr(predicate::str::contains("runtime[over_budget]"));
    assert_eq!(
        ledger(project.path(), "capped")["coverage"]["attempt_count"],
        1
    );
}

#[test]
fn configured_cap_refuses_at_boundary_and_reports_overshoot() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "boundary", "success")
        .args(["--max-total-tokens", "100"])
        .assert()
        .success();
    write_attempt(
        project.path(),
        "boundary",
        1,
        &attempt(
            1,
            "succeeded",
            usage("total_includes_cache", Some(105), None, None, Some(5)),
        ),
    );

    let report = ledger(project.path(), "boundary");
    assert_eq!(report["max_total_tokens"], 100);
    assert_eq!(report["cap_reached"], true);
    assert_eq!(report["cap_overshoot_tokens"], 10);
    binary()
        .args(["run", "step", "boundary", "--project"])
        .arg(project.path())
        .assert()
        .code(6)
        .stderr(predicate::str::contains("runtime[over_budget]"));
    assert_eq!(
        fs::read_dir(run_dir(project.path(), "boundary").join("attempts"))
            .unwrap()
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn dangling_symlink_intent_is_rejected_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let project = TempDir::new().expect("project");
    init_command(project.path(), "unsafe", "success")
        .assert()
        .success();
    let run = run_dir(project.path(), "unsafe");
    let missing = project.path().join("missing-target");
    symlink(&missing, run.join("attempt-intent.json")).unwrap();

    let mut command = binary();
    command
        .args(["run", "ledger", "unsafe", "--project"])
        .arg(project.path())
        .arg("--json")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "runtime[invalid_input]: runtime store contains an unsafe path",
        ));
    assert!(!missing.exists());
}

#[test]
fn malformed_intent_metadata_is_rejected_read_only() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "malformed-intent", "success")
        .assert()
        .success();
    let intent = run_dir(project.path(), "malformed-intent").join("attempt-intent.json");
    let bytes = serde_json::to_vec_pretty(&json!({
        "schema_version": 2,
        "attempt": 1,
        "base_revision": 0,
        "provider": "command",
        "configuration_sha256": "00".repeat(32),
        "prompt_sha256": "11".repeat(32),
        "stable_prompt_sha256": "22".repeat(32),
        "workflow_sha256": "33".repeat(32),
        "prompt_bytes": 10,
        "stable_prompt_bytes": 5,
        "dynamic_prompt_bytes": 5,
        "state_before_sha256": "44".repeat(32),
        "started_unix": 1,
        "before_git": {"available": false, "head": null, "status_sha256": null}
    }))
    .unwrap();
    fs::write(&intent, &bytes).unwrap();

    let mut command = binary();
    command
        .args(["run", "ledger", "malformed-intent", "--project"])
        .arg(project.path())
        .arg("--json")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "invalid stored runtime data: attempt-intent.json",
        ));
    assert_eq!(fs::read(intent).unwrap(), bytes);
}

#[test]
fn recovery_does_not_reconcile_an_attempt_record_with_different_intent_metadata() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "mismatch", "sleep")
        .arg("--provider-arg=--sleep")
        .arg("--provider-arg=0.2")
        .assert()
        .success();
    initialize_git(project.path());
    let run = run_dir(project.path(), "mismatch");
    let intent = run.join("attempt-intent.json");
    let mut step = spawned_binary();
    step.args(["run", "step", "mismatch", "--project"])
        .arg(project.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = step.spawn().expect("step process");
    wait_for(&intent);
    let mut stale: Value = serde_json::from_slice(&fs::read(&intent).unwrap()).unwrap();
    assert!(child.wait().expect("step exit").success());
    stale["base_revision"] = json!(9);
    fs::write(&intent, serde_json::to_vec_pretty(&stale).unwrap()).unwrap();
    let observation = project.path().join("recovery.txt");
    fs::write(&observation, "Do not reconcile mismatched metadata\n").unwrap();

    binary()
        .args(["run", "recover", "mismatch", "--project"])
        .arg(project.path())
        .arg("--observation")
        .arg(&observation)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "invalid stored runtime data: attempt-intent.json",
        ));
    assert!(intent.exists());
    assert_eq!(fs::read_dir(run.join("attempts")).unwrap().count(), 1);
}

#[test]
fn recovery_rejects_a_valid_but_reidentified_intent_without_double_charging() {
    let project = TempDir::new().expect("project");
    let run = run_dir(project.path(), "reidentified");
    let intent = run.join("attempt-intent.json");
    init_command(project.path(), "reidentified", "success")
        .arg("--provider-arg=--mutate-intent-attempt")
        .arg(format!("--provider-arg={}", intent.display()))
        .assert()
        .success();
    initialize_git(project.path());

    binary()
        .args(["run", "step", "reidentified", "--project"])
        .arg(project.path())
        .assert()
        .code(8)
        .stderr(predicate::str::contains("runtime[recovery_required]"));
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&intent).unwrap()).unwrap()["attempt"],
        2
    );
    assert!(run.join("attempts/00000001.json").exists());
    assert!(run.join("recovery.json").exists());
    let state_before = fs::read(run.join("state.json")).unwrap();
    let observation = project.path().join("recovery.txt");
    fs::write(&observation, "Reject contradictory intent identity\n").unwrap();

    binary()
        .args(["run", "recover", "reidentified", "--project"])
        .arg(project.path())
        .arg("--observation")
        .arg(&observation)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "invalid stored runtime data: attempt-intent.json",
        ));

    assert_eq!(fs::read(run.join("state.json")).unwrap(), state_before);
    assert_eq!(fs::read_dir(run.join("attempts")).unwrap().count(), 1);
    assert!(run.join("attempts/00000001.json").exists());
    assert!(!run.join("attempts/00000002.json").exists());
    assert!(intent.exists());
    assert!(run.join("recovery.json").exists());
}
