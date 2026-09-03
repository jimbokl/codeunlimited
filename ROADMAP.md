# Roadmap to production

Mission: **more code out of the subscription limits you already pay for.**
v0.1 (Python) is the validated reference implementation — the detector logic
was proven on 71k real requests before a line of product code was written.
From v0.2 the core moves to **Rust**: single static binary, sub-second scans.

## v0.2 — Rust core (speed + distribution)

- [ ] Port scanner + detectors to Rust (`serde_json` + `rayon` parallel walk):
      3.2 GB of Codex logs in seconds instead of minutes.
- [ ] Single static binary per platform — no Python required.
      Distribution: GitHub Releases, `cargo install codeunlimited`,
      Homebrew tap, Scoop bucket. (Python package remains as the
      prototyping sandbox for new detectors.)
- [ ] Golden-fixture test suite: synthetic Claude Code / Codex logs with known
      findings; CI on Linux/macOS/Windows (GitHub Actions).
- [ ] `--json` output for scripting; `--days N` window filter.
- [ ] Gemini CLI as third native source.

## v0.3 — trust & measurement (the moat)

- [ ] **Verified delta**: `init` snapshots a baseline; `audit` reports
      before/after per project — "you reclaimed N% of your weekly volume".
- [ ] Per-model limit weights (top-tier requests consume limits faster);
      rate-limit ingestion: Codex `used_percent`, Claude 5-hour blocks.
- [ ] **Opt-in anonymous benchmarks**: percentile comparisons
      ("your context tax is x3.9 vs community median x2.1").
      Accumulated data is the defensible asset; strict opt-in, counts only.
- [ ] Conservative-estimate policy documented per detector (ranges, not hype).

## v0.4 — the fix engine

- [ ] `codeunlimited fix`: generated diffs, applied only on user approval —
      CLAUDE.md tuning, `.mcp.json` pruning of unused servers, session
      hygiene suggestions.
- [ ] Claude Code skill (`/codeunlimited`) for in-agent audit + fix flow;
      listing on skills.sh.
- [ ] State-file loop scaffold (SKILL.state pattern) for long-running tasks.

## v0.5 — teams (B2B ladder)

- [ ] Org aggregation: many machines, one report.
- [ ] API-log importer + Anthropic Admin Usage API: cost map by
      feature/team/customer, unit economics per request.
- [ ] CI gate (GitHub Action): "this PR increases projected token spend by N%".
- [ ] Monthly verified-delta report — the basis for savings-based pricing.

## v1.0 — launch

- [ ] PyPI + crates.io + GitHub release, docs site.
- [ ] Launch content: "I reclaimed 52% of my Claude Code weekly limit"
      (real numbers from the author's own logs) and
      "41x is really 5x" (honest cache math behind the detectors).
- [ ] SECURITY.md, privacy statement, CHANGELOG, SemVer discipline.

## Red lines (what we will not build)

- No proxy/gateway: we never sit in the request path, never touch API keys.
- No usage-tracker features: accounting is ccusage's job — we link to it.
- No auto-transformations that can change model output quality;
  anything semantic ships as a reviewable diff.
