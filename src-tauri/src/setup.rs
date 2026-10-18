//! Setting up repositories from Bit's answers: the repository settings an
//! answer may change, the change itself, what Orbit tells Codex about each
//! repository's copy, and where Orbit keeps the copies it downloads itself.
//! Every change waits for your tap, and Orbit checks it again then.
use crate::types::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The repository settings an answer may change, by the names the summary
/// gives them.
pub const REPO_SETTINGS: [&str; 9] = [
    "setupCommand",
    "checkCommand",
    "networkAccess",
    "devContainer",
    "reviewer",
    "roundLimit",
    "approvalMinutes",
    "keepCloneUpdated",
    "autopilot",
];
/// The round limits Repositories offers.
pub const ROUND_LIMITS: [u32; 8] = [1, 2, 3, 4, 5, 6, 8, 10];
/// The approval time limits Repositories offers, in minutes; 0 waits.
pub const APPROVAL_MINUTES: [u32; 6] = [0, 5, 15, 30, 60, 120];
/// The longest command an answer may set, in characters.
pub const MAX_COMMAND: usize = 500;

/// Files at the top of a repository's copy that tell how to set it up.
pub const TELLTALE_FILES: [&str; 30] = [
    "package.json",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "deno.json",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "go.mod",
    "requirements.txt",
    "pyproject.toml",
    "uv.lock",
    "poetry.lock",
    "Pipfile.lock",
    "Gemfile.lock",
    "composer.lock",
    "mix.lock",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Makefile",
    "justfile",
    ".nvmrc",
    ".node-version",
    ".tool-versions",
    ".devcontainer.json",
    ".devcontainer/devcontainer.json",
];
/// Scripts the summary names from a copy's `package.json`.
const MAX_SCRIPTS: usize = 20;
/// The setup command Orbit suggests for a copy, by the first lock file it
/// has. Each installs exactly what the lock file lists and needs network
/// access while it runs; the JavaScript ones that can skip install scripts,
/// which run code from the pull request, do.
pub const SETUP_COMMANDS: [(&str, &str); 12] = [
    ("package-lock.json", "npm ci --ignore-scripts"),
    ("npm-shrinkwrap.json", "npm ci --ignore-scripts"),
    (
        "pnpm-lock.yaml",
        "pnpm install --frozen-lockfile --ignore-scripts",
    ),
    ("yarn.lock", "yarn install --frozen-lockfile"),
    ("bun.lock", "bun install --frozen-lockfile"),
    ("bun.lockb", "bun install --frozen-lockfile"),
    ("Cargo.lock", "cargo fetch"),
    ("uv.lock", "uv sync --frozen"),
    ("poetry.lock", "poetry install"),
    ("requirements.txt", "pip install -r requirements.txt"),
    ("go.mod", "go mod download"),
    ("Gemfile.lock", "bundle install"),
];
/// The setup command Orbit suggests for a copy with these telltale files,
/// and the file it goes by.
pub fn suggest(files: &[String]) -> Option<(&'static str, &'static str)> {
    SETUP_COMMANDS
        .into_iter()
        .find(|(file, _)| files.iter().any(|f| f == file))
}

/// A change to a repository's settings from an answer. Only what is given
/// changes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_access: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_container: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<Provider>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_clone_updated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autopilot: Option<bool>,
}
impl RepositoryPatch {
    pub fn apply(&self, r: &mut Repository) {
        if let Some(v) = &self.setup_command {
            r.setup_command = v.clone();
        }
        if let Some(v) = &self.check_command {
            r.dev_container.check_command = v.clone();
        }
        if let Some(v) = self.network_access {
            r.network_access = v;
        }
        if let Some(v) = self.dev_container {
            r.dev_container.enabled = v;
        }
        if let Some(v) = self.reviewer {
            r.provider = v;
        }
        if let Some(v) = self.round_limit {
            r.round_limit = v;
        }
        if let Some(v) = self.approval_minutes {
            r.approval_minutes = v;
        }
        if let Some(v) = self.keep_clone_updated {
            r.keep_updated = v;
        }
        if let Some(v) = self.autopilot {
            r.automatic = v;
        }
    }
}

