use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use serde_json::{json, Value};
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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packet_driver.py")
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn parser_plan(cap: u64) -> Value {
    json!({
        "schema_version": 1,
        "max_packet_tasks": cap,
        "tasks": [
            {"id":"a","task":"Update parser fixture","group":"parser","depends_on":[],"scope":["tests/parser.rs"],"risk":"low"},
            {"id":"b","task":"Update parser error case","group":"parser","depends_on":["a"],"scope":["tests/parser.rs"],"risk":"low"}
        ]
    })
}

fn four_plan(cap: u64) -> Value {
    let scope = json!(["units/a.txt", "units/b.txt", "units/c.txt", "units/d.txt"]);
    json!({
        "schema_version": 1,
        "max_packet_tasks": cap,
        "tasks": [
            {"id":"unit-a","task":"Write alpha to units/a.txt","group":"units","depends_on":[],"scope":scope,"risk":"low"},
            {"id":"unit-b","task":"Write bravo to units/b.txt","group":"units","depends_on":["unit-a"],"scope":scope,"risk":"low"},
            {"id":"unit-c","task":"Write charlie to units/c.txt","group":"units","depends_on":["unit-b"],"scope":scope,"risk":"low"},
            {"id":"unit-d","task":"Write delta to units/d.txt","group":"units","depends_on":["unit-c"],"scope":scope,"risk":"low"}
        ]
    })
}

fn checks(count: u64) -> Value {
    Value::Array(
        (1..=count)
            .map(|revision| {
                json!({
                    "revision":revision,
                    "program":"verify",
                    "args":[],
                    "passed":true,
                    "summary":"passed",
                    "workspace_sha256":null
                })
            })
            .collect(),
    )
}

fn initialize_git(project: &Path) {
    assert!(ProcessCommand::new("git")
        .args(["init", "-q"])
        .current_dir(project)
        .status()
        .unwrap()
        .success());
    assert!(ProcessCommand::new("git")
        .args(["add", "."])
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
            "fixture"
        ])
        .current_dir(project)
        .status()
        .unwrap()
        .success());
}

fn init(project: &Path, name: &str, plan: &Value, mode: &str) -> Command {
    let workflow = project.join("workflow.md");
    let plan_path = project.join(format!("{name}-plan.json"));
    fs::write(&workflow, "# Workflow\nExecute the selected work packet.\n").unwrap();
    write_json(&plan_path, plan);
    let mut command = binary();
    command
        .args(["run", "init", name, "--project"])
        .arg(project)
        .arg("--skill")
        .arg(workflow)
        .args([
            "--objective",
            "Complete the immutable work plan",
            "--provider",
            "command",
            "--provider-executable",
            python(),
        ])
        .arg(format!("--provider-arg={}", fixture().display()))
        .arg("--provider-arg=--mode")
        .arg(format!("--provider-arg={mode}"))
        .arg("--work-plan")
        .arg(plan_path)
        .args(["--verify-program", python()])
        .arg(format!("--verify-arg={}", fixture().display()))
        .arg("--verify-arg=--verify");
    command
}

fn json_output(mut command: Command) -> Value {
    let output = command.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("JSON output")
}

fn packet(project: &Path, name: &str) -> Value {
    let mut command = binary();
    command
        .args(["run", "packet", name, "--project"])
        .arg(project)
        .arg("--json");
    json_output(command)
}

fn step(project: &Path, name: &str) -> std::process::Output {
    binary()
        .args(["run", "step", name, "--project"])
        .arg(project)
        .arg("--json")
        .output()
        .unwrap()
}

#[test]
fn packet_preview_selects_ready_dependencies_in_declaration_order_without_calling_worker() {
    for (cap, expected) in [(4, vec!["a", "b"]), (1, vec!["a"])] {
        let project = TempDir::new().unwrap();
        let capture = project.path().join("worker-called");
        let mut command = init(project.path(), "parser", &parser_plan(cap), "full");
        command.arg(format!("--provider-arg=--capture={}", capture.display()));
        command.assert().success();

        let preview = packet(project.path(), "parser");
        assert_eq!(preview["selected_task_ids"], json!(expected));
        assert_eq!(preview["remaining_count"], 2);
        assert!(preview["reason"].as_str().unwrap().contains("ready"));
        assert!(!capture.exists(), "preview must not invoke a worker");
    }
}

