# Whole-machine local benefit monitor

The user requested automatic monitoring for the whole product and a GitHub push.
Ship deterministic, offline monitoring of all locally discoverable Claude/Codex
projects, enabled by the installers after global efficiency setup. No provider
calls, background LLM, uploads, permission changes or automatic task restarts.

`monitor enable [--no-schedule]`, `check [--quiet]`, `status`, and `disable`
share an optional absolute `--state-dir`. Enable freezes a seven-day baseline
and the identities of already observed sessions. Re-enabling never resets it.
Check scans local logs and retains at most 90 daily aggregate snapshots. New
sessions exclude previously observed identities and records before activation.
Compare only matching source/model/project strata, with explicit coverage and
minimum 100 usage records / 3 sessions per arm. A percent change is observational,
not causal savings or subscription quota; task quality is unknown. Empty,
incomplete or overflowed accounting must not become a positive savings claim.
Keep prompts, tool bodies and raw transcripts out of state/reports and git.

Native scheduling uses macOS LaunchAgent, Linux user systemd timer, or Windows
Task Scheduler. Run daily, as the current user, with absolute executable and
state paths, no shell interpolation. Own only distinctly named resources; reject
conflicts, offer removal, retain reports. Unsupported scheduler errors are
actionable, not a false success. `--no-schedule` supports an existing scheduler.
The current user's existing Codex heartbeat will invoke this collector; do not
install a duplicate native task on this machine.

Rust 1.82, existing dependencies only. All scheduler tests use temporary files
and mock command execution; installer tests must never register real OS jobs.
Extend the existing install-once branch/PR, without merging or release tagging.
