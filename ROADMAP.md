# Roadmap

Mission: **fit more useful agent work into the subscription limits people
already pay for.** The Rust CLI is the product. The Python package remains a
legacy detector prototype and has a separate distribution name.

## Shipped through 1.6

- Fast local parsing for Claude Code and Codex logs, project-scoped audit,
  bounded time windows, and versioned JSON output.
- Five explainable detectors: long-session context growth, heavy models on
  short replies, cache-prefix rewrites, fat session starts, and retry storms.
- Conservative estimates with ranges, per-request union accounting, explicit
  treatment of normal cache TTL expiry, and overflow-safe counters.
- `init`, per-source baselines, `delta`, Markdown/HTML reports, trend history,
  anonymization, badges, forecasts, comparisons, and parser diagnostics.
- Dry-run-first fixes, long-loop state scaffolds, project registry,
  project/global configuration, global ignore rules, and the Claude Code skill.
- Atomic file replacement, backups, symlink rejection, non-zero mutation
  failures, exact Codex path scope, and three-platform CI.
- Rust 1.82 MSRV, RustSec and package gates, tag/version validation, checksummed
  release artifacts, MIT license, and a distinct Python reference command.

## Next: measurement depth

- Per-model limit weights once provider behavior can be measured reliably.
- Claude five-hour/weekly-window ingestion when the local logs expose enough
  evidence to reconstruct those windows.
- Compaction analytics: detect compaction events and compare their effect with
  a fresh session.
- Per-MCP-server schema attribution for session-start context.
- Delegation-adoption metrics based only on metadata, never prompt content.
- Incremental indexing for very large histories, plus compressed-log support
  and reproducible benchmarks.

## Distribution and reach

- Publish the approved 1.6 crate and `v1.6.0` tag after review.
- Homebrew, Scoop, and winget packages driven from the checksummed GitHub
  artifacts.
- A small documentation site and verified installation guides for each
  supported platform.
- Add another agent CLI only after real fixtures establish a stable parser
  contract; Gemini remains the first candidate.

## Teams

- Explicit export/import of anonymized counters across machines.
- Opt-in organization summaries and monthly verified-delta reports.
- API-log importers and policy packs only with a documented privacy boundary
  and a user-controlled data path.

## Red lines

- No proxy or gateway, and no API-key handling.
- No general usage-tracker clone; accounting tools already cover that job.
- No silent semantic edits to MCP or model configuration.
- No detector that needs prompt or response text.
- No telemetry or upload without a separate, explicit opt-in design.

The prioritised idea pool is kept in [BACKLOG.md](BACKLOG.md).
