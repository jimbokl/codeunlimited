use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const RUNTIME_SCHEMA_VERSION: u64 = 1;
pub const DEFAULT_WORKFLOW_BUDGET_BYTES: usize = 24 * 1024;
pub const DEFAULT_STATE_BUDGET_BYTES: usize = 16 * 1024;
pub const DEFAULT_OBSERVATION_BUDGET_BYTES: usize = 4 * 1024;
pub const DEFAULT_PROMPT_BUDGET_BYTES: usize = 48 * 1024;
pub const MAX_WORKFLOW_BUDGET_BYTES: usize = 128 * 1024;
pub const MAX_STATE_BUDGET_BYTES: usize = 64 * 1024;
pub const MAX_OBSERVATION_BUDGET_BYTES: usize = 32 * 1024;
pub const MAX_PROMPT_BUDGET_BYTES: usize = 256 * 1024;
pub const MAX_PROVIDER_OUTPUT_BYTES: usize = 1024 * 1024;
pub const DEFAULT_PROVIDER_TIMEOUT_SECONDS: u64 = 30 * 60;
pub const MAX_PROVIDER_TIMEOUT_SECONDS: u64 = 4 * 60 * 60;
pub const DEFAULT_MAX_STEPS: u64 = 100;
pub const DEFAULT_MAX_ATTEMPTS_PER_REVISION: u64 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u64,
    pub run_name: String,
    pub project_root: PathBuf,
    pub created_unix: i64,
    pub workflow_source: PathBuf,
    pub workflow_sha256: String,
    pub objective: String,
    pub provider: ProviderConfig,
    pub max_steps: u64,
    pub max_attempts_per_revision: u64,
    pub provider_timeout_seconds: u64,
    pub workflow_budget_bytes: usize,
    pub state_budget_bytes: usize,
    pub observation_budget_bytes: usize,
    pub prompt_budget_bytes: usize,
    pub verification_command: Option<VerificationCommand>,
    pub verify_every_step: bool,
    pub allow_unverified_completion: bool,
}

