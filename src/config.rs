//! Optional `.codeunlimited.toml` config: detector thresholds and ignored
//! projects. Looked up in the current directory first, then
//! `~/.codeunlimited/config.toml`. Missing file = defaults; every field is
//! optional.
//!
//! ```toml
//! ignore_projects = ["scratch", "tmp"]
//!
//! [thresholds]
//! long_session_turns = 30
//! trivial_output_tokens = 300
//! fat_start_tokens = 25000
//! ```

use std::sync::OnceLock;

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

fn parse(raw: &str) -> Config {
    let mut cfg = Config::default();
    let Ok(v) = raw.parse::<toml::Value>() else {
        eprintln!("warning: .codeunlimited.toml is not valid TOML - using defaults");
        return cfg;
    };
    if let Some(list) = v.get("ignore_projects").and_then(|x| x.as_array()) {
        cfg.ignore_projects = list
            .iter()
            .filter_map(|s| s.as_str())
            .map(|s| s.to_lowercase())
            .collect();
    }
    if let Some(t) = v.get("thresholds") {
        if let Some(n) = t.get("long_session_turns").and_then(|x| x.as_integer()) {
            cfg.long_session_turns = n.max(2) as usize;
        }
        if let Some(n) = t.get("trivial_output_tokens").and_then(|x| x.as_integer()) {
            cfg.trivial_output_tokens = n.max(0) as u64;
        }
        if let Some(n) = t.get("fat_start_tokens").and_then(|x| x.as_integer()) {
            cfg.fat_start_tokens = n.max(0) as u64;
        }
    }
    cfg
}

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn get() -> &'static Config {
    CONFIG.get_or_init(|| {
        let local = std::path::Path::new(".codeunlimited.toml");
        let global = crate::registry::home_dir().join("config.toml");
        for p in [local.to_path_buf(), global] {
            if let Ok(raw) = std::fs::read_to_string(&p) {
                return parse(&raw);
            }
        }
        Config::default()
    })
}

/// Should this project be excluded from reports? Case-insensitive substring
/// match against the configured ignore list.
pub fn ignored(project: &str) -> bool {
    let p = project.to_lowercase();
    get().ignore_projects.iter().any(|ig| p.contains(ig))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_config() {
        let cfg = parse("ignore_projects = [\"Tmp\"]\n[thresholds]\nlong_session_turns = 50\n");
        assert_eq!(cfg.long_session_turns, 50);
        assert_eq!(cfg.trivial_output_tokens, 300);
        assert_eq!(cfg.ignore_projects, vec!["tmp"]);
    }

    #[test]
    fn bad_toml_falls_back_to_defaults() {
        let cfg = parse("not [ valid");
        assert_eq!(cfg.long_session_turns, 30);
    }
}
