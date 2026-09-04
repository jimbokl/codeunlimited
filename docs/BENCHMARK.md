# Benchmark: what is observed, modeled, and realized?

codeunlimited uses an evidence ladder. Exact local counters, detector
estimates, counterfactual models, and real-work outcomes answer different
questions and must not be collapsed into one “savings” percentage.

## 1. Observed context and a modeled bounded counterfactual

`scripts/bench_context.py` reads recognized Claude Code assistant records.
For every session strictly longer than the selected threshold it computes:

- **actual prompt tokens:** an exact sum of the recognized integer input,
  cache-read, and cache-creation counters;
- **modeled bounded prompt tokens:** the session's first-N-request mean
  multiplied by its request count;
- **modeled difference:** actual minus modeled, which may be positive, zero,
  or negative.

The second quantity is a counterfactual. It assumes later work could have run
at the early-session average; logs do not observe that alternate execution.
Version 1.9 includes every eligible session, including sessions where the model
predicts no benefit or a loss. Malformed candidate rows are counted and
unreadable files fail the run.

A historical author snapshot selected nine long sessions and reported 3,551M
observed prompt tokens versus 519M modeled bounded tokens, an x6.8 ratio. The
old script filtered out sessions where actual did not exceed the model, so
that snapshot is retained only as **selected-session modeled exposure**. It is
not an unbiased population estimate and not realized savings.

Reproduce the corrected calculation on your own machine:

```bash
python scripts/bench_context.py --json
python scripts/bench_context.py --min-turns 30 --early-turns 5
```

JSON output contains the exact observed total, modeled total, positive/
negative/zero session counts, malformed-candidate count, and
`complete_accounting`. It contains no project or session identifiers.

## 2. Exact observed experiment counters

`codeunlimited experiment` records recognized integer counters in explicit
half-open windows and divides them by declared completed-task counts. The
v1.7/v1.8 sprint artifact contains:

```text
control:   39,110,299 input tokens / 1 completed task
treatment: 50,720,723 input tokens / 1 completed task
observed change: +29.686%; observed capacity view: -22.891%
```

Those counters and arithmetic are reproducible from
[the checked-in artifact](experiments/2026-09-04-v1.7-v1.8.json). The task
mix, duration, request count, tools, and operator behavior differed, so the
comparison is low-confidence and observational. It does not attribute the
loss to codeunlimited.

## 3. Controlled short-task experiments

The author ran eight small coding tasks under three session policies. The
published rounded totals are 0.92M prompt tokens for one growing session,
1.08M for restart-per-task, and 0.65M for context-aware 3+3+2 batching. The
rounded values imply approximately **29.35% less** than the growing session
and **39.81% less** than restart-per-task for the batching arm.

The former request-level significance test was invalid because requests inside
one session are correlated. The third-arm result is also not a causal estimate:
the pre-run protocol and exact task-level counters are absent, policy request
counts differ, and one aggregate observation per policy cannot quantify
uncertainty. Version 1.9 provides a paired-task sign-flip analyzer for future
experiments with published task-level counters. See
[the experiment narrative](EXPERIMENT.md),
[the evidence verdict](EVIDENCE-VERDICT.md), and
[the paired schema](experiments/PAIRED-SCHEMA.md).

## 4. Scan performance

The redacted local performance harness measures wall time, RSS, retained
records, and index behavior separately from product benefit. Historical
Apple M4 observations and exact commands live in
[BENCHMARKING.md](BENCHMARKING.md). Scanner speed proves that running an audit
is cheap; it does not prove that applying a technique saves tokens.

## Decision rule

Use `audit` to locate opportunities, apply only the techniques whose
trade-offs fit the project, then measure comparable completed work. The
primary outcome is input tokens per accepted task, guarded by completion rate,
quality, wall time, request count, and restart boot cost. A smaller request is
not a win when completing the task requires enough extra requests to increase
the total.
