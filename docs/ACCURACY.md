# How the numbers are computed (and why they are conservative)

codeunlimited reports estimated opportunities as ranges grounded in your own
logs, not marketing multipliers. This page documents every estimate the tool makes, so
you can audit the auditor.

## Limit currency

Subscription limits are opaque and provider-specific, so the tool uses a
proxy: **weekly volume** — all tokens your requests processed (uncached input,
cache reads, cache writes, output), normalized to 7 days. Real limits weigh
models and cache tiers differently; the proxy tracks direction and scale, not
the provider's exact accounting. "Extra agent replies" converts tokens into
work using your own average output size and your own context-to-output ratio.

## Detectors

**1. Context tax of long sessions.** For every session longer than 30 turns,
the average prompt size of turns past 30 is compared with the average of the
first 5 turns. Only **60% of the excess** is counted as reclaimable — some
tail context is legitimately needed; a fresh session still needs the task
brief. The multiplier shown (e.g. "x9.6") is observed in the selected logs;
the reclaimable share remains an estimate.

**2. Top-tier model on mechanical replies.** Requests to top-tier models
(Fable/Mythos/Opus classes) whose reply was shorter than 300 tokens. Only
**half** of their dragged context is counted — not every short reply is
delegable to a cheaper model.

**3. Mid-session cache re-writes.** A cache write of ≥2,000 tokens
mid-session counts as a broken prompt prefix **only if** the gap since the
previous request is inside the cache TTL (5 min / 1 h). Writes after a longer
gap are normal TTL expiry and are **not** counted — this is the mistake naive
"cache_creation = waste" heuristics make.

**4. Fat session starts.** The median cache write of each session's first
request, compared against a 25k-token baseline (system prompt + typical tool
schemas). Half of the excess is counted; the fix (pruning unused MCP servers)
is always a manual, reviewable step.

**5. Retry storms.** Three or more consecutive requests with *identical*
prompt size, each within 90 seconds of the previous one - the signature of
auto-retries or double-submits. Because size equality is a heuristic (two
different prompts can weigh the same), only **half** of the duplicated
context is counted; the honest range is 25-75%.

## Ranges

Every estimate that rests on an assumption is reported as a range:
context tax 40-80% of the measured excess (mid 60%), delegable heavy-model
context 25-75% (mid 50%), fat-start excess 25-75% (mid 50%), retry storms
25-75% (mid 50%). Mid-session cache re-writes are directly measured waste,
so no range is shown.

## Overlapping findings

One request can match more than one detector. For example, a large first
request may be both a fat session start and a top-tier request with a short
reply. Individual finding rows keep their own estimates, but the headline
total uses the largest midpoint claim for each request. It does not add those
overlapping claims together. This conservative union is also used by JSON,
Markdown/HTML reports, comparisons, and history snapshots.

## Limit forecast

- **Codex**: the logs record `used_percent` of the rate-limit window, which
  lets the window's token capacity be calibrated from your own data:
  `capacity ~ tokens_processed_in_window / used_percent` at the
  highest-signal observation (>=20% used). "Hours to the wall" divides the
  remaining budget by your last-24h pace. Both are estimates: the provider
  weighs models and cache tiers in ways the logs don't expose.
- **Claude Code**: logs carry no limit telemetry, so the busiest observed
  week serves as a proxy ceiling - useful for direction, not for the exact
  hour. The report says which of the two methods produced each line.

## Delta and trend

`init` freezes a per-project baseline; `delta`/`report` recompute the same
metrics on activity after it. **Caveat:** right after the baseline, sessions
are young by definition, so "context per turn" dips mechanically. Trust the
**trend across a week or more** (one snapshot per `report` run), not a
single day-1 delta. The tool never extrapolates a day into a week for you.
`delta` still prints the exact retained metrics below 100 post-baseline
requests per source, but labels the sample insufficient and emits no improving,
worsening, savings, or capacity verdict until that fixed threshold is reached.

