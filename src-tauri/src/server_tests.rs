use super::*;

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}
fn parsed(list: &[&str]) -> Result<Command, String> {
    parse(
        args(list),
        Some("/home/me/.local/share/dev.orbit.review".into()),
    )
}
fn here(path: &str) -> PathBuf {
    std::env::current_dir().unwrap().join(path)
}
/// A path as Orbit takes it: on Windows, `/srv` is on the current drive.
fn abs(path: &str) -> PathBuf {
    std::path::absolute(path).unwrap()
}

#[test]
fn reads_the_options_for_running_orbit() {
    let default = abs("/home/me/.local/share/dev.orbit.review");
    assert_eq!(
        parsed(&[]),
        Ok(Command::Run {
            data: default.clone(),
            port: None,
            web: None
        })
    );
    assert_eq!(
        parsed(&[
            "--data",
            "/srv/orbit",
            "--port",
            "5000",
            "--web",
            "/opt/orbit/web"
        ]),
        Ok(Command::Run {
            data: abs("/srv/orbit"),
            port: Some(5000),
            web: Some(abs("/opt/orbit/web"))
        })
    );
    assert_eq!(
        parsed(&["--port=65535", "--data=/srv/a=b", "--web=dist"]),
        Ok(Command::Run {
            data: abs("/srv/a=b"),
            port: Some(65535),
            web: Some(here("dist"))
        })
    );
    // Relative folders are made absolute, since reviewers run elsewhere.
    assert_eq!(
        parsed(&["--data", "orbit-data"]),
        Ok(Command::Run {
            data: here("orbit-data"),
            port: None,
            web: None
        })
    );
    assert_eq!(parsed(&["--help"]), Ok(Command::Help));
    assert_eq!(parsed(&["--port", "5000", "-h"]), Ok(Command::Help));
    assert_eq!(parsed(&["-V"]), Ok(Command::Version));
    assert_eq!(parsed(&["link", "--version"]), Ok(Command::Version));
}

#[test]
fn reads_the_options_for_the_link() {
    let default = abs("/home/me/.local/share/dev.orbit.review");
    assert_eq!(
        parsed(&["link"]),
        Ok(Command::Link {
            data: default.clone(),
            new: false
        })
    );
    assert_eq!(
        parsed(&["link", "--new", "--data", "/srv/orbit"]),
        Ok(Command::Link {
            data: abs("/srv/orbit"),
            new: true
        })
    );
}

#[test]
fn reads_the_service_commands() {
    // Without --data, install keeps the folder the service had, which only
    // the service knows.
    assert_eq!(
        parsed(&["install", "--port", "5000", "--web=dist"]),
        Ok(Command::Install {
            data: None,
            port: Some(5000),
            web: Some(here("dist"))
        })
    );
    assert_eq!(
        parse(args(&["install", "--data", "orbit"]), None),
        Ok(Command::Install {
            data: Some(here("orbit")),
            port: None,
            web: None
        })
    );
    // These don't use a data folder, so they work without one.
    for (list, command) in [
        (&["uninstall"][..], Command::Uninstall),
        (&["status"], Command::Status),
        (&["logs"], Command::Logs { follow: false }),
        (&["logs", "--follow"], Command::Logs { follow: true }),
    ] {
        assert_eq!(parse(args(list), None), Ok(command), "{list:?}");
    }
}

