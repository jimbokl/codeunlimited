use std::collections::HashSet;
use std::path::{Component, Path};

use sha2::{Digest, Sha256};

use super::model::{
    ArchiveBatch, ArtifactRef, CheckResult, CodingState, Manifest, ProviderConfig, RunStatus,
    RuntimeError, StepEnvelope, StepOutcome, Transition, MAX_OBSERVATION_BUDGET_BYTES,
    MAX_PROMPT_BUDGET_BYTES, MAX_PROVIDER_TIMEOUT_SECONDS, MAX_STATE_BUDGET_BYTES,
    MAX_WORKFLOW_BUDGET_BYTES, RUNTIME_SCHEMA_VERSION,
};

pub const MAX_FOCUS_BYTES: usize = 1024;
pub const MAX_MEMORY_SUMMARY_BYTES: usize = 4096;
pub const MAX_ITEM_BYTES: usize = 512;
pub const MAX_SUMMARY_BYTES: usize = 1024;
pub const MAX_OBJECTIVE_BYTES: usize = 8192;
pub const MAX_QUEUE_ITEMS: usize = 64;
pub const MAX_COMPLETED_ITEMS: usize = 32;
pub const MAX_DECISIONS: usize = 32;
pub const MAX_BLOCKERS: usize = 16;
pub const MAX_ACTIVE_FILES: usize = 32;
pub const MAX_CHECKS: usize = 16;
pub const MAX_ARTIFACTS: usize = 32;
pub const MAX_PROVIDER_ARGS: usize = 64;
pub const MAX_ARGUMENT_BYTES: usize = 4096;
pub const MAX_TOTAL_ATTEMPTS: u64 = 100;
pub const MAX_ATTEMPTS_PER_REVISION: u64 = 10;

pub fn validate_manifest(manifest: &Manifest) -> Result<(), RuntimeError> {
    validate_schema(manifest.schema_version)?;
    validate_safe_id(&manifest.run_name).map_err(|_| RuntimeError::InvalidRunName)?;
    validate_text("objective", &manifest.objective, MAX_OBJECTIVE_BYTES, false)?;
    if !manifest.project_root.is_absolute() {
        return Err(RuntimeError::InvalidManifest(
            "project_root must be absolute".into(),
        ));
    }
    if manifest.workflow_source.as_os_str().is_empty() {
        return Err(RuntimeError::InvalidManifest(
            "workflow_source must not be empty".into(),
        ));
    }
    validate_digest("workflow_sha256", &manifest.workflow_sha256)?;
    validate_provider(&manifest.provider)?;
    if manifest.max_steps == 0 || manifest.max_steps > MAX_TOTAL_ATTEMPTS {
        return Err(RuntimeError::InvalidManifest(format!(
            "max_steps must be between 1 and {MAX_TOTAL_ATTEMPTS}"
        )));
    }
    if manifest.max_attempts_per_revision == 0
        || manifest.max_attempts_per_revision > MAX_ATTEMPTS_PER_REVISION
    {
        return Err(RuntimeError::InvalidManifest(format!(
            "max_attempts_per_revision must be between 1 and {MAX_ATTEMPTS_PER_REVISION}"
        )));
    }
    if manifest.provider_timeout_seconds == 0
        || manifest.provider_timeout_seconds > MAX_PROVIDER_TIMEOUT_SECONDS
    {
        return Err(RuntimeError::InvalidManifest(format!(
            "provider_timeout_seconds must be between 1 and {MAX_PROVIDER_TIMEOUT_SECONDS}"
        )));
    }
    validate_budget(
        "workflow_budget_bytes",
        manifest.workflow_budget_bytes,
        MAX_WORKFLOW_BUDGET_BYTES,
    )?;
    validate_budget(
        "state_budget_bytes",
        manifest.state_budget_bytes,
        MAX_STATE_BUDGET_BYTES,
    )?;
    validate_budget(
        "observation_budget_bytes",
        manifest.observation_budget_bytes,
        MAX_OBSERVATION_BUDGET_BYTES,
    )?;
    validate_budget(
        "prompt_budget_bytes",
        manifest.prompt_budget_bytes,
        MAX_PROMPT_BUDGET_BYTES,
    )?;
    if let Some(command) = &manifest.verification_command {
        validate_executable(&command.program, "verification program")?;
        validate_args(&command.args, None)?;
    }
    Ok(())
}

