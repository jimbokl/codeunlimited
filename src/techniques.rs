//! The technique catalog: every efficiency rule the tool can install is a
//! first-class, individually toggleable object. `init`/`fix` render the
//! CLAUDE.md/AGENTS.md blocks from the *enabled* set, so users switch any
//! technique off without forking the block, and new releases upgrade blocks
//! in place (versioned markers).
//!
//! Toggling (`.codeunlimited.toml`, project or `~/.codeunlimited/config.toml`):
//!
//! ```toml
//! [techniques]
//! disable = ["delegate-mechanical"]
//! enable  = ["reasoning-effort"]   # opt into default-off techniques
//! ```
//!
//! Design rule for new-generation, token-hungry models: techniques must not
//! silently trade quality for tokens. Anything that can affect output quality
//! is marked `Risk::Medium`, ships with an explicit quality guardrail in its
//! text, and the most aggressive ones default to OFF.

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Pure waste removal - cannot degrade output quality.
    Low,
    /// Trades something for tokens - guardrailed, and off by default when
    /// the trade is aggressive.
    Medium,
}

pub struct Technique {
    pub id: &'static str,
    pub title: &'static str,
    pub risk: Risk,
    pub default_on: bool,
    /// Product version that introduced the technique (upgrade trail).
    pub since: &'static str,
    /// Full bullet for CLAUDE.md.
    pub claude: &'static str,
    /// Short bullet for AGENTS.md.
    pub agents: &'static str,
}

pub const MARKER_V2: &str = "<!-- codeunlimited:v2 -->";
pub const MARKER_END: &str = "<!-- /codeunlimited -->";

