# codeunlimited 2.0 Stateful Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a provider-neutral stateful orchestration runtime that launches a fresh Claude, Codex, or command worker for every bounded coding step and carries only validated compact state between steps.

**Architecture:** Add a focused `runtime` library subsystem containing strict models, validation, atomic storage, deterministic prompt compilation, process adapters, and a lock-serialized engine. Expose it through `codeunlimited run init|status|prompt|step|auto|recover`; preserve all v1.x observation commands and verify every provider path with local fixture processes rather than paid model calls.

**Tech Stack:** Rust 2021 / Rust 1.82, clap, serde/serde_json, fs2, tempfile, sha2, wait-timeout; existing Rust integration-test stack; Python 3.10+ only for existing release checks.

**Spec:** `docs/superpowers/specs/2026-09-04-v2-stateful-runtime-design.md`

## Global Constraints

- No live Claude, Codex, or other model request may run during implementation or verification.
- Each production behavior must have a witnessed failing test before its implementation.
- Built-in providers must start a new process for every step and reject every session-continuation flag.
- Prompt compilation accepts only the immutable workflow snapshot, manifest objective, current state, and latest observation.
- State transitions are strict, revision-checked, bounded, and atomically replace only the last valid state.
- Provider arguments are arrays passed directly to `Command`; no user string is evaluated by a shell.
- Repository changes are never reset, reverted, deleted, or silently accepted after an ambiguous provider failure.
- Existing v1.x commands and persisted formats remain compatible.
- Rust 1.82 remains the MSRV.
- Release-facing metadata must agree on `2.0.0`.

---

### Task 1: Define the strict runtime model and transition validator

**Files:**
- Create: `src/runtime/mod.rs`
- Create: `src/runtime/model.rs`
- Create: `src/runtime/validate.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`, `Cargo.lock`
- Test: `src/runtime/validate.rs`

**Interfaces:**
- Produces: `Manifest`, `ProviderConfig`, `VerificationCommand`, `CodingState`, `RunStatus`, `StepEnvelope`, `StepOutcome`, `StateDelta`, `Transition`, `ArchiveBatch`
- Produces: `validate_manifest(&Manifest) -> Result<(), RuntimeError>`
- Produces: `validate_state(&Manifest, &CodingState) -> Result<(), RuntimeError>`
- Produces: `apply_delta(&Manifest, &CodingState, StepEnvelope, &[ArtifactRef], Option<CheckResult>) -> Result<Transition, RuntimeError>`
- Produces constants for every default and compiled hard cap from the approved spec

- [x] **Step 1: Add failing strict-model tests**

  Add tests that deserialize an unknown state field, a stale `base_revision`,
  duplicate queue/completed IDs, an absolute or `..` artifact path, completion
  with a non-empty queue, completion without verification permission, and a
  blocked outcome without blockers. Each test must assert the exact
  `RuntimeError` variant.

  ```rust
  #[test]
  fn stale_delta_preserves_revision_contract() {
      let manifest = fixture_manifest();
      let state = fixture_state(4);
      let envelope = StepEnvelope { base_revision: 3, ..fixture_envelope() };
      assert_eq!(
          apply_delta(&manifest, &state, envelope, &[], None),
          Err(RuntimeError::StaleRevision { expected: 4, actual: 3 })
      );
  }
  ```

- [x] **Step 2: Run the focused tests and witness RED**

  Run: `cargo test runtime::validate::tests -- --nocapture`

  Expected: compilation fails because `runtime`, `Manifest`, and `apply_delta`
  do not exist.

