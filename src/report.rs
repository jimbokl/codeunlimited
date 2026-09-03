//! Console report: findings converted into limit currency (reclaimed work).

use std::collections::BTreeMap;

use crate::detectors::{self, Finding};
use crate::parsers::ScanStats;
use crate::types::Request;

const BAR: &str = "================================================================";

/// Machine-readable variant of the report for scripting (`audit --json`).
pub fn render_json(
    reqs: &[Request],
    findings: &[Finding],
    scan_stats: Option<&ScanStats>,
) -> String {
    let mut per_src: BTreeMap<&str, (u64, u64, u64)> = BTreeMap::new();
    let mut per_proj: BTreeMap<String, u64> = BTreeMap::new();
    let mut tmin = i64::MAX;
    let mut tmax = i64::MIN;
    for r in reqs {
        let s = per_src.entry(r.source).or_default();
        s.0 = s.0.saturating_add(1);
        s.1 = s.1.saturating_add(r.prompt_total());
        s.2 = s.2.saturating_add(r.out);
        let project_total = per_proj
            .entry(format!("{}:{}", r.source, r.project))
            .or_default();
        *project_total = project_total.saturating_add(r.total());
        if let Some(t) = r.ts {
            tmin = tmin.min(t);
            tmax = tmax.max(t);
        }
    }
    let total_tokens = per_src.values().fold(0u64, |total, source| {
        total.saturating_add(source.1.saturating_add(source.2))
    });
    let days = if tmin < tmax {
        ((tmax.saturating_sub(tmin) as f64) / 86_400.0)
            .floor()
            .max(1.0)
    } else {
        1.0
    };
    let weekly = total_tokens as f64 / days * 7.0;
    let mut projects: Vec<_> = per_proj.into_iter().collect();
    projects.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
    projects.truncate(8);
    let reclaimed = detectors::reclaim_total(findings);
    let mut obj = serde_json::json!({
        "schema_version": 1,
        "period_days": days as u64,
        "sources": per_src.iter().map(|(src, s)| (src.to_string(), serde_json::json!({
            "requests": s.0, "prompt_tokens": s.1, "output_tokens": s.2,
        }))).collect::<serde_json::Map<_,_>>(),
        "weekly_volume_tokens": weekly as u64,
        "reclaimable_tokens": reclaimed,
        "reclaimable_pct_of_weekly": if weekly > 0.0 {
            (100.0 * (reclaimed as f64 / days * 7.0) / weekly) as u64
        } else { 0 },
        "findings": findings.iter().filter(|f| f.impact_tokens > 0).map(|f| serde_json::json!({
            "key": f.key,
            "title": f.title,
            "impact_tokens": f.impact_tokens,
            "impact_lo": f.impact_lo,
            "impact_hi": f.impact_hi,
            "detail": f.detail,
            "fix": f.fix,
        })).collect::<Vec<_>>(),
        "top_projects": projects.iter().map(|(p, t)| serde_json::json!({
            "project": p, "total_tokens": t,
        })).collect::<Vec<_>>(),
    });
    if let Some(stats) = scan_stats {
        obj["scan"] = serde_json::to_value(stats).unwrap_or_default();
    }
    serde_json::to_string_pretty(&obj).unwrap_or_default()
}

