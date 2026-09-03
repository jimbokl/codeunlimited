//! Optional configuration for detector thresholds and ignored projects.
//!
//! Machine-wide defaults are read from `~/.codeunlimited/config.toml` (or
//! `CODEUNLIMITED_HOME/config.toml`). A project's `.codeunlimited.toml` is then
//! layered on top when a project path was explicitly selected. Missing fields
//! retain the value from the previous layer.
//!
//! ```toml
//! ignore_projects = ["scratch", "tmp"]
//!
//! [thresholds]
//! long_session_turns = 30
//! trivial_output_tokens = 300
//! fat_start_tokens = 25000
//! ```

use std::path::Path;

#[derive(Debug, Clone)]
pub struct Config {
    pub long_session_turns: usize,
    pub trivial_output_tokens: u64,
    pub fat_start_tokens: u64,
    pub ignore_projects: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            long_session_turns: 30,
            trivial_output_tokens: 300,
            fat_start_tokens: 25_000,
            ignore_projects: vec![],
        }
    }
}

impl Config {
    /// Load machine-wide configuration, then overlay the selected project's
    /// configuration. This is intentionally evaluated for each command scope:
    /// there is no process-global cache tied to the caller's working directory.
    pub fn load_for(project_root: Option<&Path>) -> Self {
        let mut cfg = Self::default();
        cfg.overlay_file(&crate::registry::home_dir().join("config.toml"));
        if let Some(root) = project_root {
            cfg.overlay_file(&root.join(".codeunlimited.toml"));
        }
        cfg
    }

    fn overlay_file(&mut self, path: &Path) {
        match std::fs::read_to_string(path) {
            Ok(raw) => {
                if !self.overlay(&raw) {
                    eprintln!(
                        "warning: {} is not valid TOML - ignoring this layer",
                        path.display()
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => eprintln!("warning: cannot read {}: {e}", path.display()),
        }
    }

    fn overlay(&mut self, raw: &str) -> bool {
        let Ok(v) = raw.parse::<toml::Value>() else {
            return false;
        };
        if let Some(list) = v.get("ignore_projects").and_then(|x| x.as_array()) {
            self.ignore_projects = list
                .iter()
                .filter_map(|s| s.as_str())
                .map(|s| s.to_lowercase())
                .collect();
        }
        if let Some(t) = v.get("thresholds") {
            if let Some(n) = t.get("long_session_turns").and_then(|x| x.as_integer()) {
                self.long_session_turns = n.max(2) as usize;
            }
            if let Some(n) = t.get("trivial_output_tokens").and_then(|x| x.as_integer()) {
                self.trivial_output_tokens = n.max(0) as u64;
            }
            if let Some(n) = t.get("fat_start_tokens").and_then(|x| x.as_integer()) {
                self.fat_start_tokens = n.max(0) as u64;
            }
        }
        true
    }

    /// Should this project be excluded? Matching remains deliberately simple
    /// and backwards-compatible: case-insensitive substring match.
    pub fn is_ignored(&self, project: &str) -> bool {
        let project = project.to_lowercase();
        self.ignore_projects
            .iter()
            .any(|ignored| project.contains(ignored))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlays_partial_config() {
        let mut cfg = Config::default();
        assert!(cfg.overlay("ignore_projects = [\"Tmp\"]\n[thresholds]\nlong_session_turns = 50\n"));
        assert_eq!(cfg.long_session_turns, 50);
        assert_eq!(cfg.trivial_output_tokens, 300);
        assert_eq!(cfg.ignore_projects, vec!["tmp"]);
    }

    #[test]
    fn bad_toml_does_not_change_existing_values() {
        let mut cfg = Config {
            trivial_output_tokens: 17,
            ..Config::default()
        };
        assert!(!cfg.overlay("not [ valid"));
        assert_eq!(cfg.trivial_output_tokens, 17);
    }
}
