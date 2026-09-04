use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

use assert_cmd::Command;
use chrono::{SecondsFormat, Utc};
use serde_json::Value;
use tempfile::TempDir;

const FROM: &str = "2099-01-01T00:00:00Z";
const END_MINUS_ONE: &str = "2099-01-01T23:59:59Z";
const TO: &str = "2099-01-02T00:00:00Z";

fn binary(home: &Path) -> Command {
    let mut command = Command::cargo_bin("codeunlimited").expect("binary");
    command
        .env("CLAUDE_HOME", home.join("claude"))
        .env("CODEX_HOME", home.join("codex"))
        .env("CODEUNLIMITED_HOME", home.join("state"));
    command
}

fn run_json(home: &Path, args: &[&str]) -> Value {
    let output = binary(home)
        .args(args)
        .output()
        .expect("experiment process");
    assert!(
        output.status.success(),
        "experiment failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("experiment JSON")
}

fn write_window_fixtures(home: &Path, project: &Path) {
    let claude_key = codeunlimited::parsers::claude_project_key(project);
    let claude = home
        .join("claude/projects")
        .join(claude_key)
        .join("private-session.jsonl");
    fs::create_dir_all(claude.parent().expect("Claude fixture parent"))
        .expect("Claude fixture directory");
    fs::write(
        claude,
        format!(
            concat!(
                "{{\"type\":\"assistant\",\"sessionId\":\"claude-private-session\",",
                "\"timestamp\":\"{}\",\"message\":{{\"id\":\"one\",",
                "\"model\":\"claude-private-model\",\"content\":\"PRIVATE PROMPT TEXT\",",
                "\"usage\":{{\"input_tokens\":10,\"cache_read_input_tokens\":20,",
                "\"cache_creation_input_tokens\":70,\"cache_creation\":{{",
                "\"ephemeral_5m_input_tokens\":30,\"ephemeral_1h_input_tokens\":40}},",
                "\"output_tokens\":5}}}}}}\n",
                "{{\"type\":\"assistant\",\"sessionId\":\"claude-private-session\",",
                "\"timestamp\":\"{}\",\"message\":{{\"id\":\"two\",",
                "\"model\":\"claude-private-model\",\"usage\":{{",
                "\"input_tokens\":1,\"cache_read_input_tokens\":2,",
                "\"cache_creation_input_tokens\":7,\"cache_creation\":{{",
                "\"ephemeral_5m_input_tokens\":3,\"ephemeral_1h_input_tokens\":4}},",
                "\"output_tokens\":5}}}}}}\n",
                "{{\"type\":\"assistant\",\"sessionId\":\"claude-private-session\",",
                "\"timestamp\":\"{}\",\"message\":{{\"id\":\"three\",",
                "\"model\":\"claude-private-model\",\"usage\":{{",
                "\"input_tokens\":900,\"output_tokens\":900}}}}}}\n"
            ),
            FROM, END_MINUS_ONE, TO
        ),
    )
    .expect("Claude fixture");

    let codex = home.join("codex/sessions/2099/01/private-log.jsonl");
    fs::create_dir_all(codex.parent().expect("Codex fixture parent"))
        .expect("Codex fixture directory");
    fs::write(
        codex,
        format!(
            concat!(
                "{{\"type\":\"turn_context\",\"payload\":{{",
                "\"model\":\"gpt-private-model\",\"cwd\":\"{}\"}}}}\n",
                "{{\"timestamp\":\"{}\",\"type\":\"event_msg\",\"payload\":{{",
                "\"type\":\"token_count\",\"info\":{{\"last_token_usage\":{{",
                "\"input_tokens\":100,\"cached_input_tokens\":25,",
                "\"cache_write_input_tokens\":7,\"output_tokens\":8}}}}}}}}\n",
                "{{\"timestamp\":\"{}\",\"type\":\"event_msg\",\"payload\":{{",
                "\"type\":\"token_count\",\"info\":{{\"last_token_usage\":{{",
                "\"input_tokens\":50,\"cached_input_tokens\":10,",
                "\"cache_write_input_tokens\":5,\"output_tokens\":6}}}}}}}}\n",
                "{{\"timestamp\":\"{}\",\"type\":\"event_msg\",\"payload\":{{",
                "\"type\":\"token_count\",\"info\":{{\"last_token_usage\":{{",
                "\"input_tokens\":900,\"cached_input_tokens\":0,",
                "\"output_tokens\":900}}}}}}}}\n"
            ),
            project.display(),
            FROM,
            END_MINUS_ONE,
            TO
        ),
    )
    .expect("Codex fixture");
}

fn write_one_claude_request(home: &Path, project: &Path, timestamp: &str) {
    let claude_key = codeunlimited::parsers::claude_project_key(project);
    let path = home
        .join("claude/projects")
        .join(claude_key)
        .join("session.jsonl");
    fs::create_dir_all(path.parent().expect("Claude fixture parent"))
        .expect("Claude fixture directory");
    fs::write(
        path,
        format!(
            concat!(
                "{{\"type\":\"assistant\",\"sessionId\":\"s\",\"timestamp\":\"{}\",",
                "\"message\":{{\"id\":\"one\",\"model\":\"private-model\",",
                "\"usage\":{{\"input_tokens\":7,\"output_tokens\":3}}}}}}\n"
            ),
            timestamp
        ),
    )
    .expect("Claude fixture");
}

#[test]
fn record_counts_half_open_boundaries_and_redacts_private_fields() {
    let home = TempDir::new().expect("isolated homes");
    let project = TempDir::new().expect("project");
    write_window_fixtures(home.path(), project.path());

    let value = run_json(
        home.path(),
        &[
            "experiment",
            "record",
            "control",
            "--from",
            FROM,
            "--to",
            TO,
            "--tasks",
            "1",
            project.path().to_str().expect("UTF-8 project"),
            "--json",
        ],
    );

    assert_eq!(value["name"], "control");
    assert_eq!(value["started_unix"], 4_070_908_800_i64);
    assert_eq!(value["finished_unix"], 4_070_995_200_i64);
    assert_eq!(value["completed_tasks"], 1);
    assert_eq!(value["status"], "complete");
    assert_eq!(value["complete_accounting"], true);
    assert_eq!(value["records_without_timestamp"], 0);
    assert_eq!(value["totals"]["requests"], 4);
    assert_eq!(value["totals"]["sessions"], 2);
    assert_eq!(value["totals"]["uncached_input_tokens"], 126);
    assert_eq!(value["totals"]["cache_read_input_tokens"], 57);
    assert_eq!(value["totals"]["cache_write_5m_input_tokens"], 45);
    assert_eq!(value["totals"]["cache_write_1h_input_tokens"], 44);
    assert_eq!(value["totals"]["input_tokens"], 272);
    assert_eq!(value["totals"]["output_tokens"], 24);
    assert_eq!(value["totals"]["total_tokens"], 296);

    let state_path = project
        .path()
        .join(codeunlimited::experiment::EXPERIMENT_FILE);
    assert!(state_path.is_file());
    let public_bytes = [
        serde_json::to_vec(&value).expect("output bytes"),
        fs::read(state_path).expect("state bytes"),
    ]
    .concat();
    let public_text = String::from_utf8(public_bytes).expect("public UTF-8");
    for private in [
        "PRIVATE PROMPT TEXT",
        "claude-private-model",
        "gpt-private-model",
        project.path().to_str().expect("UTF-8 project"),
        home.path().to_str().expect("UTF-8 home"),
        "findings",
        "top_projects",
    ] {
        assert!(
            !public_text.contains(private),
            "leaked private value: {private}"
        );
    }
}

#[test]
fn start_finish_is_idempotent_and_list_is_sorted_json() {
    let home = TempDir::new().expect("isolated homes");
    let project = TempDir::new().expect("project");
    let project_arg = project.path().to_str().expect("UTF-8 project");

    binary(home.path())
        .args(["experiment", "start", "z-treatment", project_arg])
        .assert()
        .success();
    let state_path = project
        .path()
        .join(codeunlimited::experiment::EXPERIMENT_FILE);
    let state: Value =
        serde_json::from_slice(&fs::read(&state_path).expect("active state")).expect("active JSON");
    let started = state["records"]["z-treatment"]["started_unix"]
        .as_i64()
        .expect("start timestamp");
    let timestamp = chrono::DateTime::<Utc>::from_timestamp(started, 0)
        .expect("valid timestamp")
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    write_one_claude_request(home.path(), project.path(), &timestamp);
    while Utc::now().timestamp() <= started {
        thread::sleep(Duration::from_millis(10));
    }

    let first = binary(home.path())
        .args([
            "experiment",
            "finish",
            "z-treatment",
            "--tasks",
            "1",
            project_arg,
            "--json",
        ])
        .output()
        .expect("first finish");
    assert!(first.status.success());
    let first_value: Value = serde_json::from_slice(&first.stdout).expect("finish JSON");
    assert_eq!(first_value["totals"]["requests"], 1);
    assert_eq!(first_value["totals"]["input_tokens"], 7);
    let saved = fs::read(&state_path).expect("finished state");

    fs::remove_dir_all(home.path().join("claude")).expect("remove Claude logs");
    let second = binary(home.path())
        .args([
            "experiment",
            "finish",
            "z-treatment",
            "--tasks",
            "99",
            project_arg,
            "--json",
        ])
        .output()
        .expect("second finish");
    assert!(second.status.success());
    assert_eq!(second.stdout, first.stdout);
    assert_eq!(fs::read(&state_path).expect("unchanged state"), saved);

    run_json(
        home.path(),
        &[
            "experiment",
            "record",
            "a-control",
            "--from",
            FROM,
            "--to",
            TO,
            "--tasks",
            "1",
            project_arg,
            "--json",
        ],
    );
    let list = run_json(home.path(), &["experiment", "list", project_arg, "--json"]);
    assert_eq!(list.as_array().expect("list array").len(), 2);
    assert_eq!(list[0]["name"], "a-control");
    assert_eq!(list[1]["name"], "z-treatment");
}

#[test]
fn lifecycle_validation_failures_do_not_mutate_state() {
    let home = TempDir::new().expect("isolated homes");
    let project = TempDir::new().expect("project");
    let project_arg = project.path().to_str().expect("UTF-8 project");
    run_json(
        home.path(),
        &[
            "experiment",
            "record",
            "existing",
            "--from",
            FROM,
            "--to",
            TO,
            "--tasks",
            "1",
            project_arg,
            "--json",
        ],
    );
    let state_path = project
        .path()
        .join(codeunlimited::experiment::EXPERIMENT_FILE);
    let original = fs::read(&state_path).expect("original state");

    let invalid_commands: &[&[&str]] = &[
        &["experiment", "start", "existing", project_arg],
        &["experiment", "start", "bad/name", project_arg],
        &[
            "experiment",
            "record",
            "zero",
            "--from",
            FROM,
            "--to",
            TO,
            "--tasks",
            "0",
            project_arg,
        ],
        &[
            "experiment",
            "record",
            "bad-time",
            "--from",
            "not-a-time",
            "--to",
            TO,
            "--tasks",
            "1",
            project_arg,
        ],
        &[
            "experiment",
            "record",
            "equal",
            "--from",
            FROM,
            "--to",
            FROM,
            "--tasks",
            "1",
            project_arg,
        ],
        &[
            "experiment",
            "record",
            "reverse",
            "--from",
            TO,
            "--to",
            FROM,
            "--tasks",
            "1",
            project_arg,
        ],
        &[
            "experiment",
            "finish",
            "missing",
            "--tasks",
            "1",
            project_arg,
        ],
    ];

    for args in invalid_commands {
        binary(home.path()).args(*args).assert().failure();
        assert_eq!(
            fs::read(&state_path).expect("preserved state"),
            original,
            "state changed after {args:?}"
        );
    }
}

#[test]
fn every_mutating_command_preserves_malformed_state_bytes() {
    let home = TempDir::new().expect("isolated homes");
    let project = TempDir::new().expect("project");
    let project_arg = project.path().to_str().expect("UTF-8 project");
    let state_path = project
        .path()
        .join(codeunlimited::experiment::EXPERIMENT_FILE);
    let corrupt = br#"{"schema_version":1,"records":"private-corruption"}"#;
    fs::write(&state_path, corrupt).expect("corrupt state");

    let commands: &[&[&str]] = &[
        &["experiment", "start", "new", project_arg],
        &[
            "experiment",
            "finish",
            "existing",
            "--tasks",
            "1",
            project_arg,
        ],
        &[
            "experiment",
            "record",
            "new",
            "--from",
            FROM,
            "--to",
            TO,
            "--tasks",
            "1",
            project_arg,
        ],
    ];

    for args in commands {
        let output = binary(home.path())
            .args(*args)
            .output()
            .expect("mutation process");
        assert!(!output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stderr).lines().count(), 1);
        assert_eq!(fs::read(&state_path).expect("preserved state"), corrupt);
    }
}