pub fn validate_state(manifest: &Manifest, state: &CodingState) -> Result<(), RuntimeError> {
    validate_schema(state.schema_version)?;
    validate_text("focus", &state.focus, MAX_FOCUS_BYTES, true)?;
    validate_text(
        "memory_summary",
        &state.memory_summary,
        MAX_MEMORY_SUMMARY_BYTES,
        true,
    )?;
    validate_count("queue", state.queue.len(), MAX_QUEUE_ITEMS)?;
    validate_count("completed", state.completed.len(), MAX_COMPLETED_ITEMS)?;
    validate_count("decisions", state.decisions.len(), MAX_DECISIONS)?;
    validate_count("blockers", state.blockers.len(), MAX_BLOCKERS)?;
    validate_count("active_files", state.active_files.len(), MAX_ACTIVE_FILES)?;
    validate_count("checks", state.checks.len(), MAX_CHECKS)?;
    validate_count("artifacts", state.artifacts.len(), MAX_ARTIFACTS)?;

    let mut work_ids = HashSet::new();
    for item in &state.queue {
        validate_safe_id(&item.id)?;
        validate_text("queue.task", &item.task, MAX_ITEM_BYTES, false)?;
        insert_unique(&mut work_ids, &item.id)?;
    }
    for item in &state.completed {
        validate_safe_id(&item.id)?;
        validate_text("completed.result", &item.result, MAX_ITEM_BYTES, false)?;
        insert_unique(&mut work_ids, &item.id)?;
    }
    let mut decision_ids = HashSet::new();
    for item in &state.decisions {
        validate_safe_id(&item.id)?;
        validate_text("decision", &item.decision, MAX_ITEM_BYTES, false)?;
        insert_unique(&mut decision_ids, &item.id)?;
    }
    let mut blocker_ids = HashSet::new();
    for item in &state.blockers {
        validate_safe_id(&item.id)?;
        validate_text("blocker", &item.blocker, MAX_ITEM_BYTES, false)?;
        insert_unique(&mut blocker_ids, &item.id)?;
    }
    let mut active_paths = HashSet::new();
    for path in &state.active_files {
        validate_relative_path(path)?;
        if !active_paths.insert(path) {
            return Err(RuntimeError::InvalidState(format!(
                "duplicate active file: {path}"
            )));
        }
    }
    let mut artifact_paths = HashSet::new();
    for artifact in &state.artifacts {
        validate_relative_path(&artifact.path)?;
        validate_text("artifact.purpose", &artifact.purpose, MAX_ITEM_BYTES, false)?;
        validate_digest("artifact.sha256", &artifact.sha256)?;
        if !artifact_paths.insert(&artifact.path) {
            return Err(RuntimeError::InvalidState(format!(
                "duplicate artifact path: {}",
                artifact.path
            )));
        }
    }
    for check in &state.checks {
        validate_text("check.program", &check.program, MAX_ARGUMENT_BYTES, false)?;
        validate_args(&check.args, None)?;
        validate_text("check.summary", &check.summary, MAX_SUMMARY_BYTES, true)?;
        if check.revision > state.revision {
            return Err(RuntimeError::InvalidState(
                "check revision is newer than state".into(),
            ));
        }
        if let Some(hash) = &check.workspace_sha256 {
            validate_digest("check.workspace_sha256", hash)?;
        }
    }
    if let Some(hash) = &state.archive_hash {
        validate_digest("archive_hash", hash)?;
    }
    match state.status {
        RunStatus::Complete if !state.queue.is_empty() => {
            return Err(RuntimeError::IncompleteQueue)
        }
        RunStatus::Complete if !state.blockers.is_empty() => {
            return Err(RuntimeError::InvalidState(
                "complete state cannot contain blockers".into(),
            ))
        }
        RunStatus::Blocked if state.blockers.is_empty() => {
            return Err(RuntimeError::BlockerRequired)
        }
        _ => {}
    }

    let actual = serde_json::to_vec(state)
        .map_err(|error| RuntimeError::InvalidState(error.to_string()))?
        .len();
    if actual > manifest.state_budget_bytes {
        return Err(RuntimeError::StateBudgetExceeded {
            actual,
            allowed: manifest.state_budget_bytes,
        });
    }
    Ok(())
}

