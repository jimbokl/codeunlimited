//! Current-user native daily scheduling for the local monitor.

use crate::safeio::{atomic_create, atomic_write, read_optional_text};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Platform {
    Mac,
    Linux,
    Windows,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Context {
    platform: Platform,
    home: PathBuf,
    config_home: PathBuf,
    user: String,
}

#[derive(Debug)]
struct CommandResult {
    code: i32,
    stdout: String,
    stderr: String,
}

trait Runner {
    fn run(&mut self, program: &str, args: &[String]) -> io::Result<CommandResult>;
}

const MANIFEST: &str = "schedule-owner.json";
const OWNER: &str = "codeunlimited-local-monitor-v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    owner: String,
    state: PathBuf,
    context: Context,
    exe: PathBuf,
    registered_hash: Option<String>,
    #[serde(default)]
    registration_token: Option<String>,
}

struct Resource {
    path: PathBuf,
    text: String,
}
struct Job {
    name: String,
    files: Vec<Resource>,
}
struct NativeRunner;

impl Runner for NativeRunner {
    fn run(&mut self, program: &str, args: &[String]) -> io::Result<CommandResult> {
        let output = Command::new(program).args(args).stdin(Stdio::null()).output()
            .map_err(|e| io::Error::new(e.kind(), format!("cannot run {program}: {e}; install the current-user scheduler or use monitor enable --no-schedule")))?;
        Ok(CommandResult {
            code: output.status.code().unwrap_or(-1),
            stdout: decode_output(&output.stdout),
            stderr: decode_output(&output.stderr),
        })
    }
}

fn decode_output(bytes: &[u8]) -> String {
    // schtasks may export UTF-16 XML even when other commands use UTF-8.
    if bytes.starts_with(&[0xff, 0xfe]) || (bytes.len() > 3 && bytes[1] == 0 && bytes[3] == 0) {
        let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
        return String::from_utf16_lossy(
            &bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        );
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Register an owned daily job. The executable must remain at this absolute path.
pub fn install(exe: &Path, state_dir: &Path) -> io::Result<String> {
    path_text(exe)?;
    path_text(state_dir)?;
    let mut runner = NativeRunner;
    let context = current_context(&mut runner)?;
    install_with(exe, state_dir, &context, &mut runner)
}

/// Remove only this state's owned scheduler resources; retain monitoring reports.
pub fn remove(state_dir: &Path) -> io::Result<()> {
    path_text(state_dir)?;
    // External schedulers and fresh state never need even a native probe.
    if read_optional_text(&state_dir.join(MANIFEST))?.is_none() {
        return Ok(());
    }
    let mut runner = NativeRunner;
    let context = current_context(&mut runner)?;
    remove_with(state_dir, &context, &mut runner)
}

fn current_context(runner: &mut impl Runner) -> io::Result<Context> {
    let platform = if cfg!(target_os = "macos") {
        Platform::Mac
    } else if cfg!(target_os = "linux") {
        Platform::Linux
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "native monitoring supports macOS, Linux systemd and Windows; use monitor enable --no-schedule with an external scheduler"));
    };
    let home_var = if platform == Platform::Windows {
        "USERPROFILE"
    } else {
        "HOME"
    };
    let home = std::env::var_os(home_var)
        .map(PathBuf::from)
        .ok_or_else(|| invalid("current user's home is unavailable"))?;
    path_text(&home)?;
    let config_home = if platform == Platform::Linux {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
    } else {
        home.join(".config")
    };
    path_text(&config_home)?;
    let user = if platform == Platform::Windows {
        let out = checked(runner, "whoami.exe", &["/user", "/fo", "csv", "/nh"])?;
        out.stdout
            .split([',', '"', '\r', '\n'])
            .find(|s| {
                s.starts_with("S-1-")
                    && s.chars()
                        .all(|c| c.is_ascii_digit() || c == '-' || c == 'S')
            })
            .ok_or_else(|| invalid("cannot identify current Windows user SID"))?
            .to_owned()
    } else {
        let user = checked(runner, "id", &["-u"])?.stdout.trim().to_owned();
        if user.is_empty() || !user.chars().all(|c| c.is_ascii_digit()) {
            return Err(invalid("cannot identify current user id"));
        }
        user
    };
    Ok(Context {
        platform,
        home,
        config_home,
        user,
    })
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
fn conflict(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::AlreadyExists, format!("scheduler ownership conflict: {}; preserve this resource and inspect it before retrying", message.into()))
}
fn path_text(path: &Path) -> io::Result<&str> {
    if !path.is_absolute() {
        return Err(invalid(format!(
            "scheduler paths must be absolute: {}",
            path.display()
        )));
    }
    let value = path
        .to_str()
        .ok_or_else(|| invalid("scheduler paths must be valid Unicode"))?;
    if value.chars().any(char::is_control) {
        return Err(invalid("scheduler paths cannot contain control characters"));
    }
    Ok(value)
}
fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn checked(runner: &mut impl Runner, program: &str, args: &[&str]) -> io::Result<CommandResult> {
    let output = runner.run(
        program,
        &args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>(),
    )?;
    if output.code != 0 {
        return Err(command_error(program, &output));
    }
    Ok(output)
}
fn command_error(program: &str, output: &CommandResult) -> io::Error {
    io::Error::other(format!("{program} failed ({}): {} {}; check that the current-user scheduler is available, or use monitor enable --no-schedule; monitor disable removes owned partial setup", output.code, output.stderr.trim(), output.stdout.trim()))
}

