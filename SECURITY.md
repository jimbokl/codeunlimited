# Security & Privacy

## What this tool reads

`codeunlimited` runs entirely offline on your machine. It reads local session
logs of Claude Code (`~/.claude/projects/**/*.jsonl`) and Codex CLI
(`~/.codex/sessions/**/*.jsonl`) and extracts **only**:

- token counts (input / cached / cache-write / output),
- model identifiers,
- timestamps, session identifiers and project directory names,
- rate-limit percentages (Codex).

Prompts, responses, file contents and tool arguments are **never parsed,
stored, or transmitted**. The tool makes no network requests.

## What it writes

- `CLAUDE.md` / `AGENTS.md` efficiency blocks in a project you `init`
  (idempotent, marker-guarded);
- `.codeunlimited.baseline.json` in that project (aggregate token metrics
  only - safe to commit, safe to delete).

## Reporting a vulnerability

Open a GitHub issue with the `security` label, or use GitHub private
vulnerability reporting on this repository.