pub const CATALOG: &[Technique] = &[
    Technique {
        id: "fresh-sessions",
        title: "Context-aware session boundaries",
        risk: Risk::Low,
        default_on: true,
        since: "0.1",
        claude: "**Choose session boundaries by context reuse.** Batch small related tasks \
                 while prior context still contributes. Start a fresh session for a \
                 distinct multi-step task when the old context would mostly be dead \
                 weight; every restart also pays the session boot cost. Don't grow one \
                 chat for days, but don't restart mechanically per prompt.",
        agents: "Batch small related tasks while prior context still contributes. Start \
                 a fresh session for a distinct multi-step task when old context would \
                 mostly be dead weight; account for session boot cost.",
    },
    Technique {
        id: "state-file-loops",
        title: "Long loops run on a state file",
        risk: Risk::Low,
        default_on: true,
        since: "0.1",
        claude: "**Long loops run on a state file, not on history.** For monitoring, \
                 list-driven migrations and other repetitive loops keep a compact \
                 `state/state.json` (done / remaining / counters) and work from it \
                 instead of re-reading the conversation. Pattern: SKILL.state, \
                 arXiv 2608.26263.",
        agents: "For repetitive loops keep a compact state file (done/remaining/counters) \
                 instead of re-reading the conversation (SKILL.state, arXiv 2608.26263).",
    },
    Technique {
        id: "delegate-mechanical",
        title: "Delegate mechanical work to a light model",
        risk: Risk::Medium,
        default_on: true,
        since: "0.1",
        claude: "**Delegate mechanical work to a light model.** Renames, repetitive \
                 edits, boilerplate - a subagent on a cheap model / low effort. \
                 Quality guardrail: anything requiring reasoning, architecture or \
                 judgment stays on the main top-tier model - never downgrade those.",
        agents: "Delegate mechanical edits to the cheapest model that can do them; \
                 reasoning and architecture stay on the top model.",
    },
    Technique {
        id: "no-rereads",
        title: "Never re-read what is already in context",
        risk: Risk::Low,
        default_on: true,
        since: "0.1",
        claude: "**Never re-read what is already in context.** A file read earlier in \
                 this session is not re-read without a reason; read large files by \
                 line range.",
        agents: "Never re-read files already in context; read large files by line range.",
    },
    Technique {
        id: "concise-answers",
        title: "Answers to the point",
        risk: Risk::Low,
        default_on: true,
        since: "0.1",
        claude: "**Answers to the point.** Outcome, files changed, next command. No \
                 process narration, no full listings of already-applied diffs.",
        agents: "Keep answers to: outcome, files changed, next command.",
    },
    Technique {
        id: "mcp-hygiene",
        title: "MCP hygiene",
        risk: Risk::Low,
        default_on: true,
        since: "0.1",
        claude: "**MCP hygiene.** Keep only the MCP servers this project actually uses \
                 connected: every connected server's schemas are paid out of the limit \
                 at each session start.",
        agents: "Keep only the MCP servers this project actually uses connected.",
    },
    Technique {
        id: "manual-compact",
        title: "Compact deliberately, clear at task end",
        risk: Risk::Low,
        default_on: true,
        since: "1.9",
        claude: "**Compact deliberately.** When a long task must continue, run /compact \
                 manually and tell the model what to keep for the next phase - a \
                 directed summary beats passive autocompact. When the task is done, \
                 /clear instead of compacting a dead thread.",
        agents: "Compact deliberately with direction before autocompact; /clear once \
                 the task is done.",
    },
    Technique {
        id: "file-refs",
        title: "Reference files, don't paste them",
        risk: Risk::Low,
        default_on: true,
        since: "1.9",
        claude: "**Reference files, don't paste them.** Point at paths (@file) and ask \
                 the agent to read exactly what it needs; pasted file bodies stay in \
                 context for the rest of the session.",
        agents: "Reference file paths instead of pasting file bodies.",
    },
    Technique {
        id: "lean-memory",
        title: "Keep memory files lean",
        risk: Risk::Low,
        default_on: true,
        since: "1.9",
        claude: "**Keep CLAUDE.md/AGENTS.md lean.** These files are injected on every \
                 turn and bill on questions they have nothing to do with - stable \
                 essentials only, details belong in docs the agent reads on demand.",
        agents: "Keep this file lean - it is injected and billed on every turn.",
    },
    Technique {
        id: "scan-ignore",
        title: "Exclude scan-heavy directories",
        risk: Risk::Low,
        default_on: true,
        since: "1.9",
        claude: "**Exclude scan-heavy directories.** Data dumps, build output and \
                 vendored deps burn tokens in every search - list them in \
                 .claudeignore (Claude Code) and name directories to avoid here.",
        agents: "Exclude data dumps, build output and vendored deps from scans.",
    },
    Technique {
        id: "tool-output-budget",
        title: "Cap verbose tool output (Codex)",
        risk: Risk::Low,
        default_on: true,
        since: "1.9",
        claude: "**Cap verbose tool output.** Codex CLI: set \
                 `tool_output_token_limit = 12000` in ~/.codex/config.toml - noisy \
                 command output is the single largest avoidable cost; pipe long \
                 output through filters (tail, grep) instead of dumping it raw.",
        agents: "Cap tool output (tool_output_token_limit = 12000); filter long \
                 command output instead of dumping it raw.",
    },
    Technique {
        id: "reasoning-effort",
        title: "Lower reasoning effort for routine work",
        risk: Risk::Medium,
        default_on: false,
        since: "1.9",
        claude: "**Lower reasoning effort for routine work.** Codex CLI: \
                 `model_reasoning_effort = \"medium\"` cuts thinking tokens (billed \
                 as output) on mechanical tasks. Quality guardrail: keep full effort \
                 for debugging, design and anything you'd review carefully - this \
                 technique is opt-in for exactly that reason.",
        agents: "Routine-only: medium reasoning effort; full effort for debugging \
                 and design.",
    },
    Technique {
        id: "model-routing",
        title: "Route sessions to the cheapest capable model",
        risk: Risk::Medium,
        default_on: false,
        since: "1.9",
        claude: "**Route sessions to the cheapest capable model.** Start routine \
                 sessions on a mid-tier model (/model) and switch up only when the \
                 task demands it. Quality guardrail: architecture, tricky debugging \
                 and long-horizon work stay on the top model - opt-in because the \
                 top tier is what you pay for.",
        agents: "Routine sessions on a mid-tier model; top model for architecture \
                 and hard debugging.",
    },
];

/// The enabled set for a config: defaults, minus `disable`, plus `enable`.
pub fn enabled(cfg: &Config) -> Vec<&'static Technique> {
    CATALOG
        .iter()
        .filter(|t| {
            if cfg.techniques_disable.iter().any(|d| d == t.id) {
                return false;
            }
            t.default_on || cfg.techniques_enable.iter().any(|e| e == t.id)
        })
        .collect()
}

