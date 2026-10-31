//! `orbit-server`: Orbit with no window, for a computer that stays on. It runs
//! the same engine on the same data folder as the desktop app and serves the
//! browser page (see `remote`) from the built frontend's files, so the page
//! is the only way in. Its controls are on the command line, which a browser
//! can't reach, as the desktop-only controls are.
use crate::{engine::Engine, remote};
use std::{
    ffi::OsString,
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::watch;

/// The desktop app's identifier in `tauri.conf.json`, which names its data
/// folder.
pub const IDENTIFIER: &str = "dev.orbit.review";
/// How long a stop waits for browser requests already running.
const REQUESTS_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Run {
        data: PathBuf,
        port: Option<u16>,
        web: Option<PathBuf>,
    },
    Link {
        data: PathBuf,
        new: bool,
    },
    Help,
    Version,
}

/// Runs `orbit-server` with its arguments (without the program's name) and
/// returns the exit code: 0 when it worked, 1 when it couldn't, and 2 for
/// arguments it doesn't understand.
pub fn main(args: Vec<OsString>) -> i32 {
    let command = match parse(args, desktop_data_folder()) {
        Ok(command) => command,
        Err(error) => {
            complain(&format!(
                "{error}\nRun orbit-server --help to see what it takes."
            ));
            return 2;
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            complain(&format!("Orbit can't start: {e}"));
            return 1;
        }
    };
    let result = match command {
        Command::Help => {
            say(help().trim_end());
            Ok(())
        }
        Command::Version => {
            say(&format!("orbit-server {}", env!("CARGO_PKG_VERSION")));
            Ok(())
        }
        Command::Link { data, new } => link(&data, new).map(|token| {
            let port = remote::config(&data).port;
            say(&runtime.block_on(instructions(port, &token)));
        }),
        Command::Run { data, port, web } => runtime
            .block_on(run(data, port, web))
            .map_err(|e| format!("Orbit can't start: {e}")),
    };
    // Orbit has saved and stopped what it runs by now, as the desktop app has
    // when it exits, so nothing is left worth waiting for.
    runtime.shutdown_background();
    match result {
        Ok(()) => 0,
        Err(error) => {
            complain(&error);
            1
        }
    }
}

fn help() -> String {
    let data = desktop_data_folder()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "none found, so --data is needed".into());
    format!(
        "Orbit without a window: the same reviews and the same browser page, for a
computer that stays on.

Usage:
  orbit-server [--data <folder>] [--port <port>] [--web <folder>]
  orbit-server link [--data <folder>] [--new]

  orbit-server        Runs Orbit. Ctrl+C or a stop signal saves and stops it.
  orbit-server link   Shows the link for your other devices. With --new, it
                      makes a new link, so the old one stops working. Stop
                      Orbit before making a new link.

Options:
  --data <folder>  Orbit's data folder. The desktop app's by default:
                   {data}
  --port <port>    The port to listen on, from 1024 to 65535, on this computer
                   only. It's kept for this data folder. {default} by default.
  --web <folder>   The browser page's files, the dist folder that
                   npm run build makes.
  -h, --help       Shows this help.
  -V, --version    Shows the version.
",
        default = remote::DEFAULT_PORT,
    )
}

/// Reads the arguments. `default_data` is the desktop app's data folder.
fn parse(args: Vec<OsString>, default_data: Option<PathBuf>) -> Result<Command, String> {
    let mut args = args.into_iter().peekable();
    let linking = args.peek().is_some_and(|a| a == "link");
    if linking {
        args.next();
    }
    let (mut data, mut port, mut web, mut new) = (None, None, None, false);
    while let Some(arg) = args.next() {
        // Options are plain text; only their values can be other paths.
        let Some(text) = arg.to_str() else {
            return Err(format!("Orbit doesn't know {}.", arg.to_string_lossy()));
        };
        let (name, inline) = match text.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value)),
            _ => (text, None),
        };
        let mut value = |what: &str| -> Result<OsString, String> {
            let value = match inline {
                Some(value) => OsString::from(value),
                None => args.next().ok_or(format!("{name} needs {what}."))?,
            };
            if value.is_empty() {
                return Err(format!("{name} needs {what}."));
            }
            Ok(value)
        };
        match name {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--data" => once(&mut data, PathBuf::from(value("a folder")?), name)?,
            "--port" if !linking => {
                let given = value("a port")?;
                let number = given
                    .to_str()
                    .and_then(|t| t.parse::<u16>().ok())
                    .filter(|p| *p >= 1024)
                    .ok_or("Choose a port from 1024 to 65535.")?;
                once(&mut port, number, name)?;
            }
            "--web" if !linking => once(&mut web, PathBuf::from(value("a folder")?), name)?,
            "--new" if linking && inline.is_some() => {
                return Err("--new doesn't take a value.".into())
            }
            "--new" if linking => {
                if new {
                    return Err("--new is given more than once.".into());
                }
                new = true;
            }
            "--port" | "--web" => return Err(format!("orbit-server link doesn't take {name}.")),
            "--new" => return Err("Only orbit-server link takes --new.".into()),
            _ => return Err(format!("Orbit doesn't know {text}.")),
        }
    }
    let data = data
        .or(default_data)
        .ok_or("Orbit can't find the desktop app's data folder. Choose one with --data.")?;
    // Reviewers and Git run in other folders, so a relative path would point
    // somewhere else for them.
    let data = absolute(data)?;
    Ok(if linking {
        Command::Link { data, new }
    } else {
        let web = web.map(absolute).transpose()?;
        Command::Run { data, port, web }
    })
}
fn once<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("{name} is given more than once."));
    }
    *slot = Some(value);
    Ok(())
}
fn absolute(path: PathBuf) -> Result<PathBuf, String> {
    std::path::absolute(&path).map_err(|e| format!("Can't use {}: {e}", path.display()))
}