#[test]
fn packet_metadata_must_match_as_sets_not_string_similarity() {
    let cases = [
        ("group", json!({"group":"other"})),
        ("risk", json!({"risk":"medium"})),
        ("scope", json!({"scope":["tests/other.rs"]})),
        (
            "scope-order",
            json!({"scope":["tests/two.rs", "tests/one.rs"]}),
        ),
    ];
    for (name, replacement) in cases {
        let project = TempDir::new().unwrap();
        let mut plan = json!({"schema_version":1,"max_packet_tasks":4,"tasks":[
            {"id":"a","task":"First","group":"same","depends_on":[],"scope":["tests/one.rs","tests/two.rs"],"risk":"low"},
            {"id":"b","task":"Second","group":"same","depends_on":[],"scope":["tests/one.rs","tests/two.rs"],"risk":"low"}
        ]});
        for (key, value) in replacement.as_object().unwrap() {
            plan["tasks"][1][key] = value.clone();
        }
        init(project.path(), name, &plan, "full").assert().success();
        let selected = packet(project.path(), name)["selected_task_ids"].clone();
        if name == "scope-order" {
            assert_eq!(
                selected,
                json!(["a", "b"]),
                "scope ordering is not semantic"
            );
        } else {
            assert_eq!(
                selected,
                json!(["a"]),
                "different {name} must split packets"
            );
        }
    }
}

#[test]
fn invalid_plans_are_rejected_before_a_run_directory_exists() {
    let cases = [
        (
            "unknown-dependency",
            json!({"schema_version":1,"max_packet_tasks":4,"tasks":[{"id":"a","task":"A","group":"g","depends_on":["missing"],"scope":["a.txt"],"risk":"low"}]}),
        ),
        (
            "cycle",
            json!({"schema_version":1,"max_packet_tasks":4,"tasks":[{"id":"a","task":"A","group":"g","depends_on":["b"],"scope":["a.txt"],"risk":"low"},{"id":"b","task":"B","group":"g","depends_on":["a"],"scope":["a.txt"],"risk":"low"}]}),
        ),
        (
            "unknown-field",
            json!({"schema_version":1,"max_packet_tasks":4,"tasks":[{"id":"a","task":"A","group":"g","depends_on":[],"scope":["a.txt"],"risk":"low","extra":true}]}),
        ),
        (
            "unsafe-path",
            json!({"schema_version":1,"max_packet_tasks":4,"tasks":[{"id":"a","task":"A","group":"g","depends_on":[],"scope":["../a.txt"],"risk":"low"}]}),
        ),
    ];
    for (name, plan) in cases {
        let project = TempDir::new().unwrap();
        init(project.path(), name, &plan, "full").assert().failure();
        assert!(!project
            .path()
            .join(format!(".codeunlimited/runs/{name}"))
            .exists());
    }
}

