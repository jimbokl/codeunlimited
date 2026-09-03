# Security and privacy

## Runtime boundary

`codeunlimited` has no network client and makes no runtime network requests.
It reads Claude Code logs from `~/.claude/projects/**/*.jsonl` and Codex logs
from `~/.codex/sessions/**/*.jsonl`. JSON records are decoded locally, but the
tool extracts only:

- token counts and cache counters;
- model, timestamp, session, and project identifiers;
- Codex rate-limit percentages and window lengths.

Prompt text, response text, and tool arguments are not extracted, retained,
printed, or transmitted. Reports can contain project names unless
`report --anonymize` is used.

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
- `skill` writes `~/.claude/skills/codeunlimited/SKILL.md`; replacing different
  content requires `--force` and keeps a backup;
- on Windows, `schedule` creates or removes the named Task Scheduler entry.
  On other platforms it only prints the crontab line for the user to add or
  remove.
- `scripts/benchmark_local.py` writes a benchmark JSON only when `--output` is
  supplied. It contains timings, RSS, exit codes, source request counts, scan
  counters, and non-identifying platform fields; it excludes audit findings,
  token totals, models, projects, paths, prompts, and responses.

File replacements use a temporary sibling plus atomic persistence. Existing
instruction-file and skill permissions are preserved where the platform
supports them. Symlinked instruction or skill targets are rejected rather than
followed. A failed mutation returns a non-zero exit status.

The release workflows themselves use GitHub's network and artifact services;
that build-time behavior is separate from the installed CLI.

## Reporting a vulnerability

Use GitHub private vulnerability reporting for this repository. If private
reporting is unavailable, open an issue without exploit details and ask the
maintainer for a private contact channel.
