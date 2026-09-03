//! Self-contained HTML report: one file, inline CSS, system fonts, light and
//! dark themes, zero external requests - safe to open, mail, or screenshot.

use crate::reportcmd::{delta_change, verdict_line, ReportData};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

const CSS: &str = r#"
:root{
  --bg:#f6f7f5;--panel:#ffffff;--ink:#20241f;--muted:#6b7166;--line:#e2e5df;
  --accent:#1a7f37;--accent-soft:#d2ecd8;--bad:#c93c37;--warn:#b58419;
}
@media (prefers-color-scheme:dark){:root{
  --bg:#0e120f;--panel:#161b17;--ink:#e6eae3;--muted:#8b948a;--line:#252c26;
  --accent:#3fd158;--accent-soft:#173c20;--bad:#f26d66;--warn:#e0b34c;
}}
*{box-sizing:border-box;margin:0}
body{background:var(--bg);color:var(--ink);
  font:15px/1.55 ui-sans-serif,system-ui,"Segoe UI",Roboto,sans-serif;
  padding:40px 20px}
main{max-width:880px;margin:0 auto}
.eyebrow{font-size:12px;letter-spacing:.14em;text-transform:uppercase;color:var(--accent);font-weight:600}
h1{font-size:32px;letter-spacing:-.02em;margin:6px 0 2px}
.sub{color:var(--muted);font-size:13px}
.hero{display:flex;flex-wrap:wrap;gap:16px;align-items:flex-end;justify-content:space-between;
  border-bottom:1px solid var(--line);padding-bottom:24px;margin-bottom:28px}
.hero-num{text-align:right}
.hero-num b{font-size:44px;letter-spacing:-.03em;color:var(--accent);
  font-variant-numeric:tabular-nums;display:block;line-height:1}
.hero-num span{color:var(--muted);font-size:13px}
h2{font-size:15px;letter-spacing:.08em;text-transform:uppercase;color:var(--muted);
  font-weight:600;margin:32px 0 12px}
.cards{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:12px}
.card{background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:14px 16px}
.card .k{color:var(--muted);font-size:12px}
.card .v{font-size:22px;font-variant-numeric:tabular-nums;letter-spacing:-.01em}
.card .v small{font-size:13px;color:var(--muted)}
.finding{background:var(--panel);border:1px solid var(--line);border-radius:10px;
  padding:16px 18px;margin-bottom:12px}
.finding h3{font-size:16px;margin-bottom:8px}
.meter{height:6px;border-radius:3px;background:var(--line);overflow:hidden;margin:8px 0 10px}
.meter i{display:block;height:100%;background:var(--accent)}
.finding p{margin-bottom:8px}
.reclaim{font-weight:600}
.fix{color:var(--muted);font-size:13.5px}
.fix b{color:var(--ink)}
.total{background:var(--accent-soft);border:1px solid var(--accent);border-radius:10px;
  padding:14px 18px;font-size:16px;margin-top:16px}
table{width:100%;border-collapse:collapse;background:var(--panel);
  border:1px solid var(--line);border-radius:10px;overflow:hidden;font-variant-numeric:tabular-nums}
.tablewrap{overflow-x:auto}
th,td{text-align:left;padding:9px 14px;border-bottom:1px solid var(--line);font-size:14px}
th{color:var(--muted);font-size:12px;letter-spacing:.06em;text-transform:uppercase;font-weight:600}
tr:last-child td{border-bottom:none}
td.n,th.n{text-align:right}
.pill{display:inline-block;padding:2px 10px;border-radius:999px;font-size:12.5px;font-weight:600}
.pill.down{background:var(--accent-soft);color:var(--accent)}
.pill.up{background:color-mix(in srgb,var(--bad) 15%,transparent);color:var(--bad)}
.pill.flat{background:var(--line);color:var(--muted)}
.verdict{margin-top:10px;font-weight:600}
.trend-row{display:grid;grid-template-columns:90px 1fr 120px;gap:12px;align-items:center;
  padding:6px 0;font-variant-numeric:tabular-nums}
