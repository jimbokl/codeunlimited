use sha2::{Digest, Sha256};

use super::model::{CodingState, Manifest, RuntimeError};
use super::validate::{validate_manifest, validate_state};

pub const STEP_ENVELOPE_SCHEMA_JSON: &str = r#"{"type":"object","additionalProperties":false,"required":["schema_version","base_revision","outcome","summary","delta"],"properties":{"schema_version":{"type":"integer","const":1},"base_revision":{"type":"integer","minimum":0},"outcome":{"type":"string","enum":["continue","complete","blocked"]},"summary":{"type":"string","minLength":1,"maxLength":1024},"delta":{"type":"object","additionalProperties":false,"properties":{"focus":{"type":["string","null"]},"memory_summary":{"type":["string","null"]},"queue_replace":{"type":["array","null"],"items":{"type":"object","additionalProperties":false,"required":["id","task"],"properties":{"id":{"type":"string"},"task":{"type":"string"}}}},"completed_add":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","result"],"properties":{"id":{"type":"string"},"result":{"type":"string"}}}},"decisions_add":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","decision"],"properties":{"id":{"type":"string"},"decision":{"type":"string"}}}},"blockers_replace":{"type":["array","null"],"items":{"type":"object","additionalProperties":false,"required":["id","blocker"],"properties":{"id":{"type":"string"},"blocker":{"type":"string"}}}},"active_files_replace":{"type":["array","null"],"items":{"type":"string"}},"artifacts_add":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["path","purpose"],"properties":{"path":{"type":"string"},"purpose":{"type":"string"}}}},"epistemic_upsert":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","claim","status","evidence"],"properties":{"id":{"type":"string"},"claim":{"type":"string"},"status":{"type":"string","enum":["hypothesis","observed","verified","disputed"]},"evidence":{"type":"array","items":{"oneOf":[{"type":"object","additionalProperties":false,"required":["kind"],"properties":{"kind":{"const":"step"}}},{"type":"object","additionalProperties":false,"required":["kind","sha256"],"properties":{"kind":{"const":"observation"},"sha256":{"type":"string"}}},{"type":"object","additionalProperties":false,"required":["kind","revision"],"properties":{"kind":{"const":"check"},"revision":{"type":"integer","minimum":0}}},{"type":"object","additionalProperties":false,"required":["kind","path"],"properties":{"kind":{"const":"artifact"},"path":{"type":"string"}}}]}}}},"epistemic_retire":{"type":"array","items":{"type":"string"}}}}}}"#;

const RUNTIME_CONTRACT: &str = "codeunlimited state runtime v1\n\
You are an ephemeral coding worker. You have no prior conversation.\n\
Use the repository tools available to complete exactly one bounded work increment.\n\
Treat CURRENT_STATE as the sole memory of earlier increments.\n\
Classify durable claims as hypothesis, observed, verified, or disputed.\n\
Observed claims must cite the latest observation digest or kind=step for this increment; verified claims must cite a passed check or hashed artifact.\n\
Dispute verified knowledge before retiring it. Preserve knowledge needed to keep CURRENT_STATE sufficient.\n\
Do not claim checks you did not run. Return exactly one JSON object matching STEP_ENVELOPE_SCHEMA.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledPrompt {
    /// Self-contained prompt for generic command providers.
    pub bytes: Vec<u8>,
    /// Immutable provider instructions for cache-aligned transports.
    pub stable: Vec<u8>,
    /// Revision-specific state and observation.
    pub dynamic: Vec<u8>,
    /// Constant Codex bootstrap which orders stable and dynamic file reads.
    pub codex_bootstrap: Vec<u8>,
    pub instructions_path: std::path::PathBuf,
    pub stable_bytes: usize,
    pub dynamic_bytes: usize,
    pub stable_sha256: String,
    pub dynamic_sha256: String,
    pub prompt_sha256: String,
}

