//! `orbit-server install`: Orbit without a window as a service that starts
//! with the computer and comes back after a crash. It is a systemd user
//! service on Linux and a launch agent on macOS. The program and its browser
//! page are copied into a folder of their own first, so the service keeps
//! working when the checkout it was built in changes or goes away.
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// The systemd user service's name.
pub const UNIT: &str = "orbit.service";
/// The launch agent's label.
pub const LABEL: &str = "dev.orbit.review.server";
/// The programs Orbit runs, which the service finds on the PATH it records.
/// The first two are needed for anything; one reviewer is needed to review.
const TOOLS: [&str; 8] = [
    "gh",
    "git",
    "codex",
    "claude",
    "agy",
    "docker",
    "devcontainer",
    "tailscale",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A systemd user service (Linux).
    Systemd,
    /// A launch agent (macOS).
    Launchd,
}

/// The computer the service is set up on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    pub kind: Kind,
    pub home: PathBuf,
    /// Where Linux keeps a user's settings: `$XDG_CONFIG_HOME` or `~/.config`.
    pub config: PathBuf,
    /// Where the program's own copy goes.
    pub programs: PathBuf,
    /// The PATH the service runs with, so it finds `gh` and the reviewers.
    pub path: OsString,
    pub uid: u32,
}
impl Host {
    /// This computer, or why it can't run Orbit as a service.
    pub fn current() -> Result<Self, String> {
        let kind = if cfg!(target_os = "macos") {
            Kind::Launchd
        } else if cfg!(target_os = "linux") {
            Kind::Systemd
        } else {
            return Err("orbit-server can set itself up as a service on Linux and macOS. Here, start it another way, such as when you log in.".into());
        };
        Self::of(kind, |name| std::env::var_os(name), uid())
    }
    /// The computer described by its environment variables.
    pub fn of(
        kind: Kind,
        env: impl Fn(&str) -> Option<OsString>,
        uid: u32,
    ) -> Result<Self, String> {
        let var = |name: &str| env(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = var("HOME").ok_or("Orbit can't find your home folder (HOME isn't set).")?;
        let absolute = |p: PathBuf| p.is_absolute().then_some(p);
        let config = var("XDG_CONFIG_HOME")
            .and_then(absolute)
            .unwrap_or_else(|| home.join(".config"));
        let programs = match kind {
            Kind::Launchd => home.join("Library/Application Support/orbit-server"),
            Kind::Systemd => var("XDG_DATA_HOME")
                .and_then(absolute)
                .unwrap_or_else(|| home.join(".local/share"))
                .join("orbit-server"),
        };
        Ok(Self {
            kind,
            home,
            config,
            programs,
            path: env("PATH").unwrap_or_default(),
            uid,
        })
    }
    /// The service's file.
    pub fn service_file(&self) -> PathBuf {
        match self.kind {
            Kind::Systemd => self.config.join("systemd/user").join(UNIT),
            Kind::Launchd => self
                .home
                .join("Library/LaunchAgents")
                .join(format!("{LABEL}.plist")),
        }
    }
    /// Where a launch agent's output goes. Linux keeps it in the journal.
    pub fn log_file(&self) -> PathBuf {
        self.home.join("Library/Logs/orbit-server.log")
    }
    fn launchd_target(&self) -> String {
        format!("gui/{}", self.uid)
    }
    fn launchd_service(&self) -> String {
        format!("gui/{}/{LABEL}", self.uid)
    }
}
#[cfg(unix)]
fn uid() -> u32 {
    // SAFETY: getuid has no preconditions and can't fail.
    unsafe { libc::getuid() }
}
#[cfg(not(unix))]
fn uid() -> u32 {
    0
}

/// What a command printed, and whether it worked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ran {
    pub ok: bool,
    pub out: String,
}
/// Runs a program with its arguments: the real one runs it, tests fake it.
pub type Run<'a> = &'a dyn Fn(&str, &[&str]) -> Result<Ran, String>;
/// Runs a program and waits for it.
pub fn run(program: &str, args: &[&str]) -> Result<Ran, String> {
    let out = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Orbit can't run {program}: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(Ran {
        ok: out.status.success(),
        out: text,
    })
}
/// A step that must work.
fn must(run: Run, program: &str, args: &[&str]) -> Result<Ran, String> {
    let ran = run(program, args)?;
    if ran.ok {
        Ok(ran)
    } else {
        Err(format!(
            "{program} {} didn't work: {}",
            args.join(" "),
            ran.out.trim()
        ))
    }
}

/// How the service starts Orbit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub data: PathBuf,
    pub port: Option<u16>,
    /// The data folder `orbit-server` uses when none is given, so a command
    /// shown later can leave `--data` out.
    pub default_data: Option<PathBuf>,
}

