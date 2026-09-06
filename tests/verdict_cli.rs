use std::fs;
use std::path::Path;

use assert_cmd::Command;
use serde_json::{json, Value};
use tempfile::TempDir;

fn command(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("codeunlimited").unwrap();
    cmd.env("CLAUDE_HOME", root.join("claude"))
        .env("CODEX_HOME", root.join("codex"))
        .env("CODEUNLIMITED_HOME", root.join("state"));
    cmd
}

fn write_rows(path: &Path, rows: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = rows.iter().map(|r| format!("{r}\n")).collect::<String>();
    fs::write(path, text).unwrap();
}

fn claude_rows(values: &[Value]) -> Vec<Value> {
    values.iter().enumerate().map(|(i, n)| json!({
        "type": "assistant", "sessionId": "s", "timestamp": format!("2026-01-01T00:00:{i:02}Z"),
        "message": {"id": format!("m{i}"), "model": "test", "usage": {"input_tokens": n, "output_tokens": 1}}
    })).collect()
}

fn verdict(root: &Path, exit: i32) -> Value {
    let output = command(root).args(["verdict", "--json"]).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn shrinking_context_preserves_signed_difference_and_direction() {
    let root = TempDir::new().unwrap();
    let mut values = vec![json!(100_000); 5];
    values.extend(vec![json!(10_000); 35]);
    write_rows(
        &root.path().join("claude/projects/p/s.jsonl"),
        &claude_rows(&values),
    );
    let v = verdict(root.path(), 0);
    assert_eq!(v["modeled_difference_tokens"], -3_150_000.0);
    assert_eq!(v["sessions_negative_modeled_difference"], 1);
    assert_eq!(v["observed_prompt_tokens_included"], 850_000);
    assert_eq!(v["modeled_savings_tokens"], -3_150_000.0);
    assert_eq!(v["complete_accounting"], true);
}

#[test]
fn malformed_counter_cannot_manufacture_a_verdict() {
    let root = TempDir::new().unwrap();
    let mut values = vec![json!(10_000); 40];
    values[0] = json!("bad-private-body");
    write_rows(
        &root.path().join("claude/projects/p/s.jsonl"),
        &claude_rows(&values),
    );
    let v = verdict(root.path(), 2);
    assert_eq!(v["complete_accounting"], false);
    assert_eq!(v["status"], "incomplete");
    assert_eq!(v["scan"]["malformed_records"], 1);
    assert_eq!(v["observed_prompt_tokens_all"], 390_000);
    assert!(v["modeled_difference_tokens"].is_null());
    assert!(!v.to_string().contains("bad-private-body"));
}

#[test]
fn unreadable_file_prevents_complete_accounting() {
    let root = TempDir::new().unwrap();
    let path = root.path().join("claude/projects/p/s.jsonl");
    write_rows(&path, &claude_rows(&vec![json!(10); 40]));
    fs::write(path.with_file_name("bad.jsonl"), [0xff, 0xfe]).unwrap();
    let v = verdict(root.path(), 2);
    assert_eq!(v["scan"]["files_failed"], 1);
    assert_eq!(v["complete_accounting"], false);
    assert!(v["modeled_bounded_tokens_included"].is_null());
}

#[test]
fn empty_history_has_the_same_metric_keys_and_writes_nothing() {
    let root = TempDir::new().unwrap();
    let empty = verdict(root.path(), 0);
    write_rows(
        &root.path().join("claude/projects/p/s.jsonl"),
        &claude_rows(&vec![json!(10); 40]),
    );
    let full = verdict(root.path(), 0);
    assert_eq!(
        empty.as_object().unwrap().keys().collect::<Vec<_>>(),
        full.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    assert_eq!(empty["status"], "no_eligible_sessions");
    assert_eq!(empty["requests_total"], 0);
    assert!(empty["modeled_difference_tokens"].is_null());
    assert!(!root.path().join("state").exists());
}

#[test]
fn invalid_options_are_errors_not_empty_history() {
    let root = TempDir::new().unwrap();
    command(root.path())
        .args(["verdict", "--early-turns", "0"])
        .assert()
        .code(2);
    command(root.path())
        .args(["verdict", "--project"])
        .arg(root.path().join("absent"))
        .assert()
        .code(2);
}

fn codex_event(ts: &str, input: u64, total: Option<u64>) -> Value {
    let usage = json!({"input_tokens": input, "cached_input_tokens": 0, "output_tokens": 1});
    let mut info = json!({"last_token_usage": usage});
    if let Some(total) = total {
        info["total_token_usage"] = json!({"input_tokens": total, "cached_input_tokens": 0, "output_tokens": total / input});
    }
    json!({"type": "event_msg", "timestamp": ts, "payload": {"type": "token_count", "info": info}})
}

fn codex_rows() -> Vec<Value> {
    vec![json!({"type": "turn_context", "payload": {"cwd": "/work/p", "model": "test"}})]
}

#[test]
fn codex_repeats_are_removed_but_equal_calls_and_resets_are_kept() {
    let root = TempDir::new().unwrap();
    let mut rows = codex_rows();
    for total in [100, 100, 200, 100] {
        rows.push(codex_event("2026-01-01T00:00:00Z", 100, Some(total)));
    }
    rows.push(codex_event("2026-01-01T00:00:00Z", 100, None));
    rows.push(codex_event("2026-01-01T00:00:00Z", 100, None));
    write_rows(&root.path().join("codex/sessions/s.jsonl"), &rows);
    let v = verdict(root.path(), 0);
    assert_eq!(v["requests_total"], 5);
    assert_eq!(v["observed_prompt_tokens_all"], 500);
    assert_eq!(v["scan"]["duplicate_usage_records"], 1);
    assert_eq!(v["scan"]["usage_without_cumulative_counters"], 2);
    assert_eq!(v["scan"]["cumulative_resets"], 1);
    assert!(!root.path().join("state").exists());
}

#[test]
fn codex_deduplicates_before_the_time_cutoff() {
    let root = TempDir::new().unwrap();
    let mut rows = codex_rows();
    rows.push(codex_event("2000-01-01T00:00:00Z", 100, Some(100)));
    rows.push(codex_event("2099-01-01T00:00:00Z", 100, Some(100)));
    write_rows(&root.path().join("codex/sessions/s.jsonl"), &rows);
    let output = command(root.path())
        .args([
            "audit",
            "--source",
            "codex",
            "--days",
            "1",
            "--json",
            "--scan-stats",
            "--no-index",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(v["sources"].get("codex").is_none());
    assert_eq!(v["scan"]["duplicate_usage_records"], 1);
}

#[test]
fn overflowing_aggregate_is_not_an_exact_or_wrapped_total() {
    let root = TempDir::new().unwrap();
    write_rows(
        &root.path().join("claude/projects/p/s.jsonl"),
        &claude_rows(&vec![json!(u64::MAX); 40]),
    );
    let v = verdict(root.path(), 2);
    assert_eq!(v["overflow"], true);
    assert_eq!(v["complete_accounting"], false);
    assert!(v["observed_prompt_tokens_all"].is_null());
    assert!(v["modeled_difference_tokens"].is_null());
}

#[test]
fn audit_exposes_incompleteness_without_scan_stats_flag() {
    let root = TempDir::new().unwrap();
    let mut values = vec![json!(10_000); 40];
    values[0] = json!(-1);
    write_rows(
        &root.path().join("claude/projects/p/s.jsonl"),
        &claude_rows(&values),
    );
    let output = command(root.path())
        .args(["audit", "--json", "--no-index"])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["accounting"]["complete"], false);
    assert!(String::from_utf8_lossy(&output.stderr).contains("incomplete"));
}

#[test]
fn init_does_not_store_an_incomplete_baseline_or_claim_a_verdict() {
    let root = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let key = codeunlimited::parsers::claude_project_key(project.path());
    let mut values = vec![json!(10_000); 40];
    values[0] = json!(false);
    write_rows(
        &root
            .path()
            .join("claude/projects")
            .join(key)
            .join("s.jsonl"),
        &claude_rows(&values),
    );
    let output = command(root.path())
        .arg("init")
        .arg(project.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!project.path().join(".codeunlimited.baseline.json").exists());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("retro verdict:"));
}

#[test]
fn missing_required_counters_are_unknown_not_zero() {
    for source in ["claude", "codex"] {
        let root = TempDir::new().unwrap();
        if source == "claude" {
            let mut rows = claude_rows(&vec![json!(10_000); 40]);
            for row in &mut rows[..5] {
                row["message"]["usage"] = json!({});
            }
            write_rows(&root.path().join("claude/projects/p/s.jsonl"), &rows);
        } else {
            let mut rows = codex_rows();
            for i in 0..40 {
                let mut row = codex_event("2026-01-01T00:00:00Z", 10_000, None);
                if i < 5 {
                    row["payload"]["info"]["last_token_usage"] = json!({});
                }
                rows.push(row);
            }
            write_rows(&root.path().join("codex/sessions/s.jsonl"), &rows);
        }
        let v = verdict(root.path(), 2);
        assert_eq!(v["scan"]["malformed_records"], 5);
        assert_eq!(v["observed_prompt_tokens_all"], 350_000);
        assert!(v["modeled_difference_tokens"].is_null());
    }
}

#[test]
fn claude_split_cache_counters_cannot_disagree_with_the_total() {
    let root = TempDir::new().unwrap();
    let mut rows = claude_rows(&vec![json!(10); 40]);
    rows[0]["message"]["usage"]["cache_creation_input_tokens"] = json!(10);
    rows[0]["message"]["usage"]["cache_creation"] =
        json!({"ephemeral_5m_input_tokens": 100, "ephemeral_1h_input_tokens": 100});
    write_rows(&root.path().join("claude/projects/p/s.jsonl"), &rows);
    let v = verdict(root.path(), 2);
    assert_eq!(v["scan"]["malformed_records"], 1);
    assert!(v["modeled_difference_tokens"].is_null());
}

#[test]
fn truncated_candidate_without_usage_marker_is_not_silently_ignored() {
    for (path, tail) in [
        (
            "claude/projects/p/s.jsonl",
            "{\"type\":\"assistant\",\"message\":{",
        ),
        (
            "codex/sessions/s.jsonl",
            "{\"type\":\"event_msg\",\"payload\":{",
        ),
    ] {
        let root = TempDir::new().unwrap();
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, tail).unwrap();
        let v = verdict(root.path(), 2);
        assert_eq!(v["scan"]["malformed_records"], 1);
    }
}

#[test]
fn invalid_cumulative_cache_subset_cannot_drive_deduplication() {
    let root = TempDir::new().unwrap();
    let mut rows = codex_rows();
    let mut row = codex_event("2026-01-01T00:00:00Z", 100, Some(100));
    row["payload"]["info"]["total_token_usage"]["cached_input_tokens"] = json!(200);
    rows.extend([row.clone(), row]);
    write_rows(&root.path().join("codex/sessions/s.jsonl"), &rows);
    let v = verdict(root.path(), 2);
    assert_eq!(v["scan"]["malformed_records"], 2);
    assert_eq!(v["scan"]["duplicate_usage_records"], 0);
}

#[test]
fn claude_message_identity_is_scoped_to_project_and_session() {
    let root = TempDir::new().unwrap();
    let a = claude_rows(&vec![json!(10); 40]);
    let mut b = a.clone();
    for row in &mut b {
        row["sessionId"] = json!("different-session");
    }
    write_rows(&root.path().join("claude/projects/p/a.jsonl"), &a);
    write_rows(&root.path().join("claude/projects/p/b.jsonl"), &b);
    let v = verdict(root.path(), 0);
    assert_eq!(v["requests_total"], 80);
    assert_eq!(v["sessions_included"], 2);
    assert_eq!(v["observed_prompt_tokens_all"], 800);
}

#[test]
fn overflowing_counters_cannot_create_a_baseline_or_complete_audit() {
    let root = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let key = codeunlimited::parsers::claude_project_key(project.path());
    write_rows(
        &root
            .path()
            .join("claude/projects")
            .join(key)
            .join("s.jsonl"),
        &claude_rows(&vec![json!(u64::MAX); 40]),
    );
    let output = command(root.path())
        .args(["audit", "--json", "--no-index"])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["accounting"]["complete"], false);
    command(root.path())
        .arg("init")
        .arg(project.path())
        .assert()
        .failure();
    assert!(!project.path().join(".codeunlimited.baseline.json").exists());
}
