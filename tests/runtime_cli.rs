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
        .stdout(predicate::str::contains("cache-probe"))
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
            "cache_read_ratio_basis_points",
            "dynamic_prompt_bytes",
            "prompt_bytes",
            "provider",
            "recovery_required",
            "revision",
            "run_name",
            "schema_version",
            "stable_prompt_bytes",
            "status",
            "transported_input_tokens",
            "usage",
        ])
    );
}

#[test]
fn subscription_profiles_preserve_integrations_by_default_and_lean_is_explicit() {
    let project = TempDir::new().expect("project");
    let workflow = project.path().join("workflow.md");
    fs::write(&workflow, "# Workflow\n").unwrap();

    let mut lean = binary();
    lean.args(["run", "init", "claude-lean", "--project"])
        .arg(project.path())
        .arg("--skill")
        .arg(&workflow)
        .args([
            "--objective",
            "Test",
            "--provider",
            "claude",
            "--subscription-profile",
            "lean",
        ]);
    lean.assert().success();

    let mut standard = binary();
    standard
        .args(["run", "init", "codex-standard", "--project"])
        .arg(project.path())
        .arg("--skill")
        .arg(&workflow)
        .args(["--objective", "Test", "--provider", "codex"]);
    standard.assert().success();

    for (name, expected) in [("claude-lean", "lean"), ("codex-standard", "standard")] {
        let mut status = binary();
        status
            .args(["run", "status", name, "--project"])
            .arg(project.path())
            .arg("--json");
        let value = json_output(status);
        assert_eq!(value["provider"]["layer"], "subscription_cli");
        assert_eq!(value["provider"]["capability"], "coding_agent");
        assert_eq!(value["provider"]["subscription_profile"], expected);
    }
}

#[test]
fn command_provider_rejects_subscription_profile() {
    let project = TempDir::new().expect("project");
    let workflow = project.path().join("workflow.md");
    fs::write(&workflow, "# Workflow\n").unwrap();
    let mut command = binary();
    command
        .args(["run", "init", "bad-profile", "--project"])
        .arg(project.path())
        .arg("--skill")
        .arg(&workflow)
        .args([
            "--objective",
            "Test",
            "--provider",
            "command",
            "--provider-executable",
            "true",
            "--subscription-profile",
            "lean",
        ]);

    command
        .assert()
        .failure()
        .stderr(predicate::str::contains("only valid for claude and codex"));
}

#[test]
fn protected_provider_arguments_fail_at_init_before_creating_a_run() {
    for (provider, argument) in [
        ("claude", "--resume=session"),
        ("claude", "-rsession"),
        ("claude", "--system-prompt=replace"),
        ("claude", "--append-system-prompt=extra"),
        ("claude", "--chrome"),
        ("claude", "--output-format=text"),
        ("codex", "-cmodel_instructions_file=replace.md"),
        ("codex", "--config=developer_instructions=replace"),
        ("codex", "-ooutput.json"),
        ("codex", "--thread-id=previous"),
    ] {
        let project = TempDir::new().unwrap();
        let skill = workflow(project.path());
        binary()
            .args(["run", "init", "protected", "--project"])
            .arg(project.path())
            .arg("--skill")
            .arg(skill)
            .args(["--objective", "Test", "--provider", provider])
            .arg(format!("--provider-arg={argument}"))
            .assert()
            .failure();
        assert!(!project
            .path()
            .join(".codeunlimited/runs/protected")
            .exists());
    }
}

#[test]
fn legacy_run_remains_inspectable_when_new_prompt_exceeds_its_budget() {
    let project = TempDir::new().unwrap();
    init_command(project.path(), "legacy", "success", &[])
        .assert()
        .success();
    let run = project.path().join(".codeunlimited/runs/legacy");
    fs::remove_file(run.join("provider-instructions.md")).unwrap();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(run.join("manifest.json")).unwrap()).unwrap();
    manifest["prompt_budget_bytes"] = serde_json::json!(1);
    fs::write(
        run.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let before = fs::read(run.join("manifest.json")).unwrap();
    binary()
        .args(["run", "status", "legacy", "--project"])
        .arg(project.path())
        .arg("--json")
        .assert()
        .success();
    binary()
        .args(["run", "prompt", "legacy", "--project"])
        .arg(project.path())
        .assert()
        .success();
    assert!(!run.join("provider-instructions.md").exists());
    assert_eq!(fs::read(run.join("manifest.json")).unwrap(), before);
    binary()
        .args(["run", "step", "legacy", "--project"])
        .arg(project.path())
        .assert()
        .code(6);
    assert_eq!(fs::read_dir(run.join("attempts")).unwrap().count(), 0);
}

