//! `codeunlimited report`: saved, shareable reports - Markdown + a styled
//! self-contained HTML page. Scoped to one project, or `--all` for a summary
//! across every registered project. Each run appends a snapshot, so the trend
//! grows over time.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde_json::Value;

use crate::detectors::{self, Finding};
use crate::types::Request;
use crate::{deltacmd, forecast, html, metrics, parsers, registry};

pub const HISTORY_FILE: &str = ".codeunlimited.history.jsonl";
pub const REPORT_FILE: &str = "CODEUNLIMITED_REPORT.md";
pub const SUMMARY_FILE: &str = "CODEUNLIMITED_SUMMARY.md";

/// Delta vs a baseline captured by `init`, one entry per source with activity.
pub struct DeltaInfo {
    pub source: String,
    pub since: String,
    pub b_requests: u64,
    pub b_prompt: f64,
    pub b_growth: f64,
    pub now: metrics::Metrics,
}

pub struct SrcUsage {
    pub source: String,
    pub requests: u64,
    pub prompt: u64,
    pub out: u64,
}

/// A finding with its limit-currency numbers pre-computed for rendering.
pub struct FindingView {
    pub title: String,
    pub detail: String,
    pub fix: String,
    pub tokens: u64,
    pub lo: u64,
    pub hi: u64,
    pub pct: f64,
    pub answers: f64,
}

/// `--all` mode: one project's delta row.
pub struct ProjectDelta {
    pub project: String,
    pub delta: DeltaInfo,
}

/// Everything the renderers need - computed once, rendered as MD and HTML.
pub struct ReportData {
    pub name: String,
    pub generated: String,
    pub period: Option<(String, String, f64)>,
    pub weekly: f64,
    pub sources: Vec<SrcUsage>,
    pub findings: Vec<FindingView>,
    pub reclaim: u64,
    pub reclaim_pct: f64,
    pub deltas: Vec<DeltaInfo>,
    pub project_deltas: Vec<ProjectDelta>,
    pub top_projects: Vec<(String, u64)>,
    pub history: Vec<Value>,
    /// Limit-forecast lines (codex calibration + claude proxy ceiling).
    pub forecast: Vec<String>,
    /// Daily rate-limit peaks for the timeline: (date, used_percent).
    pub limit_days: Vec<(String, f64)>,
}