/// Copies the program and its page into their own folder, writes the
/// service and starts it. Returns what to tell you.
pub fn install(
    host: &Host,
    run: Run,
    exe: &Path,
    web: &Path,
    setup: &Setup,
) -> Result<String, String> {
    let program = host.programs.join(if cfg!(windows) {
        "orbit-server.exe"
    } else {
        "orbit-server"
    });
    let page = host.programs.join("web");
    std::fs::create_dir_all(&host.programs)
        .map_err(|e| format!("Can't make {}: {e}", host.programs.display()))?;
    copy_program(exe, &program)?;
    copy_page(web, &page)?;
    let mut args = vec![
        text(&program, "the program's folder")?,
        "--data".into(),
        text(&setup.data, "the data folder")?,
        "--web".into(),
        text(&page, "the program's folder")?,
    ];
    if let Some(port) = setup.port {
        args.extend(["--port".into(), port.to_string()]);
    }
    let path = host
        .path
        .to_str()
        .ok_or("Orbit can only record a PATH that is plain text.")?;
    let file = host.service_file();
    let contents = match host.kind {
        Kind::Systemd => unit(&args, path)?,
        Kind::Launchd => plist(&args, path, &text(&host.log_file(), "your home folder")?)?,
    };
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("Can't make {}: {e}", folder.display()))?;
    }
    if host.kind == Kind::Launchd {
        if let Some(logs) = host.log_file().parent() {
            std::fs::create_dir_all(logs)
                .map_err(|e| format!("Can't make {}: {e}", logs.display()))?;
        }
    }
    write_replacing(&file, contents.as_bytes())?;
    let file_text = text(&file, "your home folder")?;
    let mut notes = vec![];
    match host.kind {
        Kind::Systemd => {
            must(run, "systemctl", &["--user", "daemon-reload"])?;
            must(run, "systemctl", &["--user", "enable", UNIT])?;
            // Starts it, or restarts one already running on the new copy.
            must(run, "systemctl", &["--user", "restart", UNIT])?;
            let uid = host.uid.to_string();
            let linger = run(
                "loginctl",
                &["show-user", &uid, "--property=Linger", "--value"],
            );
            if linger.is_ok_and(|l| l.out.trim() == "no") {
                notes.push("Linux stops your services when you log out. To keep Orbit running then, and start it when the computer starts, run: sudo loginctl enable-linger $USER".to_string());
            }
        }
        Kind::Launchd => {
            let service = host.launchd_service();
            // One already loaded is replaced; none loaded is fine.
            let _ = run("launchctl", &["bootout", &service]);
            let _ = run("launchctl", &["enable", &service]);
            let target = host.launchd_target();
            bootstrap(run, &target, &file_text)?;
            notes.push("A launch agent runs while you're logged in. On a Mac that stays on, keep your account logged in, or turn on automatic login.".to_string());
        }
    }
    let (found, missing) = tools(&host.path);
    let mut lines = vec![
        "Orbit is set up as a service and running.".to_string(),
        format!("Program: {}", host.programs.display()),
        format!("Data folder: {}", setup.data.display()),
        format!("Service: {}", file.display()),
    ];
    lines.push(format!("Found on its PATH: {}.", list(&found)));
    if !missing.is_empty() {
        lines.push(format!("Not found: {}.", list(&missing)));
    }
    for needed in ["gh", "git"] {
        if missing.contains(&needed) {
            lines.push(format!("Orbit needs {needed}. Install it, then run orbit-server install again so the service finds it."));
        }
    }
    if !["codex", "claude", "agy"]
        .iter()
        .any(|reviewer| found.contains(reviewer))
    {
        lines.push("No reviewer was found. Install Codex, Claude Code or Antigravity, then run orbit-server install again so the service finds it.".into());
    }
    lines.extend(notes);
    // The copy isn't on your PATH, so commands name it in full.
    let me = format!("\"{}\"", program.display());
    let data = data_option(setup);
    lines.push(format!(
        "See the link for your other devices with: {me} link{data}"
    ));
    lines.push(format!(
        "See how it's doing with {me} status, and its log with {me} logs."
    ));
    Ok(lines.join("\n"))
}

/// `--data <folder>` for a command shown to you, unless it's the default.
fn data_option(setup: &Setup) -> String {
    if Some(&setup.data) == setup.default_data.as_ref() {
        String::new()
    } else {
        format!(" --data \"{}\"", setup.data.display())
    }
}

/// How long to wait before loading a launch agent again.
const PAUSE: std::time::Duration =
    std::time::Duration::from_millis(if cfg!(test) { 1 } else { 500 });

