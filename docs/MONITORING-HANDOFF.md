# 2.4.0 monitoring handoff

The existing unreleased install-once branch now includes automatic offline
monitoring. It is a deterministic Rust collector, not an LLM experiment.

## Implementation

- `src/monitor.rs`: immutable seven-day baseline, existing-file census, first-20
  record cohorts, matched observational changes, atomic locked state, 90 daily
  snapshots and a Markdown report.
- `src/parsers.rs`: monitor-only full Codex project identities and top-level
  Claude project grouping. Existing audit/report label behavior is unchanged.
- `src/monitor_schedule.rs`: daily current-user jobs for LaunchAgent, systemd
  and Task Scheduler. Ownership conflicts fail closed. A precommitted token
  and exact task validation recover interrupted Windows registration safely.
- Both installers invoke `monitor enable` after successful setup; monitor-only
  and full activation opt-outs are tested. `disable` stops collection even when
  scheduler cleanup requires manual attention, without deleting reports.

## Verification on 2026-09-06

- 318 Rust tests pass, including 17 public monitor CLI tests and 21 isolated
  scheduler tests; all 84 Python tests pass.
- Rustfmt, all-target/all-feature Clippy, Rust 1.82 all-target check, release
  metadata checks, release build and package verification pass.
- The extracted package passes all-target tests using a fresh Cargo target
  directory. Do not reuse an old extracted-package target blindly: cached test
  executables can contain paths to an already deleted fixture extraction.
- Independent review confirmed fixes for same-basename project collisions,
  pre-existing sessions without usage and interrupted Windows registration.
- The release collector was exercised locally through an existing external
  scheduler configuration. Native OS job registration was not performed on
  the development host; native adapter tests inject scheduler responses.

Before a release tag, verify the GitHub platform matrix and smoke-test actual
registration/removal on the target OS. Linux needs an available user systemd
manager; Windows uses the current interactive user; macOS uses the GUI domain.

## Claims and privacy

The reported percentage is a matched input-per-record change, not causal
savings, API cost or subscription quota. Task quality remains unknown. Empty,
incomplete, overflowing and insufficiently matched samples suppress it.
Provider log roots and project identities stay in local state; no raw logs,
private reports or user-specific monitoring data are included in this commit.
See [operation and measurement details](MONITORING.md).
