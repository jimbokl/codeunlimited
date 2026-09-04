# codeunlimited Confirmatory Session-Policy Experiment Design

**Date:** 2026-09-04
**Status:** Design approved; self-reviewed specification ready for review
**Base:** `origin/main` at `71cde837493fdeb77a108001d6e8300bef6dae5a`
**Maximum provider budget:** USD 100

## Goal

Produce a pre-registered, machine-readable, reproducible test of the bounded
claim that context-aware batching can reduce Claude Code input-token usage
relative to both one indefinitely growing session and restart-per-task while
holding task scope and functional acceptance constant.

The experiment is evidence for the session-boundary policy shipped by
codeunlimited. It is not designed to prove a universal savings percentage,
model-independent threshold, or benefit for every repository.

## Meaning of “proof”

The target is a bounded causal result under the published benchmark conditions:

- the protocol, corpus, allocation, decision rule, and analysis code are
  committed and pushed before confirmatory execution;
- each experimental block starts from byte-identical inputs for all policies;
- policy execution order is randomized and balanced;
- all recognized token counters are retained as exact integers;
- every policy is judged by the same acceptance suite;
- failures and exclusions cannot be silently removed;
- the pre-registered analysis is reproduced from the checked-in aggregates.

This upgrades the current evidence from a one-run author case study to a
controlled randomized benchmark. External replication is still required
before claiming generality outside the declared corpus, host, CLI, and model.

## Considered approaches

1. **Repeat the historical eight-task run once with exact counters.** This is
   inexpensive and would repair accounting, but still leaves one experimental
   block and no estimate of between-block variability. Rejected as the final
   proof; retained only as a possible smoke test.
2. **Randomized three-policy crossover over independent task bundles.** Every
   bundle is run under all three policies from the same initial state, with
   balanced order and exact paired analysis. Selected because it isolates the
   session policy while controlling task difficulty.
3. **Field A/B across real development sprints.** This has higher ecological
   validity but task scope, urgency, tools, and operator behavior cannot be held
   constant. Deferred as a post-confirmatory replication.

## Experimental unit and corpus

The independent experimental unit is a task bundle, not a request, session, or
individual micro-task. The confirmatory corpus contains 12 public, deterministic
Python-standard-library bundles. Each bundle contains six sequential coding
tasks that modify one small repository and culminate in one acceptance suite.
The tasks cover parsing, state transitions, data structures, validation, and
small algorithms without network access or third-party packages.

Each bundle is materialized from a committed seed directory. For every policy,
the runner copies that seed into a separate disposable working directory and
verifies the seed tree SHA-256 against the corpus manifest before execution.
Prompts, file bytes, task order within the bundle, and acceptance commands are
identical across policies. No arm can read another arm's directory or results.

The corpus manifest contains, for each bundle:

- public bundle identifier;
- ordered task prompt file hashes;
- seed tree hash;
- acceptance command;
- acceptance timeout;
- expected test count or other deterministic completion signal.

The corpus is frozen before the pilot. A corpus change after the freeze creates
a new protocol version and invalidates confirmatory results under the old one.

## Session policies

Every bundle is run under all three policies:

1. **Growing:** one Claude session receives all six tasks sequentially.
2. **Restart-per-task:** six fresh Claude sessions each receive one task.
3. **Context-aware batching:** two fresh Claude sessions receive tasks 1-3 and
   4-6 respectively.

Only the session boundaries differ. All policies receive the same concise
runner instruction, tool allowlist, task prompts, repository state, and per-arm
budget. The benchmark does not install different CLAUDE.md content between
arms, because that would confound session boundaries with instruction wording.

One long-lived non-interactive Claude process represents one session. Multiple
tasks for that session are delivered through stream-JSON input without
restarting the process. A fresh policy session receives a new UUID and process.
The runner records process start/end boundaries and refuses an arm whose
observed session count differs from the declared policy.

## Fixed Claude environment

Before the pilot, the protocol records:

- Claude Code version from `claude --version`;
- resolved model identifier returned by the CLI;
- fixed effort level;
- exact built-in tool allowlist;
- `--no-chrome`, `--disable-slash-commands`, an empty strict MCP
  configuration, and no project-level Claude settings or instruction files in
  the corpus seeds;
- a redacted SHA-256 fingerprint of every stable user-level configuration input
  that the authenticated CLI cannot disable;
- host OS and architecture;
- protocol, corpus, runner, and repository Git SHAs.

The environment must remain identical for all confirmatory arms. A CLI update,
model change, missing usage field, provider fallback, or configuration drift
stops the run before the next arm. The event is reported and the confirmatory
experiment is not resumed under the same protocol version.

