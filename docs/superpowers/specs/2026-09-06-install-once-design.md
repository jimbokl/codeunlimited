# Install once: automatic local integration

User direction: make setup automatic, choose implementation autonomously, test
and push. Version 2.4.0 builds on main's 2.3.1 accounting fixes.

## Decision

Use the providers' native, globally loaded instruction files. A daemon would
add idle work and races; mandatory CLI wrappers would miss desktop sessions.
Native files work for new local sessions without a per-project init command.
This is automatic activation of efficiency guidance, not forced adherence or
transparent replacement of the host's conversation with the stateful runtime.

`setup` installs a compact, separately marked policy in Claude's user CLAUDE.md
and Codex's AGENTS.md, plus an existing non-empty AGENTS.override.md. Honor
CLAUDE_CONFIG_DIR and CODEX_HOME. Never create an override file. Keep user
instructions, project files, model selection, permissions, hooks and credentials.

For Codex, prepend a marked top-level `tool_output_token_limit = 4000` setting
only if absent. Preserve any explicit value. `--no-tool-limit` skips this
setting. Validate TOML before any writes. This cap reduces stored tool output;
it may omit diagnostics, so policy requires recovering needed detail from logs.

## Lifecycle

- Both verified binary installers invoke `setup` by absolute path after the
  binary commit. CODEUNLIMITED_SKIP_SETUP=1 installs only the binary.
- Failure during activation exits nonzero, clearly distinguishes installed
  binary from inactive integration, and prints the repair command.
- `setup --status [--json]` is read-only and checks current files, not a stale
  success flag. It must not claim model compliance or actual savings.
- `setup --remove` removes only marked content, never restores old whole files
  over later edits. Reject ambiguous markers and changed managed TOML content.
- Reinstall updates only owned blocks, preserves first backups, and is quiet
  about unchanged files. Preflight all targets before mutating any target;
  recheck bytes before commit. Atomic individual writes; filesystem errors may
  leave a disclosed partial installation repairable by rerunning setup.
- No history scan, provider calls, service, scheduler, automatic updates, or
  external API layer is enabled by installation.

## Verification and limits

Rust 1.82, existing dependencies, macOS/Linux/Windows. Public CLI tests use
isolated provider homes: defaults, overrides, idempotency, removal after user
edits, invalid UTF-8/TOML, malformed markers, symlinks, explicit token cap,
read-only status, and partial-failure diagnostics. Execute Unix installer tests
against a local HTTP fixture and actual built binary; native Windows coverage
runs in CI. No paid model experiment is required for installation behavior.

References: official Claude Code memory and claude-directory documentation;
OpenAI AGENTS.md discovery and config-reference documentation (checked 2026-09-06).