- [x] **Step 3: Implement serialized types and exact limits**

  Define strict serde structs with `#[serde(deny_unknown_fields)]`, explicit
  schema version `1`, and the following shape:

  ```rust
  pub struct CodingState {
      pub schema_version: u64,
      pub revision: u64,
      pub status: RunStatus,
      pub focus: String,
      pub memory_summary: String,
      pub queue: Vec<WorkItem>,
      pub completed: Vec<CompletedItem>,
      pub decisions: Vec<Decision>,
      pub blockers: Vec<Blocker>,
      pub active_files: Vec<String>,
      pub checks: Vec<CheckResult>,
      pub artifacts: Vec<ArtifactRef>,
      pub archive_count: u64,
      pub archive_hash: Option<String>,
  }

  pub struct StateDelta {
      pub focus: Option<String>,
      pub memory_summary: Option<String>,
      pub queue_replace: Option<Vec<WorkItem>>,
      pub completed_add: Vec<CompletedItem>,
      pub decisions_add: Vec<Decision>,
      pub blockers_replace: Option<Vec<Blocker>>,
      pub active_files_replace: Option<Vec<String>>,
      pub artifacts_add: Vec<ArtifactCandidate>,
  }
  ```

  Add `sha2 = "0.10"` and move `wait-timeout = "0.2"` into runtime
  dependencies for later tasks. Keep dependency resolution compatible with
  Rust 1.82.

- [x] **Step 4: Implement validation and typed transitions**

  Validate names, UTF-8 byte caps, collection caps, path components, provider
  args, secret-looking flags, immutable fields, duplicate IDs, and total
  serialized state bytes. `apply_delta` must clone the old state, apply only
  typed fields, increment exactly one revision, and return archive candidates
  separately. Accept only runtime-resolved `ArtifactRef` values matching the
  model's candidates and an optional runtime-produced `CheckResult`; the model
  cannot supply either digest or check evidence.

- [x] **Step 5: Add archive and terminal-state tests**

  Add a test proving overflow archives the oldest completed/decision entries
  only when `memory_summary` changes, plus tests proving a valid `continue` and
  a valid `blocked` transition advance one revision without rewriting prior
  completed items.

- [x] **Step 6: Run GREEN and commit**

  Run: `cargo test runtime::validate::tests -- --nocapture`

  Expected: all focused model/transition tests pass.

  Commit: `feat: add bounded runtime state model`

### Task 2: Build the symlink-safe atomic run store

**Files:**
- Modify: `src/safeio.rs`
- Create: `src/runtime/store.rs`
- Modify: `src/runtime/mod.rs`
- Test: `src/safeio.rs`
- Test: `src/runtime/store.rs`

**Interfaces:**
- Produces: `safeio::atomic_create(path: &Path, bytes: &[u8]) -> io::Result<()>`
- Produces: `RunPaths::new(project_root: &Path, name: &str) -> Result<RunPaths, RuntimeError>`
- Produces: `RunStore::{create, load, try_lock, save_transition, write_attempt, write_recovery, recover}`
- Consumes: the strict model and existing `safeio::atomic_write`

- [ ] **Step 1: Add failing store tests**

  Cover first creation, duplicate creation, strict round-trip, corrupt JSON,
  unsupported schema, symlinked store/run/state/workflow/observation, immutable
  attempt collision, and two simultaneous `try_lock` calls. Assert corrupt or
  symlinked input remains byte-identical.

  ```rust
  #[test]
  fn second_lock_is_busy_without_waiting() {
      let store = fixture_store();
      let first = store.try_lock().expect("first lock");
      assert_eq!(store.try_lock().unwrap_err(), RuntimeError::RunBusy);
      drop(first);
      store.try_lock().expect("lock released");
  }
  ```

- [ ] **Step 2: Witness RED**

  Run: `cargo test runtime::store::tests -- --nocapture && cargo test safeio::tests -- --nocapture`

  Expected: compilation fails because `RunStore` and `atomic_create` are
  missing.

- [ ] **Step 3: Implement atomic create and strict path handling**

  Add `atomic_create` using a same-directory `NamedTempFile`, `sync_all`, and
  `persist_noclobber`. Reject symlinks for every security-sensitive path before
  reads or writes. Canonicalize the project root once, but never canonicalize a
  model-supplied relative path across a symlink boundary.

