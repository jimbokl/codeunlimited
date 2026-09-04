use sha2::{Digest, Sha256};

use super::model::{CodingState, Manifest, RuntimeError};
use super::validate::{validate_manifest, validate_state};

pub const STEP_ENVELOPE_SCHEMA_JSON: &str = r#"{"type":"object","additionalProperties":false,"required":["schema_version","base_revision","outcome","summary","delta"],"properties":{"schema_version":{"type":"integer","const":1},"base_revision":{"type":"integer","minimum":0},"outcome":{"type":"string","enum":["continue","complete","blocked"]},"summary":{"type":"string","minLength":1,"maxLength":1024},"delta":{"type":"object","additionalProperties":false,"properties":{"focus":{"type":["string","null"]},"memory_summary":{"type":["string","null"]},"queue_replace":{"type":["array","null"],"items":{"type":"object","additionalProperties":false,"required":["id","task"],"properties":{"id":{"type":"string"},"task":{"type":"string"}}}},"completed_add":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","result"],"properties":{"id":{"type":"string"},"result":{"type":"string"}}}},"decisions_add":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","decision"],"properties":{"id":{"type":"string"},"decision":{"type":"string"}}}},"blockers_replace":{"type":["array","null"],"items":{"type":"object","additionalProperties":false,"required":["id","blocker"],"properties":{"id":{"type":"string"},"blocker":{"type":"string"}}}},"active_files_replace":{"type":["array","null"],"items":{"type":"string"}},"artifacts_add":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["path","purpose"],"properties":{"path":{"type":"string"},"purpose":{"type":"string"}}}}}}}}"#;

const RUNTIME_CONTRACT: &str = "codeunlimited state runtime v1\n\
You are an ephemeral coding worker. You have no prior conversation.\n\
Use the repository tools available to complete exactly one bounded work increment.\n\
Treat CURRENT_STATE as the sole memory of earlier increments.\n\
Do not claim checks you did not run. Return exactly one JSON object matching STEP_ENVELOPE_SCHEMA.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledPrompt {
    pub bytes: Vec<u8>,
    pub stable_bytes: usize,
    pub dynamic_bytes: usize,
    pub stable_sha256: String,
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
        "{RUNTIME_CONTRACT}\nSTEP_ENVELOPE_SCHEMA\n{STEP_ENVELOPE_SCHEMA_JSON}\n\nIMMUTABLE_WORKFLOW sha256={}\n{}\nEND_IMMUTABLE_WORKFLOW\n\nIMMUTABLE_OBJECTIVE\n{}\nEND_IMMUTABLE_OBJECTIVE\n\n",
        manifest.workflow_sha256, workflow, objective
    );
    let dynamic = format!(
        "CURRENT_STATE\n{state_json}\nEND_CURRENT_STATE\n\nLATEST_OBSERVATION\n{observation}\nEND_LATEST_OBSERVATION\n\nPerform one bounded increment now. Base the response on revision {}.\n",
        state.revision
    );
    let stable_bytes = stable.len();
    let dynamic_bytes = dynamic.len();
    let mut bytes = stable.into_bytes();
    bytes.extend_from_slice(dynamic.as_bytes());
    if bytes.len() > manifest.prompt_budget_bytes {
        return Err(RuntimeError::FieldTooLarge {
            field: "prompt",
            actual: bytes.len(),
            allowed: manifest.prompt_budget_bytes,
        });
    }
    let stable_sha256 = format!("{:x}", Sha256::digest(&bytes[..stable_bytes]));
    let prompt_sha256 = format!("{:x}", Sha256::digest(&bytes));
    Ok(CompiledPrompt {
        bytes,
        stable_bytes,
        dynamic_bytes,
        stable_sha256,
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
