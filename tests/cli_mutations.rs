use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

fn cmd(project: &Path, state: &Path) -> Command {
    let mut command = Command::cargo_bin("codeunlimited").expect("binary");
    command
        .env("CLAUDE_HOME", state.join("claude"))
        .env("CODEX_HOME", state.join("codex"))
        .env("CODEUNLIMITED_HOME", state.join("state"))
        .arg("init")
        .arg(project);
    command
}

#[cfg(unix)]
#[test]
fn init_fails_when_project_is_not_writable() {
    use std::os::unix::fs::PermissionsExt;

    let project = TempDir::new().expect("project tempdir");
    let state = TempDir::new().expect("state tempdir");
    fs::set_permissions(project.path(), fs::Permissions::from_mode(0o555))
        .expect("make project read-only");

    cmd(project.path(), state.path()).assert().failure();

    fs::set_permissions(project.path(), fs::Permissions::from_mode(0o755))
        .expect("restore project permissions");
    assert!(!project.path().join("CLAUDE.md").exists());
    assert!(!project.path().join("AGENTS.md").exists());
    assert!(!project.path().join(".codeunlimited.baseline.json").exists());
}

#[test]
fn init_preserves_non_utf8_instruction_file() {
    let project = TempDir::new().expect("project tempdir");
    let state = TempDir::new().expect("state tempdir");
    let agents = project.path().join("AGENTS.md");
    fs::write(&agents, [0xff, 0xfe, 0xfd]).expect("write invalid UTF-8 fixture");

    cmd(project.path(), state.path()).assert().failure();

    assert_eq!(
        fs::read(&agents).expect("read original bytes"),
        [0xff, 0xfe, 0xfd]
    );
    assert!(!project.path().join("CLAUDE.md").exists());
}

#[cfg(unix)]
#[test]
fn init_rejects_symlinked_instruction_file() {
    use std::os::unix::fs::symlink;

    let project = TempDir::new().expect("project tempdir");
    let state = TempDir::new().expect("state tempdir");
    let outside = state.path().join("outside.md");
    fs::write(&outside, "keep\n").expect("write outside fixture");
    symlink(&outside, project.path().join("AGENTS.md")).expect("create symlink fixture");

    cmd(project.path(), state.path()).assert().failure();

    assert_eq!(
        fs::read_to_string(&outside).expect("read outside fixture"),
        "keep\n"
    );
    assert!(!project.path().join("CLAUDE.md").exists());
}
