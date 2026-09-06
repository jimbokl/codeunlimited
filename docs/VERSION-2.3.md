# Version 2.3.1: retrospective accounting corrections

2.3.0 introduced `codeunlimited verdict`. 2.3.1 corrects its accounting and
reporting; it does not introduce a new optimization algorithm or evidence of
realized token savings.

## Observed and modeled

For sessions with more than `--min-turns` retained usage records (default 30),
the model holds context at the mean of the first `--early-turns` records
(default 5). It compares that counterfactual with observed prompt counters.
The signed difference is **observed minus modeled**: a negative value means
the modeled scenario uses more tokens. All session directions are retained.

The formula follows [BENCHMARK.md](BENCHMARK.md) Layer 1. Differential tests
compare the Rust CLI with `scripts/bench_context.py` on valid Claude fixtures
with unambiguous timestamps. Their parser coverage is not identical: the Rust
CLI also reads Codex, validates required counters and cache splits, and
withholds models for incomplete input. The session floor is a filter, not a
universal measured break-even point.

This model does not know which context was necessary. It does not account for
work repeated after a restart, quality loss, or subscription-limit rules.
Matched-quality tasks with all attempts counted remain the evidence needed
for an actual savings claim.

## Usage-record accounting

Codex may emit rate-limit updates that repeat previously reported usage.
The parser removes a repeat only when both the cumulative and last-usage
counter snapshots equal the previous recognized snapshot in the same file.
This comparison precedes date and project filters. Advancing counters,
observed decreases, and equal-sized calls without cumulative counters are
retained. Rate-limit observations are retained independently of usage dedup.

This is conservative record accounting, not certified reconstruction of
distinct model calls. Copies in different files are not deduplicated. A reset
that is not observable in the recorded counters cannot be reconstructed.
Claude message IDs are deduplicated within their project/session identity.

Both parsers require nonnegative integer input/output counters. Optional cache
fields may be absent, but supplied values must be valid and consistent.
Malformed candidate JSON, invalid counters, discovery failures, and unreadable
files are exposed in scan diagnostics. Completeness refers to recognized log
formats and counters, not proof that a provider logged every billable event.
Diagnostics can cover files inspected while determining the requested scope.

## CLI and JSON contract

`verdict` never invokes a model, modifies logs, or reads/writes the optional
Codex metadata index. Missing provider roots mean empty history; invalid
project directories and `--early-turns 0` are argument errors.

JSON schema **2** uses the same keys for successful, empty, and incomplete
scans:

- `status`: `ok`, `no_eligible_sessions`, or `incomplete`.
- `complete_accounting`, `scan`, `warnings`, `records_without_timestamp`, and
  `overflow` disclose integrity limitations. Incomplete scans exit 2.
- Observed fields retain recognized subtotals on scan errors. Overflow makes
  observed totals null rather than presenting saturated values as exact.
- Model fields are null for incomplete scans or no eligible sessions.
- `modeled_difference_tokens` is signed and is not rounded to an integer.
  `modeled_savings_tokens` remains a deprecated alias with the same signed
  value, not the clipped nonnegative integer returned by 2.3.0.
- `requests_total`, `requests_included`, and
  `extra_requests_at_observed_average` retain their legacy names. Their unit
  is retained usage records; the last field is a signed model equivalence,
  not extra usable requests or quota.

Consumers must check `schema_version` and `complete_accounting` before using
metrics. Invalid arguments print an error to stderr and exit 2 before scanning.

`audit --json` adds an `accounting` object even without `--scan-stats`; scan
errors suppress opportunity estimates and emit a stderr warning. Audit remains
a diagnostic command with exit 0 for partial scans. `init` refuses an incomplete
baseline; instruction files may already have been installed when it reports
the baseline error. Existing baselines are not rewritten. Strict experiment
scans reject malformed usage and do not commit a partial measurement.

## Verification

Public-CLI regressions cover signed negatives, counter validation, repeated
Codex snapshots, equal-sized calls, resets, missing cumulative counters,
cross-window deduplication, Claude identity scope, corrupt files, read-only
behavior, empty schema parity, overflow, invalid arguments, and init/audit
integration. A Python differential test covers growing, flat, shrinking, and
mixed histories. These tests establish behavior, not a savings percentage.