#[test]
fn plan_limits_and_duplicate_contract_fields_are_rejected() {
    let too_many_tasks: Vec<_> = (0..33).map(|index| json!({
        "id":format!("t{index}"),"task":"A","group":"g","depends_on":[],"scope":["a.txt"],"risk":"low"
    })).collect();
    let cases = [
        (
            "zero-cap",
            json!({"schema_version":1,"max_packet_tasks":0,"tasks":[{"id":"a","task":"A","group":"g","depends_on":[],"scope":["a.txt"],"risk":"low"}]}),
        ),
        (
            "large-cap",
            json!({"schema_version":1,"max_packet_tasks":9,"tasks":[{"id":"a","task":"A","group":"g","depends_on":[],"scope":["a.txt"],"risk":"low"}]}),
        ),
        (
            "too-many",
            json!({"schema_version":1,"max_packet_tasks":1,"tasks":too_many_tasks}),
        ),
        (
            "duplicate-id",
            json!({"schema_version":1,"max_packet_tasks":1,"tasks":[{"id":"a","task":"A","group":"g","depends_on":[],"scope":["a.txt"],"risk":"low"},{"id":"a","task":"B","group":"g","depends_on":[],"scope":["b.txt"],"risk":"low"}]}),
        ),
        (
            "duplicate-dependency",
            json!({"schema_version":1,"max_packet_tasks":1,"tasks":[{"id":"a","task":"A","group":"g","depends_on":[],"scope":["a.txt"],"risk":"low"},{"id":"b","task":"B","group":"g","depends_on":["a","a"],"scope":["b.txt"],"risk":"low"}]}),
        ),
        (
            "duplicate-path",
            json!({"schema_version":1,"max_packet_tasks":1,"tasks":[{"id":"a","task":"A","group":"g","depends_on":[],"scope":["a.txt","a.txt"],"risk":"low"}]}),
        ),
        (
            "empty-scope",
            json!({"schema_version":1,"max_packet_tasks":1,"tasks":[{"id":"a","task":"A","group":"g","depends_on":[],"scope":[],"risk":"low"}]}),
        ),
        (
            "empty-group",
            json!({"schema_version":1,"max_packet_tasks":1,"tasks":[{"id":"a","task":"A","group":"","depends_on":[],"scope":["a.txt"],"risk":"low"}]}),
        ),
        (
            "utf8-task-bytes",
            json!({"schema_version":1,"max_packet_tasks":1,"tasks":[{"id":"a","task":"é".repeat(257),"group":"g","depends_on":[],"scope":["a.txt"],"risk":"low"}]}),
        ),
    ];
    for (name, plan) in cases {
        let project = TempDir::new().unwrap();
        init(project.path(), name, &plan, "full").assert().failure();
        assert!(!project
            .path()
            .join(format!(".codeunlimited/runs/{name}"))
            .exists());
    }
}

#[test]
fn work_plan_file_is_bounded_before_json_allocation() {
    let project = TempDir::new().unwrap();
    let plan_path = project.path().join("oversized-plan.json");
    fs::write(&plan_path, vec![b' '; 32 * 1024 + 1]).unwrap();
    let workflow = project.path().join("workflow.md");
    fs::write(&workflow, "# Workflow\n").unwrap();
    binary()
        .args(["run", "init", "oversized", "--project"])
        .arg(project.path())
        .arg("--skill")
        .arg(workflow)
        .args([
            "--objective",
            "X",
            "--provider",
            "command",
            "--provider-executable",
            python(),
            "--work-plan",
        ])
        .arg(plan_path)
        .args(["--verify-program", python()])
        .assert()
        .failure();
    assert!(!project
        .path()
        .join(".codeunlimited/runs/oversized")
        .exists());
}