/// The data folder the desktop app uses: Tauri's app data folder, named by
/// [`IDENTIFIER`] in the user's data folder.
fn desktop_data_folder() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    data_folder(var("HOME"), var("XDG_DATA_HOME"), var("APPDATA"))
}
fn data_folder(
    home: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
    appdata: Option<PathBuf>,
) -> Option<PathBuf> {
    let base = if cfg!(windows) {
        appdata
    } else if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Application Support"))
    } else {
        // As the XDG rules say, a relative XDG_DATA_HOME is ignored.
        xdg_data_home
            .filter(|p| p.is_absolute())
            .or_else(|| home.map(|h| h.join(".local/share")))
    };
    Some(base?.join(IDENTIFIER))
}

/// The folder with the browser page's files: the one given, else `web`
/// beside this program, else the `dist` folder of the checkout it was built
/// from.
fn web_folder(given: Option<PathBuf>) -> Result<PathBuf, String> {
    find_page(given, &page_folders())
}
/// Where the browser page is looked for when no folder is given.
fn page_folders() -> Vec<PathBuf> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("web")));
    let built = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist");
    beside.into_iter().chain([built]).collect()
}
fn find_page(given: Option<PathBuf>, defaults: &[PathBuf]) -> Result<PathBuf, String> {
    let has_page = |folder: &Path| folder.join("index.html").is_file();
    if let Some(folder) = given {
        return if has_page(&folder) {
            Ok(folder)
        } else {
            Err(format!(
                "There's no index.html in {}. Choose the folder that npm run build makes (dist) with --web.",
                folder.display()
            ))
        };
    }
    defaults
        .iter()
        .find(|folder| has_page(folder))
        .cloned()
        .ok_or_else(|| {
            "Orbit can't find the browser page's files. Build them with npm run build, then choose the dist folder with --web.".into()
        })
}

