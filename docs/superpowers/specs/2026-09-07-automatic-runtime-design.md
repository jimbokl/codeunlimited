# Two-level subscription automation — approved design

User approved the two-level design in chat on 2026-09-07. Deliver working local activation, not a claim that the desktop app is intercepted.

## Behavior
1. Ordinary Codex keeps its native compaction. An opt-in `setup --autopilot` installs a separately owned, reversible compaction policy: `model_auto_compact_token_limit = 64000` and `model_auto_compact_token_limit_scope = "body_after_prefix"`. This is an initial growth budget, not a proven optimum. Never override either user-owned key or combine an owned half-policy with a conflicting user half-policy. Report actual configured values and ownership.
2. Install a separately owned autopilot instruction block for Claude and Codex, including the active Codex override if present. For already-authorized substantial multi-step project work, the host agent should launch the bounded subscription runtime without repeated setup requests. It must preserve the chosen model/effort, permissions, integrations, objective and acceptance checks. It must not launch runtime for conversation, read-only status, trivial tasks, or recursively from a worker. This is agent-instruction routing, not desktop interception.
3. Add `run start` as the one-command subscription-only initializer plus automatic finite execution. It supplies a small built-in worker workflow unless a skill is supplied, requires a verification executable, uses standard integrations, and retains existing state validation, immutable attempt ledger, exclusive locking and recovery gates. Start defaults: provider codex, six attempts, one-million observed input+output token soft admission budget, 600-second per-process timeout. Expose smaller caller budgets; reject zero/out-of-range input before creating a run. The invocation itself authorizes the bounded work. No shell argv interpolation.
4. Never silently overwrite/reinitialize an existing run. Continuing a run uses the existing `run auto` only after reading its status, and never blindly retries an ambiguous dispatch. Completion, blocked, failed checks and budgets terminate a batch. Token limits cannot preempt provider-internal usage and are explicitly soft.
5. Worker workflow makes the runtime-worker boundary explicit; never recursively launch another codeunlimited run. The provider process sets CODEUNLIMITED_RUNTIME_WORKER=1; provider-invoking start/step/auto/cache-probe reject this marker before mutation or dispatch. Read-only status/ledger/packet remain usable. Automatically persisted validated state is the checkpoint. No transcript edits, UI automation, detached infinite watcher, account workarounds, or forced restart of the current desktop chat.
6. Status distinguishes installed policy, native compaction configuration, runtime available versus active managed run, and unsupported transparent desktop takeover. Preserve offline monitor independently.
7. New Start runs are private-by-default: place an owned ignore-all .gitignore inside the newly created run directory before worker dispatch, without changing the user root .gitignore or existing run policy. Verify with real git check-ignore/status so state is not accidentally staged.

## Global constraints
- Rust 1.82, existing dependencies, existing release metadata unchanged.
- Subscription providers only for the new convenience command: codex and claude; standard integrations only.
- No new permissions, disabled integrations, model/effort downgrades, external paid API, or real model experiments during verification.
- Existing setup defaults remain backwards-compatible; autopilot is explicit opt-in, idempotent, reversible and backup-preserving.
- User-edited owned blocks fail closed before writes. User-owned settings remain byte-preserved.
- No context deletion or claim of universal savings, quota savings, lossless compaction, automatic desktop interception or active runtime without evidence.
- Full work stays on codex/automatic-runtime; no push, merge or release publication this turn.

## Verification
Use isolated provider configuration roots and actual CLI subprocesses. Fake only the external provider boundary; run the real state engine, verifier and ledger. Cover start completion, exhausted budget, failed verification, duplicate run, blocked result, invalid input, ambiguous recovery, and no recursive dispatch. Verify installed Codex 0.153.4 accepts the native compaction fields via read-only config RPC. Validate a no-model local activation and dry/read-only state inspection; do not launch unbounded work.

## Existing baseline issue
At unmodified d158a45, full cargo test failed in tests/runtime_ledger.rs:189 (snapshot_regular_files metadata NotFound). The intent test observes a live run immediately when its intent appears, while preparation still atomically replaces files, and relies on a one-second worker sleep. A focused rerun passed. Replace that test's timing assumption with a bounded provider-ready/release handshake; do not weaken product checks or simply lengthen sleep. Cover only this named flaky fixture in this change.
