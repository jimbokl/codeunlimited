# Changelog

## 1.2.0 - 2026-09-03

- **`fix`**: turns audit findings into concrete project changes - efficiency
  rules block (runs `init`), `state/state.json` scaffold for long loops
  (SKILL.state pattern), MCP prune candidates for fat session starts
  (listed only - config is never auto-edited). Dry-run by default, `--apply`
  to write.
- **Codex delta**: baselines now capture both sources; `delta` and `report`
  show a per-source before/after. Old single-source baselines stay readable.
- Measured on the author's own project after 1 day under the rules:
  context per turn down 46%, long-session growth 10x -> 0.2x.

## 1.1.0 - 2026-09-03

- **`report`**: saved, shareable Markdown report per project
  (`CODEUNLIMITED_REPORT.md`, or `--out FILE`) - findings in limit currency,
  verified delta vs the `init` baseline, and a trend table. Each run appends
  a snapshot to `.codeunlimited.history.jsonl`, so the trend grows over time.

## 1.0.0 - 2026-09-03

First stable release.

- **Rust core**: audits gigabytes of local logs in ~2 seconds; single binary,
  no runtime dependencies. Python reference implementation kept in
  `codeunlimited/` as the detector prototyping sandbox.
- **`audit`**: four detectors reported in limit currency (% of weekly volume,
  extra agent replies) - context tax of long sessions, top-tier model on
  mechanical replies, mid-session cache re-writes, fat session starts.
  `--project` scope, `--days N` window, `--json` for scripting,
  Codex rate-limit peak surfaced in the text report.
- **`init`**: both adoption cases first-class - brand-new project and
  attach-to-existing (instant per-project baseline). Writes idempotent
  token-efficiency rules into CLAUDE.md/AGENTS.md and captures a baseline.
- **`delta`**: verified before/after per project - proves reclaimed work
  since the baseline (claude source; codex delta planned).
- **Sources**: Claude Code (`~/.claude/projects`) and Codex CLI
  (`~/.codex/sessions`) supported natively. Gemini CLI planned once real
  logs are available to validate against.
- **Privacy**: offline only; token counts, models, timestamps and project
  names - prompts and responses are never parsed, stored, or transmitted.
- Golden-fixture test suite and 3-OS CI (fmt, clippy -D warnings, tests).
