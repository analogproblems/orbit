use super::*;
use std::cell::RefCell;

/// A fake of the system's tools: records each command and answers with
/// `answer`, which gets the command as one line.
struct Fake {
    calls: RefCell<Vec<String>>,
    answer: Box<dyn Fn(&str) -> Ran>,
}
impl Fake {
    fn new(answer: impl Fn(&str) -> Ran + 'static) -> Self {
        Self {
            calls: RefCell::new(vec![]),
            answer: Box::new(answer),
        }
    }
    fn working() -> Self {
        Self::new(|_| ok(""))
    }
    fn run(&self, program: &str, args: &[&str]) -> Result<Ran, String> {
        let line = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        self.calls.borrow_mut().push(line.clone());
        Ok((self.answer)(&line))
    }
    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}
fn ok(out: &str) -> Ran {
    Ran {
        ok: true,
        out: out.into(),
    }
}
fn failed(out: &str) -> Ran {
    Ran {
        ok: false,
        out: out.into(),
    }
}

/// A home folder with a built program, its page, and some tools on PATH.
struct Machine {
    _dir: tempfile::TempDir,
    root: PathBuf,
    host: Host,
    exe: PathBuf,
    web: PathBuf,
    setup: Setup,
}
fn machine(kind: Kind) -> Machine {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let home = root.join("home");
    let bin = root.join("bin");
    let web = root.join("checkout/dist");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(web.join("index.html"), "<html></html>").unwrap();
    std::fs::write(web.join("assets/app.js"), "start()").unwrap();
    let exe = root.join("checkout/orbit-server");
    std::fs::write(&exe, "program v1").unwrap();
    for tool in ["gh", "git", "codex"] {
        std::fs::write(bin.join(tool), "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(bin.join(tool), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
    }
    // Not runnable, so not found.
    std::fs::write(bin.join("claude"), "").unwrap();
    let data = root.join("data");
    let host = Host {
        kind,
        config: home.join(".config"),
        programs: match kind {
            Kind::Systemd => home.join(".local/share/orbit-server"),
            Kind::Launchd => home.join("Library/Application Support/orbit-server"),
        },
        home,
        path: std::env::join_paths([bin, root.join("more bin")]).unwrap(),
        uid: 501,
    };
    Machine {
        _dir: dir,
        root,
        host,
        exe,
        web,
        setup: Setup {
            data: data.clone(),
            port: None,
            default_data: Some(data),
        },
    }
}

#[cfg(unix)]
#[test]
fn sets_up_a_user_service_on_linux() {
    let m = machine(Kind::Systemd);
    let fake = Fake::new(|line| match line {
        "loginctl show-user 501 --property=Linger --value" => ok("no\n"),
        _ => ok(""),
    });
    let done = install(&m.host, &|p, a| fake.run(p, a), &m.exe, &m.web, &m.setup).unwrap();
    assert_eq!(
        fake.calls(),
        [
            "systemctl --user daemon-reload",
            "systemctl --user enable orbit.service",
            "systemctl --user restart orbit.service",
            "loginctl show-user 501 --property=Linger --value",
        ]
    );
    // The program and its page have a folder of their own.
    let program = m.host.programs.join("orbit-server");
    assert_eq!(std::fs::read_to_string(&program).unwrap(), "program v1");
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&program).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
    let page = m.host.programs.join("web");
    assert_eq!(
        std::fs::read_to_string(page.join("assets/app.js")).unwrap(),
        "start()"
    );
    let file = m.host.home.join(".config/systemd/user/orbit.service");
    assert_eq!(m.host.service_file(), file);
    let unit_text = std::fs::read_to_string(&file).unwrap();
    let path = m.host.path.to_str().unwrap();
    assert_eq!(
        unit_text,
        unit(
            &[
                program.display().to_string(),
                "--data".into(),
                m.setup.data.display().to_string(),
                "--web".into(),
                page.display().to_string(),
            ],
            path
        )
        .unwrap()
    );
    assert!(unit_text.contains(&format!("\nEnvironment=\"PATH={path}\"\n")));
    assert_eq!(
        done,
        format!(
            "Orbit is set up as a service and running.
Program: {}
Data folder: {}
Service: {}
Found on its PATH: gh, git, codex.
Not found: claude, agy, docker, devcontainer, tailscale.
Linux stops your services when you log out. To keep Orbit running then, and start it when the computer starts, run: sudo loginctl enable-linger $USER
See the link for your other devices with: \"{program}\" link
See how it's doing with \"{program}\" status, and its log with \"{program}\" logs.",
            m.host.programs.display(),
            m.setup.data.display(),
            file.display(),
            program = program.display(),
        )
    );
}

