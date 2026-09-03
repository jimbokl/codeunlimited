//! `codeunlimited skill`: install the Claude Code skill so `/codeunlimited`
//! runs the audit->fix->report flow from inside a session.

const SKILL_MD: &str = include_str!("../skill/codeunlimited/SKILL.md");

pub fn run() -> i32 {
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
    if let Err(e) = std::fs::write(&path, SKILL_MD) {
        eprintln!("Cannot write {}: {e}", path.display());
        return 1;
    }
    println!("Skill installed: {}", path.display());
    println!("In Claude Code, run /codeunlimited (new sessions pick it up automatically).");
    0
}
