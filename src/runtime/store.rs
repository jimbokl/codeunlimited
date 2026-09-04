use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::safeio::{atomic_create, atomic_write, reject_symlink};

use super::model::{
    ArchiveBatch, CodingState, Manifest, RecoveryRecord, RuntimeError, RUNTIME_SCHEMA_VERSION,
};
use super::prompt::{compile_prompt, inspect_prompt};
use super::validate::{validate_manifest, validate_run_name, validate_state};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPaths {
    pub project_root: PathBuf,
    pub store_root: PathBuf,
    pub runs_root: PathBuf,
    pub run: PathBuf,
    pub manifest: PathBuf,
    pub workflow: PathBuf,
    pub provider_instructions: PathBuf,
    pub state: PathBuf,
    pub observation: PathBuf,
    pub lock: PathBuf,
    pub attempts: PathBuf,
    pub archive: PathBuf,
    pub recovery: PathBuf,
}

impl RunPaths {
    pub fn new(project_root: &Path, name: &str) -> Result<Self, RuntimeError> {
        validate_run_name(name)?;
        let project_root = fs::canonicalize(project_root).map_err(|_| io_error("canonicalize"))?;
        let store_root = project_root.join(".codeunlimited");
        let runs_root = store_root.join("runs");
        let run = runs_root.join(name);
        Ok(Self {
            project_root,
            store_root,
            runs_root,
            manifest: run.join("manifest.json"),
            workflow: run.join("workflow.md"),
            provider_instructions: run.join("provider-instructions.md"),
            state: run.join("state.json"),
            observation: run.join("observation.txt"),
            lock: run.join("lock"),
            attempts: run.join("attempts"),
            archive: run.join("archive"),
            recovery: run.join("recovery.json"),
            run,
        })
    }
}

#[derive(Debug)]
pub struct RunStore {
    paths: RunPaths,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedRun {
    pub manifest: Manifest,
    pub workflow: Vec<u8>,
    pub provider_instructions: Vec<u8>,
    pub state: CodingState,
    pub observation: Vec<u8>,
    pub recovery: Option<RecoveryRecord>,
}

#[derive(Debug)]
pub struct RunLock {
    file: File,
}

impl Drop for RunLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

impl RunStore {
    pub fn open(project_root: &Path, name: &str) -> Result<Self, RuntimeError> {
        let paths = RunPaths::new(project_root, name)?;
        ensure_existing_directory(&paths.store_root)?;
        ensure_existing_directory(&paths.runs_root)?;
        ensure_existing_directory(&paths.run).map_err(|error| match error {
            RuntimeError::Io(_) => RuntimeError::RunNotFound,
            other => other,
        })?;
        ensure_existing_directory(&paths.attempts)?;
        ensure_existing_directory(&paths.archive)?;
        Ok(Self { paths })
    }

    pub fn create(
        project_root: &Path,
        name: &str,
        manifest: &Manifest,
        workflow: &[u8],
        state: &CodingState,
    ) -> Result<Self, RuntimeError> {
        let paths = RunPaths::new(project_root, name)?;
        validate_manifest(manifest)?;
        validate_state(manifest, state)?;
        if manifest.run_name != name {
            return Err(RuntimeError::InvalidManifest(
                "run_name does not match store path".into(),
            ));
        }
        validate_workflow(manifest, workflow)?;
        ensure_directory(&paths.store_root)?;
        ensure_directory(&paths.runs_root)?;
        match fs::create_dir(&paths.run) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(RuntimeError::RunExists)
            }
            Err(_) => return Err(io_error("create run directory")),
        }