fn read_owner(state: &Path, context: &Context) -> io::Result<Option<Ownership>> {
    let Some(text) = read_optional_text(&state.join(MANIFEST))? else {
        return Ok(None);
    };
    let manifest: Ownership = serde_json::from_str(&text)
        .map_err(|e| invalid(format!("invalid scheduler ownership manifest: {e}")))?;
    if manifest.owner != OWNER || manifest.state != state || manifest.context != *context {
        return Err(conflict(
            "manifest belongs to another state directory, user, or scheduler environment",
        ));
    }
    path_text(&manifest.exe)?;
    Ok(Some(manifest))
}

fn install_with(
    exe: &Path,
    state: &Path,
    context: &Context,
    runner: &mut impl Runner,
) -> io::Result<String> {
    path_text(exe)?;
    path_text(state)?;
    fs::create_dir_all(state)?;
    let state = fs::canonicalize(state)?;
    path_text(&state)?;
    let existing = read_owner(&state, context)?;
    if existing.as_ref().is_some_and(|m| m.exe != exe) {
        return Err(conflict(
            "executable path changed; run monitor disable before installing from the new path",
        ));
    }
    let previously_owned = existing.is_some();
    let registration_token = if !previously_owned && context.platform == Platform::Windows {
        let nonce = tempfile::Builder::new()
            .rand_bytes(32)
            .tempfile_in(&state)?;
        Some(digest(&nonce.path().to_string_lossy()))
    } else {
        None
    };
    let mut owner = existing.unwrap_or_else(|| Ownership {
        owner: OWNER.into(),
        state: state.clone(),
        context: Context {
            platform: context.platform,
            home: context.home.clone(),
            config_home: context.config_home.clone(),
            user: context.user.clone(),
        },
        exe: exe.to_path_buf(),
        registered_hash: None,
        registration_token,
    });
    let job = render(exe, &state, context, owner.registration_token.as_deref())?;
    verify_files(&job, previously_owned)?;
    let registered = probe(&job, context, previously_owned.then_some(&owner), runner)?;
    if !previously_owned && registered {
        return Err(conflict(&job.name));
    }
    let manifest = state.join(MANIFEST);
    if read_optional_text(&manifest)?.is_none() {
        atomic_create(&manifest, &serde_json::to_vec_pretty(&owner)?)?;
    }
    for resource in &job.files {
        fs::create_dir_all(resource.path.parent().unwrap())?;
        if read_optional_text(&resource.path)?.is_none() {
            atomic_create(&resource.path, resource.text.as_bytes())?;
        }
    }
    match context.platform {
        Platform::Mac => {
            if !registered {
                checked(
                    runner,
                    "launchctl",
                    &[
                        "bootstrap",
                        &format!("gui/{}", context.user),
                        path_text(&job.files[0].path)?,
                    ],
                )?;
            }
        }
        Platform::Linux => {
            checked(runner, "systemctl", &["--user", "daemon-reload"])?;
            checked(
                runner,
                "systemctl",
                &["--user", "enable", "--now", &format!("{}.timer", job.name)],
            )?;
        }
        Platform::Windows => {
            if !registered {
                // No /F: a concurrent or foreign task can never be overwritten.
                checked(
                    runner,
                    "schtasks.exe",
                    &[
                        "/Create",
                        "/TN",
                        &job.name,
                        "/XML",
                        path_text(&job.files[0].path)?,
                    ],
                )?;
            }
            if owner.registered_hash.is_none() || !registered {
                let snapshot = checked(
                    runner,
                    "schtasks.exe",
                    &["/Query", "/TN", &job.name, "/XML"],
                )?;
                if !pending_windows_task_matches(&job, &owner, &snapshot.stdout) {
                    return Err(conflict(&job.name));
                }
                owner.registered_hash = Some(digest(&snapshot.stdout));
                atomic_write(&manifest, &serde_json::to_vec_pretty(&owner)?)?;
            }
        }
    }
    Ok(format!("daily {:?} job {}", context.platform, job.name))
}

fn remove_with(state: &Path, context: &Context, runner: &mut impl Runner) -> io::Result<()> {
    path_text(state)?;
    if read_optional_text(&state.join(MANIFEST))?.is_none() {
        return Ok(());
    }
    let state = fs::canonicalize(state)?;
    let Some(owner) = read_owner(&state, context)? else {
        return Ok(());
    };
    let job = render(
        &owner.exe,
        &state,
        context,
        owner.registration_token.as_deref(),
    )?;
    verify_files(&job, true)?;
    let registered = probe(&job, context, Some(&owner), runner)?;
    match context.platform {
        Platform::Mac => {
            if registered {
                checked(
                    runner,
                    "launchctl",
                    &["bootout", &format!("gui/{}/{}", context.user, job.name)],
                )?;
            }
        }
        Platform::Linux => {
            // Even inactive timers can remain enabled for the next user login.
            if job.files.iter().any(|r| r.path.exists()) || registered {
                checked(
                    runner,
                    "systemctl",
                    &["--user", "disable", "--now", &format!("{}.timer", job.name)],
                )?;
            }
        }
        Platform::Windows => {
            if registered {
                checked(runner, "schtasks.exe", &["/Delete", "/TN", &job.name, "/F"])?;
            }
        }
    }
    for resource in &job.files {
        if read_optional_text(&resource.path)?.as_deref() == Some(resource.text.as_str()) {
            fs::remove_file(&resource.path)?;
        }
    }
    if context.platform == Platform::Linux {
        checked(runner, "systemctl", &["--user", "daemon-reload"])?;
    }
    fs::remove_file(state.join(MANIFEST))?;
    Ok(())
}

