# 41x is really 5x: honest math for agent token "savings" (article draft)

Target: HN / dev.to / blog. Angle: methodological honesty as the product.

## Outline

1. **The claim.** SKILL.state (arXiv 2608.26263) reports up to 41x token
   reduction for long-horizon agent loops: replace the growing conversation
   history with a bounded state file, turning O(T^2) context into O(T).

2. **What the paper ignores: prompt caching.** Production agent harnesses
   (Claude Code, Codex CLI) already serve the unchanged prefix from cache.
   A cache read costs ~0.1x of an uncached token; a cache write 1.25-2x.
   The "41x" compares uncached quadratic re-reads against a state file -
   but nobody ships uncached quadratic re-reads anymore.

3. **The real math.** Cache-corrected, the same pattern saves roughly
   2-5x on long loops - still excellent, just honest. Show the formula:
   effective cost = unc + 0.1*cached + 1.25*w5 + 2*w1h vs the state-file
   equivalent. Then the empirical check on 71k real requests:
   bounding context at 2k/3k/5k tokens per turn yields 79-86% savings on
   Claude Code logs and 63-76% on Codex logs (x4.8-7.2 / x2.7-4.2) - and
   only for the long-loop share of traffic, not the whole subscription.

4. **Subscription users don't pay in dollars - they pay in limit.**
   For them the currency is "how much work fits into the weekly window".
   Cache reads are cheap but not free against the limit; context tax of
   long sessions is the dominant leak (measured x9.6-10x per-turn cost at
   session tails in my logs).

5. **Takeaways.**
   - Multipliers from papers assume a baseline nobody runs; measure on
     your own logs (the tool does it offline in seconds).
   - The fixes are boring and work: fresh sessions per task, state files
     for loops, light models for mechanical edits, MCP hygiene.
   - Conservative estimates beat hype: every detector's math is public
     (docs/ACCURACY.md).

6. **CTA.** codeunlimited on GitHub - audit your own logs, verify the
   delta against your own baseline. [TREND numbers from dogfood week here.]
