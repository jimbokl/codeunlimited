# Roadmap to production

Committed work only; the full idea pool lives in [BACKLOG.md](BACKLOG.md).

Mission: **more code out of the subscription limits you already pay for.**
v0.1 (Python) is the validated reference implementation — the detector logic
was proven on 71k real requests before a line of product code was written.
From v0.2 the core moves to **Rust**: single static binary, sub-second scans.

## v0.2 — Rust core (speed + distribution)

- [x] Port scanner + detectors to Rust (`serde_json` + `rayon` parallel walk):
      **3.6 GB of logs in 2.4 s** (vs ~2 min for the Python reference) —
      audit, scoped audit, init with per-project baseline, both adoption cases.
- [x] Single binary, no Python required (`cargo build --release`).
- [ ] Prebuilt binaries: GitHub Releases, `cargo install codeunlimited`,
      Homebrew tap, Scoop bucket. (Python package remains as the
      prototyping sandbox for new detectors.)
- [x] Golden-fixture test suite: synthetic Claude Code / Codex logs with known
      findings; CI workflow for Linux/macOS/Windows (fmt + clippy -D warnings
      + tests) ready to activate on first push.
- [x] `--json` output for scripting; `--days N` window filter.
- [ ] Gemini CLI as third native source (needs real local logs to validate
      the format against - accuracy over speed here).

## v0.3 — trust & measurement (the moat)

- [x] **Verified delta**: `init` captures `.codeunlimited.baseline.json`;
      `delta` reports before/after per project (claude source; codex planned).
- [x] Rate-limit ingestion: Codex `used_percent` peak surfaced in the report.
- [x] Saved Markdown reports with trend history (`codeunlimited report`):
      findings + verified delta + one snapshot row per run.
- [ ] Per-model limit weights; Claude 5-hour block ingestion.
- [ ] **Opt-in anonymous benchmarks** (post-1.0 — needs a backend):
      percentile comparisons ("your context tax is x3.9 vs median x2.1").
      Accumulated data is the defensible asset; strict opt-in, counts only.
- [x] Conservative-estimate policy documented per detector (docs/ACCURACY.md).

## Sprint v1.2 (2026-09) — fix engine + measured savings

- [x] `codeunlimited fix [--apply]`: turns audit findings into concrete project
      changes — efficiency-rules block, state-file scaffold for long loops,
      MCP prune candidates (listed, never auto-edited). Dry-run by default.
- [x] Codex delta: baselines, `delta` and `report` become per-source
      (claude + codex), old single-source baselines still readable.
- [x] Apply to the top-volume local projects and measure the savings with
      the `report` trend: ~10.4B tokens of history now under the rules;
      measured after day 1 on the author's project - context per turn
      down 46%, long-session growth 10x -> 0.2x.

## Sprint v1.3 (2026-09) — launch prep

- [x] Styled self-contained HTML reports (light/dark) next to Markdown.
- [x] `report --all`: cross-project summary, per-project delta table,
      global trend; project registry at `~/.codeunlimited/`.
- [x] docs/ACCURACY.md — the conservative math behind every estimate.
- [x] crates.io metadata; GitHub storefront (description, topics);
      positioning: set up once - up to 50% more work from the same limits.
- [x] Launch drafts: Show HN + "41x is really 5x" (docs/launch/).
- [ ] Dogfood week: `report --all` every 1-2 days, fill [TREND] numbers.
- [ ] Launch day (owner decisions: cargo token, date): public flip +
      crates.io publish + posts, all at once.

## Sprint v1.4 (2026-09) — measurement depth [SHIPPED]

- [x] Limit forecast (Codex capacity calibration + Claude proxy ceiling).
- [x] Retry-storm detector (token-count heuristics only).
- [x] Honest lo-hi ranges on every assumption-based estimate.
- [x] Rate-limit timeline (daily peaks) in HTML/MD summaries.
- [x] `doctor` - log-format drift early warning.
- [x] `fix --all`, backups before writes, `report --badge`, `--anonymize`.
- [x] Colored TTY output, detector unit tests, CONTRIBUTING + templates,
      README terminal mockup.

## Sprint v1.5 (2026-09) — rituals & reach [SHIPPED]

- [x] Claude Code skill (`codeunlimited skill` -> `/codeunlimited`).
- [x] `schedule` - weekly report --all (Task Scheduler / cron line).
- [x] `compare` - period vs previous, leak-share verdict.
- [x] `.codeunlimited.toml` - thresholds + ignore_projects.

## v0.4 — the fix engine

- [ ] `codeunlimited fix`: generated diffs, applied only on user approval —
      CLAUDE.md tuning, `.mcp.json` pruning of unused servers, session
      hygiene suggestions.
- [ ] Claude Code skill (`/codeunlimited`) for in-agent audit + fix flow;
      listing on skills.sh.
- [x] State-file loop scaffold (SKILL.state pattern) for long-running tasks
      (`fix --apply` creates `state/state.json` where long sessions are seen).

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
