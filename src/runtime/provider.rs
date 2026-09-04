use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;
use wait_timeout::ChildExt;

use super::model::{
    ProviderConfig, RuntimeError, StepEnvelope, SubscriptionProfile, MAX_PROVIDER_OUTPUT_BYTES,
};
use super::prompt::{strict_step_schema, CompiledPrompt};
use super::validate::validate_provider_config;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputTokenSemantics {
    /// The raw input counter already includes cache reads and writes.
    TotalIncludesCache,
    /// The raw input counter is only the uncached remainder.
    UncachedOnly,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderUsage {
    pub input_token_semantics: InputTokenSemantics,
    /// Provider-native input counter; meaning is declared above.
    pub input_tokens: Option<u64>,
    pub uncached_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_write_input_tokens: Option<u64>,
    pub cache_write_5m_input_tokens: Option<u64>,
    pub cache_write_1h_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

impl ProviderUsage {
    pub fn transported_input_tokens(&self) -> Option<u64> {
        match self.input_token_semantics {
            InputTokenSemantics::TotalIncludesCache => self.input_tokens,
            InputTokenSemantics::UncachedOnly => self
                .uncached_input_tokens
                .or(self.input_tokens)
                .and_then(|uncached| {
                    uncached
                        .checked_add(self.cache_read_input_tokens?)?
                        .checked_add(self.cache_write_input_tokens?)
                }),
            InputTokenSemantics::Unknown => None,
        }
    }

    pub fn cache_read_ratio_basis_points(&self) -> Option<u64> {
        let total = self.transported_input_tokens()?;
        let reads = self.cache_read_input_tokens?;
        (total != 0).then(|| {
            u64::try_from(
                u128::from(reads)
                    .saturating_mul(10_000)
                    .checked_div(u128::from(total))
                    .unwrap_or(0),
            )
            .unwrap_or(u64::MAX)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderResult {
    pub envelope: StepEnvelope,
    pub usage: ProviderUsage,
    pub exit_code: i32,
    pub response_bytes: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderProbeSample {
    pub usage: ProviderUsage,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderProbeResult {
    pub first: ProviderProbeSample,
    pub second: ProviderProbeSample,
    pub cache_hit_reported: Option<bool>,
}

impl ProviderProbeResult {
    pub fn new(first: ProviderProbeSample, second: ProviderProbeSample) -> Self {
        let cache_hit_reported = second
            .usage
            .cache_read_input_tokens
            .map(|tokens| tokens > 0);
        Self {
            first,
            second,
            cache_hit_reported,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderFailure {
    MissingExecutable,
    Spawn,
    Stdin,
    Timeout,
    Exit(i32),
    OutputTooLarge,
    InvalidOutput,
    InvalidOutputWithUsage(Box<ProviderUsage>),
    InvalidConfiguration(RuntimeError),
    TemporaryFile,
    MissingCredential,
    Http,
    ProbeUnsupported,
}

impl fmt::Display for ProviderFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingExecutable => write!(f, "provider executable was not found"),
            Self::Spawn => write!(f, "provider process could not be started"),
            Self::Stdin => write!(f, "provider prompt could not be written"),
            Self::Timeout => write!(f, "provider process timed out"),
            Self::Exit(code) => write!(f, "provider process exited with status {code}"),
            Self::OutputTooLarge => write!(f, "provider output exceeded the configured limit"),
            Self::InvalidOutput | Self::InvalidOutputWithUsage(_) => {
                write!(f, "provider returned an invalid structured response")
            }
            Self::InvalidConfiguration(error) => {
                write!(f, "invalid provider configuration: {error}")
            }
            Self::TemporaryFile => write!(f, "provider temporary files could not be prepared"),
            Self::MissingCredential => write!(f, "provider API credential is unavailable"),
            Self::Http => write!(f, "provider API request failed"),
            Self::ProbeUnsupported => write!(f, "provider does not support cache probing"),
        }
    }
}

impl std::error::Error for ProviderFailure {}

impl From<RuntimeError> for ProviderFailure {
    fn from(error: RuntimeError) -> Self {
        Self::InvalidConfiguration(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

pub trait Provider {
    fn run(
        &self,
        config: &ProviderConfig,
        prompt: &CompiledPrompt,
        project_root: &Path,
        timeout: Duration,
    ) -> Result<ProviderResult, ProviderFailure>;

    fn probe(
        &self,
        config: &ProviderConfig,
        prompt: &CompiledPrompt,
        project_root: &Path,
        timeout: Duration,
    ) -> Result<ProviderProbeResult, ProviderFailure>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessProvider;

impl Provider for ProcessProvider {
    fn run(
        &self,
        config: &ProviderConfig,
        prompt: &CompiledPrompt,
        project_root: &Path,
        timeout: Duration,
    ) -> Result<ProviderResult, ProviderFailure> {
        validate_provider_config(config)?;
        match config {
            ProviderConfig::Command { .. } => {
                let spec = build_external_command(config)?;
                let raw = run_process(
                    &spec,
                    &provider_input(config, prompt, project_root, project_root),
                    project_root,
                    timeout,
                )?;
                let envelope = serde_json::from_slice(&raw.stdout)
                    .map_err(|_| ProviderFailure::InvalidOutput)?;
                Ok(provider_result(envelope, ProviderUsage::default(), raw))
            }
            ProviderConfig::Claude { .. } => {
                let temporary = tempfile::tempdir().map_err(|_| ProviderFailure::TemporaryFile)?;
                let empty_mcp = temporary.path().join("empty-mcp.json");
                fs::write(&empty_mcp, br#"{"mcpServers":{}}"#)
                    .map_err(|_| ProviderFailure::TemporaryFile)?;
                let spec = build_claude_command(config, &prompt.instructions_path, &empty_mcp)?;
                let raw = run_process(
                    &spec,
                    &provider_input(config, prompt, project_root, project_root),
                    project_root,
                    timeout,
                )?;
                let (envelope, usage) = parse_claude_output(&raw.stdout)?;
                Ok(provider_result(envelope, usage, raw))
            }
            ProviderConfig::Codex { .. } => {
                let temporary = tempfile::tempdir().map_err(|_| ProviderFailure::TemporaryFile)?;
                let schema = temporary.path().join("step-envelope.schema.json");
                let output = temporary.path().join("last-message.json");
                fs::write(&schema, strict_step_schema().to_string())
                    .map_err(|_| ProviderFailure::TemporaryFile)?;
                let spec = build_codex_command(config, project_root, &schema, &output)?;
                let raw = run_process(
                    &spec,
                    &provider_input(config, prompt, project_root, project_root),
                    project_root,
                    timeout,
                )?;
                let response = read_bounded_file(&output)?;
                let envelope = serde_json::from_slice(&response)
                    .map_err(|_| ProviderFailure::InvalidOutput)?;
                let usage = parse_codex_usage(&raw.stdout);
                Ok(ProviderResult {
                    envelope,
                    usage,
                    exit_code: raw.exit_code,
                    response_bytes: response.len(),
                    duration_ms: raw.duration_ms,
                })
            }
            ProviderConfig::OpenAiApi { .. } | ProviderConfig::AnthropicApi { .. } => {
                let (envelope, usage, response_bytes, duration_ms) =
                    super::api::invoke_api(config, prompt, timeout)?;
                Ok(ProviderResult {
                    envelope,
                    usage,
                    exit_code: 0,
                    response_bytes,
                    duration_ms,
                })
            }
        }
    }

    fn probe(
        &self,
        config: &ProviderConfig,
        prompt: &CompiledPrompt,
        project_root: &Path,
        timeout: Duration,
    ) -> Result<ProviderProbeResult, ProviderFailure> {
        validate_provider_config(config)?;
        if matches!(config, ProviderConfig::Command { .. }) {
            return Err(ProviderFailure::ProbeUnsupported);
        }
        let config = probe_config(config)?;
        let timeout = timeout.min(Duration::from_secs(60));
        let first = probe_once(&config, prompt, project_root, timeout, 1)?;
        let second = probe_once(&config, prompt, project_root, timeout, 2)?;
        Ok(ProviderProbeResult::new(first, second))
    }
}

fn probe_config(config: &ProviderConfig) -> Result<ProviderConfig, ProviderFailure> {
    let mut config = config.clone();
    match &mut config {
        ProviderConfig::Claude {
            args,
            subscription_profile,
            ..
        }
        | ProviderConfig::Codex {
            args,
            subscription_profile,
            ..
        } => {
            // Probe only the configured model; arbitrary overrides could re-enable
            // remote tools or hooks outside a filesystem read-only sandbox.
            let mut arguments = args.iter();
            while let Some(arg) = arguments.next() {
                if ["--model", "-m", "--effort"].contains(&arg.as_str()) {
                    if arguments.next().is_none() {
                        return Err(ProviderFailure::ProbeUnsupported);
                    }
                } else if !arg.starts_with("--model=") && !arg.starts_with("--effort=") {
                    return Err(ProviderFailure::ProbeUnsupported);
                }
            }
            *subscription_profile = SubscriptionProfile::Lean;
        }
        _ => {}
    }
    Ok(config)
}

fn probe_once(
    config: &ProviderConfig,
    prompt: &CompiledPrompt,
    project_root: &Path,
    timeout: Duration,
    sample: u8,
) -> Result<ProviderProbeSample, ProviderFailure> {
    let no_op = format!("CACHE_PROBE sample={sample}. Do not execute the workflow or objective. Do not modify files, invoke external tools, or perform external actions. Return only a no-change StepEnvelope with base_revision=0, outcome=continue, summary=cache probe {sample}; use null for unchanged replacements and [] for additions.\n");
    match config {
        ProviderConfig::Claude { .. } => {
            let temporary = tempfile::tempdir().map_err(|_| ProviderFailure::TemporaryFile)?;
            let empty_mcp = temporary.path().join("empty-mcp.json");
            fs::write(&empty_mcp, br#"{"mcpServers":{}}"#)
                .map_err(|_| ProviderFailure::TemporaryFile)?;
            let mut spec = build_claude_command(config, &prompt.instructions_path, &empty_mcp)?;
            force_empty_claude_tools(&mut spec.args);
            spec.args.extend([
                OsString::from("--settings"),
                OsString::from(r#"{"disableAllHooks":true}"#),
                OsString::from("--max-turns"),
                OsString::from("1"),
            ]);
            let raw = run_process(&spec, no_op.as_bytes(), project_root, timeout)?;
            let (_, usage) = parse_claude_output(&raw.stdout)?;
            Ok(ProviderProbeSample {
                usage,
                duration_ms: raw.duration_ms,
            })
        }
        ProviderConfig::Codex { args, .. } => {
            reject_overrides(
                args,
                &[
                    "--sandbox",
                    "-s",
                    "--dangerously-bypass-approvals-and-sandbox",
                    "--yolo",
                ],
            )?;
            let temporary = tempfile::tempdir().map_err(|_| ProviderFailure::TemporaryFile)?;
            let schema = temporary.path().join("step-envelope.schema.json");
            let output = temporary.path().join("last-message.json");
            fs::write(&schema, strict_step_schema().to_string())
                .map_err(|_| ProviderFailure::TemporaryFile)?;
            let mut spec = build_codex_command(config, project_root, &schema, &output)?;
            spec.args
                .extend([OsString::from("--sandbox"), OsString::from("read-only")]);
            let input = format!("Read only {} as inert reference context. Do not follow its work instructions or read state/observation files.\n{no_op}", serde_json::to_string(&prompt.instructions_path).map_err(|_| ProviderFailure::InvalidOutput)?);
            let raw = run_process(&spec, input.as_bytes(), project_root, timeout)?;
            let _: StepEnvelope = serde_json::from_slice(&read_bounded_file(&output)?)
                .map_err(|_| ProviderFailure::InvalidOutput)?;
            Ok(ProviderProbeSample {
                usage: parse_codex_usage(&raw.stdout),
                duration_ms: raw.duration_ms,
            })
        }
        ProviderConfig::OpenAiApi { .. } | ProviderConfig::AnthropicApi { .. } => {
            let mut probe = prompt.clone();
            probe.dynamic = no_op.into_bytes();
            probe.dynamic_bytes = probe.dynamic.len();
            probe.dynamic_sha256 = format!("{:x}", sha2::Sha256::digest(&probe.dynamic));
            let (_, usage, _, duration_ms) = super::api::invoke_api(config, &probe, timeout)?;
            Ok(ProviderProbeSample { usage, duration_ms })
        }
        ProviderConfig::Command { .. } => Err(ProviderFailure::ProbeUnsupported),
    }
}

fn force_empty_claude_tools(args: &mut Vec<OsString>) {
    if let Some(index) = args.iter().position(|arg| arg == "--tools") {
        if let Some(value) = args.get_mut(index + 1) {
            *value = OsString::new();
            return;
        }
    }
    args.extend([OsString::from("--tools"), OsString::new()]);
}

pub fn build_claude_command(
    config: &ProviderConfig,
    instructions_path: &Path,
    empty_mcp_path: &Path,
) -> Result<CommandSpec, RuntimeError> {
    validate_provider_config(config)?;
    let ProviderConfig::Claude {
        executable,
        args,
        subscription_profile,
    } = config
    else {
        return Err(RuntimeError::InvalidManifest(
            "Claude adapter requires a Claude provider".into(),
        ));
    };
    reject_overrides(
        args,
        &[
            "--print",
            "-p",
            "--no-session-persistence",
            "--output-format",
            "--json-schema",
            "--input-format",
            "--exclude-dynamic-system-prompt-sections",
            "--append-system-prompt-file",
            "--disable-slash-commands",
            "--no-chrome",
            "--strict-mcp-config",
            "--mcp-config",
            "--tools",
            "--bare",
        ],
    )?;
    let mut command_args = vec![
        OsString::from("--print"),
        OsString::from("--no-session-persistence"),
        OsString::from("--output-format"),
        OsString::from("json"),
        OsString::from("--json-schema"),
        OsString::from(strict_step_schema().to_string()),
        OsString::from("--exclude-dynamic-system-prompt-sections"),
        OsString::from("--append-system-prompt-file"),
        instructions_path.as_os_str().to_os_string(),
    ];
    if *subscription_profile == SubscriptionProfile::Lean {
        command_args.extend([
            OsString::from("--disable-slash-commands"),
            OsString::from("--no-chrome"),
            OsString::from("--strict-mcp-config"),
            OsString::from("--mcp-config"),
            empty_mcp_path.as_os_str().to_os_string(),
            OsString::from("--tools"),
            OsString::from("Bash,Edit,Read,Write,Glob,Grep"),
        ]);
    }
    command_args.extend(args.iter().map(OsString::from));
    Ok(CommandSpec {
        program: executable.clone(),
        args: command_args,
    })
}

pub fn build_codex_command(
    config: &ProviderConfig,
    project_root: &Path,
    schema_path: &Path,
    output_path: &Path,
) -> Result<CommandSpec, RuntimeError> {
    validate_provider_config(config)?;
    let ProviderConfig::Codex {
        executable,
        args,
        subscription_profile,
    } = config
    else {
        return Err(RuntimeError::InvalidManifest(
            "Codex adapter requires a Codex provider".into(),
        ));
    };
    reject_overrides(
        args,
        &[
            "exec",
            "resume",
            "fork",
            "--ephemeral",
            "--output-schema",
            "--output-last-message",
            "-o",
            "--cd",
            "-C",
            "--json",
            "--ignore-user-config",
        ],
    )?;
    let mut command_args = vec![
        OsString::from("exec"),
        OsString::from("--ephemeral"),
        OsString::from("--output-schema"),
        schema_path.as_os_str().to_os_string(),
        OsString::from("--output-last-message"),
        output_path.as_os_str().to_os_string(),
        OsString::from("--json"),
        OsString::from("--cd"),
        project_root.as_os_str().to_os_string(),
    ];
    if *subscription_profile == SubscriptionProfile::Lean {
        command_args.push(OsString::from("--ignore-user-config"));
    }
    command_args.extend(args.iter().map(OsString::from));
    Ok(CommandSpec {
        program: executable.clone(),
        args: command_args,
    })
}

pub fn build_external_command(config: &ProviderConfig) -> Result<CommandSpec, RuntimeError> {
    validate_provider_config(config)?;
    let ProviderConfig::Command { executable, args } = config else {
        return Err(RuntimeError::InvalidManifest(
            "command adapter requires a command provider".into(),
        ));
    };
    Ok(CommandSpec {
        program: executable.clone(),
        args: args.iter().map(OsString::from).collect(),
    })
}

fn provider_input(
    config: &ProviderConfig,
    prompt: &CompiledPrompt,
    _project_root: &Path,
    _run_dir: &Path,
) -> Vec<u8> {
    match config {
        ProviderConfig::Claude { .. } => prompt.dynamic.clone(),
        ProviderConfig::Codex { .. } => prompt.codex_bootstrap.clone(),
        ProviderConfig::Command { .. } => prompt.bytes.clone(),
        ProviderConfig::OpenAiApi { .. } | ProviderConfig::AnthropicApi { .. } => {
            prompt.dynamic.clone()
        }
    }
}

pub fn parse_claude_output(bytes: &[u8]) -> Result<(StepEnvelope, ProviderUsage), ProviderFailure> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ProviderFailure::InvalidOutput)?;
    let envelope = match value.get("structured_output") {
        Some(output) if output.is_object() => serde_json::from_value(output.clone()),
        _ => match value.get("result").and_then(Value::as_str) {
            Some(result) => serde_json::from_str(result),
            None => serde_json::from_value(value.clone()),
        },
    }
    .map_err(|_| ProviderFailure::InvalidOutput)?;
    Ok((
        envelope,
        usage_from_value(value.get("usage"), InputTokenSemantics::UncachedOnly),
    ))
}

fn provider_result(envelope: StepEnvelope, usage: ProviderUsage, raw: RawOutput) -> ProviderResult {
    ProviderResult {
        envelope,
        usage,
        exit_code: raw.exit_code,
        response_bytes: raw.stdout.len(),
        duration_ms: raw.duration_ms,
    }
}

fn reject_overrides(args: &[String], protected: &[&str]) -> Result<(), RuntimeError> {
    for arg in args {
        if protected.iter().any(|value| {
            arg.eq_ignore_ascii_case(value)
                || arg
                    .to_ascii_lowercase()
                    .starts_with(&format!("{}=", value.to_ascii_lowercase()))
        }) {
            return Err(RuntimeError::InvalidManifest(format!(
                "provider argument overrides required runtime flag: {arg}"
            )));
        }
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct RawOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
    pub duration_ms: u64,
}

fn run_process(
    spec: &CommandSpec,
    prompt: &[u8],
    project_root: &Path,
    timeout: Duration,
) -> Result<RawOutput, ProviderFailure> {
    let output = capture_process(spec, prompt, project_root, timeout)?;
    if output.exit_code != 0 {
        return Err(ProviderFailure::Exit(output.exit_code));
    }
    Ok(output)
}

pub(crate) fn capture_process(
    spec: &CommandSpec,
    prompt: &[u8],
    project_root: &Path,
    timeout: Duration,
) -> Result<RawOutput, ProviderFailure> {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(project_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProviderFailure::MissingExecutable
        } else {
            ProviderFailure::Spawn
        }
    })?;
    let mut stdin = child.stdin.take().ok_or(ProviderFailure::Stdin)?;
    let prompt = prompt.to_vec();
    let writer = thread::spawn(move || stdin.write_all(&prompt));
    let stdout = child.stdout.take().ok_or(ProviderFailure::Spawn)?;
    let stderr = child.stderr.take().ok_or(ProviderFailure::Spawn)?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout));
    let stderr_reader = thread::spawn(move || read_bounded(stderr));

    let status = match child
        .wait_timeout(timeout)
        .map_err(|_| ProviderFailure::Spawn)?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = writer.join();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(ProviderFailure::Timeout);
        }
    };
    if writer.join().map_err(|_| ProviderFailure::Stdin)?.is_err() {
        return Err(ProviderFailure::Stdin);
    }
    let stdout = stdout_reader.join().map_err(|_| ProviderFailure::Spawn)?;
    let stderr = stderr_reader.join().map_err(|_| ProviderFailure::Spawn)?;
    if stdout.overflow
        || stderr.overflow
        || stdout.bytes.len().saturating_add(stderr.bytes.len()) > MAX_PROVIDER_OUTPUT_BYTES
    {
        return Err(ProviderFailure::OutputTooLarge);
    }
    let exit_code = status.code().unwrap_or(-1);
    Ok(RawOutput {
        stdout: stdout.bytes,
        stderr: stderr.bytes,
        exit_code,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

#[derive(Debug)]
struct BoundedBytes {
    bytes: Vec<u8>,
    overflow: bool,
}

fn read_bounded(mut reader: impl Read) -> BoundedBytes {
    let mut bytes = Vec::with_capacity(MAX_PROVIDER_OUTPUT_BYTES.min(64 * 1024));
    let mut overflow = false;
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let remaining = MAX_PROVIDER_OUTPUT_BYTES.saturating_sub(bytes.len());
                bytes.extend_from_slice(&buffer[..count.min(remaining)]);
                overflow |= count > remaining;
            }
            Err(_) => break,
        }
    }
    BoundedBytes { bytes, overflow }
}

fn read_bounded_file(path: &Path) -> Result<Vec<u8>, ProviderFailure> {
    let metadata = fs::metadata(path).map_err(|_| ProviderFailure::InvalidOutput)?;
    if metadata.len() > MAX_PROVIDER_OUTPUT_BYTES as u64 {
        return Err(ProviderFailure::OutputTooLarge);
    }
    fs::read(path).map_err(|_| ProviderFailure::InvalidOutput)
}

fn parse_codex_usage(bytes: &[u8]) -> ProviderUsage {
    bytes
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .filter_map(|value| value.get("usage").cloned())
        .map(|usage| usage_from_value(Some(&usage), InputTokenSemantics::TotalIncludesCache))
        .next_back()
        .unwrap_or_default()
}

pub(crate) fn usage_from_value(
    value: Option<&Value>,
    input_token_semantics: InputTokenSemantics,
) -> ProviderUsage {
    let get = |name: &str| {
        value
            .and_then(|usage| usage.get(name))
            .and_then(Value::as_u64)
    };
    let details = value.and_then(|usage| {
        usage
            .get("input_tokens_details")
            .or_else(|| usage.get("prompt_tokens_details"))
    });
    let get_detail = |name: &str| {
        details
            .and_then(|value| value.get(name))
            .and_then(Value::as_u64)
    };
    let cache_creation = value.and_then(|usage| usage.get("cache_creation"));
    let get_cache_creation = |name: &str| {
        cache_creation
            .and_then(|value| value.get(name))
            .and_then(Value::as_u64)
    };
    let input_tokens = get("input_tokens").or_else(|| get("prompt_tokens"));
    let cache_read_input_tokens = get("cache_read_input_tokens")
        .or_else(|| get("cached_input_tokens"))
        .or_else(|| get_detail("cached_tokens"));
    let cache_write_input_tokens = get("cache_creation_input_tokens")
        .or_else(|| get("cache_write_input_tokens"))
        .or_else(|| get_detail("cache_write_tokens"));
    let uncached_input_tokens = match input_token_semantics {
        InputTokenSemantics::UncachedOnly => input_tokens,
        InputTokenSemantics::TotalIncludesCache => input_tokens.and_then(|total| {
            let read = cache_read_input_tokens?;
            let write = cache_write_input_tokens.unwrap_or(0);
            total.checked_sub(read)?.checked_sub(write)
        }),
        InputTokenSemantics::Unknown => None,
    };
    ProviderUsage {
        input_token_semantics,
        input_tokens,
        uncached_input_tokens,
        cache_read_input_tokens,
        cache_write_input_tokens,
        cache_write_5m_input_tokens: get_cache_creation("ephemeral_5m_input_tokens"),
        cache_write_1h_input_tokens: get_cache_creation("ephemeral_1h_input_tokens"),
        output_tokens: get("output_tokens"),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    use tempfile::TempDir;

    use crate::runtime::model::{ProviderConfig, RuntimeError, SubscriptionProfile};
    use crate::runtime::prompt::CompiledPrompt;

    use super::{
        build_claude_command, build_codex_command, parse_claude_output, parse_codex_usage,
        provider_input, ProcessProvider, Provider, ProviderFailure,
    };

    fn python() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("python")
        } else {
            PathBuf::from("python3")
        }
    }

    #[cfg(unix)]
    #[test]
    fn probes_are_noop_unique_and_disable_integrations_for_standard_profile() {
        use std::os::unix::fs::PermissionsExt;
        let project = TempDir::new().unwrap();
        let driver = project.path().join("mock-cli");
        fs::write(&driver, r#"#!/usr/bin/env python3
import sys,json,pathlib
request=sys.stdin.read()
with pathlib.Path('captures.jsonl').open('a') as f:
    f.write(json.dumps({'args':sys.argv[1:],'input':request})+'\n')
envelope={'schema_version':1,'base_revision':0,'outcome':'continue','summary':'probe','delta':{}}
usage={'input_tokens':100,'cache_read_input_tokens':80,'cache_creation_input_tokens':0,'output_tokens':2}
if '--output-last-message' in sys.argv:
    pathlib.Path(sys.argv[sys.argv.index('--output-last-message')+1]).write_text(json.dumps(envelope))
print(json.dumps({'structured_output':envelope,'usage':usage}))
"#).unwrap();
        fs::set_permissions(&driver, fs::Permissions::from_mode(0o755)).unwrap();
        for codex in [false, true] {
            let config = if codex {
                ProviderConfig::Codex {
                    executable: driver.clone(),
                    args: vec![],
                    subscription_profile: SubscriptionProfile::Standard,
                }
            } else {
                ProviderConfig::Claude {
                    executable: driver.clone(),
                    args: vec![],
                    subscription_profile: SubscriptionProfile::Standard,
                }
            };
            ProcessProvider
                .probe(&config, &prompt(), project.path(), Duration::from_secs(3))
                .unwrap();
        }
        let raw = fs::read_to_string(project.path().join("captures.jsonl")).unwrap();
        let captures: Vec<serde_json::Value> = raw
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(captures.len(), 4);
        for capture in &captures {
            let input = capture["input"].as_str().unwrap();
            assert!(input.contains("CACHE_PROBE"));
            assert!(input.contains("Do not execute the workflow"));
            assert!(!input.contains("perform one bounded increment"));
        }
        assert_ne!(captures[0]["input"], captures[1]["input"]);
        assert_ne!(captures[2]["input"], captures[3]["input"]);
        for capture in &captures[..2] {
            let args = capture["args"].as_array().unwrap();
            assert!(args.contains(&serde_json::json!("--strict-mcp-config")));
            assert!(args.contains(&serde_json::json!("--no-chrome")));
            assert!(args.contains(&serde_json::json!("--disable-slash-commands")));
            let tools = args.iter().position(|v| v == "--tools").unwrap();
            assert_eq!(args[tools + 1], "");
        }
        for capture in &captures[2..] {
            let args = capture["args"].as_array().unwrap();
            assert!(args.contains(&serde_json::json!("--ignore-user-config")));
            assert!(args.contains(&serde_json::json!("read-only")));
        }
    }

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_driver.py")
    }

    fn prompt() -> CompiledPrompt {
        CompiledPrompt {
            bytes: b"bounded prompt".to_vec(),
            stable: b"stable".to_vec(),
            dynamic: b"dynamic".to_vec(),
            codex_bootstrap: b"Load `.codeunlimited/runs/feature-x/provider-instructions.md` first, `.codeunlimited/runs/feature-x/state.json` second, and `.codeunlimited/runs/feature-x/observation.txt` third.\n".to_vec(),
            instructions_path: PathBuf::from("/tmp/provider-instructions.md"),
            stable_bytes: 7,
            dynamic_bytes: 7,
            stable_sha256: "11".repeat(32),
            dynamic_sha256: "33".repeat(32),
            prompt_sha256: "22".repeat(32),
        }
    }

    fn command_config(args: &[&str]) -> ProviderConfig {
        let mut all = vec![fixture().to_string_lossy().into_owned()];
        all.extend(args.iter().map(|value| (*value).to_string()));
        ProviderConfig::Command {
            executable: python(),
            args: all,
        }
    }

    #[test]
    fn command_provider_receives_exact_prompt_and_parses_envelope() {
        let project = TempDir::new().expect("project");
        let capture = project.path().join("captured.txt");
        let config = command_config(&["--mode", "success", "--capture", capture.to_str().unwrap()]);

        let result = ProcessProvider
            .run(&config, &prompt(), project.path(), Duration::from_secs(2))
            .expect("fixture result");

        assert_eq!(result.envelope.base_revision, 0);
        assert_eq!(fs::read(capture).unwrap(), b"bounded prompt");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.usage.input_tokens, None);
    }

    #[test]
    fn timeout_and_oversized_output_fail_without_body_disclosure() {
        let project = TempDir::new().expect("project");
        let timeout = ProcessProvider
            .run(
                &command_config(&["--mode", "sleep"]),
                &prompt(),
                project.path(),
                Duration::from_millis(30),
            )
            .unwrap_err();
        assert_eq!(timeout, ProviderFailure::Timeout);

        let oversized = ProcessProvider
            .run(
                &command_config(&["--mode", "oversized"]),
                &prompt(),
                project.path(),
                Duration::from_secs(2),
            )
            .unwrap_err();
        assert_eq!(oversized, ProviderFailure::OutputTooLarge);
        assert!(!oversized.to_string().contains("xxxxxxxxxxxxxxxx"));
    }

    #[test]
    fn combined_stdout_and_stderr_share_one_output_budget() {
        let project = TempDir::new().expect("project");
        let config = ProviderConfig::Command {
            executable: python(),
            args: vec![
                "-c".into(),
                "import sys; sys.stdout.write('x'*614400); sys.stderr.write('y'*614400)".into(),
            ],
        };

        let error = ProcessProvider
            .run(&config, &prompt(), project.path(), Duration::from_secs(3))
            .unwrap_err();

        assert_eq!(error, ProviderFailure::OutputTooLarge);
    }

    #[test]
    fn timeout_kills_the_child_before_it_can_mutate_later() {
        let project = TempDir::new().expect("project");
        let marker = project.path().join("late-marker");
        let config = ProviderConfig::Command {
            executable: python(),
            args: vec![
                "-c".into(),
                "import pathlib,sys,time; time.sleep(.2); pathlib.Path(sys.argv[1]).write_text('late')"
                    .into(),
                marker.to_string_lossy().into_owned(),
            ],
        };

        assert_eq!(
            ProcessProvider
                .run(
                    &config,
                    &prompt(),
                    project.path(),
                    Duration::from_millis(30)
                )
                .unwrap_err(),
            ProviderFailure::Timeout
        );
        thread::sleep(Duration::from_millis(300));
        assert!(!marker.exists());
    }

    #[cfg(unix)]
    #[test]
    fn executable_paths_with_spaces_and_hyphen_args_are_preserved() {
        use std::os::unix::fs::symlink;

        let project = TempDir::new().expect("project");
        let resolved = Command::new("which")
            .arg(python())
            .output()
            .expect("locate python");
        assert!(resolved.status.success());
        let executable = project.path().join("provider executable with spaces");
        symlink(
            String::from_utf8(resolved.stdout)
                .expect("UTF-8 executable path")
                .trim(),
            &executable,
        )
        .expect("spaced executable symlink");
        let config = ProviderConfig::Command {
            executable,
            args: vec![
                fixture().to_string_lossy().into_owned(),
                "--mode".into(),
                "success".into(),
                "--outcome".into(),
                "continue".into(),
            ],
        };

        let result = ProcessProvider
            .run(&config, &prompt(), project.path(), Duration::from_secs(2))
            .expect("provider through spaced executable path");
        assert_eq!(result.envelope.base_revision, 0);
    }

    #[test]
    fn provider_exit_does_not_expose_stderr() {
        let project = TempDir::new().expect("project");
        let error = ProcessProvider
            .run(
                &command_config(&["--mode", "failure"]),
                &prompt(),
                project.path(),
                Duration::from_secs(2),
            )
            .unwrap_err();
        assert_eq!(error, ProviderFailure::Exit(7));
        assert!(!error.to_string().contains("PRIVATE"));
    }

    #[test]
    fn invalid_json_is_a_content_free_error() {
        let project = TempDir::new().expect("project");
        let error = ProcessProvider
            .run(
                &command_config(&["--mode", "invalid"]),
                &prompt(),
                project.path(),
                Duration::from_secs(2),
            )
            .unwrap_err();
        assert_eq!(error, ProviderFailure::InvalidOutput);
        assert!(!error.to_string().contains("not-json"));
    }

    #[test]
    fn built_in_commands_enforce_ephemeral_flags() {
        let claude = ProviderConfig::Claude {
            executable: PathBuf::from("claude"),
            args: vec!["--model".into(), "sonnet".into()],
            subscription_profile: SubscriptionProfile::Standard,
        };
        let claude = build_claude_command(
            &claude,
            Path::new("/tmp/provider-instructions.md"),
            Path::new("/tmp/empty-mcp.json"),
        )
        .expect("claude command");
        assert_eq!(claude.program, PathBuf::from("claude"));
        assert!(contains_pair(&claude.args, "--output-format", "json"));
        assert!(claude.args.contains(&OsString::from("--print")));
        assert!(claude
            .args
            .contains(&OsString::from("--no-session-persistence")));
        assert!(claude
            .args
            .contains(&OsString::from("--exclude-dynamic-system-prompt-sections")));
        assert!(contains_pair(
            &claude.args,
            "--append-system-prompt-file",
            "/tmp/provider-instructions.md"
        ));
        assert!(!claude.args.contains(&OsString::from("--strict-mcp-config")));

        let codex = ProviderConfig::Codex {
            executable: PathBuf::from("codex"),
            args: vec!["--model".into(), "gpt-test".into()],
            subscription_profile: SubscriptionProfile::Lean,
        };
        let codex = build_codex_command(
            &codex,
            Path::new("/tmp/project"),
            Path::new("/tmp/schema.json"),
            Path::new("/tmp/output.json"),
        )
        .expect("codex command");
        assert_eq!(codex.args[0], "exec");
        assert!(codex.args.contains(&OsString::from("--ephemeral")));
        assert!(codex.args.contains(&OsString::from("--ignore-user-config")));
        assert!(contains_pair(
            &codex.args,
            "--output-schema",
            "/tmp/schema.json"
        ));
        assert!(contains_pair(
            &codex.args,
            "--output-last-message",
            "/tmp/output.json"
        ));
    }

    #[test]
    fn subscription_inputs_are_transport_specific_and_revision_stable() {
        let project = Path::new("/tmp/project");
        let run = project.join(".codeunlimited/runs/feature-x");
        let claude = ProviderConfig::Claude {
            executable: "claude".into(),
            args: vec![],
            subscription_profile: SubscriptionProfile::Standard,
        };
        let codex = ProviderConfig::Codex {
            executable: "codex".into(),
            args: vec![],
            subscription_profile: SubscriptionProfile::Standard,
        };
        let command = command_config(&["--mode", "success"]);

        assert_eq!(
            provider_input(&claude, &prompt(), project, &run),
            b"dynamic"
        );
        assert_eq!(
            provider_input(&command, &prompt(), project, &run),
            b"bounded prompt"
        );
        let bootstrap =
            String::from_utf8(provider_input(&codex, &prompt(), project, &run)).unwrap();
        assert!(bootstrap.contains("provider-instructions.md` first"));
        assert!(bootstrap.contains("state.json` second"));
        assert!(bootstrap.contains("observation.txt` third"));
        assert!(!bootstrap.contains("stable"));
        assert!(!bootstrap.contains("dynamic"));
    }

    #[test]
    fn lean_claude_profile_restricts_dynamic_integrations_without_bare_mode() {
        let config = ProviderConfig::Claude {
            executable: "claude".into(),
            args: vec![],
            subscription_profile: SubscriptionProfile::Lean,
        };
        let spec = build_claude_command(
            &config,
            Path::new("/tmp/provider-instructions.md"),
            Path::new("/tmp/empty-mcp.json"),
        )
        .unwrap();

        assert!(spec
            .args
            .contains(&OsString::from("--disable-slash-commands")));
        assert!(spec.args.contains(&OsString::from("--no-chrome")));
        assert!(spec.args.contains(&OsString::from("--strict-mcp-config")));
        assert!(contains_pair(
            &spec.args,
            "--mcp-config",
            "/tmp/empty-mcp.json"
        ));
        assert!(contains_pair(
            &spec.args,
            "--tools",
            "Bash,Edit,Read,Write,Glob,Grep"
        ));
        assert!(!spec.args.contains(&OsString::from("--bare")));
    }

    #[test]
    fn continuation_secret_and_required_override_args_are_rejected() {
        for args in [
            vec!["--resume".into(), "id".into()],
            vec!["--api-key=secret".into()],
            vec!["--output-format".into(), "text".into()],
        ] {
            let config = ProviderConfig::Claude {
                executable: PathBuf::from("claude"),
                args,
                subscription_profile: SubscriptionProfile::Standard,
            };
            assert!(matches!(
                build_claude_command(
                    &config,
                    Path::new("/tmp/provider-instructions.md"),
                    Path::new("/tmp/empty-mcp.json")
                ),
                Err(RuntimeError::ContinuationArgument(_)
                    | RuntimeError::SecretArgument
                    | RuntimeError::InvalidManifest(_))
            ));
        }
    }

    #[test]
    fn claude_structured_output_and_usage_are_parsed_without_result_text() {
        let raw = serde_json::json!({
            "type": "result",
            "result": "PRIVATE RESULT TEXT",
            "structured_output": {
                "schema_version": 1,
                "base_revision": 2,
                "outcome": "continue",
                "summary": "bounded",
                "delta": {}
            },
            "usage": {
                "input_tokens": 101,
                "cache_read_input_tokens": 70,
                "cache_creation_input_tokens": 30,
                "cache_creation": {
                    "ephemeral_5m_input_tokens": 20,
                    "ephemeral_1h_input_tokens": 10
                },
                "output_tokens": 9
            }
        });
        let parsed =
            parse_claude_output(&serde_json::to_vec(&raw).unwrap()).expect("structured response");
        assert_eq!(parsed.0.base_revision, 2);
        assert_eq!(parsed.1.input_tokens, Some(101));
        assert_eq!(parsed.1.uncached_input_tokens, Some(101));
        assert_eq!(parsed.1.cache_read_input_tokens, Some(70));
        assert_eq!(parsed.1.cache_write_input_tokens, Some(30));
        assert_eq!(parsed.1.cache_write_5m_input_tokens, Some(20));
        assert_eq!(parsed.1.cache_write_1h_input_tokens, Some(10));
        assert_eq!(parsed.1.transported_input_tokens(), Some(201));
        assert_eq!(parsed.1.cache_read_ratio_basis_points(), Some(3482));
        assert_eq!(parsed.1.output_tokens, Some(9));
    }

    #[test]
    fn codex_usage_reads_current_nested_cache_counters() {
        let raw = br#"{"type":"turn.completed","usage":{"input_tokens":15000,"input_tokens_details":{"cached_tokens":12000,"cache_write_tokens":3000},"output_tokens":100}}
"#;

        let usage = parse_codex_usage(raw);

        assert_eq!(usage.input_tokens, Some(15000));
        assert_eq!(usage.uncached_input_tokens, Some(0));
        assert_eq!(usage.cache_read_input_tokens, Some(12000));
        assert_eq!(usage.cache_write_input_tokens, Some(3000));
        assert_eq!(usage.transported_input_tokens(), Some(15000));
        assert_eq!(usage.cache_read_ratio_basis_points(), Some(8000));
        assert_eq!(usage.output_tokens, Some(100));
    }

    #[test]
    fn missing_usage_stays_unknown_instead_of_becoming_zero() {
        let usage = super::usage_from_value(None, super::InputTokenSemantics::Unknown);

        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.uncached_input_tokens, None);
        assert_eq!(usage.transported_input_tokens(), None);
        assert_eq!(usage.cache_read_ratio_basis_points(), None);
    }

    #[test]
    fn cache_probe_verdict_uses_only_second_reported_cache_read() {
        let sample = |cached| super::ProviderProbeSample {
            usage: super::ProviderUsage {
                input_token_semantics: super::InputTokenSemantics::TotalIncludesCache,
                input_tokens: Some(100),
                cache_read_input_tokens: cached,
                ..Default::default()
            },
            duration_ms: 1,
        };

        assert_eq!(
            super::ProviderProbeResult::new(sample(Some(0)), sample(Some(80))).cache_hit_reported,
            Some(true)
        );
        assert_eq!(
            super::ProviderProbeResult::new(sample(Some(0)), sample(Some(0))).cache_hit_reported,
            Some(false)
        );
        assert_eq!(
            super::ProviderProbeResult::new(sample(None), sample(None)).cache_hit_reported,
            None
        );
    }

    fn contains_pair(args: &[OsString], key: &str, value: &str) -> bool {
        args.windows(2)
            .any(|pair| pair[0] == key && pair[1] == value)
    }
}