/// Loads a launch agent. One just unloaded can take a moment to go, so a
/// refusal is tried again for a few seconds.
fn bootstrap(run: Run, target: &str, file: &str) -> Result<(), String> {
    let mut tries = 0;
    loop {
        match must(run, "launchctl", &["bootstrap", target, file]) {
            Ok(_) => return Ok(()),
            Err(_) if tries < 10 => {
                tries += 1;
                std::thread::sleep(PAUSE);
            }
            Err(e) => return Err(e),
        }
    }
}

/// Stops the service and removes it and the program's copy. The data
/// folder stays.
pub fn uninstall(host: &Host, run: Run) -> Result<String, String> {
    let file = host.service_file();
    if !file.exists() && !host.programs.exists() {
        return Ok("Orbit isn't set up as a service here.".into());
    }
    match host.kind {
        Kind::Systemd => {
            if file.exists() {
                must(run, "systemctl", &["--user", "disable", "--now", UNIT])?;
                remove_file(&file)?;
                must(run, "systemctl", &["--user", "daemon-reload"])?;
            }
        }
        Kind::Launchd => {
            // It may not be loaded, such as after a restart while logged out.
            let _ = run("launchctl", &["bootout", &host.launchd_service()]);
            if file.exists() {
                remove_file(&file)?;
            }
        }
    }
    if host.programs.exists() {
        std::fs::remove_dir_all(&host.programs)
            .map_err(|e| format!("Can't remove {}: {e}", host.programs.display()))?;
    }
    Ok("Orbit's service is stopped and removed, with its copy of the program. Its data folder is kept.".into())
}

/// How the service is doing, in words.
pub fn status(host: &Host, run: Run) -> Result<String, String> {
    let file = host.service_file();
    if !file.exists() {
        return Ok(
            "Orbit isn't set up as a service here. Set it up with orbit-server install.".into(),
        );
    }
    let state = match host.kind {
        Kind::Systemd => {
            let active = run("systemctl", &["--user", "is-active", UNIT])?;
            let enabled = run("systemctl", &["--user", "is-enabled", UNIT])?;
            let state = match active.out.trim() {
                "active" | "reloading" => "running",
                "activating" => "starting",
                "deactivating" => "stopping",
                "failed" => "stopped after a problem",
                _ => "stopped",
            };
            if enabled.out.trim() == "enabled" {
                format!("Orbit's service is {state}, and starts when you log in or the computer starts.")
            } else {
                format!("Orbit's service is {state}, but it's turned off, so it won't start on its own. Run orbit-server install to turn it back on.")
            }
        }
        Kind::Launchd => {
            let printed = run("launchctl", &["print", &host.launchd_service()])?;
            if !printed.ok {
                "Orbit's service is set up but not loaded. Run orbit-server install to load it again.".to_string()
            } else if printed
                .out
                .lines()
                .any(|line| line.trim() == "state = running")
            {
                "Orbit's service is running, and starts when you log in.".to_string()
            } else {
                "Orbit's service is loaded but not running. If it keeps stopping, orbit-server logs shows why.".to_string()
            }
        }
    };
    Ok(format!(
        "{state}\nService: {}\nSee its log with orbit-server logs.",
        file.display()
    ))
}

/// The command that shows the service's log, following it if asked, or
/// `None` when a launch agent hasn't written its log yet.
pub fn log_command(host: &Host, follow: bool) -> Option<(String, Vec<String>)> {
    if host.kind == Kind::Launchd && !host.log_file().exists() {
        return None;
    }
    let mut args: Vec<String> = match host.kind {
        Kind::Systemd => ["--user", "--unit", UNIT, "--lines", "200", "--no-pager"]
            .map(String::from)
            .into(),
        Kind::Launchd => vec![
            "-n".into(),
            "200".into(),
            host.log_file().display().to_string(),
        ],
    };
    if follow {
        args.insert(0, "-f".into());
    }
    let program = match host.kind {
        Kind::Systemd => "journalctl",
        Kind::Launchd => "tail",
    };
    Some((program.into(), args))
}

