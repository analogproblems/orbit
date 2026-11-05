//! Orbit's sandbox: running a Claude or Antigravity repository's setup and
//! check commands on this computer, in Anthropic's sandbox runtime (`srt`,
//! the sandbox Claude Code runs its own commands in), so those reviewers
//! get test results without a dev container. Codex reviews run setup in
//! Codex's own sandbox (`command/exec`); Claude and Antigravity have
//! nothing like it, so Orbit runs the commands itself, before the review,
//! and hands the results to the reviewer as it does a dev container's (see
//! `crate::devcontainer::notes`). The reviewer's own flags don't change.
//!
//! The commands run in a copy of the pull request of their own, not the
//! reviewer's, so nothing they leave behind looks like the reviewer's
//! doing. They can write only to that copy, its own Git metadata and a
//! temporary folder beside it, where package managers' caches go too
//! (`CACHES`); can read the clone's Git metadata but not your sign-ins and
//! keys (`PRIVATE`) or the rest of Orbit's data; get only a short list of
//! environment variables (`KEEP`), so no token in Orbit's environment
//! reaches them; and reach no network, except that setup reaches the
//! package registries in `PACKAGE_HOSTS` when the repository's network
//! access is on. The settings are written outside the copy, so the pull
//! request can't change them. It runs on Linux and macOS; on Windows the runtime needs a
//! machine-wide install Orbit doesn't ask for.
use crate::{
    cli::Launcher,
    devcontainer::{self, CHECK_TIMEOUT, SETUP_TIMEOUT},
    types::*,
};
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    path::Path,
    time::{Duration, Instant},
};

/// What to install, said wherever it's missing.
pub const INSTALL: &str = "npm install -g @anthropic-ai/sandbox-runtime";

/// Places in your home folder the commands can't read: sign-ins, keys and
/// tokens, including the reviewers' own.
pub const PRIVATE: [&str; 24] = [
    "~/.ssh",
    "~/.gnupg",
    "~/.aws",
    "~/.azure",
    "~/.kube",
    "~/.docker",
    "~/.netrc",
    "~/.git-credentials",
    "~/.npmrc",
    "~/.yarnrc.yml",
    "~/.pypirc",
    "~/.gem/credentials",
    "~/.cargo/credentials",
    "~/.cargo/credentials.toml",
    "~/.config/gh",
    "~/.config/gcloud",
    "~/.config/hub",
    "~/.codex",
    "~/.claude",
    "~/.claude.json",
    "~/.gemini",
    "~/.local/share/keyrings",
    "~/.password-store",
    "~/Library/Keychains",
];

