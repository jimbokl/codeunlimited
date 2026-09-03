# Show HN draft (fill [TREND] numbers after the dogfood week)

**Title:** Show HN: I reclaimed half of my Claude Code weekly limit with an offline audit

**Body:**

I kept hitting the weekly limit of Claude Code and Codex CLI mid-task, so I
wrote a tool that reads the session logs both CLIs already keep on disk and
answers one question: where does the limit actually go, and how much more
work could fit into it?

On my own logs (71k requests over 113 days) the audit found ~52% of weekly
volume reclaimable. The two big leaks, in my case:

1. **Context tax of long sessions** - by turn 40+, every reply drags the
   whole accumulated history through the context window. My worst sessions
   cost ~10x per turn at the tail vs the start.
2. **Top-tier model on mechanical replies** - thousands of requests to the
   most expensive model that ended in a 3-line answer.

`codeunlimited init` drops efficiency rules into CLAUDE.md/AGENTS.md and
freezes a baseline; `fix --apply` adds a state-file scaffold for long loops;
`report` then proves (or disproves) the savings against your own baseline,
with a trend that grows one snapshot per run. After [TREND: N days] under
the rules my context per turn is down [TREND: X%].

Details that matter:

- 100% offline. Only token counts, models, timestamps and project names are
  read - prompts never. No proxy, no gateway, no API keys.
- Rust, single binary, scans 3.6 GB of logs in ~2.4 s.
- Every estimate is deliberately conservative and documented (docs/ACCURACY.md);
  the day-1 delta dip is called out as mechanically biased - trust the trend.
- It is not a usage tracker - ccusage does accounting better; this answers
  "why so much and how to fit more".

https://github.com/jimbokl/codeunlimited

---

**Reddit variant title (r/ClaudeAI):**
I audited 113 days of my Claude Code logs - half the weekly limit was going
to context re-reads. Tool + numbers inside.

**X thread opener:**
Your Claude Code weekly limit isn't too small. It's leaking.
I measured 71k of my own requests: ~52% of weekly volume was reclaimable.
Here's where it goes and the offline tool that gets it back:
