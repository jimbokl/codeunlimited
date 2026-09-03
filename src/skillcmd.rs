//! `codeunlimited skill`: install the Claude Code skill so `/codeunlimited`
//! runs the audit->fix->report flow from inside a session.

const SKILL_MD: &str = include_str!("../skill/codeunlimited/SKILL.md");

pub fn run(force: bool) -> i32 {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let dir = home.join(".claude").join("skills").join("codeunlimited");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("Cannot create {}: {e}", dir.display());
        return 1;
    }
    let path = dir.join("SKILL.md");
    let current = match crate::safeio::read_optional_text(&path) {
        Ok(current) => current,
        Err(e) => {
            eprintln!("Cannot safely read {}: {e}", path.display());
            return 1;
        }
    };
    if current.as_deref() == Some(SKILL_MD) {
        println!("Skill already installed: {}", path.display());
        return 0;
    }
    if current.is_some() && !force {
        eprintln!(
            "Refusing to replace a different skill at {}; re-run with `skill --force` \
             to update it and preserve a backup.",
            path.display()
        );
        return 1;
    }
    let outcome = match crate::safeio::update_text_with_backup(&path, SKILL_MD) {
        Ok(outcome) => outcome,
        Err(e) => {
            eprintln!("Cannot write {}: {e}", path.display());
            return 1;
        }
    };
    match outcome {
        crate::safeio::UpdateOutcome::Updated { backup } => {
            println!(
                "Skill updated: {} (previous content: {})",
                path.display(),
                backup.display()
            );
        }
        _ => println!("Skill installed: {}", path.display()),
    }
    println!("In Claude Code, run /codeunlimited (new sessions pick it up automatically).");
    0
}