#[cfg(unix)]
#[test]
fn symlinked_state_is_rejected_without_touching_the_target() {
    use std::os::unix::fs::symlink;

    let home = TempDir::new().expect("isolated homes");
    let project = TempDir::new().expect("project");
    let outside = home.path().join("outside.json");
    fs::write(&outside, b"private sentinel\n").expect("outside sentinel");
    symlink(
        &outside,
        project
            .path()
            .join(codeunlimited::experiment::EXPERIMENT_FILE),
    )
    .expect("state symlink");

    binary(home.path())
        .args([
            "experiment",
            "start",
            "new",
            project.path().to_str().expect("UTF-8 project"),
        ])
        .assert()
        .failure();

    assert_eq!(
        fs::read_to_string(outside).expect("outside sentinel"),
        "private sentinel\n"
    );
}

#[cfg(unix)]
#[test]
fn start_in_read_only_project_leaves_no_partial_state() {
    use std::os::unix::fs::PermissionsExt;

    let home = TempDir::new().expect("isolated homes");
    let project = TempDir::new().expect("project");
    fs::set_permissions(project.path(), fs::Permissions::from_mode(0o555))
        .expect("make project read-only");

    binary(home.path())
        .args([
            "experiment",
            "start",
            "new",
            project.path().to_str().expect("UTF-8 project"),
        ])
        .assert()
        .failure();

    fs::set_permissions(project.path(), fs::Permissions::from_mode(0o755))
        .expect("restore project permissions");
    assert!(!project
        .path()
        .join(codeunlimited::experiment::EXPERIMENT_FILE)
        .exists());
}
