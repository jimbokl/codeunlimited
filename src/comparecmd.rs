//! `codeunlimited compare`: this period vs the previous one - is the limit
//! being spent better or worse? Anchored at your last activity, not at
//! wall-clock now, so quiet days don't skew the picture.

use crate::types::Request;
use crate::{config, detectors, metrics, parsers};

fn window(reqs: &[Request], from: i64, to: i64) -> Vec<Request> {
    reqs.iter()
        .filter(|r| r.ts.is_some_and(|t| t > from && t <= to))
        .cloned()
        .collect()
}

struct Side {
    volume: u64,
    requests: u64,
    avg_prompt: f64,
    growth: f64,
    reclaim: u64,
}

fn side(reqs: &[Request]) -> Side {
    let m = metrics::compute(reqs);
    Side {
        volume: reqs.iter().map(|r| r.total()).sum(),
        requests: m.requests,
        avg_prompt: m.avg_prompt_per_turn,
        growth: m.context_growth,
        reclaim: detectors::run_all(reqs)
            .iter()
            .map(|f| f.impact_tokens)
            .sum(),
    }
}

fn row(name: &str, a: f64, b: f64, unit: &str, better_down: bool) {
    let arrow = if (a - b).abs() / b.max(1.0) < 0.01 {
        "→"
    } else if (a < b) == better_down {
        "↓ better"
    } else {
        "↑ worse"
    };
    println!(" {name:28} {b:>12.0}{unit} {a:>12.0}{unit}   {arrow}");
}

pub fn run(days: u64) -> i32 {
    let mut reqs = parsers::iter_claude(None);
    reqs.extend(parsers::iter_codex(None));
    reqs.retain(|r| !config::ignored(&r.project));
    let Some(anchor) = reqs.iter().filter_map(|r| r.ts).max() else {
        eprintln!("No local logs found.");
        return 1;
    };
    let d = days as i64 * 86_400;
    let cur = window(&reqs, anchor - d, anchor);
    let prev = window(&reqs, anchor - 2 * d, anchor - d);
    if cur.is_empty() || prev.is_empty() {
        eprintln!("Not enough history for two {days}-day windows - try a smaller --days.");
        return 1;
    }
    let (a, b) = (side(&cur), side(&prev));
    println!("COMPARE - last {days} days vs the {days} before (anchored at your last activity)\n");
    println!(" {:28} {:>13} {:>13}", "", "previous", "current");
    row(
        "volume, M tokens",
        a.volume as f64 / 1e6,
        b.volume as f64 / 1e6,
        "",
        true,
    );
    row("requests", a.requests as f64, b.requests as f64, "", false);
    row(
        "avg context per turn, k",
        a.avg_prompt / 1e3,
        b.avg_prompt / 1e3,
        "",
        true,
    );
    println!(
        " {:28} {:>12.1}x {:>12.1}x",
        "long-session growth", b.growth, a.growth
    );
    row(
        "reclaimable, M tokens",
        a.reclaim as f64 / 1e6,
        b.reclaim as f64 / 1e6,
        "",
        true,
    );
    let eff_a = a.reclaim as f64 / a.volume.max(1) as f64;
    let eff_b = b.reclaim as f64 / b.volume.max(1) as f64;
    if eff_a < eff_b - 0.01 {
        println!(
            "\n VERDICT: leak share fell from {:.0}% to {:.0}% of volume - the rules are working.",
            eff_b * 100.0,
            eff_a * 100.0
        );
    } else if eff_a > eff_b + 0.01 {
        println!(
            "\n VERDICT: leak share grew from {:.0}% to {:.0}% of volume - run `codeunlimited audit` for the culprits."
            , eff_b * 100.0, eff_a * 100.0
        );
    } else {
        println!("\n VERDICT: flat - keep the rules on and re-check next week.");
    }
    0
}
