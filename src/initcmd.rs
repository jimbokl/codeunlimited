//! `codeunlimited init`: efficiency rules into CLAUDE.md/AGENTS.md + instant
//! per-project baseline when the project already has history.

use std::collections::HashSet;
use std::io;
use std::path::Path;

use crate::{detectors, parsers};

pub const MARKER: &str = "<!-- codeunlimited:v1 -->";
pub use crate::techniques::{MARKER_END, MARKER_V2};

/// Compute the next file content: append the rendered block to a fresh file,
/// replace an existing v2 block in place, or upgrade a legacy v1 block.
/// Returns None when the file already carries exactly this content.
pub fn upsert_block(current: Option<&str>, rendered: &str) -> Option<(String, &'static str)> {
    let Some(text) = current else {
        return Some((rendered.to_string(), "created"));
    };
    if let (Some(start), Some(end)) = (text.find(MARKER_V2), text.find(MARKER_END)) {
        if end > start {
            let end = end + MARKER_END.len();
            let old = &text[start..end];
            if old == rendered.trim_end() {
                return None;
            }
            let mut next = String::with_capacity(text.len());
            next.push_str(&text[..start]);
            next.push_str(rendered.trim_end());
            next.push_str(&text[end..]);
            return Some((next, "updated to the current technique set"));
        }
    }
    if let Some(start) = text.find(MARKER) {
        // Legacy v1 block: marker + one "## " heading; it extends to the next
        // "## " heading after the marker or to the end of the file.
        let after_heading = text[start..]
            .find("\n## ")
            .map(|i| start + i + 1)
            .unwrap_or(text.len());
        let end = text[after_heading..]
            .find("\n## ")
            .map(|i| after_heading + i + 1)
            .unwrap_or(text.len());
        let mut next = String::with_capacity(text.len());
        next.push_str(text[..start].trim_end());
        if !next.is_empty() {
            next.push_str("\n\n");
        }
        next.push_str(rendered.trim_end());
        let tail = text[end..].trim_start();
        if !tail.is_empty() {
            next.push_str("\n\n");
            next.push_str(tail);
        }
        next.push('\n');
        return Some((next, "upgraded v1 block to v2"));
    }
    Some((
        format!("{}\n\n{}", text.trim_end(), rendered),
        "updated (block appended)",
    ))
}

fn apply_block(path: &Path, rendered: &str, current: Option<&str>) -> io::Result<&'static str> {
    let Some((next, verb)) = upsert_block(current, rendered) else {
        return Ok("already up to date");
    };
    match crate::safeio::update_text_with_backup(path, &next)? {
        crate::safeio::UpdateOutcome::Created => Ok("created"),
        crate::safeio::UpdateOutcome::Updated { .. } => Ok(verb),
        crate::safeio::UpdateOutcome::Unchanged => Ok("already up to date"),
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
    let cfg = crate::config::Config::load_for(Some(&root));
    let techs = crate::techniques::enabled(&cfg);
    let claude_block = crate::techniques::render_claude(&techs);
    let agents_block = crate::techniques::render_agents(&techs);
    let claude_status = match apply_block(&claude_path, &claude_block, claude_current.as_deref()) {
        Ok(status) => status,
        Err(e) => {
            eprintln!("Cannot update {}: {e}", claude_path.display());
            return 1;
        }
    };
    println!("  CLAUDE.md: {claude_status}");
    let agents_status = match apply_block(&agents_path, &agents_block, agents_current.as_deref()) {
        Ok(status) => status,
        Err(e) => {
            eprintln!("Cannot update {}: {e}", agents_path.display());
            return 1;
        }
    };
    println!("  AGENTS.md: {agents_status}");
    println!(
        "  techniques: {} enabled (list/toggle: codeunlimited techniques)",
        techs.len()
    );
    if let Err(e) = baseline(&root, &disp) {
        eprintln!("Cannot capture baseline in {}: {e}", root.display());
        return 1;
    }
    println!("Done. Claude Code and Codex pick the rules up automatically.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::techniques;

    fn rendered() -> String {
        techniques::render_claude(&techniques::enabled(&Config::default()))
    }

    const LEGACY_V1: &str = "<!-- codeunlimited:v1 -->\n## Token efficiency (codeunlimited)\n\n- old rule one\n- old rule two\n";

    #[test]
    fn appends_to_fresh_and_existing_files() {
        let (next, verb) = upsert_block(None, &rendered()).unwrap();
        assert!(next.starts_with(techniques::MARKER_V2));
        assert_eq!(verb, "created");

        let (next, verb) = upsert_block(Some("# My project\n\ncontent\n"), &rendered()).unwrap();
        assert!(next.starts_with("# My project"));
        assert!(next.contains(techniques::MARKER_V2));
        assert_eq!(verb, "updated (block appended)");
    }

    #[test]
    fn upgrades_v1_block_preserving_surrounding_content() {
        let file = format!("# Intro\n\n{LEGACY_V1}\n## After section\n\nkeep me\n");
        let (next, verb) = upsert_block(Some(&file), &rendered()).unwrap();
        assert_eq!(verb, "upgraded v1 block to v2");
        assert!(next.starts_with("# Intro"));
        assert!(next.contains(techniques::MARKER_V2));
        assert!(!next.contains("codeunlimited:v1"));
        assert!(!next.contains("old rule one"));
        assert!(next.contains("## After section"));
        assert!(next.contains("keep me"));
    }

    #[test]
    fn v2_block_is_idempotent_and_replaceable() {
        let block = rendered();
        let file = format!("# Intro\n\n{block}\n## Tail\n");
        assert!(upsert_block(Some(&file), &block).is_none(), "no-op rewrite");

        // Toggling a technique changes the render -> in-place replacement.
        let cfg = Config {
            techniques_disable: vec!["mcp-hygiene".into()],
            ..Config::default()
        };
        let smaller = techniques::render_claude(&techniques::enabled(&cfg));
        let (next, verb) = upsert_block(Some(&file), &smaller).unwrap();
        assert_eq!(verb, "updated to the current technique set");
        assert!(!next.contains("cu:mcp-hygiene"));
        assert!(next.contains("## Tail"));
        assert_eq!(next.matches(techniques::MARKER_V2).count(), 1);
    }
}
