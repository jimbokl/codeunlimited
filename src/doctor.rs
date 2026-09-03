//! `codeunlimited doctor`: early warning for log-format drift. Claude Code
//! and Codex update often; if their log schemas change, detectors silently
//! see less data. This command counts what the parsers recognized vs what
//! they had to skip, so drift shows up as a number, not a mystery.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use rayon::prelude::*;
use serde_json::Value;
use walkdir::WalkDir;

use crate::parsers;

#[derive(Default)]
struct Tally {
    files: u64,
    candidates: u64,
    parsed: u64,
    json_errors: u64,
    missing_usage: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceState {
    Unavailable,
    Healthy,
    Unhealthy,
}

fn add(a: Tally, b: Tally) -> Tally {
    Tally {
        files: a.files + b.files,
        candidates: a.candidates + b.candidates,
        parsed: a.parsed + b.parsed,
        json_errors: a.json_errors + b.json_errors,
        missing_usage: a.missing_usage + b.missing_usage,
    }
}

fn scan(root: &Path, is_claude: bool) -> Tally {
    let files: Vec<_> = WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .map(|e| e.into_path())
        .collect();
    files
        .par_iter()
        .map(|path| {
            let mut t = Tally {
                files: 1,
                ..Default::default()
            };
            let Ok(f) = File::open(path) else { return t };
            for line in BufReader::new(f).lines() {
                let Ok(line) = line else { continue };
                let candidate = if is_claude {
                    line.contains("\"usage\"") && line.contains("\"assistant\"")
                } else {
                    line.contains("\"token_count\"")
                };
                if !candidate {
                    continue;
                }
                t.candidates += 1;
                let Ok(d) = serde_json::from_str::<Value>(&line) else {
                    t.json_errors += 1;
                    continue;
                };
                let usage_ok = if is_claude {
                    d.get("type").and_then(Value::as_str) == Some("assistant")
                        && d["message"]["usage"].is_object()
                } else {
                    d["payload"].get("type").and_then(Value::as_str) == Some("token_count")
                        && (d["payload"]["info"]["last_token_usage"].is_object()
                            || d["payload"]["rate_limits"].is_object())
                };
                if usage_ok {
                    t.parsed += 1;
                } else {
                    t.missing_usage += 1;
                }
            }
            t
        })
        .reduce(Tally::default, add)
}

fn print_source(name: &str, root: &Path, t: &Tally) -> SourceState {
    if t.files == 0 {
        println!(" {name:6}: no logs found at {}", root.display());
        return SourceState::Unavailable;
    }
    if t.candidates == 0 {
        println!(
            " {name:6}: {} files, no usage candidates recognized",
            t.files
        );
        return SourceState::Unavailable;
    }
    let bad = t.json_errors + t.missing_usage;
    let rate = if t.candidates > 0 {
        100.0 * bad as f64 / t.candidates as f64
    } else {
        0.0
    };
    println!(
        " {name:6}: {} files, {} usage lines - {} recognized, {} JSON errors, \
         {} without usage ({rate:.1}% unrecognized)",
        t.files, t.candidates, t.parsed, t.json_errors, t.missing_usage
    );
    let healthy = rate < 5.0;
    if !healthy {
        println!(
            "         WARNING: more than 5% of {name} log lines are unrecognized - \
             the log format may have drifted; please open an issue with your \
             {name} version."
        );
    }
    if healthy {
        SourceState::Healthy
    } else {
        SourceState::Unhealthy
    }
}

pub fn run() -> i32 {
    println!("codeunlimited doctor - log format health check\n");
    let claude_root = parsers::claude_root();
    let codex_root = parsers::codex_root();
    let (ct, xt) = rayon::join(|| scan(&claude_root, true), || scan(&codex_root, false));
    let states = [
        print_source("claude", &claude_root, &ct),
        print_source("codex", &codex_root, &xt),
    ];
    println!();
    if states.contains(&SourceState::Unhealthy) {
        1
    } else if states.contains(&SourceState::Healthy) {
        println!(" All good: the parsers understand your available logs.");
        0
    } else {
        eprintln!("No local logs with recognizable usage data were available to validate.");
        1
    }
}
