//! Detectors: find where subscription-limit tokens leak, in limit currency.

use std::collections::HashMap;

use crate::config::Config;
use crate::types::Request;

const HEAVY: [&str; 3] = ["fable", "mythos", "opus"];
const EARLY_N: usize = 5;

#[derive(Debug, Clone, Copy)]
pub struct ImpactClaim {
    pub request_index: usize,
    pub mid: u64,
    pub lo: u64,
    pub hi: u64,
}

pub struct Finding {
    /// Stable machine-readable identifier.
    pub key: &'static str,
    pub title: String,
    /// Mid estimate - what the reports headline.
    pub impact_tokens: u64,
    /// Honest range around the estimate (docs/ACCURACY.md documents the
    /// assumption behind each bound). For directly-measured waste lo == hi.
    pub impact_lo: u64,
    pub impact_hi: u64,
    pub detail: String,
    pub fix: String,
    /// Per-request estimates allow report totals to form a conservative union
    /// instead of counting the same token under multiple detectors.
    pub claims: Vec<ImpactClaim>,
}

fn sum_claims(claims: &[ImpactClaim], pick: impl Fn(&ImpactClaim) -> u64) -> u64 {
    claims
        .iter()
        .fold(0, |total, claim| total.saturating_add(pick(claim)))
}

fn finding(
    key: &'static str,
    title: &str,
    claims: Vec<ImpactClaim>,
    detail: String,
    fix: &str,
) -> Finding {
    Finding {
        key,
        title: title.into(),
        impact_tokens: sum_claims(&claims, |claim| claim.mid),
        impact_lo: sum_claims(&claims, |claim| claim.lo),
        impact_hi: sum_claims(&claims, |claim| claim.hi),
        detail,
        fix: fix.into(),
        claims,
    }
}

fn portion(amount: u64, percent: u64) -> u64 {
    ((amount as u128 * percent as u128) / 100).min(u64::MAX as u128) as u64
}

fn estimated_claim(
    request_index: usize,
    amount: u64,
    mid_percent: u64,
    lo_percent: u64,
    hi_percent: u64,
) -> ImpactClaim {
    ImpactClaim {
        request_index,
        mid: portion(amount, mid_percent),
        lo: portion(amount, lo_percent),
        hi: portion(amount, hi_percent),
    }
}

fn sessions(reqs: &[Request]) -> Vec<Vec<usize>> {
    let mut by: HashMap<(&str, &str, &str), Vec<usize>> = HashMap::new();
    for (index, request) in reqs.iter().enumerate() {
        by.entry((
            request.source,
            request.project.as_ref(),
            request.session.as_ref(),
        ))
        .or_default()
        .push(index);
    }
    let mut out: Vec<Vec<usize>> = by.into_values().collect();
    for rows in &mut out {
        rows.sort_by_key(|index| {
            let request = &reqs[*index];
            (request.ts.is_none(), request.ts)
        });
    }
    out
}

fn heavy_model_on_trivial(reqs: &[Request], cfg: &Config) -> Finding {
    let mut claims = Vec::new();
    let mut count = 0u64;
    let mut tokens = 0u64;
    for (index, request) in reqs.iter().enumerate() {
        if request.out > 0
            && request.out < cfg.trivial_output_tokens
            && HEAVY.iter().any(|heavy| request.model.contains(heavy))
        {
            let amount = request.prompt_total();
            count = count.saturating_add(1);
            tokens = tokens.saturating_add(amount);
            claims.push(estimated_claim(index, amount, 50, 25, 75));
        }
    }
    finding(
        "heavy_model_trivial_output",
        "Top-tier model burned on mechanical replies",
        claims,
        format!(
            "{} requests to top-tier models ended in a reply shorter than {} tokens \
             while dragging {:.0}M tokens of context.",
            count,
            cfg.trivial_output_tokens,
            tokens as f64 / 1e6
        ),
        "Delegate mechanical work (renames, repetitive edits, status checks) to \
         subagents on a light model / low effort: add a delegation rule to \
         CLAUDE.md; in Claude Code use Task with model: haiku.",
    )
}