.trend-row .date{color:var(--muted);font-size:13px}
.trend-row .val{text-align:right;font-size:13.5px}
.tbar{height:14px;border-radius:4px;background:var(--accent);opacity:.85;min-width:2px}
footer{margin-top:40px;padding-top:16px;border-top:1px solid var(--line);
  color:var(--muted);font-size:13px}
footer a{color:var(--accent);text-decoration:none}
"#;

pub fn build_html(d: &ReportData) -> String {
    let mut b = String::with_capacity(16_384);
    b.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    b.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n");
    b.push_str(&format!(
        "<title>codeunlimited - {}</title>\n<style>{CSS}</style>\n</head>\n<body>\n<main>\n",
        esc(&d.name)
    ));

    // Hero
    let period = d
        .period
        .as_ref()
        .map(|(d0, d1, days)| format!("{d0} … {d1} · {days:.0} days · "))
        .unwrap_or_default();
    b.push_str(&format!(
        "<div class=\"hero\"><div><div class=\"eyebrow\">codeunlimited</div>\
         <h1>{}</h1><div class=\"sub\">{period}generated {} · all data local, \
         token counts only</div></div>",
        esc(&d.name),
        esc(&d.generated)
    ));
    if d.reclaim > 0 {
        b.push_str(&format!(
            "<div class=\"hero-num\"><b>{:.0}%</b><span>of weekly volume reclaimable<br>\
             ~{:.0}M tokens of extra work</span></div>",
            d.reclaim_pct,
            d.reclaim as f64 / 1e6
        ));
    }
    b.push_str("</div>\n");

    // Usage cards
    b.push_str("<h2>Usage</h2>\n<div class=\"cards\">\n");
    for s in &d.sources {
        b.push_str(&format!(
            "<div class=\"card\"><div class=\"k\">{}</div>\
             <div class=\"v\">{} <small>requests</small></div>\
             <div class=\"k\">{:.0}M context · {:.1}M code/answers</div></div>\n",
            esc(&s.source),
            s.requests,
            s.prompt as f64 / 1e6,
            s.out as f64 / 1e6
        ));
    }
    b.push_str(&format!(
        "<div class=\"card\"><div class=\"k\">weekly volume (limit proxy)</div>\
         <div class=\"v\">~{:.0}M <small>tokens</small></div></div>\n</div>\n",
        d.weekly / 1e6
    ));

    // Findings
    b.push_str("<h2>Where the limit leaks</h2>\n");
    for (i, f) in d.findings.iter().enumerate() {
        b.push_str(&format!(
            "<div class=\"finding\"><h3>{}. {}</h3>\
             <div class=\"meter\"><i style=\"width:{:.0}%\"></i></div>\
             <p>{}</p>\
             <p class=\"reclaim\">Reclaim: ~{:.0}M tokens · {:.0}% of weekly volume · \
             ~{:.0} extra agent replies</p>\
             <p class=\"fix\"><b>Fix:</b> {}</p></div>\n",
            i + 1,
            esc(&f.title),
            f.pct.min(100.0),
            esc(&f.detail),
            f.tokens as f64 / 1e6,
            f.pct,
            f.answers,
            esc(&f.fix)
        ));
    }
    if d.findings.is_empty() {
        b.push_str("<p>No significant leaks detected in this window.</p>\n");
    } else {
        b.push_str(&format!(
            "<div class=\"total\">Total reclaimable: <b>~{:.0}M tokens ≈ {:.0}% of \
             weekly volume</b> - that much more work fits into the same limit.</div>\n",
            d.reclaim as f64 / 1e6,
            d.reclaim_pct
        ));
    }

    // Top projects (summary mode)
    if !d.top_projects.is_empty() {
        b.push_str("<h2>Top projects by volume</h2>\n<div class=\"tablewrap\"><table>\n<tr><th>project</th><th class=\"n\">total, M tok</th></tr>\n");
        for (p, t) in &d.top_projects {
            b.push_str(&format!(
                "<tr><td>{}</td><td class=\"n\">{:.0}</td></tr>\n",
                esc(p),
                *t as f64 / 1e6
            ));
        }
        b.push_str("</table></div>\n");
    }

    // Delta (scoped mode)
    if let Some(first) = d.deltas.first() {
        b.push_str(&format!(
            "<h2>Delta since baseline ({})</h2>\n",
            esc(&first.since)
        ));
        for dd in &d.deltas {
            if d.deltas.len() > 1 {
                b.push_str(&format!("<p class=\"sub\">{}</p>\n", esc(&dd.source)));
            }
            b.push_str(
                "<div class=\"tablewrap\"><table>\n<tr><th>metric</th><th class=\"n\">baseline</th><th class=\"n\">now</th></tr>\n",
            );
            b.push_str(&format!(
                "<tr><td>requests analyzed</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>\n",
                dd.b_requests, dd.now.requests
            ));
            b.push_str(&format!(
                "<tr><td>avg context per turn</td><td class=\"n\">{}k</td><td class=\"n\">{}k</td></tr>\n",
                (dd.b_prompt / 1e3).round() as u64,
                (dd.now.avg_prompt_per_turn / 1e3).round() as u64
            ));
            b.push_str(&format!(
                "<tr><td>long-session context growth</td><td class=\"n\">{:.1}x</td><td class=\"n\">{:.1}x</td></tr>\n</table></div>\n",
                dd.b_growth, dd.now.context_growth
            ));
            let v = verdict_line(dd);
            if !v.is_empty() {
                b.push_str(&format!("<p class=\"verdict\">{}</p>\n", esc(&v)));
            }
        }
    }

    // Per-project deltas (summary mode)
    if !d.project_deltas.is_empty() {
        b.push_str("<h2>Per-project delta since baseline</h2>\n<div class=\"tablewrap\"><table>\n<tr><th>project</th><th>source</th><th class=\"n\">avg context/turn</th><th class=\"n\">session growth</th><th>verdict</th></tr>\n");
        for pd in &d.project_deltas {
            let dd = &pd.delta;
            let pill = match delta_change(dd) {
                Some(c) if c <= -1.0 => {
                    format!("<span class=\"pill down\">↓ {:.0}%</span>", -c)
                }
                Some(c) if c >= 1.0 => format!("<span class=\"pill up\">↑ {c:.0}%</span>"),
                Some(_) => "<span class=\"pill flat\">→ flat</span>".into(),
                None => "-".into(),
            };
            b.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td class=\"n\">{}k → {}k</td>\
                 <td class=\"n\">{:.1}x → {:.1}x</td><td>{pill}</td></tr>\n",
                esc(&pd.project),
                esc(&dd.source),
                (dd.b_prompt / 1e3).round() as u64,
                (dd.now.avg_prompt_per_turn / 1e3).round() as u64,
                dd.b_growth,
                dd.now.context_growth
            ));
        }
        b.push_str("</table></div>\n");
    }

    // Trend
    if !d.history.is_empty() {
        b.push_str("<h2>Trend - avg context per turn</h2>\n");
        let max = d
            .history
            .iter()
            .map(|h| h["avg_prompt_per_turn"].as_u64().unwrap_or(0))
            .max()
            .unwrap_or(1)
            .max(1);
        for h in &d.history {
            let avg = h["avg_prompt_per_turn"].as_u64().unwrap_or(0);
            b.push_str(&format!(
                "<div class=\"trend-row\"><span class=\"date\">{}</span>\
                 <div class=\"tbar\" style=\"width:{:.1}%\"></div>\
                 <span class=\"val\">{}k · {:.0}M reclaimable</span></div>\n",
                esc(h["date"].as_str().unwrap_or("?")),
                100.0 * avg as f64 / max as f64,
                avg / 1000,
                h["reclaimable_tokens"].as_u64().unwrap_or(0) as f64 / 1e6
            ));
        }
    }

    b.push_str(
        "<footer><a href=\"https://github.com/jimbokl/codeunlimited\">codeunlimited</a> \
         - more code out of the subscription limits you already pay for. \
         Offline; prompts are never read.</footer>\n</main>\n</body>\n</html>\n",
    );
    b
}