/// A change from an answer's settings and values, if each names a setting
/// in `REPO_SETTINGS` with a value Repositories could save: a command on one
/// line of at most `MAX_COMMAND` characters with nothing that looks like a
/// credential, `true` or `false`, a reviewer, or one of the offered limits.
pub fn patch<'a>(changes: impl IntoIterator<Item = (&'a str, &'a str)>) -> Option<RepositoryPatch> {
    let mut patch = RepositoryPatch::default();
    for (setting, value) in changes {
        let value = value.trim();
        let flag = || match value.to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        let number = |allowed: &[u32]| value.parse().ok().filter(|n| allowed.contains(n));
        match setting {
            "setupCommand" => patch.setup_command = Some(command(value)?),
            "checkCommand" => patch.check_command = Some(command(value)?),
            "networkAccess" => patch.network_access = Some(flag()?),
            "devContainer" => patch.dev_container = Some(flag()?),
            "reviewer" => {
                patch.reviewer =
                    Some(serde_json::from_value(value.to_ascii_lowercase().into()).ok()?)
            }
            "roundLimit" => patch.round_limit = Some(number(&ROUND_LIMITS)?),
            "approvalMinutes" => patch.approval_minutes = Some(number(&APPROVAL_MINUTES)?),
            "keepCloneUpdated" => patch.keep_clone_updated = Some(flag()?),
            "autopilot" => patch.autopilot = Some(flag()?),
            _ => return None,
        }
    }
    Some(patch)
}
/// Package managers whose own scripts an answer may run, as `<pm> run <script>`.
const SCRIPT_RUNNERS: [&str; 4] = ["npm", "pnpm", "yarn", "bun"];
/// The commands an answer may use besides `SCRIPT_RUNNERS` scripts: each
/// program with the words that may follow it.
const KNOWN_COMMANDS: [(&str, &[&str]); 18] = [
    ("npm", &["ci", "install", "test"]),
    ("pnpm", &["install", "test"]),
    ("yarn", &["install", "test"]),
    ("bun", &["install", "test"]),
    ("cargo", &["fetch", "build", "test", "check", "clippy"]),
    (
        "go",
        &["mod download", "build ./...", "test ./...", "vet ./..."],
    ),
    ("uv", &["sync"]),
    ("poetry", &["install"]),
    ("pip", &["install -r requirements.txt"]),
    ("pip3", &["install -r requirements.txt"]),
    ("bundle", &["install"]),
    ("composer", &["install"]),
    ("mix", &["deps.get", "test"]),
    ("pytest", &[""]),
    ("mvn", &["test", "verify", "package"]),
    ("gradle", &["test", "build"]),
    ("./gradlew", &["test", "build"]),
    ("true", &[""]),
];
/// The `--` options each program may take, wherever they sit. Each one is a
/// switch that takes no value, so none can point a command at another
/// registry, file or folder; any other option is refused.
const KNOWN_FLAGS: [(&str, &[&str]); 12] = [
    (
        "npm",
        &[
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--prefer-offline",
        ],
    ),
    (
        "pnpm",
        &["--frozen-lockfile", "--ignore-scripts", "--prefer-offline"],
    ),
    (
        "yarn",
        &[
            "--frozen-lockfile",
            "--immutable",
            "--ignore-scripts",
            "--prefer-offline",
        ],
    ),
    ("bun", &["--frozen-lockfile", "--ignore-scripts"]),
    (
        "cargo",
        &[
            "--locked",
            "--frozen",
            "--offline",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--release",
        ],
    ),
    ("uv", &["--frozen", "--locked"]),
    ("poetry", &["--no-root", "--no-interaction"]),
    ("composer", &["--no-interaction", "--no-scripts"]),
    ("mvn", &["--batch-mode", "--quiet", "--offline"]),
    ("gradle", &["--no-daemon", "--offline"]),
    ("./gradlew", &["--no-daemon", "--offline"]),
    ("pytest", &["--quiet"]),
];
/// Whether a command is made only of what an answer may set: known install
/// and test commands, a package manager running one of the repository's own
/// scripts, or `make` with a target, joined by `&&` or `||`. Only the known
/// commands take options, and only their `KNOWN_FLAGS`, since anything after
/// a script's name goes to the script. Anything else is typed in
/// Repositories, so text in a pull request can't steer Bit into offering a
/// button that runs it.
fn allowed(command: &str) -> bool {
    let name = |w: &str| {
        !w.starts_with('-')
            && w.chars()
                .all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
    };
    let part = |text: &str| {
        let words: Vec<&str> = text.split_whitespace().collect();
        let Some((&program, rest)) = words.split_first() else {
            return false;
        };
        let flags = KNOWN_FLAGS
            .iter()
            .find(|(known, _)| *known == program)
            .map_or(&[][..], |(_, flags)| *flags);
        let (options, plain): (Vec<&str>, Vec<&str>) =
            rest.iter().copied().partition(|w| w.starts_with("--"));
        if !options.iter().all(|o| flags.contains(o)) {
            return false;
        }
        match plain.as_slice() {
            ["run", script] if SCRIPT_RUNNERS.contains(&program) => {
                options.is_empty() && name(script)
            }
            [target] if program == "make" => name(target),
            [] if program == "make" => true,
            _ => KNOWN_COMMANDS.iter().any(|(known, words)| {
                *known == program && words.contains(&plain.join(" ").as_str())
            }),
        }
    };
    command.split("&&").flat_map(|p| p.split("||")).all(part)
}
/// A command an answer may set: one line, short, with nothing that looks
/// like a credential, and `allowed`. Empty clears it.
fn command(value: &str) -> Option<String> {
    let fits = value.is_empty()
        || (value.chars().count() <= MAX_COMMAND
            && !value.chars().any(char::is_control)
            && credential_kinds(value).is_empty()
            && allowed(value));
    fits.then(|| value.to_string())
}

