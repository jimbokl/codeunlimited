# Version 2.4: install once, automatic local defaults

The normal installation now does both jobs: install the verified binary and
activate efficiency defaults. Open your next local Claude Code or Codex session
as usual. There is no per-project `init`, recurring `audit`, workflow manifest,
API key, or background service to configure for these defaults.

## What setup changes

| Target | Change |
| --- | --- |
| Claude user `CLAUDE.md` | Append/update a compact, marked efficiency policy |
| Codex user `AGENTS.md` | Same policy, used by all projects by default |
| Existing non-empty Codex `AGENTS.override.md` | Same policy so the active override does not hide installation |
| Codex user `config.toml` | Add `tool_output_token_limit = 4000` only if absent |

Claude's user directory defaults to `~/.claude`; `CLAUDE_CONFIG_DIR` takes
precedence. Codex uses `~/.codex` or `CODEX_HOME`. Windows uses the user profile
directory. Override directories must be absolute. Setup never creates a Codex
override file: doing so could hide existing user guidance.

The rules ask agents to scope reads/searches, reuse fresh context, keep verbose
tool output in recoverable logs, maintain compact state for repetitive work,
preserve a useful handoff before compaction, and keep results concise. Existing
project-specific codeunlimited rules take precedence. No model downgrade or
automatic delegation is requested.

The Codex setting caps individual tool/function outputs **stored in history**.
It does not cap reasoning, response length, all tool output combined, or the
whole session. It can omit needed detail; agents must inspect relevant log
ranges instead of treating a truncated result as verification. An explicitly
configured limit is preserved, even when larger than 4,000.

## Check, update or disable

```bash
codeunlimited setup                 # also repairs/upgrades owned blocks
codeunlimited setup --status --json  # no scan, provider request or file writes
codeunlimited setup --remove         # remove only marked defaults
codeunlimited setup --no-tool-limit  # add policy without adding a missing cap
```

`--no-tool-limit` does not remove an already installed cap. To keep your own
value, remove just the two `# codeunlimited:auto...` comment markers around the
setting and edit the value. Subsequent setup/removal treats it as yours.

`setup --status` exits 0 when the current effective global instruction files
contain the current policy; 1 when missing, stale, shadowed or unreadable. JSON
reports configured files and the base Codex cap, not what a running model
actually loaded or followed. Profile/project settings, instruction byte limits,
managed policies, and explicit host overrides may take precedence. Re-run setup
if you later change provider homes or add a new global override file.

For `cargo install` or manual binary downloads, run `codeunlimited setup` once.
The shell and PowerShell installers do it automatically unless
`CODEUNLIMITED_SKIP_SETUP=1` is set in the installer's environment.

## Preserving user data

Setup preflights all target content and backup paths. Existing text outside its
markers is retained. Changed regular files are written atomically, with the
first original retained as `<filename>.codeunlimited.bak`. Removal does not
restore entire old files over later user edits; backups are kept for recovery.
An edited managed TOML block or ambiguous markers stop the operation so setup
cannot silently discard an uncertain range.

Writes are atomic per file, not a cross-directory transaction. A filesystem
failure after the first write may leave partial activation; the command reports
failure, and rerunning setup after fixing the cause repairs the remaining files.
User-owned configuration directories may themselves be symlinked; final target
and backup files may not be symlinks. External concurrent edits are checked
before each write, but configuration editors do not share a transaction lock.

The installer completes its verified binary replacement before invoking setup.
If setup fails, installation exits nonzero with a repair command. The verified
binary remains installed. Invalid checksums still leave the old binary intact.

## What this does not automate

Global instruction loading is automatic; following guidance is the agent's
behavior. Only the Codex history cap is a native configuration control, and
higher-precedence host/profile settings may override it. Setup does not replace
the provider's internal conversation with the stateful runtime, clear existing
chats, reset subscription limits, launch paid agents, or auto-update binaries.
Remote/cloud hosts require their own installation.

For hard orchestration-step bounds, use the existing explicit `codeunlimited
run` workflow. For accounting, use `audit`, `verdict`, or `experiment` when
needed. None is required to keep the installed defaults present. Installation
adds some instruction tokens; net benefit depends on the work and agent
behavior, not a promised percentage.

## Provider contracts

Checked against [Claude Code memory](https://code.claude.com/docs/en/memory),
[Claude's configuration directory](https://code.claude.com/docs/en/claude-directory),
[Codex global instruction discovery](https://developers.openai.com/codex/guides/agents-md/),
and [Codex configuration reference](https://developers.openai.com/codex/config-reference/)
on 2026-09-06. These document the native loading paths and tool-history setting;
they do not establish a savings rate for codeunlimited.
