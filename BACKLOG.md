# Backlog

Everything worth building, by priority. Sprints pull from here; ROADMAP.md
tracks what's committed. Red lines still apply (no proxy, no tracker
features, no silent semantic transforms).

## P0 — launch week

- [ ] crates.io publish (blocked: account email verification).
- [ ] Post Show HN + r/ClaudeAI + X thread (texts in docs/launch/).
- [ ] README screenshot/GIF of the HTML report + terminal audit (the
      "install once" visual).
- [ ] Social preview image for the GitHub repo.
- [ ] Token hygiene: regenerate the crates.io token after launch.
- [ ] Issue templates + "good first issue" labels for incoming traffic.

## P1 — sprint v1.4: measurement depth (trust = moat)

- [ ] **Per-model limit weights**: top-tier tokens burn the weekly window
      faster than mid-tier; weigh reclaim numbers accordingly.
- [ ] **Claude 5h-block ingestion**: reconstruct block/weekly windows from
      log timestamps; show "you hit the wall N times this month, first
      leak to fix to stop that".
- [ ] **Codex rate-limit timeline**: used_percent over time as a chart in
      the HTML report (the "I hit 100%" graph everyone screenshots).
- [ ] **Compaction analytics**: detect /compact and context-summarization
      events, measure what they saved vs a fresh session.
- [ ] **Delegation adoption metric**: share of light-model subagent replies
      over time - proves the delegation rule is being followed.
- [ ] **Limit forecast**: "at the current pace you hit the weekly wall in
      ~N hours; fixing leak #1 moves that to ~M" - turns the audit into a
      countdown people act on.
- [ ] Retry-storm detector: bursts of same-size requests in a short window
      (token-count heuristics only - prompts are never read).
- [ ] Ranges instead of point estimates in reports (min-max per detector).
- [ ] `codeunlimited doctor`: sanity-check parsers against the local log
      formats and report anything unrecognized (early warning for format
      drift in Claude Code / Codex updates).
- [ ] Per-detector unit tests on synthetic logs (beyond the golden e2e).

## P1 — sprint v1.5: fix engine v2

- [ ] `fix` generates reviewable diffs for CLAUDE.md tuning (tighten noisy
      sections, add missing delegation/state rules) - apply on approval.
- [ ] Per-MCP-server cost attribution: estimate each configured server's
      schema weight from session-start writes; name the expensive ones.
- [ ] `fix --all`: run across every registered project in one pass.
- [ ] `codeunlimited schedule`: install a weekly `report --all` task
      (Task Scheduler on Windows, cron/launchd elsewhere).
- [ ] State-file loop templates per task type (migration, monitoring,
      batch-edit) instead of one generic scaffold.
- [ ] Trust features for `--apply`: step-by-step confirmation mode,
      `--backup` before writes, `codeunlimited rollback` to undo the last
      applied change set.
- [ ] `.codeunlimited.toml` config: thresholds, ignored projects,
      per-project overrides.

## P2 — reach

- [ ] **Claude Code skill** `/codeunlimited` (audit + fix from inside a
      session) + listing on skills.sh.
- [ ] Statusline integration: estimated % of weekly window used, live.
- [ ] `report --badge`: SVG badge "50% limit reclaimed" for READMEs -
      every badge is an inbound link.
- [ ] GitHub Action: PR comment "this change adds ~N tokens per session
      start" (CLAUDE.md/MCP config diffs).
- [ ] Gemini CLI as third source (needs real local logs to validate).
- [ ] Other agent CLIs when formats are verifiable: OpenCode, Cursor CLI.
- [ ] Homebrew tap, Scoop bucket, winget manifest.
- [ ] Docs site (GitHub Pages from docs/) + user guide per scenario;
      CONTRIBUTING.md + public roadmap on GitHub Projects.
- [ ] Multi-machine merge: `export`/`import` of anonymized counters.
- [ ] `--anonymize`: hash project names in reports so they can be shared
      publicly (also the on-ramp for opt-in benchmarks).
- [ ] `codeunlimited compare`: two periods or two branches side by side.
- [ ] Interactive mode: after `audit`, pick a finding and jump straight
      into `fix` for it.
- [ ] Console polish: color output, progress while scanning huge logs.
- [ ] Parser plugin interface so the community can add sources without
      forking the core.
- [ ] Big-log scaling: aggregate cache (skip unchanged files by mtime),
      gzip/zstd log support, criterion benchmarks in CI.

## P2 — the moat (needs users first)

- [ ] **Opt-in anonymous benchmarks backend**: percentile comparisons
      ("your context tax is x3.9 vs median x2.1"). Counts only, strict
      opt-in. Accumulated data is the defensible asset.
- [ ] Public aggregate stats page ("state of agent token efficiency") -
      recurring content engine from the same data.

## P3 — B2B ladder (v0.5+)

- [ ] Org aggregation: many machines, one report.
- [ ] Anthropic Admin Usage API + API-log importer: cost map by
      team/feature/customer, unit economics per request.
- [ ] CI gate: "this PR increases projected token spend by N%".
- [ ] Monthly verified-delta report - the basis for savings-based pricing.
- [ ] Team policy packs: shared CLAUDE.md efficiency rules with
      org-level defaults.

## Icebox (decided against, revisit only with strong signal)

- Proxy/gateway anything (red line).
- Usage-tracker features - dashboards of spend per day (ccusage's job).
- Auto-editing MCP configs or anything that changes model behavior
  without a reviewable diff.
- RU localization of the product (EN-only; RU is a marketing channel,
  not a product surface).
- Detectors that require reading prompt/response content (duplicate-prompt
  similarity, "unused tool results") - privacy red line; token-count
  heuristics only.
- Chart.js/Plotly in HTML reports - reports stay self-contained with zero
  external requests; inline CSS/SVG only.
- Auto-commit of applied fixes; Slack/Jira/Prometheus integrations;
  mobile app; cloud SaaS - until the B2B ladder demands them.
