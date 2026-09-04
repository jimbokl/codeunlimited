//! `codeunlimited fix`: turns audit findings into concrete project changes.
//! Dry-run by default; `--apply` writes. Anything that could change agent
//! behavior beyond the documented efficiency rules is only suggested, never
//! auto-edited (see Red lines in ROADMAP.md).

use std::collections::HashMap;
use std::path::Path;

use crate::types::Request;
use crate::{config::Config, initcmd, parsers};

pub const STATE_SCAFFOLD: &str = "state/state.json";

fn long_sessions(reqs: &[Request], cfg: &Config) -> usize {
    let long = cfg.long_session_turns;
    let mut by: HashMap<(&str, &str), usize> = HashMap::new();
    for r in reqs {
        *by.entry((r.source, r.session.as_ref())).or_default() += 1;
    }
    by.values().filter(|&&n| n > long).count()
}

/// Median cache-write size of the first request of each claude session -
/// a proxy for what every fresh session pays before any work happens.
fn median_session_start(reqs: &[Request]) -> u64 {
    let mut by: HashMap<&str, &Request> = HashMap::new();
    for r in reqs.iter().filter(|r| r.source == "claude") {
        by.entry(r.session.as_ref())
            .and_modify(|cur| {
                if r.ts < cur.ts {
                    *cur = r;
                }
            })
            .or_insert(r);
    }
    let mut starts: Vec<u64> = by
        .values()
        .map(|request| request.w5.saturating_add(request.w1h))
        .collect();
    if starts.is_empty() {
        return 0;
    }
    starts.sort_unstable();
    starts[starts.len() / 2]
}

fn mcp_servers(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join(".mcp.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| {
            v.get("mcpServers")
                .and_then(|s| s.as_object())
                .map(|o| o.keys().cloned().collect())
        })
        .unwrap_or_default()
}

/// `fix --all`: run the fix pass over every registered project.
pub fn run_all(apply: bool) -> i32 {
    let projects = match crate::registry::projects() {
        Ok(projects) => projects,
        Err(error) => {
            eprintln!("Cannot read the project registry: {error}");
            return 1;
        }
    };
    if projects.is_empty() {
        eprintln!("No registered projects yet - run `init`, `fix` or `report` on one first.");
        return 1;
    }
    let global_cfg = Config::load_for(None);
    let mut worst = 0;
    for p in projects {
        if global_cfg.is_ignored(&p.to_string_lossy()) {
            println!("Skipping ignored project: {}", p.display());
            continue;
        }
        let cfg = Config::load_for(Some(&p));
        if cfg.is_ignored(&p.to_string_lossy()) {
            println!("Skipping ignored project: {}", p.display());
            continue;
        }
        worst = worst.max(run_with_config(&p, apply, &cfg));
        println!();
    }
    worst
}

pub fn run(path: &Path, apply: bool) -> i32 {
    let root = match path.canonicalize() {
        Ok(p) if p.is_dir() => p,
        _ => {
            eprintln!("No such directory: {}", path.display());
            return 1;
        }
    };
    let cfg = Config::load_for(Some(&root));
    run_with_config(&root, apply, &cfg)
}