fn verify_files(job: &Job, owned: bool) -> io::Result<()> {
    for resource in &job.files {
        if let Some(text) = read_optional_text(&resource.path)? {
            if !owned || text != resource.text {
                return Err(conflict(resource.path.display().to_string()));
            }
        }
    }
    Ok(())
}

fn probe(
    job: &Job,
    context: &Context,
    owner: Option<&Ownership>,
    runner: &mut impl Runner,
) -> io::Result<bool> {
    let call = |runner: &mut dyn Runner, program: &str, args: &[&str]| {
        runner.run(
            program,
            &args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
        )
    };
    match context.platform {
        Platform::Mac => {
            let output = call(
                runner,
                "launchctl",
                &["print", &format!("gui/{}/{}", context.user, job.name)],
            )?;
            if output.code == 113 {
                return Ok(false);
            }
            if output.code != 0 {
                return Err(command_error("launchctl", &output));
            }
            let expected = path_text(&job.files[0].path)?;
            if owner.is_none()
                || !output
                    .stdout
                    .lines()
                    .any(|line| line.trim().strip_prefix("path = ") == Some(expected))
            {
                return Err(conflict(&job.name));
            }
            Ok(true)
        }
        Platform::Linux => {
            let mut registered = false;
            for resource in &job.files {
                let unit = resource.path.file_name().unwrap().to_str().unwrap();
                let output = call(
                    runner,
                    "systemctl",
                    &[
                        "--user",
                        "show",
                        unit,
                        "--property=LoadState",
                        "--property=FragmentPath",
                        "--property=DropInPaths",
                    ],
                )?;
                if output.stdout.lines().any(|l| l == "LoadState=not-found") {
                    continue;
                }
                if output.code != 0 {
                    return Err(command_error("systemctl", &output));
                }
                let expected = format!("FragmentPath={}", path_text(&resource.path)?);
                if owner.is_none()
                    || !output.stdout.lines().any(|l| l == expected)
                    || output.stdout.lines().any(|l| {
                        l.strip_prefix("DropInPaths=")
                            .is_some_and(|p| !p.is_empty())
                    })
                {
                    return Err(conflict(unit));
                }
                registered = true;
            }
            Ok(registered)
        }
        Platform::Windows => {
            let output = call(
                runner,
                "schtasks.exe",
                &["/Query", "/TN", &job.name, "/XML"],
            )?;
            if output.code != 0 {
                // Missing-task errors are localized. A successful full listing confirms absence
                // without treating denied access or an unavailable scheduler as success.
                let listing = checked(runner, "schtasks.exe", &["/Query", "/FO", "CSV", "/NH"])?;
                let found = listing.stdout.lines().any(|l| {
                    l.split(',')
                        .next()
                        .unwrap_or("")
                        .trim_matches('"')
                        .trim_start_matches('\\')
                        == job.name
                });
                if found {
                    return Err(command_error("schtasks.exe", &output));
                }
                return Ok(false);
            }
            let owned = owner.is_some_and(|o| match o.registered_hash.as_deref() {
                Some(hash) => hash == digest(&output.stdout),
                None => pending_windows_task_matches(job, o, &output.stdout),
            });
            if !owned {
                return Err(conflict(&job.name));
            }
            Ok(true)
        }
    }
}

fn render(
    exe: &Path,
    state: &Path,
    context: &Context,
    registration_token: Option<&str>,
) -> io::Result<Job> {
    let exe = path_text(exe)?;
    let state_text = path_text(state)?;
    // Task Scheduler expands environment variables in both Command and Arguments.
    // It offers no literal-percent escape; refuse paths whose bytes could change.
    if context.platform == Platform::Windows && (exe.contains('%') || state_text.contains('%')) {
        return Err(invalid("Windows Task Scheduler cannot preserve literal % in executable or state paths; choose a path without % or use monitor enable --no-schedule"));
    }
    let suffix = &digest(&format!("{}\n{}", context.user, state_text))[..20];
    let arguments = [
        exe,
        "monitor",
        "check",
        "--state-dir",
        state_text,
        "--quiet",
    ];
    let name = match context.platform {
        Platform::Mac => format!("com.codeunlimited.monitor.{suffix}"),
        Platform::Linux => format!("codeunlimited-monitor-{suffix}"),
        Platform::Windows => format!("CodeUnlimited-Monitor-{suffix}"),
    };
    let description = registration_token
        .map(|token| format!("{OWNER}:{token}"))
        .unwrap_or_else(|| OWNER.into());
    let files = match context.platform {
        Platform::Mac => vec![Resource {
            path: context.home.join("Library/LaunchAgents").join(format!("{name}.plist")),
            text: format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{name}</string><key>ProgramArguments</key><array>{}</array><key>StartInterval</key><integer>86400</integer></dict></plist>\n", arguments.iter().map(|s| format!("<string>{}</string>", xml(s))).collect::<String>()),
        }],
        Platform::Linux => {
            let dir = context.config_home.join("systemd/user");
            vec![Resource { path: dir.join(format!("{name}.service")), text: format!("[Unit]\nDescription=CodeUnlimited local usage monitor\n[Service]\nType=oneshot\nExecStart={}\n", arguments.iter().enumerate().map(|(i, a)| systemd_argument(&if i == 0 { format!(":{a}") } else { (*a).to_owned() })).collect::<Vec<_>>().join(" ")) },
                Resource { path: dir.join(format!("{name}.timer")), text: format!("[Unit]\nDescription=CodeUnlimited daily local usage monitor\n[Timer]\nOnCalendar=*-*-* 11:30:00\nPersistent=true\nUnit={name}.service\n[Install]\nWantedBy=timers.target\n") }]
        },
        Platform::Windows => vec![Resource {
            path: state.join("schedule-task.xml"),
            text: format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\"><RegistrationInfo><Description>{description}</Description></RegistrationInfo><Triggers><CalendarTrigger><StartBoundary>2020-01-01T11:30:00</StartBoundary><Enabled>true</Enabled><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger></Triggers><Principals><Principal id=\"CurrentUser\"><UserId>{}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals><Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><StartWhenAvailable>true</StartWhenAvailable><Enabled>true</Enabled><ExecutionTimeLimit>PT1H</ExecutionTimeLimit></Settings><Actions Context=\"CurrentUser\"><Exec><Command>{}</Command><Arguments>{}</Arguments></Exec></Actions></Task>\n", xml(&context.user), xml(exe), xml(&arguments[1..].iter().map(|a| windows_argument(a)).collect::<Vec<_>>().join(" "))),
        }],
    };
    Ok(Job { name, files })
}