- [ ] **Step 4: Implement run creation and loading**

  `create` writes immutable `workflow.md`, `manifest.json`, initial
  `state.json`, and empty `observation.txt`. If a later file fails, remove only
  files created by this invocation and remove the run directory only when it
  is empty. `load` verifies strict JSON, workflow hash, and all budgets.

- [ ] **Step 5: Implement lock, attempt, archive, and recovery writes**

  Use `fs2::FileExt::try_lock_exclusive` for the complete step lifetime. Store
  attempt and archive files under zero-padded revision/attempt names using
  `atomic_create`. State/observation transitions use sibling temporary files
  and the state file is the commit point. Recovery removal happens only after
  the recovered state and observation commit successfully.

- [ ] **Step 6: Run GREEN and commit**

  Run: `cargo test runtime::store::tests -- --nocapture && cargo test safeio::tests -- --nocapture`

  Expected: all store and safe-write tests pass.

  Commit: `feat: add atomic runtime state store`

### Task 3: Compile deterministic cache-aligned bounded prompts

**Files:**
- Create: `src/runtime/prompt.rs`
- Modify: `src/runtime/mod.rs`
- Test: `src/runtime/prompt.rs`

**Interfaces:**
- Produces: `CompiledPrompt { bytes, stable_bytes, dynamic_bytes, stable_sha256, prompt_sha256 }`
- Produces: `compile_prompt(manifest: &Manifest, workflow: &[u8], state: &CodingState, observation: &[u8]) -> Result<CompiledPrompt, RuntimeError>`
- Produces: `STEP_ENVELOPE_SCHEMA_JSON: &str`

- [ ] **Step 1: Add failing prompt golden tests**

  Assert exact bytes for a small fixture, CRLF normalization, minified state,
  stable-prefix hash equality across revisions, whole-prompt hash inequality
  after a transition, exact budget boundary acceptance, one-byte overflow
  refusal, and absence of a sentinel string representing old transcript text.

  ```rust
  #[test]
  fn old_transcript_has_no_input_channel() {
      let prompt = compile_prompt(&manifest(), WORKFLOW, &state(), b"latest")
          .expect("bounded prompt");
      assert!(!String::from_utf8_lossy(&prompt.bytes).contains("OLD_TRANSCRIPT_SENTINEL"));
  }
  ```

- [ ] **Step 2: Witness RED**

  Run: `cargo test runtime::prompt::tests -- --nocapture`

  Expected: compilation fails because `compile_prompt` is missing.

- [ ] **Step 3: Implement canonical prompt sections**

  Emit fixed headings in this order: runtime contract/schema, workflow,
  objective/constraints, current state, latest observation, one-step command.
  Normalize workflow/observation line endings and serialize state with compact
  struct-order JSON. Hash the exact stable prefix and whole prompt with SHA-256.

- [ ] **Step 4: Enforce every admission budget before returning bytes**

  Reject invalid UTF-8 workflow/observation, workflow/state/observation cap
  failures, and total prompt overflow with variants that include actual and
  allowed byte counts but never include content.

- [ ] **Step 5: Run GREEN and commit**

  Run: `cargo test runtime::prompt::tests -- --nocapture`

  Expected: all deterministic prompt tests pass.

  Commit: `feat: compile bounded state prompts`

### Task 4: Add safe ephemeral process and provider adapters

**Files:**
- Create: `src/runtime/provider.rs`
- Modify: `src/runtime/mod.rs`
- Test: `src/runtime/provider.rs`
- Create: `tests/fixtures/runtime_driver.py`

**Interfaces:**
- Produces: `ProviderRunner::run(&ProviderConfig, &CompiledPrompt, &Path, Duration) -> Result<ProviderResult, ProviderFailure>`
- Produces: `build_claude_command`, `build_codex_command`, `build_external_command`
- Produces: `ProviderResult { envelope, usage, exit_code, response_bytes, duration_ms }`
- Guarantees: no shell, bounded output, timeout termination, no continuation flags

