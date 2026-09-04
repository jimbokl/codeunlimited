# Security and privacy

## Observation plane

The audit, delta, report, forecast, doctor, and experiment-accounting commands
make no network requests. This observation plane
reads Claude Code logs from `~/.claude/projects/**/*.jsonl` and Codex logs from
`~/.codex/sessions/**/*.jsonl`. JSON records are decoded locally, but the tool
extracts only:

- token counts and cache counters;
- model, timestamp, session, and project identifiers;
- Codex rate-limit percentages and window lengths.

Prompt text, response text, and tool arguments are not extracted, retained,
printed, or transmitted. Reports can contain project names unless
`report --anonymize` is used.

## Execution plane and provider process

`run init`, `run status`, and `run prompt` operate on local runtime control
files without invoking a model. `run step`, `run auto`, and the opt-in
`run cache-probe` form a separate execution plane: they invoke the configured
subscription process or external API transport. Probes consume usage too.

The provider process is not inside the observation plane's privacy boundary.
It inherits the user's environment and provider configuration, can read or
modify project files, can invoke tools, and may make network requests using its
existing authentication. The built-in Claude and Codex adapters start a fresh
non-resumable process for every orchestration step and require structured
output; they do not make the provider itself offline or sandbox it.

Provider arguments are stored in the run manifest. Common secret-flag values
are redacted from `run status`, and known token/password arguments are rejected
at initialization, but secrets should never be placed on a command line. The
custom `command` adapter executes the exact program and argv supplied by the
user without a shell; its behavior and network access belong to that program.

The optional API adapters send the workflow, objective, current state, and
latest observation to the configured endpoint. They expose no local coding
tools. API key values are read from environment variables at invocation and
never stored in manifests, printed, or passed in argv. Remote endpoints require
HTTPS; loopback HTTP is allowed for tests. HTTP redirects are disabled, response
bodies are capped, and errors omit remote bodies and credentials.

Cache probes disable integrations regardless of the normal run profile. Claude
uses no built-in tools/MCP/Chrome/slash commands/hooks; Codex ignores user
configuration and uses a read-only filesystem sandbox. Those are provider
controls, not an OS sandbox for an arbitrary replacement executable. Probe
results report any provider cache read, not attribution to our stable file.

## Files and system state it can change

Commands write only when their documented behavior requires it:

- `init` and `fix --apply` can create or update `CLAUDE.md` and `AGENTS.md`,
  preserving the first changed version as `*.codeunlimited.bak`;
- `init` writes `.codeunlimited.baseline.json`;
- `fix --apply` can create `state/state.json` for projects with long sessions;
- `report` writes a Markdown file, a sibling HTML file, optional
  `CODEUNLIMITED_BADGE.svg`, and a local history JSONL file;
- `init`, `fix`, and `report` maintain the path-only project registry and lock
  files under `~/.codeunlimited/` (or `CODEUNLIMITED_HOME`);
- `audit` may maintain `codex-index-v1.json` in that directory. The index
  contains JSONL file paths, normalized cwd keys, file size/modification
  fingerprints, and timestamp ranges. These paths can identify projects, but
  the index contains no prompts, responses, models, token events, or counts.
  `audit --no-index` neither reads nor writes it;
- `report --all` maintains `~/.codeunlimited/history.jsonl`;
- `experiment start`, `finish`, and `record` serialize mutations with
  `.codeunlimited.experiments.lock` and atomically maintain
  `.codeunlimited.experiments.json` in the selected project. The JSON contains only
  validated experiment names, Unix boundaries, task counts, completeness
  counts, and aggregate token/request/session counters; it contains no models,
  paths, prompts, responses, findings, hostnames, or raw log records;
- `skill` writes `~/.claude/skills/codeunlimited/SKILL.md`; replacing different
  content requires `--force` and keeps a backup;
- on Windows, `schedule` creates or removes the named Task Scheduler entry.
  On other platforms it only prints the crontab line for the user to add or
  remove.
- `scripts/benchmark_local.py` writes a benchmark JSON only when `--output` is
  supplied. It contains timings, RSS, exit codes, source request counts, scan
  counters, and non-identifying platform fields; it excludes audit findings,
  token totals, models, projects, paths, prompts, and responses.
- `run init` creates `.codeunlimited/runs/<name>/` with a manifest, immutable
  workflow and provider-instruction snapshots, bounded state and latest observation, exclusive lock,
  immutable attempt records, compacted state archives, and an optional recovery
  record. `run step`, `run auto`, and `run recover` update only that run's
  control files, apart from changes made independently by the provider process
  inside the selected project. The recommended ignore rule is
  `.codeunlimited/runs/`.

File replacements use a temporary sibling plus atomic persistence. The rename
is the commit point; durability sync after that point is best effort so a
committed mutation is never reported as though no state changed. Existing
instruction-file and skill permissions are preserved where the platform
supports them. Symlinked instruction or skill targets are rejected rather than
followed. A failed mutation returns a non-zero exit status.

The release workflows themselves use GitHub's network and artifact services;
that build-time behavior is separate from the installed CLI.

## Reporting a vulnerability

Use GitHub private vulnerability reporting for this repository. If private
reporting is unavailable, open an issue without exploit details and ask the
maintainer for a private contact channel.
