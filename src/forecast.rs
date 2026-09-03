//! Limit forecast: how many hours of work remain before the wall.
//!
//! Codex logs carry `rate_limits.primary.used_percent`, which lets us
//! calibrate the window's token capacity from the user's own data:
//! capacity ~ volume_in_window / used_percent. Claude Code logs carry no
//! limit telemetry, so the busiest observed week serves as a proxy ceiling.
//! Both are estimates and labeled as such (see docs/ACCURACY.md).

use std::collections::HashMap;

use crate::parsers::LimitSeries;
use crate::types::Request;

const MIN_CALIBRATION_PCT: f64 = 20.0;

pub fn forecast(reqs: &[Request], series: &LimitSeries) -> Vec<String> {
    let mut lines = Vec::new();

    // --- Codex: calibrate capacity at the highest-confidence observation ---
    let codex: Vec<&Request> = reqs.iter().filter(|r| r.source == "codex").collect();
    let calib = series
        .iter()
        .filter(|s| s.1 >= MIN_CALIBRATION_PCT && s.2 > 0)
        .max_by(|a, b| a.1.total_cmp(&b.1));
    if let (Some(&(ts_pk, used_pk, win_pk)), Some(&(_, used_now, _))) = (calib, series.last()) {
        let win_secs = win_pk.min(i64::MAX as u64 / 60) as i64 * 60;
        let vol_in_win: u64 = codex
            .iter()
            .filter(|r| {
                r.ts.is_some_and(|t| t > ts_pk.saturating_sub(win_secs) && t <= ts_pk)
            })
            .fold(0u64, |total, request| total.saturating_add(request.total()));
        if vol_in_win > 0 {
            let capacity = vol_in_win as f64 / (used_pk / 100.0);
            let remaining = capacity * (1.0 - used_now / 100.0);
            let anchor = codex.iter().filter_map(|r| r.ts).max().unwrap_or(ts_pk);
            let day_vol: u64 = codex
                .iter()
                .filter(|r| {
                    r.ts.is_some_and(|t| t > anchor.saturating_sub(86_400) && t <= anchor)
                })
                .fold(0u64, |total, request| total.saturating_add(request.total()));
            if day_vol > 0 {
                let hours = remaining / (day_vol as f64 / 24.0);
                lines.push(format!(
                    "codex: ~{:.0}% of the {:.0}-day window used as of the last session; \
                     ~{:.0}M of an estimated ~{:.0}M-token window left - about {:.0}h of \
                     work at your last-24h pace.",
                    used_now,
                    win_pk as f64 / 1440.0,
                    remaining / 1e6,
                    capacity / 1e6,
                    hours
                ));
            }
        }
    }

    // --- Claude: busiest observed week as the proxy ceiling ---
    let claude: Vec<&Request> = reqs.iter().filter(|r| r.source == "claude").collect();
    if let Some(anchor) = claude.iter().filter_map(|r| r.ts).max() {
        let mut weeks: HashMap<i64, u64> = HashMap::new();
        for r in &claude {
            if let Some(t) = r.ts {
                let total = weeks.entry(t / (7 * 86_400)).or_default();
                *total = total.saturating_add(r.total());
            }
        }
        let busiest = weeks.values().copied().max().unwrap_or(0);
        let trailing: u64 = claude
            .iter()
            .filter(|r| r.ts.is_some_and(|t| t > anchor.saturating_sub(7 * 86_400)))
            .fold(0u64, |total, request| total.saturating_add(request.total()));
        if busiest > 0 && trailing > 0 {
            lines.push(format!(
                "claude: trailing 7 days = {:.0}M tokens, {:.0}% of your busiest \
                 observed week ({:.0}M) - the proxy ceiling until Claude logs expose \
                 limit telemetry.",
                trailing as f64 / 1e6,
                100.0 * trailing as f64 / busiest as f64,
                busiest as f64 / 1e6
            ));
        }
    }
    lines
}

/// Downsample the rate-limit series for charting: keep the max used_percent
/// per day. Returns (unix_day_ts, used_percent).
pub fn daily_peaks(series: &LimitSeries) -> Vec<(i64, f64)> {
    let mut by_day: HashMap<i64, f64> = HashMap::new();
    for &(ts, used, _) in series {
        if ts == 0 {
            continue;
        }
        let day = ts - ts.rem_euclid(86_400);
        let e = by_day.entry(day).or_insert(0.0);
        if used > *e {
            *e = used;
        }
    }
    let mut out: Vec<(i64, f64)> = by_day.into_iter().collect();
    out.sort_unstable_by_key(|&(d, _)| d);
    out
}
