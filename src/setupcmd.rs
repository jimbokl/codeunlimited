//! One-time activation via the providers' native global instruction files.
//! No background process, usage scan, provider request or project mutation.

use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

const START: &str = "<!-- codeunlimited:auto:v1 -->";
const END: &str = "<!-- /codeunlimited:auto -->";
const CONFIG_START: &str = "# codeunlimited:auto:v1";
const CONFIG_END: &str = "# /codeunlimited:auto";
const CAP: &str = "tool_output_token_limit = 4000";
const POLICY: &str = "## Automatic token efficiency (codeunlimited)\n\
Apply these defaults while doing normal work; no audit or setup command is needed per task.\n\
- Read only needed files and line ranges. Reuse fresh context; re-read when files changed or context was lost.\n\
- Scope searches; exclude generated output, dependencies and data dumps unless relevant.\n\
- Keep tool output short. Save verbose build/test output to a local log, surface failures and the exit status, then read needed detail from that log. Truncated output is not proof of success.\n\
- For long repetitive work, maintain a compact state file with objective, done/remaining, decisions and verification results; reuse the project's existing state convention. Do not create state for trivial tasks or reread the whole conversation.\n\
- Batch related steps that share context. Before context compaction, preserve a concise handoff. Do not clear or restart a user's active session automatically.\n\
- Give concise results; omit repeated plans, whole-file dumps and duplicate narration.\n\
Keep required tests, correctness checks, model quality, permissions and user/project instructions. Do not disable integrations, delegate, switch models, or start external API calls solely to save tokens. If a project has its own codeunlimited policy, prefer it.\n\
These are workflow defaults, not a guarantee of token or subscription-quota savings.";

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn provider_home(variable: &str, default: &str) -> io::Result<PathBuf> {
    let value = if let Some(path) = std::env::var_os(variable) {
        PathBuf::from(path)
    } else {
        let user = std::env::var_os("USERPROFILE")
            .filter(|v| !v.is_empty())
            .or_else(|| std::env::var_os("HOME").filter(|v| !v.is_empty()))
            .ok_or_else(|| invalid("No user home directory; setup aborted"))?;
        PathBuf::from(user).join(default)
    };
    if !value.is_absolute() {
        return Err(invalid(format!(
            "{variable} must resolve to an absolute directory"
        )));
    }
    Ok(value)
}

fn read(path: &Path) -> io::Result<Option<String>> {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.len() > 1024 * 1024 {
            return Err(invalid(format!(
                "{} exceeds setup's 1 MiB file limit",
                path.display()
            )));
        }
    }
    crate::safeio::read_optional_text(path)
}

