# codeunlimited 2.0 Stateful Runtime Design

**Date:** 2026-09-04
**Status:** Ready for written-spec review
**Base:** `main` at `71cde837493fdeb77a108001d6e8300bef6dae5a`

## Goal

Turn codeunlimited from an offline token auditor plus instruction-policy
installer into a stateful orchestration runtime for long coding tasks. The
runtime must make bounded context a mechanical property: every orchestration
step starts a new provider process with only an immutable workflow, a compact
validated execution state, and the latest bounded observation. It must never
resume or replay the previous model transcript.

Version 2.0 must be useful without running a paid benchmark. All state-machine,
prompt-bound, persistence, provider-command, and recovery behavior is verified
locally with deterministic fake providers. Live token-savings experiments are
explicitly deferred.

## Product position

The existing `audit`, `init`, `fix`, `delta`, `report`, and `experiment`
commands remain the observation plane. The new `run` command family is the
execution plane. The product promise becomes:

> codeunlimited bounds the context carried between agent work units and records
> the actual provider counters it can observe. It does not promise a universal
> savings multiplier or that compact state is sufficient for every task.

The execution plane is opt-in. Existing commands remain offline and retain
their current privacy behavior. `run step` and `run auto` deliberately launch a
configured external model CLI, which may make network requests under that
provider's authentication and policy.

## Considered approaches

1. **More instruction rules and smarter restart recommendations.** This leaves
   context retention under the host agent's control and cannot guarantee that
   old messages disappear. Rejected as the primary 2.0 mechanism.
2. **Embed one model vendor's HTTP API and implement a complete coding-agent
   tool sandbox.** This could make each tool action a strict state transition,
   but it would introduce secret handling, networking, provider lock-in, and a
   new command-execution security boundary in one release. Deferred.
3. **Provider-neutral state machine with ephemeral Claude/Codex worker
   adapters.** Each worker performs one bounded coding increment and returns a
   typed state delta. The worker's internal tool transcript exists only for
   that increment; the next process receives no session identifier or prior
   transcript. Selected for 2.0.

The selected approach implements SKILL.state at the orchestration-step level.
A provider worker may use multiple internal tool calls during one step, so 2.0
does not claim that every individual tool action has constant context. The core
driver interface is intentionally small enough for a future direct-API,
single-action adapter without changing the state or storage contracts.

## User workflow

```text
codeunlimited run init feature-x \
  --project . \
  --skill docs/feature-x.md \
  --objective "Implement feature X and pass its acceptance tests" \
  --provider codex \
  --verify-program cargo \
  --verify-arg test \
  --verify-arg=--locked

codeunlimited run status feature-x --project .
codeunlimited run prompt feature-x --project .
codeunlimited run step feature-x --project .
codeunlimited run auto feature-x --project . --steps 8
codeunlimited run recover feature-x --project . --observation recovery.txt
```

`init` creates a durable run but does not call a model. `prompt` renders the
exact next prompt without calling a model. `step` performs at most one provider
invocation. `auto` repeats `step` until the state becomes `complete`, `blocked`,
or `needs_recovery`, or until its explicit step limit is reached. There is no
unbounded default loop. `status` is read-only. `recover` acknowledges an
ambiguous provider attempt and supplies the bounded observation needed for the
next clean step; it never resets or rewrites repository changes.

Run names use the existing bounded ASCII-safe naming rules. A run name is
unique within one project. `init` refuses to replace an existing run.

## Architecture

```text
                 existing observation plane
 local logs ───────────────────────────────────> audit/report/experiment

                 new execution plane
 workflow ─┐
 state ────┼─> PromptCompiler ─> ephemeral Provider ─> StepEnvelope
 latest O ─┘          ^                                  │
                      │                                  v
                bounded prompt <─ StateMachine <─ validate typed delta
                                      │
                                      v
                             atomic StateStore + metadata-only journal
```

The new subsystem has six boundaries:

- `runtime::model`: serialized manifest, coding state, typed delta, status,
  usage, and recovery types;
- `runtime::validate`: all field, transition, path, and byte-budget checks;
- `runtime::prompt`: deterministic, cache-friendly construction from exactly
  workflow + state + latest observation;
- `runtime::provider`: process invocation and provider-specific argument/output
  adaptation;
- `runtime::store`: locking, strict reads, atomic commits, and the attempt
  journal;
- `runtimecmd`: clap-facing orchestration with no state-machine logic in
  `main.rs`.

The library interfaces remain provider-neutral. CLI-specific parsing and exit
messages stay outside the state model.

## Runtime invariants

Every provider invocation must satisfy all of these rules:

1. A new operating-system process is started for every step.
2. No previous provider session ID is stored in the manifest or passed on the
   command line.