/// The package registries setup can reach while the repository's network
/// access is on: npm, Yarn, crates.io, PyPI, Go's module proxy, RubyGems,
/// and GitHub for dependencies fetched from it. Nothing else answers.
pub const PACKAGE_HOSTS: [&str; 16] = [
    "registry.npmjs.org",
    "registry.yarnpkg.com",
    "repo.yarnpkg.com",
    "crates.io",
    "index.crates.io",
    "static.crates.io",
    "pypi.org",
    "files.pythonhosted.org",
    "proxy.golang.org",
    "sum.golang.org",
    "rubygems.org",
    "index.rubygems.org",
    "github.com",
    "codeload.github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// The environment variables the commands get from Orbit's own, besides
/// the `LC_` ones: where programs and toolchains are, the locale and the
/// terminal, and certificates to trust. Nothing else passes, so a token in
/// Orbit's environment, such as `GH_TOKEN`, never reaches them.
pub const KEEP: [&str; 30] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LANGUAGE",
    "TZ",
    "TERM",
    "COLORTERM",
    "NO_COLOR",
    "CI",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "JAVA_HOME",
    "GOROOT",
    "NVM_DIR",
    "NVM_BIN",
    "VOLTA_HOME",
    "PNPM_HOME",
    "BUN_INSTALL",
    "PYENV_ROOT",
    "ASDF_DIR",
    "ASDF_DATA_DIR",
    "SDKMAN_DIR",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
];
/// Where package managers keep their caches and downloads, in the
/// temporary folder, as they can't write to their usual places in your home
/// folder: each variable, with its folder under the temporary one.
pub const CACHES: [(&str, &str); 15] = [
    ("XDG_CACHE_HOME", "cache"),
    ("npm_config_cache", "cache/npm"),
    ("npm_config_store_dir", "cache/pnpm"),
    ("YARN_CACHE_FOLDER", "cache/yarn"),
    ("BUN_INSTALL_CACHE_DIR", "cache/bun"),
    ("PIP_CACHE_DIR", "cache/pip"),
    ("UV_CACHE_DIR", "cache/uv"),
    ("POETRY_CACHE_DIR", "cache/poetry"),
    ("CARGO_HOME", "cargo"),
    ("GOPATH", "go"),
    ("GOMODCACHE", "go/pkg/mod"),
    ("GOCACHE", "cache/go-build"),
    ("BUNDLE_PATH", "bundle"),
    ("GEM_HOME", "gems"),
    ("GRADLE_USER_HOME", "gradle"),
];
/// The environment the commands get: from `vars`, Orbit's own, only those
/// in `KEEP` and the `LC_` ones, then the temporary folder `temp` as
/// `TMPDIR`, `TMP` and `TEMP`, and package managers' caches in it
/// (`CACHES`).
pub fn environment(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
    temp: &Path,
) -> Vec<(OsString, OsString)> {
    let mut out: Vec<(OsString, OsString)> = vars
        .into_iter()
        .filter(|(k, _)| {
            k.to_str()
                .is_some_and(|k| KEEP.contains(&k) || k.starts_with("LC_"))
        })
        .collect();
    for name in ["TMPDIR", "TMP", "TEMP"] {
        out.push((name.into(), temp.into()));
    }
    for (name, folder) in CACHES {
        out.push((name.into(), temp.join(folder).into()));
    }
    out
}

/// The clone's Git metadata for the copy, so Git works in it: the copy's
/// own, which its commands may change, and the clone's shared one, which
/// they may only read.
#[derive(Clone, Copy, Debug)]
pub struct Git<'a> {
    pub dir: &'a Path,
    pub common: &'a Path,
}

/// How to start the runtime; tests start a fake.
pub fn launcher() -> Launcher {
    Launcher::new("srt")
}

/// The runtime's settings for one step: write only to `writable`, read
/// anything but `PRIVATE` and `hidden`, apart from `writable` and
/// `readable`, and reach `PACKAGE_HOSTS` only when `network` is on.
pub fn settings(writable: &[&Path], readable: &[&Path], hidden: &Path, network: bool) -> Value {
    let strings = |paths: &[&Path]| -> Vec<String> {
        paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    };
    let writable = strings(writable);
    let mut read = writable.clone();
    read.extend(strings(readable));
    let mut deny: Vec<String> = PRIVATE.iter().map(|p| p.to_string()).collect();
    deny.push(hidden.to_string_lossy().into_owned());
    json!({
        "network": {
            "allowedDomains": if network { PACKAGE_HOSTS.to_vec() } else { vec![] },
            "deniedDomains": [],
        },
        "filesystem": {
            "denyRead": deny,
            // These can be read, even inside what's hidden.
            "allowRead": read,
            "allowWrite": writable,
            "denyWrite": [],
        },
    })
}