#[cfg(unix)]
#[test]
fn installing_again_replaces_the_copy_and_keeps_its_port_and_folder() {
    let mut m = machine(Kind::Systemd);
    let fake = Fake::new(|line| match line {
        "loginctl show-user 501 --property=Linger --value" => ok("yes\n"),
        _ => ok(""),
    });
    let run = |p: &str, a: &[&str]| fake.run(p, a);
    install(&m.host, &run, &m.exe, &m.web, &m.setup).unwrap();
    // A copy left half made by an install that was cut short goes.
    std::fs::create_dir_all(m.host.programs.join("web.new")).unwrap();
    std::fs::write(m.host.programs.join("web.new/stale.js"), "old()").unwrap();
    std::fs::write(&m.exe, "program v2").unwrap();
    std::fs::remove_file(m.web.join("assets/app.js")).unwrap();
    std::fs::write(m.web.join("assets/next.js"), "next()").unwrap();
    m.setup.data = m.root.join("other data");
    m.setup.port = Some(5000);
    let done = install(&m.host, &run, &m.exe, &m.web, &m.setup).unwrap();
    let program = m.host.programs.join("orbit-server");
    assert_eq!(std::fs::read_to_string(&program).unwrap(), "program v2");
    let page = m.host.programs.join("web");
    assert!(!page.join("assets/app.js").exists());
    assert!(page.join("assets/next.js").is_file());
    assert!(!m.host.programs.join("web.new").exists());
    assert!(!page.join("stale.js").exists());
    assert!(!m.host.programs.join("web.old").exists());
    let unit_text = std::fs::read_to_string(m.host.service_file()).unwrap();
    assert!(unit_text.contains(&format!(
        "ExecStart=\"{}\" \"--data\" \"{}\" \"--web\" \"{}\" \"--port\" \"5000\"\n",
        program.display(),
        m.setup.data.display(),
        page.display()
    )));
    // Linger is on, so there's no note about it; the data folder isn't the
    // default, so the link command names it.
    assert!(!done.contains("enable-linger"), "{done}");
    assert!(done.contains(&format!(
        "\"{}\" link --data \"{}\"",
        program.display(),
        m.setup.data.display()
    )));
    // Installing from the copy itself leaves it as it is.
    let copy = m.host.programs.join("orbit-server");
    install(&m.host, &run, &copy, &page, &m.setup).unwrap();
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), "program v2");
    assert!(page.join("assets/next.js").is_file());
}

#[cfg(unix)]
#[test]
fn says_what_is_missing_from_the_path() {
    let mut m = machine(Kind::Systemd);
    // Folders named like a tool aren't the tool.
    let empty = m.root.join("empty");
    for tool in ["gh", "git", "codex"] {
        std::fs::create_dir_all(empty.join(tool)).unwrap();
    }
    m.host.path = OsString::from(empty);
    let fake = Fake::working();
    let done = install(&m.host, &|p, a| fake.run(p, a), &m.exe, &m.web, &m.setup).unwrap();
    for line in [
        "Found on its PATH: none of them.",
        "Not found: gh, git, codex, claude, agy, docker, devcontainer, tailscale.",
        "Orbit needs gh. Install it, then run orbit-server install again so the service finds it.",
        "Orbit needs git. Install it, then run orbit-server install again so the service finds it.",
        "No reviewer was found. Install Codex, Claude Code or Antigravity, then run orbit-server install again so the service finds it.",
    ] {
        assert!(done.lines().any(|l| l == line), "{line}\n{done}");
    }
}

#[test]
fn stops_when_the_system_refuses() {
    let m = machine(Kind::Systemd);
    let fake = Fake::new(|line| match line {
        "systemctl --user enable orbit.service" => failed("Failed to connect to bus\n"),
        _ => ok(""),
    });
    let error = install(&m.host, &|p, a| fake.run(p, a), &m.exe, &m.web, &m.setup).unwrap_err();
    assert_eq!(
        error,
        "systemctl --user enable orbit.service didn't work: Failed to connect to bus"
    );
    assert_eq!(fake.calls().len(), 2);
    // A page folder with no page is refused before anything is copied by
    // `orbit-server` itself (`web_folder`); a missing program is refused here.
    let missing = m.root.join("missing");
    assert!(
        install(&m.host, &|p, a| fake.run(p, a), &missing, &m.web, &m.setup)
            .unwrap_err()
            .starts_with(&format!("Can't read {}", missing.display()))
    );
}

