mod detectors;
mod initcmd;
mod parsers;
mod report;
mod types;

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
    },
    /// Set a project up: works for a brand-new project and for attaching to an
    /// existing one (prints its baseline from history)
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Audit { source, project } => {
            let p = project.as_deref();
            let mut reqs = Vec::new();
            if matches!(source, Source::All | Source::Claude) {
                reqs.extend(parsers::iter_claude(p));
            }
            if matches!(source, Source::All | Source::Codex) {
                reqs.extend(parsers::iter_codex(p));
            }
            let findings = detectors::run_all(&reqs);
            if let Some(p) = p {
                println!("[scope: {}]", p.display());
            }
            println!("{}", report::render(&reqs, &findings));
        }
        Cmd::Init { path } => {
            std::process::exit(initcmd::run(&path));
        }
    }
}