Each arm has a USD 2.25 ceiling, allocated equally per micro-task: the growing
process receives USD 2.25, each of two adaptive processes receives USD 1.125,
and each of six restart processes receives USD 0.375. Six pilot arms plus 36
confirmatory arms imply a maximum configured spend of USD 94.50. The
orchestrator also enforces a USD 100 cumulative ceiling and starts no process
that would make the configured maximum exceed it. If the authenticated plan
does not report monetary cost, the per-process CLI ceilings remain mandatory
and the artifact marks cost accounting unavailable rather than inventing a
value.

## Randomization and contamination control

The protocol contains one committed random seed. A deterministic generator
uses it to allocate the six possible policy orders equally:

```text
GRA, GAR, RGA, RAG, AGR, ARG
```

where `G` is growing, `R` is restart-per-task, and `A` is adaptive batching.
The 12 confirmatory blocks use every order exactly twice. Pilot blocks use a
separate committed seed and never enter the confirmatory analysis.

All arms begin from isolated seed copies. Cross-arm provider cache warmth may
still exist, so order is balanced and cache-read/cache-write counters are
reported separately. The primary input metric counts the full recognized input
quantity, including cache reads and writes, matching codeunlimited's existing
accounting contract rather than provider price discounts.

## Pilot and confirmatory phases

Two pilot bundles validate the driver, counters, budgets, session counts,
timeouts, and acceptance harness. Pilot results are committed with
`phase = pilot` and are never included in the confirmatory estimator or used to
change the confirmatory claim threshold.

The implementation may be repaired after a failed pilot. After the final pilot
repair, the protocol manifest is regenerated with final code and corpus hashes,
committed, pushed, and merged. Confirmatory execution is forbidden unless the
working tree is clean and `HEAD` equals the frozen protocol commit.

The confirmatory phase contains exactly 12 bundles and 36 arms. It is
resumable only at an arm boundary. A compact local state ledger records
completed allocation cells, exact counters, remaining cells, and cumulative
budget so the runner never depends on conversation history. Restarting the
orchestrator cannot rerun or overwrite a completed arm without an explicit
new protocol version.

## Exact accounting

Raw stream events and Claude JSONL transcripts remain local under a gitignored
experiment directory. The aggregator deduplicates assistant records by message
ID and records exact non-negative integers for:

```text
requests
sessions
uncached_input_tokens
cache_read_input_tokens
cache_write_input_tokens
input_tokens
output_tokens
total_tokens
wall_milliseconds
```

`input_tokens` is the exact integer sum of the declared input categories and
`total_tokens` is input plus output. The aggregator independently cross-checks
stream output with recognized local transcript records. Missing timestamps,
duplicate identifiers with conflicting counters, arithmetic mismatch,
unrecognized usage shapes, or disagreement between sources marks the arm
`incomplete_accounting` and prevents a savings verdict.

Published arm rows contain bundle, policy, execution order, exact counters,
acceptance outcome, expected/observed test count, exit code, timeouts, retry
count, protocol SHA, corpus SHA, and hashes of the raw local evidence. They
contain no prompt or response bodies, session IDs, local paths, hostname,
credentials, or raw child output.

## Quality gate and failures

The scorer runs the committed acceptance command after every task and the full
suite after the arm. A confirmatory arm passes only when every task checkpoint
and the final suite succeed with the expected deterministic completion signal.

There are no discretionary task exclusions. Infrastructure failures are
recorded with a predeclared reason code and stop the experiment. Agent failures,
budget exhaustion, or timeouts are policy outcomes, not exclusions. If any
policy has a lower accepted-bundle count than another policy, the final result
is `no demonstrated token savings at equal completion`; token summaries remain
descriptive and no inferential savings claim is emitted.

The runner does not automatically retry an agent failure. An interrupted
process may be resumed once only when no assistant usage or file mutation was
recorded; otherwise that arm remains an observed failure. This rule prevents
selective cheap reruns.

## Pre-registered outcomes and analysis

The primary metric is exact total input tokens per accepted six-task bundle.
Requests, output tokens, cache categories, wall time, and cost are secondary
metrics. Quality and completion are mandatory guardrails.

Two primary paired comparisons are pre-registered:

1. adaptive batching versus growing;
2. adaptive batching versus restart-per-task.

For each comparison, the analyzer uses the 12 paired bundle differences and an
exact two-sided sign-flip test. It computes both raw p-values and Holm-adjusted
p-values for the two comparisons. Requests are never treated as independent
observations. The report also includes exact aggregate integer totals, the
aggregate input-token ratio, paired median absolute difference, paired median
percentage difference, and lower/higher/tied bundle counts.

The confirmatory result supports the bounded causal savings claim only if all
of the following hold:

