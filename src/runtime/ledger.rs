use std::collections::BTreeSet;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::engine::{AttemptIntent, AttemptOutcome, AttemptRecord};
use super::model::{Manifest, ProviderConfig, RuntimeError, RUNTIME_SCHEMA_VERSION};
use super::provider::{InputTokenSemantics, ProviderUsage};

enum NormalizedInput {
    Known(u64),
    Unavailable,
    Overflow,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LedgerReport {
    pub schema_version: u64,
    pub run_name: String,
    pub provider: String,
    pub configuration_sha256: String,
    pub attempts: Vec<LedgerAttempt>,
    pub pending_attempt: Option<LedgerPendingAttempt>,
    pub coverage: LedgerCoverage,
    pub max_total_tokens: Option<u64>,
    pub cap_reached: bool,
    pub cap_overshoot_tokens: Option<u64>,
    pub accepted_task_count: u64,
    pub tokens_per_accepted_task: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LedgerAttempt {
    pub attempt: u64,
    pub base_revision: u64,
    pub committed_revision: Option<u64>,
    pub outcome: String,
    pub error_category: Option<String>,
    pub provider: String,
    pub configuration_sha256: String,
    pub started_unix: i64,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub response_bytes: Option<usize>,
    pub usage: ProviderUsage,
    pub transported_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub accepted_task_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LedgerPendingAttempt {
    pub attempt: u64,
    pub base_revision: u64,
    pub provider: String,
    pub configuration_sha256: String,
    pub started_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LedgerCoverage {
    pub scope: &'static str,
    pub attempt_count: u64,
    pub complete_attempts: u64,
    pub incomplete_attempts: u64,
    pub observed_input_tokens: Option<u64>,
    pub observed_output_tokens: Option<u64>,
    pub observed_total_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub complete: bool,
    pub zero_attempts: bool,
    pub busy: bool,
    pub pending: bool,
    pub overflowed: bool,
    pub cache_probes_excluded: bool,
}

pub(crate) fn build_report(
    manifest: &Manifest,
    records: &[AttemptRecord],
    pending: Option<&AttemptIntent>,
    busy: bool,
) -> LedgerReport {
    let configuration_sha256 = configuration_sha256(&manifest.provider);
    let mut attempts = Vec::with_capacity(records.len());
    let mut complete_attempts = 0_u64;
    let mut incomplete_attempts = 0_u64;
    let mut observed_input = Some(0_u64);
    let mut observed_output = Some(0_u64);
    let mut overflowed = false;
    let mut accepted = BTreeSet::new();

    for record in records {
        let transported_input_tokens = match normalized_input(&record.usage) {
            NormalizedInput::Known(value) => Some(value),
            NormalizedInput::Unavailable => None,
            NormalizedInput::Overflow => {
                overflowed = true;
                observed_input = None;
                None
            }
        };
        let output_tokens = record.usage.output_tokens;
        let total_tokens = match (transported_input_tokens, output_tokens) {
            (Some(input), Some(output)) => match input.checked_add(output) {
                Some(total) => Some(total),
                None => {
                    overflowed = true;
                    None
                }
            },
            _ => None,
        };
        if transported_input_tokens.is_some() && output_tokens.is_some() {
            complete_attempts += 1;
        } else {
            incomplete_attempts += 1;
        }
        if let Some(value) = transported_input_tokens {
            observed_input = observed_input.and_then(|sum| sum.checked_add(value));
            overflowed |= observed_input.is_none();
        }
        if let Some(value) = output_tokens {
            observed_output = observed_output.and_then(|sum| sum.checked_add(value));
            overflowed |= observed_output.is_none();
        }
        if record.outcome == AttemptOutcome::Succeeded {
            accepted.extend(record.accepted_task_ids.iter().cloned());
        }
        attempts.push(LedgerAttempt {
            attempt: record.attempt,
            base_revision: record.base_revision,
            committed_revision: record.committed_revision,
            outcome: record.outcome.as_str().into(),
            error_category: record.error_category.clone(),
            provider: record.provider.clone(),
            configuration_sha256: if record.configuration_sha256.is_empty() {
                configuration_sha256.clone()
            } else {
                record.configuration_sha256.clone()
            },
            started_unix: record.started_unix,
            duration_ms: record.duration_ms,
            exit_code: record.exit_code,
            response_bytes: record.response_bytes,
            usage: record.usage.clone(),
            transported_input_tokens,
            output_tokens,
            total_tokens,
            accepted_task_ids: record.accepted_task_ids.clone(),
        });
    }

    let pending_is_unmatched = pending.is_some_and(|intent| {
        !records
            .iter()
            .any(|record| record.attempt == intent.attempt)
    });
    if pending_is_unmatched {
        incomplete_attempts += 1;
    }
    let attempt_count = records.len() as u64 + u64::from(pending_is_unmatched);
    let observed_total_tokens = match (observed_input, observed_output) {
        (Some(input), Some(output)) => match input.checked_add(output) {
            Some(total) => Some(total),
            None => {
                overflowed = true;
                None
            }
        },
        _ => None,
    };
    let zero_attempts = attempt_count == 0;
    let complete =
        !zero_attempts && incomplete_attempts == 0 && !overflowed && !busy && pending.is_none();
    let total_tokens = complete.then_some(observed_total_tokens).flatten();
    let coverage = LedgerCoverage {
        scope: "reported_worker_attempt_counters",
        attempt_count,
        complete_attempts,
        incomplete_attempts,
        observed_input_tokens: observed_input,
        observed_output_tokens: observed_output,
        observed_total_tokens,
        total_tokens,
        complete,
        zero_attempts,
        busy,
        pending: pending.is_some(),
        overflowed,
        cache_probes_excluded: true,
    };
    let cap_reached = manifest
        .max_total_tokens
        .zip(coverage.observed_total_tokens)
        .is_some_and(|(limit, observed)| observed >= limit);
    let cap_overshoot_tokens = manifest
        .max_total_tokens
        .zip(coverage.observed_total_tokens)
        .and_then(|(limit, observed)| observed.checked_sub(limit));
    let accepted_task_count = accepted.len() as u64;
    let tokens_per_accepted_task = coverage.total_tokens.and_then(|total| {
        (accepted_task_count != 0).then(|| total as f64 / accepted_task_count as f64)
    });

    LedgerReport {
        schema_version: RUNTIME_SCHEMA_VERSION,
        run_name: manifest.run_name.clone(),
        provider: provider_name(&manifest.provider).into(),
        configuration_sha256,
        attempts,
        pending_attempt: pending.map(|intent| LedgerPendingAttempt {
            attempt: intent.attempt,
            base_revision: intent.base_revision,
            provider: intent.provider.clone(),
            configuration_sha256: intent.configuration_sha256.clone(),
            started_unix: intent.started_unix,
        }),
        coverage,
        max_total_tokens: manifest.max_total_tokens,
        cap_reached,
        cap_overshoot_tokens,
        accepted_task_count,
        tokens_per_accepted_task,
    }
}

fn normalized_input(usage: &ProviderUsage) -> NormalizedInput {
    match usage.input_token_semantics {
        InputTokenSemantics::TotalIncludesCache => usage
            .input_tokens
            .map_or(NormalizedInput::Unavailable, NormalizedInput::Known),
        InputTokenSemantics::UncachedOnly => {
            let Some(uncached) = usage.uncached_input_tokens.or(usage.input_tokens) else {
                return NormalizedInput::Unavailable;
            };
            let (Some(cache_read), Some(cache_write)) = (
                usage.cache_read_input_tokens,
                usage.cache_write_input_tokens,
            ) else {
                return NormalizedInput::Unavailable;
            };
            uncached
                .checked_add(cache_read)
                .and_then(|value| value.checked_add(cache_write))
                .map_or(NormalizedInput::Overflow, NormalizedInput::Known)
        }
        InputTokenSemantics::Unknown => NormalizedInput::Unavailable,
    }
}

pub(crate) fn cap_admission_error(
    manifest: &Manifest,
    records: &[AttemptRecord],
) -> Option<RuntimeError> {
    let limit = manifest.max_total_tokens?;
    if records.is_empty() {
        return None;
    }
    let report = build_report(manifest, records, None, false);
    match report.coverage.total_tokens {
        Some(observed) if observed >= limit => {
            Some(RuntimeError::TokenCapReached { limit, observed })
        }
        Some(_) => None,
        None => Some(RuntimeError::TokenCapUsageUnknown),
    }
}

pub(crate) fn configuration_sha256(provider: &ProviderConfig) -> String {
    let bytes = serde_json::to_vec(provider).expect("provider configuration is serializable");
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