These are observational comparisons. A change after `init` may also reflect a
different task mix, model, operator, or provider behavior. Use the outcome
protocol in [BENCHMARKING.md](BENCHMARKING.md) when evaluating whether the
utility helped real work.

## Exact bounded experiment counters

`experiment start`/`finish` and historical `experiment record` sum recognized
integer counters whose request timestamps fall in the explicit half-open
interval `started_unix <= timestamp < finished_unix`. The persisted numerator
keeps uncached input, cache reads, 5-minute and 1-hour cache writes, output,
request count, and composite `(source, project, session)` session count.
Arithmetic saturates at the integer limit rather than wrapping.

Historical RFC 3339 boundaries must use whole-second precision; fractional
seconds are rejected rather than truncated. A missing Claude or Codex source
root contributes zero records. Once a source root exists, traversal, open, or
read failures abort the measurement before experiment state is changed, so an
I/O failure cannot be mistaken for complete zero usage.

A recognized project request without a usable timestamp cannot be assigned to
either side of a boundary. Its count is retained, the record is marked
incomplete, and comparison is refused. Active, empty, zero-token, overlapping,
and zero-task records are also not comparable. Exact here describes the sum of
the counters present in recognized local records, not provider billing weights
or a causal estimate.

Comparison embeds both exact records and their task denominators. Per-task
decimals and percentages are rounded presentation views; the integer totals
remain the reproducible source. Results always say `causality: observational`
and are low confidence when either arm declares fewer than three completed
tasks. Differences in task mix, difficulty, models, tools, operator behavior,
and provider accounting can explain an observed movement.

The dated 1.7/1.8 sprint artifact records 39,110,299 control input tokens and
38,263,622 treatment input tokens for one completed task in each arm. The exact
observed difference is -846,677 input tokens per task (-2.2%), with a
+2.2% observed capacity view. Because each arm contains one historical
sprint with uncontrolled differences in scope and difficulty, this is low
confidence and does not show attributable savings.

## Query filtering and the Codex metadata index

`audit --days N` discards records with an old or unrecognized timestamp while
parsing, before allocating a request. This matches the prior report-level time
filter. Codex project scope uses normalized cwd values from `turn_context` and
never infers project identity from a session filename.

By default, audit may maintain `codex-index-v1.json` under
`CODEUNLIMITED_HOME` (normally `~/.codeunlimited`). For each canonical JSONL
path it stores only file length, modification time in nanoseconds, normalized
cwd keys, and the minimum/maximum recognized timestamp. It does not store
models, token events, counts, prompts, or responses. Paths and cwd keys can
still identify projects.

Only an unchanged fingerprint can be skipped. Project mismatch takes
precedence over an entirely-old timestamp range in scan counters. Unknown
timestamps, unknown layouts, changed files, unreadable fingerprints, missing
indexes, and invalid regular-file indexes are handled conservatively by
opening or rebuilding rather than excluding data. A symlinked/non-regular or
unreadable index disables caching. `--no-index` performs no index read or
write. Indexed and unindexed reports are required to match after removing the
optional `scan` diagnostics object.

## Evidence levels

Scanner performance is reported as wall time, RSS, request counts, and scan
counters from the redacted local harness. It demonstrates runtime behavior on
the named machine and history only. Product benefit is a separate before/after
observation using completed tasks per million input tokens plus the guardrails
defined in [BENCHMARKING.md](BENCHMARKING.md); it is not inferred from scanner
speed or from the reclaim estimate alone.

## What the tool never does

- Never extracts, retains, prints, or transmits prompt/response text. The JSONL
  records are decoded locally so the named token and metadata fields can be
  selected.
- Never touches the network. No telemetry, no uploads.
- Never auto-edits configs that could change agent behavior beyond the
  documented efficiency rules; those changes are printed as suggestions.

If you find a case where a number is misleading, that's a bug — please open
an issue.