3. Claude adapters always use print mode and `--no-session-persistence`; they
   reject `--continue`, `--resume`, `--fork-session`, and `--session-id` in
   passthrough arguments.
4. Codex adapters always use `exec --ephemeral`; they reject `resume`, `fork`,
   and any future passthrough argument containing an explicit thread/session
   continuation option.
5. The prompt compiler accepts no transcript/history parameter. Its only
   dynamic inputs are the current validated state and latest observation.
6. The complete rendered prompt must fit the configured hard byte cap before a
   provider process can start.
7. A response cannot mutate immutable manifest fields or counters. Only a
   validated typed delta can advance the state revision.
8. Invalid output never replaces the last valid state.

These are local, testable guarantees. Provider-side hidden system prompts and
provider-side context usage remain outside codeunlimited's control and are
reported as a limitation.

## Run storage

The default store is project-local:

```text
.codeunlimited/runs/<name>/
  manifest.json
  workflow.md
  state.json
  observation.txt
  lock
  attempts/
    00000001.json
  archive/
    00000001.json
  recovery.json          # present only after an ambiguous attempt
```

The directory is rejected if it, a run directory, `manifest.json`,
`workflow.md`, `state.json`, or `observation.txt` is a symlink. JSON readers use
`deny_unknown_fields` and reject unsupported schema versions. State and
manifest writes use the existing same-directory atomic-write primitive.
Attempt files are immutable and created atomically; an existing attempt number
is never overwritten.

`workflow.md` is an immutable snapshot of the supplied skill file. The source
path is retained for provenance, but later source-file edits cannot silently
change an active run. Every load verifies the snapshot against the manifest
hash.

`manifest.json` contains configuration, not secrets:

```text
schema_version, run_name, project_root, created_unix,
workflow_path, workflow_sha256, objective,
provider, provider_args, max_steps, prompt_budget_bytes,
state_budget_bytes, observation_budget_bytes, max_attempts_per_revision,
verification_command, verify_every_step, allow_unverified_completion
```

Environment variables and authentication tokens are inherited by the provider
process and are never copied into the manifest, state, journal, or output.
`provider_args` are an exact string array passed without a shell. Init rejects
secret-bearing options such as API-key, token, password, and secret arguments;
credentials must come from the provider's environment or existing auth store.

`state.json` contains the bounded coding state:

```text
schema_version, revision, status, focus, memory_summary,
queue, completed, decisions, blockers, active_files,
checks, artifacts, archive_count, archive_hash
```

Paths in `active_files` and `artifacts` are project-relative, normalized, and
cannot contain `..` or absolute prefixes. Artifacts are references with a
runtime-computed digest and short model-supplied purpose, never copied file
bodies. Attempt counters and usage totals are derived from immutable attempt
files; they cannot appear in the prompt-visible state or the model delta.

The full state has a default serialized limit of 16 KiB. The workflow defaults
to 24 KiB, the latest observation to 4 KiB, and the complete prompt to 48 KiB.
Users may lower these limits. Raising them requires explicit flags at `init`,
is recorded in the manifest, and remains subject to compiled safety ceilings:
128 KiB workflow, 64 KiB state, 32 KiB observation, and 256 KiB prompt. Provider
output is rejected above 1 MiB before JSON decoding. A provider step defaults
to a 30-minute timeout with a four-hour compiled ceiling. The manifest defaults
to 100 total provider attempts and two attempts per state revision; `auto`
requires `--steps` in the inclusive range 1–100.

Default field caps are 1 KiB for `focus`, 4 KiB for `memory_summary`, 512 bytes
for an item description, and 1 KiB for an observation or check summary. The
visible collections allow 64 queued items, 32 completed items, 32 decisions,
16 blockers, 32 active files, 16 checks, and 32 artifacts. Validation measures
UTF-8 bytes, not characters or estimated tokens.

## Typed state delta

The provider's structured final message is a `StepEnvelope`:

```json
{
  "schema_version": 1,
  "base_revision": 3,
  "outcome": "continue",
  "summary": "Added parser validation; focused tests pass.",
  "delta": {
    "focus": "Add CLI coverage",
    "memory_summary": "Parser now rejects duplicate keys...",
    "queue_replace": [{"id":"cli-tests","task":"Add CLI coverage"}],
    "completed_add": [{"id":"parser","result":"Validation implemented"}],
    "decisions_add": [],
    "blockers_replace": [],
    "active_files_replace": ["src/parser.rs", "tests/parser.rs"],
    "artifacts_add": [{"path":"docs/parser-contract.md","purpose":"Parser contract"}]
  }
}
```

`base_revision` must equal the loaded revision. Lists are either append-only or
explicit replacement fields; the provider cannot delete completed work or
rewrite previous decisions. Duplicate IDs are rejected. Strings and
collections have per-field caps in addition to the total state budget.