#[test]
fn refuses_options_it_does_not_know_or_that_do_not_fit() {
    for (list, error) in [
        (&["--port", "80"][..], "Choose a port from 1024 to 65535."),
        (&["--port", "1023"], "Choose a port from 1024 to 65535."),
        (&["--port", "65536"], "Choose a port from 1024 to 65535."),
        (&["--port", "many"], "Choose a port from 1024 to 65535."),
        (&["--port"], "--port needs a port."),
        (&["--port="], "--port needs a port."),
        (&["--data"], "--data needs a folder."),
        (&["--data", ""], "--data needs a folder."),
        (&["--web"], "--web needs a folder."),
        (
            &["--port", "5000", "--port", "5001"],
            "--port is given more than once.",
        ),
        (
            &["--data", "/a", "--data=/b"],
            "--data is given more than once.",
        ),
        (
            &["--web", "/a", "--web", "/b"],
            "--web is given more than once.",
        ),
        (&["--new"], "Only orbit-server link takes --new."),
        (
            &["link", "--port", "5000"],
            "orbit-server link doesn't take --port.",
        ),
        (
            &["link", "--web", "/a"],
            "orbit-server link doesn't take --web.",
        ),
        (
            &["link", "--new", "--new"],
            "--new is given more than once.",
        ),
        (&["link", "--new=yes"], "--new doesn't take a value."),
        (&["serve"], "Orbit doesn't know serve."),
        (&["--data", "/a", "link"], "Orbit doesn't know link."),
        (&["--verbose"], "Orbit doesn't know --verbose."),
        (&["install", "--new"], "Only orbit-server link takes --new."),
        (&["--follow"], "Only orbit-server logs takes --follow."),
        (
            &["link", "--follow"],
            "Only orbit-server logs takes --follow.",
        ),
        (&["logs", "--follow=yes"], "--follow doesn't take a value."),
        (
            &["logs", "--follow", "--follow"],
            "--follow is given more than once.",
        ),
        (
            &["logs", "--data", "/a"],
            "orbit-server logs doesn't take --data.",
        ),
        (
            &["status", "--port", "5000"],
            "orbit-server status doesn't take --port.",
        ),
        // The service has one data folder, so these don't ask for it.
        (
            &["status", "--data", "/a"],
            "orbit-server status doesn't take --data.",
        ),
        (
            &["uninstall", "--data", "/a"],
            "orbit-server uninstall doesn't take --data.",
        ),
        (
            &["uninstall", "--web", "/a"],
            "orbit-server uninstall doesn't take --web.",
        ),
        (&["uninstall", "now"], "Orbit doesn't know now."),
        (
            &["install", "--port", "80"],
            "Choose a port from 1024 to 65535.",
        ),
        (
            &["install", "--data", "/a", "--data", "/b"],
            "--data is given more than once.",
        ),
    ] {
        assert_eq!(parsed(list), Err(error.into()), "{list:?}");
    }
    assert_eq!(
        parse(args(&[]), None),
        Err("Orbit can't find the desktop app's data folder. Choose one with --data.".into())
    );
    // A folder named after the option is still taken as a folder.
    assert_eq!(
        parse(args(&["--data", "--new"]), None),
        Ok(Command::Run {
            data: here("--new"),
            port: None,
            web: None
        })
    );
}

#[cfg(unix)]
#[test]
fn takes_any_folder_name_the_system_allows() {
    use std::os::unix::ffi::OsStringExt;
    let odd = OsString::from_vec(b"/srv/caf\xe9".to_vec());
    assert_eq!(
        parse(vec!["--data".into(), odd.clone()], None),
        Ok(Command::Run {
            data: odd.into(),
            port: None,
            web: None
        })
    );
    let option = OsString::from_vec(b"--d\xe9".to_vec());
    assert_eq!(
        parse(vec![option], None),
        Err("Orbit doesn't know --d\u{fffd}.".into())
    );
}

#[test]
fn uses_the_desktop_apps_data_folder() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    assert_eq!(config["identifier"], IDENTIFIER);
    let home = Some(PathBuf::from("/home/me"));
    let found = data_folder(
        home.clone(),
        Some("/data".into()),
        Some("C:\\Users\\me\\AppData\\Roaming".into()),
    );
    let relative = data_folder(home.clone(), Some("data".into()), None);
    if cfg!(windows) {
        let roaming = PathBuf::from("C:\\Users\\me\\AppData\\Roaming");
        assert_eq!(found, Some(roaming.join(IDENTIFIER)));
        assert_eq!(data_folder(home, None, None), None);
    } else if cfg!(target_os = "macos") {
        let support = PathBuf::from("/home/me/Library/Application Support");
        assert_eq!(found, Some(support.join(IDENTIFIER)));
        assert_eq!(relative, Some(support.join(IDENTIFIER)));
    } else {
        assert_eq!(found, Some(PathBuf::from("/data").join(IDENTIFIER)));
        // A relative XDG_DATA_HOME is ignored, as Tauri does.
        let share = PathBuf::from("/home/me/.local/share");
        assert_eq!(relative, Some(share.join(IDENTIFIER)));
        assert_eq!(data_folder(home, None, None), Some(share.join(IDENTIFIER)));
        assert_eq!(data_folder(None, None, None), None);
    }
}