1. all 36 arms have complete accounting;
2. all 36 arms pass the identical quality gate;
3. adaptive total input tokens are at least 10% lower than growing;
4. adaptive total input tokens are at least 10% lower than restart-per-task;
5. both Holm-adjusted exact p-values are below 0.05;
6. no protocol, environment, corpus, or allocation drift occurred.

The decision rule is conjunctive and cannot be relaxed after seeing results.
If it fails, the published verdict is negative or inconclusive with all valid
measurements retained.

## Artifacts and provenance

Implementation adds a versioned experiment area with these responsibilities:

```text
benchmarks/session_policy_v1/
  corpus/                 committed seed bundles and task prompts
  corpus-manifest.json    hashes and deterministic acceptance contracts
  protocol.json           frozen environment, allocation, outcomes, and rules
scripts/
  run_session_policy_experiment.py   resumable runner and local aggregation
  analyze_session_policy_experiment.py  strict confirmatory analyzer
tests/
  test_session_policy_runner.py
  test_session_policy_analysis.py
docs/experiments/session-policy-v1/
  README.md               reproduction and interpretation
  pilot-results.json      excluded instrumentation evidence
  results.json            privacy-preserving exact confirmatory rows
  analysis.json           deterministic analyzer output
  VERDICT.md              bounded claim and limitations
```

Local raw evidence and resumable state live under
`.codeunlimited-lab/session-policy-v1/`, which is gitignored. Published results
store SHA-256 hashes that bind the private raw files without disclosing their
content. The result artifact records the frozen protocol commit and refuses a
moving branch name as provenance.

The analyzer and runner use only the Python standard library. Existing Rust
CLI behavior and the v1 paired analyzer remain backward-compatible.

## Test strategy

Development uses witnessed RED/GREEN cycles with fake Claude subprocesses and
synthetic stream/transcript fixtures before any paid pilot:

1. protocol-schema tests reject missing fields, unknown fields, changed hashes,
   unbalanced allocation, non-frozen SHAs, and budgets above the ceiling;
2. driver tests prove one/six/two session boundaries, ordered prompts,
   deterministic allocation, clean seed copies, resume-at-arm-boundary, and
   no overwrite of completed arms;
3. accounting tests cover duplicate IDs, conflicting duplicates, all token
   categories, missing timestamps, malformed usage, arithmetic mismatch,
   source disagreement, rejection above unsigned 64-bit bounds, and redaction;
4. quality tests cover checkpoint failure, final-suite failure, timeout,
   budget exhaustion, interrupted zero-usage retry, and fail-closed verdicts;
5. analysis tests use hand-calculated 12-block fixtures for paired differences,
   exact sign flips, ties, Holm adjustment, effect thresholds, and every
   conjunctive decision-rule failure;
6. integration tests run the complete orchestration against a deterministic
   fake CLI without network access;
7. the existing Python discovery suite, Rust suite, formatting, clippy, MSRV,
   package audit, and three-platform CI remain green.

## Operational sequence

1. Implement and locally verify the corpus, runner, analyzer, and documentation.
2. Commit and push the implementation without running paid agents.
3. Run the two pilot bundles under the committed pilot protocol.
4. Apply instrumentation-only repairs exposed by the pilot and rerun the full
   local/CI verification suite.
5. Generate and commit the final protocol, corpus hashes, allocation, and
   analysis-code hash; merge this freeze before confirmatory execution.
6. Run the 12 confirmatory blocks without changing code or protocol.
7. Generate results and analysis with the frozen analyzer.
8. Independently recompute hashes, integer sums, paired statistics, quality
   gates, and decision rule before publishing the verdict.
9. Commit the complete evidence packet and open a reviewed GitHub PR.

## Acceptance criteria

- No paid confirmatory arm can start before its exact protocol commit is
  present on `origin/main`.
- The committed allocation has 12 blocks and each of six policy orders exactly
  twice.
- Fake-run tests demonstrate the declared one/six/two session counts.
- Every published token total is reproducible from exact arm-level integers.
- Any accounting or quality defect suppresses the savings verdict.
- The analysis uses task bundles as the inference unit and reproduces exact
  sign-flip and Holm results from committed rows.
- Published artifacts contain no raw conversation content or local identifiers.
- The total configured provider budget cannot exceed USD 100.
- A negative or inconclusive outcome is published unchanged.

## Non-goals

- Guaranteeing savings for every model, task, repository, or user.
- Treating a modeled counterfactual as observed savings.
- Using request-level observations as independent statistical units.
- Automatically shipping a v2.0 session-boundary advisor before the evidence
  supports it.
- Editing Claude account settings, provider limits, or billing configuration.
- Uploading raw prompts, responses, transcripts, or local project metadata.