#[test]
fn gives_up_on_a_launch_agent_that_never_loads() {
    let m = machine(Kind::Launchd);
    let fake = Fake::new(|line| {
        if line.starts_with("launchctl bootstrap") {
            failed("Bootstrap failed: 5: Input/output error\n")
        } else {
            ok("")
        }
    });
    let error = install(&m.host, &|p, a| fake.run(p, a), &m.exe, &m.web, &m.setup).unwrap_err();
    assert_eq!(
        error,
        format!(
            "launchctl bootstrap gui/501 {} didn't work: Bootstrap failed: 5: Input/output error",
            m.host.service_file().display()
        )
    );
    // The first try and ten more.
    let tries = fake.calls();
    assert_eq!(
        tries
            .iter()
            .filter(|l| l.starts_with("launchctl bootstrap"))
            .count(),
        11
    );
}

#[cfg(unix)]
#[test]
fn sets_up_a_launch_agent_on_macos() {
    let m = machine(Kind::Launchd);
    let tries = std::cell::Cell::new(0);
    let fake = Fake::new(move |line| {
        if line.starts_with("launchctl bootstrap") {
            // The old one is still going the first time.
            tries.set(tries.get() + 1);
            if tries.get() == 1 {
                return failed("Bootstrap failed: 5: Input/output error\n");
            }
        }
        ok("")
    });
    let done = install(&m.host, &|p, a| fake.run(p, a), &m.exe, &m.web, &m.setup).unwrap();
    let file = m
        .host
        .home
        .join("Library/LaunchAgents/dev.orbit.review.server.plist");
    assert_eq!(m.host.service_file(), file);
    let bootstrap = format!("launchctl bootstrap gui/501 {}", file.display());
    assert_eq!(
        fake.calls(),
        [
            "launchctl bootout gui/501/dev.orbit.review.server".to_string(),
            "launchctl enable gui/501/dev.orbit.review.server".into(),
            bootstrap.clone(),
            bootstrap,
        ]
    );
    let program = m.host.programs.join("orbit-server");
    assert_eq!(std::fs::read_to_string(&program).unwrap(), "program v1");
    let log = m.host.home.join("Library/Logs/orbit-server.log");
    assert_eq!(m.host.log_file(), log);
    assert!(log.parent().unwrap().is_dir());
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        plist(
            &[
                program.display().to_string(),
                "--data".into(),
                m.setup.data.display().to_string(),
                "--web".into(),
                m.host.programs.join("web").display().to_string(),
            ],
            m.host.path.to_str().unwrap(),
            log.to_str().unwrap()
        )
        .unwrap()
    );
    assert!(
        done.contains("A launch agent runs while you're logged in."),
        "{done}"
    );
    assert!(!done.contains("enable-linger"));
}

#[test]
fn writes_a_unit_systemd_reads_as_given() {
    let args = [
        "/home/me/.local/share/orbit-server/orbit-server".to_string(),
        "--data".into(),
        "/srv/100% \"odd\" $HOME \\ data".into(),
    ];
    let text = unit(&args, "/usr/bin:/home/me/$bin%").unwrap();
    assert_eq!(
        text,
        r#"# Written by orbit-server install. Run it again to change this, or
# orbit-server uninstall to remove it.
[Unit]
Description=Orbit without a window

[Service]
ExecStart="/home/me/.local/share/orbit-server/orbit-server" "--data" "/srv/100%% \"odd\" $$HOME \\ data"
Environment="PATH=/usr/bin:/home/me/$bin%%"
# A stop signal makes Orbit stop what it runs and save; whatever is left
# after a minute is stopped for it.
KillMode=mixed
TimeoutStopSec=60
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
"#
    );
    for bad in ["/srv/a\nExecStartPre=/bin/evil", "/srv/\u{7}"] {
        let error = unit(&["/bin/orbit-server".into(), bad.into()], "/usr/bin").unwrap_err();
        assert!(error.starts_with("Orbit can't put a line break"), "{error}");
    }
    assert!(unit(&["/bin/orbit-server".into()], "/usr/bin\n/evil").is_err());
}