/// Checks the runtime is installed and can start its sandbox here, by
/// running a command that does nothing in `folder`, which it then removes.
pub async fn check(srt: &Launcher, folder: &Path) -> Result<String, String> {
    if cfg!(windows) {
        return Err("Orbit's sandbox runs on Linux and macOS, not yet on Windows. Use a dev container to run setup and checks for Claude and Antigravity reviews.".into());
    }
    let version = srt
        .output(
            &["--version"],
            "srt",
            &format!("Install Anthropic's sandbox runtime ({INSTALL}) to run setup and checks for Claude and Antigravity reviews:"),
        )
        .await?;
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let file = folder.join("settings.json");
    let tried = async {
        write(&file, &settings(&[folder], &[], folder, false))?;
        let out = step(
            srt,
            &file,
            folder,
            folder,
            "exit 0",
            Duration::from_secs(60),
        )
        .await?;
        if out.passed() {
            Ok(())
        } else {
            Err(format!(
                "The sandbox can't start on this computer: {}",
                last_line(&out.output)
            ))
        }
    }
    .await;
    let _ = std::fs::remove_dir_all(folder);
    tried?;
    Ok(format!("Sandbox runtime {}", version.trim()))
}

/// One review's setup and checks in the sandbox.
pub struct Plan<'a> {
    /// The review's own folder, outside every copy, holding the settings
    /// and the temporary folder.
    pub folder: &'a Path,
    /// The copy of the pull request the commands run in.
    pub copy: &'a Path,
    /// Orbit's data folder, which the commands can't read.
    pub data: &'a Path,
    /// The copy's Git metadata, when it is a Git worktree.
    pub git: Option<Git<'a>>,
    pub network: bool,
    pub setup: &'a str,
    pub check: &'a str,
}

/// Runs the setup command, with the package registries when network access
/// is on, and then, if it passed, the check command, with no network. A
/// failing command is a result; a sandbox that can't be set up is an error.
pub async fn run_checks(srt: &Launcher, plan: Plan<'_>) -> Result<ContainerChecks, String> {
    let temp = plan.folder.join("tmp");
    std::fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
    let mut writable = vec![plan.copy, temp.as_path()];
    let mut readable = vec![];
    if let Some(git) = plan.git {
        writable.push(git.dir);
        readable.push(git.common);
    }
    let mut checks = ContainerChecks {
        image: String::new(),
        network: plan.network,
        setup: None,
        check: None,
        sandbox: true,
    };
    if !plan.setup.trim().is_empty() {
        let file = plan.folder.join("setup.json");
        write(
            &file,
            &settings(&writable, &readable, plan.data, plan.network),
        )?;
        let ran = step(srt, &file, plan.copy, &temp, plan.setup, SETUP_TIMEOUT).await?;
        let passed = ran.passed();
        checks.setup = Some(ran);
        if !passed {
            return Ok(checks);
        }
    }
    if !plan.check.trim().is_empty() {
        let file = plan.folder.join("check.json");
        write(&file, &settings(&writable, &readable, plan.data, false))?;
        checks.check = Some(step(srt, &file, plan.copy, &temp, plan.check, CHECK_TIMEOUT).await?);
    }
    Ok(checks)
}

fn write(file: &Path, settings: &Value) -> Result<(), String> {
    std::fs::write(file, settings.to_string())
        .map_err(|e| format!("Couldn't write the sandbox's settings: {e}"))
}

/// Runs `command` with the shell in `cwd`, in the sandbox `file` sets up.
async fn step(
    srt: &Launcher,
    file: &Path,
    cwd: &Path,
    temp: &Path,
    command: &str,
    timeout: Duration,
) -> Result<CheckStep, String> {
    let file = file.to_str().ok_or("Invalid sandbox settings path")?;
    let started = Instant::now();
    let env = environment(std::env::vars_os(), temp);
    let out = devcontainer::run_in(
        srt,
        &["--settings", file, "--", "/bin/sh", "-c", command],
        timeout,
        Some(cwd),
        Some(&env),
    )
    .await?;
    Ok(CheckStep {
        command: command.to_string(),
        exit_code: if out.timed_out { None } else { out.status },
        timed_out: out.timed_out,
        seconds: started.elapsed().as_secs(),
        output: out.output,
    })
}

fn last_line(output: &str) -> &str {
    output
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("it gave no reason")
        .trim()
}

#[cfg(test)]
#[path = "sandbox_tests.rs"]
pub(crate) mod tests;