#[allow(clippy::too_many_arguments)]
pub fn collect(
    name: &str,
    reqs: &[Request],
    findings: &[Finding],
    deltas: Vec<DeltaInfo>,
    project_deltas: Vec<ProjectDelta>,
    top_projects: Vec<(String, u64)>,
    history: Vec<Value>,
    generated: &str,
) -> ReportData {
    let mut per_src: BTreeMap<&str, (u64, u64, u64)> = BTreeMap::new();
    let mut tmin = i64::MAX;
    let mut tmax = i64::MIN;
    let mut out_total = 0u64;
    let mut total = 0u64;
    for r in reqs {
        let s = per_src.entry(r.source).or_default();
        s.0 += 1;
        s.1 += r.prompt_total();
        s.2 += r.out;
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
    let weekly = total as f64 / days * 7.0;
    let avg_out = out_total as f64 / reqs.len().max(1) as f64;
    let date = |t| {
        chrono::DateTime::from_timestamp(t, 0)
            .map(|d| d.date_naive().to_string())
            .unwrap_or_else(|| "?".into())
    };

    let views: Vec<FindingView> = findings
        .iter()
        .filter(|f| f.impact_tokens > 0)
        .map(|f| FindingView {
            title: f.title.clone(),
            detail: f.detail.clone(),
            fix: f.fix.clone(),
            tokens: f.impact_tokens,
            lo: f.impact_lo,
            hi: f.impact_hi,
            pct: 100.0 * (f.impact_tokens as f64 / days * 7.0) / weekly.max(1.0),
            answers: f.impact_tokens as f64
                * (out_total as f64 / (total - out_total).max(1) as f64)
                / avg_out.max(1.0),
        })
        .collect();
    let reclaim: u64 = views.iter().map(|v| v.tokens).sum();
    ReportData {
        name: name.into(),
        generated: generated.into(),
        period: (tmin < tmax).then(|| (date(tmin), date(tmax), days)),
        weekly,
        sources: per_src
            .into_iter()
            .map(|(source, (requests, prompt, out))| SrcUsage {
                source: source.into(),
                requests,
                prompt,
                out,
            })
            .collect(),
        reclaim_pct: 100.0 * (reclaim as f64 / days * 7.0) / weekly.max(1.0),
        findings: views,
        reclaim,
        deltas,
        project_deltas,
        top_projects,
        history,
        forecast: vec![],
        limit_days: vec![],
    }
}

/// Text progress bar for Markdown: `▰▰▰▰▱▱▱▱▱▱`.
fn bar(pct: f64) -> String {
    let filled = ((pct / 10.0).round() as usize).min(10);
    "▰".repeat(filled) + &"▱".repeat(10 - filled)
}

pub fn delta_change(d: &DeltaInfo) -> Option<f64> {
    (d.b_prompt > 0.0).then(|| 100.0 * (d.now.avg_prompt_per_turn - d.b_prompt) / d.b_prompt)
}

pub fn verdict_line(d: &DeltaInfo) -> String {
    match delta_change(d) {
        Some(c) if c <= -1.0 => format!(
            "↓ context per turn is down {:.0}% - about {:.0}% more work now fits \
             into the same limit.",
            -c,
            100.0 * (d.b_prompt / d.now.avg_prompt_per_turn.max(1.0) - 1.0)
        ),
        Some(c) if c >= 1.0 => {
            format!("↑ context per turn is up {c:.0}% - the leaks are growing.")
        }
        Some(_) => "→ flat so far - keep the rules on and re-check later.".into(),
        None => String::new(),
    }
}

pub fn build_markdown(d: &ReportData) -> String {
    let mut l: Vec<String> = Vec::new();
    l.push(format!("# codeunlimited report - {}", d.name));
    l.push(String::new());
    l.push(format!(
        "Generated {}. All data local; token counts only - prompts are never read.",
        d.generated
    ));
    l.push(String::new());
    l.push("## Usage".into());
    l.push(String::new());
    if let Some((d0, d1, days)) = &d.period {
        l.push(format!(
            "Period: {d0} ... {d1} ({days:.0} days). Weekly volume (limit proxy): ~{:.0}M tokens.",
            d.weekly / 1e6
        ));
        l.push(String::new());
    }
    l.push("| source | requests | context, M tok | code/answers, M tok |".into());
    l.push("|---|---:|---:|---:|".into());
    for s in &d.sources {
        l.push(format!(
            "| {} | {} | {:.0} | {:.1} |",
            s.source,
            s.requests,
            s.prompt as f64 / 1e6,
            s.out as f64 / 1e6
        ));
    }
    l.push(String::new());

    l.push("## Where the limit leaks".into());
    l.push(String::new());
    for (i, f) in d.findings.iter().enumerate() {
        l.push(format!("### {}. {}", i + 1, f.title));
        l.push(String::new());
        l.push(format!("`{}` {:.0}% of weekly volume", bar(f.pct), f.pct));
        l.push(String::new());
        l.push(f.detail.clone());
        l.push(String::new());
        let range = if f.lo != f.hi {
            format!(
                " (range {:.0}-{:.0}M)",
                f.lo as f64 / 1e6,
                f.hi as f64 / 1e6
            )
        } else {
            String::new()
        };
        l.push(format!(
            "**Reclaim:** ~{:.0}M tokens{range} (~{:.0} extra agent replies).",
            f.tokens as f64 / 1e6,
            f.answers
        ));
        l.push(String::new());
        l.push(format!("**Fix:** {}", f.fix));
        l.push(String::new());
    }
    if d.findings.is_empty() {
        l.push("No significant leaks detected in this window.".into());
        l.push(String::new());
    } else {
        l.push(format!(
            "**Total reclaimable: ~{:.0}M tokens ~ {:.0}% of weekly volume** - \
             that much more work fits into the same limit.",
            d.reclaim as f64 / 1e6,
            d.reclaim_pct
        ));
        l.push(String::new());
    }

    if !d.forecast.is_empty() {
        l.push("## Limit forecast".into());
        l.push(String::new());
        for f in &d.forecast {
            l.push(format!("- {f}"));
        }
        l.push(String::new());
    }

    if !d.limit_days.is_empty() {
        l.push("## Rate-limit peaks (codex, daily max)".into());
        l.push(String::new());
        l.push("| date | window used |".into());
        l.push("|---|---:|".into());
        for (date, pct) in &d.limit_days {
            l.push(format!("| {date} | {pct:.0}% |"));
        }
        l.push(String::new());
    }

    if !d.top_projects.is_empty() {
        l.push("## Top projects by volume".into());
        l.push(String::new());
        l.push("| project | total, M tok |".into());
        l.push("|---|---:|".into());
        for (p, t) in &d.top_projects {
            l.push(format!("| {} | {:.0} |", p, *t as f64 / 1e6));
        }
        l.push(String::new());
    }

    if let Some(first) = d.deltas.first() {
        l.push(format!("## Delta since baseline ({})", first.since));
        l.push(String::new());
        for dd in &d.deltas {
            if d.deltas.len() > 1 {
                l.push(format!("### {}", dd.source));
                l.push(String::new());
            }
            l.push("| metric | baseline | now |".into());
            l.push("|---|---:|---:|".into());
            l.push(format!(
                "| requests analyzed | {} | {} |",
                dd.b_requests, dd.now.requests
            ));
            l.push(format!(
                "| avg context per turn | {}k | {}k |",
                (dd.b_prompt / 1e3).round() as u64,
                (dd.now.avg_prompt_per_turn / 1e3).round() as u64
            ));
            l.push(format!(
                "| long-session context growth | {:.1}x | {:.1}x |",
                dd.b_growth, dd.now.context_growth
            ));
            l.push(String::new());
            let v = verdict_line(dd);
            if !v.is_empty() {
                l.push(format!("**Verdict:** {v}"));
                l.push(String::new());
            }
        }
    }

    if !d.project_deltas.is_empty() {
        l.push("## Per-project delta since baseline".into());
        l.push(String::new());
        l.push("| project | source | avg context/turn | session growth | verdict |".into());
        l.push("|---|---|---:|---:|---|".into());
        for pd in &d.project_deltas {
            let dd = &pd.delta;
            let v = match delta_change(dd) {
                Some(c) if c <= -1.0 => format!("↓ {:.0}%", -c),
                Some(c) if c >= 1.0 => format!("↑ {c:.0}%"),
                Some(_) => "→ flat".into(),
                None => "-".into(),
            };
            l.push(format!(
                "| {} | {} | {}k → {}k | {:.1}x → {:.1}x | {v} |",
                pd.project,
                dd.source,
                (dd.b_prompt / 1e3).round() as u64,
                (dd.now.avg_prompt_per_turn / 1e3).round() as u64,
                dd.b_growth,
                dd.now.context_growth,
            ));
        }
        l.push(String::new());
    }

    if !d.history.is_empty() {
        l.push("## Trend".into());
        l.push(String::new());
        l.push("One row per `codeunlimited report` run - watch the numbers fall.".into());
        l.push(String::new());
        l.push(
            "| date | requests | avg context/turn | session growth | reclaimable, M tok |".into(),
        );
        l.push("|---|---:|---:|---:|---:|".into());
        let mut prev: Option<u64> = None;
        for h in &d.history {
            let avg = h["avg_prompt_per_turn"].as_u64().unwrap_or(0);
            let arrow = match prev {
                Some(p) if avg < p => " ↓",
                Some(p) if avg > p => " ↑",
                Some(_) => " →",
                None => "",
            };
            prev = Some(avg);
            l.push(format!(
                "| {} | {} | {}k{arrow} | {:.1}x | {:.0} |",
                h["date"].as_str().unwrap_or("?"),
                h["requests"].as_u64().unwrap_or(0),
                avg / 1000,
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

/// FNV-1a hash of a project name - lets reports be shared publicly.
fn anon(name: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("proj-{:08x}", (h >> 32) as u32)
}

fn anonymize(data: &mut ReportData) {
    if data.name != "all projects" {
        data.name = anon(&data.name);
    }
    for (p, _) in &mut data.top_projects {
        *p = match p.split_once(':') {
            Some((src, rest)) => format!("{src}:{}", anon(rest)),
            None => anon(p),
        };
    }
    for pd in &mut data.project_deltas {
        pd.project = anon(&pd.project);
    }
}

fn write_badge(md_path: &Path, pct: f64) -> std::io::Result<()> {
    let value = format!("~{pct:.0}% of weekly limit");
    let vw = 10 + value.len() * 7;
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="20" role="img" aria-label="codeunlimited: {value}">
<rect width="102" height="20" rx="3" fill="#555"/>
<rect x="102" width="{vw}" height="20" rx="3" fill="#2ea44f"/>
<rect x="99" width="6" height="20" fill="#2ea44f"/>
<g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">
<text x="51" y="14">codeunlimited</text>
<text x="{vx}" y="14">{value}</text>
</g>
</svg>
"##,
        w = 102 + vw,
        vx = 102 + vw / 2,
    );
    let path = md_path.with_file_name("CODEUNLIMITED_BADGE.svg");
    crate::safeio::atomic_write(&path, svg.as_bytes())?;
    let p = path.to_string_lossy();
    println!("          Badge: {}", strip_verbatim(&p));
    Ok(())
}

fn deltas_for(root: &Path, reqs: &[Request]) -> Vec<DeltaInfo> {
    let Some((created, baselines)) = deltacmd::load_baseline(root) else {
        return vec![];
    };
    let since = chrono::DateTime::from_timestamp(created, 0)
        .map(|d| d.date_naive().to_string())
        .unwrap_or_else(|| "?".into());
    baselines
        .into_iter()
        .filter_map(|b| {
            let now: Vec<Request> = reqs
                .iter()
                .filter(|r| r.source == b.source && r.ts.is_some_and(|t| t >= created))
                .cloned()
                .collect();
            (!now.is_empty()).then(|| DeltaInfo {
                source: b.source,
                since: since.clone(),
                b_requests: b.requests,
                b_prompt: b.prompt,
                b_growth: b.growth,
                now: metrics::compute(&now),
            })
        })
        .collect()
}

fn append_history(
    path: &Path,
    reqs: &[Request],
    reclaim: u64,
    now: &chrono::DateTime<chrono::Utc>,
) -> std::io::Result<Vec<Value>> {
    let m = metrics::compute(reqs);
    let total: u64 = reqs.iter().map(|r| r.total()).sum();
    let snap = serde_json::json!({
        "ts": now.timestamp(),
        "date": now.date_naive().to_string(),
        "requests": m.requests,
        "avg_prompt_per_turn": m.avg_prompt_per_turn as u64,
        "context_growth": (m.context_growth * 100.0).round() / 100.0,
        "total_tokens": total,
        "reclaimable_tokens": reclaim,
    });
    let lock_path = path.with_file_name(format!(
        "{}.lock",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("history")
    ));
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    lock.lock_exclusive()?;
    let mut raw = std::fs::read_to_string(path).unwrap_or_default();
    if !raw.is_empty() && !raw.ends_with('\n') {
        raw.push('\n');
    }
    writeln!(&mut raw, "{snap}").expect("writing to String cannot fail");
    crate::safeio::atomic_write(path, raw.as_bytes())?;
    Ok(raw
        .lines()
        .filter_map(|ln| serde_json::from_str(ln).ok())
        .collect())
}

fn write_pair(md_path: &Path, data: &ReportData, badge: bool) -> i32 {
    if let Err(e) = crate::safeio::atomic_write(md_path, build_markdown(data).as_bytes()) {
        eprintln!("Cannot write {}: {e}", md_path.display());
        return 1;
    }
    let html_path = md_path.with_extension("html");
    if let Err(e) = crate::safeio::atomic_write(&html_path, html::build_html(data).as_bytes()) {
        eprintln!("Cannot write {}: {e}", html_path.display());
        return 1;
    }
    let md = md_path.to_string_lossy();
    let ht = html_path.to_string_lossy();
    println!(" Report written: {}", strip_verbatim(&md));
    println!(
        "           HTML: {}  (open in a browser)",
        strip_verbatim(&ht)
    );
    if badge {
        if let Err(e) = write_badge(md_path, data.reclaim_pct) {
            eprintln!("Cannot write badge: {e}");
            return 1;
        }
    }
    0
}

pub fn run(path: &Path, out: Option<&Path>, badge: bool, anon_flag: bool) -> i32 {
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
    if let Err(e) = registry::register(&root) {
        eprintln!("Cannot register {}: {e}", root.display());
        return 1;
    }
    let findings = detectors::run_all(&reqs);
    let deltas = deltas_for(&root, &reqs);
    let reclaim: u64 = findings.iter().map(|f| f.impact_tokens).sum();
    let now = chrono::Utc::now();
    let history = match append_history(&root.join(HISTORY_FILE), &reqs, reclaim, &now) {
        Ok(history) => history,
        Err(e) => {
            eprintln!("Cannot write {}: {e}", root.join(HISTORY_FILE).display());
            return 1;
        }
    };
    println!(
        " Snapshot #{} recorded in {} - the trend grows with every run.",
        history.len(),
        HISTORY_FILE
    );

    let mut data = collect(
        &name,
        &reqs,
        &findings,
        deltas,
        vec![],
        vec![],
        history,
        &now.date_naive().to_string(),
    );
    if anon_flag {
        anonymize(&mut data);
    }
    let md_path = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join(REPORT_FILE));
    write_pair(&md_path, &data, badge)
}

pub fn run_all(out: Option<&Path>, badge: bool, anon_flag: bool) -> i32 {
    let mut reqs = parsers::iter_claude(None);
    let (codex, series) = parsers::iter_codex_full(None);
    reqs.extend(codex);
    reqs.retain(|r| !crate::config::ignored(&r.project));
    if reqs.is_empty() {
        eprintln!("No local Claude Code / Codex logs found.");
        return 1;
    }
    let findings = detectors::run_all(&reqs);

    // Per-registered-project deltas.
    let mut project_deltas: Vec<ProjectDelta> = Vec::new();
    for p in registry::projects() {
        let mut pr = parsers::iter_claude(Some(&p));
        pr.extend(parsers::iter_codex(Some(&p)));
        let name = p
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        for delta in deltas_for(&p, &pr) {
            project_deltas.push(ProjectDelta {
                project: name.clone(),
                delta,
            });
        }
    }

    let mut per_proj: BTreeMap<String, u64> = BTreeMap::new();
    for r in &reqs {
        *per_proj
            .entry(format!("{}:{}", r.source, r.project))
            .or_default() += r.total();
    }
    let mut top: Vec<(String, u64)> = per_proj.into_iter().collect();
    top.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
    top.truncate(8);

    let reclaim: u64 = findings.iter().map(|f| f.impact_tokens).sum();
    let now = chrono::Utc::now();
    if let Err(e) = std::fs::create_dir_all(registry::home_dir()) {
        eprintln!("Cannot create {}: {e}", registry::home_dir().display());
        return 1;
    }
    let history_path = registry::home_dir().join("history.jsonl");
    let history = match append_history(&history_path, &reqs, reclaim, &now) {
        Ok(history) => history,
        Err(e) => {
            eprintln!("Cannot write {}: {e}", history_path.display());
            return 1;
        }
    };
    println!(
        " Snapshot #{} recorded in {} - the trend grows with every run.",
        history.len(),
        registry::home_dir().join("history.jsonl").display()
    );

    let mut data = collect(
        "all projects",
        &reqs,
        &findings,
        vec![],
        project_deltas,
        top,
        history,
        &now.date_naive().to_string(),
    );
    data.forecast = forecast::forecast(&reqs, &series);
    data.limit_days = forecast::daily_peaks(&series)
        .into_iter()
        .rev()
        .take(21)
        .rev()
        .map(|(day, pct)| {
            let date = chrono::DateTime::from_timestamp(day, 0)
                .map(|d| d.date_naive().to_string())
                .unwrap_or_else(|| "?".into());
            (date, pct)
        })
        .collect();
    if anon_flag {
        anonymize(&mut data);
    }
    let md_path = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(SUMMARY_FILE));
    write_pair(&md_path, &data, badge)
}