`outcome` is one of `continue`, `complete`, or `blocked`. Checks cannot be
claimed in the model delta. The runtime itself invokes the manifest's
shell-free verification command (`program` plus an argument array) after a
requested completion, and optionally after every step. It stores only the exit
status and a bounded output tail in `checks`. Requested completion is committed
as `running` when verification fails, and the failure becomes the next
observation. Without a configured verification command, `complete` is rejected
unless the manifest records `allow_unverified_completion`. `blocked` requires a
non-empty blocker.

When bounded lists reach their cap, the oldest detailed entries are written to
an immutable archive record and removed from the prompt-visible state only if
`memory_summary` changes in the same transition. The visible state retains
archive counts and a chain hash so loss is observable and archived details
remain locally inspectable without being replayed automatically. Archive files
contain compact state entries and therefore share the state store's privacy
classification; the attempt journal remains metadata-only.

## Prompt construction and additional savings

Prompt bytes are deterministic and ordered for prefix-cache reuse:

1. versioned runtime contract and output schema;
2. immutable workflow bytes;
3. immutable objective and project constraints;
4. canonical minified current-state JSON;
5. latest bounded observation;
6. one short instruction to perform exactly one bounded work increment.

The large stable prefix comes first; dynamic state comes last. The compiler
normalizes line endings and uses canonical key ordering so identical immutable
content produces identical bytes across steps. It reports byte counts and
SHA-256 digests for the stable prefix, dynamic suffix, and complete prompt.
Byte counts are not presented as exact tokenizer counts.

Version 2.0 adds four savings mechanisms beyond simple transcript removal:

- **two-tier memory:** compact hot state plus content-addressed artifact
  references to cold files;
- **typed deltas:** the model returns changes instead of repeating the complete
  next state;
- **cache-aligned prompts:** immutable bytes precede the changing suffix;
- **hard context admission:** an over-budget prompt fails locally before a
  provider request can spend tokens.

The runtime never performs automatic semantic truncation. If state cannot fit,
it stops with a diagnostic identifying the oversized fields. Silent truncation
could discard the sufficient statistic on which task quality depends.

## Provider adapters

### Claude

The built-in adapter invokes the locally installed `claude` executable with
`--print`, `--no-session-persistence`, `--output-format json`, and
`--json-schema`. The prompt is supplied through standard input. The adapter
extracts the structured `StepEnvelope`, exit status, and usage counters when
the installed CLI provides them. It does not use `--bare` by default because
that changes authentication and project configuration behavior.

### Codex

The built-in adapter invokes `codex exec --ephemeral`, supplies a generated
`--output-schema` file, and reads the structured last message from an explicit
temporary output path. The prompt is supplied through standard input and the
working directory is the manifest's project root. JSONL usage events are
parsed when available.

### Command

The command adapter is the stable extension point and the deterministic test
seam. The executable receives one prompt on standard input and must write one
`StepEnvelope` JSON object to standard output. Arguments are passed as an array,
never through a shell. Because codeunlimited cannot inspect an arbitrary
driver's internal persistence, status labels it `external-process isolation`
rather than `verified ephemeral provider`.

No adapter automatically enables dangerous permission-bypass flags. Users may
provide provider arguments explicitly, except for session-continuation flags,
required-flag overrides, and secret-bearing arguments.

## Attempts, accounting, and recovery

Before launching a provider, the runtime records the current state revision,
prompt byte counts and hashes, workflow hash, provider name, and a lightweight
Git snapshot (`HEAD` plus porcelain-status hash when Git is available). After
the process exits it records duration, exit code, response byte count, parsed
provider token counters, state hashes, and the post-step Git snapshot. Raw
prompts, model responses, source contents, environment variables, and tool
transcripts are not written to the attempt journal.

The provider may change repository files before returning invalid JSON or
exiting unsuccessfully. Codeunlimited must never pretend this is atomic. If
the pre/post Git snapshot differs and no valid state transition was committed,
the run enters `needs_recovery`, writes `recovery.json`, and refuses further
steps. It does not revert, reset, or delete repository changes.

`run recover --observation FILE` requires explicit user-supplied bounded text
describing the accepted repository state, removes `recovery.json`, and advances
the state revision with only a new observation. It does not fabricate a
successful model delta. If the repository did not change, a failed attempt
leaves state byte-identical and permits a manual retry up to the manifest's
`max_attempts_per_revision`. `auto` never performs that retry automatically.

Provider-reported counters are stored exactly with source labels. Missing
counters stay `null`, never zero. The status output separately reports:

- deterministic prompt bytes sent by codeunlimited;
- exact provider counters when available;
- number of successful, failed, and recovery-required attempts;
- stable-prefix reuse opportunity, without claiming provider cache hits.