/// What Orbit tells Codex about a repository's copy: its telltale files,
/// and the scripts and package manager its `package.json` names.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneFacts {
    pub files: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<String>,
    /// The setup command Orbit suggests, as `suggest` gives it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_setup: Option<String>,
}
/// Reads a copy's telltale files. Names from `package.json` are kept only
/// when short and made of plain characters, since anyone who can push to
/// the repository writes them.
pub fn clone_facts(path: &Path) -> Result<CloneFacts, String> {
    if !path.is_dir() {
        return Err("Orbit can't find this repository's copy.".into());
    }
    let files: Vec<String> = TELLTALE_FILES
        .iter()
        .filter(|f| path.join(f).is_file())
        .map(|f| f.to_string())
        .collect();
    // Only a plain file: a link could lead anywhere, even somewhere that
    // never finishes reading.
    let package_json = path.join("package.json");
    let package: serde_json::Value = std::fs::symlink_metadata(&package_json)
        .is_ok_and(|m| m.is_file() && m.len() <= 256 * 1024)
        .then(|| std::fs::File::open(&package_json).ok())
        .flatten()
        .and_then(|f| serde_json::from_reader(std::io::BufReader::new(f)).ok())
        .unwrap_or_default();
    let plain = |name: &&str| {
        !name.is_empty()
            && name.len() <= 40
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "@.:_+-/".contains(c))
    };
    let scripts = package["scripts"]
        .as_object()
        .into_iter()
        .flat_map(|scripts| scripts.keys())
        .map(String::as_str)
        .filter(plain)
        .take(MAX_SCRIPTS)
        .map(str::to_owned)
        .collect();
    // Without its checksum, as in `pnpm@9.1.0`.
    let package_manager = package["packageManager"]
        .as_str()
        .and_then(|v| v.split('+').next())
        .filter(plain)
        .map(str::to_owned);
    Ok(CloneFacts {
        suggested_setup: suggest(&files).map(|(_, command)| command.to_string()),
        files,
        scripts,
        package_manager,
    })
}

/// Where Orbit keeps its own copy of `repo`: `clones/<owner>/<name>` in
/// its data folder.
pub fn clone_folder(root: &Path, repo: &str) -> Result<PathBuf, String> {
    data_folder(root, "clones", repo)
}
/// Where Orbit downloads its copy of `repo` before moving it into
/// `clone_folder`, so a download cut short never looks like a copy:
/// `downloads/<owner>/<name>` in its data folder.
pub fn download_folder(root: &Path, repo: &str) -> Result<PathBuf, String> {
    data_folder(root, "downloads", repo)
}
fn data_folder(root: &Path, kind: &str, repo: &str) -> Result<PathBuf, String> {
    validate_repo(repo)?;
    let (owner, name) = repo.split_once('/').ok_or("Invalid repository")?;
    Ok(root.join(kind).join(owner).join(name))
}

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;