fn eol(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Only whole-line, unique, ordered markers are owned. Include the one leading
/// newline that policy installation inserts, and the marker's trailing newline.
fn owned_range(
    text: &str,
    start: &str,
    end: &str,
    leading: bool,
) -> io::Result<Option<Range<usize>>> {
    let starts: Vec<_> = text.match_indices(start).map(|(i, _)| i).collect();
    let ends: Vec<_> = text.match_indices(end).map(|(i, _)| i).collect();
    if starts.is_empty() && ends.is_empty() {
        return Ok(None);
    }
    if starts.len() != 1 || ends.len() != 1 || ends[0] <= starts[0] {
        return Err(invalid("ambiguous codeunlimited automatic-setup markers"));
    }
    for (i, marker) in [(starts[0], start), (ends[0], end)] {
        let after = &text[i + marker.len()..];
        if (i > 0 && text.as_bytes()[i - 1] != b'\n')
            || !(after.is_empty() || after.starts_with('\n') || after.starts_with("\r\n"))
        {
            return Err(invalid("automatic-setup marker must occupy a whole line"));
        }
    }
    let mut begin = starts[0];
    if leading && begin > 0 {
        begin -= 1;
        if begin > 0 && text.as_bytes()[begin - 1] == b'\r' {
            begin -= 1;
        }
    }
    let mut finish = ends[0] + end.len();
    if text[finish..].starts_with("\r\n") {
        finish += 2;
    } else if text[finish..].starts_with('\n') {
        finish += 1;
    }
    Ok(Some(begin..finish))
}

fn policy_block(current: &str) -> String {
    format!("\n{START}\n{POLICY}\n{END}\n").replace('\n', eol(current))
}

fn policy_next(current: &str, remove: bool) -> io::Result<String> {
    let range = owned_range(current, START, END, true)?;
    let block = if remove {
        String::new()
    } else {
        policy_block(current)
    };
    Ok(match range {
        Some(r) => format!("{}{}{}", &current[..r.start], block, &current[r.end..]),
        None if remove => current.to_string(),
        None => format!("{current}{block}"),
    })
}

fn config_next(current: &str, remove: bool, skip: bool) -> io::Result<String> {
    // A UTF-8 BOM is valid only at byte zero. Keep it outside owned ranges so
    // insertion, subsequent upgrades, and removal all preserve the same bytes.
    let (prefix, body) = current
        .strip_prefix('\u{feff}')
        .map_or(("", current), |body| ("\u{feff}", body));
    let next = format!("{prefix}{}", config_body_next(body, remove, skip)?);
    next.parse::<toml::Value>()
        .map_err(|_| invalid("Generated Codex configuration is invalid; no setup files changed"))?;
    Ok(next)
}

fn config_body_next(current: &str, remove: bool, skip: bool) -> io::Result<String> {
    let parsed: toml::Value = current
        .parse()
        .map_err(|_| invalid("Invalid Codex config.toml; no setup files changed"))?;
    if let Some(range) = owned_range(current, CONFIG_START, CONFIG_END, false)? {
        let expected = format!("{CONFIG_START}\n{CAP}\n{CONFIG_END}\n").replace('\n', eol(current));
        if current[range.clone()] != expected
            || parsed
                .get("tool_output_token_limit")
                .and_then(|v| v.as_integer())
                != Some(4000)
        {
            return Err(invalid("Managed Codex output cap was edited; preserve your value by removing only its codeunlimited comment markers, then retry"));
        }
        if remove {
            return Ok(format!(
                "{}{}",
                &current[..range.start],
                &current[range.end..]
            ));
        }
        return Ok(current.to_string());
    }
    if remove || skip || parsed.get("tool_output_token_limit").is_some() {
        return Ok(current.to_string());
    }
    let block = format!("{CONFIG_START}\n{CAP}\n{CONFIG_END}\n").replace('\n', eol(current));
    Ok(format!("{block}{current}"))
}

struct Target {
    path: PathBuf,
    current: Option<String>,
    next: String,
}

fn targets(remove: bool, skip: bool) -> io::Result<Vec<Target>> {
    let claude = provider_home("CLAUDE_CONFIG_DIR", ".claude")?;
    let codex = provider_home("CODEX_HOME", ".codex")?;
    let override_path = codex.join("AGENTS.override.md");
    let override_text = read(&override_path)?;
    let mut paths = vec![claude.join("CLAUDE.md"), codex.join("AGENTS.md")];
    if override_text
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        paths.push(override_path);
    }
    let mut result = Vec::new();
    for path in paths {
        let current = read(&path)?;
        let next = policy_next(current.as_deref().unwrap_or(""), remove)?;
        result.push(Target {
            path,
            current,
            next,
        });
    }
    let path = codex.join("config.toml");
    let current = read(&path)?;
    let next = config_next(current.as_deref().unwrap_or(""), remove, skip)?;
    result.push(Target {
        path,
        current,
        next,
    });
    // Preflight existing backup targets too, before touching another provider.
    for t in &result {
        if t.current.is_some() && t.current.as_deref() != Some(&t.next) {
            crate::safeio::reject_symlink(&crate::safeio::backup_path(&t.path))?;
        }
    }
    Ok(result)
}

