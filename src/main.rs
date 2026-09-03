use codeunlimited::{deltacmd, detectors, initcmd, parsers, report, reportcmd};

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "codeunlimited",
    version,
    about = "More code out of the limits you already pay for: offline audit of \
             Claude Code & Codex token usage + project efficiency setup."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, ValueEnum)]
enum Source {
    All,
    Claude,
    Codex,
}

#[derive(Subcommand)]
enum Cmd {
    /// Find where your limit leaks (offline, local logs only)
    Audit {
        #[arg(long, value_enum, default_value = "all")]
        source: Source,
        /// Scope the report to one project directory
        #[arg(long, value_name = "PATH")]
        project: Option<PathBuf>,
        /// Only look at the last N days
        #[arg(long, value_name = "N")]
        days: Option<u64>,
        /// Machine-readable output for scripting
        #[arg(long)]
        json: bool,
    },
    /// Set a project up: works for a brand-new project and for attaching to an
    /// existing one (prints its baseline from history)
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Verified before/after for a project since `init` captured its baseline
    Delta {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Write a Markdown report for a project (findings + delta + trend);
    /// each run appends a snapshot, so the trend table grows over time
    Report {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Where to write the report (default: CODEUNLIMITED_REPORT.md in the project)
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Audit {
            source,
            project,
            days,
            json,
        } => {
            let p = project.as_deref();
            let mut reqs = Vec::new();
            let mut peak: parsers::LimitPeak = None;
            if matches!(source, Source::All | Source::Claude) {
                reqs.extend(parsers::iter_claude(p));
            }
            if matches!(source, Source::All | Source::Codex) {
                let (cx, pk) = parsers::iter_codex_full(p);
                reqs.extend(cx);
                peak = pk;
            }
            if let Some(n) = days {
                let cutoff = chrono::Utc::now().timestamp() - (n as i64) * 86_400;
                reqs.retain(|r| r.ts.is_some_and(|t| t >= cutoff));
            }
            let findings = detectors::run_all(&reqs);
            if json {
                println!("{}", report::render_json(&reqs, &findings));
            } else {
                if let Some(p) = p {
                    println!("[scope: {}]", p.display());
                }
                println!("{}", report::render(&reqs, &findings));
                if let Some((used, win)) = peak {
                    println!(
                        " Codex rate limit: peak {:.0}% of the {:.0}-day window observed - \
                         every token below funds more work.",
                        used,
                        win as f64 / 1440.0
                    );
                }
            }
        }
        Cmd::Init { path } => {
            std::process::exit(initcmd::run(&path));
        }
        Cmd::Delta { path } => {
            std::process::exit(deltacmd::run(&path));
        }
        Cmd::Report { path, out } => {
            std::process::exit(reportcmd::run(&path, out.as_deref()));
        }
    }
}
