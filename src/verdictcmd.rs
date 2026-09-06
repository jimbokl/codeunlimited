//! Retro verdict: what a bounded-session discipline would have changed,
//! modeled over the project's existing history at first install.
//!
//! Observed totals are exact sums of recognized local log counters. The
//! counterfactual uses the same model as `scripts/bench_context.py` and
//! docs/BENCHMARK.md Layer 1: each qualifying session is replayed as if every
//! request had carried that session's early-request mean context. The result
//! is counterfactual exposure, never realized savings, and is labeled so.

use std::collections::HashMap;
use std::path::Path;

use crate::parsers;
use crate::types::Request;

/// Matches the published field-benchmark defaults (bench_context.py).
pub const DEFAULT_MIN_TURNS: usize = 30;
pub const DEFAULT_EARLY_TURNS: usize = 5;

#[derive(Debug, PartialEq)]
pub struct Verdict {
    pub sessions_total: usize,
    pub sessions_included: usize,
    pub requests_total: usize,
    pub requests_included: usize,
    /// Exact observed prompt tokens across included sessions.
    pub observed_tokens: u64,
    /// Modeled bounded-context total for the same sessions.
    pub modeled_tokens: f64,
    /// Exact observed prompt tokens across all sessions (context for scale).
    pub observed_all_tokens: u64,
}

impl Verdict {
    pub fn modeled_savings(&self) -> f64 {
        (self.observed_tokens as f64 - self.modeled_tokens).max(0.0)
    }

    pub fn ratio(&self) -> Option<f64> {
        (self.modeled_tokens > 0.0).then(|| self.observed_tokens as f64 / self.modeled_tokens)
    }

    /// Modeled savings expressed as requests of work at the observed average.
    pub fn extra_requests(&self) -> Option<f64> {
        if self.requests_total == 0 || self.observed_all_tokens == 0 {
            return None;
        }
        let avg = self.observed_all_tokens as f64 / self.requests_total as f64;
        (avg > 0.0).then(|| self.modeled_savings() / avg)
    }
}

/// Group request indexes into sessions and order them by timestamp, mirroring
/// the detector grouping (source, project, session).
fn grouped_sessions(reqs: &[Request]) -> Vec<Vec<usize>> {
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

pub fn compute(reqs: &[Request], min_turns: usize, early_turns: usize) -> Option<Verdict> {
    if reqs.is_empty() || early_turns == 0 {
        return None;
    }
    let grouped = grouped_sessions(reqs);
    let mut verdict = Verdict {
        sessions_total: grouped.len(),
        sessions_included: 0,
        requests_total: reqs.len(),
        requests_included: 0,
        observed_tokens: 0,
        modeled_tokens: 0.0,
        observed_all_tokens: reqs.iter().map(Request::prompt_total).sum(),
    };
    for session in &grouped {
        if session.len() <= min_turns {
            continue;
        }
        let values: Vec<u64> = session.iter().map(|&i| reqs[i].prompt_total()).collect();
        let sample = early_turns.min(values.len());
        let early_mean = values[..sample].iter().sum::<u64>() as f64 / sample as f64;
        let actual: u64 = values.iter().sum();
        verdict.sessions_included += 1;
        verdict.requests_included += values.len();
        verdict.observed_tokens = verdict.observed_tokens.saturating_add(actual);
        verdict.modeled_tokens += early_mean * values.len() as f64;
    }
    (verdict.sessions_included > 0).then_some(verdict)
}

fn m(tokens: f64) -> String {
    format!("{:.1}M", tokens / 1e6)
}

/// Multi-line human verdict. `None` when no session clears the floor.
pub fn render(verdict: &Verdict, min_turns: usize) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Retro verdict - if bounded-session discipline had been active from the start\n\
         (modeled counterfactual exposure, NOT realized savings; method: BENCHMARK.md Layer 1):\n\
         \x20 history: {} sessions / {} requests / {} prompt tokens transported (observed, exact)\n",
        verdict.sessions_total,
        verdict.requests_total,
        m(verdict.observed_all_tokens as f64),
    ));
    out.push_str(&format!(
        "\x20 long sessions (>{} requests): {} sessions carrying {} observed\n",
        min_turns,
        verdict.sessions_included,
        m(verdict.observed_tokens as f64),
    ));
    out.push_str(&format!(
        "\x20 modeled bounded-context total for the same sessions: {}\n",
        m(verdict.modeled_tokens),
    ));
    if let Some(ratio) = verdict.ratio() {
        out.push_str(&format!(
            "\x20 modeled exposure: {} tokens (x{:.1})",
            m(verdict.modeled_savings()),
            ratio,
        ));
    }
    if let Some(extra) = verdict.extra_requests() {
        out.push_str(&format!(
            " ~ {:.0} extra requests of work at your observed average\n",
            extra,
        ));
    } else {
        out.push('\n');
    }
    out.push_str(
        "\x20 short sessions are excluded on purpose: below the measured break-even\n\
         \x20 (~7 requests) a fresh session costs more than it saves.\n",
    );
    out
}