pub fn apply_delta(
    manifest: &Manifest,
    current: &CodingState,
    envelope: StepEnvelope,
    resolved_artifacts: &[ArtifactRef],
    check: Option<CheckResult>,
) -> Result<Transition, RuntimeError> {
    validate_manifest(manifest)?;
    validate_state(manifest, current)?;
    validate_schema(envelope.schema_version)?;
    if current.status != RunStatus::Running {
        return Err(RuntimeError::InvalidState(
            "terminal state cannot advance".into(),
        ));
    }
    if envelope.base_revision != current.revision {
        return Err(RuntimeError::StaleRevision {
            expected: current.revision,
            actual: envelope.base_revision,
        });
    }
    validate_text("summary", &envelope.summary, MAX_SUMMARY_BYTES, false)?;
    validate_artifact_resolution(&envelope, resolved_artifacts)?;

    let previous_summary = current.memory_summary.clone();
    let mut state = current.clone();
    if let Some(value) = envelope.delta.focus {
        state.focus = value;
    }
    if let Some(value) = envelope.delta.memory_summary {
        state.memory_summary = value;
    }
    if let Some(value) = envelope.delta.queue_replace {
        state.queue = value;
    }
    state.completed.extend(envelope.delta.completed_add);
    state.decisions.extend(envelope.delta.decisions_add);
    if let Some(value) = envelope.delta.blockers_replace {
        state.blockers = value;
    }
    if let Some(value) = envelope.delta.active_files_replace {
        state.active_files = value;
    }
    state.artifacts.extend_from_slice(resolved_artifacts);
    if let Some(check) = check {
        if check.revision != current.revision.saturating_add(1) {
            return Err(RuntimeError::InvalidState(
                "runtime check revision does not match transition".into(),
            ));
        }
        state.checks.push(check);
    }
    state.revision = current
        .revision
        .checked_add(1)
        .ok_or_else(|| RuntimeError::InvalidState("revision overflow".into()))?;

    let archive = compact_visible_history(&mut state, &previous_summary)?;
    if !archive.is_empty() {
        state.archive_count = state
            .archive_count
            .checked_add((archive.completed.len() + archive.decisions.len()) as u64)
            .ok_or_else(|| RuntimeError::InvalidState("archive count overflow".into()))?;
        state.archive_hash = Some(chain_archive_hash(state.archive_hash.as_deref(), &archive)?);
    }

    state.status = match envelope.outcome {
        StepOutcome::Continue => RunStatus::Running,
        StepOutcome::Blocked => {
            if state.blockers.is_empty() {
                return Err(RuntimeError::BlockerRequired);
            }
            RunStatus::Blocked
        }
        StepOutcome::Complete => {
            if !state.queue.is_empty() {
                return Err(RuntimeError::IncompleteQueue);
            }
            if !state.blockers.is_empty() {
                return Err(RuntimeError::InvalidState(
                    "completion requires empty blockers".into(),
                ));
            }
            match state.checks.last() {
                Some(check) if check.revision == state.revision && check.passed => {
                    RunStatus::Complete
                }
                Some(check) if check.revision == state.revision && !check.passed => {
                    RunStatus::Running
                }
                _ if manifest.allow_unverified_completion => RunStatus::Complete,
                _ => return Err(RuntimeError::VerificationRequired),
            }
        }
    };

    validate_state(manifest, &state)?;
    Ok(Transition {
        state,
        archive,
        observation: envelope.summary,
    })
}

