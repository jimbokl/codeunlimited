use sha2::{Digest, Sha256};

use super::model::{CodingState, Manifest, RunStatus, RuntimeError};
use super::validate::{validate_manifest, validate_state};

pub const STEP_ENVELOPE_SCHEMA_JSON: &str = r#"{"type":"object","additionalProperties":false,"required":["schema_version","base_revision","outcome","summary","delta"],"properties":{"schema_version":{"type":"integer","const":1},"base_revision":{"type":"integer","minimum":0},"outcome":{"type":"string","enum":["continue","complete","blocked"]},"summary":{"type":"string","minLength":1,"maxLength":1024},"delta":{"type":"object","additionalProperties":false,"properties":{"focus":{"type":["string","null"]},"memory_summary":{"type":["string","null"]},"queue_replace":{"type":["array","null"],"items":{"type":"object","additionalProperties":false,"required":["id","task"],"properties":{"id":{"type":"string"},"task":{"type":"string"}}}},"completed_add":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","result"],"properties":{"id":{"type":"string"},"result":{"type":"string"}}}},"decisions_add":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","decision"],"properties":{"id":{"type":"string"},"decision":{"type":"string"}}}},"blockers_replace":{"type":["array","null"],"items":{"type":"object","additionalProperties":false,"required":["id","blocker"],"properties":{"id":{"type":"string"},"blocker":{"type":"string"}}}},"active_files_replace":{"type":["array","null"],"items":{"type":"string"}},"artifacts_add":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["path","purpose"],"properties":{"path":{"type":"string"},"purpose":{"type":"string"}}}},"epistemic_upsert":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","claim","status","evidence"],"properties":{"id":{"type":"string"},"claim":{"type":"string"},"status":{"type":"string","enum":["hypothesis","observed","verified","disputed"]},"evidence":{"type":"array","items":{"oneOf":[{"type":"object","additionalProperties":false,"required":["kind"],"properties":{"kind":{"const":"step"}}},{"type":"object","additionalProperties":false,"required":["kind","sha256"],"properties":{"kind":{"const":"observation"},"sha256":{"type":"string"}}},{"type":"object","additionalProperties":false,"required":["kind","revision"],"properties":{"kind":{"const":"check"},"revision":{"type":"integer","minimum":0}}},{"type":"object","additionalProperties":false,"required":["kind","path"],"properties":{"kind":{"const":"artifact"},"path":{"type":"string"}}}]}}}},"epistemic_retire":{"type":"array","items":{"type":"string"}}}}}}}"#;

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

/// Provider strict-output subset. The durable delta still permits omitted fields;
/// on the wire nullable replacements use null and additive collections use [].
pub fn strict_step_schema() -> serde_json::Value {
    fn normalize(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(union) = object.remove("oneOf") {
                    object.insert("anyOf".into(), union);
                }
                if let Some(properties) = object
                    .get("properties")
                    .and_then(serde_json::Value::as_object)
                {
                    let required = properties
                        .keys()
                        .cloned()
                        .map(serde_json::Value::String)
                        .collect();
                    object.insert("required".into(), serde_json::Value::Array(required));
                }
                for child in object.values_mut() {
                    normalize(child);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    normalize(value);
                }
            }
            _ => {}
        }
    }
    let mut schema =
        serde_json::from_str(STEP_ENVELOPE_SCHEMA_JSON).expect("static envelope schema");
    normalize(&mut schema);
    schema
}

