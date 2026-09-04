# Handoff for the Codex review agent

> Historical handoff. The v1.9 trust-release review found and addressed the
> listed migration, installer, CI-discovery, and evidence-labeling defects.

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
   - Layer 1 sums exact observed counters and compares them with a modeled
     bounded-context counterfactual; only the observed quantity is exact.

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

## Completed after this handoff

- The short-task A/B reported rounded totals of 0.92M control and 1.08M
  treatment prompt tokens: approximately 17.4% more in treatment. The invalid
  request-level significance claim was removed; paired tasks are now the
  supported inference unit.
- The v1.9 trust review added fail-closed marker upgrades, mandatory installer
  checksums, full Python discovery, immutable evidence provenance, and
  evidence-safe wording across console, JSON, Markdown, HTML, and SVG output.

## Round 2 verdict - please verify (2026-09-04, post-v1.9.0)

The session break-even hypothesis gained useful support from a
**three-policy experiment**
(docs/EXPERIMENT.md, "Round 2"): the law `continue - restart = g*N*t0 - b`,
fitted on observed constants (boot b~24k, linear growth g~1.0k/request,
R^2~1), predicted that batching the same 8 tasks as 3+3+2 sessions lands
at 0.70-0.90M total tokens - below both prior arms. Outcome: **0.65M,
-29.9% vs the naive single session, -40.2% vs restart-per-task**, 123
tests green in the third arm. The prior "no savings on short batches" result and
the field x6.8 exposure are both special cases of the same law (N*t0
below/above b/g ~ 24). Requested verification:

- [ ] Re-derive the break-even algebra; recheck the sums against the raw
      agent transcripts referenced in EXPERIMENT.md (message-id dedup).
- [x] Confirm the prediction predates the third arm: round-1 commit `a1efd12`
      preserves the qualitative ~7-request heuristic and batching direction.
      The 3+3+2 assignment, 0.70-0.90M interval, and analysis plan are not in
      that commit and therefore are not independently pre-registered.
- [ ] If the law holds, consider a session-boundary advisor in `audit`
      (live N*t0 > b/g hint) for v2.0.

## Invariants (unchanged, enforced)

Privacy (token counts only), no network, no proxy, conservative ranges,
reversible mutations via safeio, English-only product texts, self-contained
reports. `docs/superpowers/` remains your evidence area; local-only agent
notes stay in the untracked root `HANDOFF.md`.
