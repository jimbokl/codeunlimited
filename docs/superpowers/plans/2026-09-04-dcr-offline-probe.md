# DCR Offline Probe Implementation Plan

> For agentic workers: use superpowers:executing-plans for this tightly coupled
> reference kernel and fixture runner. Use TDD; this is not a production runtime.

**Goal:** Test dependency invalidation and context selection without any model
call, and expose where a manual script already solves the same task.

**Architecture:** A pure Python decision-contract graph consumes two in-memory
repository snapshots. A closed-world parser fixture compares guarded reuse,
whole-snapshot invalidation, and a hand-written local oracle. The runner never
executes source text, writes a target repository, or contacts a model.

**Tech Stack:** Python 3.10+ standard library, unittest, existing Python CI.

**Spec:** `docs/research/2026-09-04-delta-conditioned-reasoning.md`.

## Global Constraints

- No live provider requests, API credentials, model-generated code execution or
  automatic publication. Do not modify Rust runtime or release metadata.
- Keep all snapshots synthetic; zero calls are measured for the fixture runner,
  not a token-saving comparison. Report token savings as null.
- Missing/unknown dependencies, invalid graphs and oversized context fail closed.
- Decision contracts are empirical. Test success does not prove sufficiency.
- Independent literal outcomes, not the tested selector, decide fixture safety.
- Preserve the v2.1 branch; work in `codex/dcr-offline-probe`.

## Task 1: Pure guarded dependency graph

Files: create `scripts/dcr_contracts.py`, `tests/test_dcr_contracts.py`.

Interfaces:
`Contract(id, dependencies, upstream=(), complete=False)`;
`assess(contracts, baseline, current, context_budget_bytes=4096)` returning
per-id `Assessment(route, changed, context, context_bytes, reason)`.
Routes are `reuse`, `reconsider`, `abstain`. Dependencies use logical snapshot
keys, not executable paths. Context follows the transitive upstream closure.

- [x] RED: declare a test that a changed unrelated README preserves reuse, but
  changing config invalidates its consumer and downstream decision:

```python
result = assess(graph, base, dict(base, config="strict"))
self.assertEqual(result["parse"].route, "reconsider")
self.assertEqual(result["accept"].route, "reconsider")
self.assertNotIn("README", result["accept"].context)
```

- [x] Run `python3 -m unittest discover -s tests -p 'test_dcr_contracts.py' -v`.
- [x] Implement immutable contracts, validation, dependency closure, changed-key
  selection, deterministic byte-bounded context and fail-closed routes. Reject
  cycles/duplicate ids and missing baseline dependencies. Unknown current keys
  abstain rather than guessing they are irrelevant.
- [x] Add RED/GREEN cases for missing input, upstream incompleteness, contract
  tampering at dispatch, exact context-byte boundary, and deterministic order.
- [x] Commit the kernel and its tests after they pass.

## Task 2: Counterexamples and closed-world experiment

Files: create `scripts/dcr_probe.py`, `tests/test_dcr_probe.py`.
Add to kernel `refine(contract, witnesses)` where each witness contains two
snapshots and independently supplied before/after outcomes. Only single-key
mutations with different outcomes add a dependency; ambiguous/missing-key
witnesses refuse. Refinement only adds dependencies, never proves completeness.

- [x] RED: an intentionally incomplete contract misses a whitespace-policy
  change; a training counterexample adds that dependency and blocks stale reuse.
- [x] GREEN: implement monotone refinement with explicit empirical status.
- [x] RED: run fixed calibration and held-out mutations through the actual
  selector and local parser operation. Unknown input/config formats abstain.
  Compare actual reuse outputs to independent literal acceptance outcomes.
- [x] GREEN: provide `python3 -m scripts.dcr_probe --json`; emit fixture ids,
  route counts, incorrect reuses, ordinary-script outcomes, counterexample
  receipts and a scenario/implementation fingerprint. No inferred provider calls.
- [x] Verify report fields: `provider_calls=0`, `token_savings_percent=null`,
  `evidence_scope=synthetic_offline`, and an explicit manual-script comparator.
- [x] Commit after focused tests and full Python discovery pass.

## Task 3: Evidence and limits

Files: `docs/research/2026-09-04-dcr-offline-verdict.md` and a dated JSON report
under `docs/experiments/`. Update the proposal with the actual feasibility result.

- [x] Record primary-source AUTO/SIGIL method overlap, not a first-ever claim.
- [x] Generate report with the real runner, reproduce it byte-for-byte, and
  commit only sanitized synthetic results (no local paths or account identifiers).
- [x] Run full Python discovery and Rust release regression tests.
- [x] Review failure modes; correct any finding with a failing regression test.
- [x] Deliver GO/NO-GO for a paid pilot. A toy passing test is insufficient;
  require a meaningful gap beyond hand-written automation before new spending.

## Execution record

- Baseline: 45 Python tests pass; no tracked changes at start.
- Ruling: the proposal's full adaptive runtime is out of scope here. Build the
  smallest closed-world falsification instrument; do not present it as learned
  causal dependency discovery. This costs transferability but avoids false proof.
- Ruling: `Contract.complete` defaults to false, unlike the original tentative
  interface. Only a reviewed fixture assumption opts in. A counterexample
  revokes that assumption rather than certifying its own fix.
- RED evidence: the fail-closed interface skeleton produced 21 assertion
  failures across 16 kernel tests; the probe skeleton produced seven assertion
  failures plus its unimplemented-CLI error. GREEN: 25 focused tests pass.
- Investigation: negative control has two wrong reuses, not the initially
  expected one. Removing an undeclared configuration dependency is unsafe too;
  retained this case and corrected the independent count assertion.
- Initial verification: 70 Python tests and 193 Rust release tests passed.
- Review correction: shared closed-DSL validation now blocks unsupported source,
  config and operation inputs before dispatch in both arms. Negative control
  drops from two wrong reuses to one; DCR now has 7 reuse / 5 reconsider / 5
  abstain. Added witness and assessment fingerprints to the training receipt.
  Focused suite: 26 tests pass. No live provider experiments run.
- Final independent re-review: no critical/important findings. Final validation:
  71 Python tests, 193 Rust tests, exact generated-report comparison and diff
  whitespace checks pass. Rust runtime and release metadata are unchanged.
- Decision: local research branch retained; NO-GO for paid parser pilot until a
  meaningful gap over manual automation is demonstrated. No push or release.
