//! Per-project efficiency metrics - the basis for verified before/after deltas.

use std::collections::HashMap;

use crate::types::Request;

const LONG_SESSION: usize = 30;
const EARLY_N: usize = 5;

#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub requests: u64,
    pub sessions: u64,
    pub avg_prompt_per_turn: f64,
    /// Mean late/early prompt-size ratio across long sessions (1.0 = flat).
    pub context_growth: f64,
}

pub fn compute(reqs: &[Request]) -> Metrics {
    let mut by: HashMap<&str, Vec<&Request>> = HashMap::new();
    for r in reqs {
        by.entry(r.session.as_str()).or_default().push(r);
    }
    let mut growth = Vec::new();
    for rows in by.values_mut() {
        rows.sort_by_key(|r| (r.ts.is_none(), r.ts));
        if rows.len() < LONG_SESSION {
            continue;
        }
        let early = &rows[..EARLY_N];
        let early_avg =
            early.iter().map(|r| r.prompt_total() as f64).sum::<f64>() / early.len() as f64;
        let late = &rows[LONG_SESSION..];
        let late_avg =
            late.iter().map(|r| r.prompt_total() as f64).sum::<f64>() / late.len().max(1) as f64;
        if early_avg > 0.0 {
            growth.push(late_avg / early_avg);
        }
    }
    let prompt_total: u64 = reqs.iter().map(|r| r.prompt_total()).sum();
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

pub fn to_json(m: &Metrics, created_unix: i64) -> String {
    serde_json::json!({
        "created_unix": created_unix,
        "source": "claude",
        "metrics": {
            "requests": m.requests,
            "sessions": m.sessions,
            "avg_prompt_per_turn": m.avg_prompt_per_turn as u64,
            "context_growth": (m.context_growth * 100.0).round() / 100.0,
        }
    })
    .to_string()
}