#[test]
fn api_lifecycle_uses_mock_transport_and_keeps_failed_usage_separate_from_probe() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;
    for provider in ["openai-api", "anthropic-api"] {
        let anthropic = provider == "anthropic-api";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for index in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 8192];
                let body = loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .unwrap()
                            .trim()
                            .parse()
                            .unwrap();
                        assert!(headers.contains(if anthropic {
                            "x-api-key: fixture-only-key"
                        } else {
                            "authorization: bearer fixture-only-key"
                        }));
                        if anthropic {
                            assert!(headers.contains("anthropic-version: 2023-06-01"));
                        }
                        if request.len() >= end + 4 + length {
                            break serde_json::from_slice::<Value>(
                                &request[end + 4..end + 4 + length],
                            )
                            .unwrap();
                        }
                    }
                };
                bodies.push(body);
                let text = if index == 3 {
                    "PRIVATE_INVALID_RESPONSE".into()
                } else {
                    serde_json::json!({"schema_version":1,"base_revision":0,"outcome":"continue","summary":"mock step","delta":{}}).to_string()
                };
                let response = if anthropic {
                    serde_json::json!({"content":[{"type":"text","text":text}],"stop_reason":"end_turn",
                        "usage":{"input_tokens":100,"cache_read_input_tokens":80,"cache_creation_input_tokens":20,"output_tokens":3}})
                } else {
                    serde_json::json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":text}]}],
                        "usage":{"input_tokens":200,"input_tokens_details":{"cached_tokens":80,"cache_write_tokens":20},"output_tokens":3}})
                }.to_string();
                write!(stream,"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response}",response.len()).unwrap();
            }
            bodies
        });
        let project = TempDir::new().unwrap();
        let skill = workflow(project.path());
        fs::write(project.path().join(".gitignore"), ".codeunlimited/runs/\n").unwrap();
        initialize_git(project.path());
        binary()
            .args(["run", "init", "api", "--project"])
            .arg(project.path())
            .arg("--skill")
            .arg(skill)
            .args([
                "--objective",
                "Mock API step",
                "--provider",
                provider,
                "--api-model",
                "fixture-model",
                "--api-key-env",
                "CODEUNLIMITED_FIXTURE_API_KEY",
                "--api-endpoint",
            ])
            .arg(format!("http://{address}/endpoint"))
            .assert()
            .success();
        let invoke = |action: &str| {
            let mut command = binary();
            command
                .args(["run", action, "api", "--project"])
                .arg(project.path())
                .arg("--json")
                .env("CODEUNLIMITED_FIXTURE_API_KEY", "fixture-only-key");
            command
        };
        let step = json_output(invoke("step"));
        assert_eq!(step["revision"], 1);
        assert_eq!(step["transported_input_tokens"], 200);
        assert_eq!(step["cache_read_ratio_basis_points"], 4000);
        let probe = json_output(invoke("cache-probe"));
        assert_eq!(probe["cache_hit_reported"], true);
        let after_probe = json_output(invoke("status"));
        assert_eq!(after_probe["attempts"], 1);
        assert_eq!(after_probe["transported_input_tokens"], 200);
        invoke("step")
            .assert()
            .code(7)
            .stderr(predicate::str::contains("PRIVATE_INVALID_RESPONSE").not());
        let status = json_output(invoke("status"));
        assert_eq!(status["revision"], 1);
        assert_eq!(status["attempts"], 2);
        assert_eq!(status["transported_input_tokens"], 400);
        assert_eq!(status["usage"]["cache_read_input_tokens"], 160);
        let manifest =
            fs::read_to_string(project.path().join(".codeunlimited/runs/api/manifest.json"))
                .unwrap();
        assert!(!manifest.contains("fixture-only-key"));
        let bodies = server.join().unwrap();
        let stable = |body: &Value| {
            if anthropic {
                body["system"].clone()
            } else {
                body["input"][0].clone()
            }
        };
        assert_eq!(stable(&bodies[0]), stable(&bodies[3]));
        assert_eq!(stable(&bodies[1]), stable(&bodies[2]));
        assert_ne!(bodies[1], bodies[2]);
    }
}