#[test]
fn serves_only_files_inside_the_page_folder() {
    let d = tempfile::tempdir().unwrap();
    let web = d.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("index.html"), "<html><head></head></html>").unwrap();
    std::fs::write(web.join("assets/app.js"), "run()").unwrap();
    std::fs::write(d.path().join("secret.txt"), "keep out").unwrap();
    // Names Unix allows, so the checks that matter on Windows show here too.
    #[cfg(unix)]
    for odd in ["c:secret.txt", "a\\b.txt"] {
        std::fs::write(web.join(odd), "odd").unwrap();
    }
    let assets = folder_assets(web.clone());
    let page = assets("index.html").unwrap();
    assert_eq!(page.bytes, b"<html><head></head></html>");
    assert_eq!(page.mime_type, "text/html");
    let script = assets("assets/app.js").unwrap();
    assert_eq!(
        (script.bytes, script.mime_type),
        (b"run()".to_vec(), "text/javascript".into())
    );
    for path in [
        "",
        "assets",
        "assets/",
        "missing.js",
        "../secret.txt",
        "assets/../../secret.txt",
        "./index.html",
        "assets//app.js",
        "assets\\app.js",
        "a\\b.txt",
        "C:/secret.txt",
        "c:secret.txt",
        "index.html\0",
    ] {
        assert!(assets(path).is_none(), "{path:?}");
    }
    let outside = d.path().join("secret.txt").display().to_string();
    assert!(assets(&outside).is_none());
    assert!(assets(outside.trim_start_matches('/')).is_none());
}

#[test]
fn names_each_built_files_type() {
    for (path, mime) in [
        ("index.html", "text/html"),
        ("a/INDEX.HTM", "text/html"),
        ("assets/index-D0vAc1yA.js", "text/javascript"),
        ("worker.mjs", "text/javascript"),
        ("assets/index-Tjr8_2mL.css", "text/css"),
        ("manifest.json", "application/json"),
        ("sw.js", "text/javascript"),
        ("manifest.webmanifest", "application/manifest+json"),
        ("assets/app.js.map", "application/json"),
        ("orbit.svg", "image/svg+xml"),
        ("icon.png", "image/png"),
        ("photo.jpg", "image/jpeg"),
        ("photo.jpeg", "image/jpeg"),
        ("anim.gif", "image/gif"),
        ("pic.webp", "image/webp"),
        ("favicon.ico", "image/vnd.microsoft.icon"),
        ("font.woff2", "font/woff2"),
        ("font.woff", "font/woff"),
        ("font.ttf", "font/ttf"),
        ("font.otf", "font/otf"),
        ("code.wasm", "application/wasm"),
        ("robots.txt", "text/plain"),
        ("LICENSE", "application/octet-stream"),
        ("archive.tar.gz", "application/octet-stream"),
    ] {
        assert_eq!(mime_type(path), mime, "{path}");
    }
}

#[test]
fn finds_the_page_it_is_given_or_says_how_to_build_it() {
    let d = tempfile::tempdir().unwrap();
    let error = web_folder(Some(d.path().into())).unwrap_err();
    assert_eq!(
        error,
        format!(
            "There's no index.html in {}. Choose the folder that npm run build makes (dist) with --web.",
            d.path().display()
        )
    );
    std::fs::write(d.path().join("index.html"), "<html></html>").unwrap();
    assert_eq!(web_folder(Some(d.path().into())), Ok(d.path().into()));
    // A folder called index.html isn't a page.
    let odd = tempfile::tempdir().unwrap();
    std::fs::create_dir(odd.path().join("index.html")).unwrap();
    assert!(web_folder(Some(odd.path().into())).is_err());

    // Without one, the first folder with a page wins.
    let page = d.path().to_path_buf();
    let empty_folder = tempfile::tempdir().unwrap();
    let empty = empty_folder.path().to_path_buf();
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("index.html"), "<html></html>").unwrap();
    let other = other.path().to_path_buf();
    assert_eq!(
        find_page(None, &[empty.clone(), page.clone()]),
        Ok(page.clone())
    );
    assert_eq!(find_page(None, &[other.clone(), page.clone()]), Ok(other));
    // A folder given is the only one looked at.
    assert!(find_page(Some(empty.clone()), std::slice::from_ref(&page)).is_err());
    assert_eq!(
        find_page(None, &[empty, odd.path().into()]),
        Err("Orbit can't find the browser page's files. Build them with npm run build, then choose the dist folder with --web.".into())
    );
    // `web` beside the program, then the dist of the checkout it was built from.
    let exe = std::env::current_exe().unwrap();
    assert_eq!(
        page_folders(),
        [
            exe.parent().unwrap().join("web"),
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist")
        ]
    );
}

