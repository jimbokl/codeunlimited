# Automatic local monitoring

The standard installer enables a daily usage monitor for every project found in
your local Claude Code and Codex logs. It runs the Rust binary, reads counters,
writes an aggregate report, and exits. It makes no LLM or external API calls.

Manual installations need two one-time commands:

```sh
codeunlimited setup
codeunlimited monitor enable
```

The monitor uses a macOS LaunchAgent, a Linux user systemd timer, or Windows Task
Scheduler. It requires a supported, available user scheduler; Linux containers
without user systemd need an existing scheduler instead. Registration failures
return a nonzero exit code and repair guidance, without removing the binary.
No administrator privileges are requested. A logged-out or sleeping machine
cannot be assumed to collect on time; inspect the latest check timestamp.

## Read or stop it

```sh
codeunlimited monitor status       # saved JSON, no scan; includes latest timestamp
codeunlimited monitor check        # collect now and update report.md
codeunlimited monitor disable      # remove owned job, keep reports and baseline
```

Reports live in `~/.codeunlimited/monitor/` (or `$CODEUNLIMITED_HOME/monitor/`).
Use `--state-dir /absolute/private/path` with any monitor command to select a
different directory. Provider log roots are captured at activation, so a daily
job does not depend on the shell environment used at login. Claude respects
`CLAUDE_HOME` (legacy override), then `CLAUDE_CONFIG_DIR`; Codex uses `CODEX_HOME`.

The state stores counters by project/provider/model, SHA-256 session identities,
the baseline and at most 90 daily snapshots. A repeated check on the same UTC
day replaces that day's snapshot. Prompts, responses and tool contents are not
copied into it. Local project paths can identify your work: **do not publish
the state or report without reviewing it.** Concurrent collectors cannot write
the same state; corrupt state or owned-job conflicts are errors, not reset paths.

Re-enabling preserves the original baseline. It is not an experiment restart.
Disabling the monitor does not remove efficiency defaults; use `setup --remove`
separately for those. Reports are retained for your own comparison.
If scheduler cleanup encounters a conflict, collection is still disabled and
the changed scheduler resource is preserved, with an error explaining cleanup.
Moving the executable requires disabling its previous job before re-enabling.
Windows scheduler paths containing `%` are rejected because Task Scheduler may
expand them as environment variables; use a different path or `--no-schedule`.

## What the percentage means

The frozen baseline contains log sessions first observed during the seven days
before activation. Checks use sessions first observed after activation and
within the last seven days. Any session already seen at activation is excluded
from the new-session cohort, including one that was open during installation.
Existing log files without usage yet are also excluded. Codex project matching
uses normalized full cwd paths; Claude uses the top-level project log directory,
including for nested subagents. Session identity is based on the local log file,
not a guarantee that every retained usage record is a distinct provider request.
Only the first 20 retained usage records of each session enter the comparison.
The complete observed counters remain separately available.

We match provider, model and project, then weight baseline input per record by
the current mix. A percentage appears after both matching arms have at least
100 records, each cohort has at least three sessions, and matching covers at
least 80% of current records. Positive means lower input per record; negative
means higher input. Output and cache-read counters are shown separately.

This is **an observational change, not proven savings or a subscription-quota
percentage**. Sessions may differ in length, task difficulty and interventions.
The baseline may already include codeunlimited. Logs cannot verify task quality
or completed work. Missing timestamps, known accounting errors or overflow
suppress the percentage. An empty baseline stays insufficient; reinstalling
does not manufacture a control group. These caveats also apply per project.

The collector's elapsed time is measured. CPU/disk work still costs resources;
an external LLM notification agent can also spend tokens, which are not
automatically attributed to monitoring. No quality loss is inferred from logs.

## Existing scheduler and installer opt-outs

To use an existing Codex heartbeat or another scheduler without a duplicate job:

```sh
codeunlimited monitor enable --no-schedule
# Have that scheduler invoke the installed absolute binary:
codeunlimited monitor check --quiet
```

`--quiet` suppresses success output, not errors. `status` reports the stored
scheduler choice; it does not probe the operating system's live scheduler.

`CODEUNLIMITED_SKIP_MONITOR=1` skips only monitoring in either installer.
`CODEUNLIMITED_SKIP_SETUP=1` skips both global defaults and monitoring, leaving
a binary-only installation. Set these on the shell/process running the installer.

The legacy `schedule` command remains a separate weekly diagnostic report.
Do not enable it merely to start this monitor.
