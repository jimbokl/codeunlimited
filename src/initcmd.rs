//! `codeunlimited init`: efficiency rules into CLAUDE.md/AGENTS.md + instant
//! per-project baseline when the project already has history.

use std::collections::HashSet;
use std::io;
use std::path::Path;

use crate::{detectors, parsers};

pub const MARKER: &str = "<!-- codeunlimited:v1 -->";

const CLAUDE_BLOCK: &str = r#"<!-- codeunlimited:v1 -->
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
"#;

const AGENTS_BLOCK: &str = r#"<!-- codeunlimited:v1 -->
## Token efficiency (codeunlimited)

- New task = new session; don't grow one thread for days.
- For repetitive loops keep a compact state file (done/remaining/counters) and
  work from it instead of re-reading the conversation (SKILL.state pattern,
  arXiv 2608.26263).
- Delegate mechanical edits to the cheapest model that can do them.
- Never re-read files already in context; read large files by line range.
- Keep answers to: outcome, files changed, next command.
"#;

fn append_block(path: &Path, block: &str, current: Option<&str>) -> io::Result<&'static str> {
    if current.is_some_and(|text| text.contains(MARKER)) {
        return Ok("already set up");
    }
    let next = current
        .map(|text| format!("{}\n\n{}", text.trim_end(), block))
        .unwrap_or_else(|| block.to_string());
    match crate::safeio::update_text_with_backup(path, &next)? {
        crate::safeio::UpdateOutcome::Created => Ok("created"),
        crate::safeio::UpdateOutcome::Updated { .. } => {
            Ok("updated (backup kept as *.codeunlimited.bak)")
        }
        crate::safeio::UpdateOutcome::Unchanged => Ok("already set up"),
    }
}

fn baseline(root: &Path, disp: &str) -> io::Result<()> {
    let cfg = crate::config::Config::load_for(Some(root));
    let mut reqs = parsers::iter_claude(Some(root));
    let codex = parsers::iter_codex(Some(root));
    // Capture the baseline once; `codeunlimited delta` compares against it.
    let bl = root.join(crate::deltacmd::BASELINE_FILE);
    if !bl.exists() {
        let m_claude = crate::metrics::compute(&reqs, cfg.long_session_turns);
        let m_codex = crate::metrics::compute(&codex, cfg.long_session_turns);
        crate::safeio::atomic_write(
            &bl,
            crate::metrics::to_json_multi(
                &[("claude", m_claude), ("codex", m_codex)],
                chrono::Utc::now().timestamp(),
            )
            .as_bytes(),
        )?;
        println!(
            "  baseline captured: {} (check progress later with `codeunlimited delta`)",
            crate::deltacmd::BASELINE_FILE
        );
    }
    reqs.extend(codex);
    if reqs.is_empty() {
        println!("  history: none yet - new project, baseline starts now");
        return Ok(());
    }
    let sessions: HashSet<&str> = reqs.iter().map(|r| r.session.as_ref()).collect();
    let total = reqs
        .iter()
        .fold(0u64, |total, request| total.saturating_add(request.total()));
    println!(
        "  history: {} requests in {} sessions ({:.0}M tokens) - existing project, baseline captured",
        reqs.len(),
        sessions.len(),
        total as f64 / 1e6
    );
    if let Some(top) = detectors::run_all(&reqs, &cfg)
        .into_iter()
        .find(|f| f.impact_tokens > 0)
    {
        println!(
            "  top leak here: {} (~{:.0}M tok. reclaimable)",
            top.title,
            top.impact_tokens as f64 / 1e6
        );
    }
    println!("  full scoped report: codeunlimited audit --project \"{disp}\"");
    Ok(())
}

pub fn run(path: &Path) -> i32 {
    let root = match path.canonicalize() {
        Ok(p) if p.is_dir() => p,
        _ => {
            eprintln!("No such directory: {}", path.display());
            return 1;
        }
    };
    // strip windows verbatim prefix for display
    let disp = root.to_string_lossy();
    let disp = disp.strip_prefix(r"\\?\").unwrap_or(&disp).to_string();
    let claude_path = root.join("CLAUDE.md");
    let agents_path = root.join("AGENTS.md");
    let claude_current = match crate::safeio::read_optional_text(&claude_path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("Cannot safely read {}: {e}", claude_path.display());
            return 1;
        }
    };
    let agents_current = match crate::safeio::read_optional_text(&agents_path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("Cannot safely read {}: {e}", agents_path.display());
            return 1;
        }
    };
    if let Err(e) = crate::registry::register(&root) {
        eprintln!("Cannot register {}: {e}", root.display());
        return 1;
    }
    println!("codeunlimited init -> {disp}");
    let claude_status = match append_block(&claude_path, CLAUDE_BLOCK, claude_current.as_deref()) {
        Ok(status) => status,
        Err(e) => {
            eprintln!("Cannot update {}: {e}", claude_path.display());
            return 1;
        }
    };
    println!("  CLAUDE.md: {claude_status}");
    let agents_status = match append_block(&agents_path, AGENTS_BLOCK, agents_current.as_deref()) {
        Ok(status) => status,
        Err(e) => {
            eprintln!("Cannot update {}: {e}", agents_path.display());
            return 1;
        }
    };
    println!("  AGENTS.md: {agents_status}");
    if let Err(e) = baseline(&root, &disp) {
        eprintln!("Cannot capture baseline in {}: {e}", root.display());
        return 1;
    }
    println!("Done. Claude Code and Codex pick the rules up automatically.");
    0
}