fn compact_visible_history(
    state: &mut CodingState,
    previous_summary: &str,
) -> Result<ArchiveBatch, RuntimeError> {
    let completed_overflow = state.completed.len().saturating_sub(MAX_COMPLETED_ITEMS);
    let decisions_overflow = state.decisions.len().saturating_sub(MAX_DECISIONS);
    if completed_overflow == 0 && decisions_overflow == 0 {
        return Ok(ArchiveBatch::default());
    }
    if state.memory_summary == previous_summary {
        return Err(RuntimeError::SummaryRequiredForArchive);
    }
    Ok(ArchiveBatch {
        completed: state.completed.drain(..completed_overflow).collect(),
        decisions: state.decisions.drain(..decisions_overflow).collect(),
    })
}

fn chain_archive_hash(
    previous: Option<&str>,
    archive: &ArchiveBatch,
) -> Result<String, RuntimeError> {
    let mut hasher = Sha256::new();
    if let Some(previous) = previous {
        hasher.update(previous.as_bytes());
    }
    hasher.update(
        serde_json::to_vec(archive)
            .map_err(|error| RuntimeError::InvalidState(error.to_string()))?,
    );
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_artifact_resolution(
    envelope: &StepEnvelope,
    resolved: &[ArtifactRef],
) -> Result<(), RuntimeError> {
    if envelope.delta.artifacts_add.len() != resolved.len() {
        return Err(RuntimeError::ArtifactMismatch);
    }
    for (candidate, artifact) in envelope.delta.artifacts_add.iter().zip(resolved) {
        validate_relative_path(&candidate.path)?;
        validate_text(
            "artifact.purpose",
            &candidate.purpose,
            MAX_ITEM_BYTES,
            false,
        )?;
        validate_relative_path(&artifact.path)?;
        validate_digest("artifact.sha256", &artifact.sha256)?;
        if candidate.path != artifact.path || candidate.purpose != artifact.purpose {
            return Err(RuntimeError::ArtifactMismatch);
        }
    }
    Ok(())
}

fn validate_provider(provider: &ProviderConfig) -> Result<(), RuntimeError> {
    validate_executable(provider.executable(), "provider executable")?;
    let provider_name = match provider {
        ProviderConfig::Claude { .. } => Some("claude"),
        ProviderConfig::Codex { .. } => Some("codex"),
        ProviderConfig::Command { .. } => None,
    };
    validate_args(provider.args(), provider_name)
}

fn validate_executable(path: &Path, field: &str) -> Result<(), RuntimeError> {
    if path.as_os_str().is_empty() {
        return Err(RuntimeError::InvalidManifest(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

fn validate_args(args: &[String], provider: Option<&str>) -> Result<(), RuntimeError> {
    validate_count("arguments", args.len(), MAX_PROVIDER_ARGS)?;
    for arg in args {
        validate_text("argument", arg, MAX_ARGUMENT_BYTES, true)?;
        let lower = arg.to_ascii_lowercase();
        if ["--api-key", "--apikey", "--token", "--password", "--secret"]
            .iter()
            .any(|secret| lower == *secret || lower.starts_with(&format!("{secret}=")))
        {
            return Err(RuntimeError::SecretArgument);
        }
        let continuation = match provider {
            Some("claude") => [
                "--continue",
                "-c",
                "--resume",
                "-r",
                "--fork-session",
                "--session-id",
            ]
            .contains(&lower.as_str()),
            Some("codex") => {
                ["resume", "fork", "--session-id", "--thread-id"].contains(&lower.as_str())
            }
            _ => false,
        };
        if continuation {
            return Err(RuntimeError::ContinuationArgument(arg.clone()));
        }
    }
    Ok(())
}

fn validate_budget(field: &str, value: usize, maximum: usize) -> Result<(), RuntimeError> {
    if value == 0 || value > maximum {
        return Err(RuntimeError::InvalidManifest(format!(
            "{field} must be between 1 and {maximum}"
        )));
    }
    Ok(())
}

fn validate_schema(actual: u64) -> Result<(), RuntimeError> {
    if actual != RUNTIME_SCHEMA_VERSION {
        return Err(RuntimeError::UnsupportedSchema {
            expected: RUNTIME_SCHEMA_VERSION,
            actual,
        });
    }
    Ok(())
}

fn validate_safe_id(value: &str) -> Result<(), RuntimeError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(RuntimeError::InvalidState(
            "invalid bounded ASCII id".into(),
        ));
    }
    Ok(())
}

fn validate_text(
    field: &'static str,
    value: &str,
    allowed: usize,
    allow_empty: bool,
) -> Result<(), RuntimeError> {
    if !allow_empty && value.is_empty() {
        return Err(RuntimeError::InvalidState(format!(
            "{field} must not be empty"
        )));
    }
    if value.len() > allowed {
        return Err(RuntimeError::FieldTooLarge {
            field,
            actual: value.len(),
            allowed,
        });
    }
    Ok(())
}

fn validate_count(field: &'static str, actual: usize, allowed: usize) -> Result<(), RuntimeError> {
    if actual > allowed {
        return Err(RuntimeError::TooManyItems {
            field,
            actual,
            allowed,
        });
    }
    Ok(())
}

fn insert_unique(seen: &mut HashSet<String>, id: &str) -> Result<(), RuntimeError> {
    if !seen.insert(id.to_string()) {
        return Err(RuntimeError::DuplicateId(id.to_string()));
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<(), RuntimeError> {
    let parsed = Path::new(path);
    let windows_prefix = path.as_bytes().get(1) == Some(&b':');
    if path.is_empty()
        || path.contains('\\')
        || path.starts_with("./")
        || path.contains("/./")
        || path.contains("//")
        || path.ends_with('/')
        || path.ends_with("/.")
        || windows_prefix
        || parsed.is_absolute()
        || parsed
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(RuntimeError::InvalidRelativePath(path.to_string()));
    }
    Ok(())
}

fn validate_digest(field: &str, digest: &str) -> Result<(), RuntimeError> {
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(RuntimeError::InvalidDigest(field.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::runtime::model::{
        ArtifactRef, CodingState, Manifest, ProviderConfig, RunStatus, RuntimeError, StateDelta,
        StepEnvelope, StepOutcome, WorkItem,
    };

    use super::apply_delta;

    fn manifest() -> Manifest {
        Manifest::for_test(
            "feature-x",
            PathBuf::from("/tmp/project"),
            "Implement feature X",
            ProviderConfig::Command {
                executable: PathBuf::from("fixture-driver"),
                args: vec![],
            },
        )
    }

    fn state(revision: u64) -> CodingState {
        CodingState {
            revision,
            focus: "parser".into(),
            queue: vec![WorkItem {
                id: "parser".into(),
                task: "Implement parser".into(),
            }],
            ..CodingState::initial()
        }
    }

    fn envelope(revision: u64) -> StepEnvelope {
        StepEnvelope {
            schema_version: 1,
            base_revision: revision,
            outcome: StepOutcome::Continue,
            summary: "Parser work advanced".into(),
            delta: StateDelta::default(),
        }
    }

    #[test]
    fn stale_delta_preserves_revision_contract() {
        let error = apply_delta(&manifest(), &state(4), envelope(3), &[], None).unwrap_err();
        assert_eq!(
            error,
            RuntimeError::StaleRevision {
                expected: 4,
                actual: 3,
            }
        );
    }

    #[test]
    fn unknown_state_fields_are_rejected() {
        let raw = serde_json::json!({
            "schema_version": 1,
            "revision": 0,
            "status": "running",
            "focus": "",
            "memory_summary": "",
            "queue": [],
            "completed": [],
            "decisions": [],
            "blockers": [],
            "active_files": [],
            "checks": [],
            "artifacts": [],
            "archive_count": 0,
            "archive_hash": null,
            "transcript": "must never be accepted"
        });

        assert!(serde_json::from_value::<CodingState>(raw).is_err());
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let mut next = envelope(0);
        next.delta.queue_replace = Some(vec![
            WorkItem {
                id: "same".into(),
                task: "one".into(),
            },
            WorkItem {
                id: "same".into(),
                task: "two".into(),
            },
        ]);

        assert_eq!(
            apply_delta(&manifest(), &CodingState::initial(), next, &[], None).unwrap_err(),
            RuntimeError::DuplicateId("same".into())
        );
    }

    #[test]
    fn artifact_paths_must_be_project_relative() {
        for path in ["/tmp/secret", "../outside", "src/../../outside"] {
            let artifact = ArtifactRef {
                path: path.into(),
                purpose: "evidence".into(),
                sha256: "00".repeat(32),
            };
            assert!(apply_delta(
                &manifest(),
                &CodingState::initial(),
                envelope(0),
                &[artifact],
                None,
            )
            .is_err());
        }
    }

    #[test]
    fn completion_requires_empty_work_and_runtime_verification() {
        let mut next = envelope(0);
        next.outcome = StepOutcome::Complete;

        assert_eq!(
            apply_delta(&manifest(), &state(0), next.clone(), &[], None).unwrap_err(),
            RuntimeError::IncompleteQueue
        );

        next.delta.queue_replace = Some(vec![]);
        assert_eq!(
            apply_delta(&manifest(), &state(0), next, &[], None).unwrap_err(),
            RuntimeError::VerificationRequired
        );
    }

    #[test]
    fn blocked_outcome_requires_a_blocker() {
        let mut next = envelope(0);
        next.outcome = StepOutcome::Blocked;
        next.delta.queue_replace = Some(vec![]);

        assert_eq!(
            apply_delta(&manifest(), &CodingState::initial(), next, &[], None).unwrap_err(),
            RuntimeError::BlockerRequired
        );
    }

    #[test]
    fn continue_advances_exactly_one_revision() {
        let transition = apply_delta(&manifest(), &CodingState::initial(), envelope(0), &[], None)
            .expect("valid transition");

        assert_eq!(transition.state.revision, 1);
        assert_eq!(transition.state.status, RunStatus::Running);
    }

    #[test]
    fn overflow_archives_oldest_entry_only_with_a_new_memory_summary() {
        let mut current = CodingState::initial();
        current.memory_summary = "old summary".into();
        current.completed = (0..32)
            .map(|index| crate::runtime::model::CompletedItem {
                id: format!("done-{index}"),
                result: format!("result {index}"),
            })
            .collect();
        let mut next = envelope(0);
        next.delta.completed_add = vec![crate::runtime::model::CompletedItem {
            id: "done-32".into(),
            result: "result 32".into(),
        }];

        assert_eq!(
            apply_delta(&manifest(), &current, next.clone(), &[], None).unwrap_err(),
            RuntimeError::SummaryRequiredForArchive
        );

        next.delta.memory_summary = Some("new cumulative summary".into());
        let transition = apply_delta(&manifest(), &current, next, &[], None)
            .expect("summary permits bounded archive");
        assert_eq!(transition.archive.completed[0].id, "done-0");
        assert_eq!(transition.state.completed.len(), 32);
        assert_eq!(transition.state.archive_count, 1);
        assert_eq!(transition.state.archive_hash.as_deref().unwrap().len(), 64);
    }

    #[test]
    fn paths_must_already_be_normalized() {
        for path in ["./src/lib.rs", "src//lib.rs", "src/./lib.rs"] {
            let mut next = envelope(0);
            next.delta.active_files_replace = Some(vec![path.into()]);
            assert_eq!(
                apply_delta(&manifest(), &CodingState::initial(), next, &[], None).unwrap_err(),
                RuntimeError::InvalidRelativePath(path.into())
            );
        }
    }

    #[test]
    fn terminal_state_cannot_advance() {
        let mut current = CodingState::initial();
        current.status = RunStatus::Complete;
        let mut approved = manifest();
        approved.allow_unverified_completion = true;

        assert!(matches!(
            apply_delta(&approved, &current, envelope(0), &[], None),
            Err(RuntimeError::InvalidState(message)) if message == "terminal state cannot advance"
        ));
    }
}