pub fn render(reqs: &[Request], findings: &[Finding], color: bool) -> String {
    let (bold, green, yellow, dim, off) = if color {
        ("\x1b[1m", "\x1b[32m", "\x1b[33m", "\x1b[2m", "\x1b[0m")
    } else {
        ("", "", "", "", "")
    };
    if reqs.is_empty() {
        return "No data: no Claude Code logs (~/.claude/projects) or Codex logs \
                (~/.codex/sessions) found."
            .into();
    }
    let mut per_src: BTreeMap<&str, (u64, u64, u64)> = BTreeMap::new();
    let mut per_proj: BTreeMap<String, u64> = BTreeMap::new();
    let mut tmin = i64::MAX;
    let mut tmax = i64::MIN;
    for r in reqs {
        let s = per_src.entry(r.source).or_default();
        s.0 = s.0.saturating_add(1);
        s.1 = s.1.saturating_add(r.prompt_total());
        s.2 = s.2.saturating_add(r.out);
        let project_total = per_proj
            .entry(format!("{}:{}", r.source, r.project))
            .or_default();
        *project_total = project_total.saturating_add(r.total());
        if let Some(t) = r.ts {
            tmin = tmin.min(t);
            tmax = tmax.max(t);
        }
    }
    let total_tokens = per_src.values().fold(0u64, |total, source| {
        total.saturating_add(source.1.saturating_add(source.2))
    });
    let out_total = per_src
        .values()
        .fold(0u64, |total, source| total.saturating_add(source.2));
    let days = if tmin < tmax {
        ((tmax.saturating_sub(tmin) as f64) / 86_400.0)
            .floor()
            .max(1.0)
    } else {
        1.0
    };
    let weekly = total_tokens as f64 / days * 7.0;
    let avg_out = out_total as f64 / reqs.len() as f64;

    let mut l: Vec<String> = Vec::new();
    l.push(format!("{green}{BAR}{off}"));
    l.push(format!(
        "{green}{bold} CODEUNLIMITED{off} - more code out of the limits you already pay for"
    ));
    l.push(format!("{green}{BAR}{off}"));
    if tmin < tmax {
        let d0 = chrono::DateTime::from_timestamp(tmin, 0)
            .unwrap()
            .date_naive();
        let d1 = chrono::DateTime::from_timestamp(tmax, 0)
            .unwrap()
            .date_naive();
        l.push(format!(" Period: {d0} ... {d1}  ({days:.0} days)"));
    }
    for (src, s) in &per_src {
        l.push(format!(
            " {:6}: {:>6} requests | context {:>8.0}M tok. | code/answers {:>6.1}M tok.",
            src,
            s.0,
            s.1 as f64 / 1e6,
            s.2 as f64 / 1e6
        ));
    }
    l.push(format!(
        " Weekly volume (limit proxy): ~{:.0}M tokens",
        weekly / 1e6
    ));
    l.push(String::new());
    l.push(" FINDINGS - where your limit leaks (by impact):".into());
    l.push(String::new());

    let reclaimed = detectors::reclaim_total(findings);
    let mut i = 0;
    for f in findings.iter().filter(|f| f.impact_tokens > 0) {
        i += 1;
        let pct = 100.0 * (f.impact_tokens as f64 / days * 7.0) / weekly.max(1.0);
        let answers = f.impact_tokens as f64
            * (out_total as f64 / total_tokens.saturating_sub(out_total).max(1) as f64)
            / avg_out.max(1.0);
        l.push(format!(" {yellow}{bold}{}. {}{off}", i, f.title));
        l.push(format!("    {}", f.detail));
        let range = if f.impact_lo != f.impact_hi {
            format!(
                " [range {:.0}-{:.0}M]",
                f.impact_lo as f64 / 1e6,
                f.impact_hi as f64 / 1e6
            )
        } else {
            String::new()
        };
        l.push(format!(
            "    Reclaim: ~{:.0}M tok.{range} (~{:.0}% of weekly volume, ~{:.0} extra agent replies)",
            f.impact_tokens as f64 / 1e6,
            pct,
            answers
        ));
        l.push(format!("    {dim}Fix: {}{off}", f.fix));
        l.push(String::new());
    }

    let pct_all = 100.0 * (reclaimed as f64 / days * 7.0) / weekly.max(1.0);
    l.push(format!("{green}{BAR}{off}"));
    l.push(format!(
        " {green}{bold}TOTAL reclaimable: ~{:.0}M tokens ~ {:.0}% of weekly volume{off} - \
         that much more work fits into the same limit.",
        reclaimed as f64 / 1e6,
        pct_all
    ));
    l.push(format!("{green}{BAR}{off}"));
    l.push(" Top projects by volume:".into());
    let mut projects: Vec<_> = per_proj.into_iter().collect();
    projects.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
    for (p, t) in projects.into_iter().take(8) {
        l.push(format!("   {:44} {:>8.0}M tok.", p, t as f64 / 1e6));
    }
    l.push(String::new());
    l.push(
        " Next: codeunlimited init <project> - efficiency rules into CLAUDE.md/AGENTS.md".into(),
    );
    l.join("\n")
}
