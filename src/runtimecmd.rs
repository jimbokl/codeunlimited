use std::fs;
use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use crate::runtime::engine::{
    init_run, recover, render_next_prompt, run_steps, status, step, InitRequest, RunRef,
    RunStatusView,
};
use crate::runtime::model::{
    ProviderConfig, RuntimeError, SubscriptionProfile, VerificationCommand,
    DEFAULT_MAX_ATTEMPTS_PER_REVISION, DEFAULT_MAX_STEPS, DEFAULT_OBSERVATION_BUDGET_BYTES,
    DEFAULT_PROMPT_BUDGET_BYTES, DEFAULT_PROVIDER_TIMEOUT_SECONDS, DEFAULT_STATE_BUDGET_BYTES,
    DEFAULT_WORKFLOW_BUDGET_BYTES, MAX_OBSERVATION_BUDGET_BYTES,
};
use crate::runtime::provider::ProcessProvider;

const EXIT_INVALID_INPUT: i32 = 2;
const EXIT_BUSY: i32 = 3;
const EXIT_MISSING_PROVIDER: i32 = 4;
const EXIT_PROVIDER_FAILURE: i32 = 5;
const EXIT_OVER_BUDGET: i32 = 6;
const EXIT_INVALID_TRANSITION: i32 = 7;
const EXIT_RECOVERY_REQUIRED: i32 = 8;
const EXIT_TIMEOUT: i32 = 9;
const EXIT_TERMINAL: i32 = 10;

