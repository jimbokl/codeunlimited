//! Detectors: find where subscription-limit tokens leak, in limit currency.

use std::collections::HashMap;

use crate::types::Request;

const HEAVY: [&str; 3] = ["fable", "mythos", "opus"];
const TRIVIAL_OUT: u64 = 300;
const LONG_SESSION: usize = 30;
const EARLY_N: usize = 5;

pub struct Finding {
    pub title: String,
    pub impact_tokens: u64,
    pub detail: String,
    pub fix: String,
}

fn sessions(reqs: &[Request]) -> Vec<Vec<&Request>> {
    let mut by: HashMap<(&str, &str, &str), Vec<&Request>> = HashMap::new();
    for r in reqs {
        by.entry((r.source, &r.project, &r.session))
            .or_default()
            .push(r);
    }
    let mut out: Vec<Vec<&Request>> = by.into_values().collect();
    for rows in &mut out {
        rows.sort_by_key(|r| (r.ts.is_none(), r.ts));
    }
    out
}

fn heavy_model_on_trivial(reqs: &[Request]) -> Finding {
    let mut n = 0u64;
    let mut toks = 0u64;
    for r in reqs {
        if r.out > 0 && r.out < TRIVIAL_OUT && HEAVY.iter().any(|h| r.model.contains(h)) {
            n += 1;
            toks += r.prompt_total();
        }
    }
    Finding {
        title: "Top-tier model burned on mechanical replies".into(),
        impact_tokens: toks / 2, // conservative: half realistically delegable
        detail: format!(
            "{} requests to top-tier models ended in a reply shorter than {} tokens \
             while dragging {:.0}M tokens of context.",
            n, TRIVIAL_OUT, toks as f64 / 1e6
        ),
        fix: "Delegate mechanical work (renames, repetitive edits, status checks) to \
              subagents on a light model / low effort: add a delegation rule to \
              CLAUDE.md; in Claude Code use Task with model: haiku."
            .into(),
    }
}

fn context_tax(reqs: &[Request]) -> Finding {
    let mut excess = 0f64;
    let mut hit = 0u64;
    let mut growth = Vec::new();
    for rows in sessions(reqs) {
        if rows.len() < LONG_SESSION {
            continue;
        }
        let early = &rows[..EARLY_N.min(rows.len())];
        let early_avg =
            early.iter().map(|r| r.prompt_total() as f64).sum::<f64>() / early.len() as f64;
        let late = &rows[LONG_SESSION..];
        let e: f64 = late
            .iter()
            .map(|r| (r.prompt_total() as f64 - early_avg).max(0.0))
            .sum();
        if e > 0.0 {
            hit += 1;
            excess += e;
            let late_avg =
                late.iter().map(|r| r.prompt_total() as f64).sum::<f64>() / late.len() as f64;
            if early_avg > 0.0 {
                growth.push(late_avg / early_avg);
            }
        }
    }
    let g = if growth.is_empty() {
        0.0
    } else {
        growth.iter().sum::<f64>() / growth.len() as f64
    };
    Finding {
        title: "Context tax of long sessions".into(),
        impact_tokens: (excess * 0.6) as u64, // conservative: part of context is needed
        detail: format!(
            "{} sessions ran past {} turns; by the tail of a session each turn costs \
             on average x{:.1} of an early turn.",
            hit, LONG_SESSION, g
        ),
        fix: "New task = new session (/clear). For long repetitive loops keep a compact \
              state file instead of conversation history (SKILL.state pattern, \
              arXiv 2608.26263) - `codeunlimited init` adds the rule to CLAUDE.md."
            .into(),
    }
}

fn cache_rewrites(reqs: &[Request]) -> Finding {
    let (mut brk, mut ttl, mut brk_ev, mut ttl_ev) = (0u64, 0u64, 0u64, 0u64);
    for rows in sessions(reqs) {
        for i in 1..rows.len() {
            let r = rows[i];
            if r.source != "claude" || r.cached_in > 0 {
                continue;
            }
            let w = r.w5 + r.w1h;
            if w < 2000 {
                continue;
            }
            let prev = rows[i - 1];
            if let (Some(a), Some(b)) = (r.ts, prev.ts) {
                let gap = a - b;
                let limit = if r.w1h > 0 || prev.w1h > 0 { 3600 } else { 300 };
                if gap > limit {
                    ttl += w;
                    ttl_ev += 1;
                    continue;
                }
            }
            brk += w;
            brk_ev += 1;
        }
    }
    Finding {
        title: "Mid-session cache re-writes".into(),
        impact_tokens: brk + ttl,
        detail: format!(
            "{} prefix breaks ({:.1}M tok.) and {} TTL expirations ({:.1}M tok.) \
             re-paid for context instead of reading it back from cache.",
            brk_ev,
            brk as f64 / 1e6,
            ttl_ev,
            ttl as f64 / 1e6
        ),
        fix: "Breaks: move mutating blocks (timestamps, dynamic state) out of the \
              prompt prefix. Expirations: avoid 5+ minute pauses mid-task."
            .into(),
    }
}

fn heavy_session_start(reqs: &[Request]) -> Finding {
    let mut firsts: Vec<u64> = sessions(reqs)
        .into_iter()
        .filter_map(|rows| rows.first().copied().cloned())
        .filter(|r| r.source == "claude")
        .map(|r| r.w5 + r.w1h + r.unc_in)
        .filter(|w| *w > 0)
        .collect();
    firsts.sort_unstable();
    let med = firsts.get(firsts.len() / 2).copied().unwrap_or(0);
    let over: u64 = firsts.iter().map(|w| w.saturating_sub(25_000)).sum();
    Finding {
        title: "Fat session starts (tool/MCP schemas in the system prompt)".into(),
        impact_tokens: over / 2,
        detail: format!(
            "The median first request of a session writes {:.0}k tokens of context; \
             anything above ~25k is usually schemas of unused MCP servers and tools.",
            med as f64 / 1000.0
        ),
        fix: "Disable unused MCP servers per project (.mcp.json / `claude mcp remove`) \
              - their schemas are paid out of your limit on every new session."
            .into(),
    }
}

pub fn run_all(reqs: &[Request]) -> Vec<Finding> {
    let mut f = vec![
        heavy_model_on_trivial(reqs),
        context_tax(reqs),
        cache_rewrites(reqs),
        heavy_session_start(reqs),
    ];
    f.sort_by_key(|x| std::cmp::Reverse(x.impact_tokens));
    f
}