#[test]
fn legacy_subscription_manifest_defaults_without_rewriting_on_inspection() {
    let project = TempDir::new().unwrap();
    let skill = workflow(project.path());
    binary()
        .args(["run", "init", "legacy", "--project"])
        .arg(project.path())
        .arg("--skill")
        .arg(skill)
        .args(["--objective", "Legacy", "--provider", "claude"])
        .assert()
        .success();
    let run = project.path().join(".codeunlimited/runs/legacy");
    let path = run.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["provider"]
        .as_object_mut()
        .unwrap()
        .remove("subscription_profile");
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::remove_file(run.join("provider-instructions.md")).unwrap();
    let before = fs::read(&path).unwrap();
    let mut command = binary();
    command
        .args(["run", "status", "legacy", "--project"])
        .arg(project.path())
        .arg("--json");
    let status = json_output(command);
    assert_eq!(status["provider"]["subscription_profile"], "standard");
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!run.join("provider-instructions.md").exists());
}

#[test]
fn api_provider_is_a_separate_keyless_init_layer() {
    let project = TempDir::new().expect("project");
    let workflow = project.path().join("workflow.md");
    fs::write(&workflow, "# Workflow\n").unwrap();
    let mut command = binary();
    command
        .args(["run", "init", "api-plan", "--project"])
        .arg(project.path())
        .arg("--skill")
        .arg(&workflow)
        .args([
            "--objective",
            "Plan one step",
            "--provider",
            "openai-api",
            "--api-model",
            "gpt-5.6",
            "--api-key-env",
            "PRIVATE_OPENAI_TOKEN",
        ]);
    command.assert().success();

    let mut status = binary();
    status
        .args(["run", "status", "api-plan", "--project"])
        .arg(project.path())
        .arg("--json");
    let value = json_output(status);
    assert_eq!(value["provider"]["kind"], "openai_api");
    assert_eq!(value["provider"]["layer"], "external_api");
    assert_eq!(
        value["provider"]["capability"],
        "single_turn_no_local_tools"
    );
    assert!(!value.to_string().contains("PRIVATE_OPENAI_TOKEN"));
}

#[test]
fn api_provider_rejects_remote_plain_http_and_wrong_ttl() {
    let project = TempDir::new().expect("project");
    let workflow = project.path().join("workflow.md");
    fs::write(&workflow, "# Workflow\n").unwrap();
    for extra in [
        vec!["--api-endpoint", "http://example.com/v1/responses"],
        vec!["--cache-ttl", "1h"],
    ] {
        let mut command = binary();
        command
            .args(["run", "init", "bad-api", "--project"])
            .arg(project.path())
            .arg("--skill")
            .arg(&workflow)
            .args([
                "--objective",
                "Test",
                "--provider",
                "openai-api",
                "--api-model",
                "gpt-5.6",
            ])
            .args(extra);
        command.assert().failure();
    }
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
fn fresh_provider_steps_promote_bounded_knowledge_from_hypothesis_to_verified() {
    let project = TempDir::new().expect("project");
    init_command(project.path(), "epistemic", "epistemic", &[])
        .arg("--verify-program")
        .arg(python())
        .arg("--verify-arg=-c")
        .arg("--verify-arg=raise SystemExit(0)")
        .arg("--verify-every-step")
        .assert()
        .success();

    let mut command = binary();
    command
        .args(["run", "auto", "epistemic", "--project"])
        .arg(project.path())
        .args(["--steps", "3", "--json"]);
    let report = json_output(command);
    assert_eq!(report["steps"].as_array().unwrap().len(), 2);
    assert_eq!(report["steps"][1]["status"], "complete");

    let state: Value = serde_json::from_slice(
        &fs::read(
            project
                .path()
                .join(".codeunlimited/runs/epistemic/state.json"),
        )
        .expect("state"),
    )
    .expect("state JSON");
    assert_eq!(state["epistemic"].as_array().unwrap().len(), 1);
    assert_eq!(state["epistemic"][0]["id"], "root-cause");
    assert_eq!(state["epistemic"][0]["status"], "verified");
    assert_eq!(state["epistemic"][0]["evidence"][0]["kind"], "check");
    assert_eq!(state["epistemic"][0]["evidence"][0]["revision"], 2);
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
