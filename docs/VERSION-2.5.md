# Version 2.5: bounded subscription autopilot (opt-in)

Version 2.5 adds `run start`: one command that initializes a bounded run and
drives it through the stateful runtime against a subscription CLI, with the
attempt ledger, verification, and admission caps from 2.2 applied on every
step. It is the two-level automation the runtime was built toward: the outer
loop is deterministic Rust, the inner worker is a fresh bounded provider
process per step.

Nothing starts by itself. `setup --autopilot` is an explicit opt-in flag that
installs routing instructions and a native Codex compaction budget; the
default installers do not enable it, do not launch providers, and 2.4
behavior is unchanged unless the flag is used.

## Boundaries the tests enforce

- Invalid inputs are rejected before any run directory is created or any
  provider invoked.
- A failed or blocked verification stops the run; the soft total-token budget
  stops admission of further workers; provider failure, unknown usage, and
  ambiguous output all terminate finitely and stay visible in the ledger.
- Workers carry a runtime marker and cannot dispatch runs themselves -
  read-only inspection stays available to them. A generic captured process
  (such as a verifier) is not marked as a runtime worker.
- Duplicate `run start` preserves the existing run without a worker call.
- Managed autopilot instruction blocks refuse replacement when the user
  edited them; removal restores prior bytes.

## What this release does not claim

No realized token-savings percentage, no model-adherence guarantee, and no
change to the evidence status of earlier releases. The autopilot consumes the
user's own subscription quota under explicit caps; measuring what it saves
against a native full-history agent remains the pre-registered experiment
milestone.
