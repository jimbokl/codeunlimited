# Handoff for the Codex review agent

State of main as of 2026-09-04 (post-1.8.0). Everything below was added
after your v1.8-measurement branch merged; please verify it with the same
rigor you applied to 1.6-1.8. Release target: **v1.9.0 on Monday Sep 7**.

## What landed since 1.8.0 (review these)

1. **Technique catalog** (`src/techniques.rs`, `techniques` command)
   - 13 first-class techniques rendered into the CLAUDE.md/AGENTS.md blocks;
     per-technique toggles via `[techniques] enable/disable` in
     `.codeunlimited.toml` (project layered over global - `config.rs`).
   - Versioned block markers: `<!-- codeunlimited:v2 -->` ...
     `<!-- /codeunlimited -->`; `initcmd::upsert_block` appends, replaces a
     v2 block in place, or upgrades a legacy v1 block while preserving the
     surrounding file (unit-tested). All writes still go through `safeio`.
   - Quality policy for token-hungry next-gen models: `Risk::Medium`
     techniques must contain an explicit guardrail in their text (enforced
     by a unit test); the aggressive ones (`reasoning-effort`,
     `model-routing`) default to OFF.
   - `fix` gained: v1-block "upgrade available" finding, lean-memory size
     warning, read-only Codex `config.toml` hint (`tool_output_token_limit`).
     Nothing auto-edits configs.

2. **One-command installers** (`install.sh`, `install.ps1`)
   - Latest-release download, sha256 verification, PATH setup. Windows
     PowerShell 5.1 returns the `.sha256` asset as raw bytes - decoded
     explicitly; verify the same path on your side. Please test install.sh
     on Linux/macOS if you can.

3. **Evidence docs** (`docs/BENCHMARK.md`, `scripts/bench_context.py`,
   README charts in `docs/assets/chart-*.svg`)
   - Layer 1 is exact arithmetic over raw logs (bounded-context vs actual);
     check the math and the honesty of the labels (exact vs estimate).

## Verification checklist

- [ ] `cargo fmt --check`, `clippy --all-targets -- -D warnings`, full tests
      (83 as of dc9c936) on all three OS.
- [ ] `upsert_block` edge cases: marker without end marker, multiple blocks,
      v1 block at file start/middle/end, CRLF files.
- [ ] Technique toggles: disable of a default-on, enable of a default-off,
      unknown ids (should be inert), interaction with `fix --all`.
- [ ] Installer scripts: shellcheck install.sh; simulate missing sha256
      asset; PATH idempotency on re-run.
- [ ] README claims match `docs/ACCURACY.md` semantics (estimate vs exact).

## In flight (Claude side)

- A/B experiment running now: control = 8 coding tasks in one growing
  session; treatment = the same 8 tasks in fresh bounded sessions with the
  rules applied. Exact per-arm token accounting from agent transcripts +
  significance tests. Results will land in `docs/EXPERIMENT.md` - please
  review the statistical method when it appears.
- Weekend: dogfood snapshots; your branches merge via the usual flow
  (`codex/*` -> review -> ff/merge -> tag).

## Invariants (unchanged, enforced)

Privacy (token counts only), no network, no proxy, conservative ranges,
reversible mutations via safeio, English-only product texts, self-contained
reports. `docs/superpowers/` remains your evidence area; local-only agent
notes stay in the untracked root `HANDOFF.md`.