- [ ] **Step 1: Create a deterministic fixture driver and failing adapter tests**

  The Python fixture reads stdin, optionally captures it, emits a supplied
  envelope, emits oversized output, sleeps, exits non-zero, or changes a
  fixture repository based only on argv. Tests must not reference installed
  Claude/Codex binaries.

- [ ] **Step 2: Add exact command-construction tests**

  Assert Claude contains `--print --no-session-persistence --output-format
  json --json-schema`; Codex contains `exec --ephemeral --output-schema` and an
  explicit output file; command adapter starts with the configured executable.
  Assert resume/continue/session-ID, required-flag override, and secret-bearing
  arguments fail before process creation.

- [ ] **Step 3: Witness RED**

  Run: `cargo test runtime::provider::tests -- --nocapture`

  Expected: compilation fails because the provider module is absent.

- [ ] **Step 4: Implement bounded child-process execution**

  Spawn with piped stdin/stdout/stderr, write prompt bytes, drain both output
  pipes concurrently while retaining at most 1 MiB, and use `wait_timeout`.
  On timeout kill and wait for the child. Never include retained body bytes in
  a `Display` diagnostic.

- [ ] **Step 5: Implement provider parsing**

  Command accepts a direct `StepEnvelope`. Claude accepts `structured_output`
  as an object or a JSON `result` string and reads optional usage fields. Codex
  reads the schema-constrained last-message file and optional JSONL token events.
  Missing usage fields map to `None`, not zero.

- [ ] **Step 6: Run GREEN and commit**

  Run: `cargo test runtime::provider::tests -- --nocapture`

  Expected: argument, valid output, invalid output, oversized output, non-zero
  exit, and timeout tests pass with no network activity.

  Commit: `feat: add ephemeral provider adapters`

### Task 5: Implement the serialized orchestration engine and recovery

**Files:**
- Create: `src/runtime/engine.rs`
- Modify: `src/runtime/mod.rs`
- Test: `src/runtime/engine.rs`
- Modify: `tests/fixtures/runtime_driver.py`

**Interfaces:**
- Produces: `init_run(InitRequest) -> Result<RunStatusView, RuntimeError>`
- Produces: `render_next_prompt(&RunRef) -> Result<CompiledPrompt, RuntimeError>`
- Produces: `step(&RunRef, &dyn Provider) -> Result<StepReport, RuntimeError>`
- Produces: `run_steps(&RunRef, NonZeroUsize, &dyn Provider) -> Result<AutoReport, RuntimeError>`
- Produces: `recover(&RunRef, &[u8]) -> Result<RunStatusView, RuntimeError>`
- Produces: `git_snapshot(project_root: &Path) -> GitSnapshot`

- [ ] **Step 1: Add failing engine lifecycle tests**

  Cover init without provider call, one successful step, terminal complete,
  blocked, finite auto loop, max attempts, total-attempt limit, verification
  success/failure, state byte preservation after invalid delta, and status
  aggregation with unknown provider counters.

- [ ] **Step 2: Add recovery tests**

  Simulate invalid JSON with no repository change (retry permitted), invalid
  JSON after a repository change (`needs_recovery`), refusal of another step,
  bounded manual recovery, and repository-byte preservation throughout.

- [ ] **Step 3: Witness RED**

  Run: `cargo test runtime::engine::tests -- --nocapture`

  Expected: compilation fails because engine entry points are absent.

- [ ] **Step 4: Implement Git snapshots and the one-step transaction**

  Invoke Git without a shell, tolerate non-Git projects as an explicit
  unavailable snapshot, hold the run lock from load through final persistence,
  compile before spawning, snapshot before/after, parse and validate the delta,
  compute artifact digests, optionally execute verification, and commit exactly
  one revision.

- [ ] **Step 5: Implement verification and observations**

  Execute only the manifest's program-plus-argv command. Capture a bounded
  combined output tail. Runtime-generated `CheckResult` receives the post-step
  workspace hash. A failed completion check commits the useful state delta as
  `running` and sets the bounded failure tail as the next observation.

