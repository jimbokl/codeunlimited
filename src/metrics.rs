//! Per-project efficiency metrics - the basis for before/after observations.

use std::collections::HashMap;

use crate::types::Request;

const EARLY_N: usize = 5;

#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub requests: u64,
    pub sessions: u64,
    pub avg_prompt_per_turn: f64,
    /// Mean late/early prompt-size ratio across long sessions (1.0 = flat).
    pub context_growth: f64,
}

pub fn compute(reqs: &[Request], long_session_turns: usize) -> Metrics {
    let mut by: HashMap<(&str, &str, &str), Vec<&Request>> = HashMap::new();
    for r in reqs {
        by.entry((r.source, r.project.as_ref(), r.session.as_ref()))
            .or_default()
            .push(r);
    }
    let mut growth = Vec::new();
    for rows in by.values_mut() {
        rows.sort_by_key(|r| (r.ts.is_none(), r.ts));
        if rows.len() <= long_session_turns {
            continue;
        }
        let early = &rows[..EARLY_N.min(long_session_turns).min(rows.len())];
        let early_avg =
            early.iter().map(|r| r.prompt_total() as f64).sum::<f64>() / early.len() as f64;
        let late = &rows[long_session_turns..];
        let late_avg =
            late.iter().map(|r| r.prompt_total() as f64).sum::<f64>() / late.len() as f64;
        if early_avg > 0.0 {
            growth.push(late_avg / early_avg);
        }
    }
    let prompt_total = reqs.iter().fold(0u64, |total, request| {
        total.saturating_add(request.prompt_total())
    });
    Metrics {
        requests: reqs.len() as u64,
        sessions: by.len() as u64,
        avg_prompt_per_turn: prompt_total as f64 / reqs.len().max(1) as f64,
        context_growth: if growth.is_empty() {
            1.0
        } else {
            growth.iter().sum::<f64>() / growth.len() as f64
        },
    }
}

fn metrics_json(m: &Metrics) -> serde_json::Value {
    serde_json::json!({
        "requests": m.requests,
        "sessions": m.sessions,
        "avg_prompt_per_turn": m.avg_prompt_per_turn as u64,
        "context_growth": (m.context_growth * 100.0).round() / 100.0,
    })
}

pub fn to_json(m: &Metrics, created_unix: i64) -> String {
    serde_json::json!({
        "created_unix": created_unix,
        "source": "claude",
        "metrics": metrics_json(m),
    })
    .to_string()
}

/// Baseline format v2: one metrics block per source (claude + codex).
pub fn to_json_multi(sources: &[(&str, Metrics)], created_unix: i64) -> String {
    let map: serde_json::Map<String, serde_json::Value> = sources
        .iter()
        .map(|(src, m)| (src.to_string(), metrics_json(m)))
        .collect();
    serde_json::json!({ "created_unix": created_unix, "sources": map }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(turn: i64) -> Request {
        Request {
            source: "claude",
            project: "p".into(),
            session: "s".into(),
            ts: Some(turn),
            model: "m".into(),
            unc_in: 100,
            cached_in: 0,
            w5: 0,
            w1h: 0,
            out: 1,
        }
    }

    #[test]
    fn threshold_without_a_tail_is_flat() {
        let rows: Vec<_> = (0..30).map(request).collect();
        assert_eq!(compute(&rows, 30).context_growth, 1.0);
    }

    #[test]
    fn equal_session_ids_in_different_projects_stay_separate() {
        let mut first = request(0);
        first.project = "first".into();
        let mut second = request(1);
        second.project = "second".into();

        assert_eq!(compute(&[first, second], 30).sessions, 2);
    }
}