fn context_tax(reqs: &[Request], cfg: &Config, grouped: &[Vec<usize>]) -> Finding {
    let mut claims = Vec::new();
    let mut hit = 0u64;
    let mut growth = Vec::new();
    let long = cfg.long_session_turns;
    for rows in grouped {
        if rows.len() <= long {
            continue;
        }
        let early_count = EARLY_N.min(long).min(rows.len());
        if early_count == 0 {
            continue;
        }
        let early = &rows[..early_count];
        let early_avg = early
            .iter()
            .map(|index| reqs[*index].prompt_total() as f64)
            .sum::<f64>()
            / early.len() as f64;
        let late = &rows[long..];
        let mut session_excess = 0u64;
        for index in late {
            let excess = (reqs[*index].prompt_total() as f64 - early_avg).max(0.0) as u64;
            session_excess = session_excess.saturating_add(excess);
            if excess > 0 {
                claims.push(estimated_claim(*index, excess, 60, 40, 80));
            }
        }
        if session_excess > 0 {
            hit = hit.saturating_add(1);
            let late_avg = late
                .iter()
                .map(|index| reqs[*index].prompt_total() as f64)
                .sum::<f64>()
                / late.len() as f64;
            if early_avg > 0.0 {
                growth.push(late_avg / early_avg);
            }
        }
    }
    let average_growth = if growth.is_empty() {
        0.0
    } else {
        growth.iter().sum::<f64>() / growth.len() as f64
    };
    finding(
        "long_session_context_tax",
        "Context tax of long sessions",
        claims,
        format!(
            "{} sessions ran past {} turns; by the tail of a session each turn costs \
             on average x{:.1} of an early turn.",
            hit, cfg.long_session_turns, average_growth
        ),
        "New task = new session (/clear). For long repetitive loops keep a compact \
         state file instead of conversation history (SKILL.state pattern, \
         arXiv 2608.26263) - `codeunlimited init` adds the rule to CLAUDE.md.",
    )
}

fn cache_rewrites(reqs: &[Request], grouped: &[Vec<usize>]) -> Finding {
    let (mut breaks, mut ttl, mut unknown) = (0u64, 0u64, 0u64);
    let (mut break_events, mut ttl_events, mut unknown_events) = (0u64, 0u64, 0u64);
    let mut claims = Vec::new();
    for rows in grouped {
        for position in 1..rows.len() {
            let index = rows[position];
            let request = &reqs[index];
            if request.source != "claude" || request.cached_in > 0 {
                continue;
            }
            let written = request.w5.saturating_add(request.w1h);
            if written < 2_000 {
                continue;
            }
            let previous = &reqs[rows[position - 1]];
            let (Some(a), Some(b)) = (request.ts, previous.ts) else {
                unknown = unknown.saturating_add(written);
                unknown_events = unknown_events.saturating_add(1);
                continue;
            };
            let gap = a.saturating_sub(b);
            let limit = if request.w1h > 0 || previous.w1h > 0 {
                3_600
            } else {
                300
            };
            if gap > limit {
                ttl = ttl.saturating_add(written);
                ttl_events = ttl_events.saturating_add(1);
                continue;
            }
            breaks = breaks.saturating_add(written);
            break_events = break_events.saturating_add(1);
            claims.push(ImpactClaim {
                request_index: index,
                mid: written,
                lo: written,
                hi: written,
            });
        }
    }
    finding(
        "mid_session_cache_rewrites",
        "Mid-session cache re-writes",
        claims,
        format!(
            "{} prefix breaks ({:.1}M tok.), {} TTL expirations ({:.1}M tok.), and \
             {} {} with an unknown gap ({:.1}M tok.) re-paid for context instead of \
             reading it back from cache. TTL expirations and unknown gaps are diagnostic \
             only and excluded from reclaimable totals.",
            break_events,
            breaks as f64 / 1e6,
            ttl_events,
            ttl as f64 / 1e6,
            unknown_events,
            if unknown_events == 1 {
                "rewrite"
            } else {
                "rewrites"
            },
            unknown as f64 / 1e6
        ),
        "Breaks: move mutating blocks (timestamps, dynamic state) out of the \
         prompt prefix. Expirations: avoid 5+ minute pauses mid-task.",
    )
}

