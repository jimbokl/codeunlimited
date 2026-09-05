# Changelog

## 2.2.0 - 2026-09-04

- Added immutable, opt-in work plans and deterministic dependency-aware packet
  selection. `run packet` previews the next packet without invoking a provider.
- Added pre-dispatch attempt intents, immutable all-attempt records, explicit
  interrupted-attempt recovery, and `run ledger` accounting. Missing or
  incomplete provider counters remain unknown.
- Added an optional soft total-token admission cap. The first attempt is
  allowed, a running attempt is never killed at the boundary, and unknown
  usage stops further admission.
- Required frozen verification for managed plans and accepted only an ordered
  prefix of the selected packet. Scope paths remain planning metadata, not an
  operating-system sandbox.
- Added a deterministic offline public-CLI fixture. It demonstrates four
  one-task worker starts versus one four-task worker start with identical final
  files; it makes no model-call, native-agent, or realized-token-savings claim.
- Extended CI and crate auditing to build the CLI before Python packet tests,
  exercise packaged packet integration tests, and exclude private task reports.

## 2.1.0 - 2026-09-04

- Made subscription CLIs the primary runtime path, with immutable instruction
  files, dynamic state channels, an additive Claude system-prompt file, and an
  ordered-read Codex bootstrap that preserves its built-in instructions.
- Added explicit `standard` (default) and `lean` subscription profiles. Lean
  disables optional integrations; it does not disable subscription login.
- Added a separate opt-in `openai-api` / `anthropic-api` layer, explicit cache
  controls, strict structured output, environment-only credentials, bounded
  HTTP responses, and disabled redirects. API adapters have no local tools.
- Normalized provider-native input/cache counters without treating missing
  usage as zero. Invalid API output retains reported usage in the failed
  attempt. Cache-read ratio is not reported as saved tokens or quota.
- Added `run cache-probe`: two opt-in no-op requests with isolated integrations,
  distinct samples, separate usage, and an explicit evidence scope. It does
  not run automatically or advance the work state.
- Rejected required-flag, instruction, continuation, and config overrides at
  initialization, including equals and attached short-option forms.
- Kept legacy manifests readable without rewriting them during inspection,
  including runs whose old prompt budget is too tight for the new compiler.
  Execution still enforces the configured budget. Runtime state/envelope
  schema remains version 1; new optional fields have compatible defaults.
- Retained Rust 1.82 support, added local API lifecycle and probe regressions,
  and corrected the API/subscription privacy boundary in the documentation.
  Local tests establish behavior, not a realized savings percentage.

## 2.0.0 - 2026-09-04

- Added `codeunlimited run init|status|prompt|step|auto|recover`, a durable
  stateful orchestration runtime that starts a fresh Claude Code, Codex, or
  explicit command process for every bounded increment.
- Compiled every orchestration prompt from an immutable workflow/objective,
  current validated state, and latest observation only. The stable prefix is
  deterministic and separately hashed and measured; prior prompts, reasoning,
  responses, tool transcripts, and provider session IDs have no next-step
  input channel.
- Added typed epistemic memory. Agents can promote bounded claims from
  hypothesis to observed or verified only with resolvable observation, check,
  or content-addressed artifact evidence. Verified claims must be disputed
  before retirement, and retired claims enter the hash-chained archive.
- Added strict revisioned deltas, hard byte/item/attempt/time/output limits,
  exclusive run locks, atomic control-state persistence, immutable attempt
  records, external verification, and explicit recovery after ambiguous
  repository mutations.
- Added provider adapters that enforce non-resumable structured execution:
  Claude print mode with session persistence disabled and dynamic system-prompt
  sections excluded, and `codex exec --ephemeral` with an output schema.
  Provider cache read/write counters are retained when the CLI reports them.
- Split the public privacy model into an offline observation plane and an
  execution plane whose configured provider process may modify project files,
  use existing authentication, and access the network.
- Local tests prove the transport and state invariants, including a two-process
  hypothesis-to-verified scenario. They do not establish realized token
  savings; a matched-quality, increasing-horizon comparison remains required.

## 1.9.0 - 2026-09-04

- Added 13 named, individually toggleable techniques and versioned v2
  instruction blocks. Context-sensitive techniques remain opt-in, and session
  guidance now weighs reuse against restart boot cost instead of demanding a
  new session for every task.
- Made instruction upgrades fail closed. Duplicate, orphaned, reversed, mixed,
  or malformed v1/v2 markers leave the target byte-identical; valid LF and CRLF
  blocks upgrade in place through the existing safe-write and backup layer.
  Marker validation also completes before the project registry is changed.
- Replaced the historical context-tax script's Windows-only path and
  positive-result filter. It now includes favorable and unfavorable sessions,
  exposes accounting completeness, keeps identifiers private, and labels the
  early-context counterfactual as modeled rather than exact savings. Invalid
  UTF-8 now aborts instead of silently dropping bytes from the accounting;
  malformed top-level JSON is counted and failure diagnostics redact paths.
- Added a strict paired-task experiment analyzer with an exact sign-flip test.
  The historical controlled run is reported as a negative total-token result
  (approximately +17.4% for treatment), and its invalid request-level
  significance claim was removed.
- Unix and PowerShell installers now require a valid sha256 asset, smoke-test
  the download, and finish fallible setup before atomically replacing an
  existing installation. Unix prints PATH guidance; PowerShell updates user
  PATH idempotently and rolls it back if the final replacement fails.
- CI explicitly discovers the complete Python suite, validates historical
  evidence against immutable release commits, tests installers on their native
  runners, and smoke-tests the `techniques` command in release artifacts.
  Python 3.10 and 3.12 now execute the identical discovered suite, and both CI
  and tagged releases audit and retest the exact unpacked crate archive.
- Public documentation and graphics now distinguish exact observed counters,
  modeled counterfactuals, detector estimates, and realized observational
  outcomes. Console, Markdown, HTML, badge, and JSON output label detector
  opportunity values as estimates. No fixed savings percentage is promised.

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
  tokens and 50,720,723 treatment input tokens: an observed difference of
  +11,610,424 input tokens per completed task (+29.7%) and -22.9% observed
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
- crates.io metadata and the initial efficiency positioning, superseded by the
  evidence-level language introduced in 1.9.

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