pub fn compile_prompt(
    manifest: &Manifest,
    workflow: &[u8],
    state: &CodingState,
    observation: &[u8],
) -> Result<CompiledPrompt, RuntimeError> {
    validate_manifest(manifest)?;
    validate_state(manifest, state)?;
    validate_input(
        "workflow",
        "workflow.md",
        workflow,
        manifest.workflow_budget_bytes,
    )?;
    validate_input(
        "observation",
        "observation.txt",
        observation,
        manifest.observation_budget_bytes,
    )?;
    let actual_workflow_hash = format!("{:x}", Sha256::digest(workflow));
    if actual_workflow_hash != manifest.workflow_sha256 {
        return Err(RuntimeError::WorkflowHashMismatch);
    }

    let workflow = normalize_newlines(std::str::from_utf8(workflow).expect("validated UTF-8"));
    let objective = normalize_newlines(&manifest.objective);
    let observation =
        normalize_newlines(std::str::from_utf8(observation).expect("validated observation UTF-8"));
    let state_json =
        serde_json::to_string(state).map_err(|_| RuntimeError::InvalidStoredData("state.json"))?;

    let stable = format!(
        "{RUNTIME_CONTRACT}\nThe provider-specific structured-output schema is authoritative and is supplied out of band.\n\nIMMUTABLE_WORKFLOW sha256={}\n{}\nEND_IMMUTABLE_WORKFLOW\n\nIMMUTABLE_OBJECTIVE\n{}\nEND_IMMUTABLE_OBJECTIVE\n",
        manifest.workflow_sha256, workflow, objective
    );
    let dynamic = format!(
        "CURRENT_STATE\n{state_json}\nEND_CURRENT_STATE\n\nLATEST_OBSERVATION sha256={:x}\n{observation}\nEND_LATEST_OBSERVATION\n\nPerform one bounded increment now. Base the response on revision {}.\n",
        Sha256::digest(observation.as_bytes()),
        state.revision
    );
    let stable = stable.into_bytes();
    let dynamic = dynamic.into_bytes();
    let stable_bytes = stable.len();
    let dynamic_bytes = dynamic.len();
    let mut bytes = stable.clone();
    bytes.extend_from_slice(b"\nSTEP_ENVELOPE_SCHEMA\n");
    bytes.extend_from_slice(STEP_ENVELOPE_SCHEMA_JSON.as_bytes());
    bytes.extend_from_slice(b"\n\n");
    bytes.extend_from_slice(&dynamic);
    if bytes.len() > manifest.prompt_budget_bytes {
        return Err(RuntimeError::FieldTooLarge {
            field: "prompt",
            actual: bytes.len(),
            allowed: manifest.prompt_budget_bytes,
        });
    }
    let stable_sha256 = format!("{:x}", Sha256::digest(&stable));
    let dynamic_sha256 = format!("{:x}", Sha256::digest(&dynamic));
    let prompt_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let run_dir = manifest
        .project_root
        .join(".codeunlimited")
        .join("runs")
        .join(&manifest.run_name);
    let relative_run = format!(".codeunlimited/runs/{}", manifest.run_name);
    let codex_bootstrap = format!(
        "Load the bounded runtime inputs with three separate reads, in this exact order: `{relative_run}/provider-instructions.md` first, `{relative_run}/state.json` second, and `{relative_run}/observation.txt` third. Follow the first file as immutable instructions, treat the latter two as the only changing input, perform one bounded increment, then return the required structured response. Do not combine the three reads.\n"
    )
    .into_bytes();
    Ok(CompiledPrompt {
        bytes,
        stable,
        dynamic,
        codex_bootstrap,
        instructions_path: run_dir.join("provider-instructions.md"),
        stable_bytes,
        dynamic_bytes,
        stable_sha256,
        dynamic_sha256,
        prompt_sha256,
    })
}