/// A pending registration can be recovered only with its precommitted nonce and
/// exact task structure. Scheduler-exported defaults are permitted only at their
/// documented values; changed actions, triggers, principals or settings fail closed.
fn pending_windows_task_matches(job: &Job, owner: &Ownership, exported: &str) -> bool {
    if !owner
        .registration_token
        .as_ref()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return false;
    }
    let (Some(expected), Some(mut actual)) = (
        task_xml_fields(&job.files[0].text),
        task_xml_fields(exported),
    ) else {
        return false;
    };
    for (path, field) in expected {
        if actual.remove(&path).as_ref() != Some(&field) {
            return false;
        }
    }
    actual.into_iter().all(|(path, field)| {
        if !field.attributes.is_empty() {
            return false;
        }
        let value = match path.as_str() {
            "Task/RegistrationInfo/URI" => {
                return !field.has_children && field.text == format!("\\{}", job.name)
            }
            "Task/Settings/IdleSettings" => return field.has_children && field.text.is_empty(),
            "Task/Settings/IdleSettings/Duration" => "PT10M",
            "Task/Settings/IdleSettings/WaitTimeout" => "PT1H",
            "Task/Settings/IdleSettings/StopOnIdleEnd"
            | "Task/Settings/AllowStartOnDemand"
            | "Task/Settings/AllowHardTerminate" => "true",
            "Task/Settings/IdleSettings/RestartOnIdle"
            | "Task/Settings/RunOnlyIfIdle"
            | "Task/Settings/RunOnlyIfNetworkAvailable"
            | "Task/Settings/DisallowStartOnRemoteAppSession"
            | "Task/Settings/UseUnifiedSchedulingEngine"
            | "Task/Settings/WakeToRun"
            | "Task/Settings/Hidden"
            | "Task/Settings/Volatile" => "false",
            "Task/Settings/Priority" => "7",
            _ => return false,
        };
        !field.has_children && field.text == value
    })
}

#[derive(Default, PartialEq, Eq)]
struct XmlField {
    attributes: BTreeMap<String, String>,
    text: String,
    has_children: bool,
}

/// Parse just the unprefixed Task Scheduler XML subset. No DTDs, entities beyond
/// XML character references, mixed content, duplicate paths, or unbounded nesting.
/// Comparing a structural map tolerates exporter indentation/attribute order while
/// preserving every semantic value (including literal spaces inside arguments).
fn task_xml_fields(document: &str) -> Option<BTreeMap<String, XmlField>> {
    if document.len() > 131_072 {
        return None;
    }
    let mut input = document.trim_start_matches('\u{feff}').trim();
    if input.starts_with("<?xml ") {
        input = input.split_once("?>")?.1.trim_start();
    }
    let mut fields = BTreeMap::new();
    let mut stack: Vec<(String, XmlField)> = Vec::new();
    while !input.is_empty() {
        let opening = input.find('<')?;
        let text = &input[..opening];
        if let Some((_, field)) = stack.last_mut() {
            field.text.push_str(text);
        } else if !text.trim().is_empty() {
            return None;
        }
        input = &input[opening + 1..];
        let end = input.find('>')?;
        let tag = input[..end].trim();
        input = &input[end + 1..];
        let (closing, empty) = (tag.starts_with('/'), tag.ends_with('/'));
        if !closing {
            let tag = tag.strip_suffix('/').unwrap_or(tag).trim_end();
            let split = tag.find(char::is_whitespace).unwrap_or(tag.len());
            let name = &tag[..split];
            if name.is_empty()
                || !name.bytes().all(|c| c.is_ascii_alphanumeric())
                || stack.len() >= 16
            {
                return None;
            }
            if stack.is_empty() && (name != "Task" || !fields.is_empty()) {
                return None;
            }
            if let Some((_, field)) = stack.last_mut() {
                field.has_children = true;
            }
            let mut attributes = BTreeMap::new();
            let mut rest = tag[split..].trim();
            while !rest.is_empty() {
                let (key, value) = rest.split_once('=')?;
                let key = key.trim();
                if key.is_empty() || !key.bytes().all(|c| c.is_ascii_alphanumeric()) {
                    return None;
                }
                let value = value.trim_start();
                let quote = value.chars().next()?;
                if !matches!(quote, '\'' | '"') {
                    return None;
                }
                let (value, remaining) = value[1..].split_once(quote)?;
                if attributes.insert(key.into(), xml_text(value)?).is_some() {
                    return None;
                }
                rest = remaining.trim_start();
            }
            stack.push((
                name.into(),
                XmlField {
                    attributes,
                    ..XmlField::default()
                },
            ));
        }
        if closing || empty {
            let path = stack
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join("/");
            let (name, mut field) = stack.pop()?;
            if closing && tag[1..].trim() != name {
                return None;
            }
            if field.has_children {
                if !field.text.trim().is_empty() {
                    return None;
                }
                field.text.clear();
            } else {
                field.text = xml_text(&field.text)?;
            }
            if fields.insert(path, field).is_some() {
                return None;
            }
        }
        if stack.is_empty() {
            input = input.trim();
        }
    }
    if !stack.is_empty() || !fields.contains_key("Task") {
        return None;
    }
    Some(fields)
}

