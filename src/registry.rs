//! Local project registry (~/.codeunlimited/projects.json): every project that
//! `init`, `fix` or `report` touches is remembered, so `report --all` can walk
//! them without re-discovery. Paths only - nothing leaves the machine.

use std::path::{Path, PathBuf};

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

pub fn projects() -> Vec<PathBuf> {
    std::fs::read_to_string(registry_file())
        .ok()
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect()
}

pub fn register(path: &Path) {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = canon.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    let mut list: Vec<String> = std::fs::read_to_string(registry_file())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    if list.iter().any(|p| p.eq_ignore_ascii_case(&s)) {
        return;
    }
    list.push(s);
    list.sort();
    let _ = std::fs::create_dir_all(home_dir());
    let _ = std::fs::write(
        registry_file(),
        serde_json::to_string_pretty(&list).unwrap_or_default(),
    );
}
