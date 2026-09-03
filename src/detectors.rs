//! Detectors: find where subscription-limit tokens leak, in limit currency.

use std::collections::HashMap;

use crate::types::Request;

const HEAVY: [&str; 3] = ["fable", "mythos", "opus"];
const EARLY_N: usize = 5;

fn trivial_out() -> u64 {
    crate::config::get().trivial_output_tokens
}
fn long_session() -> usize {
    crate::config::get().long_session_turns
}

pub struct Finding {
    pub title: String,
    /// Mid estimate - what the reports headline.
    pub impact_tokens: u64,
    /// Honest range around the estimate (docs/ACCURACY.md documents the
    /// assumption behind each bound). For directly-measured waste lo == hi.
    pub impact_lo: u64,
    pub impact_hi: u64,
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
        if r.out > 0 && r.out < trivial_out() && HEAVY.iter().any(|h| r.model.contains(h)) {
            n += 1;
            toks += r.prompt_total();
        }
    }
    Finding {
        title: "Top-tier model burned on mechanical replies".into(),
        impact_tokens: toks / 2, // conservative: half realistically delegable
        impact_lo: toks / 4,
        impact_hi: toks * 3 / 4,
        detail: format!(
            "{} requests to top-tier models ended in a reply shorter than {} tokens \
             while dragging {:.0}M tokens of context.",
            n,
            trivial_out(),
            toks as f64 / 1e6
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
    let long = long_session();
    for rows in sessions(reqs) {
        if rows.len() < long {
            continue;
        }
        let early = &rows[..EARLY_N.min(rows.len())];
        let early_avg =
            early.iter().map(|r| r.prompt_total() as f64).sum::<f64>() / early.len() as f64;
        let late = &rows[long..];
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
        impact_lo: (excess * 0.4) as u64,
        impact_hi: (excess * 0.8) as u64,
        detail: format!(
            "{} sessions ran past {} turns; by the tail of a session each turn costs \
             on average x{:.1} of an early turn.",
            hit,
            long_session(),
            g
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
        impact_tokens: brk + ttl, // directly measured waste - no range needed
        impact_lo: brk + ttl,
        impact_hi: brk + ttl,
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
    let fat = crate::config::get().fat_start_tokens;
    let med = firsts.get(firsts.len() / 2).copied().unwrap_or(0);
    let over: u64 = firsts.iter().map(|w| w.saturating_sub(fat)).sum();
    Finding {
        title: "Fat session starts (tool/MCP schemas in the system prompt)".into(),
        impact_tokens: over / 2,
        impact_lo: over / 4,
        impact_hi: over * 3 / 4,
        detail: format!(
            "The median first request of a session writes {:.0}k tokens of context; \
             anything above ~{}k is usually schemas of unused MCP servers and tools.",
            med as f64 / 1000.0,
            fat / 1000
        ),
        fix: "Disable unused MCP servers per project (.mcp.json / `claude mcp remove`) \
              - their schemas are paid out of your limit on every new session."
            .into(),
    }
}

fn retry_storms(reqs: &[Request]) -> Finding {
    // Same prompt size, back to back, within seconds - a re-sent request.
    // Token-count heuristics only; prompts are never read, so we demand a
    // strong signal: >=3 identical prompt sizes with <=90s gaps.
    let mut dup_toks = 0u64;
    let mut dups = 0u64;
    for rows in sessions(reqs) {
        let mut i = 0;
        while i < rows.len() {
            let mut j = i + 1;
            while j < rows.len()
                && rows[j].prompt_total() == rows[i].prompt_total()
                && rows[i].prompt_total() > 1000
                && rows[j]
                    .ts
                    .zip(rows[j - 1].ts)
                    .is_some_and(|(a, b)| a - b <= 90)
            {
                j += 1;
            }
            if j - i >= 3 {
                dups += (j - i - 1) as u64;
                dup_toks += rows[i + 1..j].iter().map(|r| r.prompt_total()).sum::<u64>();
            }
            i = j.max(i + 1);
        }
    }
    Finding {
        title: "Retry storms - the same request re-sent in bursts".into(),
        impact_tokens: dup_toks / 2, // size-equality is a heuristic, discount half
        impact_lo: dup_toks / 4,
        impact_hi: dup_toks * 3 / 4,
        detail: format!(
            "{dups} duplicate-sized requests fired in bursts (identical prompt size, \
             <=90s apart, 3+ in a row) - usually auto-retries after errors or \
             double-submits."
        ),
        fix: "Check for flaky MCP servers / network errors that trigger silent \
              retries; a failing tool that the agent retries in a loop burns the \
              full context every attempt."
            .into(),
    }
}

pub fn run_all(reqs: &[Request]) -> Vec<Finding> {
    let mut f = vec![
        heavy_model_on_trivial(reqs),
        context_tax(reqs),
        cache_rewrites(reqs),
        heavy_session_start(reqs),
        retry_storms(reqs),
    ];
    f.sort_by_key(|x| std::cmp::Reverse(x.impact_tokens));
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(session: &str, ts: i64, prompt: u64, out: u64) -> Request {
        Request {
            source: "claude",
            project: "p".into(),
            session: session.into(),
            ts: Some(ts),
            model: "claude-opus-5".into(),
            unc_in: 0,
            cached_in: 0,
            w5: prompt,
            w1h: 0,
            out,
        }
    }

    #[test]
    fn retry_storm_detects_bursts_only() {
        // 4 identical-size requests 30s apart -> 3 duplicates counted.
        let storm: Vec<Request> = (0..4).map(|i| req("s1", i * 30, 50_000, 10)).collect();
        let f = retry_storms(&storm);
        assert!(f.detail.starts_with("3 duplicate"));
        assert_eq!(f.impact_tokens, 3 * 50_000 / 2);
        assert!(f.impact_lo <= f.impact_tokens && f.impact_tokens <= f.impact_hi);

        // Same sizes but 10 minutes apart -> not a storm.
        let slow: Vec<Request> = (0..4).map(|i| req("s2", i * 600, 50_000, 10)).collect();
        assert_eq!(retry_storms(&slow).impact_tokens, 0);

        // Only 2 in a row -> below the 3+ threshold.
        let pair: Vec<Request> = (0..2).map(|i| req("s3", i * 30, 50_000, 10)).collect();
        assert_eq!(retry_storms(&pair).impact_tokens, 0);
    }

    #[test]
    fn ranges_bracket_the_estimate() {
        let reqs: Vec<Request> = (0..40)
            .map(|i| req("s", i * 60, (10_000 + i * 5_000) as u64, 200))
            .collect();
        for f in run_all(&reqs) {
            assert!(f.impact_lo <= f.impact_tokens, "{}", f.title);
            assert!(f.impact_tokens <= f.impact_hi, "{}", f.title);
        }
    }

    #[test]
    fn context_tax_flags_growing_sessions() {
        let reqs: Vec<Request> = (0..40)
            .map(|i| req("s", i * 60, (10_000 + i * 5_000) as u64, 200))
            .collect();
        let f = context_tax(&reqs);
        assert!(f.impact_tokens > 0);
        assert!(f.detail.contains("1 sessions ran past 30 turns"));
    }
}
