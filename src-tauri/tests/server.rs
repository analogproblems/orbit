//! Starts the real `orbit-server` program on an empty data folder, uses it
//! the way a browser with the link does, and stops it with a stop signal.
//! Its PATH is empty, so it reaches no reviewer, GitHub or Tailscale.
#![cfg(unix)]
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

const PROGRAM: &str = env!("CARGO_BIN_EXE_orbit-server");
const WAIT: Duration = Duration::from_secs(60);

fn orbit_server(home: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(PROGRAM);
    command
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("PATH", home.join("no-tools"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
/// A free port below the ephemeral range, as in `remote_tests.rs`, so no
/// client connection takes it before the server listens.
fn free_port() -> u16 {
    use std::sync::atomic::{AtomicU16, Ordering};
    static NEXT: AtomicU16 = AtomicU16::new(0);
    let base = 30_000 + (std::process::id() % 500) as u16 * 20;
    loop {
        let port = base + NEXT.fetch_add(1, Ordering::SeqCst) % 10_000;
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
}
/// Passes on what the program prints, a line at a time.
fn lines(child: &mut Child) -> mpsc::Receiver<String> {
    let (send, lines) = mpsc::channel();
    let out = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if send.send(line).is_err() {
                break;
            }
        }
    });
    lines
}
fn wait_for_line(lines: &mpsc::Receiver<String>, start: &str) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match lines.recv_timeout(left) {
            Ok(line) if line.starts_with(start) => return line,
            Ok(_) => {}
            Err(e) => panic!("orbit-server never printed {start:?}: {e}"),
        }
    }
}
fn wait_for_exit(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("orbit-server didn't stop");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn link_of(output: &Output, port: u16) -> String {
    assert!(output.status.success(), "{}", text(&output.stderr));
    let printed = text(&output.stdout);
    let prefix = format!("http://127.0.0.1:{port}/#token=");
    let line = printed
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no link in {printed:?}"));
    let token = &line[prefix.len()..];
    assert_eq!(token.len(), 64, "{line}");
    assert!(token.bytes().all(|b| b.is_ascii_hexdigit()), "{line}");
    token.into()
}

#[tokio::test]
async fn runs_orbit_without_a_window_and_saves_when_stopped() {
    let d = tempfile::tempdir().unwrap();
    let (home, data, web) = (
        d.path().join("home"),
        d.path().join("data"),
        d.path().join("web"),
    );
    for folder in [&home, &data, &web.join("assets")] {
        std::fs::create_dir_all(folder).unwrap();
    }
    std::fs::write(
        web.join("index.html"),
        "<!doctype html><html><head></head><body>Orbit</body></html>",
    )
    .unwrap();
    std::fs::write(web.join("assets/app.js"), "start()").unwrap();
    let port = free_port();
    let data_arg = data.to_str().unwrap();
    let port_arg = port.to_string();
    let mut server = orbit_server(
        &home,
        &[
            "--data",
            data_arg,
            "--port",
            &port_arg,
            "--web",
            web.to_str().unwrap(),
        ],
    )
    .spawn()
    .unwrap();
    let printed = lines(&mut server);
    assert_eq!(
        wait_for_line(&printed, "Listening on "),
        format!("Listening on http://127.0.0.1:{port}, on this computer only.")
    );
    // Outside a terminal the link, which works like a key, isn't printed.
    let hint = wait_for_line(&printed, "To see the link");
    assert_eq!(
        hint,
        format!(
            "To see the link for your other devices, run: orbit-server link --data \"{data_arg}\""
        )
    );
    // The port is kept for the data folder, so the link has it too.
    let saved: Value =
        serde_json::from_slice(&std::fs::read(data.join("remote.json")).unwrap()).unwrap();
    assert_eq!(saved, json!({"enabled": false, "port": port}));
    let token = link_of(
        &orbit_server(&home, &["link", "--data", data_arg])
            .output()
            .unwrap(),
        port,
    );

    // A second Orbit on the same folder stops before touching it.
    let other = orbit_server(
        &home,
        &[
            "--data",
            data_arg,
            "--port",
            &free_port().to_string(),
            "--web",
            web.to_str().unwrap(),
        ],
    )
    .output()
    .unwrap();
    assert_eq!(other.status.code(), Some(1));
    assert_eq!(
        text(&other.stderr),
        "Orbit can't start: Orbit is already running for this workspace.\n"
    );
    let saved: Value =
        serde_json::from_slice(&std::fs::read(data.join("remote.json")).unwrap()).unwrap();
    assert_eq!(saved["port"], port);

    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let page = client.get(format!("{base}/")).send().await.unwrap();
    assert_eq!(page.status(), 200);
    assert_eq!(
        page.text().await.unwrap(),
        format!(
            "<!doctype html><html><head>{}</head><body>Orbit</body></html>",
            orbit_lib::remote::SERVER_MARK
        )
    );
    let script = client
        .get(format!("{base}/assets/app.js"))
        .send()
        .await
        .unwrap();
    assert_eq!(script.headers()["content-type"], "text/javascript");
    assert_eq!(script.text().await.unwrap(), "start()");
    let missing = client
        .get(format!("{base}/../home/secret"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);

    let call = |command: &str, token: Option<&str>, args: Value| {
        let mut request = client.post(format!("{base}/api/{command}")).json(&args);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        request.send()
    };
    let refused = call("get_snapshot", None, json!({})).await.unwrap();
    assert_eq!(refused.status(), 401);
    let refused = call("get_snapshot", Some(&"0".repeat(64)), json!({}))
        .await
        .unwrap();
    assert_eq!(refused.status(), 401);
    // The desktop-only controls stay on the desktop, and here on the command line.
    for command in [
        "open_github",
        "set_remote_access",
        "reset_remote_link",
        "choose_checkout",
    ] {
        let refused = call(command, Some(&token), json!({})).await.unwrap();
        assert_eq!(refused.status(), 404, "{command}");
    }
    let snapshot: Value = call("get_snapshot", Some(&token), json!({}))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(snapshot["settings"]["pollSeconds"], 120);
    let changed = call(
        "update_settings",
        Some(&token),
        json!({"patch": {"pollSeconds": 300}}),
    )
    .await
    .unwrap();
    assert_eq!(changed.status(), 200, "{}", changed.text().await.unwrap());
    let snapshot: Value = call("get_snapshot", Some(&token), json!({}))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(snapshot["settings"]["pollSeconds"], 300);

    // A running Orbit keeps its link.
    let refused = orbit_server(&home, &["link", "--new", "--data", data_arg])
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(1));
    assert!(text(&refused.stderr).starts_with("Orbit is running on this data folder"));
    assert_eq!(
        link_of(
            &orbit_server(&home, &["link", "--data", data_arg])
                .output()
                .unwrap(),
            port
        ),
        token
    );

    assert_eq!(unsafe { libc::kill(server.id() as i32, libc::SIGTERM) }, 0);
    let status = wait_for_exit(&mut server);
    assert!(status.success(), "{status:?}");
    let mut rest = vec![];
    while let Ok(line) = printed.recv_timeout(Duration::from_secs(5)) {
        rest.push(line);
    }
    assert_eq!(rest, ["Stopping Orbit…", "Orbit stopped."]);
    let mut errors = String::new();
    server
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut errors)
        .unwrap();
    assert_eq!(errors, "");
    let state: Value =
        serde_json::from_slice(&std::fs::read(data.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["settings"]["pollSeconds"], 300);

    // Once it's stopped, a new link replaces the old one.
    let new = link_of(
        &orbit_server(&home, &["link", "--new", "--data", data_arg])
            .output()
            .unwrap(),
        port,
    );
    assert_ne!(new, token);
    assert_eq!(
        std::fs::read_to_string(data.join("remote-token")).unwrap(),
        new
    );
}

#[test]
fn says_what_is_wrong_before_starting() {
    let d = tempfile::tempdir().unwrap();
    let data = d.path().join("data");
    let empty = d.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let no_page = orbit_server(
        d.path(),
        &[
            "--data",
            data.to_str().unwrap(),
            "--web",
            empty.to_str().unwrap(),
        ],
    )
    .output()
    .unwrap();
    assert_eq!(no_page.status.code(), Some(1));
    assert_eq!(
        text(&no_page.stderr),
        format!(
            "Orbit can't start: There's no index.html in {}. Choose the folder that npm run build makes (dist) with --web.\n",
            empty.display()
        )
    );
    // Nothing was made in the data folder.
    assert!(!data.exists());
    let unknown = orbit_server(d.path(), &["--verbose"]).output().unwrap();
    assert_eq!(unknown.status.code(), Some(2));
    assert_eq!(
        text(&unknown.stderr),
        "Orbit doesn't know --verbose.\nRun orbit-server --help to see what it takes.\n"
    );
    let help = orbit_server(d.path(), &["--help"]).output().unwrap();
    assert!(help.status.success());
    assert!(text(&help.stdout).starts_with("Orbit without a window"));
    let version = orbit_server(d.path(), &["--version"]).output().unwrap();
    assert_eq!(
        text(&version.stdout),
        format!("orbit-server {}\n", env!("CARGO_PKG_VERSION"))
    );
}

/// In a terminal, Orbit shows the link and how to stop, and Ctrl+C stops it.
#[test]
fn shows_the_link_in_a_terminal_and_stops_on_ctrl_c() {
    use std::os::fd::{FromRawFd, OwnedFd};
    let d = tempfile::tempdir().unwrap();
    let (data, web) = (d.path().join("data"), d.path().join("web"));
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(web.join("index.html"), "<html><head></head></html>").unwrap();
    let (mut leader, mut follower) = (0, 0);
    let opened = unsafe {
        libc::openpty(
            &mut leader,
            &mut follower,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(opened, 0, "{}", std::io::Error::last_os_error());
    let (leader, follower) =
        unsafe { (OwnedFd::from_raw_fd(leader), OwnedFd::from_raw_fd(follower)) };
    let port = free_port();
    let mut server = {
        let mut command = orbit_server(
            d.path(),
            &[
                "--data",
                data.to_str().unwrap(),
                "--port",
                &port.to_string(),
                "--web",
                web.to_str().unwrap(),
            ],
        );
        command.stdout(Stdio::from(follower));
        command.spawn().unwrap()
    };
    let (send, printed) = mpsc::channel();
    std::thread::spawn(move || {
        // The terminal ends lines with \r\n, and reading fails once Orbit closes it.
        for line in BufReader::new(std::fs::File::from(leader))
            .lines()
            .map_while(Result::ok)
        {
            if send.send(line.trim_end_matches('\r').to_string()).is_err() {
                break;
            }
        }
    });
    let mut shown = vec![];
    loop {
        let line = printed
            .recv_timeout(WAIT)
            .expect("orbit-server never said how to stop");
        if line == "Press Ctrl+C to stop." {
            break;
        }
        shown.push(line);
    }
    let token = std::fs::read_to_string(data.join("remote-token")).unwrap();
    assert!(
        shown.contains(&format!("   http://127.0.0.1:{port}/#token={token}")),
        "{shown:?}"
    );
    assert!(
        !shown.iter().any(|line| line.starts_with("To see the link")),
        "{shown:?}"
    );
    assert_eq!(unsafe { libc::kill(server.id() as i32, libc::SIGINT) }, 0);
    let status = wait_for_exit(&mut server);
    assert!(status.success(), "{status:?}");
    let mut rest = vec![];
    while let Ok(line) = printed.recv_timeout(Duration::from_secs(5)) {
        rest.push(line);
    }
    assert_eq!(rest, ["Stopping Orbit…", "Orbit stopped."]);
    assert!(data.join("state.json").is_file());
}

/// Sets Orbit up as a user service with fake systemd tools, which write down
/// what they were asked, then checks on it, shows its log and removes it.
#[cfg(target_os = "linux")]
#[test]
fn sets_itself_up_as_a_service_and_removes_it() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let (home, bin, web, data) = (
        d.path().join("home"),
        d.path().join("bin"),
        d.path().join("web"),
        d.path().join("data"),
    );
    for folder in [&home, &bin, &web.join("assets")] {
        std::fs::create_dir_all(folder).unwrap();
    }
    std::fs::write(web.join("index.html"), "<html><head></head></html>").unwrap();
    std::fs::write(web.join("assets/app.js"), "start()").unwrap();
    let asked = d.path().join("asked.txt");
    let fake = |name: &str, answer: &str| {
        let file = bin.join(name);
        std::fs::write(
            &file,
            format!("#!/bin/sh\necho \"{name} $*\" >> \"$ASKED\"\n{answer}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    fake(
        "systemctl",
        r#"case "$2" in is-active) echo active;; is-enabled) echo enabled;; esac"#,
    );
    fake("loginctl", "echo yes");
    fake("journalctl", "echo 'Orbit is running without a window.'");
    fake("gh", "");
    let server = |args: &[&str]| {
        orbit_server(&home, args)
            .env("PATH", &bin)
            .env("ASKED", &asked)
            .output()
            .unwrap()
    };

    let installed = server(&[
        "install",
        "--data",
        data.to_str().unwrap(),
        "--web",
        web.to_str().unwrap(),
    ]);
    assert!(installed.status.success(), "{}", text(&installed.stderr));
    let said = text(&installed.stdout);
    assert!(
        said.starts_with("Orbit is set up as a service and running.\n"),
        "{said}"
    );
    assert!(said.contains("\nFound on its PATH: gh.\n"), "{said}");
    let programs = home.join(".local/share/orbit-server");
    let program = programs.join("orbit-server");
    assert_eq!(
        std::fs::read(&program).unwrap(),
        std::fs::read(PROGRAM).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(programs.join("web/assets/app.js")).unwrap(),
        "start()"
    );
    let unit = std::fs::read_to_string(home.join(".config/systemd/user/orbit.service")).unwrap();
    assert!(
        unit.contains(&format!(
            "ExecStart=\"{}\" \"--data\" \"{}\" \"--web\" \"{}\"\n",
            program.display(),
            data.display(),
            programs.join("web").display()
        )),
        "{unit}"
    );
    assert!(
        unit.contains(&format!("Environment=\"PATH={}\"\n", bin.display())),
        "{unit}"
    );
    // systemd reads the unit without a complaint, where its checker is
    // installed (as on CI).
    let checker = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|folder| folder.join("systemd-analyze"))
        .find(|file| file.is_file());
    if let Some(checker) = checker {
        let checked = Command::new(checker)
            .arg("verify")
            .arg(home.join(".config/systemd/user/orbit.service"))
            .output()
            .unwrap();
        assert!(checked.status.success(), "{}", text(&checked.stderr));
        assert_eq!(text(&checked.stderr), "");
    }
    let uid = unsafe { libc::getuid() };
    assert_eq!(
        std::fs::read_to_string(&asked).unwrap(),
        format!("systemctl --user daemon-reload\nsystemctl --user enable orbit.service\nsystemctl --user restart orbit.service\nloginctl show-user {uid} --property=Linger --value\n")
    );
    std::fs::remove_file(&asked).unwrap();

    // The copy serves the page it was installed with.
    let port = free_port();
    let mut copy = Command::new(&program)
        .args([
            "--data",
            data.to_str().unwrap(),
            "--port",
            &port.to_string(),
        ])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &bin)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let printed = lines(&mut copy);
    assert_eq!(
        wait_for_line(&printed, "Browser page: "),
        format!("Browser page: {}", programs.join("web").display())
    );
    let _ = copy.kill();
    let _ = copy.wait();

    // Installing again without --data, as after an update, keeps the
    // service on its folder rather than the desktop app's.
    let again = server(&["install", "--web", web.to_str().unwrap()]);
    assert!(again.status.success(), "{}", text(&again.stderr));
    assert!(
        text(&again.stdout).contains(&format!("\nData folder: {} (as before)\n", data.display())),
        "{}",
        text(&again.stdout)
    );
    let unit_again =
        std::fs::read_to_string(home.join(".config/systemd/user/orbit.service")).unwrap();
    assert_eq!(unit_again, unit);
    std::fs::remove_file(&asked).unwrap();

    let status = server(&["status"]);
    assert_eq!(
        text(&status.stdout),
        format!(
            "Orbit's service is running, and starts when you log in or the computer starts.\nService: {}\nSee its log with orbit-server logs.\n",
            home.join(".config/systemd/user/orbit.service").display()
        )
    );
    let logs = server(&["logs", "--follow"]);
    assert!(logs.status.success());
    assert_eq!(text(&logs.stdout), "Orbit is running without a window.\n");
    assert_eq!(
        std::fs::read_to_string(&asked).unwrap(),
        "systemctl --user is-active orbit.service\nsystemctl --user is-enabled orbit.service\njournalctl -f --user --unit orbit.service --lines 200 --no-pager\n"
    );
    std::fs::remove_file(&asked).unwrap();

    let removed = server(&["uninstall"]);
    assert!(removed.status.success(), "{}", text(&removed.stderr));
    assert!(!home.join(".config/systemd/user/orbit.service").exists());
    assert!(!programs.exists());
    assert_eq!(
        std::fs::read_to_string(&asked).unwrap(),
        "systemctl --user disable --now orbit.service\nsystemctl --user daemon-reload\n"
    );
    let status = server(&["status"]);
    assert_eq!(
        text(&status.stdout),
        "Orbit isn't set up as a service here. Set it up with orbit-server install.\n"
    );
}
