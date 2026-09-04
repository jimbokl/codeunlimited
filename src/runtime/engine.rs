use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::ledger::LedgerReport;
use super::model::{
    ArtifactRef, CheckResult, CodingState, GitSnapshot, Manifest, ProviderConfig, RecoveryRecord,
    RunStatus, RuntimeError, StepOutcome, VerificationCommand, WorkPlan,
    DEFAULT_MAX_ATTEMPTS_PER_REVISION, DEFAULT_MAX_STEPS, DEFAULT_OBSERVATION_BUDGET_BYTES,
    DEFAULT_PROMPT_BUDGET_BYTES, DEFAULT_PROVIDER_TIMEOUT_SECONDS, DEFAULT_STATE_BUDGET_BYTES,
    DEFAULT_WORKFLOW_BUDGET_BYTES, RUNTIME_SCHEMA_VERSION,
};
use super::prompt::{compile_prompt, inspect_prompt, CompiledPrompt};
use super::provider::{
    capture_process, CommandSpec, InputTokenSemantics, Provider, ProviderFailure,
    ProviderProbeSample, ProviderUsage,
};
use super::store::RunStore;
use super::validate::{apply_delta, validate_manifest};

#[derive(Debug, Clone)]
pub struct InitRequest {
    pub project_root: PathBuf,
    pub run_name: String,
    pub workflow_source: PathBuf,
    pub objective: String,
    pub provider: ProviderConfig,
    pub max_steps: u64,
    pub max_total_tokens: Option<u64>,
    pub max_attempts_per_revision: u64,
    pub provider_timeout_seconds: u64,
    pub workflow_budget_bytes: usize,
    pub state_budget_bytes: usize,
    pub observation_budget_bytes: usize,
    pub prompt_budget_bytes: usize,
    pub verification_command: Option<VerificationCommand>,
    pub verify_every_step: bool,
    pub allow_unverified_completion: bool,
    pub work_plan: Option<WorkPlan>,
}

