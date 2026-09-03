//! Best-effort metadata index used to skip unchanged Codex session files that
//! are provably outside an audit's project or time scope.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

pub const INDEX_FILE: &str = "codex-index-v1.json";
const SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FileFingerprint {
    pub len: u64,
    pub modified_ns: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileIndexEntry {
    pub fingerprint: FileFingerprint,
    pub cwd_keys: Vec<String>,
    pub min_ts: Option<i64>,
    pub max_ts: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CodexIndex {
    pub schema_version: u64,
    pub files: BTreeMap<String, FileIndexEntry>,
}

impl Default for CodexIndex {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            files: BTreeMap::new(),
        }
    }
}

pub enum IndexAccess {
    Enabled(CodexIndex),
    Disabled,
}

pub fn file_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

pub fn fingerprint(path: &Path) -> io::Result<FileFingerprint> {
    let metadata = fs::metadata(path)?;
    let modified = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    Ok(FileFingerprint {
        len: metadata.len(),
        modified_ns: modified.as_nanos().min(u64::MAX as u128) as u64,
    })
}

fn index_path() -> PathBuf {
    crate::registry::home_dir().join(INDEX_FILE)
}

impl IndexAccess {
    pub fn load() -> Self {
        let path = index_path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Self::Enabled(CodexIndex::default());
            }
            Err(error) => {
                eprintln!("warning: cannot inspect {}: {error}", path.display());
                return Self::Disabled;
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            eprintln!(
                "warning: refusing Codex metadata index at non-regular path {}",
                path.display()
            );
            return Self::Disabled;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("warning: cannot read {}: {error}", path.display());
                return Self::Disabled;
            }
        };
        match serde_json::from_slice::<CodexIndex>(&bytes) {
            Ok(index) if index.schema_version == SCHEMA_VERSION => Self::Enabled(index),
            Ok(_) | Err(_) => {
                eprintln!(
                    "warning: {} is not a valid version-{SCHEMA_VERSION} Codex metadata index; rebuilding",
                    path.display()
                );
                Self::Enabled(CodexIndex::default())
            }
        }
    }

    pub fn save(&self) {
        let Self::Enabled(index) = self else {
            return;
        };
        let path = index_path();
        let Some(parent) = path.parent() else {
            return;
        };
        if let Err(error) = fs::create_dir_all(parent) {
            eprintln!("warning: cannot create {}: {error}", parent.display());
            return;
        }
        let bytes = match serde_json::to_vec_pretty(index) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("warning: cannot encode {}: {error}", path.display());
                return;
            }
        };
        if let Err(error) = crate::safeio::atomic_write(&path, &bytes) {
            eprintln!("warning: cannot write {}: {error}", path.display());
        }
    }
}
