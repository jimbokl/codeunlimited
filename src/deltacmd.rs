//! `codeunlimited delta`: verified before/after for a project - proves how much
//! work the efficiency rules reclaimed since `init` captured the baseline.

use std::path::Path;

use serde_json::Value;

use crate::{metrics, parsers};

pub const BASELINE_FILE: &str = ".codeunlimited.baseline.json";

pub fn run(path: &Path) -> i32 {
    let root = match path.canonicalize() {
        Ok(p) if p.is_dir() => p,
        _ => {
            eprintln!("No such directory: {}", path.display());
            return 1;
        }
    };
    let bl_path = root.join(BASELINE_FILE);
    let Ok(raw) = std::fs::read_to_string(&bl_path) else {
        eprintln!(
            "No baseline found ({}). Run `codeunlimited init` first - it captures \
             the baseline this command compares against.",
            bl_path.display()
        );
        return 1;
    };
    let Ok(bl) = serde_json::from_str::<Value>(&raw) else {
        eprintln!("Baseline file is corrupted: {}", bl_path.display());
        return 1;
    };
    let created = bl["created_unix"].as_i64().unwrap_or(0);
    let b = &bl["metrics"];
    let (b_req, b_prompt, b_growth) = (
        b["requests"].as_u64().unwrap_or(0),
        b["avg_prompt_per_turn"].as_u64().unwrap_or(0) as f64,
        b["context_growth"].as_f64().unwrap_or(1.0),
    );

    let mut reqs = parsers::iter_claude(Some(&root));
    reqs.retain(|r| r.ts.is_some_and(|t| t >= created));
    if reqs.is_empty() {
        println!(
            "No activity in this project since the baseline was captured - \
             work a while, then re-run `codeunlimited delta`."
        );
        return 0;
    }
    let m = metrics::compute(&reqs);

    let when = chrono::DateTime::from_timestamp(created, 0)
        .map(|d| d.date_naive().to_string())
        .unwrap_or_else(|| "?".into());
    println!("DELTA since baseline ({when}) - source: claude, this project only\n");
    println!(" {:26} {:>14} {:>14}", "", "baseline", "now");
    println!(
        " {:26} {:>14} {:>14}",
        "requests analyzed", b_req, m.requests
    );
    println!(
        " {:26} {:>13}k {:>13}k",
        "avg context per turn",
        (b_prompt / 1e3).round() as u64,
        (m.avg_prompt_per_turn / 1e3).round() as u64
    );
    println!(
        " {:26} {:>13.1}x {:>13.1}x",
        "long-session context growth", b_growth, m.context_growth
    );
    if b_prompt > 0.0 {
        let change = 100.0 * (m.avg_prompt_per_turn - b_prompt) / b_prompt;
        if change <= -1.0 {
            println!(
                "\n VERDICT: context per turn is down {:.0}% - about {:.0}% more work \
                 now fits into the same limit.",
                -change,
                100.0 * (b_prompt / m.avg_prompt_per_turn.max(1.0) - 1.0)
            );
        } else if change >= 1.0 {
            println!(
                "\n VERDICT: context per turn is up {change:.0}% - the leaks are growing; \
                 run `codeunlimited audit --project .` to see where."
            );
        } else {
            println!("\n VERDICT: flat so far - keep the rules on and re-check later.");
        }
    }
    0
}
