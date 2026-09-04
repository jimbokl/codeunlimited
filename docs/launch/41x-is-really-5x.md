# Why agent token multipliers need an evidence ladder (article draft)

Target: HN / dev.to / blog. Angle: methodological honesty as the product.

## Outline

1. **The claim.** SKILL.state (arXiv 2608.26263) reports up to 41x token
   reduction for long-horizon agent loops: replace the growing conversation
   history with a bounded state file, turning O(T^2) context into O(T).

2. **A production complication: prompt caching.** Agent harnesses can serve an
   unchanged prefix from cache, so an uncached quadratic baseline does not
   directly describe provider accounting or subscription limits. Illustrative
   cache weights such as 0.1x reads and 1.25-2x writes are assumptions that
   must be replaced when the provider exposes authoritative weights.

3. **The model after cache weights.** Under the stated cache-price
   assumptions, the same pattern models a roughly 2-5x reduction on long
   loops. Show the formula:
   effective cost = unc + 0.1*cached + 1.25*w5 + 2*w1h vs the state-file
   equivalent. Then the empirical check on 71k real requests:
   bounding context at 2k/3k/5k tokens per turn models 79-86% reduction on
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
   - The candidate fixes are boring: context-aware session boundaries, state
     files for loops, light models for mechanical edits, and MCP hygiene.
   - Conservative estimates beat hype: every detector's math is public
     (docs/ACCURACY.md).

6. **CTA.** codeunlimited on GitHub - audit your own logs, then verify total
   input tokens per comparable completed task. The historical 52% detector
   output is a model-dependent opportunity estimate, not realized savings.