#[test]
fn initialized_plan_is_an_immutable_snapshot_and_seeds_literal_queue() {
    let project = TempDir::new().unwrap();
    init(project.path(), "snapshot", &parser_plan(4), "full")
        .assert()
        .success();
    write_json(&project.path().join("snapshot-plan.json"), &four_plan(1));

    assert_eq!(
        packet(project.path(), "snapshot")["selected_task_ids"],
        json!(["a", "b"])
    );
    let state: Value = serde_json::from_slice(
        &fs::read(
            project
                .path()
                .join(".codeunlimited/runs/snapshot/state.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        state["queue"],
        json!([
            {"id":"a","task":"Update parser fixture"},
            {"id":"b","task":"Update parser error case"}
        ])
    );
}

#[test]
fn managed_run_requires_frozen_verification_and_never_allows_unverified_completion() {
    let project = TempDir::new().unwrap();
    let plan_path = project.path().join("plan.json");
    let workflow = project.path().join("workflow.md");
    write_json(&plan_path, &parser_plan(4));
    fs::write(&workflow, "# Workflow\n").unwrap();
    binary()
        .args(["run", "init", "unverified", "--project"])
        .arg(project.path())
        .arg("--skill")
        .arg(workflow)
        .args([
            "--objective",
            "X",
            "--provider",
            "command",
            "--provider-executable",
            python(),
            "--work-plan",
        ])
        .arg(plan_path)
        .arg("--allow-unverified-completion")
        .assert()
        .failure();
    assert!(!project
        .path()
        .join(".codeunlimited/runs/unverified")
        .exists());
}

#[test]
fn one_worker_accepts_four_verified_units_and_final_files_are_independently_checked() {
    let project = TempDir::new().unwrap();
    init(project.path(), "four", &four_plan(4), "full")
        .assert()
        .success();
    initialize_git(project.path());

    let output = step(project.path(), "four");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["attempt"], 1);
    assert_eq!(report["status"], "complete");
    for (path, expected) in [
        ("a.txt", "alpha\n"),
        ("b.txt", "bravo\n"),
        ("c.txt", "charlie\n"),
        ("d.txt", "delta\n"),
    ] {
        assert_eq!(
            fs::read_to_string(project.path().join("units").join(path)).unwrap(),
            expected
        );
    }
    let mut ledger = binary();
    ledger
        .args(["run", "ledger", "four", "--project"])
        .arg(project.path())
        .arg("--json");
    let ledger = json_output(ledger);
    assert_eq!(ledger["accepted_task_count"], 4);
    assert_eq!(
        ledger["attempts"][0]["accepted_task_ids"],
        json!(["unit-a", "unit-b", "unit-c", "unit-d"])
    );
}

#[test]
fn partial_prefix_acceptance_derives_remaining_queue_without_rewriting_tasks() {
    let project = TempDir::new().unwrap();
    init(project.path(), "partial", &four_plan(4), "partial")
        .assert()
        .success();
    initialize_git(project.path());
    let output = step(project.path(), "partial");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        packet(project.path(), "partial")["selected_task_ids"],
        json!(["unit-c", "unit-d"])
    );
    let state: Value = serde_json::from_slice(
        &fs::read(
            project
                .path()
                .join(".codeunlimited/runs/partial/state.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        state["queue"],
        json!([
            {"id":"unit-c","task":"Write charlie to units/c.txt"},
            {"id":"unit-d","task":"Write delta to units/d.txt"}
        ])
    );
}

#[test]
fn skipped_rewritten_and_premature_completion_responses_are_rejected() {
    for mode in ["invalid", "rewrite-task", "premature-complete"] {
        let project = TempDir::new().unwrap();
        init(project.path(), mode, &four_plan(4), mode)
            .assert()
            .success();
        initialize_git(project.path());
        let output = step(project.path(), mode);
        assert_eq!(
            output.status.code(),
            Some(7),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let state: Value = serde_json::from_slice(
            &fs::read(
                project
                    .path()
                    .join(format!(".codeunlimited/runs/{mode}/state.json")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(state["revision"], 0);
        assert_eq!(state["completed"], json!([]));
    }
}

#[test]
fn final_noncomplete_outcomes_are_rejected_before_verification_or_commit() {
    for mode in ["final-continue", "final-blocked"] {
        let project = TempDir::new().unwrap();
        let capture = project.path().join("verification-called");
        let mut command = init(project.path(), mode, &four_plan(4), mode);
        command
            .arg("--verify-arg=--capture")
            .arg(format!("--verify-arg={}", capture.display()));
        command.assert().success();
        initialize_git(project.path());

        let output = step(project.path(), mode);
        assert_eq!(output.status.code(), Some(7));
        assert!(!capture.exists(), "{mode} must fail before verification");
        let state: Value = serde_json::from_slice(
            &fs::read(
                project
                    .path()
                    .join(format!(".codeunlimited/runs/{mode}/state.json")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(state["revision"], 0);
        assert_eq!(state["queue"].as_array().unwrap().len(), 4);
    }
}

#[test]
fn blocked_outcome_can_accept_a_verified_partial_prefix() {
    let project = TempDir::new().unwrap();
    init(
        project.path(),
        "blocked-prefix",
        &four_plan(4),
        "blocked-partial",
    )
    .assert()
    .success();
    initialize_git(project.path());

    let output = step(project.path(), "blocked-prefix");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "blocked");
    let state: Value = serde_json::from_slice(
        &fs::read(
            project
                .path()
                .join(".codeunlimited/runs/blocked-prefix/state.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        state["completed"],
        json!([
            {"id":"unit-a","result":"fixture accepted"},
            {"id":"unit-b","result":"fixture accepted"}
        ])
    );
    assert_eq!(
        state["queue"],
        json!([
            {"id":"unit-c","task":"Write charlie to units/c.txt"},
            {"id":"unit-d","task":"Write delta to units/d.txt"}
        ])
    );
}

#[test]
fn managed_runs_retain_32_checks_without_relaxing_legacy_limit() {
    let legacy = TempDir::new().unwrap();
    let workflow = legacy.path().join("workflow.md");
    fs::write(&workflow, "# Workflow\n").unwrap();
    binary()
        .args(["run", "init", "legacy-checks", "--project"])
        .arg(legacy.path())
        .arg("--skill")
        .arg(workflow)
        .args([
            "--objective",
            "X",
            "--provider",
            "command",
            "--provider-executable",
            python(),
            "--allow-unverified-completion",
        ])
        .assert()
        .success();
    let legacy_state = legacy
        .path()
        .join(".codeunlimited/runs/legacy-checks/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&legacy_state).unwrap()).unwrap();
    state["revision"] = json!(17);
    state["checks"] = checks(17);
    write_json(&legacy_state, &state);
    binary()
        .args(["run", "status", "legacy-checks", "--project"])
        .arg(legacy.path())
        .arg("--json")
        .assert()
        .failure();

    let managed = TempDir::new().unwrap();
    init(managed.path(), "managed-checks", &four_plan(4), "full")
        .assert()
        .success();
    let managed_state = managed
        .path()
        .join(".codeunlimited/runs/managed-checks/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&managed_state).unwrap()).unwrap();
    state["revision"] = json!(17);
    state["checks"] = checks(17);
    write_json(&managed_state, &state);
    binary()
        .args(["run", "status", "managed-checks", "--project"])
        .arg(managed.path())
        .arg("--json")
        .assert()
        .success();
}

#[cfg(unix)]
#[test]
fn fifo_work_plan_is_rejected_without_blocking_on_open() {
    let project = TempDir::new().unwrap();
    let workflow = project.path().join("workflow.md");
    let fifo = project.path().join("plan.fifo");
    fs::write(&workflow, "# Workflow\n").unwrap();
    assert!(ProcessCommand::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_codeunlimited"))
        .args(["run", "init", "fifo", "--project"])
        .arg(project.path())
        .arg("--skill")
        .arg(workflow)
        .args([
            "--objective",
            "X",
            "--provider",
            "command",
            "--provider-executable",
            python(),
            "--work-plan",
        ])
        .arg(fifo)
        .args(["--verify-program", python()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    if status.is_none() {
        child.kill().unwrap();
        child.wait().unwrap();
    }
    assert!(status.is_some(), "FIFO plan open blocked the CLI");
    assert!(!status.unwrap().success());
}

#[test]
fn private_task_report_is_neither_tracked_nor_packaged() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let report = ".superpowers/sdd/2026-09-04-v2.2-delivery/task-2-report.md";
    let tracked = ProcessCommand::new("git")
        .args(["ls-files", "--error-unmatch", report])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        !tracked.status.success(),
        "private report must remain untracked"
    );

    let package = ProcessCommand::new("cargo")
        .args(["package", "--list", "--allow-dirty", "--locked"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        package.status.success(),
        "{}",
        String::from_utf8_lossy(&package.stderr)
    );
    let files = String::from_utf8(package.stdout).unwrap();
    assert!(!files.lines().any(|path| path == report));
    assert!(!files.lines().any(|path| path.starts_with(".superpowers/")));
}

#[test]
fn failed_verification_accepts_no_tasks_even_when_the_worker_claims_one() {
    let project = TempDir::new().unwrap();
    init(
        project.path(),
        "verify-fails",
        &four_plan(4),
        "wrong-verification",
    )
    .assert()
    .success();
    initialize_git(project.path());
    let output = step(project.path(), "verify-fails");
    assert!(!output.status.success());
    let mut ledger = binary();
    ledger
        .args(["run", "ledger", "verify-fails", "--project"])
        .arg(project.path())
        .arg("--json");
    let ledger = json_output(ledger);
    assert_eq!(ledger["accepted_task_count"], 0);
    assert_eq!(ledger["attempts"][0]["accepted_task_ids"], json!([]));
    let attempt: Value = serde_json::from_slice(
        &fs::read(
            project
                .path()
                .join(".codeunlimited/runs/verify-fails/attempts/00000001.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(attempt["verification_passed"], false);
}

#[test]
fn ledger_requires_declared_ids_and_recorded_verification_evidence() {
    for mutation in ["missing-evidence", "undeclared-id", "duplicate-id"] {
        let project = TempDir::new().unwrap();
        init(project.path(), mutation, &four_plan(4), "full")
            .assert()
            .success();
        initialize_git(project.path());
        assert!(step(project.path(), mutation).status.success());
        let attempt_path = project.path().join(format!(
            ".codeunlimited/runs/{mutation}/attempts/00000001.json"
        ));
        let mut attempt: Value = serde_json::from_slice(&fs::read(&attempt_path).unwrap()).unwrap();
        if mutation == "missing-evidence" {
            attempt
                .as_object_mut()
                .unwrap()
                .remove("verification_passed");
        } else if mutation == "undeclared-id" {
            attempt["accepted_task_ids"] = json!(["unit-a", "invented"]);
        } else {
            attempt["accepted_task_ids"] = json!(["unit-a", "unit-a"]);
        }
        write_json(&attempt_path, &attempt);
        binary()
            .args(["run", "ledger", mutation, "--project"])
            .arg(project.path())
            .arg("--json")
            .assert()
            .failure();
    }
}

#[test]
fn ledger_denominator_counts_verified_declared_tasks_only() {
    let project = TempDir::new().unwrap();
    init(project.path(), "denominator", &four_plan(4), "full")
        .assert()
        .success();
    initialize_git(project.path());
    assert!(step(project.path(), "denominator").status.success());
    let attempt_path = project
        .path()
        .join(".codeunlimited/runs/denominator/attempts/00000001.json");
    let mut attempt: Value = serde_json::from_slice(&fs::read(&attempt_path).unwrap()).unwrap();
    attempt["usage"] = json!({
        "input_token_semantics":"total_includes_cache",
        "input_tokens":80,
        "uncached_input_tokens":null,
        "cache_read_input_tokens":null,
        "cache_write_input_tokens":null,
        "cache_write_5m_input_tokens":null,
        "cache_write_1h_input_tokens":null,
        "output_tokens":20
    });
    write_json(&attempt_path, &attempt);
    let mut ledger = binary();
    ledger
        .args(["run", "ledger", "denominator", "--project"])
        .arg(project.path())
        .arg("--json");
    let report = json_output(ledger);
    assert_eq!(report["accepted_task_count"], 4);
    assert_eq!(report["tokens_per_accepted_task"], 25.0);
}

#[test]
fn legacy_prompt_contains_no_packet_contract() {
    let project = TempDir::new().unwrap();
    let workflow = project.path().join("workflow.md");
    fs::write(&workflow, "# Workflow\nPerform one bounded increment.\n").unwrap();
    binary()
        .args(["run", "init", "legacy", "--project"])
        .arg(project.path())
        .arg("--skill")
        .arg(workflow)
        .args([
            "--objective",
            "Implement the fixture feature",
            "--provider",
            "command",
            "--provider-executable",
            python(),
            "--allow-unverified-completion",
        ])
        .assert()
        .success();
    let output = binary()
        .args(["run", "prompt", "legacy", "--project"])
        .arg(project.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("SELECTED_PACKET"));
    assert!(!text.contains("managed work plan"));
}

#[test]
fn packet_preview_json_has_only_bounded_public_fields() {
    let project = TempDir::new().unwrap();
    init(project.path(), "shape", &parser_plan(4), "full")
        .assert()
        .success();
    let value = packet(project.path(), "shape");
    let keys: BTreeSet<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "reason",
            "remaining_count",
            "revision",
            "run_name",
            "schema_version",
            "selected_task_ids",
            "tasks"
        ])
    );
}
