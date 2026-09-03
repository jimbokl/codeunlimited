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

The core is a single **Rust** binary - scans gigabytes of logs in ~2 seconds,
no runtime dependencies:

```bash
cargo install codeunlimited       # or grab a binary from GitHub Releases

codeunlimited audit               # offline scan of ~/.claude and ~/.codex logs
codeunlimited init myproject/     # efficiency rules into CLAUDE.md + AGENTS.md
codeunlimited audit --project .   # report scoped to one project
codeunlimited delta myproject/    # verified before/after since init's baseline
codeunlimited report myproject/   # saved Markdown report: findings + delta + trend
codeunlimited fix myproject/      # findings -> concrete changes (dry-run; --apply)
```

A Python reference implementation lives in `codeunlimited/` (same detectors;
used as the prototyping sandbox): `pip install -e . && python -m codeunlimited audit`.

## Two ways to adopt — both first-class

**Starting a new project:** run `codeunlimited init` in the fresh directory.
Token-efficiency rules (fresh-session discipline, state-file pattern for long
loops per [SKILL.state, arXiv 2608.26263](https://arxiv.org/abs/2608.26263),
light-model delegation, MCP hygiene) are in place from day one and picked up
by Claude Code and Codex automatically.

**Attaching to an existing project:** the same `codeunlimited init` appends
the rules to your existing CLAUDE.md/AGENTS.md (idempotent, marker-guarded)
and — because the project already has history — instantly prints its
baseline: requests, sessions, and the top limit leak found in *this* project.
Then `codeunlimited audit --project <path>` gives the full scoped report,
and re-running it later shows your delta.

## Reports you can keep and share

`codeunlimited report <project>` writes `CODEUNLIMITED_REPORT.md` into the
project: current leaks in limit currency, the verified delta since `init`
captured the baseline, and a trend table with one row per run (snapshots
accumulate in `.codeunlimited.history.jsonl`). Re-run it weekly — the trend
table is the proof that the efficiency rules are paying off. Both files are
plain text you can commit, ignore, or paste into a standup.

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
