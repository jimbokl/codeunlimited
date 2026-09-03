# codeunlimited

**More code out of the subscription limits you already pay for.**

You hit the weekly limit of Claude Code or Codex CLI mid-task. The limit is
fixed — but how much *work* fits inside it is not. `codeunlimited` reads the
session logs already on your machine, shows where limit tokens leak, and sets
your projects up so the same subscription produces more code.

> Not a usage tracker. For accounting ("how much did I use") see
> [ccusage](https://github.com/ryoppippi/ccusage). codeunlimited answers the
> next question: **why so much, and how to fit more work into the same limit.**

## What it finds

- **Context tax of long sessions** — by turn 40 every reply drags the whole
  history through the context window; measured multiplier per session.
- **Heavy model on mechanical replies** — top-tier requests that ended in a
  3-line answer; delegable to a light model.
- **Mid-session cache re-writes** — broken prompt prefixes and expired TTLs
  that re-pay for context instead of reading it back.
- **Fat session starts** — unused MCP servers whose schemas are paid on every
  new session.

Each finding is reported in **limit currency**: tokens reclaimed, % of your
weekly volume, extra agent answers that fit into the same limit.

## Quick start

```bash
pipx install codeunlimited        # or: pip install codeunlimited

codeunlimited audit               # offline scan of ~/.claude and ~/.codex logs
codeunlimited init myproject/     # efficiency rules into CLAUDE.md + AGENTS.md
```

`init` is the "start of a new project" step: it writes token-efficiency rules
(fresh-session discipline, state-file pattern for long loops per
[SKILL.state, arXiv 2608.26263](https://arxiv.org/abs/2608.26263), light-model
delegation, MCP hygiene) that Claude Code and Codex pick up automatically.

## Privacy

Everything runs offline on your machine. Only token counts, model names,
timestamps and project names are read — prompts and responses are never
parsed, stored, or transmitted.

## Supported sources

| Tool | Location | Status |
|---|---|---|
| Claude Code | `~/.claude/projects/**/*.jsonl` | ✅ |
| Codex CLI | `~/.codex/sessions/**/*.jsonl` | ✅ |
| Gemini CLI | — | planned |

MIT license.
