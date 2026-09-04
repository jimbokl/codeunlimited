# Stateful orchestration runtime

Version 2.0 adds a bounded, stateful orchestration runtime alongside the
existing local auditor. It is intended for long coding jobs where repeatedly
transporting the full conversation is expensive.

The runtime uses the SKILL.state boundary `P + state + observation`:

- `P` is the immutable workflow plus terminal objective;
- `state` is the current validated `CodingState` JSON;
- `observation` is the latest bounded result needed by the next step.

Earlier prompts, model reasoning, responses, and tool transcripts are not
inserted into the next orchestration prompt. Each step launches a fresh
provider process. The claim is deliberately scoped to orchestration-step
boundaries: a Claude Code or Codex process can still accumulate context while
it performs multiple tool actions inside one step.

## Quick start

Create a small UTF-8 workflow such as `workflow.md`, then initialize a run:

```bash
codeunlimited run init sprint-2 \
  --project . \
  --skill workflow.md \
  --objective "Implement and verify the next planned increment" \
  --provider claude \
  --verify-program cargo \
  --verify-arg test \
  --verify-every-step
```

Inspect the exact next prompt without invoking a model, execute one increment,
or run a finite batch:

```bash
codeunlimited run status sprint-2 --project . --json
codeunlimited run prompt sprint-2 --project .
codeunlimited run step sprint-2 --project . --json
codeunlimited run auto sprint-2 --project . --steps 4 --json
```

The built-in providers are `claude` and `codex`. `command` accepts an explicit
executable implementing the same JSON envelope contract and is useful for
local fixtures or another agent CLI:

```bash
codeunlimited run init fixture \
  --skill workflow.md \
  --objective "Exercise the state transition" \
  --provider command \
  --provider-executable ./driver
```

Arguments are passed as exact argv values with repeated `--provider-arg` or
`--verify-arg`; no shell is involved. Provider arguments that would disable the
required ephemeral or structured-output controls are rejected.

## Durable state and transition contract

The hot state contains only bounded fields needed to continue the run:

- revision and `running`, `complete`, or `blocked` status;
- current focus and a compact memory summary;
- queued and completed work items;
- decisions, blockers, and active file paths;
- verification results and content-addressed artifact references;
- a bounded epistemic ledger of hypotheses, observations, verified claims, and
  disputed claims with evidence references;
- an archive count and hash chain.

The provider returns one strict `StepEnvelope` with the current base revision,
an outcome, a summary, and a typed delta. The delta can replace the queue,
blockers, active files, focus, or summary, and can append completed items,
decisions, and artifacts. Unknown fields, stale revisions, duplicate IDs,
unsafe paths, invalid completion, and over-budget state fail before the durable
state advances. Older completed items and decisions can be archived only when
the provider also supplies a new memory summary.

### Epistemic retention

The model, not a keyword heuristic, proposes which claims are worth retaining;
the runtime validates the proposal. A hypothesis may start without evidence.
An `observed` claim must cite the digest printed beside the latest observation,
or `kind=step` to bind it to the current step summary. A `verified` claim must
cite a passing verification revision or an artifact whose path and current
SHA-256 are resolved by the runtime. Stored evidence is revalidated whenever
the state is loaded.

Status changes are monotonic: hypothesis can be tested, observed can be
verified or disputed, and verified can only remain verified or become
disputed. A disputed claim can be retired only with a replacement memory
summary; the removed record enters the hash-chained archive. The hot ledger is
capped at 32 claims and eight evidence references per claim, so the agent must
prefer decision-relevant knowledge over a disguised transcript.

This is intentionally stricter than an unrestricted JSON merge. The paper's
open-weight-model analysis found premature overwrite or deletion to be the
dominant state-management failure, so destructive generic patch semantics are
not exposed here.

## Storage layout

Each run lives under `.codeunlimited/runs/<name>/`:

```text
manifest.json       immutable configuration and budgets
workflow.md         immutable workflow snapshot
state.json          current validated hot state
observation.txt     latest bounded observation
lock                exclusive run lock
attempts/           immutable metadata-only attempt records
archive/            compacted completed items and decisions
recovery.json       present only after an ambiguous attempt
```

Control files are written with atomic replacement, regular-file checks, and
symlink rejection. The run lock serializes state transitions. Add this local
runtime state to the project ignore file unless the team has explicitly chosen
another state-sharing policy:

```gitignore
.codeunlimited/runs/
```

