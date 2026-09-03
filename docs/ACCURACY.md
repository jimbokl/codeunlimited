# How the numbers are computed (and why they are conservative)

codeunlimited reports savings as ranges grounded in your own logs, not
marketing multipliers. This page documents every estimate the tool makes, so
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
brief. The multiplier shown (e.g. "x9.6") is measured, not estimated.

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

## Delta and trend

`init` freezes a per-project baseline; `delta`/`report` recompute the same
metrics on activity after it. **Caveat:** right after the baseline, sessions
are young by definition, so "context per turn" dips mechanically. Trust the
**trend across a week or more** (one snapshot per `report` run), not a
single day-1 delta. The tool never extrapolates a day into a week for you.

## What the tool never does

- Never reads prompts or responses — only token counts, models, timestamps,
  project names.
- Never touches the network. No telemetry, no uploads.
- Never auto-edits configs that could change agent behavior beyond the
  documented efficiency rules; those changes are printed as suggestions.

If you find a case where a number is misleading, that's a bug — please open
an issue.