        let result = (|| {
            ensure_directory(&paths.attempts)?;
            ensure_directory(&paths.archive)?;
            atomic_create(&paths.workflow, workflow).map_err(|_| io_error("create workflow"))?;
            let instructions = compile_prompt(manifest, workflow, state, b"")?.stable;
            atomic_create(&paths.provider_instructions, &instructions)
                .map_err(|_| io_error("create provider instructions"))?;
            write_new_json(&paths.manifest, manifest, "manifest.json")?;
            write_new_json(&paths.state, state, "state.json")?;
            atomic_create(&paths.observation, b"").map_err(|_| io_error("create observation"))?;
            atomic_create(&paths.lock, b"").map_err(|_| io_error("create lock"))?;
            Ok(())
        })();
        if let Err(error) = result {
            cleanup_partial_create(&paths);
            return Err(error);
        }
        Ok(Self { paths })
    }

    pub fn paths(&self) -> &RunPaths {
        &self.paths
    }

    pub fn load(&self) -> Result<LoadedRun, RuntimeError> {
        check_run_paths(&self.paths)?;
        let manifest: Manifest = read_json(&self.paths.manifest, "manifest.json")?;
        validate_manifest(&manifest)?;
        let workflow = read_bytes(&self.paths.workflow)?;
        validate_workflow(&manifest, &workflow)?;
        let state: CodingState = read_json(&self.paths.state, "state.json")?;
        validate_state(&manifest, &state)?;
        let observation = read_bytes(&self.paths.observation)?;
        validate_observation(&manifest, &observation)?;
        let expected_instructions =
            inspect_prompt(&manifest, &workflow, &state, &observation)?.stable;
        let provider_instructions = if self.paths.provider_instructions.exists() {
            let bytes = read_bytes(&self.paths.provider_instructions)?;
            if bytes != expected_instructions {
                return Err(RuntimeError::InstructionHashMismatch);
            }
            bytes
        } else {
            expected_instructions
        };
        let recovery = if self.paths.recovery.exists() {
            let value: RecoveryRecord = read_json(&self.paths.recovery, "recovery.json")?;
            if value.schema_version != RUNTIME_SCHEMA_VERSION {
                return Err(RuntimeError::InvalidStoredData("recovery.json"));
            }
            Some(value)
        } else {
            None
        };
        Ok(LoadedRun {
            manifest,
            workflow,
            provider_instructions,
            state,
            observation,
            recovery,
        })
    }

    pub fn try_lock(&self) -> Result<RunLock, RuntimeError> {
        reject_symlink(&self.paths.lock).map_err(|_| RuntimeError::UnsafeStorePath)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.paths.lock)
            .map_err(|_| io_error("open run lock"))?;
        file.try_lock_exclusive().map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                RuntimeError::RunBusy
            } else {
                io_error("lock run")
            }
        })?;
        Ok(RunLock { file })
    }

    pub fn save_transition(
        &self,
        state: &CodingState,
        observation: &[u8],
        archive: Option<&ArchiveBatch>,
    ) -> Result<(), RuntimeError> {
        let loaded = self.load()?;
        validate_state(&loaded.manifest, state)?;
        validate_observation(&loaded.manifest, observation)?;
        if let Some(archive) = archive.filter(|archive| !archive.is_empty()) {
            self.write_archive(state.revision, archive)?;
        }
        let previous_observation = loaded.observation;
        atomic_write(&self.paths.observation, observation)
            .map_err(|_| io_error("write observation"))?;
        if let Err(error) = write_json(&self.paths.state, state, "state.json") {
            let _ = atomic_write(&self.paths.observation, &previous_observation);
            return Err(error);
        }
        Ok(())
    }

    pub fn write_attempt<T: Serialize>(&self, attempt: u64, value: &T) -> Result<(), RuntimeError> {
        let path = self.paths.attempts.join(format!("{attempt:08}.json"));
        match write_new_json(&path, value, "attempt") {
            Err(RuntimeError::Io(_)) if path.exists() => Err(RuntimeError::AttemptExists(attempt)),
            result => result,
        }
    }

    pub fn read_attempts<T: DeserializeOwned>(&self) -> Result<Vec<T>, RuntimeError> {
        ensure_existing_directory(&self.paths.attempts)?;
        let mut paths = fs::read_dir(&self.paths.attempts)
            .map_err(|_| io_error("list attempts"))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| io_error("list attempts"))?;
        paths.sort();
        paths
            .into_iter()
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .map(|path| read_json(&path, "attempt"))
            .collect()
    }

    pub fn control_hash(&self) -> Result<String, RuntimeError> {
        let mut hasher = Sha256::new();
        for (name, path) in [
            ("manifest", &self.paths.manifest),
            ("workflow", &self.paths.workflow),
            ("state", &self.paths.state),
            ("observation", &self.paths.observation),
        ] {
            hasher.update(name.as_bytes());
            hasher.update(read_bytes(path)?);
        }
        if self.paths.provider_instructions.exists() {
            hasher.update(b"provider_instructions");
            hasher.update(read_bytes(&self.paths.provider_instructions)?);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Materialize the v2.1 immutable instruction snapshot for a legacy v2.0
    /// run. Read-only operations intentionally do not mutate old runs.
    pub fn ensure_provider_instructions(&self, loaded: &LoadedRun) -> Result<(), RuntimeError> {
        if self.paths.provider_instructions.exists() {
            let current = read_bytes(&self.paths.provider_instructions)?;
            return (current == loaded.provider_instructions)
                .then_some(())
                .ok_or(RuntimeError::InstructionHashMismatch);
        }
        atomic_create(
            &self.paths.provider_instructions,
            &loaded.provider_instructions,
        )
        .map_err(|_| io_error("create provider instructions"))
    }

    pub fn write_recovery(&self, recovery: &RecoveryRecord) -> Result<(), RuntimeError> {
        write_json(&self.paths.recovery, recovery, "recovery.json")
    }

    pub fn clear_recovery(&self) -> Result<(), RuntimeError> {
        reject_symlink(&self.paths.recovery).map_err(|_| RuntimeError::UnsafeStorePath)?;
        match fs::remove_file(&self.paths.recovery) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(io_error("remove recovery")),
        }
    }

    fn write_archive<T: Serialize>(&self, revision: u64, value: &T) -> Result<(), RuntimeError> {
        let path = self.paths.archive.join(format!("{revision:08}.json"));
        write_new_json(&path, value, "archive")
    }
}

