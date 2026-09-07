# Autopilot: native compaction plus bounded subscription runs

Autopilot is an explicit setup option:

```bash
codeunlimited setup --autopilot
codeunlimited setup --status --json
```

It installs two separate, reversible layers. It does not invoke a provider,
restart the current desktop conversation, or claim that a managed run is active.
Setup status does not inspect a project, so `active_managed_run` is `null` with
`inspection` set to `not_inspected`; use `run status` for a named project run.

## Layer 1: native Codex compaction

When neither setting is already user-owned, setup adds this owned block to the
global Codex configuration:

```toml
model_auto_compact_token_limit = 64000
model_auto_compact_token_limit_scope = "body_after_prefix"
```

The 64,000-token value is an initial growth budget, not a measured optimum.
Ordinary Codex conversations continue to use Codex's native compaction. If
either key already exists outside the owned block, setup preserves the user's
configuration bytes and adds neither missing half nor an override. Status
reports the configured threshold, scope, and whether ownership is
`codeunlimited`, `user`, or the provider `default`.

These fields were checked through the read-only configuration RPC of Codex CLI
0.153.4 on macOS. Older or newer Codex versions and other platforms can differ;
inspect `codeunlimited setup --status --json` and the provider's current config
support before relying on them.

## Layer 2: host-agent routing

Setup adds a separately owned instruction block to Claude and Codex global
instructions, including a non-empty active Codex override. For already-
authorized, substantial multi-step coding with a deterministic verifier, the
host agent may prepare a bounded workflow and invoke `codeunlimited run start`
without another setup exchange. It must preserve the objective, checks,
permissions, integrations, and explicit model or effort settings supported by
the provider arguments.

Ordinary conversation, read-only review or status work, and trivial tasks stay
in the normal chat. This routing is guidance consumed by a host agent. It is
not transparent desktop interception, UI automation, or an OS-level guarantee
that every eligible task is routed.

## Bounded runtime behavior

`run start` accepts only the Codex and Claude subscription CLIs. It requires a
verification executable, uses the standard integration profile, and defaults
to six attempts, a 1,000,000 reported-token soft admission budget, and a
600-second timeout for each provider process. Callers can select smaller
positive values. Arguments are passed as exact argv entries; no shell command
interpolation is used.

Each worker starts with cold provider context plus the immutable workflow,
objective, current validated state, and latest bounded observation. Fresh
processes avoid transporting old orchestration transcripts, but their native
system prompt, tool schemas, repository discovery, and cache misses still have
a cold-context cost. No fixed or universal savings are claimed.

Validated state is the automatic checkpoint: it retains the objective,
completed and remaining work, decisions, evidence, verification results, and
next step. Verification runs after every successful response. Completion,
blocked state, failed checks, attempt exhaustion, unknown accounting, or a
reached token boundary ends the finite batch. The token boundary is soft: an
already-running provider cannot be preempted at an exact internal token count
and may overshoot before the next admission check.

An ambiguous dispatch is never retried blindly. Inspect `run status` and
`run ledger`, then use the existing explicit recovery workflow. Continuing a
non-terminal run uses `run auto` only after inspection; a duplicate `run start`
never resumes or replaces it. Provider processes carry a runtime-worker marker,
and provider-dispatching runtime commands reject recursive execution.

New `run start` checkpoints contain a run-local `.gitignore` with `*`, so they
stay private from a normal `git add`. Manual `run init` retains its existing
sharing policy and may need `.codeunlimited/runs/` in the project ignore file.

## Removal and compatibility

```bash
codeunlimited setup --remove
```

Removal deletes only the owned ordinary and autopilot instruction/config
blocks. User bytes and first backups remain. An edited owned block fails closed
so setup cannot silently discard a user's changes; remove only the marker lines
to adopt those contents as user-owned, then retry. Existing runtime directories
and the independent offline monitor are not removed.

Autopilot uses the installed provider CLI and existing subscription login. It
adds no API key, paid API transport, daemon, transcript editing, permission
change, model downgrade, integration disablement, or forced desktop restart.