fn render(techs: &[&Technique], body: fn(&Technique) -> &'static str, header: &str) -> String {
    let mut s = String::with_capacity(4096);
    s.push_str(MARKER_V2);
    s.push('\n');
    s.push_str("## Token efficiency (codeunlimited)\n\n");
    if !header.is_empty() {
        s.push_str(header);
        s.push_str("\n\n");
    }
    for t in techs {
        s.push_str("- ");
        s.push_str(body(t));
        s.push_str(" <!-- cu:");
        s.push_str(t.id);
        s.push_str(" -->\n");
    }
    s.push_str(MARKER_END);
    s.push('\n');
    s
}

pub fn render_claude(techs: &[&Technique]) -> String {
    render(
        techs,
        |t| t.claude,
        "Rules intended to reduce avoidable context within the same subscription limit. Each rule is \
         toggleable: `codeunlimited techniques` lists them; disable any via \
         .codeunlimited.toml -> [techniques] disable = [\"id\"]. Quality guardrail: \
         none of these may trade output quality for tokens - aggressive trades are \
         opt-in and say so.",
    )
}

pub fn render_agents(techs: &[&Technique]) -> String {
    render(techs, |t| t.agents, "")
}

/// `codeunlimited techniques`: print the catalog with per-config status.
pub fn list(cfg: &Config) -> i32 {
    let on = enabled(cfg);
    println!("Techniques (toggle via .codeunlimited.toml -> [techniques] enable/disable):\n");
    for t in CATALOG {
        let state = if on.iter().any(|e| e.id == t.id) {
            "on "
        } else {
            "off"
        };
        let risk = match t.risk {
            Risk::Low => "low risk   ",
            Risk::Medium => "guardrailed",
        };
        let opt = if t.default_on { "" } else { " (opt-in)" };
        println!(
            " [{state}] {:22} {risk}  since {}  {}{opt}",
            t.id, t.since, t.title
        );
    }
    println!(
        "\nRe-render blocks after toggling: codeunlimited init <project> \
         (upgrades v1/v2 blocks in place, backup kept)."
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(disable: &[&str], enable: &[&str]) -> Config {
        Config {
            techniques_disable: disable.iter().map(|s| s.to_string()).collect(),
            techniques_enable: enable.iter().map(|s| s.to_string()).collect(),
            ..Config::default()
        }
    }

    #[test]
    fn defaults_exclude_opt_in_techniques() {
        let on = enabled(&cfg(&[], &[]));
        assert!(on.iter().any(|t| t.id == "fresh-sessions"));
        assert!(on.iter().any(|t| t.id == "tool-output-budget"));
        assert!(!on.iter().any(|t| t.id == "reasoning-effort"));
        assert!(!on.iter().any(|t| t.id == "model-routing"));
    }

    #[test]
    fn toggles_work_both_ways() {
        let on = enabled(&cfg(&["delegate-mechanical"], &["model-routing"]));
        assert!(!on.iter().any(|t| t.id == "delegate-mechanical"));
        assert!(on.iter().any(|t| t.id == "model-routing"));
    }

    #[test]
    fn rendered_blocks_carry_markers_and_ids() {
        let on = enabled(&Config::default());
        for block in [render_claude(&on), render_agents(&on)] {
            assert!(block.starts_with(MARKER_V2));
            assert!(block.trim_end().ends_with(MARKER_END));
            assert!(block.contains("<!-- cu:fresh-sessions -->"));
            assert!(!block.contains("cu:reasoning-effort"), "opt-in stays out");
        }
    }

    #[test]
    fn medium_risk_texts_carry_guardrails() {
        for t in CATALOG.iter().filter(|t| t.risk == Risk::Medium) {
            assert!(
                t.claude.to_lowercase().contains("guardrail"),
                "{} lacks a quality guardrail",
                t.id
            );
        }
    }

    #[test]
    fn fresh_session_rule_is_conditional_on_context_reuse() {
        let technique = CATALOG
            .iter()
            .find(|technique| technique.id == "fresh-sessions")
            .expect("fresh-sessions technique");
        for text in [technique.claude, technique.agents] {
            assert!(text.contains("Batch small related tasks"));
            assert!(text.contains("prior context"));
            assert!(!text.contains("New task = new session"));
        }
    }
}