fn validate_workflow(manifest: &Manifest, workflow: &[u8]) -> Result<(), RuntimeError> {
    if workflow.len() > manifest.workflow_budget_bytes || std::str::from_utf8(workflow).is_err() {
        return Err(RuntimeError::InvalidStoredData("workflow.md"));
    }
    let digest = format!("{:x}", Sha256::digest(workflow));
    if digest != manifest.workflow_sha256 {
        return Err(RuntimeError::WorkflowHashMismatch);
    }
    Ok(())
}

fn validate_observation(manifest: &Manifest, bytes: &[u8]) -> Result<(), RuntimeError> {
    if bytes.len() > manifest.observation_budget_bytes || std::str::from_utf8(bytes).is_err() {
        return Err(RuntimeError::InvalidStoredData("observation.txt"));
    }
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<(), RuntimeError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(RuntimeError::UnsafeStorePath),
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(RuntimeError::UnsafeStorePath),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| io_error("create store directory"))
        }
        Err(_) => Err(io_error("inspect store directory")),
    }
}

fn ensure_existing_directory(path: &Path) -> Result<(), RuntimeError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(RuntimeError::UnsafeStorePath),
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(RuntimeError::UnsafeStorePath),
        Err(_) => Err(io_error("open run directory")),
    }
}

