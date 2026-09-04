# Show HN draft

**Title:** Show HN: codeunlimited — an offline audit for Claude Code and Codex context waste

**Body:**

I kept hitting agent subscription limits mid-task, so I built a Rust CLI that
reads the session logs Claude Code and Codex already keep locally. It shows
where tokens accumulate: long-session context growth, cache-prefix rewrites,
retry storms, large session boots, and top-tier models used for mechanical
replies.

The difficult part turned out to be measurement, not parsing. Version 1.9
separates four evidence levels:

- exact observed log counters;
- modeled counterfactuals such as “what if later turns stayed near the early
  context size?”;
- detector estimates with visible ranges;
- realized input tokens per comparable completed task.

That distinction changed the product guidance. A controlled short-task run
made individual requests about 28% lighter, yet eight fresh sessions used
roughly 17.4% more prompt tokens overall because they paid more boots and made
more requests. A separate observational sprint comparison was also worse for
treatment. So the tool no longer says “new task = new session” or promises a
fixed savings percentage: batch small related tasks while context is useful,
restart when a distinct multi-step task would mostly drag dead history, and
measure the result.

`codeunlimited init` installs individually toggleable rules into
CLAUDE.md/AGENTS.md; `audit` explains opportunities; `experiment` records exact
observed counters in bounded windows; and the paired-task harness evaluates
repeated work.
Malformed instruction markers now fail without changing the file, and the
installers refuse binaries without a valid release checksum.

Details:

- Offline: no proxy, gateway, telemetry, API keys, prompts, or responses.
- Single Rust binary with Claude Code and Codex parsers.
- Every estimate and model is documented in `docs/ACCURACY.md`.
- Negative experiment results are published in `docs/EXPERIMENT.md`.
- It complements accounting tools such as ccusage; it focuses on diagnosis and
  outcome experiments.

https://github.com/jimbokl/codeunlimited

---

**Reddit variant title (r/ClaudeAI):**
I built an offline audit for agent context waste — and its first controlled
experiment found a loss, not a win.

**X thread opener:**
Smaller agent requests do not automatically mean lower total usage. Our
short-task treatment cut context per request but used ~17% more overall.
codeunlimited now audits the leak and measures the outcome:
