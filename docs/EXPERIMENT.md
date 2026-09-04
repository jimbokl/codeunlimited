# Controlled A/B: useful negative evidence, not a savings proof

On 2026-09-04 the author ran two arms in parallel on the same reported model
and eight small coding tasks (roman numerals, LRU cache, CSV parser, token
bucket, interval merge, edit distance, JSON flattener, and RLE codec):

- **Control:** one agent completed all tasks sequentially in one growing
  conversation: 24 requests and 165 passing tests.
- **Treatment:** eight fresh agents each handled one task with the efficiency
  rules installed: 39 requests and all task tests passing.

## Observed result

| Arm | Requests | Author-reported prompt tokens |
|---|---:|---:|
| control, one growing session | 24 | 0.92M |
| treatment, eight fresh sessions | 39 | 1.08M |

The treatment used approximately **17.4% more prompt tokens in total**. Fresh
sessions made each request lighter but also paid eight session boots and made
15 additional requests. For this short batch, the extra work outweighed the
smaller context per request.

The reported request medians were 39.0k for control and 28.2k for treatment,
approximately 27.7% lower in treatment. That is a descriptive request-level
observation, not an independent-task result. The request-level rows and exact
integer arm totals are not checked into this repository, so those rounded
medians and totals cannot be independently recomputed here.

## Why the old significance claim was removed

Earlier text applied a Mann-Whitney test to 24 control requests and 39
treatment requests. Requests within the same session and task are correlated;
they are not 63 independent experimental units. Treating them as independent
is pseudoreplication and produces an unjustifiably small p-value.

The experimental unit must be a comparable task pair (or another independently
randomized unit). Version 1.9 includes
`scripts/analyze_paired_experiment.py`, which accepts exact task-level pairs and
uses an exact two-sided sign-flip test. The format is documented in
[PAIRED-SCHEMA.md](experiments/PAIRED-SCHEMA.md). No unpublished task-level
dataset has been invented for this historical run, so it has no replacement
significance claim.

## Break-even interpretation

The control context reportedly grew from about 25k to 50k tokens over 24
requests. Fitting a linear slope and equating accumulated growth with one 24k
fresh-session boot gives a **rough seven-request heuristic** for that one clean
run. It is a modeled crossover, not a measured universal threshold and not an
exact match for the 30-turn audit detector.

The product rule is therefore conditional: batch small related tasks when the
existing context remains useful; use a fresh session for a distinct multi-step
task when the old context would mostly be dead weight. Measure tokens per
completed comparable task instead of optimizing request size alone.

## Round 2: pre-registered three-arm test (the decisive one)

The two-arm result left a dispute: "no savings" (treatment lost 17%) vs
"savings exist" (field x6.8). Both follow from one law fitted on observed
constants - session boot b~24k, linear context growth g~1.0k/request:

    continue - restart = g*N*t0 - b   (restart pays iff N*t0 > b/g ~ 24)

Minimizing total cost gives an optimal session length of ~7 requests
(2-3 of these micro-tasks per session). **Prediction registered before the
run**: a third arm batching the same 8 tasks as 3+3+2 per session would
cost 0.70-0.90M - below BOTH prior arms.

| arm | requests | total tokens |
|---|---:|---:|
| control - 1 session x 8 tasks | 24 | 0.92M |
| treatment - 8 sessions x 1 task | 39 | 1.08M |
| **optimal - 3 sessions (3+3+2)** | **22** | **0.65M** |

Outcome: **-29.9% vs control, -40.2% vs treatment** - the predicted
ordering held decisively (the point estimate even beat the registered
interval's lower edge by 0.05M; all 123 tests green, same quality bar).
The savings dispute is resolved: session-boundary discipline saves tokens
when session length follows the break-even law, and burns tokens when it
does not. The `fresh-sessions` technique text carries exactly this rule.

## Threats to validity

- Only eight small, self-contained tasks were used.
- One control session was compared with eight treatment sessions, so session
  boots and agent orchestration are deliberately confounded with the policy.
- The checked-in document does not include exact per-task counters, random task
  assignment, blinded quality scores, wall time, or request-level observations.
- Concurrent arms may share provider cache state.
- Passing tests establishes task-specific functional completion, not equal
  maintainability or code quality.

This experiment is still valuable: it falsifies the simplistic claim that
fresh sessions automatically save tokens. It does not estimate a universal
savings percentage.

## Reproduce correctly

1. Pre-register comparable tasks and their acceptance tests.
2. Randomize each task's control/treatment order and keep model/tool settings
   fixed.
3. Record exact input counters and request counts for each completed task.
4. Store only aggregate task-level counters using the paired schema; do not
   include prompts, responses, paths, project names, models, or session IDs.
5. Run:

   ```bash
   python scripts/analyze_paired_experiment.py paired-results.json --json
   ```

6. Report aggregate totals, task-pair directions, the paired p-value, quality
   failures, and every exclusion. A lower per-request median is secondary to
   total tokens per accepted task.
