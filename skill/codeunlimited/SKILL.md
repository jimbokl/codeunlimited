---
name: codeunlimited
description: Audit estimated subscription-limit leaks, apply efficiency fixes, and report observed before/after trends using the codeunlimited CLI. Use when the user asks about token usage, hitting limits, or wants more work out of Claude Code.
---

# codeunlimited - more work from the same limit

You have the `codeunlimited` CLI available (if not: `cargo install codeunlimited`
or download a binary from https://github.com/jimbokl/codeunlimited/releases).

## Flow

1. **Audit first.** Run `codeunlimited audit --project .` (or without
   `--project` for the whole machine) via Bash. Summarize the findings for
   the user in 3-5 lines: the top leaks, the reclaimable share of weekly
   volume, and the limit forecast if present. Do not paste the whole report.
2. **Offer the fixes.** If the audit found leaks, show what
   `codeunlimited fix .` would change (it's a dry run by default) and ask
   the user whether to apply. On yes: `codeunlimited fix . --apply`.
3. **Set up measurement.** If the project has no baseline yet, `fix --apply`
   captures one. Tell the user to re-check in a few days with
   `codeunlimited report .` - the trend table provides observational evidence,
   not proof that codeunlimited caused a change. For a bounded task cohort, use
   `codeunlimited experiment start <name> .`, then
   `codeunlimited experiment finish <name> --tasks <N> . --json` after the
   declared tasks finish. Compare only non-overlapping complete records.
4. **Weekly ritual.** For "keep an eye on it" requests, suggest
   `codeunlimited schedule` (weekly summary) and `codeunlimited compare`
   (this week vs last).

## Rules

- The tool is offline and reads only token counts - never claim it reads
  prompts.
- Describe opportunities as estimates and quote numbers exactly as the tool
  prints them, including ranges; they are deliberately conservative
  (docs/ACCURACY.md in the repo).
- Describe experiment totals as exact observed counters for their explicit
  half-open windows, but never describe an observed difference as proven or
  guaranteed savings. Preserve low-confidence and observational labels.
- If a command reports unrecognized log lines, run `codeunlimited doctor`
  and suggest filing a format-drift issue.