#[test]
fn writes_a_launch_agent_with_its_text_escaped() {
    let text = plist(
        &["/Users/me/orbit-server".into(), "/Users/me/<a&b>'\"".into()],
        "/usr/bin:/opt/homebrew/bin",
        "/Users/me/Library/Logs/orbit-server.log",
    )
    .unwrap();
    assert_eq!(
        text,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<!-- Written by orbit-server install. Run it again to change this, or orbit-server uninstall to remove it. -->
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>dev.orbit.review.server</string>
	<key>ProgramArguments</key>
	<array>
		<string>/Users/me/orbit-server</string>
		<string>/Users/me/&lt;a&amp;b&gt;&apos;&quot;</string>
	</array>
	<key>EnvironmentVariables</key>
	<dict>
		<key>PATH</key>
		<string>/usr/bin:/opt/homebrew/bin</string>
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
	<string>/Users/me/Library/Logs/orbit-server.log</string>
	<key>StandardErrorPath</key>
	<string>/Users/me/Library/Logs/orbit-server.log</string>
</dict>
</plist>
"#
    );
    assert!(plist(&["/a\nb".into()], "/usr/bin", "/log").is_err());
    assert!(plist(&["/a".into()], "/usr/bin\u{0}", "/log").is_err());
}

#[test]
fn removes_the_service_and_its_copy_but_keeps_the_data() {
    for kind in [Kind::Systemd, Kind::Launchd] {
        let m = machine(kind);
        let fake = Fake::working();
        let run = |p: &str, a: &[&str]| fake.run(p, a);
        assert_eq!(
            uninstall(&m.host, &run).unwrap(),
            "Orbit isn't set up as a service here."
        );
        assert!(fake.calls().is_empty());
        install(&m.host, &run, &m.exe, &m.web, &m.setup).unwrap();
        std::fs::create_dir_all(&m.setup.data).unwrap();
        fake.calls.borrow_mut().clear();
        assert_eq!(
            uninstall(&m.host, &run).unwrap(),
            "Orbit's service is stopped and removed, with its copy of the program. Its data folder is kept."
        );
        let expected: Vec<String> = match kind {
            Kind::Systemd => vec![
                "systemctl --user disable --now orbit.service".into(),
                "systemctl --user daemon-reload".into(),
            ],
            Kind::Launchd => vec!["launchctl bootout gui/501/dev.orbit.review.server".into()],
        };
        assert_eq!(fake.calls(), expected, "{kind:?}");
        assert!(!m.host.service_file().exists());
        assert!(!m.host.programs.exists());
        assert!(m.setup.data.is_dir());

        // A copy left behind without its service is removed too.
        install(&m.host, &run, &m.exe, &m.web, &m.setup).unwrap();
        std::fs::remove_file(m.host.service_file()).unwrap();
        fake.calls.borrow_mut().clear();
        assert_eq!(
            uninstall(&m.host, &run).unwrap(),
            "Orbit's service is stopped and removed, with its copy of the program. Its data folder is kept."
        );
        assert!(!m.host.programs.exists());
        let expected: Vec<String> = match kind {
            Kind::Systemd => vec![],
            Kind::Launchd => vec!["launchctl bootout gui/501/dev.orbit.review.server".into()],
        };
        assert_eq!(fake.calls(), expected, "{kind:?}");
    }
}

#[test]
fn says_how_the_service_is_doing() {
    let m = machine(Kind::Systemd);
    let none = Fake::working();
    assert_eq!(
        status(&m.host, &|p, a| none.run(p, a)).unwrap(),
        "Orbit isn't set up as a service here. Set it up with orbit-server install."
    );
    assert!(none.calls().is_empty());
    install(&m.host, &|p, a| none.run(p, a), &m.exe, &m.web, &m.setup).unwrap();
    let file = m.host.service_file().display().to_string();
    for (active, enabled, said) in [
        ("active", "enabled", "Orbit's service is running, and starts when you log in or the computer starts."),
        ("activating", "enabled", "Orbit's service is starting, and starts when you log in or the computer starts."),
        ("deactivating", "enabled", "Orbit's service is stopping, and starts when you log in or the computer starts."),
        ("failed", "enabled", "Orbit's service is stopped after a problem, and starts when you log in or the computer starts."),
        ("inactive", "disabled", "Orbit's service is stopped, but it's turned off, so it won't start on its own. Run orbit-server install to turn it back on."),
    ] {
        let fake = Fake::new(move |line| match line {
            "systemctl --user is-active orbit.service" => Ran { ok: active == "active", out: format!("{active}\n") },
            "systemctl --user is-enabled orbit.service" => Ran { ok: enabled == "enabled", out: format!("{enabled}\n") },
            _ => failed("unexpected"),
        });
        assert_eq!(
            status(&m.host, &|p, a| fake.run(p, a)).unwrap(),
            format!("{said}\nService: {file}\nSee its log with orbit-server logs.")
        );
    }

    let m = machine(Kind::Launchd);
    let quiet = Fake::working();
    install(&m.host, &|p, a| quiet.run(p, a), &m.exe, &m.web, &m.setup).unwrap();
    let file = m.host.service_file().display().to_string();
    for (printed, said) in [
        (failed("Could not find service"), "Orbit's service is set up but not loaded. Run orbit-server install to load it again."),
        (ok("gui/501/dev.orbit.review.server = {\n\tstate = running\n\tpid = 42\n}"), "Orbit's service is running, and starts when you log in."),
        (ok("gui/501/dev.orbit.review.server = {\n\tstate = not running\n}"), "Orbit's service is loaded but not running. If it keeps stopping, orbit-server logs shows why."),
    ] {
        let fake = Fake::new(move |line| {
            assert_eq!(line, "launchctl print gui/501/dev.orbit.review.server");
            printed.clone()
        });
        assert_eq!(
            status(&m.host, &|p, a| fake.run(p, a)).unwrap(),
            format!("{said}\nService: {file}\nSee its log with orbit-server logs.")
        );
    }
}

