//! Retro verdict: what a bounded-session discipline would have changed,
//! modeled over the project's existing history at first install.
//!
//! Observed totals are exact sums of recognized local log counters. The
//! counterfactual uses the same model as `scripts/bench_context.py` and
//! docs/BENCHMARK.md Layer 1: each qualifying session is replayed as if every
//! request had carried that session's early-request mean context. The result
//! is counterfactual exposure, never realized savings, and is labeled so.

use std::collections::BTreeMap;
use std::path::Path;

use crate::parsers;
use crate::types::Request;

/// Matches the published field-benchmark defaults (bench_context.py).
pub const DEFAULT_MIN_TURNS: usize = 30;
pub const DEFAULT_EARLY_TURNS: usize = 5;

/// Real-world logs are never byte-perfect: a handful of malformed lines in a
/// multi-year history must not withhold the whole verdict, or the first-install
/// experience dies on virtually every real machine. Malformed records up to
/// this share of the retained scope are DISCLOSED (exact count and share in
/// every rendering) instead of withholding; anything larger, or any failure
/// whose magnitude is unknowable (unreadable files, discovery errors, counter
/// overflow, missing timestamps), still withholds the model entirely.
pub const MAX_DISCLOSED_MALFORMED_SHARE: f64 = 0.001;

/// Completeness gate shared by `verdict` and the `init` baseline.
#[derive(Debug, PartialEq)]
pub enum AccountingGate {
    Complete,
    /// Only bounded malformed records; safe to proceed with disclosure.
    Disclosed {
        malformed: u64,
        share: f64,
    },
    Withheld,
}

pub fn accounting_gate(stats: &parsers::ScanStats, retained: usize) -> AccountingGate {
    if stats.complete_accounting() {
        return AccountingGate::Complete;
    }
    let only_malformed = stats.files_failed == 0
        && stats.discovery_errors == 0
        && !stats.counter_overflow
        && stats.malformed_records > 0;
    if !only_malformed {
        return AccountingGate::Withheld;
    }
    let denominator = (retained as u64).saturating_add(stats.malformed_records);
    if denominator == 0 {
        return AccountingGate::Withheld;
    }
    let share = stats.malformed_records as f64 / denominator as f64;
    if share <= MAX_DISCLOSED_MALFORMED_SHARE {
        AccountingGate::Disclosed {
            malformed: stats.malformed_records,
            share,
        }
    } else {
        AccountingGate::Withheld
    }
}

#[derive(Debug, Default, PartialEq)]
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
    pub positive_sessions: usize,
    pub negative_sessions: usize,
    pub zero_sessions: usize,
    pub overflow: bool,
}

impl Verdict {
    pub fn modeled_savings(&self) -> f64 {
        self.observed_tokens as f64 - self.modeled_tokens
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
    let mut by: BTreeMap<(&str, &str, &str), Vec<usize>> = BTreeMap::new();
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
    if early_turns == 0 || reqs.iter().any(|r| r.ts.is_none()) {
        return None;
    }
    let verdict = summarize(reqs, min_turns, early_turns);
    (verdict.sessions_included > 0 && !verdict.overflow).then_some(verdict)
}

fn checked_sum(values: impl Iterator<Item = u64>, overflow: &mut bool) -> u64 {
    values.fold(0u64, |total, value| {
        total.checked_add(value).unwrap_or_else(|| {
            *overflow = true;
            u64::MAX
        })
    })
}

fn summarize(reqs: &[Request], min_turns: usize, early_turns: usize) -> Verdict {
    let grouped = grouped_sessions(reqs);
    let mut overflow = false;
    let prompts: Vec<u64> = reqs
        .iter()
        .map(|r| {
            checked_sum(
                [r.unc_in, r.cached_in, r.w5, r.w1h].into_iter(),
                &mut overflow,
            )
        })
        .collect();
    let mut verdict = Verdict {
        sessions_total: grouped.len(),
        requests_total: reqs.len(),
        observed_all_tokens: checked_sum(prompts.iter().copied(), &mut overflow),
        ..Verdict::default()
    };
    for session in &grouped {
        if session.len() <= min_turns {
            continue;
        }
        let values: Vec<u64> = session.iter().map(|&i| prompts[i]).collect();
        let sample = early_turns.min(values.len());
        let early_mean =
            checked_sum(values[..sample].iter().copied(), &mut overflow) as f64 / sample as f64;
        let actual = checked_sum(values.iter().copied(), &mut overflow);
        let modeled = early_mean * values.len() as f64;
        if actual as f64 > modeled {
            verdict.positive_sessions += 1;
        } else if (actual as f64) < modeled {
            verdict.negative_sessions += 1;
        } else {
            verdict.zero_sessions += 1;
        }
        verdict.sessions_included += 1;
        verdict.requests_included += values.len();
        verdict.observed_tokens =
            checked_sum([verdict.observed_tokens, actual].into_iter(), &mut overflow);
        verdict.modeled_tokens += modeled;
    }
    verdict.overflow = overflow;
    verdict
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
         \x20 history: {} sessions / {} retained usage records / {} observed prompt tokens\n",
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
            "\x20 signed observed-minus-modeled difference: {} tokens (x{:.1})",
            m(verdict.modeled_savings()),
            ratio,
        ));
    }
    if let Some(extra) = verdict.extra_requests() {
        out.push_str(&format!(
            " ~ {:.0} record-equivalents at the observed average (NOT extra quota)\n",
            extra,
        ));
    } else {
        out.push('\n');
    }
    out.push_str(
        "\x20 the session floor is an analysis filter, not a universal break-even.\n\
         \x20 retained usage records are not guaranteed distinct model requests.\n",
    );
    out
}

