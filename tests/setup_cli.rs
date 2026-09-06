use std::fs;
use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn command(root: &Path) -> Command {
    let mut c = Command::cargo_bin("codeunlimited").unwrap();
    c.env("HOME", root)
        .env("USERPROFILE", root)
        .env("CLAUDE_CONFIG_DIR", root.join("claude"))
        .env("CODEX_HOME", root.join("codex"))
        .env("CODEUNLIMITED_HOME", root.join("state"))
        .arg("setup");
    c
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn read(root: &Path, path: &str) -> String {
    fs::read_to_string(root.join(path)).unwrap()
}

#[test]
fn install_activates_both_providers_without_project_init_or_log_scan() {
    let t = TempDir::new().unwrap();
    command(t.path()).assert().success();
    let out = command(t.path())
        .args(["--status", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let status: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(status["enabled"], true);
    assert_eq!(status["activation"], "new_local_sessions");
    assert_eq!(status["codex_tool_output_token_limit"], 4000);
    assert_eq!(status["realized_savings_verified"], false);
    for p in ["claude/CLAUDE.md", "codex/AGENTS.md"] {
        assert!(read(t.path(), p).contains("<!-- codeunlimited:auto:v1 -->"));
    }
    assert!(!t.path().join(".codeunlimited.baseline.json").exists());
    assert!(!t.path().join("state").exists());
    assert!(!t.path().join("codex/AGENTS.override.md").exists());
}

#[test]
fn status_without_installation_is_read_only_and_nonzero() {
    let t = TempDir::new().unwrap();
    let out = command(t.path())
        .args(["--status", "--json"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let status: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(status["enabled"], false);
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 0);
}

#[test]
fn reinstall_preserves_user_instructions_and_first_backup() {
    let t = TempDir::new().unwrap();
    write(t.path(), "claude/CLAUDE.md", "Keep my rules.\n");
    command(t.path()).assert().success();
    let first = read(t.path(), "claude/CLAUDE.md");
    command(t.path()).assert().success();
    assert_eq!(first, read(t.path(), "claude/CLAUDE.md"));
    assert!(first.starts_with("Keep my rules.\n"));
    assert_eq!(
        read(t.path(), "claude/CLAUDE.md.codeunlimited.bak"),
        "Keep my rules.\n"
    );
}

#[test]
fn existing_codex_override_and_fallback_are_both_covered() {
    let t = TempDir::new().unwrap();
    write(t.path(), "codex/AGENTS.override.md", "Override rules.\n");
    command(t.path()).assert().success();
    assert!(read(t.path(), "codex/AGENTS.override.md").starts_with("Override rules.\n"));
    command(t.path()).arg("--status").assert().success();
    fs::remove_file(t.path().join("codex/AGENTS.override.md")).unwrap();
    command(t.path()).arg("--status").assert().success();
}

#[test]
fn empty_override_is_not_populated_so_it_does_not_shadow_user_guidance() {
    let t = TempDir::new().unwrap();
    write(t.path(), "codex/AGENTS.override.md", "  \n");
    command(t.path()).assert().success();
    assert_eq!(read(t.path(), "codex/AGENTS.override.md"), "  \n");
}

#[test]
fn explicit_codex_configuration_remains_byte_identical() {
    let t = TempDir::new().unwrap();
    let original = "# personal\ntool_output_token_limit = 9000\nmodel = 'my-model'\n[features]\nexample = false\n";
    write(t.path(), "codex/config.toml", original);
    command(t.path()).assert().success();
    assert_eq!(read(t.path(), "codex/config.toml"), original);
    command(t.path()).arg("--remove").assert().success();
    assert_eq!(read(t.path(), "codex/config.toml"), original);
}

#[test]
fn token_cap_is_top_level_preserving_tables_and_comments() {
    let t = TempDir::new().unwrap();
    let original = "# personal settings\n[features]\nexample = false\n";
    write(t.path(), "codex/config.toml", original);
    command(t.path()).assert().success();
    let text = read(t.path(), "codex/config.toml");
    let config: toml::Value = text.parse().unwrap();
    assert_eq!(config["tool_output_token_limit"].as_integer(), Some(4000));
    assert_eq!(config["features"]["example"].as_bool(), Some(false));
    assert!(text.ends_with(original));
}

#[test]
fn disabling_managed_setup_preserves_later_user_edits() {
    let t = TempDir::new().unwrap();
    write(t.path(), "claude/CLAUDE.md", "Before.\r\n");
    write(t.path(), "codex/config.toml", "# mine\n");
    command(t.path()).assert().success();
    let text = read(t.path(), "claude/CLAUDE.md") + "After.\r\n";
    write(t.path(), "claude/CLAUDE.md", &text);
    let config = read(t.path(), "codex/config.toml") + "model = 'my-model'\n";
    write(t.path(), "codex/config.toml", &config);
    command(t.path()).arg("--remove").assert().success();
    assert_eq!(read(t.path(), "claude/CLAUDE.md"), "Before.\r\nAfter.\r\n");
    assert_eq!(
        read(t.path(), "codex/config.toml"),
        "# mine\nmodel = 'my-model'\n"
    );
    command(t.path()).arg("--remove").assert().success();
    command(t.path()).arg("--status").assert().failure();
}

#[test]
fn invalid_toml_prevents_all_instruction_writes() {
    let t = TempDir::new().unwrap();
    write(t.path(), "codex/config.toml", "invalid = [");
    command(t.path()).assert().failure();
    assert!(!t.path().join("claude/CLAUDE.md").exists());
    assert_eq!(read(t.path(), "codex/config.toml"), "invalid = [");
}

#[test]
fn malformed_owned_markers_prevent_other_target_writes() {
    let t = TempDir::new().unwrap();
    let bad = "My text\n<!-- codeunlimited:auto:v1 -->\nunclosed";
    write(t.path(), "codex/AGENTS.md", bad);
    command(t.path()).assert().failure();
    assert!(!t.path().join("claude/CLAUDE.md").exists());
    assert_eq!(read(t.path(), "codex/AGENTS.md"), bad);
}

#[test]
fn status_detects_removed_and_shadowed_policy() {
    let t = TempDir::new().unwrap();
    command(t.path()).assert().success();
    write(
        t.path(),
        "codex/AGENTS.override.md",
        "New unrelated override",
    );
    command(t.path()).arg("--status").assert().failure();
    command(t.path()).assert().success();
    command(t.path()).arg("--status").assert().success();
}

#[test]
fn no_tool_limit_skips_config_creation() {
    let t = TempDir::new().unwrap();
    command(t.path()).arg("--no-tool-limit").assert().success();
    assert!(!t.path().join("codex/config.toml").exists());
    command(t.path()).arg("--status").assert().success();
}

#[test]
fn user_change_inside_managed_config_is_never_silently_removed() {
    let t = TempDir::new().unwrap();
    command(t.path()).assert().success();
    let changed = read(t.path(), "codex/config.toml").replace("4000", "6500");
    write(t.path(), "codex/config.toml", &changed);
    command(t.path()).arg("--remove").assert().failure();
    assert_eq!(read(t.path(), "codex/config.toml"), changed);
    assert!(read(t.path(), "claude/CLAUDE.md").contains("<!-- codeunlimited:auto:v1 -->"));
}

#[test]
fn relocated_managed_toml_is_not_mistaken_for_a_top_level_cap() {
    let t = TempDir::new().unwrap();
    command(t.path()).assert().success();
    let changed = "[features]\n".to_string() + &read(t.path(), "codex/config.toml");
    write(t.path(), "codex/config.toml", &changed);
    command(t.path()).assert().code(1);
    assert_eq!(read(t.path(), "codex/config.toml"), changed);
}

#[test]
fn invalid_utf8_prevents_writes_to_other_provider() {
    let t = TempDir::new().unwrap();
    write(t.path(), "codex/AGENTS.md", "");
    fs::write(t.path().join("codex/AGENTS.md"), [255, 254]).unwrap();
    command(t.path()).assert().code(1);
    assert_eq!(
        fs::read(t.path().join("codex/AGENTS.md")).unwrap(),
        [255, 254]
    );
    assert!(!t.path().join("claude/CLAUDE.md").exists());
}

#[test]
fn default_provider_homes_are_used_without_overrides() {
    let t = TempDir::new().unwrap();
    command(t.path())
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .assert()
        .success();
    assert!(t.path().join(".claude/CLAUDE.md").is_file());
    assert!(t.path().join(".codex/AGENTS.md").is_file());
    assert!(!t.path().join("claude").exists());
}

#[test]
fn relative_provider_home_is_rejected_without_writes() {
    let t = TempDir::new().unwrap();
    command(t.path())
        .env("CLAUDE_CONFIG_DIR", "relative-config")
        .assert()
        .code(1);
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 0);
}

#[test]
fn bom_config_round_trip_is_valid_and_preserves_original_bytes() {
    let t = TempDir::new().unwrap();
    let original = "\u{feff}# Windows editor\r\nmodel = 'my-model'\r\n";
    write(t.path(), "codex/config.toml", original);
    command(t.path()).assert().success();
    let installed = read(t.path(), "codex/config.toml");
    assert!(installed.starts_with('\u{feff}'));
    let parsed: toml::Value = installed.parse().unwrap();
    assert_eq!(parsed["tool_output_token_limit"].as_integer(), Some(4000));
    command(t.path()).assert().success();
    assert_eq!(read(t.path(), "codex/config.toml"), installed);
    command(t.path()).arg("--remove").assert().success();
    assert_eq!(read(t.path(), "codex/config.toml"), original);
}

#[cfg(unix)]
#[test]
fn symlinked_instruction_file_is_not_followed() {
    let t = TempDir::new().unwrap();
    write(t.path(), "outside", "private rules");
    fs::create_dir_all(t.path().join("codex")).unwrap();
    std::os::unix::fs::symlink(t.path().join("outside"), t.path().join("codex/AGENTS.md")).unwrap();
    command(t.path()).assert().failure();
    assert_eq!(read(t.path(), "outside"), "private rules");
    assert!(!t.path().join("claude/CLAUDE.md").exists());
}