impl Manifest {
    #[cfg(test)]
    pub fn for_test(
        run_name: &str,
        project_root: PathBuf,
        objective: &str,
        provider: ProviderConfig,
    ) -> Self {
        Self {
            schema_version: RUNTIME_SCHEMA_VERSION,
            run_name: run_name.to_string(),
            project_root,
            created_unix: 0,
            workflow_source: PathBuf::from("workflow.md"),
            workflow_sha256: "00".repeat(32),
            objective: objective.to_string(),
            provider,
            max_steps: DEFAULT_MAX_STEPS,
            max_attempts_per_revision: DEFAULT_MAX_ATTEMPTS_PER_REVISION,
            provider_timeout_seconds: DEFAULT_PROVIDER_TIMEOUT_SECONDS,
            workflow_budget_bytes: DEFAULT_WORKFLOW_BUDGET_BYTES,
            state_budget_bytes: DEFAULT_STATE_BUDGET_BYTES,
            observation_budget_bytes: DEFAULT_OBSERVATION_BUDGET_BYTES,
            prompt_budget_bytes: DEFAULT_PROMPT_BUDGET_BYTES,
            verification_command: None,
            verify_every_step: false,
            allow_unverified_completion: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum ProviderConfig {
    Claude {
        executable: PathBuf,
        args: Vec<String>,
    },
    Codex {
        executable: PathBuf,
        args: Vec<String>,
    },
    Command {
        executable: PathBuf,
        args: Vec<String>,
    },
}

impl ProviderConfig {
    pub fn executable(&self) -> &PathBuf {
        match self {
            Self::Claude { executable, .. }
            | Self::Codex { executable, .. }
            | Self::Command { executable, .. } => executable,
        }
    }

    pub fn args(&self) -> &[String] {
        match self {
            Self::Claude { args, .. } | Self::Codex { args, .. } | Self::Command { args, .. } => {
                args
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Complete,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkItem {
    pub id: String,
    pub task: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletedItem {
    pub id: String,
    pub result: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub id: String,
    pub decision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blocker {
    pub id: String,
    pub blocker: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckResult {
    pub revision: u64,
    pub program: String,
    pub args: Vec<String>,
    pub passed: bool,
    pub summary: String,
    pub workspace_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub path: String,
    pub purpose: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactCandidate {
    pub path: String,
    pub purpose: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodingState {
    pub schema_version: u64,
    pub revision: u64,
    pub status: RunStatus,
    pub focus: String,
    pub memory_summary: String,
    pub queue: Vec<WorkItem>,
    pub completed: Vec<CompletedItem>,
    pub decisions: Vec<Decision>,
    pub blockers: Vec<Blocker>,
    pub active_files: Vec<String>,
    pub checks: Vec<CheckResult>,
    pub artifacts: Vec<ArtifactRef>,
    pub archive_count: u64,
    pub archive_hash: Option<String>,
}

impl CodingState {
    pub fn initial() -> Self {
        Self {
            schema_version: RUNTIME_SCHEMA_VERSION,
            revision: 0,
            status: RunStatus::Running,
            focus: String::new(),
            memory_summary: String::new(),
            queue: Vec::new(),
            completed: Vec::new(),
            decisions: Vec::new(),
            blockers: Vec::new(),
            active_files: Vec::new(),
            checks: Vec::new(),
            artifacts: Vec::new(),
            archive_count: 0,
            archive_hash: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepOutcome {
    Continue,
    Complete,
    Blocked,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StateDelta {
    pub focus: Option<String>,
    pub memory_summary: Option<String>,
    pub queue_replace: Option<Vec<WorkItem>>,
    pub completed_add: Vec<CompletedItem>,
    pub decisions_add: Vec<Decision>,
    pub blockers_replace: Option<Vec<Blocker>>,
    pub active_files_replace: Option<Vec<String>>,
    pub artifacts_add: Vec<ArtifactCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepEnvelope {
    pub schema_version: u64,
    pub base_revision: u64,
    pub outcome: StepOutcome,
    pub summary: String,
    pub delta: StateDelta,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveBatch {
    pub completed: Vec<CompletedItem>,
    pub decisions: Vec<Decision>,
}

impl ArchiveBatch {
    pub fn is_empty(&self) -> bool {
        self.completed.is_empty() && self.decisions.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub state: CodingState,
    pub archive: ArchiveBatch,
    pub observation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    UnsupportedSchema {
        expected: u64,
        actual: u64,
    },
    InvalidRunName,
    InvalidManifest(String),
    InvalidState(String),
    FieldTooLarge {
        field: &'static str,
        actual: usize,
        allowed: usize,
    },
    TooManyItems {
        field: &'static str,
        actual: usize,
        allowed: usize,
    },
    DuplicateId(String),
    InvalidRelativePath(String),
    InvalidDigest(String),
    SecretArgument,
    ContinuationArgument(String),
    StaleRevision {
        expected: u64,
        actual: u64,
    },
    IncompleteQueue,
    VerificationRequired,
    BlockerRequired,
    ArtifactMismatch,
    SummaryRequiredForArchive,
    StateBudgetExceeded {
        actual: usize,
        allowed: usize,
    },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema { expected, actual } => {
                write!(
                    f,
                    "unsupported schema version {actual}; expected {expected}"
                )
            }
            Self::InvalidRunName => write!(f, "invalid run name"),
            Self::InvalidManifest(message) => write!(f, "invalid manifest: {message}"),
            Self::InvalidState(message) => write!(f, "invalid state: {message}"),
            Self::FieldTooLarge {
                field,
                actual,
                allowed,
            } => {
                write!(f, "{field} is {actual} bytes; limit is {allowed}")
            }
            Self::TooManyItems {
                field,
                actual,
                allowed,
            } => {
                write!(f, "{field} has {actual} items; limit is {allowed}")
            }
            Self::DuplicateId(id) => write!(f, "duplicate id: {id}"),
            Self::InvalidRelativePath(path) => write!(f, "invalid project-relative path: {path}"),
            Self::InvalidDigest(field) => write!(f, "invalid sha256 digest in {field}"),
            Self::SecretArgument => write!(f, "secret-bearing provider arguments are not allowed"),
            Self::ContinuationArgument(arg) => {
                write!(f, "provider session continuation is not allowed: {arg}")
            }
            Self::StaleRevision { expected, actual } => {
                write!(f, "stale state revision {actual}; expected {expected}")
            }
            Self::IncompleteQueue => write!(f, "completion requires an empty work queue"),
            Self::VerificationRequired => write!(f, "completion requires runtime verification"),
            Self::BlockerRequired => write!(f, "blocked outcome requires a blocker"),
            Self::ArtifactMismatch => write!(f, "resolved artifacts do not match the state delta"),
            Self::SummaryRequiredForArchive => {
                write!(
                    f,
                    "memory_summary must change before state entries are archived"
                )
            }
            Self::StateBudgetExceeded { actual, allowed } => {
                write!(f, "state is {actual} bytes; limit is {allowed}")
            }
        }
    }
}

impl std::error::Error for RuntimeError {}
