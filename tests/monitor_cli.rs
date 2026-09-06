use assert_cmd::Command;
use chrono::Utc;
use serde_json::{json, Value};
use std::{fs, path::Path};
use tempfile::TempDir;

fn command(root: &Path) -> Command {
    let mut c = Command::cargo_bin("codeunlimited").unwrap();
    c.env("HOME", root)
        .env("USERPROFILE", root)
        .env_remove("CLAUDE_HOME")
        .env("CLAUDE_CONFIG_DIR", root.join("claude"))
        .env("CODEX_HOME", root.join("codex"))
        .env("CODEUNLIMITED_HOME", root.join("state"))
        .arg("monitor");
    c
}

fn saved(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(root.join("state/monitor/state.json")).unwrap()).unwrap()
}

fn rows(root: &Path, session: &str, ts: i64, tokens: u64, count: usize) {
    let path = root.join(format!("claude/projects/p/{session}.jsonl"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text: String = (0..count).map(|i| format!("{}\n", json!({
        "type":"assistant", "sessionId":session,
        "timestamp":chrono::DateTime::from_timestamp(ts,0).unwrap().to_rfc3339(),
        "message":{"id":format!("{session}-{i}"), "model":"test", "content":"SECRET TRANSCRIPT",
        "usage":{"input_tokens":tokens,"output_tokens":10}}
    }))).collect();
    fs::write(path, text).unwrap();
}

#[test]
fn empty_baseline_is_not_zero_percent_savings_and_reenable_keeps_it() {
    let t = TempDir::new().unwrap();
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    let initial = saved(t.path());
    assert_eq!(initial["baseline"]["records"], 0);
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    assert_eq!(initial, saved(t.path()));
    command(t.path())
        .args(["check", "--quiet"])
        .assert()
        .success()
        .stdout("");
    let state = saved(t.path());
    assert_eq!(
        state["snapshots"][0]["comparison"]["input_reduction_pct"],
        Value::Null
    );
    assert_eq!(
        state["snapshots"][0]["comparison"]["status"],
        "insufficient_data"
    );
    assert!(t.path().join("state/monitor/report.md").exists());
    assert!(
        !fs::read_to_string(t.path().join("state/monitor/state.json"))
            .unwrap()
            .contains("SECRET TRANSCRIPT")
    );
}

#[test]
fn check_excludes_old_sessions_and_replaces_the_same_daily_snapshot() {
    let t = TempDir::new().unwrap();
    rows(t.path(), "old", Utc::now().timestamp() - 60, 1000, 2);
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    let activation = saved(t.path())["activated_at"].as_i64().unwrap();
    rows(t.path(), "old", activation, 1000, 3);
    rows(t.path(), "new", activation, 100, 2);
    command(t.path()).arg("check").assert().success();
    command(t.path()).arg("check").assert().success();
    let state = saved(t.path());
    assert_eq!(state["snapshots"].as_array().unwrap().len(), 1);
    assert_eq!(state["snapshots"][0]["cohort"]["records"], 2);
    assert_eq!(state["snapshots"][0]["cohort"]["prompt_tokens"], 200);
    assert_eq!(state["snapshots"][0]["observed"]["records"], 5);
    assert_eq!(state["baseline"]["records"], 2);
}

#[test]
fn monitor_status_is_read_only_and_disable_preserves_reports() {
    let t = TempDir::new().unwrap();
    command(t.path()).arg("status").assert().failure();
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 0);
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    command(t.path()).arg("check").assert().success();
    command(t.path()).arg("disable").assert().success();
    assert_eq!(saved(t.path())["enabled"], false);
    assert!(t.path().join("state/monitor/report.md").exists());
    command(t.path())
        .args(["check", "--quiet"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn malformed_or_symlink_state_is_not_overwritten() {
    let t = TempDir::new().unwrap();
    fs::create_dir_all(t.path().join("state/monitor")).unwrap();
    let path = t.path().join("state/monitor/state.json");
    fs::write(&path, "broken").unwrap();
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .failure();
    assert_eq!(fs::read_to_string(&path).unwrap(), "broken");
}

#[test]
fn malformed_usage_prevents_claim_and_is_visible() {
    let t = TempDir::new().unwrap();
    rows(t.path(), "old", Utc::now().timestamp() - 60, 1000, 2);
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    fs::write(
        t.path().join("claude/projects/p/bad.jsonl"),
        "{\"type\":\"assistant\", bad json\n",
    )
    .unwrap();
    command(t.path())
        .args(["check", "--quiet"])
        .assert()
        .failure();
    let state = saved(t.path());
    assert_eq!(
        state["snapshots"][0]["comparison"]["status"],
        "incomplete_accounting"
    );
    assert_eq!(
        state["snapshots"][0]["comparison"]["input_reduction_pct"],
        Value::Null
    );
}

#[test]
fn matched_cohorts_report_observational_input_change_not_causal_savings() {
    let t = TempDir::new().unwrap();
    for i in 0..5 {
        rows(
            t.path(),
            &format!("base{i}"),
            Utc::now().timestamp() - 60,
            1000,
            20,
        );
    }
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    let activation = saved(t.path())["activated_at"].as_i64().unwrap();
    for i in 0..5 {
        rows(t.path(), &format!("new{i}"), activation, 500, 20);
    }
    command(t.path()).arg("check").assert().success();
    let state = saved(t.path());
    let cmp = &state["snapshots"][0]["comparison"];
    assert_eq!(cmp["status"], "observational_change");
    assert_eq!(cmp["input_reduction_pct"], 50.0);
    assert_eq!(cmp["current_coverage_pct"], 100.0);
    assert_eq!(cmp["causal_savings_verified"], false);
    assert!(cmp["quality"].as_str().unwrap().starts_with("unknown"));
    assert!(
        !fs::read_to_string(t.path().join("state/monitor/state.json"))
            .unwrap()
            .contains("SECRET TRANSCRIPT")
    );
}

#[test]
fn monitor_uses_saved_provider_roots_when_scheduler_environment_changes() {
    let t = TempDir::new().unwrap();
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    let activation = saved(t.path())["activated_at"].as_i64().unwrap();
    rows(t.path(), "new", activation, 50, 3);
    command(t.path())
        .env("CLAUDE_CONFIG_DIR", t.path().join("wrong"))
        .env("CODEX_HOME", t.path().join("wrong2"))
        .arg("check")
        .assert()
        .success();
    assert_eq!(saved(t.path())["snapshots"][0]["cohort"]["records"], 3);
}

#[test]
fn unknown_timestamp_cannot_seed_a_complete_baseline() {
    let t = TempDir::new().unwrap();
    rows(t.path(), "bad", Utc::now().timestamp() - 60, 1000, 1);
    let path = t.path().join("claude/projects/p/bad.jsonl");
    let mut row: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    row["timestamp"] = json!("unknown");
    fs::write(path, row.to_string()).unwrap();
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .failure();
    assert!(!t.path().join("state/monitor/state.json").exists());
}

#[test]
fn check_keeps_at_most_ninety_snapshots() {
    let t = TempDir::new().unwrap();
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    command(t.path()).arg("check").assert().success();
    let mut state = saved(t.path());
    let template = state["snapshots"][0].clone();
    let now = Utc::now().timestamp();
    state["snapshots"] = json!((1..=90)
        .rev()
        .map(|i| {
            let mut snapshot = template.clone();
            snapshot["checked_at"] = json!(now - i * 86400);
            snapshot
        })
        .collect::<Vec<_>>());
    fs::write(t.path().join("state/monitor/state.json"), state.to_string()).unwrap();
    command(t.path()).arg("check").assert().success();
    let state = saved(t.path());
    assert_eq!(state["snapshots"].as_array().unwrap().len(), 90);
    assert!(state["snapshots"][0]["checked_at"].as_i64().unwrap() > now - 90 * 86400);
}

#[cfg(unix)]
#[test]
fn final_state_symlink_cannot_replace_an_unrelated_file() {
    let t = TempDir::new().unwrap();
    fs::create_dir_all(t.path().join("state/monitor")).unwrap();
    let other = t.path().join("unrelated");
    fs::write(&other, "preserve").unwrap();
    std::os::unix::fs::symlink(&other, t.path().join("state/monitor/state.json")).unwrap();
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .failure();
    assert_eq!(fs::read_to_string(other).unwrap(), "preserve");
}

#[cfg(unix)]
#[test]
fn explicit_existing_state_directory_permissions_are_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let t = TempDir::new().unwrap();
    let directory = t.path().join("chosen");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o750)).unwrap();
    command(t.path())
        .args(["enable", "--no-schedule", "--state-dir"])
        .arg(&directory)
        .assert()
        .success();
    assert_eq!(
        fs::metadata(directory).unwrap().permissions().mode() & 0o777,
        0o750
    );
}

#[test]
fn a_preexisting_file_without_usage_is_not_a_new_session() {
    let t = TempDir::new().unwrap();
    let path = t.path().join("claude/projects/p/already-open.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "{\"type\":\"user\",\"sessionId\":\"already-open\"}\n",
    )
    .unwrap();
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    let activation = saved(t.path())["activated_at"].as_i64().unwrap();
    rows(t.path(), "already-open", activation, 100, 2);
    command(t.path()).arg("check").assert().success();
    assert_eq!(saved(t.path())["snapshots"][0]["cohort"]["records"], 0);
    assert_eq!(saved(t.path())["snapshots"][0]["observed"]["records"], 2);
}

fn codex_rows(root: &Path, session: &str, cwd: &str, ts: i64) {
    let dir = root.join("codex/sessions");
    fs::create_dir_all(&dir).unwrap();
    let mut text = format!(
        "{}\n",
        json!({"type":"turn_context", "payload":{"cwd":cwd,"model":"test"}})
    );
    for _ in 0..20 {
        text.push_str(&format!(
            "{}\n",
            json!({"type":"event_msg",
            "timestamp":chrono::DateTime::from_timestamp(ts,0).unwrap().to_rfc3339(),
            "payload":{"type":"token_count","info":{"last_token_usage":{
                "input_tokens":100,"cached_input_tokens":0,"output_tokens":10}}}})
        ));
    }
    fs::write(dir.join(format!("{session}.jsonl")), text).unwrap();
}

#[test]
fn codex_projects_with_same_basename_cannot_match() {
    let t = TempDir::new().unwrap();
    for i in 0..5 {
        codex_rows(
            t.path(),
            &format!("base{i}"),
            "/client-a/app",
            Utc::now().timestamp() - 60,
        );
    }
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    let activation = saved(t.path())["activated_at"].as_i64().unwrap();
    for i in 0..5 {
        codex_rows(t.path(), &format!("new{i}"), "/client-b/app", activation);
    }
    command(t.path()).arg("check").assert().success();
    let state = saved(t.path());
    assert_eq!(
        state["snapshots"][0]["comparison"]["current_coverage_pct"],
        0.0
    );
    assert_eq!(
        state["snapshots"][0]["comparison"]["input_reduction_pct"],
        Value::Null
    );
}

#[test]
fn claude_nested_subagents_keep_their_project_identity() {
    let t = TempDir::new().unwrap();
    rows(t.path(), "agent-a", Utc::now().timestamp() - 60, 100, 1);
    rows(t.path(), "agent-b", Utc::now().timestamp() - 60, 100, 1);
    for (name, project) in [("agent-a", "project-a"), ("agent-b", "project-b")] {
        let dest = t.path().join(format!(
            "claude/projects/{project}/s/subagents/{name}.jsonl"
        ));
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::rename(
            t.path().join(format!("claude/projects/p/{name}.jsonl")),
            dest,
        )
        .unwrap();
    }
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    assert_eq!(
        saved(t.path())["baseline"]["groups"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn disable_stops_collection_even_if_owned_scheduler_metadata_is_corrupt() {
    let t = TempDir::new().unwrap();
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    fs::write(
        t.path().join("state/monitor/schedule-owner.json"),
        "invalid",
    )
    .unwrap();
    command(t.path()).arg("disable").assert().failure();
    assert_eq!(saved(t.path())["enabled"], false);
    command(t.path())
        .args(["check", "--quiet"])
        .assert()
        .success();
    assert_eq!(saved(t.path())["snapshots"].as_array().unwrap().len(), 0);
}

#[test]
fn overflowed_usage_is_incomplete_not_a_savings_number() {
    let t = TempDir::new().unwrap();
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    let activation = saved(t.path())["activated_at"].as_i64().unwrap();
    rows(t.path(), "overflow", activation, u64::MAX, 2);
    command(t.path()).arg("check").assert().failure();
    let state = saved(t.path());
    assert_eq!(
        state["snapshots"][0]["comparison"]["status"],
        "incomplete_accounting"
    );
    assert_eq!(
        state["snapshots"][0]["comparison"]["input_reduction_pct"],
        Value::Null
    );
}

#[test]
fn baseline_comparison_caps_session_size_but_reports_observed_full_usage() {
    let t = TempDir::new().unwrap();
    command(t.path())
        .args(["enable", "--no-schedule"])
        .assert()
        .success();
    let activation = saved(t.path())["activated_at"].as_i64().unwrap();
    rows(t.path(), "large", activation, 100, 40);
    command(t.path()).arg("check").assert().success();
    let state = saved(t.path());
    assert_eq!(state["snapshots"][0]["observed"]["records"], 40);
    assert_eq!(state["snapshots"][0]["cohort"]["records"], 20);
}