fn heavy_session_start(reqs: &[Request], cfg: &Config, grouped: &[Vec<usize>]) -> Finding {
    let firsts: Vec<(usize, u64)> = grouped
        .iter()
        .filter_map(|rows| rows.first().copied())
        .filter(|index| reqs[*index].source == "claude")
        .map(|index| {
            let request = &reqs[index];
            (index, request.w5.saturating_add(request.w1h))
        })
        .collect();
    let mut sizes: Vec<u64> = firsts.iter().map(|(_, written)| *written).collect();
    sizes.sort_unstable();
    let median = sizes.get(sizes.len() / 2).copied().unwrap_or(0);
    let claims = firsts
        .into_iter()
        .filter_map(|(index, written)| {
            let excess = written.saturating_sub(cfg.fat_start_tokens);
            (excess > 0).then(|| estimated_claim(index, excess, 50, 25, 75))
        })
        .collect();
    finding(
        "fat_session_start",
        "Fat session starts (tool/MCP schemas in the system prompt)",
        claims,
        format!(
            "The median first request of a session writes {:.0}k tokens of context; \
             anything above ~{}k is usually schemas of unused MCP servers and tools.",
            median as f64 / 1_000.0,
            cfg.fat_start_tokens / 1_000
        ),
        "Disable unused MCP servers per project (.mcp.json / `claude mcp remove`) \
         - their schemas are paid out of your limit on every new session.",
    )
}

fn retry_storms(reqs: &[Request], grouped: &[Vec<usize>]) -> Finding {
    // Same prompt size, back to back, within seconds - a re-sent request.
    // Token-count heuristics only; prompts are never read, so we demand a
    // strong signal: >=3 identical prompt sizes with <=90s gaps.
    let mut claims = Vec::new();
    let mut duplicates = 0u64;
    for rows in grouped {
        let mut i = 0;
        while i < rows.len() {
            let mut j = i + 1;
            while j < rows.len()
                && reqs[rows[j]].prompt_total() == reqs[rows[i]].prompt_total()
                && reqs[rows[i]].prompt_total() > 1_000
                && reqs[rows[j]]
                    .ts
                    .zip(reqs[rows[j - 1]].ts)
                    .is_some_and(|(a, b)| a.saturating_sub(b) <= 90)
            {
                j += 1;
            }
            if j - i >= 3 {
                duplicates = duplicates.saturating_add((j - i - 1) as u64);
                claims.extend(
                    rows[i + 1..j].iter().map(|index| {
                        estimated_claim(*index, reqs[*index].prompt_total(), 50, 25, 75)
                    }),
                );
            }
            i = j.max(i + 1);
        }
    }
    finding(
        "retry_storm",
        "Retry storms - the same request re-sent in bursts",
        claims,
        format!(
            "{duplicates} duplicate-sized requests fired in bursts (identical prompt size, \
             <=90s apart, 3+ in a row) - usually auto-retries after errors or \
             double-submits."
        ),
        "Check for flaky MCP servers / network errors that trigger silent \
         retries; a failing tool that the agent retries in a loop burns the \
         full context every attempt.",
    )
}

pub fn run_all(reqs: &[Request], cfg: &Config) -> Vec<Finding> {
    let grouped = sessions(reqs);
    let mut findings = vec![
        heavy_model_on_trivial(reqs, cfg),
        context_tax(reqs, cfg, &grouped),
        cache_rewrites(reqs, &grouped),
        heavy_session_start(reqs, cfg, &grouped),
        retry_storms(reqs, &grouped),
    ];
    findings.sort_by_key(|finding| std::cmp::Reverse(finding.impact_tokens));
    findings
}

