//! `codeunlimited init`: efficiency rules into CLAUDE.md/AGENTS.md + instant
//! per-project baseline when the project already has history.

use std::collections::HashSet;
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

fn append_block(path: &Path, block: &str) -> &'static str {
    if path.exists() {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if text.contains(MARKER) {
            return "already set up";
        }
        let _ = std::fs::write(path, format!("{}\n\n{}", text.trim_end(), block));
        "updated"
    } else {
        let _ = std::fs::write(path, block);
        "created"
    }
}

fn baseline(root: &Path, disp: &str) {
    let mut reqs = parsers::iter_claude(Some(root));
    let codex = parsers::iter_codex(Some(root));
    // Capture the baseline once; `codeunlimited delta` compares against it.
    let bl = root.join(crate::deltacmd::BASELINE_FILE);
    if !bl.exists() {
        let m_claude = crate::metrics::compute(&reqs);
        let m_codex = crate::metrics::compute(&codex);
        let _ = std::fs::write(
            &bl,
            crate::metrics::to_json_multi(
                &[("claude", m_claude), ("codex", m_codex)],
                chrono::Utc::now().timestamp(),
            ),
        );
        println!(
            "  baseline captured: {} (check progress later with `codeunlimited delta`)",
            crate::deltacmd::BASELINE_FILE
        );
    }
    reqs.extend(codex);
    if reqs.is_empty() {
        println!("  history: none yet - new project, baseline starts now");
        return;
    }
    let sessions: HashSet<&str> = reqs.iter().map(|r| r.session.as_str()).collect();
    let total: u64 = reqs.iter().map(|r| r.total()).sum();
    println!(
        "  history: {} requests in {} sessions ({:.0}M tokens) - existing project, baseline captured",
        reqs.len(),
        sessions.len(),
        total as f64 / 1e6
    );
    if let Some(top) = detectors::run_all(&reqs)
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
    crate::registry::register(&root);
    println!("codeunlimited init -> {disp}");
    println!(
        "  CLAUDE.md: {}",
        append_block(&root.join("CLAUDE.md"), CLAUDE_BLOCK)
    );
    println!(
        "  AGENTS.md: {}",
        append_block(&root.join("AGENTS.md"), AGENTS_BLOCK)
    );
    baseline(&root, &disp);
    println!("Done. Claude Code and Codex pick the rules up automatically.");
    0
}
