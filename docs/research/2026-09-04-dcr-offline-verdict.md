# DCR offline feasibility verdict

Date: 2026-09-04. Prototype: `codex/dcr-offline-probe`.
Implementation introduced at `ea68727`; final source fingerprint is in the
machine-readable report. Scope: a synthetic, closed-world parser workflow.

## Decision

**GO for the invalidation mechanism as a research component. NO-GO for a paid
token-savings pilot of this parser scenario.** The mechanism works on the frozen
fixtures, but ordinary hand-written automation already solves every supported
case without a model. No incremental value over that baseline is established.

Do not enable DCR in the production runtime or advertise a saving from this
result. The prototype does not learn a complete dependency graph, integrate
with subscription agents, or measure token/quota savings.

## Reproduce

From the repository root:

```bash
python3 -m unittest discover -s tests -p 'test_dcr_*.py' -v
python3 -m scripts.dcr_probe
python3 -m scripts.dcr_probe --json
```

The generated JSON is checked in at
[`../experiments/dcr-offline-2026-09-04.json`](../experiments/dcr-offline-2026-09-04.json).
It contains scenario and implementation SHA-256 fingerprints, all case outcomes,
and explicit unknown values. No live provider is invoked. The surrounding
development/review conversation is not counted as “free” by this probe.

## Observed results

| Method | Direct correct reuse | Wrong reuse | Deferred/abstained |
| --- | ---: | ---: | ---: |
| Contract missing whitespace dependency (negative control) | 7 | 1 | 9 |
| Refined, explicitly reviewed fixture contract | 7 | 0 | 10 |

The ten non-reuse cases comprise five requiring reconsideration and five safe
abstentions. They are not completed tasks or free repairs. Route expectations
match all 17 cases. A single calibration counterexample adds the whitespace
dependency; the candidate stays unapproved until the fixture author explicitly
assumes the finite DSL dependency list is complete.

The manual script computes correct current-policy results for **14 supported
cases** and abstains on **3 unsupported cases**, with no wrong result. It does
not need DCR or a model. Its coverage is greater than direct guarded reuse
because it already implements all supported policy variations.

The whole-snapshot comparison is a routing-only strawman. It is not a competent
native-agent baseline and supports no token or quality advantage claim.

## What the checks establish

- Upstream changes invalidate dependent decisions; unrelated known inputs are
  excluded from selected context.
- Unknown inputs, missing required values, incomplete dependency models and
  insufficient context budgets block reuse.
- Cycles, duplicate ids and invalid baseline dependencies are rejected.
- A receipt cannot be reused after snapshot or contract changes. This is an
  in-memory check, not atomic filesystem authorization or a sandbox.
- Counterexamples can add dependencies but cannot certify completeness or
  silently remove existing dependencies.
- The initial dependency-only negative control failed both on whitespace-policy
  change and on deletion of that policy. Review added a shared domain gate to
  both arms: deletion is now rejected before dispatch, while the negative
  control still incorrectly reuses its decision for a supported policy change.
  These are different measured revisions, not selectively excluded cases.
- The CLI runs under an audit-hook test denying network/process execution and
  file writes. Snapshot strings are data, never executed Python or shell code.

These are mechanism tests. The fixtures are small, human-authored and visible
to the implementer. “Holdout” means not supplied to the one-witness refinement
step; it is not a blind statistical holdout or a scientific benchmark sample.

Independent review identified the unsupported-format route mismatch and a
missing counterexample receipt. Regressions now cover unknown source/config
values, non-text and oversized operation inputs; the report includes separate
witness and assessment hashes. Dependency-tuple reordering can conservatively
invalidate a receipt; it does not permit unsafe reuse and is left unchanged.
Final re-review has no critical/important findings. Local verification passed
26 focused tests, 71 total Python tests and 193 Rust release tests. The JSON
report reproduces byte-for-byte; Rust runtime/release metadata are unchanged.

## Prior-art check: narrower novelty than the initial idea

[AUTO, sections 3–4](https://arxiv.org/html/2607.04542v1) already uses a typed
graph, capability/memory effects, counterexample-guided extraction, guarded
execution and return to a reference agent. Its empirical guards use lexical
input similarity with calibration; unmeasurable verification blocks emission.
Our logical-input dependency check is a different small mechanism, but that
difference alone establishes neither novelty nor superiority.

[SIGIL, sections 2.3–3.3](https://arxiv.org/html/2607.27309v2) already separates
code-owned procedure from model-owned judgment and carries typed dependencies
and instruction-scoped knowledge in an executable graph. Therefore “one graph
for knowledge and execution” is also too broad to claim as our invention.

This pass inspected the primary method descriptions, not a full reproduction
or exhaustive code-level novelty audit. A scientific contribution would require
a precise distinction and a direct, budget-matched comparison.

## Digital replicators: useful engineering interpretation

Retain versioned local operations with applicability contracts and reuse them
by content-addressed reference. Do not copy their text into every model prompt.
The useful property is repeated execution without another model request—not
the number of copies. Local execution still consumes CPU, storage and time;
generation, validation and repair can consume model tokens.

A future bounded candidate-selection experiment could compare operation
variants against frozen tests and total lifecycle cost. It must cap candidates,
generations and resource use, retain provenance, require explicit promotion,
and allow immediate disablement. No self-propagation, background agents,
credential acquisition, permission growth, or automatic deployments are
implemented or authorized by this prototype.

## Next experiment worth doing

Select an evolving repository task where hand-written scripts leave genuine
semantic decisions unresolved. Compare automatic candidate contracts with a
frozen human contract and existing guarded reuse. Charge for dependency
extraction, all probes and every fallback. First test relevance under held-out
specification/configuration changes. Only if a measurable gap remains is the
previously proposed small live A/B worth its budget.

The current result supports investing in complete-cost measurement and local
automation for v2.2. It does **not** yet support a general self-improving agent,
a scientific discovery, or 25%/50%/5x savings.
