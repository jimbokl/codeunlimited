use codeunlimited::{
    comparecmd, config, deltacmd, detectors, doctor, experiment, fixcmd, forecast, initcmd,
    parsers, report, reportcmd, schedule, skillcmd, techniques,
};

use std::io::IsTerminal;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

fn parse_days(value: &str) -> Result<u64, String> {
    let days = value
        .parse::<u64>()
        .map_err(|_| "days must be an integer from 1 through 36500".to_string())?;
    (1..=36_500)
        .contains(&days)
        .then_some(days)
        .ok_or_else(|| "days must be from 1 through 36500".to_string())
}

#[derive(Parser)]
#[command(
    name = "codeunlimited",
    version,
    about = "Offline estimates of token-leak opportunities, one-command fixes, and \
             before/after tracking for Claude Code and Codex CLI."
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
enum ExperimentCmd {
    /// Start a bounded measurement window
    Start {
        name: String,
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Finish an active window and save exact observed counters
    Finish {
        name: String,
        #[arg(long, value_name = "N")]
        tasks: u64,
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Backfill a completed window from RFC 3339 boundaries
    Record {
        name: String,
        #[arg(long, value_name = "RFC3339")]
        from: String,
        #[arg(long, value_name = "RFC3339")]
        to: String,
        #[arg(long, value_name = "N")]
        tasks: u64,
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Compare exact observed counters per completed task
    Compare {
        control: String,
        treatment: String,
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// List saved experiment windows
    List {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum Cmd {
    /// Find estimated reclaimable opportunities (offline, local logs only)
    Audit {
        #[arg(long, value_enum, default_value = "all")]
        source: Source,
        /// Scope the report to one project directory
        #[arg(long, value_name = "PATH")]
        project: Option<PathBuf>,
        /// Only look at the last N days
        #[arg(long, value_name = "N", value_parser = parse_days)]
        days: Option<u64>,
        /// Machine-readable output for scripting
        #[arg(long)]
        json: bool,
        /// Disable the optional Codex metadata index
        #[arg(long)]
        no_index: bool,
        /// Include local scan counters in JSON output
        #[arg(long, requires = "json")]
        scan_stats: bool,
    },
    /// Set a project up: works for a brand-new project and for attaching to an
    /// existing one (prints its baseline from history)
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Before/after tracking since `init` captured a project baseline
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
        /// Also write CODEUNLIMITED_BADGE.svg (estimated opportunity as a README badge)
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
        #[arg(long, default_value = "7", value_parser = parse_days)]
        days: u64,
    },
    /// Install a weekly `report --all` task (Windows Task Scheduler / cron line)
    Schedule {
        /// Remove the scheduled task instead of creating it
        #[arg(long)]
        remove: bool,
    },
    /// Install the Claude Code skill (/codeunlimited inside a session)
    Skill {
        /// Replace a different existing skill, preserving its first backup
        #[arg(long)]
        force: bool,
    },
    /// Record and compare bounded token-accounting experiments
    Experiment {
        #[command(subcommand)]
        command: ExperimentCmd,
    },
    /// List every efficiency technique with on/off status and how to toggle it
    Techniques {
        /// Evaluate against this project's config (default: current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
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
            no_index,
            scan_stats,
        } => {
            let p = project.as_deref();
            let cutoff = days.map(|n| {
                chrono::Utc::now()
                    .timestamp()
                    .saturating_sub((n as i64).saturating_mul(86_400))
            });
            let options = parsers::ScanOptions {
                project: project.clone(),
                since: cutoff,
                use_index: !no_index,
            };
            let mut reqs = Vec::new();
            let mut series: parsers::LimitSeries = Vec::new();
            let mut stats = parsers::ScanStats::default();
            if matches!(source, Source::All | Source::Claude) {
                let scan = parsers::scan_claude(&options);
                reqs.extend(scan.requests);
                stats += scan.stats;
            }
            if matches!(source, Source::All | Source::Codex) {
                let scan = parsers::scan_codex(&options);
                reqs.extend(scan.requests);
                series = scan.series;
                stats += scan.stats;
            }
            let cfg = config::Config::load_for(p);
            reqs.retain(|r| !cfg.is_ignored(&r.project));
            let findings = detectors::run_all(&reqs, &cfg);
            if json {
                println!(
                    "{}",
                    report::render_json(&reqs, &findings, scan_stats.then_some(&stats))
                );
            } else {
                if let Some(p) = p {
                    println!("[scope: {}]", p.display());
                }
                let color = std::io::stdout().is_terminal();
                println!("{}", report::render(&reqs, &findings, color));
                if let Some((used, win)) = parsers::peak(&series) {
                    println!(
                        " Codex rate limit: peak {:.0}% of the {:.0}-day window observed.",
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
            std::process::exit(comparecmd::run(days));
        }
        Cmd::Schedule { remove } => {
            std::process::exit(schedule::run(remove));
        }
        Cmd::Skill { force } => {
            std::process::exit(skillcmd::run(force));
        }
        Cmd::Experiment { command } => {
            std::process::exit(match command {
                ExperimentCmd::Start { name, path } => experiment::start(&name, &path),
                ExperimentCmd::Finish {
                    name,
                    tasks,
                    path,
                    json,
                } => experiment::finish(&name, tasks, &path, json),
                ExperimentCmd::Record {
                    name,
                    from,
                    to,
                    tasks,
                    path,
                    json,
                } => experiment::record(&name, &from, &to, tasks, &path, json),
                ExperimentCmd::Compare {
                    control,
                    treatment,
                    path,
                    json,
                } => experiment::compare(&control, &treatment, &path, json),
                ExperimentCmd::List { path, json } => experiment::list(&path, json),
            });
        }
        Cmd::Techniques { path } => {
            let root = path.canonicalize().ok();
            let cfg = config::Config::load_for(root.as_deref());
            std::process::exit(techniques::list(&cfg));
        }
    }
}