- [ ] **Step 6: Implement attempts and recovery state**

  Record hashes, byte counts, duration, status, optional usage, and Git
  snapshots without prompt/response bodies. When a failed/invalid provider
  changed the workspace, atomically write `recovery.json` before returning
  `RecoveryRequired`. `recover` advances one revision using only bounded user
  observation and never modifies project content.

- [ ] **Step 7: Implement bounded auto execution**

  Reuse `step` without a second state path. Stop on complete, blocked, first
  failure, recovery, manifest attempt limit, or requested count. Never retry a
  failed revision inside `auto`.

- [ ] **Step 8: Run GREEN and commit**

  Run: `cargo test runtime::engine::tests -- --nocapture`

  Expected: all state-machine and recovery cases pass locally.

  Commit: `feat: orchestrate bounded agent steps`

### Task 6: Expose the complete `codeunlimited run` CLI

**Files:**
- Create: `src/runtimecmd.rs`
- Modify: `src/main.rs`
- Modify: `src/lib.rs`
- Create: `tests/runtime_cli.rs`
- Test: `tests/runtime_cli.rs`

**Interfaces:**
- Adds: `run init`, `run status`, `run prompt`, `run step`, `run auto`, `run recover`
- Supports: `--json` on status/step/auto and exact provider/verification argv flags
- Maps: stable runtime error categories to non-zero process exits without content leaks

- [ ] **Step 1: Add failing CLI help and init/status tests**

  Assert all subcommands appear, `run init` performs no fixture-provider call,
  duplicate names fail, status JSON is strict/versioned, and the printed ignore
  guidance is exactly `.codeunlimited/runs/`.

- [ ] **Step 2: Add failing prompt/step/auto/recover tests**

  Use the command fixture to prove prompt is read-only, step invokes exactly
  once, auto invokes no more than `--steps`, terminal state stops early,
  missing `--steps` is a clap failure, and recovery requires an existing
  recovery record plus a bounded regular observation file.

- [ ] **Step 3: Witness RED**

  Run: `cargo test --test runtime_cli -- --nocapture`

  Expected: clap rejects the absent `run` command.

- [ ] **Step 4: Implement clap types and thin dispatch**

  Add a `RunCmd` enum and argument structs in `runtimecmd.rs`; keep `main.rs` to
  one `Cmd::Run { command } => exit(runtimecmd::run(command))` dispatch arm.
  Resolve paths once and render human/JSON results from typed engine reports.

- [ ] **Step 5: Implement exit and redaction contract**

  Use stable exit categories for invalid input, busy, provider failure,
  over-budget, invalid transition, and recovery required. Redact values after
  secret-looking flags in status output even though new manifests reject them.
  Never print raw provider stdout/stderr or compiled prompts except the explicit
  local `run prompt` command.

- [ ] **Step 6: Run GREEN and commit**

  Run: `cargo test --test runtime_cli -- --nocapture`

  Expected: complete local CLI lifecycle passes without live providers.

  Commit: `feat: expose stateful run commands`

### Task 7: Harden cross-platform and failure boundaries

**Files:**
- Modify only: `src/runtime/*.rs`, `src/runtimecmd.rs`, `tests/runtime_cli.rs`, `tests/fixtures/runtime_driver.py`
- Test: all runtime unit and integration tests

**Interfaces:**
- Preserves all interfaces from Tasks 1–6
- Adds no new product feature; closes review findings only

- [ ] **Step 1: Add regression tests for boundary failures**

  Cover non-UTF-8 workflow/observation, read-only store, deleted provider,
  executable path with spaces, Unicode objective byte limits, stdout/stderr
  flood, child timeout cleanup, provider args beginning with hyphens, Git
  unavailable, detached HEAD, untracked-file changes, archive collisions, and
  post-provider state-write failure.

- [ ] **Step 2: Witness RED for every discovered gap**

  Run each new exact test before its fix and record the expected failure in the
  commit notes. Do not change production code for a test that is already green.

