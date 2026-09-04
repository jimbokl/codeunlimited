# Delta-Conditioned Reasoning: a research hypothesis for codeunlimited

Date: 2026-09-04. Status: proposal, not a discovery, proof, or implementation.
Working name: DCR. No priority, patentability, or first-in-literature claim.

Update: the [offline feasibility probe](2026-09-04-dcr-offline-verdict.md)
is implemented. It validates the narrow mechanism on synthetic cases but finds
no demonstrated advantage over a manual script; a paid pilot is not justified
by this fixture. The broader research hypothesis below remains untested.

## Research question

Can one learned, executable dependency contract jointly decide (a) whether a
past decision must be reconsidered, (b) which facts enter the next model call,
and (c) which local actions remain reusable after a repository changes?

The hypothesis is that sharing this contract reduces **net tokens per accepted
task** compared with independently optimizing memory, plan reuse and session
boundaries. The target is an evolving codebase, not repeated identical prompts.

## Closest work: do not rebrand it as our invention

| Primary source | Established overlap | Consequence |
| --- | --- | --- |
| [SKILL.state v3](https://arxiv.org/html/2608.26263v3) | Explicit state replaces transcript growth | Bounded state alone is not our novelty |
| [Agentic Plan Caching](https://arxiv.org/abs/2506.14852) | Extracts, adapts and reuses plan templates | Caching a plan rather than an answer is not new |
| [SIGIL v2](https://arxiv.org/abs/2607.27309v2) | Compiles prose skills into typed executable harnesses | Compiling deterministic steps is not new |
| [AUTO](https://arxiv.org/abs/2607.04542) | Extracts verified programs, guards execution and returns to a reference agent on guard failure | Verified execution with fallback is directly overlapping prior art |
| [MAGE](https://arxiv.org/abs/2606.06090) | Execution-state memory with dependency-aware trajectories | A structured execution memory is not sufficient novelty |

This is a targeted primary-source screen, not a systematic novelty review.
Read full methods and inspect relevant code before claiming a distinction from
AUTO/SIGIL. Their published results have not been independently reproduced here.
Incremental computation, program slicing and counterexample-guided refinement
are established methods; applying their vocabulary does not create a theorem.

## Candidate contribution: decision-revision contracts

Instead of remembering only “decision X is verified”, retain a candidate
contract describing **what could make X wrong**. One contract connects:

1. the original objective and acceptance obligations;
2. exact source/configuration/environment dependencies;
3. deterministic applicability predicates;
4. a bounded local action or a decision requiring model judgment;
5. a postcondition and recovery path;
6. counterexamples that invalidated earlier contracts;
7. complete construction, validation, reuse and repair costs.

The same graph produces a small active context cut and invalidates executable
reuse. This coupling—not memory, guards, scripts or caching independently—is
the experimental contribution candidate. It may still overlap prior work.

Example contract representation, illustrative rather than an implemented API:

```json
{
  "decision_id": "parser-empty-input-contract",
  "objective_ref": "bugfix-17",
  "dependencies": ["parser-signature", "empty-input-spec", "encoding-config"],
  "unknown_dependencies": true,
  "guards": ["public-api-unchanged", "regression-fixture-current"],
  "action_ref": "local-empty-input-reproducer",
  "postcondition_ref": "frozen-empty-input-acceptance",
  "counterexample_refs": [],
  "assurance": "empirical-only",
  "reuse_enabled": false
}
```

An incomplete contract cannot enable autonomous write reuse. Hashes establish
identity, not semantic truth. The model can propose dependencies but cannot
declare its own proposal proved. Contracts never grant new permissions.

## Learning and execution loop

1. A native subscription agent solves an authorized first task. Extract a
   candidate decision contract from artifacts and explicit decisions, not hidden
   reasoning traces. Count this extraction if it uses a model.
2. Build read sets using structural dependencies plus exact configuration and
   environment inputs. Unknown dynamic inputs prevent a soundness claim.
3. In disposable test worktrees, perturb candidate dependencies and supposedly
   irrelevant inputs. Compare observable acceptance obligations, not whether a
   stochastic agent produces identical text. A violation adds a counterexample
   and enlarges or rejects the proposed contract.
4. At a later change, inspect the affected contract graph locally. Reuse a
   bounded operation only when its permissions and guards hold; otherwise ask
   the native agent about the affected obligations with the smallest available
   sufficient context. Unknown relevance widens the context or falls back.
5. Preserve provenance outside hot state. Update only affected contracts, record
   every fallback and repair, and disable contracts with poor net economics.

Perturbation testing finds counterexamples; passing a finite set does not prove
minimal dependencies or safety for arbitrary unseen changes. Reserve formal
claims for a closed DSL with explicit semantics and mechanically checked proofs.
The first prototype uses read-only checks/reproducers. Write macros require
isolated execution, diff validation, frozen tests and explicit action authority;
no speculative writes or external side effects in the user's working tree.

## Why this could outperform state-only orchestration

State-only execution can still ask a model to re-derive an unchanged decision.
Independent memory compression can remove a fact that an execution guard needs.
Whole-repository hashes safely invalidate reuse but lose opportunities whenever
an unrelated file changes. A shared, sufficiently accurate contract could avoid
all three problems and react only to decision-relevant change.

This is a conditional mechanism, not a proven complexity bound. For arbitrary
coding work we cannot assert O(number of semantic changes), because discovering
relevance, retrieval, guard checking and model failure can themselves be costly.

## Accounting: a falsifiable break-even calculation

For a simplified family of N comparable repetitions:

```text
T_native = N * L
T_DCR    = C + N * ((1-p) * L + p * V + F)

positive saving requires:
N * (p * (L - V) - F) > C
```

L is mean native total tokens per item, p the fraction safely reused, C all
contract construction/calibration tokens, V model-token overhead on a reuse,
and F mean extra fallback/repair tokens per item across the entire stream.
Local CPU checks can make V zero; they are not zero latency or zero energy.
If fallback calls need larger contexts, include that excess in F. In real
evaluation use exact observed totals rather than this stationary approximation.

If 80% of **token-weighted** model work were eliminated with negligible overhead,
5x would follow arithmetically. Neither 80% reuse nor negligible overhead is
established, and eliminating 80% of calls is not necessarily 80% of tokens.
Report cold-start and amortized results separately; never hide C off benchmark.

## Decisive experiment

Construct independent, evolving repository episodes with controlled changes:
irrelevant files, relevant source, shared configuration, specification changes,
and hidden dependency changes. Include nonrepetitive tasks where no reuse should
occur. Freeze acceptance contracts and test suites outside agent-writable scope.

Compare the same model/settings under:

- competent native agent with normal context management;
- v2.1 bounded-state runtime;
- manually authored scripts/codemods plus the same agent for uncovered cases;
- a reproducible guarded compilation/plan-reuse baseline (AUTO/SIGIL where
  integration permits), with an explicit compatibility gap if unavailable;
- DCR, then DCR without joint context/guard selection and without counterexample
  refinement, holding construction budget constant.

Primary outcome: complete total tokens per accepted task, including creation,
checking, subagents, failures and repair. Guardrails: unchanged acceptance,
unsafe-reuse rate, false invalidations, latency and subscription accounting scope.
Local fixture counts are not independent samples of model performance.

Reject or narrow the hypothesis if hand-written automation captures the same
benefit at lower total cost, guards miss important changes, refinement never
amortizes, the effect disappears on held-out changes, or the joint graph is no
better than independent components. A benchmark showing these boundaries is
still useful, but not evidence of a breakthrough.

## Relationship to the product roadmap

v2.2 delivers opt-in work packets and complete attempt accounting, the first
milestone of the adaptive-runtime design. DCR remains research-only, with no
demonstrated advantage over hand-written automation. Keep the research module
experimental and out of the critical release path. Prototype offline first;
live comparison requires a frozen protocol and explicit experiment budget.

Possible publishable outcome: a method and held-out evidence showing when
joint decision invalidation and context selection improves the token/quality
frontier over strong reuse and state-management baselines. If the novelty
review finds an equivalent method, reproduce/adapt it with attribution and
compete on implementation and evidence instead of inventing a new name.