fn check_run_paths(paths: &RunPaths) -> Result<(), RuntimeError> {
    for directory in [
        &paths.store_root,
        &paths.runs_root,
        &paths.run,
        &paths.attempts,
        &paths.archive,
    ] {
        ensure_existing_directory(directory)?;
    }
    for file in [
        &paths.manifest,
        &paths.workflow,
        &paths.state,
        &paths.observation,
        &paths.lock,
    ] {
        reject_symlink(file).map_err(|_| RuntimeError::UnsafeStorePath)?;
    }
    if paths.provider_instructions.exists() {
        reject_symlink(&paths.provider_instructions).map_err(|_| RuntimeError::UnsafeStorePath)?;
    }
    if paths.recovery.exists() {
        reject_symlink(&paths.recovery).map_err(|_| RuntimeError::UnsafeStorePath)?;
    }
    Ok(())
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, RuntimeError> {
    reject_symlink(path).map_err(|_| RuntimeError::UnsafeStorePath)?;
    fs::read(path).map_err(|_| io_error("read runtime file"))
}

fn read_json<T: DeserializeOwned>(path: &Path, label: &'static str) -> Result<T, RuntimeError> {
    let bytes = read_bytes(path)?;
    serde_json::from_slice(&bytes).map_err(|_| RuntimeError::InvalidStoredData(label))
}

fn write_json<T: Serialize>(
    path: &Path,
    value: &T,
    label: &'static str,
) -> Result<(), RuntimeError> {
    let mut bytes =
        serde_json::to_vec_pretty(value).map_err(|_| RuntimeError::InvalidStoredData(label))?;
    bytes.push(b'\n');
    atomic_write(path, &bytes).map_err(|_| io_error("write runtime JSON"))
}

fn write_new_json<T: Serialize>(
    path: &Path,
    value: &T,
    label: &'static str,
) -> Result<(), RuntimeError> {
    let mut bytes =
        serde_json::to_vec_pretty(value).map_err(|_| RuntimeError::InvalidStoredData(label))?;
    bytes.push(b'\n');
    atomic_create(path, &bytes).map_err(|_| io_error("create runtime JSON"))
}

fn cleanup_partial_create(paths: &RunPaths) {
    for file in [
        &paths.lock,
        &paths.observation,
        &paths.state,
        &paths.manifest,
        &paths.provider_instructions,
        &paths.workflow,
    ] {
        let _ = fs::remove_file(file);
    }
    let _ = fs::remove_dir(&paths.archive);
    let _ = fs::remove_dir(&paths.attempts);
    let _ = fs::remove_dir(&paths.run);
}

fn io_error(operation: &str) -> RuntimeError {
    RuntimeError::Io(operation.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use sha2::{Digest, Sha256};
    use tempfile::TempDir;

    use crate::runtime::model::{
        ArchiveBatch, CodingState, CompletedItem, Manifest, ProviderConfig, RecoveryRecord,
        RuntimeError,
    };

    use super::RunStore;

    fn fixture() -> (TempDir, Manifest, Vec<u8>, CodingState) {
        let project = TempDir::new().expect("project");
        let workflow = b"# Workflow\nDo one bounded task.\n".to_vec();
        let mut manifest = Manifest::for_test(
            "feature-x",
            project.path().to_path_buf(),
            "Implement feature X",
            ProviderConfig::Command {
                executable: PathBuf::from("fixture-driver"),
                args: vec![],
            },
        );
        manifest.workflow_sha256 = format!("{:x}", Sha256::digest(&workflow));
        (project, manifest, workflow, CodingState::initial())
    }

    #[test]
    fn create_is_exclusive_and_strict_load_round_trips() {
        let (project, manifest, workflow, state) = fixture();
        let store = RunStore::create(project.path(), "feature-x", &manifest, &workflow, &state)
            .expect("create run");

        let loaded = store.load().expect("load run");
        assert_eq!(loaded.manifest, manifest);
        assert_eq!(loaded.workflow, workflow);
        assert_eq!(loaded.state, state);
        assert!(loaded.observation.is_empty());
        assert!(loaded.recovery.is_none());
        assert_eq!(
            loaded.provider_instructions,
            crate::runtime::prompt::compile_prompt(&manifest, &workflow, &state, b"")
                .unwrap()
                .stable
        );
        assert_eq!(
            RunStore::create(project.path(), "feature-x", &manifest, &workflow, &state)
                .unwrap_err(),
            RuntimeError::RunExists
        );
    }

    #[test]
    fn instruction_hash_drift_is_rejected() {
        let (project, manifest, workflow, state) = fixture();
        let store = RunStore::create(project.path(), "feature-x", &manifest, &workflow, &state)
            .expect("create run");
        fs::write(&store.paths().provider_instructions, b"changed").unwrap();

        assert_eq!(
            store.load().unwrap_err(),
            RuntimeError::InstructionHashMismatch
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_provider_instructions_are_rejected() {
        use std::os::unix::fs::symlink;

        let (project, manifest, workflow, state) = fixture();
        let store = RunStore::create(project.path(), "feature-x", &manifest, &workflow, &state)
            .expect("create run");
        let outside = project.path().join("outside-instructions.md");
        fs::write(&outside, b"keep").unwrap();
        fs::remove_file(&store.paths().provider_instructions).unwrap();
        symlink(&outside, &store.paths().provider_instructions).unwrap();

        assert_eq!(store.load().unwrap_err(), RuntimeError::UnsafeStorePath);
        assert_eq!(fs::read(outside).unwrap(), b"keep");
    }

    #[test]
    fn corrupt_state_is_preserved_and_rejected() {
        let (project, manifest, workflow, state) = fixture();
        let store = RunStore::create(project.path(), "feature-x", &manifest, &workflow, &state)
            .expect("create run");
        fs::write(&store.paths().state, b"{broken").expect("corrupt state");

        assert_eq!(
            store.load().unwrap_err(),
            RuntimeError::InvalidStoredData("state.json")
        );
        assert_eq!(fs::read(&store.paths().state).unwrap(), b"{broken");
    }

    #[test]
    fn second_lock_is_busy_without_waiting() {
        let (project, manifest, workflow, state) = fixture();
        let store = RunStore::create(project.path(), "feature-x", &manifest, &workflow, &state)
            .expect("create run");
        let first = store.try_lock().expect("first lock");
        assert_eq!(store.try_lock().unwrap_err(), RuntimeError::RunBusy);
        drop(first);
        store.try_lock().expect("lock released");
    }

    #[test]
    fn transition_updates_state_and_observation() {
        let (project, manifest, workflow, state) = fixture();
        let store = RunStore::create(project.path(), "feature-x", &manifest, &workflow, &state)
            .expect("create run");
        let mut next = state;
        next.revision = 1;
        next.focus = "tests".into();

        store
            .save_transition(&next, b"parser complete", None)
            .expect("save transition");
        let loaded = store.load().expect("load transition");
        assert_eq!(loaded.state, next);
        assert_eq!(loaded.observation, b"parser complete");
    }

    #[test]
    fn attempts_are_immutable() {
        let (project, manifest, workflow, state) = fixture();
        let store = RunStore::create(project.path(), "feature-x", &manifest, &workflow, &state)
            .expect("create run");
        let record = serde_json::json!({"schema_version": 1, "attempt": 1});

        store.write_attempt(1, &record).expect("first attempt");
        assert_eq!(
            store.write_attempt(1, &record).unwrap_err(),
            RuntimeError::AttemptExists(1)
        );
    }

    #[test]
    fn recovery_round_trip_and_clear_are_explicit() {
        let (project, manifest, workflow, state) = fixture();
        let store = RunStore::create(project.path(), "feature-x", &manifest, &workflow, &state)
            .expect("create run");
        let recovery = RecoveryRecord::for_test(1, 0);

        store.write_recovery(&recovery).expect("write recovery");
        assert_eq!(store.load().unwrap().recovery, Some(recovery));
        store.clear_recovery().expect("clear recovery");
        assert!(store.load().unwrap().recovery.is_none());
    }

    #[test]
    fn archive_collision_preserves_existing_archive_state_and_observation() {
        let (project, manifest, workflow, state) = fixture();
        let store = RunStore::create(project.path(), "feature-x", &manifest, &workflow, &state)
            .expect("create run");
        let collision = store.paths().archive.join("00000001.json");
        fs::write(&collision, b"keep-existing-archive").expect("collision fixture");
        let mut next = state.clone();
        next.revision = 1;
        let archive = ArchiveBatch {
            completed: vec![CompletedItem {
                id: "old-work".into(),
                result: "already summarized".into(),
            }],
            decisions: Vec::new(),
            epistemic: Vec::new(),
        };

        assert!(store
            .save_transition(&next, b"new observation", Some(&archive))
            .is_err());
        assert_eq!(fs::read(collision).unwrap(), b"keep-existing-archive");
        let loaded = store.load().expect("unchanged run");
        assert_eq!(loaded.state, state);
        assert!(loaded.observation.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_state_is_rejected_without_touching_target() {
        use std::os::unix::fs::symlink;

        let (project, manifest, workflow, state) = fixture();
        let store = RunStore::create(project.path(), "feature-x", &manifest, &workflow, &state)
            .expect("create run");
        let outside = project.path().join("outside.json");
        fs::write(&outside, b"keep").expect("outside fixture");
        fs::remove_file(&store.paths().state).expect("remove state");
        symlink(&outside, &store.paths().state).expect("symlink state");

        assert_eq!(store.load().unwrap_err(), RuntimeError::UnsafeStorePath);
        assert_eq!(fs::read(outside).unwrap(), b"keep");
    }
}