/// Serves the files in `folder`. The router has already refused escapes,
/// parent segments and backslashes; they, drive letters (`C:`) and empty or
/// `.` segments are refused here too, so a path can only name a file inside
/// the folder, whoever builds the router.
fn folder_assets(folder: PathBuf) -> remote::Assets {
    Arc::new(move |path: &str| {
        let file = inside(&folder, path)?;
        let bytes = std::fs::read(file).ok()?;
        Some(remote::Asset {
            bytes,
            mime_type: mime_type(path).into(),
        })
    })
}
fn inside(folder: &Path, path: &str) -> Option<PathBuf> {
    let mut file = folder.to_path_buf();
    for segment in path.split('/') {
        let plain = !segment.is_empty()
            && segment != "."
            && segment != ".."
            && !segment.contains([':', '\\']);
        if !plain {
            return None;
        }
        file.push(segment);
    }
    Some(file)
}
/// The media type of a built file, by its extension, as Tauri gives it.
fn mime_type(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match extension.as_deref() {
        Some("html" | "htm") => "text/html",
        Some("js" | "mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/vnd.microsoft.icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("wasm") => "application/wasm",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
}

/// The access token for `orbit-server link`. A new one is only made while
/// Orbit isn't running there: a running Orbit keeps the token it started
/// with, so a new one would only lock its browsers out.
fn link(data: &Path, new: bool) -> Result<String, String> {
    if !data.is_dir() {
        return Err(format!(
            "There's no Orbit data folder at {}. Start orbit-server once, or choose the folder with --data.",
            data.display()
        ));
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data.join("orbit.lock"))
        .map_err(|e| format!("Can't open {}: {e}", data.display()))?;
    let running = lock.try_lock().is_err();
    if running && new {
        return Err("Orbit is running on this data folder, so its link can't change now. Stop it, run orbit-server link --new again, then start it.".into());
    }
    if running {
        // Never make a token here: the running Orbit wouldn't know it.
        return remote::saved_token(data).ok_or_else(|| {
            "Orbit is running but its link is missing. Stop it, run orbit-server link --new, then start it.".into()
        });
    }
    // The lock is held until `lock` is dropped on return, so Orbit can't
    // start part way through.
    let token = if new {
        remote::new_token(data)
    } else {
        remote::token(data)
    };
    token.map_err(|e| format!("Orbit can't keep its link: {e}"))
}

/// The steps to open Orbit on another device, as Settings shows them on the
/// desktop.
async fn instructions(port: u16, token: &str) -> String {
    match remote::tailnet_host().await {
        Some(host) => format!(
            "1. Publish Orbit on your tailnet, once, from this computer:
   tailscale serve --bg {port}
2. Open this link on your other device:
   https://{host}/#token={token}"
        ),
        None => format!(
            "Open this link on your other device:
   {}
Tailscale wasn't found on this computer, so this is the local address. On
another device, replace 127.0.0.1:{port} with this computer's Tailscale name.",
            remote::local_link(port, token)
        ),
    }
}

async fn run(data: PathBuf, port: Option<u16>, web: Option<PathBuf>) -> Result<(), String> {
    // Watched from the start, so a stop that comes while Orbit starts still
    // saves and stops it once it has started.
    let mut stop_signals =
        StopSignals::new().map_err(|e| format!("Can't watch for Ctrl+C: {e}"))?;
    let web = web_folder(web)?;
    let hub = remote::Hub::default();
    let browsers = hub.clone();
    // Takes the data folder's lock first, so nothing below touches a folder
    // the desktop app or another orbit-server is using.
    let engine = Engine::new(data.clone(), move |snapshot| browsers.publish(snapshot))?;
    let mut config = remote::config(&data);
    if let Some(port) = port {
        config.port = port;
        remote::save_config(&data, &config)
            .map_err(|e| format!("Can't keep the port in {}: {e}", data.display()))?;
    }
    let token = remote::token(&data).map_err(|e| format!("Can't keep the access link: {e}"))?;
    let port = config.port;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| format!("Port {port} is not available ({e}). Choose another with --port."))?;
    let (stop, stopped) = watch::channel(false);
    let app = remote::router(
        engine.clone(),
        hub,
        token.clone(),
        folder_assets(web.clone()),
        stopped.clone(),
        remote::SERVER_MARK,
    );
    let mut shutdown = stopped;
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown.wait_for(|stop| *stop).await;
            })
            .await
    });
    engine.launch();
    say(&format!(
        "Orbit is running without a window.
Data folder: {}
Browser page: {}
Listening on http://127.0.0.1:{port}, on this computer only.",
        data.display(),
        web.display()
    ));
    // A service's log is kept, so the link, which works like a key, is only
    // shown in a terminal.
    if std::io::stdout().is_terminal() {
        say(&instructions(port, &token).await);
        say("Press Ctrl+C to stop.");
    } else {
        let folder = if Some(&data) == desktop_data_folder().as_ref() {
            String::new()
        } else {
            format!(" --data \"{}\"", data.display())
        };
        say(&format!(
            "To see the link for your other devices, run: orbit-server link{folder}"
        ));
    }
    stop_signals.wait().await;
    say("Stopping Orbit…");
    // Browsers first, so nothing new starts while the engine stops.
    let _ = stop.send(true);
    let _ = tokio::time::timeout(REQUESTS_GRACE, server).await;
    engine.shutdown().await;
    say("Orbit stopped.");
    Ok(())
}

/// Ctrl+C, and on Unix a stop signal (SIGTERM), from when it's made.
#[cfg(unix)]
struct StopSignals(tokio::signal::unix::Signal, tokio::signal::unix::Signal);
#[cfg(unix)]
impl StopSignals {
    fn new() -> std::io::Result<Self> {
        use tokio::signal::unix::{signal, SignalKind};
        Ok(Self(
            signal(SignalKind::interrupt())?,
            signal(SignalKind::terminate())?,
        ))
    }
    async fn wait(&mut self) {
        tokio::select! {
            _ = self.0.recv() => {}
            _ = self.1.recv() => {}
        }
    }
}
#[cfg(windows)]
struct StopSignals(tokio::signal::windows::CtrlC);
#[cfg(windows)]
impl StopSignals {
    fn new() -> std::io::Result<Self> {
        Ok(Self(tokio::signal::windows::ctrl_c()?))
    }
    async fn wait(&mut self) {
        self.0.recv().await;
    }
}

/// Writes to the console. A closed console, such as a terminal that was shut,
/// must not stop Orbit, so failures are ignored.
fn say(text: &str) {
    let _ = writeln!(std::io::stdout(), "{text}");
}
fn complain(text: &str) {
    let _ = writeln!(std::io::stderr(), "{text}");
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