The workflow is snapshotted and hashed at initialization. Editing the original
workflow file does not silently change an existing run.

## Budgets and termination

Defaults are 24 KiB for the immutable workflow, 16 KiB for serialized hot
state, 4 KiB for the latest observation, and 48 KiB for the complete prompt.
They are byte limits, not tokenizer estimates. Configurable hard ceilings are
128 KiB, 64 KiB, 32 KiB, and 256 KiB respectively. Provider stdout plus stderr
is capped at 1 MiB, one provider process defaults to a 30-minute timeout, a run
defaults to 100 total attempts, and a revision defaults to two failed attempts.

`run auto` is always finite (`--steps 1..100`) and also respects the run-wide
limits. Terminal or exhausted runs do not invoke another provider.

## Provider isolation and prompt caching

The stable prefix contains the runtime contract, JSON schema, immutable
workflow, its SHA-256, and objective. The dynamic suffix contains only current
state, latest observation, and revision. `run status --json` exposes
`prompt_bytes`, `stable_prompt_bytes`, `dynamic_prompt_bytes`, and cumulative
provider counters when reported:

```text
input_tokens
cache_read_input_tokens
cache_write_input_tokens
output_tokens
```

The Claude adapter uses print mode, disables session persistence, requests
schema-constrained JSON, and excludes dynamic system-prompt sections. The Codex
adapter uses `codex exec --ephemeral`, a JSON output schema, and a temporary
last-message file. Neither adapter resumes a provider conversation or stores a
provider session ID.

These choices make the visible prefix deterministic and improve the conditions
for provider-side prompt-cache reuse. They do not force a cache hit. Cache
eligibility, minimum prefix size, routing, retention, billing, and counter
semantics belong to the provider and model. In particular, byte counts are not
token counts, and OpenAI and Anthropic usage fields must not be combined with
one generic cost formula. The runtime does not pad small prompts merely to
cross a cache threshold.

The CLI adapters also cannot expose every cache-control option available in
the providers' direct APIs. API users can apply provider-specific cache
controls around the same stable-prefix/dynamic-suffix boundary; that is a
separate integration from this local process runtime. Consult the current
[OpenAI prompt-caching guide](https://developers.openai.com/api/docs/guides/prompt-caching),
[Anthropic prompt-caching guide](https://platform.claude.com/docs/en/build-with-claude/prompt-caching),
and [Claude Code CLI reference](https://code.claude.com/docs/en/cli-usage)
rather than treating API cache fields as CLI flags.

## Verification and recovery

A successful provider response is not enough to declare completion. Configure
`--verify-program` and repeated `--verify-arg` values; completion requires a
passing check unless `--allow-unverified-completion` was explicitly selected.
`--verify-every-step` makes the same check run after every successful response.

The provider can edit the repository before its response is validated. If a
provider fails, emits an invalid transition, or state persistence fails after
the repository changed, codeunlimited records the attempt and blocks the run
with `recovery_required` instead of guessing whether to retry. Inspect the
working tree, write a bounded UTF-8 observation describing the accepted state,
then acknowledge it explicitly:

```bash
codeunlimited run recover sprint-2 \
  --project . \
  --observation recovery-note.txt
```

Recovery preserves repository changes and advances the durable revision; it
does not roll back user work. The runtime records Git HEAD and a content-free
status digest when Git is available. If Git is unavailable, a failed provider
attempt is conservatively treated as ambiguous.

## Security and evidence boundary

There are two distinct planes:

- The **observation plane** (`audit`, `delta`, `report`, and experiment
  accounting) parses local metadata and has no network client.
- The **execution plane** (`run step` and `run auto`) launches the configured
  provider process in the project directory. That process inherits the user's
  environment and can read or modify project files, use its existing
  authentication, invoke tools, and make network requests according to the
  provider's own configuration.

Do not pass secrets on provider argv. Status output redacts common secret-flag
values, but process listings and the provider itself remain outside the local
auditor's privacy boundary.

Local tests prove deterministic prompt construction, exclusion of prior
orchestration transcripts, hard byte and attempt bounds, fresh-process flags,
strict state validation, atomic control-state updates, and recovery behavior.
That structural evidence **does not prove realized token savings**. The
SKILL.state paper reports large savings in its own warehouse and benchmark
setups, but those results are not a product guarantee for a coding repository.
A paid, matched-quality experiment comparing a full-history agent with this
runtime across increasing horizons is still required for a causal savings
claim.
