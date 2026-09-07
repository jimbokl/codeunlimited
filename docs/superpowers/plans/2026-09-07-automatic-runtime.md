# Two-level Automatic Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Enable reversible native compaction plus host-routed finite subscription execution and verify local activation.

**Architecture:** Extend setup with independently owned autopilot policy/config blocks. Add one subscription convenience command over the existing state engine; do not build a second engine or take over the desktop app.

**Tech Stack:** Rust 1.82, clap, serde, toml, existing process runtime and Rust integration tests.

**Spec:** docs/superpowers/specs/2026-09-07-automatic-runtime-design.md

## Global Constraints
- Rust 1.82, existing dependencies, existing release metadata unchanged.
- Subscription providers only for the new convenience command: codex and claude; standard integrations only.
- No new permissions, disabled integrations, model/effort downgrades, external paid API, or real model experiments during verification.
- Existing setup defaults remain backwards-compatible; autopilot is explicit opt-in, idempotent, reversible and backup-preserving.
- User-edited owned blocks fail closed before writes. User-owned settings remain byte-preserved.
- No context deletion or claim of universal savings, quota savings, lossless compaction, automatic desktop interception or active runtime without evidence.
- Full work stays on codex/automatic-runtime; no push, merge or release publication this turn.

### Task 1: Integrated native policy and bounded subscription entry point

**Files:**
- Modify: src/main.rs (setup flag).
- Modify: src/setupcmd.rs; create src/autopilot.rs only if needed to keep owned policy/config logic focused; wire module in src/lib.rs if created.
- Modify: src/runtimecmd.rs (Start command reusing engine).
- Modify: src/runtime/provider.rs (set CODEUNLIMITED_RUNTIME_WORKER=1 for child processes; guard provider-invoking CLI commands in runtimecmd before mutation).
- Modify: tests/setup_cli.rs; create tests/runtime_start.rs.
- Modify: tests/runtime_ledger.rs and tests/fixtures/runtime_driver.py only to replace the named live-snapshot race with readiness/release.
- Modify: README.md and docs/RUNTIME.md; create docs/AUTOPILOT.md.
- Do not edit user home, installed binary, project configuration, unrelated source, release version metadata or global skills.

**Interfaces:**
- Consumes existing setup safe I/O and owned marker safeguards; existing runtime InitRequest/init_run/run_steps/RunRef/ProviderConfig and verify/ledger/recovery contracts.
- Produces `setup --autopilot` and additive `setup --status --json` fields, and `run start NAME --objective TEXT --verify-program PROGRAM [--verify-arg ARG] [--provider codex|claude] [--skill FILE] [--provider-executable PROGRAM] [--provider-arg ARG] [--max-steps N] [--max-total-tokens N] [--provider-timeout-seconds N] [--project PATH] [--json]`.
- Defaults: six attempts, 1_000_000 soft tokens, 600 seconds, standard subscription profile. Require verification and run it each step; no unverified-completion escape in Start.
- `setup --autopilot` installs both layers of instructions and config only, never invokes a provider. Plain setup retains an already-installed autopilot policy. `setup --remove` removes owned autopilot and ordinary setup blocks while preserving user bytes.
- Native config is 64_000 tokens counted as body_after_prefix. If either key already exists outside our block, preserve both user keys, install no half-policy, report user-managed/default values.
- Status declares `autopilot.enabled`, `autopilot.routing = "host_agent_instructions"`, `autopilot.desktop_interception = false`; compaction exposes configured threshold/scope/ownership and a runtime statement that setup does not mean a managed run is active.
- Before a new Start worker dispatch, safely write a private ignore-all .gitignore within its newly created run directory; preserve root .gitignore and existing runs. Test real git check-ignore/status, not just file text.