impl InitRequest {
    pub fn new(
        project_root: PathBuf,
        run_name: String,
        workflow_source: PathBuf,
        objective: String,
        provider: ProviderConfig,
    ) -> Self {
        Self {
            project_root,
            run_name,
            workflow_source,
            objective,
            provider,
            max_steps: DEFAULT_MAX_STEPS,
            max_total_tokens: None,
            max_attempts_per_revision: DEFAULT_MAX_ATTEMPTS_PER_REVISION,
            provider_timeout_seconds: DEFAULT_PROVIDER_TIMEOUT_SECONDS,
            workflow_budget_bytes: DEFAULT_WORKFLOW_BUDGET_BYTES,
            state_budget_bytes: DEFAULT_STATE_BUDGET_BYTES,
            observation_budget_bytes: DEFAULT_OBSERVATION_BUDGET_BYTES,
            prompt_budget_bytes: DEFAULT_PROMPT_BUDGET_BYTES,
            verification_command: None,
            verify_every_step: false,
            allow_unverified_completion: false,
            work_plan: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRef {
    pub project_root: PathBuf,
    pub run_name: String,
}

impl RunRef {
    pub fn new(project_root: PathBuf, run_name: String) -> Self {
        Self {
            project_root,
            run_name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunStatusView {
    pub schema_version: u64,
    pub run_name: String,
    pub revision: u64,
    pub status: RunStatus,
    pub recovery_required: bool,
    pub busy: bool,
    pub attempts: u64,
    pub prompt_bytes: usize,
    pub stable_prompt_bytes: usize,
    pub dynamic_prompt_bytes: usize,
    pub usage: ProviderUsage,
    pub transported_input_tokens: Option<u64>,
    pub cache_read_ratio_basis_points: Option<u64>,
    pub provider: ProviderStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderStatus {
    pub kind: String,
    pub layer: String,
    pub capability: String,
    pub subscription_profile: Option<String>,
    pub executable: String,
    pub args: Vec<String>,
    pub isolation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepReport {
    pub schema_version: u64,
    pub run_name: String,
    pub revision: u64,
    pub status: RunStatus,
    pub attempt: u64,
    pub prompt_bytes: usize,
    pub stable_prompt_bytes: usize,
    pub dynamic_prompt_bytes: usize,
    pub usage: ProviderUsage,
    pub transported_input_tokens: Option<u64>,
    pub cache_read_ratio_basis_points: Option<u64>,
    pub verification_passed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutoReport {
    pub schema_version: u64,
    pub run_name: String,
    pub steps: Vec<StepReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CacheProbeReport {
    pub schema_version: u64,
    pub run_name: String,
    pub provider: String,
    pub stable_prompt_sha256: String,
    pub stable_prompt_bytes: usize,
    pub first: ProviderProbeSample,
    pub second: ProviderProbeSample,
    pub cache_hit_reported: Option<bool>,
    pub evidence_scope: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AttemptOutcome {
    Succeeded,
    ProviderFailed,
    InvalidTransition,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttemptRecord {
    pub(crate) schema_version: u64,
    pub(crate) attempt: u64,
    pub(crate) base_revision: u64,
    pub(crate) committed_revision: Option<u64>,
    pub(crate) outcome: AttemptOutcome,
    pub(crate) error_category: Option<String>,
    pub(crate) prompt_bytes: usize,
    pub(crate) stable_prompt_bytes: usize,
    pub(crate) dynamic_prompt_bytes: usize,
    pub(crate) prompt_sha256: String,
    pub(crate) stable_prompt_sha256: String,
    pub(crate) workflow_sha256: String,
    pub(crate) provider: String,
    pub(crate) started_unix: i64,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) exit_code: Option<i32>,
    pub(crate) response_bytes: Option<usize>,
    pub(crate) usage: ProviderUsage,
    pub(crate) state_before_sha256: String,
    pub(crate) state_after_sha256: Option<String>,
    pub(crate) before_git: GitSnapshot,
    pub(crate) after_git: GitSnapshot,
    #[serde(default)]
    pub(crate) accepted_task_ids: Vec<String>,
    #[serde(default)]
    pub(crate) verification_passed: Option<bool>,
    #[serde(default)]
    pub(crate) configuration_sha256: String,
}

impl AttemptOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::ProviderFailed => "provider_failed",
            Self::InvalidTransition => "invalid_transition",
            Self::RecoveryRequired => "recovery_required",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttemptIntent {
    pub(crate) schema_version: u64,
    pub(crate) attempt: u64,
    pub(crate) base_revision: u64,
    pub(crate) provider: String,
    pub(crate) configuration_sha256: String,
    pub(crate) prompt_sha256: String,
    pub(crate) stable_prompt_sha256: String,
    pub(crate) workflow_sha256: String,
    pub(crate) prompt_bytes: usize,
    pub(crate) stable_prompt_bytes: usize,
    pub(crate) dynamic_prompt_bytes: usize,
    pub(crate) state_before_sha256: String,
    pub(crate) started_unix: i64,
    pub(crate) before_git: GitSnapshot,
}

impl AttemptIntent {
    fn validate(&self) -> Result<(), RuntimeError> {
        let valid_digest =
            |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
        if self.schema_version != RUNTIME_SCHEMA_VERSION
            || self.attempt == 0
            || !matches!(
                self.provider.as_str(),
                "claude" | "codex" | "command" | "openai_api" | "anthropic_api"
            )
            || ![
                &self.configuration_sha256,
                &self.prompt_sha256,
                &self.stable_prompt_sha256,
                &self.workflow_sha256,
                &self.state_before_sha256,
            ]
            .into_iter()
            .all(|digest| valid_digest(digest))
        {
            return Err(RuntimeError::InvalidStoredData("attempt-intent.json"));
        }
        Ok(())
    }
}

pub fn init_run(request: InitRequest) -> Result<RunStatusView, RuntimeError> {
    let project_root = fs::canonicalize(&request.project_root)
        .map_err(|_| RuntimeError::Io("canonicalize project".into()))?;
    let workflow_source = if request.workflow_source.is_absolute() {
        request.workflow_source.clone()
    } else {
        project_root.join(&request.workflow_source)
    };
    let metadata = fs::symlink_metadata(&workflow_source)
        .map_err(|_| RuntimeError::Io("read workflow".into()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RuntimeError::UnsafeStorePath);
    }
    let workflow =
        fs::read(&workflow_source).map_err(|_| RuntimeError::Io("read workflow".into()))?;
    let manifest = Manifest {
        schema_version: RUNTIME_SCHEMA_VERSION,
        run_name: request.run_name.clone(),
        project_root: project_root.clone(),
        created_unix: chrono::Utc::now().timestamp(),
        workflow_source,
        workflow_sha256: sha256(&workflow),
        objective: request.objective,
        provider: request.provider,
        max_steps: request.max_steps,
        max_total_tokens: request.max_total_tokens,
        max_attempts_per_revision: request.max_attempts_per_revision,
        provider_timeout_seconds: request.provider_timeout_seconds,
        workflow_budget_bytes: request.workflow_budget_bytes,
        state_budget_bytes: request.state_budget_bytes,
        observation_budget_bytes: request.observation_budget_bytes,
        prompt_budget_bytes: request.prompt_budget_bytes,
        verification_command: request.verification_command,
        verify_every_step: request.verify_every_step,
        allow_unverified_completion: request.allow_unverified_completion,
        work_plan: request.work_plan,
    };
    validate_manifest(&manifest)?;
    let mut initial_state = CodingState::initial();
    if let Some(plan) = &manifest.work_plan {
        initial_state.queue = super::packet::initial_queue(plan);
    }
    let store = RunStore::create(
        &project_root,
        &request.run_name,
        &manifest,
        &workflow,
        &initial_state,
    )?;
    status_for_store(&store)
}

pub fn status(reference: &RunRef) -> Result<RunStatusView, RuntimeError> {
    let store = RunStore::open(&reference.project_root, &reference.run_name)?;
    status_for_store(&store)
}

pub fn ledger(reference: &RunRef) -> Result<LedgerReport, RuntimeError> {
    let store = RunStore::open(&reference.project_root, &reference.run_name)?;
    let snapshot_lock = match store.try_lock() {
        Ok(lock) => Some(lock),
        Err(RuntimeError::RunBusy) => None,
        Err(error) => return Err(error),
    };
    let busy = snapshot_lock.is_none();
    let loaded = store.load()?;
    let attempts: Vec<AttemptRecord> = store.read_attempts()?;
    let intent = read_attempt_intent(&store)?;
    let report = super::ledger::build_report(&loaded.manifest, &attempts, intent.as_ref(), busy)?;
    drop(snapshot_lock);
    Ok(report)
}

pub fn packet(reference: &RunRef) -> Result<super::packet::PacketPreview, RuntimeError> {
    let store = RunStore::open(&reference.project_root, &reference.run_name)?;
    let _lock = store.try_lock()?;
    let loaded = store.load()?;
    let plan = loaded
        .manifest
        .work_plan
        .as_ref()
        .ok_or_else(|| RuntimeError::InvalidState("run has no work plan".into()))?;
    super::packet::preview(&loaded.manifest.run_name, plan, &loaded.state)
}

pub fn render_next_prompt(reference: &RunRef) -> Result<CompiledPrompt, RuntimeError> {
    let store = RunStore::open(&reference.project_root, &reference.run_name)?;
    let loaded = store.load()?;
    inspect_prompt(
        &loaded.manifest,
        &loaded.workflow,
        &loaded.state,
        &loaded.observation,
    )
}

pub fn step(reference: &RunRef, provider: &dyn Provider) -> Result<StepReport, RuntimeError> {
    let store = RunStore::open(&reference.project_root, &reference.run_name)?;
    let _lock = store.try_lock()?;
    let loaded = store.load()?;
    store.ensure_provider_instructions(&loaded)?;
    if loaded.recovery.is_some() || read_attempt_intent(&store)?.is_some() {
        return Err(RuntimeError::RecoveryRequired);
    }
    if loaded.state.status != RunStatus::Running {
        return Err(RuntimeError::TerminalRun);
    }
    let attempts: Vec<AttemptRecord> = store.read_attempts()?;
    if let Some(error) = super::ledger::cap_admission_error(&loaded.manifest, &attempts)? {
        return Err(error);
    }
    if attempts.len() as u64 >= loaded.manifest.max_steps
        || attempts
            .iter()
            .filter(|attempt| attempt.base_revision == loaded.state.revision)
            .count() as u64
            >= loaded.manifest.max_attempts_per_revision
    {
        return Err(RuntimeError::AttemptLimit);
    }
    let attempt = attempts.len() as u64 + 1;
    let prompt = compile_prompt(
        &loaded.manifest,
        &loaded.workflow,
        &loaded.state,
        &loaded.observation,
    )?;
    let before_git = git_snapshot(&loaded.manifest.project_root);
    let before_control = store.control_hash()?;
    let started_unix = chrono::Utc::now().timestamp();
    let intent = AttemptIntent {
        schema_version: RUNTIME_SCHEMA_VERSION,
        attempt,
        base_revision: loaded.state.revision,
        provider: provider_name(&loaded.manifest.provider).into(),
        configuration_sha256: super::ledger::configuration_sha256(&loaded.manifest.provider),
        prompt_sha256: prompt.prompt_sha256.clone(),
        stable_prompt_sha256: prompt.stable_sha256.clone(),
        workflow_sha256: loaded.manifest.workflow_sha256.clone(),
        prompt_bytes: prompt.bytes.len(),
        stable_prompt_bytes: prompt.stable_bytes,
        dynamic_prompt_bytes: prompt.dynamic_bytes,
        state_before_sha256: state_hash(&loaded.state),
        started_unix,
        before_git: before_git.clone(),
    };
    store.write_attempt_intent(&intent)?;
    let provider_result = provider.run(
        &loaded.manifest.provider,
        &prompt,
        &loaded.manifest.project_root,
        Duration::from_secs(loaded.manifest.provider_timeout_seconds),
    );
    let control_unchanged = store
        .control_hash()
        .is_ok_and(|after| after == before_control);
    let intent_intact = store.attempt_intent_matches(&intent).unwrap_or(false);

    let result = match provider_result {
        Ok(result) => result,
        Err(error) => {
            let after_git = git_snapshot(&loaded.manifest.project_root);
            let changed =
                !intent_intact || !control_unchanged || workspace_changed(&before_git, &after_git);
            let outcome = if changed {
                AttemptOutcome::RecoveryRequired
            } else {
                AttemptOutcome::ProviderFailed
            };
            let mut record = failed_attempt(
                &loaded.manifest,
                &loaded.state,
                &prompt,
                attempt,
                started_unix,
                outcome,
                provider_failure_category(&error),
                before_git.clone(),
                after_git.clone(),
            );
            if let ProviderFailure::InvalidOutputWithUsage(usage) = &error {
                record.usage = usage.as_ref().clone();
            }
            let recovery = changed.then(|| RecoveryRecord {
                schema_version: RUNTIME_SCHEMA_VERSION,
                attempt,
                base_revision: loaded.state.revision,
                reason: provider_failure_category(&error),
                before_git: before_git.clone(),
                after_git: after_git.clone(),
            });
            finalize_attempt(&store, &intent, &record, recovery.as_ref(), intent_intact)?;
            if changed {
                return Err(RuntimeError::RecoveryRequired);
            }
            return Err(RuntimeError::ProviderFailed(provider_failure_category(
                &error,
            )));
        }
    };
    if !control_unchanged || !intent_intact {
        return fail_transition(
            &store,
            &intent,
            intent_intact,
            &loaded.manifest,
            &loaded.state,
            &prompt,
            attempt,
            started_unix,
            &result,
            before_git,
            "runtime_control_changed".into(),
            false,
        );
    }

    if let Some(plan) = &loaded.manifest.work_plan {
        if let Err(error) = super::packet::validate_response(plan, &loaded.state, &result.envelope)
        {
            return fail_transition(
                &store,
                &intent,
                intent_intact,
                &loaded.manifest,
                &loaded.state,
                &prompt,
                attempt,
                started_unix,
                &result,
                before_git,
                error.to_string(),
                control_unchanged,
            );
        }
    }

    let resolved_artifacts = match resolve_artifacts(
        &loaded.manifest.project_root,
        &result.envelope.delta.artifacts_add,
    ) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            return fail_transition(
                &store,
                &intent,
                intent_intact,
                &loaded.manifest,
                &loaded.state,
                &prompt,
                attempt,
                started_unix,
                &result,
                before_git,
                error.to_string(),
                control_unchanged,
            )
        }
    };
    let should_verify = loaded.manifest.work_plan.is_some()
        || loaded.manifest.verify_every_step
        || matches!(result.envelope.outcome, StepOutcome::Complete);
    let check = if should_verify {
        match loaded.manifest.verification_command.as_ref() {
            Some(command) => match run_verification(
                command,
                &loaded.manifest.project_root,
                loaded.state.revision + 1,
                loaded.manifest.provider_timeout_seconds,
            ) {
                Ok(check) => Some(check),
                Err(error) => {
                    return fail_transition(
                        &store,
                        &intent,
                        intent_intact,
                        &loaded.manifest,
                        &loaded.state,
                        &prompt,
                        attempt,
                        started_unix,
                        &result,
                        before_git,
                        verification_failure_category(&error),
                        control_unchanged,
                    )
                }
            },
            None => None,
        }
    } else {
        None
    };
    let verification_passed = check.as_ref().map(|check| check.passed);
    if loaded.manifest.work_plan.is_some() && verification_passed == Some(false) {
        return fail_transition(
            &store,
            &intent,
            intent_intact,
            &loaded.manifest,
            &loaded.state,
            &prompt,
            attempt,
            started_unix,
            &result,
            before_git,
            "verification_failed".into(),
            control_unchanged,
        );
    }
    let after_git = git_snapshot(&loaded.manifest.project_root);
    let mut envelope = result.envelope.clone();
    if let Some(plan) = &loaded.manifest.work_plan {
        envelope = match super::packet::validate_and_derive_delta(
            plan,
            &loaded.state,
            envelope,
            verification_passed == Some(true),
        ) {
            Ok(envelope) => envelope,
            Err(error) => {
                return fail_transition(
                    &store,
                    &intent,
                    intent_intact,
                    &loaded.manifest,
                    &loaded.state,
                    &prompt,
                    attempt,
                    started_unix,
                    &result,
                    before_git,
                    error.to_string(),
                    control_unchanged,
                )
            }
        };
    }
    let accepted_task_ids = if loaded.manifest.work_plan.is_some() {
        super::packet::accepted_ids(&envelope)
    } else {
        Vec::new()
    };
    let mut transition = match apply_delta(
        &loaded.manifest,
        &loaded.state,
        envelope,
        &resolved_artifacts,
        check,
        &loaded.observation,
    ) {
        Ok(transition) => transition,
        Err(error) => {
            return fail_transition(
                &store,
                &intent,
                intent_intact,
                &loaded.manifest,
                &loaded.state,
                &prompt,
                attempt,
                started_unix,
                &result,
                before_git,
                error.to_string(),
                control_unchanged,
            )
        }
    };
    if verification_passed == Some(false) {
        let detail = transition
            .state
            .checks
            .last()
            .map(|check| check.summary.as_str())
            .unwrap_or("verification failed");
        transition.observation =
            format!("{}\nVerification failed: {detail}", transition.observation);
    }
    if transition.observation.len() > loaded.manifest.observation_budget_bytes {
        return fail_transition(
            &store,
            &intent,
            intent_intact,
            &loaded.manifest,
            &loaded.state,
            &prompt,
            attempt,
            started_unix,
            &result,
            before_git,
            "observation budget exceeded".into(),
            control_unchanged,
        );
    }
    if let Err(error) = store.save_transition(
        &transition.state,
        transition.observation.as_bytes(),
        (!transition.archive.is_empty()).then_some(&transition.archive),
    ) {
        let changed =
            !intent_intact || !control_unchanged || workspace_changed(&before_git, &after_git);
        let record = result_attempt(
            &loaded.manifest,
            &loaded.state,
            &prompt,
            attempt,
            started_unix,
            &result,
            if changed {
                AttemptOutcome::RecoveryRequired
            } else {
                AttemptOutcome::InvalidTransition
            },
            "state_commit_failed".into(),
            before_git.clone(),
            after_git.clone(),
        );
        let recovery = changed.then(|| RecoveryRecord {
            schema_version: RUNTIME_SCHEMA_VERSION,
            attempt,
            base_revision: loaded.state.revision,
            reason: "state commit failed".into(),
            before_git: before_git.clone(),
            after_git: after_git.clone(),
        });
        finalize_attempt(&store, &intent, &record, recovery.as_ref(), intent_intact)?;
        if changed {
            return Err(RuntimeError::RecoveryRequired);
        }
        return Err(error);
    }
    let record = successful_attempt(
        &loaded.manifest,
        &loaded.state,
        &transition.state,
        &prompt,
        attempt,
        started_unix,
        &result,
        before_git,
        after_git,
        accepted_task_ids,
        verification_passed,
    );
    finalize_attempt(&store, &intent, &record, None, intent_intact)?;
    let transported_input_tokens = result.usage.transported_input_tokens();
    let cache_read_ratio_basis_points = result.usage.cache_read_ratio_basis_points();
    Ok(StepReport {
        schema_version: RUNTIME_SCHEMA_VERSION,
        run_name: loaded.manifest.run_name,
        revision: transition.state.revision,
        status: transition.state.status,
        attempt,
        prompt_bytes: prompt.bytes.len(),
        stable_prompt_bytes: prompt.stable_bytes,
        dynamic_prompt_bytes: prompt.dynamic_bytes,
        usage: result.usage,
        transported_input_tokens,
        cache_read_ratio_basis_points,
        verification_passed,
    })
}

pub fn run_steps(
    reference: &RunRef,
    count: NonZeroUsize,
    provider: &dyn Provider,
) -> Result<AutoReport, RuntimeError> {
    let mut reports = Vec::with_capacity(count.get());
    for _ in 0..count.get() {
        let view = status(reference)?;
        if view.status != RunStatus::Running || view.recovery_required {
            break;
        }
        let report = step(reference, provider)?;
        let terminal = report.status != RunStatus::Running;
        reports.push(report);
        if terminal {
            break;
        }
    }
    Ok(AutoReport {
        schema_version: RUNTIME_SCHEMA_VERSION,
        run_name: reference.run_name.clone(),
        steps: reports,
    })
}

pub fn cache_probe(
    reference: &RunRef,
    provider: &dyn Provider,
) -> Result<CacheProbeReport, RuntimeError> {
    let store = RunStore::open(&reference.project_root, &reference.run_name)?;
    let _lock = store.try_lock()?;
    let loaded = store.load()?;
    if loaded.recovery.is_some() {
        return Err(RuntimeError::RecoveryRequired);
    }
    store.ensure_provider_instructions(&loaded)?;
    let prompt = compile_prompt(
        &loaded.manifest,
        &loaded.workflow,
        &loaded.state,
        &loaded.observation,
    )?;
    let before_git = git_snapshot(&loaded.manifest.project_root);
    let before_control = store.control_hash()?;
    let result = provider.probe(
        &loaded.manifest.provider,
        &prompt,
        &loaded.manifest.project_root,
        Duration::from_secs(loaded.manifest.provider_timeout_seconds),
    );
    let after_git = git_snapshot(&loaded.manifest.project_root);
    let observable_git_change =
        before_git.available && after_git.available && before_git != after_git;
    if !store
        .control_hash()
        .is_ok_and(|hash| hash == before_control)
        || observable_git_change
    {
        store.write_recovery(&RecoveryRecord {
            schema_version: RUNTIME_SCHEMA_VERSION,
            attempt: 0, // Probe diagnostics are outside the work-attempt ledger.
            base_revision: loaded.state.revision,
            reason: "cache_probe_changed_workspace".into(),
            before_git,
            after_git,
        })?;
        return Err(RuntimeError::RecoveryRequired);
    }
    let result =
        result.map_err(|error| RuntimeError::ProviderFailed(provider_failure_category(&error)))?;
    Ok(CacheProbeReport {
        schema_version: RUNTIME_SCHEMA_VERSION,
        run_name: loaded.manifest.run_name,
        provider: provider_name(&loaded.manifest.provider).into(),
        stable_prompt_sha256: prompt.stable_sha256,
        stable_prompt_bytes: prompt.stable_bytes,
        first: result.first,
        second: result.second,
        cache_hit_reported: result.cache_hit_reported,
        evidence_scope: "two immediate no-op calls with integration-free probe controls; cache reads may include the provider system prompt, not necessarily this workflow; not a production-profile or savings benchmark; probe usage is separate from step totals".into(),
    })
}

pub fn recover(reference: &RunRef, observation: &[u8]) -> Result<RunStatusView, RuntimeError> {
    let store = RunStore::open(&reference.project_root, &reference.run_name)?;
    let _lock = store.try_lock()?;
    let loaded = store.load()?;
    if observation.len() > loaded.manifest.observation_budget_bytes
        || std::str::from_utf8(observation).is_err()
    {
        return Err(RuntimeError::InvalidStoredData("observation.txt"));
    }
    let intent_result = read_attempt_intent(&store);
    let intent = match intent_result {
        Ok(intent) => intent,
        Err(_) if loaded.recovery.is_some() => None,
        Err(error) => return Err(error),
    };
    if loaded.recovery.is_none() && intent.is_none() {
        return Err(RuntimeError::InvalidState(
            "run does not require recovery".into(),
        ));
    }

    let attempts: Vec<AttemptRecord> = store.read_attempts()?;
    if let Some(recovery) = loaded.recovery.as_ref() {
        let recovered = attempts
            .iter()
            .find(|record| record.attempt == recovery.attempt)
            .ok_or(RuntimeError::InvalidStoredData("recovery.json"))?;
        if recovered.base_revision != recovery.base_revision {
            return Err(RuntimeError::InvalidStoredData("recovery.json"));
        }
        if let Some(intent) = intent.as_ref() {
            if intent.attempt != recovery.attempt
                || intent.base_revision != recovery.base_revision
                || !record_matches_intent(recovered, intent)
            {
                return Err(RuntimeError::InvalidStoredData("attempt-intent.json"));
            }
        }
    }
    let mut apply_observation = loaded.recovery.is_some();
    if let Some(intent) = intent.as_ref() {
        match attempts
            .iter()
            .find(|record| record.attempt == intent.attempt)
        {
            Some(record) => {
                if !record_matches_intent(record, intent) {
                    return Err(RuntimeError::InvalidStoredData("attempt-intent.json"));
                }
                apply_observation |= record.outcome == AttemptOutcome::RecoveryRequired
                    && loaded.state.revision == record.base_revision;
            }
            None => {
                let record =
                    interrupted_attempt(intent, git_snapshot(&loaded.manifest.project_root));
                // The immutable attempt is the recovery commit point. If a
                // later write fails, the intent remains for a deduplicating
                // retry instead of losing the interrupted invocation.
                store.write_attempt(record.attempt, &record)?;
                // State may already have committed before finalization failed.
                // Preserve that transition (including terminal status and its
                // verification revision), without inferring attempt accounting.
                apply_observation |= loaded.state.revision == intent.base_revision;
            }
        }
    }

    if apply_observation {
        let mut state = loaded.state;
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| RuntimeError::InvalidState("revision overflow".into()))?;
        state.status = RunStatus::Running;
        store.save_transition(&state, observation, None)?;
    }
    if loaded.recovery.is_some() {
        store.clear_recovery()?;
    }
    store.clear_attempt_intent()?;
    drop(_lock);
    status_for_store(&store)
}

pub fn git_snapshot(project_root: &Path) -> GitSnapshot {
    let status = Command::new("git")
        .args([
            "-C",
            &project_root.to_string_lossy(),
            "status",
            "--porcelain=v2",
            "--untracked-files=all",
            "--",
            ".",
            ":(exclude).codeunlimited/**",
        ])
        .output();
    let Ok(status) = status else {
        return GitSnapshot::default();
    };
    if !status.status.success() {
        return GitSnapshot::default();
    }
    let head = Command::new("git")
        .args([
            "-C",
            &project_root.to_string_lossy(),
            "rev-parse",
            "--verify",
            "HEAD",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string());
    GitSnapshot {
        available: true,
        head,
        status_sha256: Some(sha256(&status.stdout)),
    }
}

fn status_for_store(store: &RunStore) -> Result<RunStatusView, RuntimeError> {
    let snapshot_lock = match store.try_lock() {
        Ok(lock) => Some(lock),
        Err(RuntimeError::RunBusy) => None,
        Err(error) => return Err(error),
    };
    let busy = snapshot_lock.is_none();
    let loaded = store.load()?;
    let attempts: Vec<AttemptRecord> = store.read_attempts()?;
    let intent = read_attempt_intent(store)?;
    let prompt = inspect_prompt(
        &loaded.manifest,
        &loaded.workflow,
        &loaded.state,
        &loaded.observation,
    )?;
    let usage = aggregate_usage(&attempts);
    let transported_input_tokens = usage.transported_input_tokens();
    let cache_read_ratio_basis_points = usage.cache_read_ratio_basis_points();
    let view = RunStatusView {
        schema_version: RUNTIME_SCHEMA_VERSION,
        run_name: loaded.manifest.run_name,
        revision: loaded.state.revision,
        status: loaded.state.status,
        recovery_required: loaded.recovery.is_some() || intent.is_some(),
        busy,
        attempts: attempts.len() as u64,
        prompt_bytes: prompt.bytes.len(),
        stable_prompt_bytes: prompt.stable_bytes,
        dynamic_prompt_bytes: prompt.dynamic_bytes,
        usage,
        transported_input_tokens,
        cache_read_ratio_basis_points,
        provider: provider_status(&loaded.manifest.provider),
    };
    drop(snapshot_lock);
    Ok(view)
}

fn provider_status(provider: &ProviderConfig) -> ProviderStatus {
    let (kind, layer, capability, isolation) = match provider {
        ProviderConfig::Claude { .. } => (
            "claude",
            "subscription_cli",
            "coding_agent",
            "ephemeral provider process",
        ),
        ProviderConfig::Codex { .. } => (
            "codex",
            "subscription_cli",
            "coding_agent",
            "ephemeral provider process",
        ),
        ProviderConfig::Command { .. } => (
            "command",
            "external_command",
            "provider_defined",
            "external-process isolation",
        ),
        ProviderConfig::OpenAiApi { .. } => (
            "openai_api",
            "external_api",
            "single_turn_no_local_tools",
            "stateless HTTPS request",
        ),
        ProviderConfig::AnthropicApi { .. } => (
            "anthropic_api",
            "external_api",
            "single_turn_no_local_tools",
            "stateless HTTPS request",
        ),
    };
    ProviderStatus {
        kind: kind.into(),
        layer: layer.into(),
        capability: capability.into(),
        subscription_profile: provider
            .subscription_profile()
            .map(|profile| match profile {
                super::model::SubscriptionProfile::Standard => "standard".into(),
                super::model::SubscriptionProfile::Lean => "lean".into(),
            }),
        executable: provider
            .executable()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<api>".into()),
        args: provider.args().to_vec(),
        isolation: isolation.into(),
    }
}

fn resolve_artifacts(
    project_root: &Path,
    candidates: &[super::model::ArtifactCandidate],
) -> Result<Vec<ArtifactRef>, RuntimeError> {
    let canonical_root = fs::canonicalize(project_root)
        .map_err(|_| RuntimeError::Io("canonicalize project".into()))?;
    candidates
        .iter()
        .map(|candidate| {
            let path = canonical_root.join(&candidate.path);
            let metadata = fs::symlink_metadata(&path)
                .map_err(|_| RuntimeError::InvalidRelativePath(candidate.path.clone()))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(RuntimeError::UnsafeStorePath);
            }
            let canonical = fs::canonicalize(&path)
                .map_err(|_| RuntimeError::InvalidRelativePath(candidate.path.clone()))?;
            if !canonical.starts_with(&canonical_root) {
                return Err(RuntimeError::InvalidRelativePath(candidate.path.clone()));
            }
            let bytes =
                fs::read(canonical).map_err(|_| RuntimeError::Io("read artifact".into()))?;
            Ok(ArtifactRef {
                path: candidate.path.clone(),
                purpose: candidate.purpose.clone(),
                sha256: sha256(&bytes),
            })
        })
        .collect()
}

fn run_verification(
    command: &VerificationCommand,
    project_root: &Path,
    revision: u64,
    timeout_seconds: u64,
) -> Result<CheckResult, RuntimeError> {
    let spec = CommandSpec {
        program: command.program.clone(),
        args: command.args.iter().map(Into::into).collect(),
    };
    let output = capture_process(
        &spec,
        b"",
        project_root,
        Duration::from_secs(timeout_seconds),
    )
    .map_err(|error| RuntimeError::ProviderFailed(provider_failure_category(&error)))?;
    let mut combined = output.stdout;
    combined.extend_from_slice(&output.stderr);
    let summary = bounded_tail(&combined, 1024);
    let snapshot = git_snapshot(project_root);
    Ok(CheckResult {
        revision,
        program: command.program.to_string_lossy().into_owned(),
        args: command.args.clone(),
        passed: output.exit_code == 0,
        summary,
        workspace_sha256: snapshot.status_sha256,
    })
}

#[allow(clippy::too_many_arguments)]
fn fail_transition(
    store: &RunStore,
    intent: &AttemptIntent,
    intent_intact: bool,
    manifest: &Manifest,
    state: &CodingState,
    prompt: &CompiledPrompt,
    attempt: u64,
    started_unix: i64,
    result: &super::provider::ProviderResult,
    before_git: GitSnapshot,
    category: String,
    control_unchanged: bool,
) -> Result<StepReport, RuntimeError> {
    let after_git = git_snapshot(&manifest.project_root);
    let changed =
        !intent_intact || !control_unchanged || workspace_changed(&before_git, &after_git);
    let mut record = result_attempt(
        manifest,
        state,
        prompt,
        attempt,
        started_unix,
        result,
        if changed {
            AttemptOutcome::RecoveryRequired
        } else {
            AttemptOutcome::InvalidTransition
        },
        category.clone(),
        before_git.clone(),
        after_git.clone(),
    );
    if category == "verification_failed" {
        record.verification_passed = Some(false);
    }
    let recovery = changed.then_some(RecoveryRecord {
        schema_version: RUNTIME_SCHEMA_VERSION,
        attempt,
        base_revision: state.revision,
        reason: category,
        before_git,
        after_git,
    });
    finalize_attempt(store, intent, &record, recovery.as_ref(), intent_intact)?;
    if changed {
        Err(RuntimeError::RecoveryRequired)
    } else {
        Err(RuntimeError::ProviderFailed("invalid_transition".into()))
    }
}

fn finalize_attempt(
    store: &RunStore,
    intent: &AttemptIntent,
    record: &AttemptRecord,
    recovery: Option<&RecoveryRecord>,
    intent_intact: bool,
) -> Result<(), RuntimeError> {
    store.write_attempt(record.attempt, record)?;
    if let Some(recovery) = recovery {
        store.write_recovery(recovery)?;
    }
    if intent_intact {
        // Recheck immediately before removal so an altered intent is never
        // silently discarded after a provider had access to the project.
        if !store.attempt_intent_matches(intent)? {
            return Err(RuntimeError::RecoveryRequired);
        }
        store.clear_attempt_intent()?;
    }
    Ok(())
}

fn read_attempt_intent(store: &RunStore) -> Result<Option<AttemptIntent>, RuntimeError> {
    let intent: Option<AttemptIntent> = store.read_attempt_intent()?;
    if let Some(intent) = intent.as_ref() {
        intent.validate()?;
    }
    Ok(intent)
}

fn record_matches_intent(record: &AttemptRecord, intent: &AttemptIntent) -> bool {
    record.attempt == intent.attempt
        && record.base_revision == intent.base_revision
        && record.provider == intent.provider
        && record.configuration_sha256 == intent.configuration_sha256
        && record.prompt_sha256 == intent.prompt_sha256
        && record.stable_prompt_sha256 == intent.stable_prompt_sha256
        && record.workflow_sha256 == intent.workflow_sha256
        && record.prompt_bytes == intent.prompt_bytes
        && record.stable_prompt_bytes == intent.stable_prompt_bytes
        && record.dynamic_prompt_bytes == intent.dynamic_prompt_bytes
        && record.state_before_sha256 == intent.state_before_sha256
        && record.started_unix == intent.started_unix
        && record.before_git == intent.before_git
}

#[allow(clippy::too_many_arguments)]
fn result_attempt(
    manifest: &Manifest,
    state: &CodingState,
    prompt: &CompiledPrompt,
    attempt: u64,
    started_unix: i64,
    result: &super::provider::ProviderResult,
    outcome: AttemptOutcome,
    category: String,
    before_git: GitSnapshot,
    after_git: GitSnapshot,
) -> AttemptRecord {
    AttemptRecord {
        schema_version: RUNTIME_SCHEMA_VERSION,
        attempt,
        base_revision: state.revision,
        committed_revision: None,
        outcome,
        error_category: Some(category),
        prompt_bytes: prompt.bytes.len(),
        stable_prompt_bytes: prompt.stable_bytes,
        dynamic_prompt_bytes: prompt.dynamic_bytes,
        prompt_sha256: prompt.prompt_sha256.clone(),
        stable_prompt_sha256: prompt.stable_sha256.clone(),
        workflow_sha256: manifest.workflow_sha256.clone(),
        provider: provider_name(&manifest.provider).into(),
        started_unix,
        duration_ms: Some(result.duration_ms),
        exit_code: Some(result.exit_code),
        response_bytes: Some(result.response_bytes),
        usage: result.usage.clone(),
        state_before_sha256: state_hash(state),
        state_after_sha256: None,
        before_git,
        after_git,
        accepted_task_ids: Vec::new(),
        verification_passed: None,
        configuration_sha256: super::ledger::configuration_sha256(&manifest.provider),
    }
}

#[allow(clippy::too_many_arguments)]
fn failed_attempt(
    manifest: &Manifest,
    state: &CodingState,
    prompt: &CompiledPrompt,
    attempt: u64,
    started_unix: i64,
    outcome: AttemptOutcome,
    category: String,
    before_git: GitSnapshot,
    after_git: GitSnapshot,
) -> AttemptRecord {
    AttemptRecord {
        schema_version: RUNTIME_SCHEMA_VERSION,
        attempt,
        base_revision: state.revision,
        committed_revision: None,
        outcome,
        error_category: Some(category),
        prompt_bytes: prompt.bytes.len(),
        stable_prompt_bytes: prompt.stable_bytes,
        dynamic_prompt_bytes: prompt.dynamic_bytes,
        prompt_sha256: prompt.prompt_sha256.clone(),
        stable_prompt_sha256: prompt.stable_sha256.clone(),
        workflow_sha256: manifest.workflow_sha256.clone(),
        provider: provider_name(&manifest.provider).into(),
        started_unix,
        duration_ms: None,
        exit_code: None,
        response_bytes: None,
        usage: ProviderUsage::default(),
        state_before_sha256: state_hash(state),
        state_after_sha256: None,
        before_git,
        after_git,
        accepted_task_ids: Vec::new(),
        verification_passed: None,
        configuration_sha256: super::ledger::configuration_sha256(&manifest.provider),
    }
}

fn interrupted_attempt(intent: &AttemptIntent, after_git: GitSnapshot) -> AttemptRecord {
    AttemptRecord {
        schema_version: RUNTIME_SCHEMA_VERSION,
        attempt: intent.attempt,
        base_revision: intent.base_revision,
        committed_revision: None,
        outcome: AttemptOutcome::RecoveryRequired,
        error_category: Some("interrupted".into()),
        prompt_bytes: intent.prompt_bytes,
        stable_prompt_bytes: intent.stable_prompt_bytes,
        dynamic_prompt_bytes: intent.dynamic_prompt_bytes,
        prompt_sha256: intent.prompt_sha256.clone(),
        stable_prompt_sha256: intent.stable_prompt_sha256.clone(),
        workflow_sha256: intent.workflow_sha256.clone(),
        provider: intent.provider.clone(),
        started_unix: intent.started_unix,
        duration_ms: None,
        exit_code: None,
        response_bytes: None,
        usage: ProviderUsage::default(),
        state_before_sha256: intent.state_before_sha256.clone(),
        state_after_sha256: None,
        before_git: intent.before_git.clone(),
        after_git,
        accepted_task_ids: Vec::new(),
        verification_passed: None,
        configuration_sha256: intent.configuration_sha256.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
fn successful_attempt(
    manifest: &Manifest,
    before_state: &CodingState,
    after_state: &CodingState,
    prompt: &CompiledPrompt,
    attempt: u64,
    started_unix: i64,
    result: &super::provider::ProviderResult,
    before_git: GitSnapshot,
    after_git: GitSnapshot,
    accepted_task_ids: Vec<String>,
    verification_passed: Option<bool>,
) -> AttemptRecord {
    AttemptRecord {
        schema_version: RUNTIME_SCHEMA_VERSION,
        attempt,
        base_revision: before_state.revision,
        committed_revision: Some(after_state.revision),
        outcome: AttemptOutcome::Succeeded,
        error_category: None,
        prompt_bytes: prompt.bytes.len(),
        stable_prompt_bytes: prompt.stable_bytes,
        dynamic_prompt_bytes: prompt.dynamic_bytes,
        prompt_sha256: prompt.prompt_sha256.clone(),
        stable_prompt_sha256: prompt.stable_sha256.clone(),
        workflow_sha256: manifest.workflow_sha256.clone(),
        provider: provider_name(&manifest.provider).into(),
        started_unix,
        duration_ms: Some(result.duration_ms),
        exit_code: Some(result.exit_code),
        response_bytes: Some(result.response_bytes),
        usage: result.usage.clone(),
        state_before_sha256: state_hash(before_state),
        state_after_sha256: Some(state_hash(after_state)),
        before_git,
        after_git,
        accepted_task_ids,
        verification_passed,
        configuration_sha256: super::ledger::configuration_sha256(&manifest.provider),
    }
}

fn aggregate_usage(attempts: &[AttemptRecord]) -> ProviderUsage {
    fn sum_if_complete(
        attempts: &[AttemptRecord],
        get: impl Fn(&ProviderUsage) -> Option<u64>,
    ) -> Option<u64> {
        if attempts.is_empty() {
            return Some(0);
        }
        attempts.iter().try_fold(0_u64, |sum, attempt| {
            get(&attempt.usage).map(|value| sum.saturating_add(value))
        })
    }
    let input_token_semantics = attempts
        .first()
        .map(|attempt| attempt.usage.input_token_semantics)
        .filter(|first| {
            attempts
                .iter()
                .all(|attempt| attempt.usage.input_token_semantics == *first)
        })
        .unwrap_or(InputTokenSemantics::Unknown);
    ProviderUsage {
        input_token_semantics,
        input_tokens: sum_if_complete(attempts, |usage| usage.input_tokens),
        uncached_input_tokens: sum_if_complete(attempts, |usage| usage.uncached_input_tokens),
        cache_read_input_tokens: sum_if_complete(attempts, |usage| usage.cache_read_input_tokens),
        cache_write_input_tokens: sum_if_complete(attempts, |usage| usage.cache_write_input_tokens),
        cache_write_5m_input_tokens: sum_if_complete(attempts, |usage| {
            usage.cache_write_5m_input_tokens
        }),
        cache_write_1h_input_tokens: sum_if_complete(attempts, |usage| {
            usage.cache_write_1h_input_tokens
        }),
        output_tokens: sum_if_complete(attempts, |usage| usage.output_tokens),
    }
}

fn workspace_changed(before: &GitSnapshot, after: &GitSnapshot) -> bool {
    !before.available || !after.available || before != after
}

fn state_hash(state: &CodingState) -> String {
    sha256(&serde_json::to_vec(state).expect("runtime state is serializable"))
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn provider_name(provider: &ProviderConfig) -> &'static str {
    match provider {
        ProviderConfig::Claude { .. } => "claude",
        ProviderConfig::Codex { .. } => "codex",
        ProviderConfig::Command { .. } => "command",
        ProviderConfig::OpenAiApi { .. } => "openai_api",
        ProviderConfig::AnthropicApi { .. } => "anthropic_api",
    }
}

fn provider_failure_category(error: &ProviderFailure) -> String {
    match error {
        ProviderFailure::MissingExecutable => "missing_executable",
        ProviderFailure::Spawn => "spawn",
        ProviderFailure::Stdin => "stdin",
        ProviderFailure::Timeout => "timeout",
        ProviderFailure::Exit(_) => "exit",
        ProviderFailure::OutputTooLarge => "output_too_large",
        ProviderFailure::InvalidOutput | ProviderFailure::InvalidOutputWithUsage(_) => {
            "invalid_output"
        }
        ProviderFailure::InvalidConfiguration(_) => "invalid_configuration",
        ProviderFailure::TemporaryFile => "temporary_file",
        ProviderFailure::MissingCredential => "missing_credential",
        ProviderFailure::Http => "http",
        ProviderFailure::ProbeUnsupported => "probe_unsupported",
    }
    .into()
}

fn verification_failure_category(error: &RuntimeError) -> String {
    match error {
        RuntimeError::ProviderFailed(category) => format!("verification_{category}"),
        _ => "verification_error".into(),
    }
}

fn bounded_tail(bytes: &[u8], max_bytes: usize) -> String {
    let start = bytes.len().saturating_sub(max_bytes);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::num::NonZeroUsize;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use tempfile::TempDir;

    use crate::runtime::model::{ProviderConfig, RunStatus, RuntimeError, VerificationCommand};
    use crate::runtime::provider::ProcessProvider;

    use super::{
        cache_probe, git_snapshot, init_run, recover, render_next_prompt, run_steps, status, step,
        InitRequest, RunRef,
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

    fn provider_args(mode: &str) -> Vec<String> {
        vec![
            fixture().to_string_lossy().into_owned(),
            "--mode".into(),
            mode.into(),
            "--revision-from-prompt".into(),
        ]
    }

    fn setup(mode: &str) -> (TempDir, RunRef) {
        let project = TempDir::new().expect("project");
        let workflow = project.path().join("workflow.md");
        fs::write(&workflow, "# Workflow\nDo one bounded increment.\n").unwrap();
        let request = InitRequest::new(
            project.path().to_path_buf(),
            "feature-x".into(),
            workflow,
            "Implement feature X".into(),
            ProviderConfig::Command {
                executable: python(),
                args: provider_args(mode),
            },
        );
        init_run(request).expect("initialize run");
        let reference = RunRef::new(project.path().to_path_buf(), "feature-x".into());
        (project, reference)
    }

    #[test]
    fn init_status_and_prompt_never_invoke_provider() {
        let project = TempDir::new().expect("project");
        let workflow = project.path().join("workflow.md");
        let capture = project.path().join("provider-called");
        fs::write(&workflow, "# Workflow\n").unwrap();
        let mut args = provider_args("success");
        args.extend(["--capture".into(), capture.to_string_lossy().into_owned()]);
        let reference = RunRef::new(project.path().to_path_buf(), "feature-x".into());
        init_run(InitRequest::new(
            project.path().to_path_buf(),
            "feature-x".into(),
            workflow,
            "Implement X".into(),
            ProviderConfig::Command {
                executable: python(),
                args,
            },
        ))
        .expect("init");

        assert_eq!(status(&reference).unwrap().revision, 0);
        assert!(render_next_prompt(&reference).unwrap().bytes.len() > 100);
        assert!(!capture.exists());
    }

    #[test]
    fn successful_step_commits_one_revision_and_attempt() {
        let (_project, reference) = setup("success");

        let report = step(&reference, &ProcessProvider).expect("step");

        assert_eq!(report.revision, 1);
        assert_eq!(report.attempt, 1);
        assert_eq!(report.status, RunStatus::Running);
        let view = status(&reference).unwrap();
        assert_eq!(view.revision, 1);
        assert_eq!(view.attempts, 1);
    }

    #[test]
    fn cache_probe_reports_provider_counters_without_advancing_state() {
        struct ProbeOnly(Option<PathBuf>);
        impl crate::runtime::provider::Provider for ProbeOnly {
            fn run(
                &self,
                _config: &ProviderConfig,
                _prompt: &crate::runtime::prompt::CompiledPrompt,
                _project_root: &Path,
                _timeout: std::time::Duration,
            ) -> Result<
                crate::runtime::provider::ProviderResult,
                crate::runtime::provider::ProviderFailure,
            > {
                unreachable!("cache probe must not execute a state step")
            }

            fn probe(
                &self,
                _config: &ProviderConfig,
                _prompt: &crate::runtime::prompt::CompiledPrompt,
                _project_root: &Path,
                _timeout: std::time::Duration,
            ) -> Result<
                crate::runtime::provider::ProviderProbeResult,
                crate::runtime::provider::ProviderFailure,
            > {
                if let Some(path) = &self.0 {
                    fs::write(path, "changed by failed probe").unwrap();
                    return Err(crate::runtime::provider::ProviderFailure::InvalidOutput);
                }
                let sample = |cached| crate::runtime::provider::ProviderProbeSample {
                    usage: crate::runtime::provider::ProviderUsage {
                        input_token_semantics:
                            crate::runtime::provider::InputTokenSemantics::TotalIncludesCache,
                        input_tokens: Some(100),
                        cache_read_input_tokens: Some(cached),
                        ..Default::default()
                    },
                    duration_ms: 1,
                };
                Ok(crate::runtime::provider::ProviderProbeResult::new(
                    sample(0),
                    sample(80),
                ))
            }
        }
        let (_project, reference) = setup("success");

        let report = cache_probe(&reference, &ProbeOnly(None)).expect("probe report");

        assert_eq!(report.cache_hit_reported, Some(true));
        assert_eq!(report.second.usage.cache_read_input_tokens, Some(80));
        assert_eq!(status(&reference).unwrap().revision, 0);
        assert_eq!(status(&reference).unwrap().attempts, 0);
        let observation = reference
            .project_root
            .join(".codeunlimited/runs")
            .join(&reference.run_name)
            .join("observation.txt");
        assert_eq!(
            cache_probe(&reference, &ProbeOnly(Some(observation))),
            Err(RuntimeError::RecoveryRequired)
        );
        assert!(status(&reference).unwrap().recovery_required);
        assert_eq!(
            step(&reference, &ProcessProvider),
            Err(RuntimeError::RecoveryRequired)
        );
    }

    #[test]
    fn auto_is_finite_and_uses_fresh_revision_each_step() {
        let (_project, reference) = setup("success");

        let report = run_steps(&reference, NonZeroUsize::new(3).unwrap(), &ProcessProvider)
            .expect("bounded auto run");

        assert_eq!(report.steps.len(), 3);
        assert_eq!(report.steps.last().unwrap().revision, 3);
        assert_eq!(status(&reference).unwrap().attempts, 3);
    }

    #[test]
    fn requested_completion_requires_runtime_check_or_explicit_override() {
        let (project, reference) = setup("success");
        rewrite_provider_args(
            project.path(),
            "feature-x",
            &provider_args_with_outcome("complete"),
            true,
            None,
        );
        let report = step(&reference, &ProcessProvider).expect("allowed completion");
        assert_eq!(report.status, RunStatus::Complete);

        let (project, reference) = setup("success");
        rewrite_provider_args(
            project.path(),
            "feature-x",
            &provider_args_with_outcome("complete"),
            false,
            Some(VerificationCommand {
                program: python(),
                args: vec![
                    fixture().to_string_lossy().into_owned(),
                    "--mode".into(),
                    "failure".into(),
                ],
            }),
        );
        let report = step(&reference, &ProcessProvider).expect("failed check is next state");
        assert_eq!(report.status, RunStatus::Running);
        assert_eq!(report.verification_passed, Some(false));
    }

    #[test]
    fn successful_runtime_verification_is_required_evidence_for_completion() {
        let (project, reference) = setup("success");
        rewrite_provider_args(
            project.path(),
            "feature-x",
            &provider_args_with_outcome("complete"),
            false,
            Some(VerificationCommand {
                program: python(),
                args: vec![
                    fixture().to_string_lossy().into_owned(),
                    "--mode".into(),
                    "success".into(),
                ],
            }),
        );

        let report = step(&reference, &ProcessProvider).expect("verified completion");
        assert_eq!(report.status, RunStatus::Complete);
        assert_eq!(report.verification_passed, Some(true));
    }

    #[test]
    fn verification_launch_failure_is_recorded_as_an_attempt() {
        let (project, reference) = setup("success");
        initialize_git(project.path());
        rewrite_provider_args(
            project.path(),
            "feature-x",
            &provider_args_with_outcome("complete"),
            false,
            Some(VerificationCommand {
                program: project.path().join("missing-verifier"),
                args: Vec::new(),
            }),
        );

        assert!(matches!(
            step(&reference, &ProcessProvider),
            Err(RuntimeError::ProviderFailed(_))
        ));
        assert_eq!(status(&reference).unwrap().attempts, 1);
        assert_eq!(status(&reference).unwrap().revision, 0);
    }

    #[test]
    fn invalid_output_without_repo_change_preserves_state_and_allows_bounded_retry() {
        let (project, reference) = setup("invalid");
        initialize_git(project.path());
        let state_path = project
            .path()
            .join(".codeunlimited/runs/feature-x/state.json");
        let before = fs::read(&state_path).unwrap();

        assert!(matches!(
            step(&reference, &ProcessProvider),
            Err(RuntimeError::ProviderFailed(_))
        ));
        assert_eq!(fs::read(&state_path).unwrap(), before);
        assert_eq!(status(&reference).unwrap().attempts, 1);
    }

    #[test]
    fn git_unavailable_makes_a_failed_attempt_require_recovery() {
        let (project, reference) = setup("invalid");
        assert!(!git_snapshot(project.path()).available);

        assert_eq!(
            step(&reference, &ProcessProvider).unwrap_err(),
            RuntimeError::RecoveryRequired
        );
        assert!(status(&reference).unwrap().recovery_required);
    }

    #[test]
    fn detached_head_is_still_a_valid_git_snapshot() {
        let (project, _reference) = setup("success");
        initialize_git(project.path());
        assert!(Command::new("git")
            .args(["checkout", "--detach", "-q"])
            .current_dir(project.path())
            .status()
            .unwrap()
            .success());

        let snapshot = git_snapshot(project.path());
        assert!(snapshot.available);
        assert!(snapshot.head.is_some());
        assert!(snapshot.status_sha256.is_some());
    }

    #[test]
    fn changed_repo_plus_invalid_output_requires_explicit_recovery() {
        let (project, reference) = setup("invalid");
        initialize_git(project.path());
        let changed = project.path().join("changed.txt");
        rewrite_provider_args(
            project.path(),
            "feature-x",
            &[
                fixture().to_string_lossy().into_owned(),
                "--mode".into(),
                "invalid".into(),
                "--change".into(),
                changed.to_string_lossy().into_owned(),
            ],
            false,
            None,
        );

        assert_eq!(
            step(&reference, &ProcessProvider).unwrap_err(),
            RuntimeError::RecoveryRequired
        );
        assert!(changed.exists());
        assert!(status(&reference).unwrap().recovery_required);
        assert_eq!(
            step(&reference, &ProcessProvider).unwrap_err(),
            RuntimeError::RecoveryRequired
        );

        recover(&reference, b"Accepted changed.txt after manual inspection")
            .expect("explicit recovery");
        assert!(!status(&reference).unwrap().recovery_required);
        assert_eq!(status(&reference).unwrap().revision, 1);
        assert!(changed.exists());
    }

    #[test]
    fn provider_cannot_silently_modify_runtime_control_files() {
        let (project, reference) = setup("success");
        initialize_git(project.path());
        let state_path = project
            .path()
            .join(".codeunlimited/runs/feature-x/state.json");
        rewrite_provider_args(
            project.path(),
            "feature-x",
            &[
                fixture().to_string_lossy().into_owned(),
                "--mode".into(),
                "success".into(),
                "--revision-from-prompt".into(),
                "--change".into(),
                state_path.to_string_lossy().into_owned(),
            ],
            false,
            None,
        );

        assert_eq!(
            step(&reference, &ProcessProvider).unwrap_err(),
            RuntimeError::RecoveryRequired
        );
        assert!(project
            .path()
            .join(".codeunlimited/runs/feature-x/recovery.json")
            .exists());
    }

    #[test]
    fn attempts_per_revision_are_hard_limited() {
        let (project, reference) = setup("invalid");
        initialize_git(project.path());
        let manifest_path = project
            .path()
            .join(".codeunlimited/runs/feature-x/manifest.json");
        let mut manifest: crate::runtime::model::Manifest =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest.max_attempts_per_revision = 1;
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        assert!(matches!(
            step(&reference, &ProcessProvider),
            Err(RuntimeError::ProviderFailed(_))
        ));
        assert_eq!(
            step(&reference, &ProcessProvider).unwrap_err(),
            RuntimeError::AttemptLimit
        );
    }

    fn provider_args_with_outcome(outcome: &str) -> Vec<String> {
        let mut args = provider_args("success");
        args.extend(["--outcome".into(), outcome.into()]);
        args
    }

    fn rewrite_provider_args(
        project: &Path,
        name: &str,
        args: &[String],
        allow_unverified: bool,
        verification: Option<VerificationCommand>,
    ) {
        let path = project.join(format!(".codeunlimited/runs/{name}/manifest.json"));
        let mut manifest: crate::runtime::model::Manifest =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest.provider = ProviderConfig::Command {
            executable: python(),
            args: args.to_vec(),
        };
        manifest.allow_unverified_completion = allow_unverified;
        manifest.verification_command = verification;
        fs::write(path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    }

    fn initialize_git(project: &Path) {
        assert!(Command::new("git")
            .args(["init", "-q"])
            .current_dir(project)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["add", "."])
            .current_dir(project)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-qm",
                "fixture",
            ])
            .current_dir(project)
            .status()
            .unwrap()
            .success());
    }
}
