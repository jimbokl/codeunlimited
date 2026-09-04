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
/// Returns `Ok(None)` when the file already carries exactly this content and
/// fails without producing replacement bytes when markers are ambiguous.
pub fn upsert_block(
    current: Option<&str>,
    rendered: &str,
) -> io::Result<Option<(String, &'static str)>> {
    let Some(text) = current else {
        return Ok(Some((rendered.to_string(), "created")));
    };

    let v2_starts: Vec<_> = text.match_indices(MARKER_V2).map(|(i, _)| i).collect();
    let v2_ends: Vec<_> = text.match_indices(MARKER_END).map(|(i, _)| i).collect();
    let v1_starts: Vec<_> = text.match_indices(MARKER).map(|(i, _)| i).collect();
    let has_v2_marker = !v2_starts.is_empty() || !v2_ends.is_empty();
    if has_v2_marker {
        if v2_starts.len() != 1 || v2_ends.len() != 1 || !v1_starts.is_empty() {
            return Err(invalid_block("ambiguous codeunlimited block markers"));
        }
        let start = v2_starts[0];
        let end_start = v2_ends[0];
        if end_start <= start {
            return Err(invalid_block("codeunlimited end marker precedes its start"));
        }
        let end = end_start + MARKER_END.len();
        let replacement = rendered_for_file(text, rendered);
        let old = &text[start..end];
        if old == replacement.trim_end() {
            return Ok(None);
        }
        let mut next = String::with_capacity(text.len() + replacement.len());
        next.push_str(&text[..start]);
        next.push_str(replacement.trim_end());
        next.push_str(&text[end..]);
        return Ok(Some((next, "updated to the current technique set")));
    }

    if !v1_starts.is_empty() {
        if v1_starts.len() != 1 {
            return Err(invalid_block("multiple legacy codeunlimited markers"));
        }
        const LEGACY_HEADING: &str = "## Token efficiency (codeunlimited)";
        let start = v1_starts[0];
        let after_marker = start + MARKER.len();
        let heading_start = consume_line_ending(text, after_marker)
            .ok_or_else(|| invalid_block("legacy marker is not followed by its heading"))?;
        if !text[heading_start..].starts_with(LEGACY_HEADING) {
            return Err(invalid_block(
                "legacy marker is not followed by its heading",
            ));
        }
        let heading_end = heading_start + LEGACY_HEADING.len();
        let body_start = if heading_end == text.len() {
            heading_end
        } else {
            consume_line_ending(text, heading_end)
                .ok_or_else(|| invalid_block("legacy heading has trailing content"))?
        };
        let next_heading = if text[body_start..].starts_with("## ") {
            Some(body_start)
        } else {
            text[body_start..].find("\n## ").map(|i| body_start + i + 1)
        };

        let replacement = rendered_for_file(text, rendered);
        let mut next = String::with_capacity(text.len() + replacement.len());
        next.push_str(&text[..start]);
        if let Some(heading) = next_heading {
            let mut separator = heading;
            while separator > body_start && matches!(text.as_bytes()[separator - 1], b'\r' | b'\n')
            {
                separator -= 1;
            }
            next.push_str(replacement.trim_end());
            next.push_str(&text[separator..]);
        } else {
            next.push_str(&replacement);
        }
        return Ok(Some((next, "upgraded v1 block to v2")));
    }

    let replacement = rendered_for_file(text, rendered);
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let separator = if text.is_empty() || text.ends_with(&format!("{eol}{eol}")) {
        ""
    } else if text.ends_with(eol) {
        eol
    } else if eol == "\r\n" {
        "\r\n\r\n"
    } else {
        "\n\n"
    };
    Ok(Some((
        format!("{text}{separator}{replacement}"),
        "updated (block appended)",
    )))
}

