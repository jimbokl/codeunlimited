# Changelog

## 1.5.0 - 2026-09-03

- **Claude Code skill**: `codeunlimited skill` installs `/codeunlimited` -
  the audit -> fix -> report flow from inside a session.
- **`schedule`**: weekly `report --all` without thinking about it
  (Windows Task Scheduler entry; cron line printed elsewhere).
- **`compare`**: last N days vs the N before, anchored at your last
  activity - verdict based on leak share of volume.
- **`.codeunlimited.toml`**: detector thresholds (long_session_turns,
  trivial_output_tokens, fat_start_tokens) and ignore_projects; project
  file first, then `~/.codeunlimited/config.toml`.

## 1.4.0 - 2026-09-03

- **Limit forecast**: Codex `used_percent` telemetry calibrates your
  window's token capacity from your own logs - the audit and `report --all`
  now answer "how many hours of work are left before the wall". Claude gets
  a busiest-week proxy ceiling until its logs expose limit telemetry.
- **Retry-storm detector**: 3+ identical-size requests within 90s bursts -
  silent auto-retries that re-pay the full context every attempt.
- **Honest ranges**: every assumption-based estimate now reports lo-hi
  bounds (documented in docs/ACCURACY.md); measured waste stays exact.
- **Rate-limit timeline**: daily `used_percent` peaks charted in the HTML
  summary and tabled in Markdown.
- **`doctor`**: log-format drift check - % of unrecognized lines per source,
  warns above 5%.
- **`fix --all`** across every registered project; `init`/`fix` now keep a
  `*.codeunlimited.bak` backup before modifying CLAUDE.md/AGENTS.md.
- **`report --badge`** (SVG badge with reclaimable %) and
  **`report --anonymize`** (hashed project names for public sharing).
- Colored terminal output (TTY only); detector unit tests; CONTRIBUTING.md
  and issue templates.

## 1.3.0 - 2026-09-03

- **Styled HTML reports**: `report` now writes a self-contained
  `CODEUNLIMITED_REPORT.html` next to the Markdown - light/dark themes,
  impact meters, delta pills, trend bars; zero external requests.
- **`report --all`**: one summary across every project `init`/`fix`/`report`
  has touched - global usage, top projects, per-project delta table, global
  trend (`~/.codeunlimited/history.jsonl`).
- Project registry at `~/.codeunlimited/projects.json` (paths only).
- Markdown polish: impact bars per finding, trend arrows.
- `docs/ACCURACY.md`: the conservative math behind every estimate,
  including the day-1 delta caveat.
- crates.io metadata; sharper positioning: set up once - up to 50% more
  work from the same limits.

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
