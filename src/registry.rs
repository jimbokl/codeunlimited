//! Local project registry (~/.codeunlimited/projects.json): every project that
//! `init`, `fix` or `report` touches is remembered, so `report --all` can walk
//! them without re-discovery. Paths only - nothing leaves the machine.

use std::path::{Path, PathBuf};

use fs2::FileExt;

pub fn home_dir() -> PathBuf {
    std::env::var_os("CODEUNLIMITED_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".codeunlimited")
        })
}

fn registry_file() -> PathBuf {
    home_dir().join("projects.json")
}

fn read_registry() -> std::io::Result<Vec<String>> {
    let Some(raw) = crate::safeio::read_optional_text(&registry_file())? else {
        return Ok(Vec::new());
    };
    serde_json::from_str(&raw).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid project registry: {error}"),
        )
    })
}

pub fn projects() -> std::io::Result<Vec<PathBuf>> {
    Ok(read_registry()?
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect())
}

pub fn register(path: &Path) -> std::io::Result<()> {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = canon.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    let list = read_registry()?;
    if list.iter().any(|p| p.eq_ignore_ascii_case(&s)) {
        return Ok(());
    }
    std::fs::create_dir_all(home_dir())?;
    let lock_path = home_dir().join("projects.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    lock.lock_exclusive()?;
    let mut list = read_registry()?;
    if list.iter().any(|p| p.eq_ignore_ascii_case(&s)) {
        return Ok(());
    }
    list.push(s);
    list.sort();
    let encoded = serde_json::to_vec_pretty(&list).map_err(std::io::Error::other)?;
    crate::safeio::atomic_write(&registry_file(), &encoded)
}
