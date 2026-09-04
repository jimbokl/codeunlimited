# Token-savings evidence verdict

Historical experiment commit audited: `3a3a3d757d24a49f21debc75c13cffafdc21607b`

Audit date: 2026-09-04

## 2.0 architecture verdict

Version 2.0 changes the product from advice plus measurement into a two-plane
system: the existing observation plane diagnoses local counters, while the new
execution plane runs each coding increment in a fresh provider process from an
immutable workflow, current bounded state, and latest bounded observation.
Prior orchestration prompts, responses, reasoning, and tool transcripts have
no next-step input channel.

The implementation now matches the central SKILL.state architecture at the
orchestration boundary. It also adds repository-oriented controls beyond the
paper's abstract transition loop: typed deltas, external verification,
content-addressed artifacts, immutable attempts, Git change detection, atomic
control state, and explicit recovery. A bounded epistemic ledger lets the
agent choose which claims survive while the runtime enforces evidence-gated
`hypothesis -> observed -> verified/disputed` transitions and prevents silent
deletion of verified knowledge.

This is structural proof, not an outcome measurement. Local tests establish
deterministic bounded prompt construction and a two-fresh-process promotion
from hypothesis to verification. They do not establish how many billed or
limit-weighted tokens a real provider will save, whether provider-side prompt
caching will hit, or whether a generic coding state is sufficient for every
task.

The paper's reported results explain why the architecture is worth testing,
but they do not transfer as a codeunlimited claim. In its warehouse experiment
at horizon 100, SKILL.state used 65,408 total tokens versus 1,062,387 for its
stateful LangGraph baseline (about 16.2x lower) while reporting comparable task
accuracy. Its public CTF and airline experiments reported 40.5%-65.9% fewer
tokens than selected baselines. Those figures depend on the paper's domains,
schemas, models, tools, action granularity, and accounting.

The paper also states the central failure condition: state must be a sufficient
statistic. A fact omitted before an observation disappears cannot be recovered
from the runtime transcript, and tasks whose goal is historical trajectory or
provenance need another representation. In codeunlimited, a provider process
may also accumulate context across several tool actions inside one increment;
therefore the bounded-context claim applies between orchestration steps, not to
every internal tool action.

## Verdict

The latest three-policy run reports a large and directionally credible token
reduction for context-aware batching. It is useful evidence for the session
break-even hypothesis, but it is not yet a reproducible causal proof that
codeunlimited saves a fixed percentage of tokens.

Using the rounded totals published in `EXPERIMENT.md`:

| Session policy | Requests | Author-reported prompt tokens |
|---|---:|---:|
| One growing session | 24 | 0.92M |
| Restart for every task | 39 | 1.08M |
| Context-aware batching, 3+3+2 | 22 | 0.65M |

The rounded values imply:

- `100 * (0.65 / 0.92 - 1) = -29.35%` versus one growing session;
- `100 * (0.65 / 1.08 - 1) = -39.81%` versus restart-per-task.

The reported `-29.9%` and `-40.2%` may be consistent with unpublished exact
totals, but they cannot be independently recomputed from the checked-in
rounded values.

## What the run supports

The result supports a U-shaped session-cost mechanism: an indefinitely growing
session repeatedly pays for stale context, while restarting too frequently
repeatedly pays the session boot cost. Under the documented assumptions
`b ~= 24k` and `g ~= 1k/request`, the modeled optimum
`sqrt(2 * b / g) ~= 6.93` requests is internally consistent.

This makes context-aware batching a credible technique worth using and
measuring. It does not make seven requests a universal threshold: boot size,
context growth, task relatedness, model, tools, and provider accounting all
change the crossover.

## Why this is not an exact proof yet

1. Commit `a1efd12`, made before the third-arm run, records the qualitative
   prediction that the modeled optimum is about seven requests and that these
   micro-tasks should be batched. That supports genuine prospective direction.
   The specific 3+3+2 arm, 0.70-0.90M interval, randomization plan, analysis
   plan, and outcome first appear together in `3a3a3d7`, so the quantitative
   protocol cannot be independently verified as pre-registered.
2. Exact per-task and per-request counters are not checked in, so neither the
   reported percentages nor uncertainty can be reproduced.
3. There is one aggregate observation per policy and no independently
   randomized experimental unit.
4. The policies used 24, 39, and 22 requests. Some of the observed difference
   may therefore be caused by extra work rather than session boundaries alone.
5. The control reports 165 passing tests while context-aware batching reports
   123. Passing different test suites establishes completion, not equal quality.
6. The 0.65M outcome is below the pre-run interval's 0.70M lower bound. The
   directional ordering matched; the quantitative interval did not.
7. Green CI on a documentation-only commit validates repository integrity, not
   the experimental counters.

The historical exact v1.7/v1.8 artifact also observed `+29.686%` input tokens
for its treatment, with one task per arm and uncontrolled scope. Together the
runs reject both simplistic extremes: neither “always restart” nor “never
restart” is a defensible universal policy.

## Product positioning

Position codeunlimited 2.0 as:

> A local token-efficiency auditor and bounded stateful orchestration runtime
> for coding agents. It finds measured waste, replaces growing cross-step
> transcripts with evidence-backed state, and records whether the change saves
> tokens on comparable work in your own project.

A compact promise is:

> Measure first. Carry forward state, not conversation. Verify what survives.

The current result may be described as an explicitly qualified case study:

> In one author-reported eight-task microbenchmark, context-aware batching used
> about 29% fewer prompt tokens than one growing session and about 40% fewer
> than restarting for every task. The result has not yet been independently
> reproduced.

Do not claim that codeunlimited guarantees 30-50% savings, 5x savings, or the
paper's 16.2x result. The defensible value is local measurement plus a runtime
that removes the architectural cause of cross-step transcript growth while
guarding the knowledge that replaces it.

## Evidence required for a causal savings claim

Before a run, commit an immutable protocol containing the task set, acceptance
suite, arm definitions, randomization seed and schedule, model/tool settings,
primary outcome, quality gates, exclusion rules, stopping rule, and analysis.

Then publish exact task-level counters for every assigned arm, including input,
output, cache-read and cache-write tokens, requests, sessions, wall time,
acceptance result, and every exclusion. Use at least 12-20 comparable task
blocks, randomize policy order, evaluate quality consistently, and analyze the
paired differences rather than treating requests as independent observations.

Only that pre-registered, machine-readable, reproducible dataset can upgrade
the claim from a promising case study to measured causal savings.

For 2.0, the primary arms must be the same agent and tasks under (A) normal
full-history continuation and (B) fresh-process stateful orchestration. Run
matched blocks at increasing horizons so the expected complexity difference
is visible, use one acceptance suite and independent quality rubric, and
retain exact per-step prompt bytes plus provider-reported input, output,
cache-read, and cache-write counters. Add ablations for bounded state without a
stable cache prefix and for the stable prefix without epistemic retention.
Because these mechanisms overlap, report each arm directly rather than adding
their percentage savings.
