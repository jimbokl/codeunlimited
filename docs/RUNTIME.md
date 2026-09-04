# Stateful orchestration runtime

Version 2.1 builds on the bounded stateful runtime introduced in 2.0. Its
primary execution layer uses existing Claude Code and Codex subscription
logins. An optional, separately configured OpenAI/Anthropic API layer uses
metered API credentials. The local auditor remains offline.

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
  --subscription-profile lean \
  --verify-program cargo \
  --verify-arg test \
  --verify-every-step
```

Inspect the self-contained next prompt without invoking a model, execute one increment,
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
required ephemeral, instruction, profile, or structured-output controls are
rejected at initialization. Codex `-c`/`--config` and profile overrides are not
accepted through this adapter; configure the supported runtime options instead.

`standard` is the default for both existing and new subscription runs. `lean`
is opt-in: Claude disables MCP servers, Chrome, and slash commands and retains
Bash/Edit/Read/Write/Glob/Grep; Codex ignores user configuration while retaining
authentication. Use standard when a workflow depends on those integrations.
Lean does not change the model or reasoning effort and does not use Claude
`--bare` (which would bypass subscription OAuth).

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
provider-instructions.md  verified stable instructions for built-in/API providers
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

The state/envelope/manifest format stays at schema version 1. New provider
profile fields default to standard when absent. A missing instruction file in
a legacy run is reconstructed in memory during `status`/`prompt`, then created
before execution. Inspection never rewrites the manifest. It remains available
if the newer compiled prompt exceeds a legacy run's configured budget; actual
execution still rejects the over-budget prompt before invoking a provider.

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

The stable channel contains the runtime contract, immutable workflow, its
SHA-256, and objective. The dynamic channel contains current state, latest
observation, and revision. Built-in/API providers receive the response schema
out of band. The generic command provider receives a combined self-contained
prompt including that schema; `run prompt` prints this combined inspection
form, not a capture of hidden provider context. `run status --json` exposes
`prompt_bytes`, `stable_prompt_bytes`, `dynamic_prompt_bytes`, and cumulative
provider counters when reported:

```text
input_tokens
input_token_semantics
uncached_input_tokens
cache_read_input_tokens
cache_write_input_tokens
cache_write_5m_input_tokens
cache_write_1h_input_tokens
output_tokens
```

Claude uses `--append-system-prompt-file` for stable instructions and receives
only dynamic input on stdin. Codex uses `codex exec --ephemeral` and a constant
bootstrap requesting three ordered reads: stable instructions first, then
state and observation. It does not replace Codex's built-in instructions with
`model_instructions_file`. Native project rules remain the CLI's responsibility;
the stable artifact is not an AGENTS.md snapshot. Ordered reads are a worker
instruction, not an enforced provider cache boundary. Neither adapter resumes
a provider conversation or stores a provider session ID.

OpenAI/Codex input totals include cached input. Anthropic `input_tokens` is the
uncached remainder, so transported input is uncached + cache read + cache write.
Status and step reports derive `transported_input_tokens` and
`cache_read_ratio_basis_points` only from sufficient reported counters. Unknown
values remain null; mixed legacy semantics make aggregate derivations unknown.
A response with invalid structured output still contributes API usage when
reported. Transport failures without a readable usage record remain unknown.
Probe usage is excluded from step totals and reported separately.

These choices make the visible prefix deterministic and improve the conditions
for provider-side prompt-cache reuse. They do not force a cache hit. Cache
eligibility, minimum prefix size, routing, retention, billing, and counter
semantics belong to the provider and model. In particular, byte counts are not
token counts, and OpenAI and Anthropic usage fields must not be combined with
one generic cost formula. The runtime does not pad small prompts merely to
cross a cache threshold.

Cache-read ratio describes reuse, not fewer transported tokens or recovered
subscription quota. Provider-native prefixes may already be cached without
codeunlimited. Public API discounts do not establish how subscription limits
are charged. Consult the current
[OpenAI prompt-caching guide](https://developers.openai.com/api/docs/guides/prompt-caching),
[Anthropic prompt-caching guide](https://platform.claude.com/docs/en/build-with-claude/prompt-caching),
and [Claude Code CLI reference](https://code.claude.com/docs/en/cli-usage)
rather than treating API cache fields as CLI flags.

### Optional API layer

```bash
# Supply OPENAI_API_KEY securely in the environment first.
codeunlimited run init planner --skill workflow.md \
  --objective "Produce one bounded planning increment" \
  --provider openai-api --api-model YOUR_SUPPORTED_MODEL --cache-ttl 30m
```

For Anthropic use `--provider anthropic-api --api-model YOUR_SUPPORTED_MODEL`,
with `ANTHROPIC_API_KEY` and `--cache-ttl 5m` (default) or `1h`. OpenAI uses the
Responses API, a stable developer block, explicit cache breakpoint, stable key,
and `30m` TTL. Anthropic caches the stable system block. Both require a model
supporting the selected cache and JSON-schema features; model compatibility is
not inferred from its name or validated by a paid request at init.

The API capability is `single_turn_no_local_tools`: these adapters can return
state transitions but cannot inspect or edit a repository. A workflow must
arrange any external work independently. They are not subscription transports
and are not drop-in replacements for the coding CLIs. Prompts may contain the
workflow, objective, retained state, and observation; the API sends these to the
configured endpoint. Keys are read only from the environment. Remote endpoints
require HTTPS, loopback HTTP is permitted for tests, and redirects are disabled.
Responses are limited to 1 MiB and model output to 4096 tokens. HTTP failures
are content-free and are not automatically retried.

### Opt-in cache probe

```bash
codeunlimited run cache-probe sprint-2 --project . --json
```

This command consumes two provider calls and never runs automatically. It uses
two distinct no-op samples with a maximum 60 seconds per call. Subscription
probes force the reduced integration profile: Claude has no built-in tools,
MCP, Chrome, slash commands, or hooks; Codex ignores user configuration and
uses a read-only filesystem sandbox. Only model/effort provider arguments are
accepted for probing. A user-supplied executable is still trusted code; the
runtime cannot make an arbitrary binary harmless.

The report contains both raw usage records, duration, stable hash, and
`cache_hit_reported`: true for a positive second cache-read count, false for
reported zero, null when unavailable. Cached tokens can belong to the provider's
own system prompt. The probe does not establish that our workflow was cached,
does not reproduce a standard-profile coding workload, and does not establish
incremental savings. It commits no work transition and runs no verification
command. Save its report separately when accounting for all experiment costs.

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
  accounting) parses local metadata and makes no network requests.
- The **execution plane** (`run step`, `run auto`, and `run cache-probe`) invokes
  the selected subscription process or optional API. A provider process inherits the user's
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