fn xml_text(mut text: &str) -> Option<String> {
    let mut decoded = String::new();
    while let Some(index) = text.find('&') {
        decoded.push_str(&text[..index]);
        let (entity, tail) = text[index + 1..].split_once(';')?;
        decoded.push(match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let code = if let Some(hex) = entity.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16).ok()?
                } else {
                    entity.strip_prefix('#')?.parse().ok()?
                };
                char::from_u32(code)?
            }
        });
        text = tail;
    }
    decoded.push_str(text);
    Some(decoded)
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn systemd_argument(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    )
}
fn windows_argument(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        result.push_str(&"\\".repeat(if c == '"' { slashes * 2 + 1 } else { slashes }));
        slashes = 0;
        result.push(c);
    }
    result.push_str(&"\\".repeat(slashes * 2));
    result.push('"');
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[derive(Default)]
    struct FakeRunner {
        calls: Vec<(String, Vec<String>)>,
        registered: bool,
        failure: Option<String>,
        foreign: bool,
        task_xml: String,
        fail_query_after_create: bool,
    }
    impl Runner for FakeRunner {
        fn run(&mut self, program: &str, args: &[String]) -> io::Result<CommandResult> {
            self.calls.push((program.into(), args.to_vec()));
            if self.fail_query_after_create && self.registered && args.iter().any(|a| a == "/Query")
            {
                return Err(io::Error::other("injected post-create query failure"));
            }
            if self
                .failure
                .as_ref()
                .is_some_and(|f| args.iter().any(|a| a == f))
            {
                return Ok(CommandResult {
                    code: 1,
                    stdout: String::new(),
                    stderr: "injected registration failure".into(),
                });
            }
            let mut out = CommandResult {
                code: 0,
                stdout: String::new(),
                stderr: String::new(),
            };
            if args.iter().any(|a| a == "print") {
                if self.foreign {
                    out.stdout = "path = /foreign/other.plist\n".into();
                } else if self.registered {
                    out.stdout = format!("path = {}\n", self.task_xml);
                } else {
                    out.code = 113;
                    out.stderr = "Could not find service".into();
                }
            } else if args.iter().any(|a| a == "show") {
                out.stdout = if self.foreign {
                    "LoadState=loaded\nFragmentPath=/foreign/job.service\n".into()
                } else if self.registered {
                    let unit = args.get(2).unwrap();
                    format!(
                        "LoadState=loaded\nFragmentPath={}\nDropInPaths=\n",
                        Path::new(&self.task_xml).join(unit).display()
                    )
                } else {
                    "LoadState=not-found\nFragmentPath=\n".into()
                };
            } else if args.iter().any(|a| a == "/Query") && args.iter().any(|a| a == "/TN") {
                if self.foreign {
                    out.stdout = "<Task>foreign</Task>".into();
                } else if self.registered {
                    out.stdout = self.task_xml.clone();
                } else {
                    out.code = 1;
                    out.stderr = "ERROR: The system cannot find the file specified.".into();
                }
            } else if args.iter().any(|a| a == "bootstrap") {
                self.registered = true;
                self.task_xml = args.last().unwrap().clone();
            } else if args.iter().any(|a| a == "enable") {
                self.registered = true;
            } else if args.iter().any(|a| a == "/Create") {
                self.registered = true;
                let i = args.iter().position(|a| a == "/XML").unwrap();
                self.task_xml = fs::read_to_string(&args[i + 1]).unwrap();
            } else if args
                .iter()
                .any(|a| matches!(a.as_str(), "bootout" | "disable" | "/Delete"))
            {
                self.registered = false;
            }
            Ok(out)
        }
    }

    fn fixture(platform: Platform) -> (tempfile::TempDir, Context, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let state = temp.path().join("state");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&state).unwrap();
        let exe = temp.path().join("bin folder/codeunlimited");
        (
            temp,
            Context {
                platform,
                config_home: home.join(".config"),
                home,
                user: "501".into(),
            },
            exe,
            state,
        )
    }

    fn files_at(path: &Path, suffix: &str) -> Vec<PathBuf> {
        fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().ends_with(suffix))
            .collect()
    }

    // The Unix-adapter tests below create fixture directories whose names are
    // legal on Unix but rejected by Windows (quotes, angle brackets) or assert
    // forward-slash path renderings; the adapters they cover never run on
    // Windows, and the ubuntu/macos CI jobs keep them exercised.
    #[cfg(unix)]
    #[test]
    fn launchagent_passes_hostile_paths_as_six_literal_arguments_and_runs_daily() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Mac);
        let state = state.join("quotes'\" &<tag> $HOME %x");
        fs::create_dir(&state).unwrap();
        let mut runner = FakeRunner::default();
        install_with(&exe, &state, &ctx, &mut runner).unwrap();
        let files = files_at(&ctx.home.join("Library/LaunchAgents"), ".plist");
        assert_eq!(files.len(), 1);
        let xml = fs::read_to_string(&files[0]).unwrap();
        assert!(xml.contains("quotes&apos;&quot; &amp;&lt;tag&gt; $HOME %x"));
        assert!(xml.contains("<integer>86400</integer>"));
        assert_eq!(
            xml.split("<array>")
                .nth(1)
                .unwrap()
                .split("</array>")
                .next()
                .unwrap()
                .matches("<string>")
                .count(),
            6
        );
        assert!(xml.contains(
            "<string>monitor</string><string>check</string><string>--state-dir</string>"
        ));
        assert!(runner.calls.iter().any(|(p, a)| p == "launchctl"
            && a == &vec![
                String::from("bootstrap"),
                String::from("gui/501"),
                files[0].to_str().unwrap().to_owned()
            ]));
    }

    #[cfg(unix)]
    #[test]
    fn systemd_escapes_specifiers_variables_quotes_and_backslashes() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Linux);
        let state = state.join("a $HOME %n \"x\" \\ b");
        fs::create_dir(&state).unwrap();
        let mut runner = FakeRunner::default();
        install_with(&exe, &state, &ctx, &mut runner).unwrap();
        let dir = ctx.config_home.join("systemd/user");
        let service = fs::read_to_string(&files_at(&dir, ".service")[0]).unwrap();
        assert!(service.contains("a $HOME %%n \\\"x\\\" \\\\ b"));
        assert!(service.contains("\"monitor\" \"check\" \"--state-dir\""));
        assert!(service.contains("\"--quiet\""));
        let timer = fs::read_to_string(&files_at(&dir, ".timer")[0]).unwrap();
        assert!(timer.contains("OnCalendar=*-*-* 11:30:00"));
        assert!(timer.contains("Persistent=true"));
        assert!(runner
            .calls
            .iter()
            .all(|(_, args)| args.first().is_some_and(|a| a == "--user")));
    }

    #[test]
    fn windows_creates_daily_interactive_task_without_force_or_shell() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Windows);
        let mut runner = FakeRunner::default();
        install_with(&exe, &state, &ctx, &mut runner).unwrap();
        assert!(runner.task_xml.contains("<DaysInterval>1</DaysInterval>"));
        assert!(runner
            .task_xml
            .contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(runner
            .task_xml
            .contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(runner
            .task_xml
            .contains("&quot;monitor&quot; &quot;check&quot; &quot;--state-dir&quot;"));
        assert!(runner
            .calls
            .iter()
            .all(|(p, a)| p == "schtasks.exe" && !a.iter().any(|x| x == "/F")));
    }

    #[test]
    fn windows_argument_obeys_backslash_before_quote_and_trailing_backslash_rules() {
        assert_eq!(windows_argument("C:\\a b\\"), "\"C:\\a b\\\\\"");
        assert_eq!(windows_argument("a\\\"b"), "\"a\\\\\\\"b\"");
    }

    #[test]
    fn repeat_install_preserves_manifest_and_remove_retains_report() {
        for platform in [Platform::Mac, Platform::Windows] {
            let (_tmp, ctx, exe, state) = fixture(platform);
            let mut runner = FakeRunner::default();
            let first = install_with(&exe, &state, &ctx, &mut runner).unwrap();
            let manifest = fs::read(state.join("schedule-owner.json")).unwrap();
            fs::write(state.join("report.md"), "report stays").unwrap();
            assert_eq!(
                install_with(&exe, &state, &ctx, &mut runner).unwrap(),
                first
            );
            assert_eq!(
                fs::read(state.join("schedule-owner.json")).unwrap(),
                manifest
            );
            remove_with(&state, &ctx, &mut runner).unwrap();
            assert!(!runner.registered);
            assert!(!state.join("schedule-owner.json").exists());
            assert_eq!(
                fs::read_to_string(state.join("report.md")).unwrap(),
                "report stays"
            );
            remove_with(&state, &ctx, &mut runner).unwrap();
        }
    }

    #[test]
    fn foreign_runtime_resource_is_never_overwritten() {
        for platform in [Platform::Mac, Platform::Linux, Platform::Windows] {
            let (_tmp, ctx, exe, state) = fixture(platform);
            let mut runner = FakeRunner {
                foreign: true,
                ..FakeRunner::default()
            };
            assert!(install_with(&exe, &state, &ctx, &mut runner).is_err());
            assert!(runner.calls.iter().all(|(_, a)| !a.iter().any(|x| matches!(
                x.as_str(),
                "bootstrap" | "enable" | "/Create" | "/Delete" | "bootout" | "disable"
            ))));
            assert!(!state.join("schedule-owner.json").exists());
        }
    }

    #[test]
    fn externally_modified_file_is_preserved_by_install_and_remove() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Mac);
        let mut runner = FakeRunner::default();
        install_with(&exe, &state, &ctx, &mut runner).unwrap();
        let file = files_at(&ctx.home.join("Library/LaunchAgents"), ".plist").remove(0);
        fs::write(&file, "foreign replacement").unwrap();
        assert!(install_with(&exe, &state, &ctx, &mut runner).is_err());
        assert!(remove_with(&state, &ctx, &mut runner).is_err());
        assert_eq!(fs::read_to_string(file).unwrap(), "foreign replacement");
        assert!(runner.registered);
    }

    #[test]
    fn copied_manifest_cannot_remove_another_states_job() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Mac);
        let mut runner = FakeRunner::default();
        install_with(&exe, &state, &ctx, &mut runner).unwrap();
        let other = state.join("other");
        fs::create_dir(&other).unwrap();
        fs::copy(
            state.join("schedule-owner.json"),
            other.join("schedule-owner.json"),
        )
        .unwrap();
        assert!(remove_with(&other, &ctx, &mut runner).is_err());
        assert!(runner.registered);
    }

    #[test]
    fn registration_failure_reports_error_and_leaves_recoverable_owned_files() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Mac);
        let mut runner = FakeRunner {
            failure: Some("bootstrap".into()),
            ..FakeRunner::default()
        };
        let error = install_with(&exe, &state, &ctx, &mut runner).unwrap_err();
        assert!(error.to_string().contains("injected registration failure"));
        assert!(state.join("schedule-owner.json").exists());
        runner.failure = None;
        remove_with(&state, &ctx, &mut runner).unwrap();
        assert!(files_at(&ctx.home.join("Library/LaunchAgents"), ".plist").is_empty());
    }

    #[test]
    fn relative_state_and_executable_fail_before_any_native_command() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Mac);
        let mut runner = FakeRunner::default();
        assert!(install_with(Path::new("relative"), &state, &ctx, &mut runner).is_err());
        assert!(install_with(&exe, Path::new("relative"), &ctx, &mut runner).is_err());
        assert!(remove_with(Path::new("relative"), &ctx, &mut runner).is_err());
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn no_manifest_removal_never_queries_native_scheduler() {
        for platform in [Platform::Mac, Platform::Linux, Platform::Windows] {
            let (_tmp, ctx, _exe, state) = fixture(platform);
            let mut runner = FakeRunner::default();
            remove_with(&state, &ctx, &mut runner).unwrap();
            assert!(runner.calls.is_empty());
        }
    }

    #[test]
    fn failed_removal_retains_manifest_and_definition_for_retry() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Mac);
        let mut runner = FakeRunner::default();
        install_with(&exe, &state, &ctx, &mut runner).unwrap();
        runner.failure = Some("bootout".into());
        assert!(remove_with(&state, &ctx, &mut runner).is_err());
        assert!(state.join(MANIFEST).exists());
        assert_eq!(
            files_at(&ctx.home.join("Library/LaunchAgents"), ".plist").len(),
            1
        );
        runner.failure = None;
        remove_with(&state, &ctx, &mut runner).unwrap();
        assert!(!state.join(MANIFEST).exists());
    }

    #[test]
    fn unowned_definition_is_preserved_even_if_identical_to_generated_file() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Mac);
        let mut runner = FakeRunner::default();
        install_with(&exe, &state, &ctx, &mut runner).unwrap();
        fs::remove_file(state.join(MANIFEST)).unwrap();
        runner.registered = false;
        let file = files_at(&ctx.home.join("Library/LaunchAgents"), ".plist").remove(0);
        let original = fs::read(&file).unwrap();
        assert!(install_with(&exe, &state, &ctx, &mut runner).is_err());
        assert_eq!(fs::read(&file).unwrap(), original);
    }

    #[test]
    fn changed_windows_task_is_not_deleted_or_replaced() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Windows);
        let mut runner = FakeRunner::default();
        install_with(&exe, &state, &ctx, &mut runner).unwrap();
        runner.task_xml = "<Task>foreign replacement</Task>".into();
        assert!(remove_with(&state, &ctx, &mut runner).is_err());
        assert!(install_with(&exe, &state, &ctx, &mut runner).is_err());
        assert!(runner.registered);
        assert!(state.join(MANIFEST).exists());
    }

    #[test]
    fn windows_paths_with_environment_expansion_are_rejected_before_registration() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Windows);
        let state = state.join("literal %PATH%");
        fs::create_dir(&state).unwrap();
        let mut runner = FakeRunner::default();
        assert!(install_with(&exe, &state, &ctx, &mut runner).is_err());
        assert!(!runner.registered);
        assert!(!state.join(MANIFEST).exists());
    }

    #[cfg(unix)]
    #[test]
    fn systemd_disables_environment_expansion_for_dollars_in_executable_and_state() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Linux);
        let exe = exe.with_file_name("$EXE");
        let state = state.join("${DATA}");
        fs::create_dir(&state).unwrap();
        let mut runner = FakeRunner::default();
        install_with(&exe, &state, &ctx, &mut runner).unwrap();
        let service = fs::read_to_string(
            files_at(&ctx.config_home.join("systemd/user"), ".service").remove(0),
        )
        .unwrap();
        assert!(service.contains(&format!("ExecStart=\":{}\"", exe.display())));
        assert!(service.contains("/${DATA}\" \"--quiet\""));
        assert!(!service.contains("$$"));
    }

    #[test]
    fn windows_utf16_export_is_decoded_without_losing_ownership_bytes() {
        let bytes = [0xff, 0xfe, 0x3c, 0, 0x54, 0, 0x3e, 0, 0x14, 0x04];
        assert_eq!(decode_output(&bytes), "<T>Д");
    }

    #[test]
    fn systemd_repeat_install_and_disable_own_both_units_and_keep_unrelated_unit() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Linux);
        let dir = ctx.config_home.join("systemd/user");
        let mut runner = FakeRunner {
            task_xml: dir.to_str().unwrap().into(),
            ..FakeRunner::default()
        };
        install_with(&exe, &state, &ctx, &mut runner).unwrap();
        fs::write(dir.join("unrelated.service"), "foreign service stays").unwrap();
        install_with(&exe, &state, &ctx, &mut runner).unwrap();
        remove_with(&state, &ctx, &mut runner).unwrap();
        assert!(!state.join(MANIFEST).exists());
        assert!(!runner.registered);
        assert!(files_at(&dir, ".timer").is_empty());
        assert_eq!(
            files_at(&dir, ".service"),
            vec![dir.join("unrelated.service")]
        );
        assert_eq!(
            fs::read_to_string(dir.join("unrelated.service")).unwrap(),
            "foreign service stays"
        );
    }

    #[test]
    fn windows_post_create_query_failure_can_resume_or_remove_exact_owned_task() {
        for resume in [false, true] {
            let (_tmp, ctx, exe, state) = fixture(Platform::Windows);
            let mut runner = FakeRunner {
                fail_query_after_create: true,
                ..FakeRunner::default()
            };
            let error = install_with(&exe, &state, &ctx, &mut runner).unwrap_err();
            assert!(error.to_string().contains("post-create query failure"));
            assert!(runner.registered);
            let owner: Ownership =
                serde_json::from_slice(&fs::read(state.join(MANIFEST)).unwrap()).unwrap();
            assert!(owner.registered_hash.is_none());
            runner.fail_query_after_create = false;
            if resume {
                install_with(&exe, &state, &ctx, &mut runner).unwrap();
                let owner: Ownership =
                    serde_json::from_slice(&fs::read(state.join(MANIFEST)).unwrap()).unwrap();
                assert!(owner.registered_hash.is_some());
                assert_eq!(
                    runner
                        .calls
                        .iter()
                        .filter(|(_, a)| a.iter().any(|s| s == "/Create"))
                        .count(),
                    1
                );
            }
            remove_with(&state, &ctx, &mut runner).unwrap();
            assert!(!runner.registered);
            assert!(!state.join(MANIFEST).exists());
        }
    }

    #[test]
    fn windows_partial_registration_preserves_foreign_modified_task() {
        for (old, new) in [
            (OWNER, "foreign-owner"),
            ("<Command>", "<Command>foreign-"),
            (
                "<DaysInterval>1</DaysInterval>",
                "<DaysInterval>2</DaysInterval>",
            ),
            ("<UserId>501</UserId>", "<UserId>other-user</UserId>"),
            (
                "<RunLevel>LeastPrivilege</RunLevel>",
                "<RunLevel>HighestAvailable</RunLevel>",
            ),
            ("&quot;--quiet&quot;", "&quot;--different&quot;"),
            (
                "</Exec>",
                "</Exec><Exec><Command>foreign.exe</Command></Exec>",
            ),
        ] {
            let (_tmp, ctx, exe, state) = fixture(Platform::Windows);
            let mut runner = FakeRunner {
                fail_query_after_create: true,
                ..FakeRunner::default()
            };
            assert!(install_with(&exe, &state, &ctx, &mut runner).is_err());
            runner.fail_query_after_create = false;
            assert!(runner.task_xml.contains(old));
            runner.task_xml = runner.task_xml.replace(old, new);
            assert!(install_with(&exe, &state, &ctx, &mut runner).is_err());
            assert!(remove_with(&state, &ctx, &mut runner).is_err());
            assert!(runner.registered);
            assert!(state.join(MANIFEST).exists());
        }
    }

    #[test]
    fn windows_pending_recovery_accepts_native_export_format_and_default_settings() {
        let (_tmp, ctx, exe, state) = fixture(Platform::Windows);
        let mut runner = FakeRunner {
            fail_query_after_create: true,
            ..FakeRunner::default()
        };
        assert!(install_with(&exe, &state, &ctx, &mut runner).is_err());
        runner.fail_query_after_create = false;
        let task_name = runner
            .calls
            .iter()
            .find(|(_, a)| a.first().is_some_and(|s| s == "/Create"))
            .unwrap()
            .1[2]
            .clone();
        runner.task_xml = runner.task_xml
            .replace("encoding=\"UTF-8\"", "encoding=\"UTF-16\"")
            .replace("<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">", "<Task xmlns='http://schemas.microsoft.com/windows/2004/02/mit/task' version='1.2'>")
            .replace("&quot;", "&#34;")
            .replace("</RegistrationInfo>", &format!("<URI>\\{task_name}</URI></RegistrationInfo>"))
            .replace("</Settings>", "<IdleSettings><Duration>PT10M</Duration><WaitTimeout>PT1H</WaitTimeout><StopOnIdleEnd>true</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings><AllowHardTerminate>true</AllowHardTerminate><Hidden>false</Hidden><AllowStartOnDemand>true</AllowStartOnDemand><Priority>7</Priority><WakeToRun>false</WakeToRun></Settings>")
            .replace("><", ">\r\n  <");
        install_with(&exe, &state, &ctx, &mut runner).unwrap();
        remove_with(&state, &ctx, &mut runner).unwrap();
        assert!(!runner.registered);
    }
}
