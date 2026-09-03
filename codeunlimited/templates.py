# -*- coding: utf-8 -*-
"""Efficiency blocks written by `codeunlimited init` into project instruction files."""

MARKER = "<!-- codeunlimited:v1 -->"

CLAUDE_BLOCK = f"""{MARKER}
## Token efficiency (codeunlimited)

Rules that fit more work into the same subscription limit:

- **New task = new session.** Don't grow one chat for days: by the tail of a
  long session every turn drags the whole accumulated context. Task done -
  /clear.
- **Long loops run on a state file, not on history.** For monitoring,
  list-driven migrations and other repetitive loops keep a compact
  `state/state.json` (done / remaining / counters) and work from it instead of
  re-reading the conversation. Pattern: SKILL.state, arXiv 2608.26263.
- **Delegate mechanical work to a light model.** Renames, repetitive edits,
  boilerplate - a Task subagent with model haiku / low effort, not the main
  top-tier model.
- **Never re-read what is already in context.** A file read earlier in this
  session is not re-read without a reason; read large files by line range.
- **Answers to the point.** Outcome, files changed, next command. No process
  narration, no full listings of already-applied diffs.
- **MCP hygiene.** Keep only the MCP servers this project actually uses
  connected: every connected server's schemas are paid out of the limit at
  each session start.
"""

AGENTS_BLOCK = f"""{MARKER}
## Token efficiency (codeunlimited)

- New task = new session; don't grow one thread for days.
- For repetitive loops keep a compact state file (done/remaining/counters) and
  work from it instead of re-reading the conversation (SKILL.state pattern,
  arXiv 2608.26263).
- Delegate mechanical edits to the cheapest model that can do them.
- Never re-read files already in context; read large files by line range.
- Keep answers to: outcome, files changed, next command.
"""