fn inspect() -> io::Result<Value> {
    let claude = provider_home("CLAUDE_CONFIG_DIR", ".claude")?;
    let codex = provider_home("CODEX_HOME", ".codex")?;
    let override_path = codex.join("AGENTS.override.md");
    let override_text = read(&override_path)?;
    let active_codex = if override_text
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        override_path
    } else {
        codex.join("AGENTS.md")
    };
    let mut files = Vec::new();
    let mut enabled = true;
    for path in [claude.join("CLAUDE.md"), active_codex] {
        let text = read(&path)?.unwrap_or_default();
        let active =
            owned_range(&text, START, END, true)?.is_some_and(|r| text[r] == policy_block(&text));
        enabled &= active;
        files.push(json!({ "path": path, "policy_installed": active }));
    }
    let config_text = read(&codex.join("config.toml"))?.unwrap_or_default();
    let config: toml::Value = config_text
        .parse()
        .map_err(|_| invalid("Invalid Codex config.toml"))?;
    Ok(json!({
        "schema_version": 1,
        "enabled": enabled,
        "activation": "new_local_sessions",
        "files": files,
        "codex_tool_output_token_limit": config.get("tool_output_token_limit").and_then(|v| v.as_integer()),
        "realized_savings_verified": false,
        "scope": "Local global defaults; project/profile overrides and host instruction limits may take precedence. Existing sessions are not restarted."
    }))
}

fn apply(remove: bool, skip: bool) -> io::Result<()> {
    let changes = targets(remove, skip)?;
    for target in changes {
        if target.current.as_deref().unwrap_or("") == target.next {
            continue;
        }
        if read(&target.path)? != target.current {
            return Err(invalid(
                "Setup target changed concurrently; rerun setup to repair any partial activation",
            ));
        }
        std::fs::create_dir_all(
            target
                .path
                .parent()
                .ok_or_else(|| invalid("Missing setup parent"))?,
        )?;
        crate::safeio::update_text_with_backup(&target.path, &target.next)?;
    }
    Ok(())
}

pub fn run(remove: bool, status: bool, json_output: bool, no_tool_limit: bool) -> i32 {
    let result = (|| {
        if !status {
            apply(remove, no_tool_limit)?;
        }
        inspect()
    })();
    match result {
        Ok(mut report) => {
            report["action"] = json!(if status {
                "status"
            } else if remove {
                "remove"
            } else {
                "install"
            });
            let enabled = report["enabled"].as_bool().unwrap_or(false);
            if json_output {
                println!("{report}");
            } else {
                println!(
                    "Automatic efficiency defaults: {}",
                    if enabled {
                        "installed"
                    } else {
                        "not installed"
                    }
                );
                for file in report["files"].as_array().into_iter().flatten() {
                    println!(
                        "  {}: {}",
                        file["path"].as_str().unwrap_or("?"),
                        if file["policy_installed"] == true {
                            "ready"
                        } else {
                            "inactive"
                        }
                    );
                }
                if let Some(cap) = report["codex_tool_output_token_limit"].as_i64() {
                    println!("  Codex configured tool-output history cap: {cap} tokens (explicit overrides take precedence)");
                }
                if enabled {
                    println!("Global defaults are ready for automatic loading in new local Claude Code/Codex sessions. No per-project init, daemon or API key needed.");
                    println!("Project/profile overrides and host instruction limits may take precedence; this check verifies installed files, not a running model.");
                    println!("Existing sessions are unchanged. Guidance is advisory; this does not measure savings.");
                }
                if remove {
                    println!("Removed only managed blocks; user settings and backups retained.");
                }
            }
            i32::from(status && !enabled)
        }
        Err(error) => {
            if json_output {
                println!(
                    "{}",
                    json!({"schema_version":1, "enabled":false, "error":error.to_string()})
                );
            }
            eprintln!("Setup failed: {error}. Any completed file writes retain backups; fix the reported conflict and rerun `codeunlimited setup`.");
            1
        }
    }
}
