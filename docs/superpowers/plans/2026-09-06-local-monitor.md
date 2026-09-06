# Local Benefit Monitor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Install-once offline monitoring across every local project, then push the tested implementation.

**Architecture:** A Rust one-shot collector owns an immutable baseline and bounded daily snapshots. A separate platform adapter registers a daily native job; existing external schedulers can invoke the same collector. Reports distinguish recorded usage, observational changes and unknown causal savings.

**Tech Stack:** Rust 1.82, clap, serde, chrono, sha2, fs2, existing safe atomic file helpers; shell/PowerShell installers.

**Spec:** `docs/superpowers/specs/2026-09-06-local-monitor.md`

## Global Constraints

- Rust 1.82, existing dependencies only.
- No provider calls, uploads, raw transcript persistence or automatic session restarts.
- Retain at most 90 daily aggregate snapshots; baseline is immutable on re-enable.
- Minimum comparison sample: 100 usage records and 3 sessions per arm.
- Native scheduler tests must never register real OS jobs.
- Extend the existing install-once branch/PR, without merging or release tagging.

### Task 1: Deterministic collector and CLI

**Files:** Create `src/monitor.rs`, `tests/monitor_cli.rs`; modify `src/lib.rs`, `src/main.rs`.

**Interfaces:** `monitor::run(action: &str, state_dir: Option<PathBuf>, no_schedule: bool, quiet: bool) -> i32`; scheduler interface specified in Task 2.

- [x] Write public CLI tests: enable freezes baseline, check excludes old sessions, no-data produces null change, repeat enable preserves bytes, malformed state refuses replacement, disable retains reports.
- [x] Run `cargo test --test monitor_cli --locked` and observe missing-command failure.
- [x] Implement state serialization, exclusive fs2 lock, SHA-256 composite session IDs, per source/model/project aggregates, checked counters and matched comparison. Save `state.json` atomically and render `report.md` without raw text.
- [x] Run `cargo test --test monitor_cli --locked`; public CLI fixtures cover identical model/project matching, insufficient sample, counter overflow and 90-day retention.

### Task 2: Native scheduling adapter (independent)

**Files:** Create `src/monitor_schedule.rs` and its internal unit tests.

**Interfaces:** `install(exe: &Path, state_dir: &Path) -> io::Result<String>` and `remove(state_dir: &Path) -> io::Result<()>`. Generated job invokes the exact argv `[exe, "monitor", "check", "--state-dir", state_dir, "--quiet"]` daily. Register only the current user's owned job, persist an ownership manifest in state_dir; repeat install idempotently and refuse foreign conflicts. Public `install` is not called by any unit test; inject the command runner into the internal implementation.

- [x] Test hostile spaces/quotes/XML characters, foreign conflicts, command failure, repeat install and removal with injected runners.
- [x] Implement macOS LaunchAgent, Linux user systemd timer and Windows task using official native scheduler formats, preserving unrelated resources.
- [x] Run module tests through the library after the parent adds its module export; 21 isolated scheduler tests pass, including interrupted Windows registration recovery.

### Task 3: Install-once integration and documentation

**Files:** Modify `install.sh`, `install.ps1`, installer tests, `README.md`, `CHANGELOG.md`; add `docs/MONITORING.md`.

- [x] Add installer assertions that default setup invokes monitor enable; opt-out avoids both activation and monitoring. Isolated real-binary tests set `CODEUNLIMITED_SKIP_MONITOR=1`, while command fixtures validate the ordinary default path.
- [x] Invoke the installed absolute executable with `monitor enable`, fail with explicit repair guidance if registration fails, retain the verified binary and reports.
- [x] Document `monitor status`, `monitor check`, `monitor disable`, no-schedule integration, storage/privacy, cohort limitations and scheduler prerequisites.

Review additions: monitor-only identities retain full normalized Codex cwd and
the top-level Claude project log directory without changing legacy report
labels. Existing log files without completed usage are frozen too. Missing
timestamps reject baseline creation; disable stops collection before attempting
potentially conflicted native cleanup. Version remains 2.4.0 in the existing
unreleased PR, rather than creating a second release on top of it.

### Task 4: Verify, activate and push

- [x] Run `cargo fmt --check`, `cargo test --all-targets --locked -- --test-threads=2`, `cargo clippy --all-targets --all-features --locked -- -D warnings`, Python regression tests and release metadata checks.
- [x] Review the diff independently, address findings, repeat affected tests.
- [x] Build release binary, install locally atomically, run `monitor enable --no-schedule`; update the existing heartbeat to call `monitor check --quiet` and read the compact report. Do not add a duplicate native job.
- [x] Commit only source/docs/tests and push to `codex/install-once` using the existing SSH remote. Implementation delivered as `16a28cd`; PR14 remains unmerged, with no release tag.