/// The systemd user service.
pub fn unit(args: &[String], path: &str) -> Result<String, String> {
    let command = args
        .iter()
        .map(|a| systemd_quoted(a, true))
        .collect::<Result<Vec<_>, _>>()?
        .join(" ");
    let path = systemd_quoted(&format!("PATH={path}"), false)?;
    Ok(format!(
        "# Written by orbit-server install. Run it again to change this, or
# orbit-server uninstall to remove it.
[Unit]
Description=Orbit without a window

[Service]
ExecStart={command}
Environment={path}
# A stop signal makes Orbit stop what it runs and save; whatever is left
# after a minute is stopped for it.
KillMode=mixed
TimeoutStopSec=60
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
"
    ))
}
/// A value in double quotes as systemd reads it. In a command line `$` starts
/// a variable, and everywhere `%` starts a specifier, so both are doubled.
fn systemd_quoted(value: &str, command: bool) -> Result<String, String> {
    if value.chars().any(char::is_control) {
        return Err(format!(
            "Orbit can't put a line break or other control character in a service: {value:?}"
        ));
    }
    let mut quoted = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '%' => quoted.push_str("%%"),
            '$' if command => quoted.push_str("$$"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

/// The launch agent.
pub fn plist(args: &[String], path: &str, log: &str) -> Result<String, String> {
    let strings = |values: &[String], indent: &str| -> Result<String, String> {
        values
            .iter()
            .map(|v| Ok(format!("{indent}<string>{}</string>", xml(v)?)))
            .collect::<Result<Vec<_>, String>>()
            .map(|lines| lines.join("\n"))
    };
    let log = xml(log)?;
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<!-- Written by orbit-server install. Run it again to change this, or orbit-server uninstall to remove it. -->
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
{args}
	</array>
	<key>EnvironmentVariables</key>
	<dict>
		<key>PATH</key>
		<string>{path}</string>
	</dict>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<key>ThrottleInterval</key>
	<integer>10</integer>
	<key>ExitTimeOut</key>
	<integer>60</integer>
	<key>StandardOutPath</key>
	<string>{log}</string>
	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>
"#,
        args = strings(args, "\t\t")?,
        path = xml(path)?,
    ))
}
/// Text for an XML element. XML has no way to write most control
/// characters, so they are refused.
fn xml(value: &str) -> Result<String, String> {
    if value.chars().any(char::is_control) {
        return Err(format!(
            "Orbit can't put a line break or other control character in a service: {value:?}"
        ));
    }
    Ok(value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;"))
}

/// The tools found on `path` and those that aren't, in [`TOOLS`] order.
fn tools(path: &OsString) -> (Vec<&'static str>, Vec<&'static str>) {
    let folders: Vec<PathBuf> = std::env::split_paths(path).collect();
    TOOLS
        .iter()
        .partition(|tool| folders.iter().any(|folder| runnable(&folder.join(tool))))
}
fn runnable(file: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(file).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        file.with_extension("exe").is_file()
    }
}
fn list(names: &[&str]) -> String {
    if names.is_empty() {
        "none of them".into()
    } else {
        names.join(", ")
    }
}

fn text(path: &Path, what: &str) -> Result<String, String> {
    path.to_str().map(String::from).ok_or_else(|| {
        format!(
            "Orbit can only set up a service when {what} has a plain-text name: {}",
            path.display()
        )
    })
}
/// Copies the program beside the old copy and then over it, which works
/// even while the old copy runs, or is the program copying itself.
fn copy_program(from: &Path, to: &Path) -> Result<(), String> {
    let bytes = std::fs::read(from).map_err(|e| format!("Can't read {}: {e}", from.display()))?;
    write_replacing(to, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(to, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Can't make {} runnable: {e}", to.display()))?;
    }
    Ok(())
}
/// Copies the page's folder beside the old copy and then in its place, which
/// also works when the page is that copy.
fn copy_page(from: &Path, to: &Path) -> Result<(), String> {
    let fresh = to.with_extension("new");
    let old = to.with_extension("old");
    for leftover in [&fresh, &old] {
        if leftover.exists() {
            std::fs::remove_dir_all(leftover)
                .map_err(|e| format!("Can't remove {}: {e}", leftover.display()))?;
        }
    }
    copy_folder(from, &fresh)?;
    if to.exists() {
        std::fs::rename(to, &old).map_err(|e| format!("Can't replace {}: {e}", to.display()))?;
    }
    std::fs::rename(&fresh, to).map_err(|e| format!("Can't replace {}: {e}", to.display()))?;
    if old.exists() {
        let _ = std::fs::remove_dir_all(&old);
    }
    Ok(())
}
/// Copies files and folders, leaving out links: a built page has none.
fn copy_folder(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("Can't make {}: {e}", to.display()))?;
    let entries =
        std::fs::read_dir(from).map_err(|e| format!("Can't read {}: {e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Can't read {}: {e}", from.display()))?;
        let kind = entry
            .file_type()
            .map_err(|e| format!("Can't read {}: {e}", entry.path().display()))?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)
                .map_err(|e| format!("Can't copy {}: {e}", entry.path().display()))?;
        }
    }
    Ok(())
}
/// Writes beside the file and then over it, so nothing ever reads half of it.
fn write_replacing(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut temp = file.as_os_str().to_owned();
    temp.push(".new");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, bytes).map_err(|e| format!("Can't write {}: {e}", temp.display()))?;
    std::fs::rename(&temp, file).map_err(|e| format!("Can't write {}: {e}", file.display()))
}
fn remove_file(file: &Path) -> Result<(), String> {
    std::fs::remove_file(file).map_err(|e| format!("Can't remove {}: {e}", file.display()))
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