- [ ] **Step 1: Write RED setup behavior tests.**
Use existing `command(root)` subprocess helper, compare parsed TOML and actual file preservation rather than source-code strings:
```rust
#[test]
fn autopilot_installs_native_growth_budget_and_is_reversible() {
    let t = TempDir::new().unwrap();
    write(t.path(), "codex/config.toml", "model = \"chosen-model\"\n");
    command(t.path()).arg("--autopilot").assert().success();
    let text = read(t.path(), "codex/config.toml");
    let config: toml::Value = text.parse().unwrap();
    assert_eq!(config["model"].as_str(), Some("chosen-model"));
    assert_eq!(config["model_auto_compact_token_limit"].as_integer(), Some(64000));
    assert_eq!(config["model_auto_compact_token_limit_scope"].as_str(), Some("body_after_prefix"));
    command(t.path()).arg("--autopilot").assert().success();
    assert_eq!(read(t.path(), "codex/config.toml"), text);
    command(t.path()).arg("--remove").assert().success();
    assert_eq!(read(t.path(), "codex/config.toml"), "model = \"chosen-model\"\n");
}
```
Add cases for conflicting user threshold or scope, edited managed block, invalid config/symlink preflight, CRLF/BOM, override instructions, status no writes, plain setup preserving opt-in and default setup not enabling autopilot.
Run `cargo test --locked --test setup_cli autopilot`, record expected unknown-flag RED.

- [ ] **Step 2: Implement owned configuration and instruction lifecycle.**
Use the existing owned-range/safeio preflight/update mechanism; no wholesale TOML serialization. Add a clap bool and pass it into setup. Use a separate marker pair for autopilot so legacy removal ownership stays valid. Autopilot prose: for authorized multi-step scoped coding with a deterministic verifier, the host may prepare a bounded workflow/plan and invoke Start automatically; never for ordinary chat/read-only work; never inside an explicit runtime worker; retain explicit current model and effort via supported provider args, otherwise do not silently switch; checkpoints contain objective, completed/remaining, decisions, evidence and next step. Never clear the live desktop thread. Run focused tests.

- [ ] **Step 3: Write RED Start behavioral tests.**
Assert new CLI rejects no-verifier, API provider, zero budgets and duplicate names without worker calls. Exercise an actual fixture executable speaking the selected built-in provider protocol, not a mock of the engine. Example CLI expectations:
```rust
Command::cargo_bin("codeunlimited").unwrap()
    .args(["run", "start", "missing-check", "--objective", "bounded fixture"])
    .assert().failure();
Command::cargo_bin("codeunlimited").unwrap()
    .args(["run", "start", "bad-provider", "--objective", "fixture",
           "--verify-program", "cargo", "--provider", "openai-api"])
    .assert().failure();
```
For valid local fixture runs assert terminal state, real verifier outcome, bounded attempt count, ledger totals/unknown coverage, and unchanged state on duplicate Start. Cover failed verification, blocked worker, budget exhaustion, malformed response requiring recovery, and finite failure handling. Record RED unknown-subcommand before implementation.

- [ ] **Step 4: Implement Start over existing validated engine.**
Define a narrow Start args type, translate to InitRequest with standard Codex/Claude ProviderConfig only, validate all input before init. Snapshot an optional workflow or a built-in bounded workflow; use a temp file only if the existing init API requires a path. Built-in workflow must explicitly say it is a runtime worker and never launch a nested run; it must preserve the declared objective and verify gate. Set CODEUNLIMITED_RUNTIME_WORKER=1 in provider child processes and reject provider-invoking start/step/auto/cache-probe from marked workers before mutation or dispatch; retain read-only commands. Test actual fixture-observed environment and CLI refusal without run creation. Invoke init_run then existing run_steps for the finite cap, return existing classified exit codes and truthful JSON. No second state loop, no new retry policy, no silent existing-run resume, no detached daemon. Run focused tests and existing runtime CLI/packets/ledger tests.

- [ ] **Step 5: Stabilize the observed live-ledger fixture race.**
In tests/fixtures/runtime_driver.py add a bounded test-only ready/release handshake. The live-ledger test must wait for provider readiness (all engine preparation complete), inspect an unchanged live run, then release and join the worker. Keep timeout and cleanup to avoid orphan fixtures on assertion failure. Do not ignore NotFound broadly or alter production ledger semantics. Run the named test then runtime_ledger.

- [ ] **Step 6: Document, verify, commit, report.**
Describe native compaction versus runtime, automatic host routing versus unsupported desktop interception, cold context costs, soft budget overshoot, checkpoint/verification gates, uninstall and platform/version compatibility (scope supported by inspected Codex 0.153.4). Explain that the user can keep ordinary chat while substantial approved execution routes to Start, but instructions are not an OS-level guarantee. Do not claim measured savings.
Run `cargo fmt --check`, focused integration tests and full `cargo test --locked --quiet` once after integration, `cargo clippy --locked --all-targets -- -D warnings`; save verbose logs and return exact exits. Commit only owned changes; write task report with RED/GREEN evidence. Controller owns independent review and local installation.
