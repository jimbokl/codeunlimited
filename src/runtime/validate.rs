use std::collections::HashSet;
use std::path::{Component, Path};

use sha2::{Digest, Sha256};

use super::model::{
    ArchiveBatch, ArtifactRef, CheckResult, CodingState, EpistemicCandidate, EpistemicEvidence,
    EpistemicEvidenceCandidate, EpistemicItem, EpistemicStatus, Manifest, ProviderConfig,
    RunStatus, RuntimeError, StepEnvelope, StepOutcome, Transition, MAX_OBSERVATION_BUDGET_BYTES,
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
pub const MAX_EPISTEMIC_ITEMS: usize = 32;
pub const MAX_EVIDENCE_PER_ITEM: usize = 8;
pub const MAX_PROVIDER_ARGS: usize = 64;
pub const MAX_ARGUMENT_BYTES: usize = 4096;
pub const MAX_TOTAL_ATTEMPTS: u64 = 100;
pub const MAX_ATTEMPTS_PER_REVISION: u64 = 10;

pub fn validate_manifest(manifest: &Manifest) -> Result<(), RuntimeError> {
    validate_schema(manifest.schema_version)?;
    validate_run_name(&manifest.run_name)?;
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
    validate_provider_config(&manifest.provider)?;
    if manifest.max_steps == 0 || manifest.max_steps > MAX_TOTAL_ATTEMPTS {
        return Err(RuntimeError::InvalidManifest(format!(
            "max_steps must be between 1 and {MAX_TOTAL_ATTEMPTS}"
        )));
    }
    if manifest.max_total_tokens == Some(0) {
        return Err(RuntimeError::InvalidManifest(
            "max_total_tokens must be greater than zero".into(),
        ));
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

pub fn validate_run_name(value: &str) -> Result<(), RuntimeError> {
    validate_safe_id(value).map_err(|_| RuntimeError::InvalidRunName)
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
    validate_count("epistemic", state.epistemic.len(), MAX_EPISTEMIC_ITEMS)?;

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
    let mut epistemic_ids = HashSet::new();
    for item in &state.epistemic {
        validate_safe_id(&item.id)?;
        validate_text("epistemic.claim", &item.claim, MAX_ITEM_BYTES, false)?;
        validate_count(
            "epistemic.evidence",
            item.evidence.len(),
            MAX_EVIDENCE_PER_ITEM,
        )?;
        insert_unique(&mut epistemic_ids, &item.id)?;
        if item.updated_revision > state.revision {
            return Err(RuntimeError::InvalidState(
                "epistemic revision is newer than state".into(),
            ));
        }
        validate_persisted_evidence(item, state)?;
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
    latest_observation: &[u8],
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

    let epistemic_archive = apply_epistemic_delta(
        &mut state,
        envelope.delta.epistemic_upsert,
        envelope.delta.epistemic_retire,
        latest_observation,
        envelope.summary.as_bytes(),
        &previous_summary,
    )?;

    let mut archive = compact_visible_history(&mut state, &previous_summary)?;
    archive.epistemic = epistemic_archive;
    if !archive.is_empty() {
        state.archive_count = state
            .archive_count
            .checked_add(
                (archive.completed.len() + archive.decisions.len() + archive.epistemic.len())
                    as u64,
            )
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
        epistemic: Vec::new(),
    })
}

fn apply_epistemic_delta(
    state: &mut CodingState,
    upserts: Vec<EpistemicCandidate>,
    retirements: Vec<String>,
    latest_observation: &[u8],
    step_summary: &[u8],
    previous_summary: &str,
) -> Result<Vec<EpistemicItem>, RuntimeError> {
    let mut changed_ids = HashSet::new();
    for candidate in &upserts {
        validate_safe_id(&candidate.id)?;
        validate_text("epistemic.claim", &candidate.claim, MAX_ITEM_BYTES, false)?;
        validate_count(
            "epistemic.evidence",
            candidate.evidence.len(),
            MAX_EVIDENCE_PER_ITEM,
        )?;
        if !changed_ids.insert(candidate.id.clone()) {
            return Err(RuntimeError::DuplicateId(candidate.id.clone()));
        }
    }
    for id in &retirements {
        validate_safe_id(id)?;
        if !changed_ids.insert(id.clone()) {
            return Err(RuntimeError::DuplicateId(id.clone()));
        }
    }

    for candidate in upserts {
        let prior = state
            .epistemic
            .iter()
            .find(|item| item.id == candidate.id)
            .map(|item| item.status);
        validate_epistemic_status_change(prior, candidate.status)?;
        let evidence =
            resolve_epistemic_evidence(state, &candidate, latest_observation, step_summary)?;
        validate_evidence_for_status(candidate.status, &evidence)?;
        let item = EpistemicItem {
            id: candidate.id,
            claim: candidate.claim,
            status: candidate.status,
            evidence,
            updated_revision: state.revision,
        };
        if let Some(index) = state.epistemic.iter().position(|value| value.id == item.id) {
            state.epistemic[index] = item;
        } else {
            state.epistemic.push(item);
        }
    }

    if !retirements.is_empty() && state.memory_summary == previous_summary {
        return Err(RuntimeError::SummaryRequiredForArchive);
    }
    let mut archive = Vec::with_capacity(retirements.len());
    for id in retirements {
        let index = state
            .epistemic
            .iter()
            .position(|item| item.id == id)
            .ok_or_else(|| RuntimeError::InvalidState("retired knowledge was not found".into()))?;
        if state.epistemic[index].status != EpistemicStatus::Disputed {
            return Err(RuntimeError::InvalidState(
                "knowledge must be disputed before retirement".into(),
            ));
        }
        archive.push(state.epistemic.remove(index));
    }
    validate_count("epistemic", state.epistemic.len(), MAX_EPISTEMIC_ITEMS)?;
    Ok(archive)
}

fn resolve_epistemic_evidence(
    state: &CodingState,
    candidate: &EpistemicCandidate,
    latest_observation: &[u8],
    step_summary: &[u8],
) -> Result<Vec<EpistemicEvidence>, RuntimeError> {
    let latest_hash = format!("{:x}", Sha256::digest(latest_observation));
    let mut unique = HashSet::new();
    candidate
        .evidence
        .iter()
        .map(|evidence| {
            let resolved = match evidence {
                EpistemicEvidenceCandidate::Step => EpistemicEvidence::Observation {
                    sha256: format!("{:x}", Sha256::digest(step_summary)),
                },
                EpistemicEvidenceCandidate::Observation { sha256 } => {
                    validate_digest("epistemic observation", sha256)?;
                    if latest_observation.is_empty() || sha256 != &latest_hash {
                        return Err(RuntimeError::InvalidState(
                            "observed knowledge must cite the latest observation".into(),
                        ));
                    }
                    EpistemicEvidence::Observation {
                        sha256: sha256.clone(),
                    }
                }
                EpistemicEvidenceCandidate::Check { revision } => {
                    if !state
                        .checks
                        .iter()
                        .any(|check| check.revision == *revision && check.passed)
                    {
                        return Err(RuntimeError::InvalidState(
                            "verified knowledge must cite a passed check".into(),
                        ));
                    }
                    EpistemicEvidence::Check {
                        revision: *revision,
                    }
                }
                EpistemicEvidenceCandidate::Artifact { path } => {
                    validate_relative_path(path)?;
                    let artifact = state
                        .artifacts
                        .iter()
                        .find(|artifact| artifact.path == *path)
                        .ok_or(RuntimeError::ArtifactMismatch)?;
                    EpistemicEvidence::Artifact {
                        path: path.clone(),
                        sha256: artifact.sha256.clone(),
                    }
                }
            };
            let key = serde_json::to_string(&resolved)
                .map_err(|error| RuntimeError::InvalidState(error.to_string()))?;
            if !unique.insert(key) {
                return Err(RuntimeError::InvalidState(
                    "duplicate epistemic evidence".into(),
                ));
            }
            Ok(resolved)
        })
        .collect()
}

fn validate_epistemic_status_change(
    prior: Option<EpistemicStatus>,
    next: EpistemicStatus,
) -> Result<(), RuntimeError> {
    let valid = match prior {
        None | Some(EpistemicStatus::Hypothesis) => true,
        Some(EpistemicStatus::Observed) => {
            matches!(
                next,
                EpistemicStatus::Observed | EpistemicStatus::Verified | EpistemicStatus::Disputed
            )
        }
        Some(EpistemicStatus::Verified) => {
            matches!(next, EpistemicStatus::Verified | EpistemicStatus::Disputed)
        }
        Some(EpistemicStatus::Disputed) => next == EpistemicStatus::Disputed,
    };
    if valid {
        Ok(())
    } else {
        Err(RuntimeError::InvalidState(
            "invalid epistemic status regression".into(),
        ))
    }
}

fn validate_evidence_for_status(
    status: EpistemicStatus,
    evidence: &[EpistemicEvidence],
) -> Result<(), RuntimeError> {
    let supported = match status {
        EpistemicStatus::Hypothesis => true,
        EpistemicStatus::Observed => evidence
            .iter()
            .any(|item| matches!(item, EpistemicEvidence::Observation { .. })),
        EpistemicStatus::Verified => evidence.iter().any(|item| {
            matches!(
                item,
                EpistemicEvidence::Check { .. } | EpistemicEvidence::Artifact { .. }
            )
        }),
        EpistemicStatus::Disputed => !evidence.is_empty(),
    };
    if supported {
        Ok(())
    } else {
        Err(RuntimeError::InvalidState(
            "epistemic status lacks required evidence".into(),
        ))
    }
}

fn validate_persisted_evidence(
    item: &EpistemicItem,
    state: &CodingState,
) -> Result<(), RuntimeError> {
    for evidence in &item.evidence {
        match evidence {
            EpistemicEvidence::Observation { sha256 } => {
                validate_digest("epistemic observation", sha256)?;
            }
            EpistemicEvidence::Check { revision } => {
                if *revision > state.revision {
                    return Err(RuntimeError::InvalidState(
                        "epistemic check is newer than state".into(),
                    ));
                }
                if !state
                    .checks
                    .iter()
                    .any(|check| check.revision == *revision && check.passed)
                {
                    return Err(RuntimeError::InvalidState(
                        "epistemic persisted check evidence is missing".into(),
                    ));
                }
            }
            EpistemicEvidence::Artifact { path, sha256 } => {
                validate_relative_path(path)?;
                validate_digest("epistemic artifact", sha256)?;
                if !state
                    .artifacts
                    .iter()
                    .any(|artifact| artifact.path == *path && artifact.sha256 == *sha256)
                {
                    return Err(RuntimeError::InvalidState(
                        "epistemic persisted artifact evidence is missing".into(),
                    ));
                }
            }
        }
    }
    validate_evidence_for_status(item.status, &item.evidence)
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

pub fn validate_provider_config(provider: &ProviderConfig) -> Result<(), RuntimeError> {
    match provider {
        ProviderConfig::Claude { executable, .. } => {
            validate_executable(executable, "provider executable")?;
            validate_args(provider.args(), Some("claude"))?;
            validate_runtime_args(provider.args(), "claude")
        }
        ProviderConfig::Codex { executable, .. } => {
            validate_executable(executable, "provider executable")?;
            validate_args(provider.args(), Some("codex"))?;
            validate_runtime_args(provider.args(), "codex")
        }
        ProviderConfig::Command { executable, .. } => {
            validate_executable(executable, "provider executable")?;
            validate_args(provider.args(), None)
        }
        ProviderConfig::OpenAiApi {
            endpoint,
            model,
            api_key_env,
            cache_ttl,
        } => {
            validate_api_settings(endpoint, model, api_key_env)?;
            if *cache_ttl != super::model::ApiCacheTtl::ThirtyMinutes {
                return Err(RuntimeError::InvalidManifest(
                    "OpenAI API cache TTL must be 30m".into(),
                ));
            }
            Ok(())
        }
        ProviderConfig::AnthropicApi {
            endpoint,
            model,
            api_key_env,
            cache_ttl,
        } => {
            validate_api_settings(endpoint, model, api_key_env)?;
            if !matches!(
                cache_ttl,
                super::model::ApiCacheTtl::FiveMinutes | super::model::ApiCacheTtl::OneHour
            ) {
                return Err(RuntimeError::InvalidManifest(
                    "Anthropic API cache TTL must be 5m or 1h".into(),
                ));
            }
            Ok(())
        }
    }
}

fn validate_api_settings(
    endpoint: &str,
    model: &str,
    api_key_env: &str,
) -> Result<(), RuntimeError> {
    validate_text("API endpoint", endpoint, 2048, false)?;
    validate_text("API model", model, 256, false)?;
    validate_text("API key environment variable", api_key_env, 128, false)?;
    if !api_key_env.bytes().enumerate().all(|(index, byte)| {
        byte == b'_' || byte.is_ascii_uppercase() || (index > 0 && byte.is_ascii_digit())
    }) || api_key_env
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_digit)
    {
        return Err(RuntimeError::InvalidManifest(
            "API key environment variable must use uppercase ASCII, digits, and underscores".into(),
        ));
    }
    let url = url::Url::parse(endpoint).map_err(|_| {
        RuntimeError::InvalidManifest("API endpoint must be an absolute URL".into())
    })?;
    let secure = url.scheme() == "https";
    let loopback_http = url.scheme() == "http"
        && match url.host() {
            Some(url::Host::Domain("localhost")) => true,
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
    if (!secure && !loopback_http) || url.username() != "" || url.password().is_some() {
        return Err(RuntimeError::InvalidManifest(
            "API endpoint must use HTTPS; HTTP is allowed only for loopback tests".into(),
        ));
    }
    Ok(())
}

fn validate_executable(path: &Path, field: &str) -> Result<(), RuntimeError> {
    if path.as_os_str().is_empty() {
        return Err(RuntimeError::InvalidManifest(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

fn option_matches(arg: &str, flag: &str) -> bool {
    arg == flag
        || arg.starts_with(&format!("{flag}="))
        || (flag.len() == 2 && flag.starts_with('-') && flag != "--" && arg.starts_with(flag))
}

fn validate_runtime_args(args: &[String], provider: &str) -> Result<(), RuntimeError> {
    let protected: &[&str] = if provider == "claude" {
        &[
            "--print",
            "-p",
            "--no-session-persistence",
            "--output-format",
            "--json-schema",
            "--input-format",
            "--exclude-dynamic-system-prompt-sections",
            "--append-system-prompt-file",
            "--append-system-prompt",
            "--system-prompt",
            "--system-prompt-file",
            "--disable-slash-commands",
            "--no-chrome",
            "--chrome",
            "--strict-mcp-config",
            "--mcp-config",
            "--tools",
            "--bare",
            "--agent",
            "--agents",
            "--settings",
            "--setting-sources",
            "--plugin-dir",
            "--plugin-url",
            "--",
        ]
    } else {
        &[
            "exec",
            "resume",
            "fork",
            "--ephemeral",
            "--output-schema",
            "--output-last-message",
            "-o",
            "--cd",
            "-c",
            "--json",
            "--ignore-user-config",
            "--config",
            "--profile",
            "-p",
            "--",
        ]
    };
    let invalid = || {
        RuntimeError::InvalidManifest(
            "provider argument overrides required runtime configuration".into(),
        )
    };
    let mut arguments = args.iter();
    while let Some(arg) = arguments.next() {
        if provider == "codex" {
            let setting = if arg == "-c" || arg == "--config" {
                Some(arguments.next().ok_or_else(invalid)?.as_str())
            } else {
                arg.strip_prefix("--config=")
                    .or_else(|| arg.strip_prefix("-c"))
            };
            if let Some(setting) = setting {
                if !safe_codex_tuning(setting) {
                    return Err(invalid());
                }
                continue;
            }
        }
        if protected
            .iter()
            .any(|flag| option_matches(&arg.to_ascii_lowercase(), flag))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn safe_codex_tuning(setting: &str) -> bool {
    let Some((key, value)) = setting.split_once('=') else {
        return false;
    };
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value);
    match key.trim() {
        "model_reasoning_effort" => [
            "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
        ]
        .contains(&value),
        "model_verbosity" => ["low", "medium", "high"].contains(&value),
        _ => false,
    }
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
                "--from-pr",
                "-r",
                "--fork-session",
                "--session-id",
            ]
            .iter()
            .any(|flag| option_matches(&lower, flag)),
            Some("codex") => ["resume", "fork", "--session-id", "--thread-id"]
                .iter()
                .any(|flag| option_matches(&lower, flag)),
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

    use sha2::{Digest, Sha256};

    use crate::runtime::model::{
        ArtifactRef, CheckResult, CodingState, EpistemicCandidate, EpistemicEvidence,
        EpistemicEvidenceCandidate, EpistemicItem, EpistemicStatus, Manifest, ProviderConfig,
        RunStatus, RuntimeError, StateDelta, StepEnvelope, StepOutcome, WorkItem,
    };

    use super::{apply_delta, validate_state, MAX_EPISTEMIC_ITEMS};

    fn project_root() -> PathBuf {
        // Absolute on every platform: "/tmp/project" is not absolute on Windows.
        if cfg!(windows) {
            PathBuf::from(r"C:\tmp\project")
        } else {
            PathBuf::from("/tmp/project")
        }
    }

    fn manifest() -> Manifest {
        Manifest::for_test(
            "feature-x",
            project_root(),
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
        let error = apply_delta(&manifest(), &state(4), envelope(3), &[], None, b"").unwrap_err();
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
            "epistemic": [],
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
            apply_delta(&manifest(), &CodingState::initial(), next, &[], None, b"").unwrap_err(),
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
                b"",
            )
            .is_err());
        }
    }

    #[test]
    fn completion_requires_empty_work_and_runtime_verification() {
        let mut next = envelope(0);
        next.outcome = StepOutcome::Complete;

        assert_eq!(
            apply_delta(&manifest(), &state(0), next.clone(), &[], None, b"").unwrap_err(),
            RuntimeError::IncompleteQueue
        );

        next.delta.queue_replace = Some(vec![]);
        assert_eq!(
            apply_delta(&manifest(), &state(0), next, &[], None, b"").unwrap_err(),
            RuntimeError::VerificationRequired
        );
    }

    #[test]
    fn blocked_outcome_requires_a_blocker() {
        let mut next = envelope(0);
        next.outcome = StepOutcome::Blocked;
        next.delta.queue_replace = Some(vec![]);

        assert_eq!(
            apply_delta(&manifest(), &CodingState::initial(), next, &[], None, b"").unwrap_err(),
            RuntimeError::BlockerRequired
        );
    }

    #[test]
    fn continue_advances_exactly_one_revision() {
        let transition = apply_delta(
            &manifest(),
            &CodingState::initial(),
            envelope(0),
            &[],
            None,
            b"",
        )
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
            apply_delta(&manifest(), &current, next.clone(), &[], None, b"").unwrap_err(),
            RuntimeError::SummaryRequiredForArchive
        );

        next.delta.memory_summary = Some("new cumulative summary".into());
        let transition = apply_delta(&manifest(), &current, next, &[], None, b"")
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
                apply_delta(&manifest(), &CodingState::initial(), next, &[], None, b"")
                    .unwrap_err(),
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
            apply_delta(&approved, &current, envelope(0), &[], None, b""),
            Err(RuntimeError::InvalidState(message)) if message == "terminal state cannot advance"
        ));
    }

    #[test]
    fn hypotheses_are_bounded_but_do_not_require_evidence() {
        let mut next = envelope(0);
        next.delta.epistemic_upsert = vec![EpistemicCandidate {
            id: "parser-cause".into(),
            claim: "The parser drops nested usage records".into(),
            status: EpistemicStatus::Hypothesis,
            evidence: vec![],
        }];

        let transition = apply_delta(&manifest(), &CodingState::initial(), next, &[], None, b"")
            .expect("hypothesis is valid");
        let item = &transition.state.epistemic[0];
        assert_eq!(item.updated_revision, 1);
        assert_eq!(item.status, EpistemicStatus::Hypothesis);
    }

    #[test]
    fn observed_claims_must_bind_to_the_latest_observation() {
        let observation = b"fixture reproduced at line 12";
        let observation_sha256 = format!("{:x}", Sha256::digest(observation));
        let mut next = envelope(0);
        next.delta.epistemic_upsert = vec![EpistemicCandidate {
            id: "parser-observation".into(),
            claim: "The fixture reproduces the parser bug".into(),
            status: EpistemicStatus::Observed,
            evidence: vec![EpistemicEvidenceCandidate::Observation {
                sha256: observation_sha256.clone(),
            }],
        }];

        let transition = apply_delta(
            &manifest(),
            &CodingState::initial(),
            next.clone(),
            &[],
            None,
            observation,
        )
        .expect("matching observation is evidence");
        assert_eq!(transition.state.epistemic[0].evidence.len(), 1);

        next.delta.epistemic_upsert[0].evidence = vec![EpistemicEvidenceCandidate::Observation {
            sha256: "00".repeat(32),
        }];
        assert!(matches!(
            apply_delta(
                &manifest(),
                &CodingState::initial(),
                next,
                &[],
                None,
                observation,
            ),
            Err(RuntimeError::InvalidState(message)) if message.contains("latest observation")
        ));
    }

    #[test]
    fn current_step_evidence_can_be_retained_without_an_extra_round_trip() {
        let mut next = envelope(0);
        next.summary = "parser failure reproduced in this increment".into();
        next.delta.epistemic_upsert = vec![EpistemicCandidate {
            id: "parser-reproduction".into(),
            claim: "The parser failure is reproducible".into(),
            status: EpistemicStatus::Observed,
            evidence: vec![EpistemicEvidenceCandidate::Step],
        }];

        let transition = apply_delta(&manifest(), &CodingState::initial(), next, &[], None, b"")
            .expect("current step summary is bounded evidence");
        assert_eq!(
            transition.state.epistemic[0].evidence,
            vec![crate::runtime::model::EpistemicEvidence::Observation {
                sha256: format!(
                    "{:x}",
                    Sha256::digest(b"parser failure reproduced in this increment")
                ),
            }]
        );
    }

    #[test]
    fn persisted_verified_knowledge_cannot_forge_missing_evidence() {
        let mut state = CodingState::initial();
        state.epistemic = vec![EpistemicItem {
            id: "forged".into(),
            claim: "Tests passed".into(),
            status: EpistemicStatus::Verified,
            evidence: vec![EpistemicEvidence::Check { revision: 0 }],
            updated_revision: 0,
        }];

        assert!(matches!(
            validate_state(&manifest(), &state),
            Err(RuntimeError::InvalidState(message)) if message.contains("persisted check")
        ));
    }

    #[test]
    fn epistemic_memory_has_a_hard_item_cap() {
        let mut next = envelope(0);
        next.delta.epistemic_upsert = (0..=MAX_EPISTEMIC_ITEMS)
            .map(|index| EpistemicCandidate {
                id: format!("claim-{index}"),
                claim: format!("bounded hypothesis {index}"),
                status: EpistemicStatus::Hypothesis,
                evidence: vec![],
            })
            .collect();

        assert_eq!(
            apply_delta(&manifest(), &CodingState::initial(), next, &[], None, b"",).unwrap_err(),
            RuntimeError::TooManyItems {
                field: "epistemic",
                actual: MAX_EPISTEMIC_ITEMS + 1,
                allowed: MAX_EPISTEMIC_ITEMS,
            }
        );
    }

    #[test]
    fn verified_claims_require_a_passed_check_or_hashed_artifact() {
        let check = CheckResult {
            revision: 1,
            program: "cargo".into(),
            args: vec!["test".into()],
            passed: true,
            summary: "tests passed".into(),
            workspace_sha256: Some("11".repeat(32)),
        };
        let mut next = envelope(0);
        next.delta.epistemic_upsert = vec![EpistemicCandidate {
            id: "suite-green".into(),
            claim: "The test suite passes at revision 1".into(),
            status: EpistemicStatus::Verified,
            evidence: vec![EpistemicEvidenceCandidate::Check { revision: 1 }],
        }];

        let transition = apply_delta(
            &manifest(),
            &CodingState::initial(),
            next.clone(),
            &[],
            Some(check),
            b"",
        )
        .expect("passed check verifies claim");
        assert_eq!(
            transition.state.epistemic[0].status,
            EpistemicStatus::Verified
        );

        assert!(matches!(
            apply_delta(
                &manifest(),
                &CodingState::initial(),
                next,
                &[],
                None,
                b"",
            ),
            Err(RuntimeError::InvalidState(message)) if message.contains("passed check")
        ));
    }

    #[test]
    fn verified_knowledge_must_be_disputed_before_retirement() {
        let mut create = envelope(0);
        create.delta.epistemic_upsert = vec![EpistemicCandidate {
            id: "api-contract".into(),
            claim: "The fixture validates the API contract".into(),
            status: EpistemicStatus::Verified,
            evidence: vec![EpistemicEvidenceCandidate::Artifact {
                path: "evidence.json".into(),
            }],
        }];
        let artifact = ArtifactRef {
            path: "evidence.json".into(),
            purpose: "test evidence".into(),
            sha256: "22".repeat(32),
        };
        create.delta.artifacts_add = vec![crate::runtime::model::ArtifactCandidate {
            path: "evidence.json".into(),
            purpose: "test evidence".into(),
        }];
        let current = apply_delta(
            &manifest(),
            &CodingState::initial(),
            create,
            std::slice::from_ref(&artifact),
            None,
            b"",
        )
        .expect("artifact verifies claim")
        .state;

        let mut retire = envelope(1);
        retire.delta.memory_summary = Some("The earlier contract claim is obsolete".into());
        retire.delta.epistemic_retire = vec!["api-contract".into()];
        assert!(matches!(
            apply_delta(&manifest(), &current, retire, &[], None, b""),
            Err(RuntimeError::InvalidState(message)) if message.contains("disputed")
        ));

        let mut dispute = envelope(1);
        dispute.delta.epistemic_upsert = vec![EpistemicCandidate {
            id: "api-contract".into(),
            claim: "The earlier fixture no longer matches the API contract".into(),
            status: EpistemicStatus::Disputed,
            evidence: vec![EpistemicEvidenceCandidate::Observation {
                sha256: format!("{:x}", Sha256::digest(b"API changed")),
            }],
        }];
        let disputed = apply_delta(&manifest(), &current, dispute, &[], None, b"API changed")
            .expect("verified claim may become disputed")
            .state;
        let mut retire = envelope(2);
        retire.delta.memory_summary = Some("Removed the obsolete contract claim".into());
        retire.delta.epistemic_retire = vec!["api-contract".into()];
        let transition = apply_delta(&manifest(), &disputed, retire, &[], None, b"")
            .expect("disputed knowledge may be retired");
        assert!(transition.state.epistemic.is_empty());
        assert_eq!(transition.archive.epistemic.len(), 1);
    }
}
