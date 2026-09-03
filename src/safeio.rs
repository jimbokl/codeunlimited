//! Shared filesystem mutation helpers.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    Created,
    Updated { backup: PathBuf },
    Unchanged,
}

fn reject_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing to replace symlink: {}", path.display()),
        )),
        Ok(meta) if !meta.is_file() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a regular file: {}", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn read_optional_text(path: &Path) -> io::Result<Option<String>> {
    reject_symlink(path)?;
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Atomically replace a regular file with bytes written in the same directory.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    reject_symlink(path)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let existing_permissions = fs::metadata(path).ok().map(|m| m.permissions());
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    if let Some(permissions) = existing_permissions {
        tmp.as_file().set_permissions(permissions)?;
    }
    let persisted = tmp.persist(path).map_err(|e| e.error)?;
    persisted.sync_all()?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn backup_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.codeunlimited.bak",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("file")
    ))
}

pub fn update_text_with_backup(path: &Path, text: &str) -> io::Result<UpdateOutcome> {
    let current = read_optional_text(path)?;
    if current.as_deref() == Some(text) {
        return Ok(UpdateOutcome::Unchanged);
    }
    let backup = current.as_ref().map(|old| {
        let backup = backup_path(path);
        if !backup.exists() {
            atomic_write(&backup, old.as_bytes())?;
        }
        Ok::<PathBuf, io::Error>(backup)
    });
    let backup = match backup {
        Some(result) => Some(result?),
        None => None,
    };
    atomic_write(path, text.as_bytes())?;
    Ok(match backup {
        Some(backup) => UpdateOutcome::Updated { backup },
        None => UpdateOutcome::Created,
    })
}
