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
use wait_timeout::ChildExt;

use super::model::{ProviderConfig, RuntimeError, StepEnvelope, MAX_PROVIDER_OUTPUT_BYTES};
use super::prompt::{CompiledPrompt, STEP_ENVELOPE_SCHEMA_JSON};
use super::validate::validate_provider_config;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderUsage {
    pub input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_write_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderResult {
    pub envelope: StepEnvelope,
    pub usage: ProviderUsage,
    pub exit_code: i32,
    pub response_bytes: usize,
    pub duration_ms: u64,
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
    InvalidConfiguration(RuntimeError),
    TemporaryFile,
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
            Self::InvalidOutput => write!(f, "provider returned an invalid structured response"),
            Self::InvalidConfiguration(error) => {
                write!(f, "invalid provider configuration: {error}")
            }
            Self::TemporaryFile => write!(f, "provider temporary files could not be prepared"),
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
                let raw = run_process(&spec, &prompt.bytes, project_root, timeout)?;
                let envelope = serde_json::from_slice(&raw.stdout)
                    .map_err(|_| ProviderFailure::InvalidOutput)?;
                Ok(provider_result(envelope, ProviderUsage::default(), raw))
            }
            ProviderConfig::Claude { .. } => {
                let spec = build_claude_command(config)?;
                let raw = run_process(&spec, &prompt.bytes, project_root, timeout)?;
                let (envelope, usage) = parse_claude_output(&raw.stdout)?;
                Ok(provider_result(envelope, usage, raw))
            }
            ProviderConfig::Codex { .. } => {
                let temporary = tempfile::tempdir().map_err(|_| ProviderFailure::TemporaryFile)?;
                let schema = temporary.path().join("step-envelope.schema.json");
                let output = temporary.path().join("last-message.json");
                fs::write(&schema, STEP_ENVELOPE_SCHEMA_JSON)
                    .map_err(|_| ProviderFailure::TemporaryFile)?;
                let spec = build_codex_command(config, project_root, &schema, &output)?;
                let raw = run_process(&spec, &prompt.bytes, project_root, timeout)?;
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
        }
    }
}

pub fn build_claude_command(config: &ProviderConfig) -> Result<CommandSpec, RuntimeError> {
    validate_provider_config(config)?;
    let ProviderConfig::Claude { executable, args } = config else {
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
        ],
    )?;
    let mut command_args = vec![
        OsString::from("--print"),
        OsString::from("--no-session-persistence"),
        OsString::from("--output-format"),
        OsString::from("json"),
        OsString::from("--json-schema"),
        OsString::from(STEP_ENVELOPE_SCHEMA_JSON),
        OsString::from("--exclude-dynamic-system-prompt-sections"),
    ];
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
    let ProviderConfig::Codex { executable, args } = config else {
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
    Ok((envelope, usage_from_value(value.get("usage"))))
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
        .map(|usage| usage_from_value(Some(&usage)))
        .next_back()
        .unwrap_or_default()
}

fn usage_from_value(value: Option<&Value>) -> ProviderUsage {
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
    ProviderUsage {
        input_tokens: get("input_tokens").or_else(|| get("prompt_tokens")),
        cache_read_input_tokens: get("cache_read_input_tokens")
            .or_else(|| get("cached_input_tokens"))
            .or_else(|| get_detail("cached_tokens")),
        cache_write_input_tokens: get("cache_creation_input_tokens")
            .or_else(|| get("cache_write_input_tokens"))
            .or_else(|| get_detail("cache_write_tokens")),
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

    use crate::runtime::model::{ProviderConfig, RuntimeError};
    use crate::runtime::prompt::CompiledPrompt;

    use super::{
        build_claude_command, build_codex_command, parse_claude_output, parse_codex_usage,
        ProcessProvider, Provider, ProviderFailure,
    };

    fn python() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("python")
        } else {
            PathBuf::from("python3")
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
        };
        let claude = build_claude_command(&claude).expect("claude command");
        assert_eq!(claude.program, PathBuf::from("claude"));
        assert!(contains_pair(&claude.args, "--output-format", "json"));
        assert!(claude.args.contains(&OsString::from("--print")));
        assert!(claude
            .args
            .contains(&OsString::from("--no-session-persistence")));
        assert!(claude
            .args
            .contains(&OsString::from("--exclude-dynamic-system-prompt-sections")));

        let codex = ProviderConfig::Codex {
            executable: PathBuf::from("codex"),
            args: vec!["--model".into(), "gpt-test".into()],
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
    fn continuation_secret_and_required_override_args_are_rejected() {
        for args in [
            vec!["--resume".into(), "id".into()],
            vec!["--api-key=secret".into()],
            vec!["--output-format".into(), "text".into()],
        ] {
            let config = ProviderConfig::Claude {
                executable: PathBuf::from("claude"),
                args,
            };
            assert!(matches!(
                build_claude_command(&config),
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
                "output_tokens": 9
            }
        });
        let parsed =
            parse_claude_output(&serde_json::to_vec(&raw).unwrap()).expect("structured response");
        assert_eq!(parsed.0.base_revision, 2);
        assert_eq!(parsed.1.input_tokens, Some(101));
        assert_eq!(parsed.1.cache_read_input_tokens, Some(70));
        assert_eq!(parsed.1.output_tokens, Some(9));
    }

    #[test]
    fn codex_usage_reads_current_nested_cache_counters() {
        let raw = br#"{"type":"turn.completed","usage":{"input_tokens":15000,"input_tokens_details":{"cached_tokens":12000,"cache_write_tokens":3000},"output_tokens":100}}
"#;

        let usage = parse_codex_usage(raw);

        assert_eq!(usage.input_tokens, Some(15000));
        assert_eq!(usage.cache_read_input_tokens, Some(12000));
        assert_eq!(usage.cache_write_input_tokens, Some(3000));
        assert_eq!(usage.output_tokens, Some(100));
    }

    fn contains_pair(args: &[OsString], key: &str, value: &str) -> bool {
        args.windows(2)
            .any(|pair| pair[0] == key && pair[1] == value)
    }
}