/// Inspection must remain available when a legacy run's configured prompt
/// budget is smaller than a newer compiler's output. Invocation uses the strict
/// compiler below and continues to enforce the original limit.
pub fn inspect_prompt(
    manifest: &Manifest,
    workflow: &[u8],
    state: &CodingState,
    observation: &[u8],
) -> Result<CompiledPrompt, RuntimeError> {
    validate_manifest(manifest)?;
    let mut inspection = manifest.clone();
    inspection.prompt_budget_bytes = super::model::MAX_PROMPT_BUDGET_BYTES;
    compile_prompt(&inspection, workflow, state, observation)
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

    let mut stable = format!(
        "{RUNTIME_CONTRACT}\nThe provider-specific structured-output schema is authoritative and is supplied out of band.\n\nIMMUTABLE_WORKFLOW sha256={}\n{}\nEND_IMMUTABLE_WORKFLOW\n\nIMMUTABLE_OBJECTIVE\n{}\nEND_IMMUTABLE_OBJECTIVE\n",
        manifest.workflow_sha256, workflow, objective
    );
    let mut dynamic = format!(
        "CURRENT_STATE\n{state_json}\nEND_CURRENT_STATE\n\nLATEST_OBSERVATION sha256={:x}\n{observation}\nEND_LATEST_OBSERVATION\n\nPerform one bounded increment now. Base the response on revision {}.\n",
        Sha256::digest(observation.as_bytes()),
        state.revision
    );
    let selected_json = if let Some(plan) = &manifest.work_plan {
        stable.push_str(
            "\nThis is a managed work plan. Complete only tasks in SELECTED_PACKET, in dependency order. Return one structured response for the packet. completed_add must be a nonempty prefix of the selected IDs unless the outcome is explicitly blocked. Scope paths are planning metadata, not a filesystem sandbox or expanded authorization. The runtime freezes only the verification program and argv, not mutable test contents or the executable environment, and runs that command before accepting work.\n",
        );
        let selected = if state.status == RunStatus::Running {
            super::packet::select_tasks(plan, state)?
        } else {
            Vec::new()
        };
        let json = serde_json::to_string(&selected)
            .map_err(|_| RuntimeError::InvalidStoredData("work_plan"))?;
        dynamic.push_str(&format!("\nSELECTED_PACKET\n{json}\nEND_SELECTED_PACKET\n"));
        Some(json)
    } else {
        None
    };
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
    let mut codex_bootstrap = format!(
        "Load the bounded runtime inputs with three separate reads, in this exact order: `{relative_run}/provider-instructions.md` first, `{relative_run}/state.json` second, and `{relative_run}/observation.txt` third. Follow the first file as immutable instructions, treat the latter two as the only changing input, perform one bounded increment, then return the required structured response. Do not combine the three reads.\n"
    );
    if let Some(json) = selected_json {
        codex_bootstrap.push_str(&format!(
            "After those ordered reads, use exactly this packet:\nSELECTED_PACKET\n{json}\nEND_SELECTED_PACKET\n"
        ));
    }
    let codex_bootstrap = codex_bootstrap.into_bytes();
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

    use crate::runtime::model::{
        CodingState, Manifest, PlannedTask, ProviderConfig, RuntimeError, TaskRisk, WorkPlan,
    };

    use super::compile_prompt;

    const WORKFLOW: &[u8] = b"# Workflow\r\nDo one bounded task.\r\n";

    fn project_root() -> PathBuf {
        // Absolute on every platform: "/tmp/project" is not absolute on Windows.
        if cfg!(windows) {
            PathBuf::from(r"C:\tmp\project")
        } else {
            PathBuf::from("/tmp/project")
        }
    }

    fn manifest() -> Manifest {
        let mut value = Manifest::for_test(
            "feature-x",
            project_root(),
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

    #[test]
    fn managed_packet_is_identical_in_dynamic_and_codex_inputs() {
        let mut manifest = manifest();
        manifest.verification_command = Some(crate::runtime::model::VerificationCommand {
            program: "verify".into(),
            args: vec!["--all".into()],
        });
        manifest.work_plan = Some(WorkPlan {
            schema_version: 1,
            max_packet_tasks: 2,
            tasks: vec![
                PlannedTask {
                    id: "a".into(),
                    task: "First".into(),
                    group: "g".into(),
                    depends_on: vec![],
                    scope: vec!["src/a.rs".into()],
                    risk: TaskRisk::Low,
                },
                PlannedTask {
                    id: "b".into(),
                    task: "Second".into(),
                    group: "g".into(),
                    depends_on: vec!["a".into()],
                    scope: vec!["src/a.rs".into()],
                    risk: TaskRisk::Low,
                },
            ],
        });
        let mut state = CodingState::initial();
        state.queue = crate::runtime::packet::initial_queue(manifest.work_plan.as_ref().unwrap());
        let prompt = compile_prompt(&manifest, WORKFLOW, &state, b"").unwrap();
        let marker = "SELECTED_PACKET\n[{\"id\":\"a\",\"task\":\"First\",\"group\":\"g\",\"depends_on\":[],\"scope\":[\"src/a.rs\"],\"risk\":\"low\"},{\"id\":\"b\",\"task\":\"Second\",\"group\":\"g\",\"depends_on\":[\"a\"],\"scope\":[\"src/a.rs\"],\"risk\":\"low\"}]\nEND_SELECTED_PACKET";

        assert!(String::from_utf8(prompt.dynamic.clone())
            .unwrap()
            .contains(marker));
        assert!(String::from_utf8(prompt.bytes.clone())
            .unwrap()
            .contains(marker));
        let bootstrap = String::from_utf8(prompt.codex_bootstrap).unwrap();
        assert!(bootstrap.contains("observation.txt` third"));
        assert!(bootstrap.contains(marker));
    }
}