- [ ] **Step 3: Apply minimal fixes and keep diagnostics content-free**

  Change only the responsible unit. Any error type added here must expose
  category plus counts/paths, never workflow, prompt, state, observation, or
  provider output bodies.

- [ ] **Step 4: Run the complete runtime suite and commit**

  Run: `cargo test runtime -- --nocapture && cargo test --test runtime_cli -- --nocapture`

  Expected: all runtime tests pass on the local platform.

  Commit: `fix: harden stateful runtime boundaries`

### Task 8: Publish the honest 2.0 release surface

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`
- Modify: `README.md`, `SECURITY.md`, `CHANGELOG.md`, `ROADMAP.md`
- Modify: `docs/EVIDENCE-VERDICT.md`, `docs/BENCHMARKING.md`
- Modify: `scripts/check_release.py`
- Modify: `tests/release_metadata.rs`, `tests/test_release_tooling.py`
- Create: `docs/RUNTIME.md`

**Interfaces:**
- Package version: `2.0.0`
- Public distinction: observation plane / execution plane / provider network boundary
- Evidence claim: locally proven bounded context transport, no realized savings claim yet

- [ ] **Step 1: Add failing release metadata and claim tests**

  Expect version `2.0.0`, a runtime documentation link, the execution-plane
  privacy disclosure, and absence of universal `5x`, fixed-percent, or
  per-tool-action bounded-context claims.

- [ ] **Step 2: Witness RED**

  Run: `cargo test --test release_metadata -- --nocapture && python3 -m unittest -v tests.test_release_tooling`

  Expected: version and documentation assertions fail against 1.9 metadata.

- [ ] **Step 3: Update version and documentation**

  Bump Cargo manifest/lock and release checker to `2.0.0`. Document exact CLI
  workflows, state format, provider isolation levels, budgets, verification,
  recovery, `.gitignore` guidance, and the fact that provider commands may use
  network/auth while legacy audit commands remain offline.

- [ ] **Step 4: Preserve evidence honesty**

  State that local tests prove prompt construction and history exclusion at
  orchestration boundaries, not token savings. Keep the paid experiment
  postponed and describe the future comparison as full-history versus the 2.0
  runtime at matched task quality and increasing horizons.

- [ ] **Step 5: Run release checks and commit**

  Run: `python3 scripts/check_release.py --expected 2.0.0`

  Run: `bash scripts/check_release.sh 2.0`

  Expected: both release metadata checks pass.

  Commit: `release: prepare codeunlimited 2.0.0`

### Task 9: Complete the local release gate and branch handoff

**Files:**
- Modify only files required by a witnessed failing regression test

**Interfaces:**
- Branch: `codex/v2-state-runtime`
- No release tag or merge is created without a separate explicit publication decision

- [ ] **Step 1: Map every spec acceptance criterion to evidence**

  Add a checklist to the implementation notes identifying the exact test or
  command for each criterion. Fix uncovered criteria test-first.

- [ ] **Step 2: Run format and lint gates**

  Run: `cargo fmt --all -- --check`

  Run: `cargo clippy --all-targets --locked -- -D warnings`

- [ ] **Step 3: Run all Rust and Python tests**

  Run: `cargo test --release --all-targets --locked`

  Run: `python3 -m unittest discover -s tests -p 'test_*.py' -v`

- [ ] **Step 4: Run compatibility and package gates**

  Run: `cargo +1.82.0 check --all-targets --locked` when installed.

  Run: `bash scripts/check_release.sh 2.0`

  Run: `cargo package --locked`

- [ ] **Step 5: Inspect the complete change set**

  Run: `git diff --check`

  Run: `git status --short`

  Run: `git diff --stat main...HEAD`

  Review all `main...HEAD` changes for accidental secrets, prompt/response
  persistence, shell invocation, destructive Git operations, unsupported
  savings claims, and unrelated edits.

- [ ] **Step 6: Push the implementation branch**

  Push `codex/v2-state-runtime` and report the branch/commit plus every local
  verification result. Do not run a live provider, create a tag, or merge as
  part of this plan.
