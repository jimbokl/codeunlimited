//! `codeunlimited schedule`: a weekly `report --all` ritual without thinking
//! about it. Windows: a Task Scheduler entry (removable with --remove).
//! Other platforms: prints the crontab line to add.

const TASK_NAME: &str = "codeunlimited-weekly-report";

pub fn run(remove: bool) -> i32 {
    let Ok(exe) = std::env::current_exe() else {
        eprintln!("Cannot resolve the codeunlimited executable path.");
        return 1;
    };
    let exe = exe.to_string_lossy().to_string();
    let out = crate::registry::home_dir().join("CODEUNLIMITED_SUMMARY.md");
    let cmd = format!("\"{exe}\" report --all --out \"{}\"", out.display());

    if cfg!(windows) {
        let status = if remove {
            std::process::Command::new("schtasks")
                .args(["/Delete", "/TN", TASK_NAME, "/F"])
                .status()
        } else {
            std::process::Command::new("schtasks")
                .args([
                    "/Create", "/F", "/SC", "WEEKLY", "/D", "MON", "/ST", "09:00", "/TN",
                    TASK_NAME, "/TR", &cmd,
                ])
                .status()
        };
        match status {
            Ok(s) if s.success() => {
                if remove {
                    println!("Weekly report task removed.");
                } else {
                    println!("Scheduled: every Monday 09:00 -> {}", out.display());
                    println!("Remove any time with: codeunlimited schedule --remove");
                }
                0
            }
            _ => {
                eprintln!("schtasks failed - run the command manually or check permissions.");
                1
            }
        }
    } else {
        if remove {
            println!("Remove the codeunlimited line from `crontab -e`.");
        } else {
            println!("Add this line via `crontab -e` (every Monday 09:00):");
            println!("0 9 * * 1 {cmd}");
        }
        0
    }
}