#[tokio::test]
async fn gives_the_link_and_makes_a_new_one_only_while_orbit_is_stopped() {
    let d = tempfile::tempdir().unwrap();
    let missing = d.path().join("missing");
    assert_eq!(
        link(&missing, false),
        Err(format!(
            "There's no Orbit data folder at {}. Start orbit-server once, or choose the folder with --data.",
            missing.display()
        ))
    );
    assert!(!missing.exists());
    let first = link(d.path(), false).unwrap();
    assert_eq!(remote::saved_token(d.path()), Some(first.clone()));
    assert_eq!(link(d.path(), false).unwrap(), first);
    let second = link(d.path(), true).unwrap();
    assert_ne!(second, first);
    assert_eq!(remote::saved_token(d.path()), Some(second.clone()));
    {
        let engine = Engine::new(d.path().into(), |_| {}).unwrap();
        assert_eq!(link(d.path(), false).unwrap(), second);
        assert_eq!(
            link(d.path(), true),
            Err("Orbit is running on this data folder, so its link can't change now. Stop it, run orbit-server link --new again, then start it.".into())
        );
        std::fs::remove_file(d.path().join("remote-token")).unwrap();
        assert_eq!(
            link(d.path(), false),
            Err("Orbit is running but its link is missing. Stop it, run orbit-server link --new, then start it.".into())
        );
        assert!(!d.path().join("remote-token").exists());
        drop(engine);
    }
    // A link run releases the folder for Orbit to start.
    let third = link(d.path(), false).unwrap();
    assert_ne!(third, second);
    let engine = Engine::new(d.path().into(), |_| {});
    assert!(engine.is_ok());
}

#[tokio::test]
async fn says_how_to_open_the_link_without_tailscale() {
    // Tests never find a Tailscale CLI here; the tailnet text is Settings'.
    if remote::tailnet_host().await.is_some() {
        return;
    }
    assert_eq!(
        instructions(4317, "abc").await,
        "Open this link on your other device:
   http://127.0.0.1:4317/#token=abc
Tailscale wasn't found on this computer, so this is the local address. On
another device, replace 127.0.0.1:4317 with this computer's Tailscale name."
    );
}

#[test]
fn help_names_every_option() {
    let text = help();
    for option in [
        "--data",
        "--port",
        "--web",
        "--new",
        "--help",
        "--version",
        "--follow",
        "link",
        "install",
        "status",
        "logs",
        "uninstall",
    ] {
        assert!(text.contains(option), "{option}");
    }
    assert!(text.contains(&remote::DEFAULT_PORT.to_string()));
}

/// `tauri dev` runs `cargo run` without naming a program, and Tauri bundles
/// every program the build makes, so the desktop app has to stay the default
/// and `orbit-server` has to stay out of its build.
#[test]
fn the_desktop_app_stays_the_program_cargo_starts_and_bundles() {
    let out = std::process::Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--offline",
            "--format-version",
            "1",
        ])
        .arg("--manifest-path")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let package = &metadata["packages"][0];
    // Tauri takes the program named like the package as the app.
    assert_eq!(package["name"], "orbit");
    assert_eq!(package["default_run"], "orbit");
    let needs = |kind: &str, name: &str| {
        package["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name && t["kind"][0] == kind)
            .unwrap_or_else(|| panic!("no {kind} {name}"))["required-features"]
            .clone()
    };
    assert_eq!(needs("bin", "orbit"), serde_json::json!(["desktop"]));
    assert_eq!(needs("bin", "orbit-server"), serde_json::json!(["server"]));
    assert_eq!(needs("test", "server"), serde_json::json!(["server"]));
    assert_eq!(
        package["features"]["default"],
        serde_json::json!(["desktop"])
    );
}