## Concurrency and failure handling

A run lock is held for the complete `step` transaction, including provider
execution, preventing two workers from modifying the same run concurrently.
Read-only status reports that a run is busy rather than blocking indefinitely.

Expected local failures have distinct non-zero exits: missing provider,
invalid manifest/state, over-budget prompt, timeout, provider failure, invalid
envelope, stale revision, invalid transition, and recovery required. Diagnostic
messages may include run names and project-relative paths, but never prompt or
response bodies. A timeout terminates the child, waits for it, captures the
post-step Git snapshot, and follows the same recovery rule as any ambiguous
failure.

`auto` stops on the first failed attempt. It cannot retry an ambiguous attempt
or a validation failure that followed repository changes.

## Security and privacy

The Rust binary continues to contain no HTTP client. Only the explicitly
selected provider executable can access the network. Child processes receive
the project root as their working directory and inherit the user's provider
configuration and operating-system environment.

Provider executables and passthrough arguments are shown by `run status` with
secret-looking argument values redacted. Provider output is size-limited before
JSON decoding. Temporary schema/output files use private permissions where the
platform supports them and are removed after the step.

Unlike the observation plane, execution state necessarily contains user-authored
objectives, compact work summaries, checks, and project-relative paths. The
documentation makes this boundary explicit. These files stay local unless the
user commits or otherwise shares `.codeunlimited/`.

## Local verification strategy

All implementation behavior is test-driven and requires no model calls:

- model tests cover strict deserialization, field caps, ID uniqueness,
  append/replace semantics, completion guards, archive rules, and stale
  revisions;
- prompt golden tests prove deterministic ordering, canonical JSON, stable
  prefix hashes, absence of previous transcript text, and hard budget refusal;
- store tests cover symlink rejection, corrupt-state preservation, atomic
  commits, immutable attempts, locking, and recovery persistence;
- provider tests use fixture executables to capture stdin, inspect arguments,
  emit valid/invalid/oversized responses, time out, and simulate repository
  changes;
- CLI integration tests cover init/status/prompt/step/auto/recover lifecycle,
  bounded loops, refusal to continue sessions, and byte-identical state after
  failed validation;
- existing Rust and Python suites remain green;
- release checks require all metadata and documentation to agree on `2.0.0`.

Tests may count bytes, processes, files, and supplied fixture counters. They
must not describe those values as realized token savings.

## Release and migration

The Rust package version becomes `2.0.0`. Existing detector configuration,
baselines, reports, experiment ledgers, instruction blocks, and the legacy
Python reference remain readable. There is no automatic migration into a run.

README and SECURITY gain a clear observation-plane versus execution-plane
split. The existing negative and observational evidence remains published.
The postponed paid session-policy draft remains on its research branch, is not
a prerequisite for 2.0, and will be superseded by this architecture when causal
testing resumes.

`.codeunlimited/runs/` is not added to a project's `.gitignore` automatically.
`run init` prints the exact recommended ignore entry. This avoids silently
editing user-owned ignore rules.

## Acceptance criteria

- A locally tested provider invocation cannot receive a previous transcript or
  a resumable session identifier through any built-in adapter.
- Repeated prompt rendering with unchanged workflow/state/observation is
  byte-identical; only the dynamic suffix changes after a valid step.
- Every provider call is rejected locally when the configured context budget
  would be exceeded.
- A valid typed delta advances exactly one revision through an atomic state
  replacement; malformed, stale, or invalid deltas preserve the previous
  bytes.
- Concurrent steps for one run cannot both invoke providers.
- Repository changes plus an uncommitted state transition force explicit
  recovery and are never automatically reverted.
- `auto` always has an explicit finite step limit and stops on complete,
  blocked, failure, or recovery.
- Status distinguishes prompt bytes, provider-reported tokens, and unknown
  usage; no value is relabeled as measured savings.
- Claude, Codex, and command adapter contracts are covered by local fixture
  tests without live API calls.
- All legacy commands and persisted v1.x data remain compatible.
- Release-facing metadata, package tests, README, SECURITY, and changelog agree
  on version `2.0.0` and the new network/privacy boundary.

## Non-goals

- Running a paid token-savings experiment or claiming a 5x result.
- Reimplementing the Claude Code or Codex tool sandbox inside codeunlimited.
- Guaranteeing that a provider's hidden system prompt or internal per-step
  transcript is bounded.
- Silently summarizing or truncating over-budget state.
- Automatically reverting Git or filesystem changes after provider failure.
- Multi-agent state merging or concurrent writers to one run.
- A hosted service, telemetry, API-key storage, or a built-in HTTP client.
- Supporting arbitrary domain schemas in 2.0; the first schema is deliberately
  optimized for repository coding work.
