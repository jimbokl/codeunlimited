# Backlog

Only unshipped work belongs here. Completed capabilities are summarized in
[ROADMAP.md](ROADMAP.md), and the privacy red lines still apply.

## P0 — release v1.9.0

- [ ] Review and merge `codex/v1.9-trust-release` after every CI job passes.
- [ ] Tag the reviewed merge as `v1.9.0` and verify all three binaries plus
      mandatory sha256 assets.
- [ ] Run the live Unix and PowerShell installers where their native platforms
      are available; both must expose `codeunlimited techniques`.
- [ ] Publish the crate from the same commit only after package and live-install
      verification.

## P1 — measurement depth

- [ ] Per-model limit weights backed by observed provider accounting.
- [ ] Claude five-hour and weekly-window reconstruction if logs expose a
      defensible source of truth.
- [ ] Compaction analytics: measured effect of `/compact` versus a fresh
      session.
- [ ] Per-MCP-server session-start attribution.
- [ ] Delegation-adoption metric based only on model/session metadata.
- [ ] Confidence flags when a parser sees partial or internally inconsistent
      usage records.

## P1 — safer fixes

- [ ] Reviewable diffs for instruction-file tuning before application.
- [ ] Step-by-step confirmation mode and a first-class rollback command.
- [ ] Task-specific state templates for migrations, monitoring, and batch
      edits.
- [ ] Explain why each configured MCP server is a prune candidate without
      editing `.mcp.json` automatically.

## P2 — scale and distribution

- [ ] Harden the metadata index for coarse-timestamp filesystems and network
      volumes without storing log content.
- [ ] gzip/zstd log support and benchmark fixtures for multi-gigabyte scans.
- [ ] Homebrew tap, Scoop bucket, and winget manifest generated from verified
      GitHub checksums.
- [ ] Gemini CLI parser after real local logs and golden fixtures are available.
- [ ] Parser extension contract for independently maintained sources.
- [ ] Documentation site with platform-specific install and scheduling guides.
- [ ] Status-line integration that reuses already parsed rate-limit metadata.

## P3 — teams, only after user demand

- [ ] Explicit export/import of anonymized counters across machines.
- [ ] Opt-in organization aggregation and monthly verified-delta reports.
- [ ] API-log importer with documented ownership, retention, and deletion
      controls.
- [ ] Shared policy packs with organization defaults and project overrides.

## Icebox

- Proxy or gateway features, API-key handling, and general spend dashboards.
- Silent model or MCP configuration changes.
- Detectors that inspect prompt/response content.
- Third-party charting in generated reports; HTML stays self-contained.
- Auto-commit, Slack/Jira integrations, a mobile app, or hosted SaaS without a
  concrete team workflow that requires them.