fn validate_input(
    field: &'static str,
    label: &'static str,
    bytes: &[u8],
    allowed: usize,
) -> Result<(), RuntimeError> {
    if bytes.len() > allowed {
        return Err(RuntimeError::FieldTooLarge {
            field,
            actual: bytes.len(),
            allowed,
        });
    }
    std::str::from_utf8(bytes)
        .map(|_| ())
        .map_err(|_| RuntimeError::InvalidStoredData(label))
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sha2::{Digest, Sha256};

    use crate::runtime::model::{CodingState, Manifest, ProviderConfig, RuntimeError};

    use super::compile_prompt;

    const WORKFLOW: &[u8] = b"# Workflow\r\nDo one bounded task.\r\n";

    fn manifest() -> Manifest {
        let mut value = Manifest::for_test(
            "feature-x",
            PathBuf::from("/tmp/project"),
            "Implement feature X",
            ProviderConfig::Command {
                executable: PathBuf::from("fixture-driver"),
                args: vec![],
            },
        );
        value.workflow_sha256 = format!("{:x}", Sha256::digest(WORKFLOW));
        value
    }

    #[test]
    fn prompt_is_deterministic_minified_and_normalized() {
        let first = compile_prompt(
            &manifest(),
            WORKFLOW,
            &CodingState::initial(),
            b"latest\r\n",
        )
        .expect("compiled prompt");
        let second = compile_prompt(
            &manifest(),
            WORKFLOW,
            &CodingState::initial(),
            b"latest\r\n",
        )
        .expect("same prompt");

        assert_eq!(first, second);
        assert!(!first.bytes.contains(&b'\r'));
        let text = String::from_utf8(first.bytes).unwrap();
        assert!(text.contains("# Workflow\nDo one bounded task.\n"));
        assert!(text.contains("\"revision\":0"));
        assert!(!text.contains("\n  \"revision\""));
    }

    #[test]
    fn stable_prefix_is_identical_across_state_revisions() {
        let first = compile_prompt(&manifest(), WORKFLOW, &CodingState::initial(), b"first")
            .expect("first prompt");
        let mut next_state = CodingState::initial();
        next_state.revision = 1;
        next_state.focus = "tests".into();
        let second =
            compile_prompt(&manifest(), WORKFLOW, &next_state, b"second").expect("second prompt");

        assert_eq!(first.stable_sha256, second.stable_sha256);
        assert_eq!(first.stable_bytes, second.stable_bytes);
        assert_ne!(first.prompt_sha256, second.prompt_sha256);
        assert_ne!(first.bytes, second.bytes);
    }

    #[test]
    fn channels_keep_stable_workflow_out_of_dynamic_input() {
        let prompt = compile_prompt(&manifest(), WORKFLOW, &CodingState::initial(), b"latest")
            .expect("split prompt");
        let stable = String::from_utf8(prompt.stable.clone()).unwrap();
        let dynamic = String::from_utf8(prompt.dynamic.clone()).unwrap();
        let combined = String::from_utf8(prompt.bytes.clone()).unwrap();

        assert!(stable.contains("# Workflow\nDo one bounded task."));
        assert!(stable.contains("Implement feature X"));
        assert!(!stable.contains("STEP_ENVELOPE_SCHEMA\n{"));
        assert!(!dynamic.contains("# Workflow"));
        assert!(!dynamic.contains("Implement feature X"));
        assert!(!dynamic.contains("STEP_ENVELOPE_SCHEMA"));
        assert!(dynamic.contains("\"revision\":0"));
        assert!(combined.contains("STEP_ENVELOPE_SCHEMA\n{"));
        assert!(combined.ends_with(&dynamic));
    }

    #[test]
    fn stable_and_dynamic_hashes_describe_exact_channel_bytes() {
        let prompt = compile_prompt(&manifest(), WORKFLOW, &CodingState::initial(), b"latest")
            .expect("split prompt");

        assert_eq!(prompt.stable_bytes, prompt.stable.len());
        assert_eq!(prompt.dynamic_bytes, prompt.dynamic.len());
        assert_eq!(
            prompt.stable_sha256,
            format!("{:x}", Sha256::digest(&prompt.stable))
        );
        assert_eq!(
            prompt.dynamic_sha256,
            format!("{:x}", Sha256::digest(&prompt.dynamic))
        );
    }

    #[test]
    fn prompt_budget_accepts_exact_boundary_and_rejects_one_byte_less() {
        let compiled = compile_prompt(&manifest(), WORKFLOW, &CodingState::initial(), b"latest")
            .expect("measure fixture");
        let mut exact = manifest();
        exact.prompt_budget_bytes = compiled.bytes.len();
        assert!(compile_prompt(&exact, WORKFLOW, &CodingState::initial(), b"latest").is_ok());

        exact.prompt_budget_bytes -= 1;
        assert_eq!(
            compile_prompt(&exact, WORKFLOW, &CodingState::initial(), b"latest").unwrap_err(),
            RuntimeError::FieldTooLarge {
                field: "prompt",
                actual: compiled.bytes.len(),
                allowed: compiled.bytes.len() - 1,
            }
        );
    }

    #[test]
    fn old_transcript_has_no_input_channel() {
        let prompt = compile_prompt(&manifest(), WORKFLOW, &CodingState::initial(), b"latest")
            .expect("bounded prompt");
        assert!(!String::from_utf8_lossy(&prompt.bytes).contains("OLD_TRANSCRIPT_SENTINEL"));
    }

    #[test]
    fn prompt_exposes_epistemic_rules_and_latest_observation_digest() {
        let observation = b"new test evidence";
        let prompt = compile_prompt(&manifest(), WORKFLOW, &CodingState::initial(), observation)
            .expect("bounded prompt");
        let text = String::from_utf8(prompt.bytes).unwrap();

        assert!(text.contains("hypothesis, observed, verified, or disputed"));
        assert!(text.contains("Dispute verified knowledge before retiring it"));
        assert!(text.contains(&format!(
            "LATEST_OBSERVATION sha256={:x}",
            Sha256::digest(observation)
        )));
    }

    #[test]
    fn invalid_or_oversized_inputs_fail_before_prompt_creation() {
        let mut too_small = manifest();
        too_small.workflow_budget_bytes = WORKFLOW.len() - 1;
        assert!(matches!(
            compile_prompt(&too_small, WORKFLOW, &CodingState::initial(), b"latest"),
            Err(RuntimeError::FieldTooLarge {
                field: "workflow",
                ..
            })
        ));
        assert_eq!(
            compile_prompt(&manifest(), &[0xff], &CodingState::initial(), b"latest").unwrap_err(),
            RuntimeError::InvalidStoredData("workflow.md")
        );
        assert_eq!(
            compile_prompt(&manifest(), WORKFLOW, &CodingState::initial(), &[0xff]).unwrap_err(),
            RuntimeError::InvalidStoredData("observation.txt")
        );
    }
}
