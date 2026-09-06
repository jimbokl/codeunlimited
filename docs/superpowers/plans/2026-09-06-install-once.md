# Install Once Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement task-by-task. User authorized autonomous inline execution and push.

**Goal:** One installation activates local efficiency defaults in future Claude Code and Codex sessions.

**Architecture:** Native global instruction files and an additive Codex output-history cap. Existing safeio handles backups and atomic writes; all target content is preflighted before mutation.

**Tech Stack:** Rust 1.82, clap, serde_json, toml, existing filesystem helpers; POSIX sh and PowerShell.

**Spec:** docs/superpowers/specs/2026-09-06-install-once-design.md

## Global Constraints

- Rust 1.82; no new runtime dependency or provider request.
- Do not modify project files, model choice, permissions, credentials or hooks.
- Respect CLAUDE_CONFIG_DIR and CODEX_HOME; reject unsafe/ambiguous files.
- Installation is automatic activation, not evidence of realized savings.

### Task 1: Safe global integration

Files: src/setupcmd.rs (new), src/main.rs, src/lib.rs, tests/setup_cli.rs (new).

Interface: `setupcmd::run(remove: bool, status: bool, json: bool, no_tool_limit: bool) -> i32`.

- [x] Add public CLI tests. A fresh `setup` must create global instructions and
  TOML with integer `tool_output_token_limit` equal to 4000. Read `setup --status
  --json` and assert `enabled == true`; absent installation exits 1 with false.
- [x] Run `cargo test --test setup_cli --locked`: expect missing setup command.
- [x] Implement target discovery, strict managed blocks, TOML preflight,
  install/status/remove and backup-preserving writes. Existing explicit settings
  remain byte-identical; duplicate install makes no change; removal retains
  unrelated edits and fails closed on malformed markers.
- [x] Run setup tests and full Rust suite; include implementation/tests in the final feature commit.

### Task 2: Installer activation

Files: install.sh, install.ps1, tests/test_installers.py, tests/test_install_ps1.ps1.

Consumes: `codeunlimited setup`, `setup --status --json`.

- [x] Add local-HTTP integration tests using the real binary and isolated homes:
  one installer invocation must make `setup --status --json` succeed; skip flag
  must leave homes untouched; activation conflict must return failure while
  leaving the verified binary installed and existing instructions intact.
- [x] Run `python3 -m unittest tests.test_installers` and observe missing activation.
- [x] Invoke the installed absolute binary after commit, check setup exit status,
  and report a distinct repairable activation failure. Add equivalent Windows
  scenarios in the native PowerShell harness, including isolated homes.
- [x] Run installer tests; include changes in the final feature commit.

### Task 3: Release and dogfood

Files: Cargo.toml, Cargo.lock, README.md, CHANGELOG.md, docs/VERSION-2.4.md,
.github/workflows/ci.yml and release version pins in tests.

- [x] Set current package/version pins to 2.4.0; document automatic vs optional
  behavior, scope, cap tradeoff, uninstall and repair commands.
- [x] Run full Rust/Python suites, fmt, Clippy, MSRV check, release metadata and
  package audit. Review changed code for safety and scope.
- [x] Build release, activate the same tested setup on this machine, verify status
  read-back without a model request. Preserve local backups.
- [ ] Commit, push codex/install-once and open a PR against main. Do not merge or
  create a release tag without separate release authorization.
