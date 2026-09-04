# Version 2.1: subscription-first orchestration

The main product path remains Claude Code and Codex using the user's existing
subscription authentication. Stable workflow instructions are separated from
changing state. Claude receives an additive instruction file; Codex receives a
constant bootstrap ordering file reads without replacing its built-in
instructions. Both start fresh workers at bounded orchestration boundaries.

`standard` preserves integrations and remains the default. Opt-in `lean`
reduces integrations and can remove tools needed by a task. It does not choose
a cheaper model or lower reasoning effort. Existing `audit`, `delta`, `report`,
and experiment-accounting commands remain offline.

External APIs are a separate optional layer: OpenAI Responses and Anthropic
Messages with explicit cache configuration and environment-only keys. These
adapters have no repository tools in 2.1. They serve state/planning workflows;
they are not replacements for the subscription coding agents.

## What the evidence says

Local fixtures establish prompt separation, revision-invariant stable bytes,
provider flags, usage arithmetic, legacy inspection, strict state transitions,
and API request/response handling. A loopback lifecycle covers init, step,
status, two cache-probe samples, and a failed response whose reported usage
remains counted. No live model calls are part of the release test suite.

These checks do not measure realized token savings, recovered subscription
quota, model quality, or live provider cache retention. No percentage is claimed
for 2.1. Historical experiments remain in [the evidence verdict](EVIDENCE-VERDICT.md)
and must not be relabeled as results of this release.

`cache_read_ratio_basis_points` is the share of reported input served from
cache. It is not saved tokens: cached tokens still count as transported input.
The explicit `cache-probe` command costs two calls, uses reduced integrations,
and reports counters separately from work attempts. Even a positive result
does not attribute the cache read to our workflow instead of the provider's
existing prefix. A paid increasing-horizon, quality-matched benchmark remains
the next evidence milestone; it was intentionally not run for this release.

## Tradeoffs and limits

- Fresh workers repeat boot overhead. Small tasks may cost more; select useful
  work increments instead of restarting after every trivial tool action.
- State maintenance can lose information if the agent fails to retain it.
  Typed evidence and monotonic knowledge states constrain this risk but do not
  prove task-level completeness. Keep artifacts retrievable and use verification.
- Bounds apply between workers. Tool histories inside one CLI worker remain
  controlled by that CLI. Codex's ordered-read bootstrap is not enforced cache
  placement and may add file-read calls.
- Lean removes MCP/Chrome/slash integrations for Claude or user configuration
  for Codex. Choose standard when those capabilities matter.
- API model/cache/schema compatibility is provider-specific; init validates
  local configuration without a paid connectivity check. API calls are
  non-streaming, capped, and not automatically retried.
- Old unknown usage remains unknown. API failures without readable usage
  cannot be reconstructed as zero. Interrupted probes may consume usage without
  producing a complete two-sample report.

See [runtime commands and accounting](RUNTIME.md), [security boundaries](../SECURITY.md),
and [saved research corrections / deferred Gemini work](research/2026-09-04-context-saving-inputs.md).
