# Version 2.3: the first-install retro verdict

Version 2.3 adds one read-only command, `codeunlimited verdict`, and a one-line
summary of it at the end of `init` when the project already has history. It
answers the question every new user asks at install time: what would this tool
have changed if it had been running from the start?

## What the number is

For every session in local history longer than `--min-turns` requests
(default 30), the verdict replays the session as if each request had carried
that session's early-request mean context (`--early-turns`, default 5). The
observed side is an exact sum of recognized local log counters. The modeled
side is the same counterfactual published in [BENCHMARK.md](BENCHMARK.md)
Layer 1 and `scripts/bench_context.py`; the defaults match that script so the
CLI and the reference implementation cannot drift apart silently.

## What the number is not

- It is **modeled counterfactual exposure, not realized savings**. Nothing was
  actually saved; the console, JSON, and `init` renderings all carry the label.
- It is not causal. Long sessions may have carried context the work genuinely
  needed; the model cannot distinguish dead history from a live working set.
- It makes no quality claim. A bounded replay of a past session might have
  needed extra requests to re-acquire context; the model does not price that.
- Sessions at or below the min-turns floor are excluded on purpose: below the
  measured break-even (~7 requests) a fresh session costs more than it saves,
  and modeling savings there would overstate the case.

## Evidence scope

Unit tests cover growing, flat, short, and mixed histories, the modeled/label
wording, and the `init` gating. The command reads the same local logs as
`audit`, writes nothing, and never leaves the machine. A realized-savings
number would require the pre-registered increasing-horizon experiment that
remains the next evidence milestone; this release deliberately does not claim
one.
