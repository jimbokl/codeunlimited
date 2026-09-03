//! `codeunlimited report`: a saved, shareable Markdown report for a project -
//! current limit leaks, verified delta vs the `init` baseline, and a trend
//! table that grows with every run (snapshots in `.codeunlimited.history.jsonl`).

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;

use serde_json::Value;

use crate::detectors::{self, Finding};
use crate::types::Request;
use crate::{deltacmd, metrics, parsers};

pub const HISTORY_FILE: &str = ".codeunlimited.history.jsonl";
pub const REPORT_FILE: &str = "CODEUNLIMITED_REPORT.md";

/// Delta vs the baseline captured by `init` (claude source only, like `delta`).
pub struct DeltaInfo {
    pub since: String,
    pub b_requests: u64,
    pub b_prompt: f64,
    pub b_growth: f64,
    pub now: metrics::Metrics,
}

struct Volume {
    days: f64,
    weekly: f64,
    out_total: u64,
    total: u64,
    avg_out: f64,
    period: Option<(String, String)>,
}

fn volume(reqs: &[Request]) -> Volume {
    let mut tmin = i64::MAX;
    let mut tmax = i64::MIN;
    let mut out_total = 0u64;
    let mut total = 0u64;
    for r in reqs {
        out_total += r.out;
        total += r.total();
        if let Some(t) = r.ts {
            tmin = tmin.min(t);
            tmax = tmax.max(t);
        }
    }
    let days = if tmin < tmax {
        (((tmax - tmin) as f64) / 86_400.0).floor().max(1.0)
    } else {
        1.0
    };
    let date = |t| {
        chrono::DateTime::from_timestamp(t, 0)
            .map(|d| d.date_naive().to_string())
            .unwrap_or_else(|| "?".into())
    };
    Volume {
        days,
        weekly: total as f64 / days * 7.0,
        out_total,
        total,
        avg_out: out_total as f64 / reqs.len().max(1) as f64,
        period: (tmin < tmax).then(|| (date(tmin), date(tmax))),
    }
}

/// Pure Markdown builder - unit-testable without touching the filesystem.
pub fn build_markdown(
    name: &str,
    reqs: &[Request],
    findings: &[Finding],
    delta: Option<&DeltaInfo>,
    history: &[Value],
    generated: &str,
) -> String {
    let v = volume(reqs);
    let mut per_src: BTreeMap<&str, (u64, u64, u64)> = BTreeMap::new();
    for r in reqs {
        let s = per_src.entry(r.source).or_default();
        s.0 += 1;
        s.1 += r.prompt_total();
        s.2 += r.out;
    }

    let mut l: Vec<String> = Vec::new();
    l.push(format!("# codeunlimited report - {name}"));
    l.push(String::new());
    l.push(format!(
        "Generated {generated}. All data local; token counts only - prompts are never read."
    ));
    l.push(String::new());
    l.push("## Usage".into());
    l.push(String::new());
    if let Some((d0, d1)) = &v.period {
        l.push(format!(
            "Period: {d0} ... {d1} ({:.0} days). Weekly volume (limit proxy): ~{:.0}M tokens.",
            v.days,
            v.weekly / 1e6
        ));
        l.push(String::new());
    }
    l.push("| source | requests | context, M tok | code/answers, M tok |".into());
    l.push("|---|---:|---:|---:|".into());
    for (src, s) in &per_src {
        l.push(format!(
            "| {src} | {} | {:.0} | {:.1} |",
            s.0,
            s.1 as f64 / 1e6,
            s.2 as f64 / 1e6
        ));
    }
    l.push(String::new());

    l.push("## Where the limit leaks".into());
    l.push(String::new());
    let mut reclaimed = 0u64;
    let mut i = 0;
    for f in findings.iter().filter(|f| f.impact_tokens > 0) {
        i += 1;
        reclaimed += f.impact_tokens;
        let pct = 100.0 * (f.impact_tokens as f64 / v.days * 7.0) / v.weekly.max(1.0);
        let answers = f.impact_tokens as f64
            * (v.out_total as f64 / (v.total - v.out_total).max(1) as f64)
            / v.avg_out.max(1.0);
        l.push(format!("### {i}. {}", f.title));
        l.push(String::new());
        l.push(f.detail.clone());
        l.push(String::new());
        l.push(format!(
            "**Reclaim:** ~{:.0}M tokens (~{pct:.0}% of weekly volume, ~{answers:.0} extra agent replies).",
            f.impact_tokens as f64 / 1e6
        ));
        l.push(String::new());
        l.push(format!("**Fix:** {}", f.fix));
        l.push(String::new());
    }
    if i == 0 {
        l.push("No significant leaks detected in this window.".into());
        l.push(String::new());
    } else {
        let pct_all = 100.0 * (reclaimed as f64 / v.days * 7.0) / v.weekly.max(1.0);
        l.push(format!(
            "**Total reclaimable: ~{:.0}M tokens ~ {pct_all:.0}% of weekly volume** - \
             that much more work fits into the same limit.",
            reclaimed as f64 / 1e6
        ));
        l.push(String::new());
    }

    if let Some(d) = delta {
        l.push(format!("## Delta since baseline ({})", d.since));
        l.push(String::new());
        l.push("| metric | baseline | now |".into());
        l.push("|---|---:|---:|".into());
        l.push(format!(
            "| requests analyzed | {} | {} |",
            d.b_requests, d.now.requests
        ));
        l.push(format!(
            "| avg context per turn | {}k | {}k |",
            (d.b_prompt / 1e3).round() as u64,
            (d.now.avg_prompt_per_turn / 1e3).round() as u64
        ));
        l.push(format!(
            "| long-session context growth | {:.1}x | {:.1}x |",
            d.b_growth, d.now.context_growth
        ));
        l.push(String::new());
        if d.b_prompt > 0.0 {
            let change = 100.0 * (d.now.avg_prompt_per_turn - d.b_prompt) / d.b_prompt;
            l.push(if change <= -1.0 {
                format!(
                    "**Verdict:** context per turn is down {:.0}% - about {:.0}% more \
                     work now fits into the same limit.",
                    -change,
                    100.0 * (d.b_prompt / d.now.avg_prompt_per_turn.max(1.0) - 1.0)
                )
            } else if change >= 1.0 {
                format!(
                    "**Verdict:** context per turn is up {change:.0}% - the leaks are \
                     growing; see the findings above."
                )
            } else {
                "**Verdict:** flat so far - keep the rules on and re-check later.".into()
            });
            l.push(String::new());
        }
    }

    if !history.is_empty() {
        l.push("## Trend".into());
        l.push(String::new());
        l.push("One row per `codeunlimited report` run - watch the numbers fall.".into());
        l.push(String::new());
        l.push(
            "| date | requests | avg context/turn | session growth | reclaimable, M tok |".into(),
        );
        l.push("|---|---:|---:|---:|---:|".into());
        for h in history {
            l.push(format!(
                "| {} | {} | {}k | {:.1}x | {:.0} |",
                h["date"].as_str().unwrap_or("?"),
                h["requests"].as_u64().unwrap_or(0),
                h["avg_prompt_per_turn"].as_u64().unwrap_or(0) / 1000,
                h["context_growth"].as_f64().unwrap_or(1.0),
                h["reclaimable_tokens"].as_u64().unwrap_or(0) as f64 / 1e6,
            ));
        }
        l.push(String::new());
    }

    l.push("---".into());
    l.push(
        "*[codeunlimited](https://github.com/jimbokl/codeunlimited) - more code out of \
         the subscription limits you already pay for.*"
            .into(),
    );
    l.push(String::new());
    l.join("\n")
}