fn invalid_block(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn consume_line_ending(text: &str, offset: usize) -> Option<usize> {
    let rest = text.get(offset..)?;
    if rest.starts_with("\r\n") {
        Some(offset + 2)
    } else if rest.starts_with('\n') {
        Some(offset + 1)
    } else {
        None
    }
}

fn rendered_for_file(current: &str, rendered: &str) -> String {
    let normalized = rendered.replace("\r\n", "\n");
    if current.contains("\r\n") {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    }
}

fn apply_block(path: &Path, rendered: &str, current: Option<&str>) -> io::Result<&'static str> {
    let Some((next, verb)) = upsert_block(current, rendered)? else {
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
            "  top estimated opportunity: {} (~{:.0}M tok.)",
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
    let cfg = crate::config::Config::load_for(Some(&root));
    let techs = crate::techniques::enabled(&cfg);
    let claude_block = crate::techniques::render_claude(&techs);
    let agents_block = crate::techniques::render_agents(&techs);
    if let Err(e) = upsert_block(claude_current.as_deref(), &claude_block) {
        eprintln!("Cannot update {}: {e}", claude_path.display());
        return 1;
    }
    if let Err(e) = upsert_block(agents_current.as_deref(), &agents_block) {
        eprintln!("Cannot update {}: {e}", agents_path.display());
        return 1;
    }
    if let Err(e) = crate::registry::register(&root) {
        eprintln!("Cannot register {}: {e}", root.display());
        return 1;
    }
    println!("codeunlimited init -> {disp}");
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
        let (next, verb) = upsert_block(None, &rendered()).unwrap().unwrap();
        assert!(next.starts_with(techniques::MARKER_V2));
        assert_eq!(verb, "created");

        let (next, verb) = upsert_block(Some("# My project\n\ncontent\n"), &rendered())
            .unwrap()
            .unwrap();
        assert!(next.starts_with("# My project"));
        assert!(next.contains(techniques::MARKER_V2));
        assert_eq!(verb, "updated (block appended)");
    }

    #[test]
    fn upgrades_v1_block_preserving_surrounding_content() {
        let file = format!("# Intro\n\n{LEGACY_V1}\n## After section\n\nkeep me\n");
        let (next, verb) = upsert_block(Some(&file), &rendered()).unwrap().unwrap();
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
        assert!(
            upsert_block(Some(&file), &block).unwrap().is_none(),
            "no-op rewrite"
        );

        // Toggling a technique changes the render -> in-place replacement.
        let cfg = Config {
            techniques_disable: vec!["mcp-hygiene".into()],
            ..Config::default()
        };
        let smaller = techniques::render_claude(&techniques::enabled(&cfg));
        let (next, verb) = upsert_block(Some(&file), &smaller).unwrap().unwrap();
        assert_eq!(verb, "updated to the current technique set");
        assert!(!next.contains("cu:mcp-hygiene"));
        assert!(next.contains("## Tail"));
        assert_eq!(next.matches(techniques::MARKER_V2).count(), 1);
    }

    #[test]
    fn rejects_malformed_or_ambiguous_markers() {
        let block = rendered();
        let cases = [
            "<!-- codeunlimited:v1 -->\nIMPORTANT USER CONTENT\n",
            "<!-- codeunlimited:v1 -->\n<!-- codeunlimited:v1 -->\n## Token efficiency (codeunlimited)\n",
            "<!-- codeunlimited:v2 -->\nunfinished\n",
            "<!-- /codeunlimited -->\n",
            "<!-- /codeunlimited -->\n<!-- codeunlimited:v2 -->\n",
        ];
        for current in cases {
            let error = upsert_block(Some(current), &block).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{current:?}");
        }

        let duplicated = format!("{block}\n{block}");
        let error = upsert_block(Some(&duplicated), &block).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn upgrades_legacy_block_at_start_and_end() {
        let block = rendered();
        let at_start = format!("{LEGACY_V1}\n## Keep\n\nbody\n");
        let (next, _) = upsert_block(Some(&at_start), &block).unwrap().unwrap();
        assert!(next.starts_with(techniques::MARKER_V2));
        assert!(next.ends_with("## Keep\n\nbody\n"));

        let at_end = format!("# Keep\n\n{LEGACY_V1}");
        let (next, _) = upsert_block(Some(&at_end), &block).unwrap().unwrap();
        assert!(next.starts_with("# Keep\n\n"));
        assert!(next.ends_with("<!-- /codeunlimited -->\n"));
        assert_eq!(next.matches(techniques::MARKER_V2).count(), 1);
    }

    #[test]
    fn preserves_crlf_when_upgrading_and_replacing() {
        let rendered_crlf = rendered().replace('\n', "\r\n");
        let legacy = "# Intro\r\n\r\n<!-- codeunlimited:v1 -->\r\n## Token efficiency (codeunlimited)\r\n\r\n- old\r\n\r\n## Tail\r\nkeep\r\n";
        let (upgraded, _) = upsert_block(Some(legacy), &rendered()).unwrap().unwrap();
        assert_eq!(
            upgraded,
            format!("# Intro\r\n\r\n{rendered_crlf}\r\n## Tail\r\nkeep\r\n")
        );

        let current = format!("# Intro\r\n\r\n{rendered_crlf}\r\n## Tail\r\n");
        let cfg = Config {
            techniques_disable: vec!["mcp-hygiene".into()],
            ..Config::default()
        };
        let smaller = techniques::render_claude(&techniques::enabled(&cfg));
        let (replaced, _) = upsert_block(Some(&current), &smaller).unwrap().unwrap();
        assert!(!replaced.replace("\r\n", "").contains('\n'));
        assert!(replaced.ends_with("\r\n## Tail\r\n"));
    }

    #[test]
    fn appending_preserves_existing_crlf_bytes() {
        let current = "# Project\r\n\r\nuser content  \r\n";
        let (next, verb) = upsert_block(Some(current), &rendered()).unwrap().unwrap();
        assert_eq!(verb, "updated (block appended)");
        assert!(next.starts_with(current));
        assert!(!next.replace("\r\n", "").contains('\n'));
        assert!(next.contains("\r\n\r\n<!-- codeunlimited:v2 -->\r\n"));
    }
}
