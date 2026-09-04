# Controlled A/B: the techniques vs a naive session, exact accounting

2026-09-04, author's machine. Two arms ran **in parallel, same model, same
8 coding tasks** (roman numerals, LRU cache, CSV parser, token bucket,
interval merge, edit distance, JSON flattener, RLE codec - each: implement
+ pytest suite, iterate to green):

- **Control**: one agent, all 8 tasks sequentially in a single growing
  conversation, no efficiency rules. Finished 165 tests green.
- **Treatment**: 8 agents, one task each in a fresh bounded context, rules
  applied (straight to work, no re-reads, one-line replies). All green.

Token counts are exact sums over the agents' own transcripts
(deduplicated by message id; prompt = uncached + cache reads + cache
writes). No estimates anywhere in this document.

## Results

| | requests | prompt tokens | total |
|---|---:|---:|---:|
| control (1 growing session) | 24 | 0.92M | 0.92M |
| treatment (8 fresh sessions) | 39 | 1.08M | 1.08M |

Three findings, in decreasing order of certainty:

**1. Rules make every request lighter - highly significant.** Per-request
context: control median 39.0k vs treatment 28.2k (-28%). Mann-Whitney
U = 860, z = 5.55, **two-sided p = 2.9e-08** (n = 24 + 39).

**2. On short task batches the naive session WON on totals (-17% for
treatment).** Eight fresh sessions paid eight boots (avg first-request
context 24k), and 8 x 24k of boot overhead exceeded what context growth
cost in a session that only reached 24 requests. We publish this loss
because it is true, and because it defines when the technique applies.

**3. The break-even is measurable and matches the shipped threshold.**
Control's context grew perfectly linearly: 25k -> 50k over 24 requests
(least-squares slope 1.0k/request). Growth excess of a continuing session
equals one fresh boot after **~7 requests** (slope*k^2/2 = 24k). So:
restarting pays whenever the *next task* is more than a handful of
requests - and the field sessions the audit flags run 100-6,800 turns
with slopes far steeper than this lab's clean 1k/request (real sessions
drag file reads and tool output; the field benchmark measured x6.8 -
docs/BENCHMARK.md). The `fresh-sessions` technique says "new *task* =
new session", the long-session detector fires at 30 turns - both on the
right side of this break-even, and micro-tasks should stay batched.

## Threats to validity

- n = 8 tasks, small and self-contained; agent harness, not interactive
  use. The per-request effect (finding 1) is the statistically strong
  claim; the totals (finding 2) describe this batch size specifically.
- Arms ran concurrently on one account; prompt-cache warmth could favor
  either arm symmetrically.
- Control's slope (1.0k/req) is a lower bound on real sessions, which
  accumulate tool output and file reads; field data shows tail turns at
  x6.8-x10 of early turns.

## Reproduce

The accounting scripts are stdlib-only: `scripts/bench_context.py` (field
data) and the A/B parser pattern documented here; agent transcripts are
standard Claude Code JSONL. Run your own arms with
`codeunlimited experiment start/finish` around comparable task sets.