fn parse_auto_steps(value: &str) -> Result<usize, String> {
    let steps = value
        .parse::<usize>()
        .map_err(|_| "steps must be an integer from 1 through 100".to_string())?;
    (1..=100)
        .contains(&steps)
        .then_some(steps)
        .ok_or_else(|| "steps must be from 1 through 100".to_string())
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ProviderKind {
    Claude,
    Codex,
    Command,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SubscriptionProfileArg {
    Standard,
    Lean,
}

impl From<SubscriptionProfileArg> for SubscriptionProfile {
    fn from(value: SubscriptionProfileArg) -> Self {
        match value {
            SubscriptionProfileArg::Standard => Self::Standard,
            SubscriptionProfileArg::Lean => Self::Lean,
        }
    }
}

#[derive(Debug, Args)]
pub struct TargetArgs {
    /// Durable run name within the project
    name: String,
    /// Project root containing .codeunlimited/runs
    #[arg(long, default_value = ".", value_name = "PATH")]
    project: PathBuf,
}

#[derive(Debug, Subcommand)]
pub enum RunCmd {
    /// Create durable state without invoking a provider
    Init {
        /// Durable run name within the project
        name: String,
        /// Project root
        #[arg(long, default_value = ".", value_name = "PATH")]
        project: PathBuf,
        /// Markdown workflow/skill file to snapshot
        #[arg(long, value_name = "FILE")]
        skill: PathBuf,
        /// Terminal objective supplied to every bounded step
        #[arg(long)]
        objective: String,
        /// Ephemeral provider adapter
        #[arg(long, value_enum)]
        provider: ProviderKind,
        /// Provider binary; defaults to claude or codex for built-ins
        #[arg(long, value_name = "PROGRAM")]
        provider_executable: Option<PathBuf>,
        /// Exact provider argument; repeat to preserve argv boundaries
        #[arg(long, value_name = "ARG")]
        provider_arg: Vec<String>,
        /// Subscription CLI surface: standard preserves integrations; lean minimizes them
        #[arg(long, value_enum)]
        subscription_profile: Option<SubscriptionProfileArg>,
        /// Verification executable (no shell)
        #[arg(long, value_name = "PROGRAM")]
        verify_program: Option<PathBuf>,
        /// Exact verification argument; repeat to preserve argv boundaries
        #[arg(long, value_name = "ARG", requires = "verify_program")]
        verify_arg: Vec<String>,
        /// Run verification after every successful provider step
        #[arg(long)]
        verify_every_step: bool,
        /// Allow completion without a configured verification command
        #[arg(long)]
        allow_unverified_completion: bool,
        /// Maximum provider attempts for the complete run
        #[arg(long, default_value_t = DEFAULT_MAX_STEPS)]
        max_steps: u64,
        /// Maximum failed attempts against one state revision
        #[arg(long, default_value_t = DEFAULT_MAX_ATTEMPTS_PER_REVISION)]
        max_attempts_per_revision: u64,
        /// Hard timeout for one provider process
        #[arg(long, default_value_t = DEFAULT_PROVIDER_TIMEOUT_SECONDS)]
        provider_timeout_seconds: u64,
        /// Maximum immutable workflow bytes
        #[arg(long, default_value_t = DEFAULT_WORKFLOW_BUDGET_BYTES)]
        workflow_budget_bytes: usize,
        /// Maximum serialized hot-state bytes
        #[arg(long, default_value_t = DEFAULT_STATE_BUDGET_BYTES)]
        state_budget_bytes: usize,
        /// Maximum latest-observation bytes
        #[arg(long, default_value_t = DEFAULT_OBSERVATION_BUDGET_BYTES)]
        observation_budget_bytes: usize,
        /// Maximum complete prompt bytes
        #[arg(long, default_value_t = DEFAULT_PROMPT_BUDGET_BYTES)]
        prompt_budget_bytes: usize,
    },
    /// Show bounded state, prompt sizes, attempts, usage, and provider isolation
    Status {
        #[command(flatten)]
        target: TargetArgs,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Print the exact next prompt without invoking a provider
    Prompt {
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Invoke one fresh provider process and commit at most one transition
    Step {
        #[command(flatten)]
        target: TargetArgs,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Execute a finite number of fresh-process steps
    Auto {
        #[command(flatten)]
        target: TargetArgs,
        /// Maximum steps for this invocation (1-100)
        #[arg(long, value_parser = parse_auto_steps)]
        steps: usize,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Acknowledge an ambiguous attempt with a bounded observation
    Recover {
        #[command(flatten)]
        target: TargetArgs,
        /// Regular UTF-8 file describing the accepted repository state
        #[arg(long, value_name = "FILE")]
        observation: PathBuf,
    },
}

pub fn run(command: RunCmd) -> i32 {
    match execute(command) {
        Ok(()) => 0,
        Err(error) => {
            let (code, category) = error.exit();
            eprintln!("runtime[{category}]: {error}");
            code
        }
    }
}

fn execute(command: RunCmd) -> Result<(), RunCliError> {
    match command {
        RunCmd::Init {
            name,
            project,
            skill,
            objective,
            provider,
            provider_executable,
            provider_arg,
            subscription_profile,
            verify_program,
            verify_arg,
            verify_every_step,
            allow_unverified_completion,
            max_steps,
            max_attempts_per_revision,
            provider_timeout_seconds,
            workflow_budget_bytes,
            state_budget_bytes,
            observation_budget_bytes,
            prompt_budget_bytes,
        } => {
            let project = resolve_project(&project)?;
            let executable = match (provider, provider_executable) {
                (ProviderKind::Claude, value) => value.unwrap_or_else(|| "claude".into()),
                (ProviderKind::Codex, value) => value.unwrap_or_else(|| "codex".into()),
                (ProviderKind::Command, Some(value)) => value,
                (ProviderKind::Command, None) => {
                    return Err(RunCliError::Input(
                        "--provider-executable is required for command providers",
                    ))
                }
            };
            let provider = match provider {
                ProviderKind::Claude => ProviderConfig::Claude {
                    executable,
                    args: provider_arg,
                    subscription_profile: subscription_profile
                        .map(Into::into)
                        .unwrap_or(SubscriptionProfile::Lean),
                },
                ProviderKind::Codex => ProviderConfig::Codex {
                    executable,
                    args: provider_arg,
                    subscription_profile: subscription_profile
                        .map(Into::into)
                        .unwrap_or(SubscriptionProfile::Lean),
                },
                ProviderKind::Command => ProviderConfig::Command {
                    executable: if subscription_profile.is_some() {
                        return Err(RunCliError::Input(
                            "--subscription-profile is only valid for claude and codex",
                        ));
                    } else {
                        executable
                    },
                    args: provider_arg,
                },
            };
            let verification_command = verify_program.map(|program| VerificationCommand {
                program,
                args: verify_arg,
            });
            let mut request = InitRequest::new(project, name, skill, objective, provider);
            request.max_steps = max_steps;
            request.max_attempts_per_revision = max_attempts_per_revision;
            request.provider_timeout_seconds = provider_timeout_seconds;
            request.workflow_budget_bytes = workflow_budget_bytes;
            request.state_budget_bytes = state_budget_bytes;
            request.observation_budget_bytes = observation_budget_bytes;
            request.prompt_budget_bytes = prompt_budget_bytes;
            request.verification_command = verification_command;
            request.verify_every_step = verify_every_step;
            request.allow_unverified_completion = allow_unverified_completion;
            let view = init_run(request)?;
            println!(
                "Initialized run {} at revision {}.\nRecommended .gitignore entry: .codeunlimited/runs/",
                view.run_name, view.revision
            );
        }
        RunCmd::Status { target, json } => {
            let mut view = status(&reference(target)?)?;
            redact_secret_arguments(&mut view.provider.args);
            if json {
                print_json(&view)?;
            } else {
                print_status(&view);
            }
        }
        RunCmd::Prompt { target } => {
            let prompt = render_next_prompt(&reference(target)?)?;
            io::stdout()
                .write_all(&prompt.bytes)
                .map_err(|_| RunCliError::Output)?;
        }
        RunCmd::Step { target, json } => {
            let report = step(&reference(target)?, &ProcessProvider)?;
            if json {
                print_json(&report)?;
            } else {
                println!(
                    "run={} revision={} status={:?} attempt={} prompt_bytes={}",
                    report.run_name,
                    report.revision,
                    report.status,
                    report.attempt,
                    report.prompt_bytes
                );
            }
        }
        RunCmd::Auto {
            target,
            steps,
            json,
        } => {
            let count = NonZeroUsize::new(steps)
                .ok_or(RunCliError::Input("--steps must be between 1 and 100"))?;
            let report = run_steps(&reference(target)?, count, &ProcessProvider)?;
            if json {
                print_json(&report)?;
            } else {
                println!(
                    "run={} committed_steps={}",
                    report.run_name,
                    report.steps.len()
                );
            }
        }
        RunCmd::Recover {
            target,
            observation,
        } => {
            let observation = read_observation(&observation)?;
            let view = recover(&reference(target)?, &observation)?;
            println!(
                "Recovered run {} at revision {}; repository changes were preserved.",
                view.run_name, view.revision
            );
        }
    }
    Ok(())
}

fn reference(target: TargetArgs) -> Result<RunRef, RunCliError> {
    Ok(RunRef::new(resolve_project(&target.project)?, target.name))
}

fn resolve_project(path: &Path) -> Result<PathBuf, RunCliError> {
    path.canonicalize()
        .ok()
        .filter(|path| path.is_dir())
        .ok_or(RunCliError::Input("project must be an existing directory"))
}

fn read_observation(path: &Path) -> Result<Vec<u8>, RunCliError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| RunCliError::Input("observation must be a readable regular file"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_OBSERVATION_BUDGET_BYTES as u64
    {
        return Err(RunCliError::Input(
            "observation must be a bounded regular file",
        ));
    }
    fs::read(path).map_err(|_| RunCliError::Input("observation file could not be read"))
}

fn print_json(value: &impl Serialize) -> Result<(), RunCliError> {
    let rendered = serde_json::to_string_pretty(value).map_err(|_| RunCliError::Output)?;
    println!("{rendered}");
    Ok(())
}

fn print_status(view: &RunStatusView) {
    println!(
        "run={} revision={} status={:?} recovery_required={} busy={}",
        view.run_name, view.revision, view.status, view.recovery_required, view.busy
    );
    println!(
        "prompt_bytes={} stable={} dynamic={} attempts={}",
        view.prompt_bytes, view.stable_prompt_bytes, view.dynamic_prompt_bytes, view.attempts
    );
    println!(
        "provider={} layer={} capability={} profile={} executable={} isolation={}",
        view.provider.kind,
        view.provider.layer,
        view.provider.capability,
        view.provider
            .subscription_profile
            .as_deref()
            .unwrap_or("n/a"),
        view.provider.executable,
        view.provider.isolation
    );
    println!("provider_args={:?}", view.provider.args);
}

fn redact_secret_arguments(args: &mut [String]) {
    let mut redact_next = false;
    for argument in args {
        if redact_next {
            *argument = "<redacted>".into();
            redact_next = false;
            continue;
        }
        let lower = argument.to_ascii_lowercase();
        let (flag, has_value) = lower
            .split_once('=')
            .map_or((lower.as_str(), false), |(flag, _)| (flag, true));
        if is_secret_flag(flag) {
            if has_value {
                let original_flag = argument.split_once('=').map_or(argument.as_str(), |v| v.0);
                *argument = format!("{original_flag}=<redacted>");
            } else {
                redact_next = true;
            }
        }
    }
}

fn is_secret_flag(value: &str) -> bool {
    ["--api-key", "--apikey", "--token", "--password", "--secret"].contains(&value)
}

#[derive(Debug)]
enum RunCliError {
    Input(&'static str),
    Runtime(RuntimeError),
    Output,
}

impl RunCliError {
    fn exit(&self) -> (i32, &'static str) {
        match self {
            Self::Input(_) | Self::Output => (EXIT_INVALID_INPUT, "invalid_input"),
            Self::Runtime(error) => runtime_exit(error),
        }
    }
}

impl std::fmt::Display for RunCliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Input(message) => formatter.write_str(message),
            Self::Runtime(error) => error.fmt(formatter),
            Self::Output => formatter.write_str("could not write command output"),
        }
    }
}

impl From<RuntimeError> for RunCliError {
    fn from(value: RuntimeError) -> Self {
        Self::Runtime(value)
    }
}

fn runtime_exit(error: &RuntimeError) -> (i32, &'static str) {
    match error {
        RuntimeError::RunBusy => (EXIT_BUSY, "busy"),
        RuntimeError::RecoveryRequired => (EXIT_RECOVERY_REQUIRED, "recovery_required"),
        RuntimeError::FieldTooLarge { .. }
        | RuntimeError::TooManyItems { .. }
        | RuntimeError::StateBudgetExceeded { .. } => (EXIT_OVER_BUDGET, "over_budget"),
        RuntimeError::ProviderFailed(category) if category == "missing_executable" => {
            (EXIT_MISSING_PROVIDER, "missing_provider")
        }
        RuntimeError::ProviderFailed(category) if category == "timeout" => {
            (EXIT_TIMEOUT, "timeout")
        }
        RuntimeError::ProviderFailed(category)
            if category == "invalid_output" || category.starts_with("verification_") =>
        {
            (EXIT_INVALID_TRANSITION, "invalid_transition")
        }
        RuntimeError::ProviderFailed(_) => (EXIT_PROVIDER_FAILURE, "provider_failure"),
        RuntimeError::AttemptLimit | RuntimeError::TerminalRun => (EXIT_TERMINAL, "terminal"),
        RuntimeError::StaleRevision { .. }
        | RuntimeError::IncompleteQueue
        | RuntimeError::VerificationRequired
        | RuntimeError::BlockerRequired
        | RuntimeError::ArtifactMismatch
        | RuntimeError::SummaryRequiredForArchive
        | RuntimeError::DuplicateId(_)
        | RuntimeError::InvalidRelativePath(_)
        | RuntimeError::InvalidDigest(_) => (EXIT_INVALID_TRANSITION, "invalid_transition"),
        _ => (EXIT_INVALID_INPUT, "invalid_input"),
    }
}

#[cfg(test)]
mod tests {
    use super::redact_secret_arguments;

    #[test]
    fn status_redaction_preserves_argv_shape_without_exposing_values() {
        let mut args = vec![
            "--TOKEN".into(),
            "private-one".into(),
            "--api-key=private-two".into(),
            "safe".into(),
        ];
        redact_secret_arguments(&mut args);
        assert_eq!(
            args,
            ["--TOKEN", "<redacted>", "--api-key=<redacted>", "safe"]
        );
    }
}
