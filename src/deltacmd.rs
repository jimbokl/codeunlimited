//! `codeunlimited delta`: observational before/after tracking for a project.
//! work the efficiency rules reclaimed since `init` captured the baseline.

use std::path::Path;

use serde_json::Value;

use crate::types::Request;
use crate::{metrics, parsers};

pub const BASELINE_FILE: &str = ".codeunlimited.baseline.json";
pub const MIN_VERDICT_REQUESTS: u64 = 100;

/// One source's slice of the baseline captured by `init`.
pub struct BaselineSrc {
    pub source: String,
    pub requests: u64,
    pub prompt: f64,
    pub growth: f64,
}

/// Parses both baseline formats: v2 `{"sources": {...}}` and the original
/// single-source `{"source": "claude", "metrics": {...}}`.
pub fn parse_baseline(raw: &str) -> Option<(i64, Vec<BaselineSrc>)> {
    let bl: Value = serde_json::from_str(raw).ok()?;
    let created = bl["created_unix"].as_i64()?;
    let of = |src: &str, m: &Value| BaselineSrc {
        source: src.to_string(),
        requests: m["requests"].as_u64().unwrap_or(0),
        prompt: m["avg_prompt_per_turn"].as_u64().unwrap_or(0) as f64,
        growth: m["context_growth"].as_f64().unwrap_or(1.0),
    };
    let mut out = Vec::new();
    if let Some(srcs) = bl["sources"].as_object() {
        for (k, m) in srcs {
            out.push(of(k, m));
        }
    } else if bl["metrics"].is_object() {
        out.push(of(
            bl["source"].as_str().unwrap_or("claude"),
            &bl["metrics"],
        ));
    }
    (!out.is_empty()).then_some((created, out))
}

pub fn load_baseline(root: &Path) -> Option<(i64, Vec<BaselineSrc>)> {
    let raw = std::fs::read_to_string(root.join(BASELINE_FILE)).ok()?;
    parse_baseline(&raw)
}

fn since(root: &Path, source: &str, created: i64) -> Vec<Request> {
    let mut reqs = match source {
        "codex" => parsers::iter_codex(Some(root)),
        _ => parsers::iter_claude(Some(root)),
    };
    reqs.retain(|r| r.ts.is_some_and(|t| t >= created));
    reqs
}

fn print_source(b: &BaselineSrc, now: &metrics::Metrics) {
    println!(" [{}]", b.source);
    println!(" {:26} {:>14} {:>14}", "", "baseline", "now");
    println!(
        " {:26} {:>14} {:>14}",
        "requests analyzed", b.requests, now.requests
    );
    println!(
        " {:26} {:>13}k {:>13}k",
        "avg context per turn",
        (b.prompt / 1e3).round() as u64,
        (now.avg_prompt_per_turn / 1e3).round() as u64
    );
    println!(
        " {:26} {:>13.1}x {:>13.1}x",
        "long-session context growth", b.growth, now.context_growth
    );
    if now.requests < MIN_VERDICT_REQUESTS {
        println!(
            " insufficient sample ({}/{} requests) - exact metrics shown without a directional verdict.",
            now.requests, MIN_VERDICT_REQUESTS
        );
        println!();
        return;
    }
    if b.prompt > 0.0 {
        let change = 100.0 * (now.avg_prompt_per_turn - b.prompt) / b.prompt;
        if change <= -1.0 {
            println!(
                " METRIC TREND: context per turn is down {:.0}%; modeled capacity proxy \
                 +{:.0}%. Measure comparable completed tasks before attributing an outcome.",
                -change,
                100.0 * (b.prompt / now.avg_prompt_per_turn.max(1.0) - 1.0)
            );
        } else if change >= 1.0 {
            println!(
                " METRIC TREND: context per turn is up {change:.0}%; \
                 run `codeunlimited audit --project .` to see where."
            );
        } else {
            println!(" METRIC TREND: context per turn is flat so far; re-check later.");
        }
    }
    println!();
}

pub fn run(path: &Path) -> i32 {
    let root = match path.canonicalize() {
        Ok(p) if p.is_dir() => p,
        _ => {
            eprintln!("No such directory: {}", path.display());
            return 1;
        }
    };
    let cfg = crate::config::Config::load_for(Some(&root));
    let Some((created, baselines)) = load_baseline(&root) else {
        eprintln!(
            "No baseline found ({}). Run `codeunlimited init` first - it captures \
             the baseline this command compares against.",
            root.join(BASELINE_FILE).display()
        );
        return 1;
    };

    let when = chrono::DateTime::from_timestamp(created, 0)
        .map(|d| d.date_naive().to_string())
        .unwrap_or_else(|| "?".into());
    println!("DELTA since baseline ({when}) - this project only\n");
    let mut any = false;
    for b in &baselines {
        let reqs = since(&root, &b.source, created);
        if !reqs.is_empty() {
            any = true;
        }
        print_source(b, &metrics::compute(&reqs, cfg.long_session_turns));
    }
    if !any {
        println!(
            "No activity in this project since the baseline was captured - \
             work a while, then re-run `codeunlimited delta`."
        );
    }
    0
}