pub fn render_json(verdict: &Verdict, min_turns: usize, early_turns: usize) -> String {
    serde_json::json!({
        "schema_version": 1,
        "method": "observed prompt sum versus modeled early-context counterfactual",
        "label": "modeled counterfactual exposure, not realized savings",
        "min_turns_exclusive": min_turns,
        "early_turns": early_turns,
        "sessions_total": verdict.sessions_total,
        "sessions_included": verdict.sessions_included,
        "requests_total": verdict.requests_total,
        "requests_included": verdict.requests_included,
        "observed_prompt_tokens_all": verdict.observed_all_tokens,
        "observed_prompt_tokens_included": verdict.observed_tokens,
        "modeled_bounded_tokens_included": verdict.modeled_tokens.round() as u64,
        "modeled_savings_tokens": verdict.modeled_savings().round() as u64,
        "observed_over_modeled_ratio": verdict.ratio(),
        "extra_requests_at_observed_average": verdict.extra_requests(),
    })
    .to_string()
}

/// CLI entry point for `codeunlimited verdict`.
pub fn run(project: Option<&Path>, min_turns: usize, early_turns: usize, json: bool) -> i32 {
    let options = parsers::ScanOptions {
        project: project.map(Path::to_path_buf),
        since: None,
        use_index: true,
    };
    let cfg = crate::config::Config::load_for(project);
    let mut reqs = parsers::scan_claude(&options).requests;
    reqs.extend(parsers::scan_codex(&options).requests);
    reqs.retain(|r| !cfg.is_ignored(&r.project));
    match compute(&reqs, min_turns, early_turns) {
        Some(verdict) => {
            if json {
                println!("{}", render_json(&verdict, min_turns, early_turns));
            } else {
                if let Some(p) = project {
                    println!("[scope: {}]", p.display());
                }
                print!("{}", render(&verdict, min_turns));
            }
            0
        }
        None => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "schema_version": 1,
                        "sessions_included": 0,
                        "note": "no session exceeds the min-turns floor; nothing to model",
                    })
                );
            } else {
                println!(
                    "No session in the scanned history exceeds {min_turns} requests - \
                     nothing to model yet. Re-run after some real work, or lower \
                     --min-turns (the published benchmark uses {DEFAULT_MIN_TURNS})."
                );
            }
            0
        }
    }
}

/// Compact first-install verdict appended to `init` output when history exists.
pub fn first_install_line(reqs: &[Request]) -> Option<String> {
    let verdict = compute(reqs, DEFAULT_MIN_TURNS, DEFAULT_EARLY_TURNS)?;
    let ratio = verdict.ratio()?;
    Some(format!(
        "  retro verdict: had bounded sessions been active from the start, the {} \
         long sessions here would have transported ~{} instead of {} \
         (x{:.1} modeled exposure, not realized savings; details: `codeunlimited verdict`)",
        verdict.sessions_included,
        m(verdict.modeled_tokens),
        m(verdict.observed_tokens as f64),
        ratio,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn req(session: &str, ts: i64, prompt: u64) -> Request {
        Request {
            source: "claude",
            project: Arc::from("proj"),
            session: Arc::from(session),
            ts: Some(ts),
            model: Arc::from("claude-test"),
            unc_in: prompt,
            cached_in: 0,
            w5: 0,
            w1h: 0,
            out: 10,
        }
    }

    fn growing_session(name: &str, turns: usize, start: u64, step: u64) -> Vec<Request> {
        (0..turns)
            .map(|i| req(name, i as i64, start + step * i as u64))
            .collect()
    }

    #[test]
    fn growing_session_yields_positive_modeled_savings() {
        let reqs = growing_session("s1", 40, 20_000, 5_000);
        let verdict = compute(&reqs, 30, 5).expect("verdict");
        assert_eq!(verdict.sessions_included, 1);
        assert_eq!(verdict.requests_included, 40);
        assert!(verdict.modeled_savings() > 0.0);
        assert!(verdict.ratio().expect("ratio") > 1.0);
    }

    #[test]
    fn flat_session_models_no_savings() {
        let reqs = growing_session("flat", 40, 50_000, 0);
        let verdict = compute(&reqs, 30, 5).expect("verdict");
        assert_eq!(verdict.modeled_savings(), 0.0);
        assert_eq!(verdict.ratio(), Some(1.0));
    }

    #[test]
    fn short_sessions_are_excluded_and_empty_history_is_none() {
        let reqs = growing_session("short", 5, 20_000, 5_000);
        assert_eq!(compute(&reqs, 30, 5), None);
        assert_eq!(compute(&[], 30, 5), None);
    }

    #[test]
    fn mixed_history_counts_only_long_sessions_but_reports_all() {
        let mut reqs = growing_session("long", 35, 20_000, 4_000);
        reqs.extend(growing_session("tiny", 3, 10_000, 1_000));
        let verdict = compute(&reqs, 30, 5).expect("verdict");
        assert_eq!(verdict.sessions_total, 2);
        assert_eq!(verdict.sessions_included, 1);
        assert_eq!(verdict.requests_total, 38);
        assert_eq!(verdict.requests_included, 35);
        assert!(verdict.observed_all_tokens > verdict.observed_tokens);
    }

    #[test]
    fn render_labels_the_number_as_modeled_not_realized() {
        let reqs = growing_session("s1", 40, 20_000, 5_000);
        let verdict = compute(&reqs, 30, 5).expect("verdict");
        let text = render(&verdict, 30);
        assert!(text.contains("NOT realized savings"));
        assert!(text.contains("modeled"));
        let json = render_json(&verdict, 30, 5);
        assert!(json.contains("not realized savings"));
    }

    #[test]
    fn first_install_line_is_present_only_with_qualifying_history() {
        let reqs = growing_session("s1", 40, 20_000, 5_000);
        assert!(first_install_line(&reqs)
            .expect("line")
            .contains("retro verdict"));
        let short = growing_session("s", 4, 20_000, 5_000);
        assert_eq!(first_install_line(&short), None);
    }
}
