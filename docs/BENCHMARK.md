# Benchmark: is the benefit real?

Fair question. Three layers of evidence, ordered from strongest to weakest,
all reproducible on your own machine. No synthetic workloads - the author's
real logs (71k+ requests, 113 days).

## 1. Context tax is arithmetic, not opinion (exact)

For every session longer than 30 turns, two exact sums over the raw log
records (`scripts/bench_context.py`, stdlib only):

- **actual** = prompt tokens the session really processed;
- **bounded** = the same number of turns, each priced at that session's own
  first-5-turn average context - what a fresh-session / state-file loop
  pays per turn (SKILL.state pattern, arXiv 2608.26263).

Author's machine, 2026-09-04, Claude Code logs:

```
long sessions (> 30 turns): 9
actual prompt tokens processed :   3,551M
bounded-context cost (exact)   :     519M
overall multiplier             :    x6.8
tokens burned by context growth:   3,033M
```

Worst real sessions: x8.1 (664 turns), x7.8 (388 turns), x7.0 (416 turns).
Three billion tokens - roughly four busy weeks of the weekly window - went
to re-dragging conversation history, not to producing code. The only
assumption is that a turn could run with early-session context given a
state file; that assumption is the pattern the tool installs, and every
number in the table is an exact sum, not an estimate.

## 2. Before/after with exact counters (observational)

`codeunlimited experiment` (v1.8) records bounded windows with exact
observed counters. One of the author's projects, rules installed 2026-09-03:

```
control   (13 days pre-rules):  568 requests, 237.9M input tokens, 2 sessions
treatment (post-rules):         274 requests,  70.1M input tokens, 1 session
exact observed change: -41.1% input tokens per task; capacity +69.8%
```

The tool itself prints the caveats: fewer than three tasks per arm = low
confidence, and an observed difference does not establish causality. That
honesty is deliberate - run your own experiment with
`codeunlimited experiment start/finish` around comparable tasks.

## 2b. Controlled A/B (parallel arms, exact counters)

Same 8 coding tasks, same model, run in parallel: one naive growing
session vs 8 fresh rule-following sessions. Per-request context fell 28%
(median 39.0k -> 28.2k, Mann-Whitney p = 2.9e-08); totals favored the
naive session on this *short* batch (8 boots outweighed 24 requests of
growth), and the measured break-even (~7 requests of work per restart)
lands exactly where the shipped 30-turn detector threshold sits. Full
numbers, including the honest loss: [EXPERIMENT.md](EXPERIMENT.md).

## 3. Scan speed (for the ritual to be free)

Full audit over ~3.6 GB of mixed Claude Code + Codex logs: **~2-4 s**
(Rust, rayon, string pre-filters before JSON). The Python reference
implementation takes ~2 minutes on the same data. A weekly `report --all`
costs nothing; `codeunlimited schedule` makes it automatic.

## Reproduce

```bash
python scripts/bench_context.py        # layer 1 on your logs (stdlib only)
codeunlimited audit                    # measured multipliers + forecast
codeunlimited experiment record ...    # layer 2 on your own windows
```