fn strip_verbatim(s: &str) -> &str {
    s.strip_prefix(r"\\?\").unwrap_or(s)
}

pub fn run(path: &Path, out: Option<&Path>) -> i32 {
    let root = match path.canonicalize() {
        Ok(p) if p.is_dir() => p,
        _ => {
            eprintln!("No such directory: {}", path.display());
            return 1;
        }
    };
    let disp = root.to_string_lossy();
    let disp = strip_verbatim(&disp).to_string();
    let name = root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| disp.clone());

    let mut reqs = parsers::iter_claude(Some(&root));
    reqs.extend(parsers::iter_codex(Some(&root)));
    if reqs.is_empty() {
        eprintln!(
            "No local history for this project yet ({disp}) - work a while, \
             then re-run `codeunlimited report`."
        );
        return 1;
    }
    let findings = detectors::run_all(&reqs);

    let delta = std::fs::read_to_string(root.join(deltacmd::BASELINE_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|bl| {
            let created = bl["created_unix"].as_i64()?;
            let cl: Vec<Request> = reqs
                .iter()
                .filter(|r| r.source == "claude" && r.ts.is_some_and(|t| t >= created))
                .cloned()
                .collect();
            if cl.is_empty() {
                return None;
            }
            Some(DeltaInfo {
                since: chrono::DateTime::from_timestamp(created, 0)
                    .map(|d| d.date_naive().to_string())
                    .unwrap_or_else(|| "?".into()),
                b_requests: bl["metrics"]["requests"].as_u64().unwrap_or(0),
                b_prompt: bl["metrics"]["avg_prompt_per_turn"].as_u64().unwrap_or(0) as f64,
                b_growth: bl["metrics"]["context_growth"].as_f64().unwrap_or(1.0),
                now: metrics::compute(&cl),
            })
        });

    // Append today's snapshot to the trend history (one line per run).
    let v = volume(&reqs);
    let m = metrics::compute(&reqs);
    let reclaimed: u64 = findings.iter().map(|f| f.impact_tokens).sum();
    let now = chrono::Utc::now();
    let snap = serde_json::json!({
        "ts": now.timestamp(),
        "date": now.date_naive().to_string(),
        "requests": m.requests,
        "avg_prompt_per_turn": m.avg_prompt_per_turn as u64,
        "context_growth": (m.context_growth * 100.0).round() / 100.0,
        "weekly_volume_tokens": v.weekly as u64,
        "reclaimable_tokens": reclaimed,
    });
    let hist_path = root.join(HISTORY_FILE);
    let appended = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&hist_path)
        .and_then(|mut f| writeln!(f, "{snap}"));
    if let Err(e) = appended {
        eprintln!("Cannot write {}: {e}", hist_path.display());
    }
    let history: Vec<Value> = std::fs::read_to_string(&hist_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|ln| serde_json::from_str(ln).ok())
        .collect();

    let md = build_markdown(
        &name,
        &reqs,
        &findings,
        delta.as_ref(),
        &history,
        &now.date_naive().to_string(),
    );
    let out_path = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join(REPORT_FILE));
    if let Err(e) = std::fs::write(&out_path, md) {
        eprintln!("Cannot write {}: {e}", out_path.display());
        return 1;
    }
    let out_disp = out_path.to_string_lossy();
    println!(" Report written: {}", strip_verbatim(&out_disp));
    println!(
        " Snapshot #{} recorded in {} - the trend table grows with every run.",
        history.len(),
        HISTORY_FILE
    );
    0
}
