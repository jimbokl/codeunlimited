# Roadmap

Mission: **fit more useful agent work into the subscription limits people
already pay for.** The Rust CLI is the product. The Python package remains a
legacy detector prototype and has a separate distribution name.

## Sprint v2.0 (Sep 4, 2026) — stateful orchestration

- [x] Durable `P + state + observation` runtime with no prior-transcript input
      channel between orchestration steps.
- [x] Fresh-process Claude, Codex, and custom command adapters with strict
      structured output and provider usage-counter capture.
- [x] Bounded epistemic memory with evidence-gated promotion, monotonic status,
      dispute-before-retire, and hash-chained archives.
- [x] Revisioned state deltas, verification-gated completion, hard resource
      budgets, exclusive locks, immutable attempts, and explicit recovery.
- [x] Observation/execution-plane security boundary and evidence-safe product
      claims.
- [ ] Run a paid, pre-registered, matched-quality increasing-horizon benchmark
      against full-history orchestration.

## Sprint v1.9 (Sep 4, 2026) — trust release

- [x] Thirteen named, toggleable techniques with opt-in quality-sensitive
      settings and context-aware session boundaries.
- [x] Fail-closed v1/v2 instruction upgrades with duplicate, malformed, and
      CRLF coverage.
- [x] Portable, unbiased context-tax model with visible incomplete accounting.
- [x] Paired-task experiment analysis and publication of negative total-token
      results without request-level pseudoreplication.
- [x] Checksum-mandatory staged installers and native-runner tests.
- [x] Complete Python discovery, immutable evidence provenance, and release
      artifact smoke tests.
- [ ] Tag v1.9.0, wait for all release jobs, and verify both live installers.

## Shipped through 2.0

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
- Query-aware parsing, metadata-only Codex indexing, deterministic bounded
  parallel aggregation, compact shared request metadata, scan diagnostics, and
  redacted reproducible local benchmarks for very large histories.
- Rust 1.82 MSRV, RustSec and package gates, tag/version validation, checksummed
  release artifacts, Python 3.10 compatibility, MIT license, and a distinct
  Python reference command.
- Strict offline experiment ledgers with exact half-open-window counters,
  completed-task normalization, observational comparisons, low-confidence
  disclosure, and a 100-request minimum for directional `delta` verdicts.
- Evidence-safe benchmark vocabulary, paired-task inference, fail-closed
  instruction migrations, and verified installers that preserve the prior
  binary on failure.
- Bounded stateful execution, deterministic cache-friendly prompt prefixes,
  strict epistemic retention, and per-attempt transport/cache telemetry.

## Next: measurement depth

- Compare full-history and 2.0 stateful arms at matched task quality over
  increasing horizons, publishing exact prompt bytes and provider counters.
- Separate the contribution of transcript exclusion, epistemic state, stable
  prefix caching, and session boot cost without double-counting overlap.
- Add domain-specific state profiles only after ablations show which fields are
  necessary sufficient statistics for each class of coding task.
- Per-model limit weights once provider behavior can be measured reliably.
- Claude five-hour/weekly-window ingestion when the local logs expose enough
  evidence to reconstruct those windows.
- Compaction analytics: detect compaction events and compare their effect with
  a fresh session.
- Per-MCP-server schema attribution for session-start context.
- Delegation-adoption metrics based only on metadata, never prompt content.
- Compressed-log support and fixtures that exercise multi-gigabyte scans.

## Distribution and reach

- Publish and independently verify v2.0.0 from its reviewed commit.
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
