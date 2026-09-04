# Changelog

## 1.8.0 - 2026-09-04

- Added `experiment start|finish|record|compare|list`, a privacy-preserving
  ledger of exact observed token counters for explicit half-open time windows.
  Saved and JSON forms retain integer source totals and completed-task counts;
  comparison percentages are presentation fields.
- Experiment comparison refuses active, incomplete, empty, zero-token, or
  overlapping records. Results are labeled observational, and either arm with
  fewer than three completed tasks is called low confidence rather than causal
  evidence of savings.
- `delta` now requires 100 retained post-baseline requests per source before it
  emits any directional verdict. Smaller samples show exact metrics and an
  insufficient-sample message.
- Experiment state uses strict versioned JSON and atomic replacement. Corrupt,
  unreadable, unsupported, or symlinked state fails visibly without replacing
  original bytes; experiment output excludes prompts, responses, models,
  project paths, hostnames, and raw log records.
- The bounded one-sprint-per-arm evidence recorded 39,110,299 control input
  tokens and 47,368,983 treatment input tokens: an observed difference of
  +8,258,684 input tokens per completed task (+21.1%) and -17.4% observed
  capacity. This low-confidence historical comparison does not attribute the
  difference to codeunlimited.

## 1.7.0 - 2026-09-03

- Audit time and project filters now run inside the parsers, before retained
  usage records allocate `Request` values. `audit --json --scan-stats` exposes
  files discovered/opened/skipped and retained usage-record counts;
  `--scan-stats` requires JSON output.
- A best-effort Codex metadata index skips unchanged session files only when
  their cwd or timestamp range proves them irrelevant. `--no-index` disables
  all index reads and writes; missing, stale, invalid, unreadable, and symlinked
  cache states fall back conservatively.
- Repeated request metadata uses shared immutable strings, reducing the
  `Request` layout to 120 bytes without changing report or detector semantics.
- `scripts/benchmark_local.py` provides redacted fixture, full, bounded, and
  project-scoped measurements with wall-time and RSS summaries. Performance
  evidence and the real-work before/after outcome protocol are documented
  separately.
- Product wording now describes estimated token-leak opportunities and
  observational before/after tracking instead of causal or guaranteed savings.
- On the documented Apple M4 history, the final three-run median was 18.92 s
  for a full unindexed audit, 7.00 s for a warm indexed 30-day audit, and
  0.025 s for the all-files-skipped warm project scope. The published benchmark
  records the full and 30-day memory targets as misses rather than moving them.

## 1.6.0 - 2026-09-03

- Project-scoped commands now load global configuration first and then the
  selected project's `.codeunlimited.toml`. `fix --all` respects the global
  ignore list.
- Codex project filtering compares normalized full paths, so unrelated
  directories with the same basename no longer share a report. Only actual
  `turn_context` records can set Codex model and working-directory metadata.
- Reclaimable totals now form a conservative union of per-request detector
  claims. A request found by two detectors is counted once, and normal cache
  TTL expiry remains visible without being presented as an avoidable opportunity.
- Token arithmetic saturates safely, long-session metrics require a real tail,
  and JSON output includes `schema_version: 1` plus stable finding keys.
- Project, registry, baseline, history, report, badge, state, and skill writes
  now fail visibly and use atomic replacement where applicable. Instruction
  files and forced skill updates retain a backup; symlinked instruction targets
  are rejected.
- `doctor` fails when there are no logs to inspect. Day windows are restricted
  to 1–36,500, report output cannot alias its generated HTML file, and history
  is appended only after the report files succeed.
- The legacy Python prototype has a distinct distribution and command name.
  CI now covers the Rust 1.82 MSRV, release metadata, package contents, RustSec,
  and tag-to-artifact version consistency.

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
- Observed on the author's own project after 1 day under the rules:
  context per turn down 46%, long-session growth 10x -> 0.2x.

## 1.1.0 - 2026-09-03

- **`report`**: saved, shareable Markdown report per project
  (`CODEUNLIMITED_REPORT.md`, or `--out FILE`) - findings in limit currency,
  before/after delta vs the `init` baseline, and a trend table. Each run appends
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
- **`delta`**: observational before/after per project since the baseline
  (claude source; codex delta planned).
- **Sources**: Claude Code (`~/.claude/projects`) and Codex CLI
  (`~/.codex/sessions`) supported natively. Gemini CLI planned once real
  logs are available to validate against.
- **Privacy**: offline only; token counts, models, timestamps and project
  names - prompt and response text is not extracted, stored, or transmitted.
- Golden-fixture test suite and 3-OS CI (fmt, clippy -D warnings, tests).