#[test]
fn shows_the_log_where_each_system_keeps_it() {
    let linux = machine(Kind::Systemd);
    assert_eq!(
        log_command(&linux.host, false).unwrap(),
        (
            "journalctl".to_string(),
            [
                "--user",
                "--unit",
                "orbit.service",
                "--lines",
                "200",
                "--no-pager"
            ]
            .map(String::from)
            .to_vec()
        )
    );
    assert_eq!(log_command(&linux.host, true).unwrap().1[0], "-f");
    let mac = machine(Kind::Launchd);
    // A launch agent that hasn't started yet has no log to show.
    assert_eq!(log_command(&mac.host, false), None);
    let log = mac.host.log_file();
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, "Orbit is running.\n").unwrap();
    let log = log.display().to_string();
    assert_eq!(
        log_command(&mac.host, false).unwrap(),
        (
            "tail".to_string(),
            vec!["-n".into(), "200".into(), log.clone()]
        )
    );
    assert_eq!(
        log_command(&mac.host, true).unwrap().1,
        ["-f".to_string(), "-n".into(), "200".into(), log]
    );
}

#[test]
fn finds_the_folders_each_system_uses() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            vars.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| OsString::from(v))
        }
    };
    let linux = Host::of(
        Kind::Systemd,
        env(&[("HOME", "/home/me"), ("PATH", "/usr/bin:/home/me/bin")]),
        1000,
    )
    .unwrap();
    assert_eq!(
        linux,
        Host {
            kind: Kind::Systemd,
            home: "/home/me".into(),
            config: "/home/me/.config".into(),
            programs: "/home/me/.local/share/orbit-server".into(),
            path: "/usr/bin:/home/me/bin".into(),
            uid: 1000,
        }
    );
    let moved = Host::of(
        Kind::Systemd,
        env(&[
            ("HOME", "/home/me"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_DATA_HOME", "/data"),
        ]),
        1000,
    )
    .unwrap();
    assert_eq!(moved.config, PathBuf::from("/cfg"));
    assert_eq!(moved.programs, PathBuf::from("/data/orbit-server"));
    assert_eq!(
        moved.service_file(),
        PathBuf::from("/cfg/systemd/user/orbit.service")
    );
    assert_eq!(moved.path, OsString::new());
    // Relative XDG folders are ignored, as the XDG rules say.
    let relative = Host::of(
        Kind::Systemd,
        env(&[
            ("HOME", "/home/me"),
            ("XDG_CONFIG_HOME", "cfg"),
            ("XDG_DATA_HOME", "data"),
        ]),
        1000,
    )
    .unwrap();
    assert_eq!(relative.config, linux.config);
    assert_eq!(relative.programs, linux.programs);
    let mac = Host::of(
        Kind::Launchd,
        env(&[("HOME", "/Users/me"), ("XDG_DATA_HOME", "/data")]),
        501,
    )
    .unwrap();
    assert_eq!(
        mac.programs,
        PathBuf::from("/Users/me/Library/Application Support/orbit-server")
    );
    assert_eq!(
        mac.service_file(),
        PathBuf::from("/Users/me/Library/LaunchAgents/dev.orbit.review.server.plist")
    );
    assert_eq!(
        Host::of(Kind::Systemd, env(&[("HOME", "")]), 1000),
        Err("Orbit can't find your home folder (HOME isn't set).".into())
    );
}

#[cfg(target_os = "linux")]
#[test]
fn finds_this_computers_folders() {
    let host = Host::current().unwrap();
    assert_eq!(host.kind, Kind::Systemd);
    assert_eq!(host.uid, unsafe { libc::getuid() });
    assert!(host.programs.ends_with("orbit-server"));
    assert!(host.service_file().ends_with("systemd/user/orbit.service"));
}
