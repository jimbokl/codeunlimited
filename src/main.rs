use codeunlimited::{
    comparecmd, config, deltacmd, detectors, doctor, fixcmd, forecast, initcmd, parsers, report,
    reportcmd, schedule, skillcmd,
};

use std::io::IsTerminal;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "codeunlimited",
    version,
    about = "Set up once - up to 50% more work from the same Claude Code / Codex \
             limits. Offline token-leak audit, one-command fixes, verified savings."
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
    /// Write a report (Markdown + styled HTML) for a project - findings, delta,
    /// trend; each run appends a snapshot, so the trend grows over time
    Report {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Where to write the report (default: CODEUNLIMITED_REPORT.md in the project)
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
        /// Summary across every project seen by init/fix/report, plus global trend
        #[arg(long)]
        all: bool,
        /// Also write CODEUNLIMITED_BADGE.svg (reclaimable % as a README badge)
        #[arg(long)]
        badge: bool,
        /// Hash project names so the report can be shared publicly
        #[arg(long)]
        anonymize: bool,
    },
    /// Turn audit findings into concrete project changes (dry-run by default)
    Fix {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Actually write the changes (default is a dry run)
        #[arg(long)]
        apply: bool,
        /// Run over every registered project instead of one path
        #[arg(long)]
        all: bool,
    },
    /// Check that the parsers still understand your local log formats
    Doctor,
    /// This period vs the previous one: is the limit spent better or worse?
    Compare {
        /// Window size in days (compares last N days vs the N before)
        #[arg(long, default_value = "7")]
        days: u64,
    },
    /// Install a weekly `report --all` task (Windows Task Scheduler / cron line)
    Schedule {
        /// Remove the scheduled task instead of creating it
        #[arg(long)]
        remove: bool,
    },
    /// Install the Claude Code skill (/codeunlimited inside a session)
    Skill,
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
            let mut series: parsers::LimitSeries = Vec::new();
            if matches!(source, Source::All | Source::Claude) {
                reqs.extend(parsers::iter_claude(p));
            }
            if matches!(source, Source::All | Source::Codex) {
                let (cx, s) = parsers::iter_codex_full(p);
                reqs.extend(cx);
                series = s;
            }
            reqs.retain(|r| !config::ignored(&r.project));
            if let Some(n) = days {
                let cutoff = chrono::Utc::now().timestamp() - (n as i64) * 86_400;
                reqs.retain(|r| r.ts.is_some_and(|t| t >= cutoff));
                series.retain(|&(t, _, _)| t >= cutoff);
            }
            let findings = detectors::run_all(&reqs);
            if json {
                println!("{}", report::render_json(&reqs, &findings));
            } else {
                if let Some(p) = p {
                    println!("[scope: {}]", p.display());
                }
                let color = std::io::stdout().is_terminal();
                println!("{}", report::render(&reqs, &findings, color));
                if let Some((used, win)) = parsers::peak(&series) {
                    println!(
                        " Codex rate limit: peak {:.0}% of the {:.0}-day window observed - \
                         every token below funds more work.",
                        used,
                        win as f64 / 1440.0
                    );
                }
                for line in forecast::forecast(&reqs, &series) {
                    println!(" Forecast: {line}");
                }
            }
        }
        Cmd::Init { path } => {
            std::process::exit(initcmd::run(&path));
        }
        Cmd::Delta { path } => {
            std::process::exit(deltacmd::run(&path));
        }
        Cmd::Report {
            path,
            out,
            all,
            badge,
            anonymize,
        } => {
            std::process::exit(if all {
                reportcmd::run_all(out.as_deref(), badge, anonymize)
            } else {
                reportcmd::run(&path, out.as_deref(), badge, anonymize)
            });
        }
        Cmd::Fix { path, apply, all } => {
            std::process::exit(if all {
                fixcmd::run_all(apply)
            } else {
                fixcmd::run(&path, apply)
            });
        }
        Cmd::Doctor => {
            std::process::exit(doctor::run());
        }
        Cmd::Compare { days } => {
            std::process::exit(comparecmd::run(days.max(1)));
        }
        Cmd::Schedule { remove } => {
            std::process::exit(schedule::run(remove));
        }
        Cmd::Skill => {
            std::process::exit(skillcmd::run());
        }
    }
}