/// Conservative union of all findings. If multiple detectors claim the same
/// request, only the largest mid estimate is included in the headline total.
pub fn reclaim_total(findings: &[Finding]) -> u64 {
    let claim_count = findings
        .iter()
        .map(|finding| finding.claims.len())
        .sum::<usize>();
    let max_index = findings
        .iter()
        .flat_map(|finding| &finding.claims)
        .map(|claim| claim.request_index)
        .max();
    if let Some(max_index) = max_index {
        if max_index <= claim_count.saturating_mul(4).max(1_024) {
            let mut by_request = vec![0u64; max_index.saturating_add(1)];
            for claim in findings.iter().flat_map(|finding| &finding.claims) {
                by_request[claim.request_index] = by_request[claim.request_index].max(claim.mid);
            }
            return by_request.into_iter().fold(0u64, u64::saturating_add);
        }
    }
    let mut by_request: HashMap<usize, u64> = HashMap::new();
    for claim in findings.iter().flat_map(|finding| &finding.claims) {
        by_request
            .entry(claim.request_index)
            .and_modify(|current| *current = (*current).max(claim.mid))
            .or_insert(claim.mid);
    }
    by_request.into_values().fold(0u64, u64::saturating_add)
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
        let storm: Vec<Request> = (0..4).map(|i| req("s1", i * 30, 50_000, 10)).collect();
        let finding = retry_storms(&storm, &sessions(&storm));
        assert!(finding.detail.starts_with("3 duplicate"));
        assert_eq!(finding.impact_tokens, 3 * 50_000 / 2);
        assert!(finding.impact_lo <= finding.impact_tokens);
        assert!(finding.impact_tokens <= finding.impact_hi);

        let slow: Vec<Request> = (0..4).map(|i| req("s2", i * 600, 50_000, 10)).collect();
        assert_eq!(retry_storms(&slow, &sessions(&slow)).impact_tokens, 0);

        let pair: Vec<Request> = (0..2).map(|i| req("s3", i * 30, 50_000, 10)).collect();
        assert_eq!(retry_storms(&pair, &sessions(&pair)).impact_tokens, 0);
    }

    #[test]
    fn ranges_bracket_the_estimate() {
        let reqs: Vec<Request> = (0..40)
            .map(|i| req("s", i * 60, (10_000 + i * 5_000) as u64, 200))
            .collect();
        for finding in run_all(&reqs, &Config::default()) {
            assert!(
                finding.impact_lo <= finding.impact_tokens,
                "{}",
                finding.title
            );
            assert!(
                finding.impact_tokens <= finding.impact_hi,
                "{}",
                finding.title
            );
        }
    }

    #[test]
    fn context_tax_flags_growing_sessions() {
        let reqs: Vec<Request> = (0..40)
            .map(|i| req("s", i * 60, (10_000 + i * 5_000) as u64, 200))
            .collect();
        let finding = context_tax(&reqs, &Config::default(), &sessions(&reqs));
        assert!(finding.impact_tokens > 0);
        assert!(finding.detail.contains("1 sessions ran past 30 turns"));
    }

    #[test]
    fn ttl_expiration_is_not_reclaimable() {
        let rows = vec![req("s", 0, 50_000, 10), req("s", 301, 50_000, 10)];
        let finding = cache_rewrites(&rows, &sessions(&rows));
        assert_eq!(finding.impact_tokens, 0);
        assert!(finding.detail.contains("1 TTL expiration"));
    }

    #[test]
    fn unknown_cache_gap_is_not_reclaimable() {
        let mut rows = vec![req("s", 0, 50_000, 10), req("s", 30, 50_000, 10)];
        rows[1].ts = None;

        let finding = cache_rewrites(&rows, &sessions(&rows));

        assert_eq!(finding.impact_tokens, 0);
        assert!(finding.detail.contains("1 rewrite with an unknown gap"));
    }

    #[test]
    fn uncached_prompt_is_not_a_fat_session_start() {
        let mut row = req("s", 0, 0, 10);
        row.unc_in = 100_000;

        let rows = [row];
        let finding = heavy_session_start(&rows, &Config::default(), &sessions(&rows));

        assert_eq!(finding.impact_tokens, 0);
        assert!(finding.detail.contains("0k tokens"));
    }

    #[test]
    fn short_session_tail_is_not_used_in_its_own_baseline() {
        let rows = vec![
            req("s", 0, 10_000, 10),
            req("s", 1, 10_000, 10),
            req("s", 2, 100_000, 10),
        ];
        let cfg = Config {
            long_session_turns: 2,
            ..Config::default()
        };

        let finding = context_tax(&rows, &cfg, &sessions(&rows));

        assert!(finding.impact_tokens > 0);
    }

    #[test]
    fn total_uses_union_for_overlapping_findings() {
        let rows = vec![req("s", 0, 100_000, 10)];
        let findings = run_all(&rows, &Config::default());
        assert_eq!(reclaim_total(&findings), 50_000);
    }
}