fn run_with_config(root: &Path, apply: bool, cfg: &Config) -> i32 {
    let disp = root.to_string_lossy();
    let disp = disp.strip_prefix(r"\\?\").unwrap_or(&disp).to_string();
    if let Err(e) = crate::registry::register(root) {
        eprintln!("Cannot register {}: {e}", root.display());
        return 1;
    }
    println!(
        "codeunlimited fix -> {disp}{}",
        if apply {
            ""
        } else {
            "  (dry run; --apply to write)"
        }
    );

    let mut reqs = parsers::iter_claude(Some(root));
    reqs.extend(parsers::iter_codex(Some(root)));

    let mut n = 0;
    let mut applied = 0;
    let mut failed = false;

    // 1. Efficiency rules block present and current (v2, technique-rendered)?
    let has_current = ["CLAUDE.md", "AGENTS.md"].iter().all(|f| {
        std::fs::read_to_string(root.join(f))
            .is_ok_and(|t| t.contains(crate::techniques::MARKER_V2))
    });
    if !has_current {
        let has_legacy = ["CLAUDE.md", "AGENTS.md"].iter().any(|f| {
            std::fs::read_to_string(root.join(f)).is_ok_and(|t| t.contains(initcmd::MARKER))
        });
        n += 1;
        if has_legacy {
            println!(" {n}. [rules] efficiency block is v1 - an in-place upgrade is available");
        } else {
            println!(" {n}. [rules] CLAUDE.md/AGENTS.md lack the efficiency block");
        }
        if apply {
            if initcmd::run(root) == 0 {
                applied += 1;
            } else {
                failed = true;
            }
        } else {
            println!("        -> --apply runs `init` here (renders the enabled technique set)");
        }
    }

    // 2. Long loops without a state-file scaffold (SKILL.state pattern).
    let long = long_sessions(&reqs, cfg);
    let state = root.join(STATE_SCAFFOLD);
    if long > 0 && !state.exists() {
        n += 1;
        println!(
            " {n}. [state] {long} session(s) ran past {} turns and no \
             {STATE_SCAFFOLD} scaffold exists",
            cfg.long_session_turns
        );
        if apply {
            let ok = std::fs::create_dir_all(state.parent().unwrap()).and_then(|_| {
                crate::safeio::atomic_write(
                    &state,
                    b"{\n  \"task\": \"\",\n  \"done\": [],\n  \"remaining\": [],\n  \"counters\": {}\n}\n",
                )
            });
            match ok {
                Ok(_) => {
                    applied += 1;
                    println!("        -> created {STATE_SCAFFOLD} (agents keep loop state there instead of re-reading history)");
                }
                Err(e) => {
                    failed = true;
                    eprintln!("        -> cannot create {STATE_SCAFFOLD}: {e}");
                }
            }
        } else {
            println!("        -> --apply creates a compact state file for long loops");
        }
    }

    // 3. Fat session starts: suggested only - MCP config is never auto-edited.
    let start = median_session_start(&reqs);
    if start > cfg.fat_start_tokens {
        n += 1;
        println!(
            " {n}. [mcp]   median session start writes {}k tokens of context before any work",
            start / 1000
        );
        let servers = mcp_servers(root);
        if servers.is_empty() {
            println!(
                "        -> check user-level MCP servers (`claude mcp list`): every \
                 connected server's schemas are paid at each session start"
            );
        } else {
            println!(
                "        -> .mcp.json configures {}: disable the ones this project \
                 doesn't use (manual - never auto-edited)",
                servers.join(", ")
            );
        }
    }

    // 4. Lean memory files: CLAUDE.md/AGENTS.md bill on every turn.
    let technique_on = |id: &str| crate::techniques::enabled(cfg).iter().any(|t| t.id == id);
    if technique_on("lean-memory") {
        let bytes: u64 = ["CLAUDE.md", "AGENTS.md"]
            .iter()
            .filter_map(|f| std::fs::metadata(root.join(f)).ok())
            .map(|m| m.len())
            .sum();
        let est_tokens = bytes / 4;
        if est_tokens > 8_000 {
            n += 1;
            println!(
                " {n}. [memory] CLAUDE.md+AGENTS.md weigh ~{}k tokens and are injected \
                 every turn - trim to stable essentials (manual, reviewable)",
                est_tokens / 1000
            );
        }
    }

    // 5. Codex config hints (read-only - config is never auto-edited).
    if technique_on("tool-output-budget") {
        let codex_cfg = crate::parsers::codex_root()
            .parent()
            .map(|p| p.join("config.toml"));
        if let Some(p) = codex_cfg {
            let raw = std::fs::read_to_string(&p).unwrap_or_default();
            if p.exists() && !raw.contains("tool_output_token_limit") {
                n += 1;
                println!(
                    " {n}. [codex] {} has no tool_output_token_limit - verbose command \
                     output is the largest avoidable cost; suggested: \
                     tool_output_token_limit = 12000 (manual, reviewable)",
                    p.display()
                );
            }
        }
    }

    if n == 0 {
        println!(" Nothing to fix - rules in place, no long-loop or fat-start signals.");
    } else if apply {
        println!(" Applied {applied} of {n} finding(s); the rest are manual suggestions.");
    }
    println!(" Measure the effect later: codeunlimited report \"{disp}\"");
    i32::from(failed)
}