pub fn render_json(verdict: &Verdict, min_turns: usize, early_turns: usize) -> String {
    report_value(
        verdict,
        min_turns,
        early_turns,
        &parsers::ScanStats::default(),
        0,
    )
    .to_string()
}

fn report_value(
    verdict: &Verdict,
    min_turns: usize,
    early_turns: usize,
    stats: &parsers::ScanStats,
    missing_ts: usize,
) -> serde_json::Value {
    let complete = stats.complete_accounting() && !verdict.overflow && missing_ts == 0;
    let gate = if verdict.overflow || missing_ts > 0 {
        AccountingGate::Withheld
    } else {
        accounting_gate(stats, verdict.requests_total)
    };
    let disclosed_share = match gate {
        AccountingGate::Disclosed { share, .. } => Some(share),
        _ => None,
    };
    let usable = complete || disclosed_share.is_some();
    let eligible = usable && verdict.sessions_included > 0;
    let mut warnings: Vec<String> = Vec::new();
    if let AccountingGate::Disclosed { malformed, share } = gate {
        warnings.push(format!(
            "Bounded incompleteness disclosed: {malformed} malformed records \
             ({:.4}% of retained scope) are excluded from exact totals and the \
             model; every other counter is complete.",
            share * 100.0
        ));
    } else if !stats.complete_accounting() {
        warnings.push(
            "Incomplete scan: invalid records or file/discovery errors; model withheld.".into(),
        );
    }
    if verdict.overflow {
        warnings.push("Counter aggregation overflow: exact totals and model withheld.".into());
    }
    if missing_ts > 0 {
        warnings
            .push("Missing timestamps: early-context ordering is unknown; model withheld.".into());
    }
    if stats.usage_without_cumulative_counters > 0 {
        warnings.push(
            "Some Codex records lack cumulative counters; no deduplication is inferred for those records."
                .into(),
        );
    }
    if stats.cumulative_resets > 0 {
        warnings.push(
            "Codex cumulative decreases were retained as new counter epochs, not reconstructed requests."
                .into(),
        );
    }
    serde_json::json!({
        "schema_version": 2,
        "status": if !usable { "incomplete" } else if verdict.sessions_included == 0 { "no_eligible_sessions" } else if disclosed_share.is_some() { "disclosed_incomplete" } else { "ok" },
        "complete_accounting": complete,
        "disclosed_malformed_share": disclosed_share,
        "overflow": verdict.overflow,
        "records_without_timestamp": missing_ts,
        "scan": stats,
        "warnings": warnings,
        "method": "observed prompt sum versus modeled early-context counterfactual",
        "label": "modeled counterfactual exposure, not realized savings",
        "record_unit": "retained usage records; not guaranteed distinct model requests",
        "deduplication": "Codex: consecutive equal cumulative AND last counters per file, before scope filters; missing cumulative counters retained",
        "min_turns_exclusive": min_turns,
        "early_turns": early_turns,
        "sessions_total": verdict.sessions_total,
        "sessions_included": verdict.sessions_included,
        "requests_total": verdict.requests_total,
        "requests_included": verdict.requests_included,
        "sessions_positive_modeled_difference": eligible.then_some(verdict.positive_sessions),
        "sessions_negative_modeled_difference": eligible.then_some(verdict.negative_sessions),
        "sessions_zero_modeled_difference": eligible.then_some(verdict.zero_sessions),
        "observed_prompt_tokens_all": (!verdict.overflow).then_some(verdict.observed_all_tokens),
        "observed_prompt_tokens_included": (!verdict.overflow).then_some(verdict.observed_tokens),
        "modeled_bounded_tokens_included": eligible.then_some(verdict.modeled_tokens),
        "modeled_difference_tokens": eligible.then(|| verdict.modeled_savings()),
        "modeled_savings_tokens": eligible.then(|| verdict.modeled_savings()),
        "observed_over_modeled_ratio": eligible.then(|| verdict.ratio()).flatten(),
        "extra_requests_at_observed_average": eligible.then(|| verdict.extra_requests()).flatten(),
    })
}

/// CLI entry point for `codeunlimited verdict`.
pub fn run(project: Option<&Path>, min_turns: usize, early_turns: usize, json: bool) -> i32 {
    if early_turns == 0 {
        eprintln!("--early-turns must be positive");
        return 2;
    }
    if project.is_some_and(|p| !p.is_dir()) {
        eprintln!("--project must name an existing directory");
        return 2;
    }
    let options = parsers::ScanOptions {
        project: project.map(Path::to_path_buf),
        since: None,
        use_index: false,
    };
    let cfg = crate::config::Config::load_for(project);
    let claude = parsers::scan_claude(&options);
    let codex = parsers::scan_codex(&options);
    let mut stats = claude.stats;
    stats += codex.stats;
    let mut reqs = claude.requests;
    reqs.extend(codex.requests);
    reqs.retain(|r| !cfg.is_ignored(&r.project));
    let verdict = summarize(&reqs, min_turns, early_turns);
    let missing_ts = reqs.iter().filter(|r| r.ts.is_none()).count();
    let value = report_value(&verdict, min_turns, early_turns, &stats, missing_ts);
    let withheld = value["status"] == "incomplete";
    if json {
        println!("{value}");
    } else {
        if withheld {
            println!("Incomplete accounting: modeled verdict withheld; inspect `verdict --json`.");
        } else if verdict.sessions_included == 0 {
            println!("No session in the scanned history exceeds {min_turns} retained usage records; nothing to model yet ({} records observed).", verdict.requests_total);
        } else {
            print!("{}", render(&verdict, min_turns));
        }
        for warning in value["warnings"].as_array().into_iter().flatten() {
            println!("warning: {}", warning.as_str().unwrap_or_default());
        }
    }
    if withheld {
        2
    } else {
        0
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
