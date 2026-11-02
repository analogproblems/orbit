//! Version files: what the files Orbit knows say a repository's version
//! is, at its default branch's latest commit, and where files that should
//! agree don't. Orbit reads them with the workflow files
//! (`crate::workflows::read`), at the top of the repository and in a Tauri
//! app's `src-tauri` folder. Packages in a workspace keep their own
//! versions, so only these files are read.
//!
//! Files agree in groups: a manifest and the lockfile that records it
//! (`package.json` and `package-lock.json`, `Cargo.toml` and `Cargo.lock`,
//! `pyproject.toml` and `uv.lock`), and a Tauri app's `tauri.conf.json`,
//! when it gives a version itself, with the `Cargo.toml` beside it and the
//! `package.json` at the top. Other files may keep versions of their own,
//! so they are never compared. Nothing here reaches GitHub.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The files Orbit reads, by path, each with its name in
/// `crate::workflows::query`, which reads `package.json` already.
pub const FILES: [(&str, &str); 9] = [
    ("package", "package.json"),
    ("packageLock", "package-lock.json"),
    ("cargo", "Cargo.toml"),
    ("cargoLock", "Cargo.lock"),
    ("pyproject", "pyproject.toml"),
    ("uvLock", "uv.lock"),
    ("tauri", "src-tauri/tauri.conf.json"),
    ("tauriCargo", "src-tauri/Cargo.toml"),
    ("tauriLock", "src-tauri/Cargo.lock"),
];
/// The longest version Orbit keeps; a longer one isn't a version.
const LONGEST: usize = 64;

/// What a file is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Package,
    PackageLock,
    Cargo,
    CargoLock,
    Pyproject,
    UvLock,
    Tauri,
}
impl Kind {
    fn is_lock(&self) -> bool {
        matches!(self, Kind::PackageLock | Kind::CargoLock | Kind::UvLock)
    }
}
/// A file that says a version.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VersionFile {
    pub path: String,
    pub kind: Kind,
    pub version: String,
    /// For a lockfile, the manifest whose package it records, as a
    /// workspace's one lockfile records each of its packages.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub of: String,
}
/// A file that says another version than the files it should agree with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Mismatch {
    /// Its number in its problem, never used twice.
    pub number: u32,
    pub path: String,
    pub kind: Kind,
    /// For a lockfile, the manifest whose package it records.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub of: String,
    /// What it says, and what the others say.
    pub says: String,
    pub want: String,
    /// The files that say `want`.
    pub others: Vec<String>,
    /// Every other file it was compared with, some of which may only join
    /// it to the others, such as a `tauri.conf.json` between a
    /// `package.json` and a `Cargo.toml`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub group: Vec<String>,
    /// The commit Orbit first found it at.
    pub since: String,
}
impl Mismatch {
    /// Whether it is about the same file as `other`, whatever the versions
    /// now.
    pub fn same(&self, other: &Mismatch) -> bool {
        self.path == other.path && self.of == other.of
    }
    /// What it is, in a few words: "src-tauri/Cargo.toml says 0.2.0".
    pub fn name(&self) -> String {
        format!("{} says {}", self.path, self.says)
    }
    /// How to make it agree, for a file the others say another version
    /// than: a lockfile is made again from its manifest.
    pub fn fix(&self) -> &'static str {
        match self.kind {
            Kind::PackageLock => "npm install --package-lock-only",
            Kind::CargoLock => "cargo update --workspace",
            Kind::UvLock => "uv lock",
            _ => "",
        }
    }
}

/// What a file says, as Orbit reads it.
enum Says<T> {
    /// Orbit can't make it out.
    Unreadable,
    /// It gives no version, or works it out when building.
    Nothing,
    Version(T),
}

/// What the files Orbit read say. `files` holds each of `FILES` that is
/// there, with its text, or None when GitHub didn't send all of it.
/// Returns the files that say a version, and the paths Orbit couldn't read
/// or make out, which may say one.
pub fn found(files: &[(&str, Option<&str>)]) -> (Vec<VersionFile>, Vec<String>) {
    let text = |path: &str| files.iter().find(|(p, _)| *p == path).and_then(|(_, t)| *t);
    let mut read: Vec<VersionFile> = vec![];
    let mut unread: Vec<String> = files
        .iter()
        .filter(|(_, t)| t.is_none())
        .map(|(p, _)| p.to_string())
        .collect();
    let mut keep = |path: &str, kind: Kind, says: Says<String>, of: &str| match says {
        Says::Version(version) => read.push(VersionFile {
            path: path.to_string(),
            kind,
            version,
            of: of.to_string(),
        }),
        Says::Unreadable => unread.push(path.to_string()),
        Says::Nothing => {}
    };
    if let Some(t) = text("package.json") {
        match package(t) {
            Says::Version((name, version)) => {
                keep("package.json", Kind::Package, Says::Version(version), "");
                if let Some(lock) = text("package-lock.json") {
                    let says = package_lock(lock, &name);
                    keep("package-lock.json", Kind::PackageLock, says, "package.json");
                }
            }
            Says::Unreadable => keep("package.json", Kind::Package, Says::Unreadable, ""),
            Says::Nothing => {}
        }
    }
    // The top `Cargo.toml`, for a member's workspace version and lockfile.
    let top = text("Cargo.toml").and_then(|t| toml::from_str::<toml::Table>(t).ok());
    let workspace = top.as_ref().and_then(|t| {
        let v = t.get("workspace")?.get("package")?.get("version")?;
        v.as_str().map(str::to_string)
    });
    for folder in ["", "src-tauri/"] {
        let manifest = format!("{folder}Cargo.toml");
        let Some(t) = text(&manifest) else {
            continue;
        };
        match cargo(t, workspace.as_deref()) {
            Says::Version((name, version)) => {
                keep(&manifest, Kind::Cargo, Says::Version(version), "");
                // A member of the top's workspace uses the top's lockfile,
                // as Cargo does, whatever lockfile of its own is left in
                // its folder; another package its own, or else the top's.
                let own = format!("{folder}Cargo.lock");
                let member = !folder.is_empty() && member(top.as_ref(), t, folder);
                let paths = match member {
                    true => vec!["Cargo.lock"],
                    false => vec![own.as_str(), "Cargo.lock"],
                };
                let lock = paths.into_iter().find_map(|p| Some((p, text(p)?)));
                if let Some((path, lock)) = lock {
                    keep(
                        path,
                        Kind::CargoLock,
                        lock_entry(lock, &name, false),
                        &manifest,
                    );
                }
            }
            Says::Unreadable => keep(&manifest, Kind::Cargo, Says::Unreadable, ""),
            Says::Nothing => {}
        }
    }
    if let Some(t) = text("pyproject.toml") {
        match pyproject(t) {
            Says::Version((name, version)) => {
                keep(
                    "pyproject.toml",
                    Kind::Pyproject,
                    Says::Version(version),
                    "",
                );
                if let Some(lock) = text("uv.lock") {
                    let says = lock_entry(lock, &name, true);
                    keep("uv.lock", Kind::UvLock, says, "pyproject.toml");
                }
            }
            Says::Unreadable => keep("pyproject.toml", Kind::Pyproject, Says::Unreadable, ""),
            Says::Nothing => {}
        }
    }
    if let Some(t) = text("src-tauri/tauri.conf.json") {
        keep("src-tauri/tauri.conf.json", Kind::Tauri, tauri(t), "");
    }
    unread.sort();
    unread.dedup();
    (read, unread)
}
/// What the files say in GitHub's answer to `crate::workflows::query`, as
/// `found` reads them: a file that isn't there, or is a folder, is left
/// out, and one GitHub didn't send all of, or sent as binary, can't be
/// read.
pub fn read(repository: &Value) -> (Vec<VersionFile>, Vec<String>) {
    let files: Vec<(&str, Option<&str>)> = FILES
        .iter()
        .filter_map(|(name, path)| {
            let blob = repository.get(*name).filter(|b| b.get("text").is_some())?;
            let whole = blob["isTruncated"] != true && blob["isBinary"] != true;
            Some((*path, blob["text"].as_str().filter(|_| whole)))
        })
        .collect();
    found(&files)
}
/// Files that disagree in a sentence: "A says 1.0.0, B says 0.9.0 and 2
/// more".
pub fn names(mismatches: &[Mismatch]) -> String {
    let all: Vec<String> = mismatches.iter().map(Mismatch::name).collect();
    match all.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
    }
}
/// How many files disagree, in a few words: "1 version file", "3 version
/// files".
pub fn count(n: usize) -> String {
    match n {
        1 => "1 version file".to_string(),
        n => format!("{n} version files"),
    }
}
/// A version as a file says it, or None when it isn't one.
fn plain(v: Option<&str>) -> Option<String> {
    let v = v?.trim();
    let fine = !v.is_empty()
        && v.len() <= LONGEST
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c));
    fine.then(|| v.to_string())
}
fn version(v: Option<&str>) -> Says<String> {
    match plain(v) {
        Some(v) => Says::Version(v),
        None => Says::Unreadable,
    }
}
/// A `package.json`'s name and version.
fn package(text: &str) -> Says<(String, String)> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Says::Unreadable;
    };
    let Some(given) = v.get("version") else {
        return Says::Nothing;
    };
    let name = v["name"].as_str().unwrap_or_default().to_string();
    match version(given.as_str()) {
        Says::Version(version) => Says::Version((name, version)),
        _ => Says::Unreadable,
    }
}
/// The version a `package-lock.json` records for its own package.
fn package_lock(text: &str, name: &str) -> Says<String> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Says::Unreadable;
    };
    let own = &v["packages"][""];
    if !own.is_null() {
        return match own.get("version") {
            Some(given) => version(given.as_str()),
            None => Says::Nothing,
        };
    }
    // An old lockfile, with no packages: its own fields.
    if v["name"].as_str().is_some_and(|n| n != name) {
        return Says::Nothing;
    }
    match v.get("version") {
        Some(given) => version(given.as_str()),
        None => Says::Nothing,
    }
}
/// A folder or pattern as Cargo writes one in a workspace: "./a/" is "a".
fn folder_name(path: &str) -> &str {
    path.trim_start_matches("./").trim_end_matches('/')
}
/// Whether a glob such as `crates/*` or `src-?auri` takes in all of `name`.
fn glob(pattern: &str, name: &str) -> bool {
    match (pattern.chars().next(), name.chars().next()) {
        (None, None) => true,
        (Some('*'), _) => {
            glob(&pattern[1..], name)
                || name
                    .chars()
                    .next()
                    .is_some_and(|c| c != '/' && glob(pattern, &name[c.len_utf8()..]))
        }
        (Some('?'), Some(c)) if c != '/' => glob(&pattern[1..], &name[c.len_utf8()..]),
        (Some(p), Some(c)) if p == c => glob(&pattern[p.len_utf8()..], &name[c.len_utf8()..]),
        _ => false,
    }
}
/// Whether the package in `folder` (such as `src-tauri/`), with the
/// manifest `text`, is a member of the workspace in the top `Cargo.toml`:
/// its manifest names the top as its workspace, or the top's
/// `workspace.members` take it in, or the top's package depends on it by
/// path, and `workspace.exclude` doesn't leave it out.
fn member(top: Option<&toml::Table>, text: &str, folder: &str) -> bool {
    let Some(top) = top else {
        return false;
    };
    let Some(workspace) = top.get("workspace") else {
        return false;
    };
    let folder = folder_name(folder);
    let listed = |key: &str| -> Vec<&str> {
        workspace
            .get(key)
            .and_then(|l| l.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .map(folder_name)
            .collect()
    };
    let own = toml::from_str::<toml::Table>(text).ok();
    if let Some(at) = own
        .as_ref()
        .and_then(|t| t.get("package")?.get("workspace")?.as_str())
    {
        return folder_name(at) == "..";
    }
    if listed("exclude")
        .iter()
        .any(|e| folder == *e || folder.starts_with(&format!("{e}/")))
    {
        return false;
    }
    let by_path = ["dependencies", "dev-dependencies", "build-dependencies"]
        .iter()
        .filter_map(|k| top.get(*k)?.as_table())
        .flat_map(|deps| deps.values())
        .filter_map(|d| d.get("path")?.as_str())
        .any(|p| folder_name(p) == folder);
    by_path || listed("members").iter().any(|m| glob(m, folder))
}
/// A `Cargo.toml`'s package name and version, taking the workspace's when
/// it says to.
fn cargo(text: &str, workspace: Option<&str>) -> Says<(String, String)> {
    let Ok(t) = toml::from_str::<toml::Table>(text) else {
        return Says::Unreadable;
    };
    // A workspace with no package of its own.
    let Some(package) = t.get("package") else {
        return Says::Nothing;
    };
    let Some(name) = package.get("name").and_then(|n| n.as_str()) else {
        return Says::Unreadable;
    };
    let given = match package.get("version") {
        None => return Says::Nothing,
        Some(toml::Value::String(v)) => Some(v.as_str()),
        // `version.workspace = true`: the workspace's.
        Some(toml::Value::Table(v))
            if v.get("workspace").and_then(|w| w.as_bool()) == Some(true) =>
        {
            let own = t
                .get("workspace")
                .and_then(|w| w.get("package")?.get("version")?.as_str());
            match own.or(workspace) {
                Some(v) => Some(v),
                None => return Says::Nothing,
            }
        }
        Some(_) => None,
    };
    match version(given) {
        Says::Version(v) => Says::Version((name.to_string(), v)),
        _ => Says::Unreadable,
    }
}
/// The version a `Cargo.lock` or `uv.lock` records for `name`, with
/// Python's way of comparing names for `uv.lock`. Two of the same name are
/// a dependency's versions too, so Orbit can't tell which is the
/// repository's own, and none says nothing.
fn lock_entry(text: &str, name: &str, python: bool) -> Says<String> {
    let Ok(t) = toml::from_str::<toml::Table>(text) else {
        return Says::Unreadable;
    };
    let Some(packages) = t.get("package").and_then(|p| p.as_array()) else {
        return Says::Nothing;
    };
    let same = |n: &str| match python {
        true => python_name(n) == python_name(name),
        false => n == name,
    };
    let named: Vec<&toml::Value> = packages
        .iter()
        .filter(|p| p.get("name").and_then(|n| n.as_str()).is_some_and(same))
        .collect();
    match named.as_slice() {
        [one] => version(one.get("version").and_then(|v| v.as_str())),
        _ => Says::Nothing,
    }
}
/// A Python package's name as its index compares them: lower case, with
/// each run of `-`, `_` and `.` as one `-`.
fn python_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = if "-_.".contains(c) {
            '-'
        } else {
            c.to_ascii_lowercase()
        };
        if !(c == '-' && out.ends_with('-')) {
            out.push(c);
        }
    }
    out
}
/// A `pyproject.toml`'s name and version, from `[project]` or Poetry's
/// table. One that works its version out when building says none.
fn pyproject(text: &str) -> Says<(String, String)> {
    let Ok(t) = toml::from_str::<toml::Table>(text) else {
        return Says::Unreadable;
    };
    let poetry = t.get("tool").and_then(|t| t.get("poetry"));
    let Some(table) = t.get("project").or(poetry) else {
        return Says::Nothing;
    };
    let name = table
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or_default();
    match table.get("version") {
        None => Says::Nothing,
        Some(v) => match version(v.as_str()) {
            Says::Version(v) => Says::Version((name.to_string(), v)),
            _ => Says::Unreadable,
        },
    }
}
/// The version a `tauri.conf.json` gives itself. One that names a file to
/// take it from, such as `../package.json`, or has none, says none.
fn tauri(text: &str) -> Says<String> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Says::Unreadable;
    };
    match v.get("version") {
        None => Says::Nothing,
        Some(Value::String(s)) if s.ends_with(".json") => Says::Nothing,
        Some(given) => version(given.as_str()),
    }
}

/// Which of `files` should say the same version, as a group for each:
/// a manifest with the lockfile entries for its package, and a Tauri app's
/// `tauri.conf.json` with the `Cargo.toml` beside it and the top
/// `package.json`. Groups that share a file are one group.
fn groups(files: &[VersionFile]) -> Vec<usize> {
    fn top(group: &[usize], mut i: usize) -> usize {
        while group[i] != i {
            i = group[i];
        }
        i
    }
    fn join(group: &mut [usize], a: usize, b: usize) {
        let (a, b) = (top(group, a), top(group, b));
        group[a.max(b)] = a.min(b);
    }
    let mut group: Vec<usize> = (0..files.len()).collect();
    let manifest = |path: &str| {
        files
            .iter()
            .position(|f| f.path == path && !f.kind.is_lock())
    };
    for (i, f) in files.iter().enumerate() {
        if let Some(m) = manifest(&f.of).filter(|_| f.kind.is_lock()) {
            join(&mut group, i, m);
        }
    }
    if let Some(t) = manifest("src-tauri/tauri.conf.json") {
        for path in ["src-tauri/Cargo.toml", "package.json"] {
            if let Some(m) = manifest(path) {
                join(&mut group, t, m);
            }
        }
    }
    (0..files.len()).map(|i| top(&group, i)).collect()
}
/// A Python version as PEP 440 compares it, written one way: `1.0-alpha1`,
/// `1.0a1` and `1.0.0a1` are all `1a1`. None when it isn't one.
pub fn pep440(v: &str) -> Option<String> {
    let v = v.trim().to_ascii_lowercase();
    let v = v.strip_prefix('v').unwrap_or(&v);
    let (v, local) = match v.split_once('+') {
        Some((v, local)) => (v, Some(local)),
        None => (v, None),
    };
    let (epoch, mut rest) = match v.split_once('!') {
        Some((e, rest)) => (e.parse::<u64>().ok()?, rest),
        None => (0, v),
    };
    let digits = |s: &str| s.chars().take_while(|c| c.is_ascii_digit()).count();
    // The release: numbers between dots.
    let mut release = vec![];
    loop {
        let n = digits(rest);
        if n == 0 {
            return None;
        }
        release.push(rest[..n].parse::<u64>().ok()?);
        rest = &rest[n..];
        match rest.strip_prefix('.') {
            Some(after) if digits(after) > 0 => rest = after,
            _ => break,
        }
    }
    while release.len() > 1 && release.last() == Some(&0) {
        release.pop();
    }
    let sep = |s: &str| s.strip_prefix(['.', '-', '_']).unwrap_or(s).to_string();
    // A word, with what follows it, after an optional separator.
    let word = |s: &str, words: &[&str]| -> Option<(String, String)> {
        let after = sep(s);
        words
            .iter()
            .find(|w| after.starts_with(**w))
            .map(|w| (w.to_string(), after[w.len()..].to_string()))
    };
    // A number after an optional separator, or 0.
    let number = |s: &str| -> Option<(u64, String)> {
        let after = sep(s);
        let n = digits(&after);
        if n == 0 {
            return Some((0, s.to_string()));
        }
        Some((after[..n].parse().ok()?, after[n..].to_string()))
    };
    let mut out = String::new();
    if epoch > 0 {
        out.push_str(&format!("{epoch}!"));
    }
    let release: Vec<String> = release.iter().map(u64::to_string).collect();
    out.push_str(&release.join("."));
    let mut rest = rest.to_string();
    let pre = ["alpha", "beta", "preview", "pre", "rc", "a", "b", "c"];
    if let Some((w, after)) = word(&rest, &pre) {
        let (n, after) = number(&after)?;
        let w = match w.as_str() {
            "alpha" | "a" => "a",
            "beta" | "b" => "b",
            _ => "rc",
        };
        out.push_str(&format!("{w}{n}"));
        rest = after;
    }
    if let Some(after) = rest.strip_prefix('-').filter(|a| digits(a) > 0) {
        let n = digits(after);
        out.push_str(&format!(".post{}", after[..n].parse::<u64>().ok()?));
        rest = after[n..].to_string();
    } else if let Some((_, after)) = word(&rest, &["post", "rev", "r"]) {
        let (n, after) = number(&after)?;
        out.push_str(&format!(".post{n}"));
        rest = after;
    }
    if let Some((_, after)) = word(&rest, &["dev"]) {
        let (n, after) = number(&after)?;
        out.push_str(&format!(".dev{n}"));
        rest = after;
    }
    if !rest.is_empty() {
        return None;
    }
    if let Some(local) = local {
        let parts: Vec<&str> = local.split(['.', '-', '_']).collect();
        if parts
            .iter()
            .any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric()))
        {
            return None;
        }
        out.push('+');
        out.push_str(&parts.join("."));
    }
    Some(out)
}
/// What a file's version is compared by: as written, but a Python
/// project's as PEP 440 compares it, as `uv lock` writes it its own way.
fn compared(f: &VersionFile) -> String {
    match f.kind {
        Kind::Pyproject | Kind::UvLock => pep440(&f.version).unwrap_or_else(|| f.version.clone()),
        _ => f.version.clone(),
    }
}
/// The files that say another version than the files they should agree
/// with, found at `sha`, for the caller to number. In each group, the
/// version most of its manifests say is the one, as a lockfile only copies
/// its manifest; with no most, the one the first manifest says, in the
/// order of `FILES`.
pub fn mismatches(files: &[VersionFile], sha: &str) -> Vec<Mismatch> {
    let group = groups(files);
    let mut found = vec![];
    for (i, f) in files.iter().enumerate() {
        let members: Vec<&VersionFile> = files
            .iter()
            .zip(&group)
            .filter(|(_, g)| **g == group[i])
            .map(|(f, _)| f)
            .collect();
        if members.len() < 2 {
            continue;
        }
        let group: Vec<String> = members
            .iter()
            .filter(|m| m.path != f.path || m.of != f.of)
            .map(|m| m.path.clone())
            .collect();
        let manifests: Vec<&VersionFile> =
            members.into_iter().filter(|m| !m.kind.is_lock()).collect();
        let count = |v: &str| manifests.iter().filter(|m| compared(m) == v).count();
        let most = manifests.iter().map(|m| count(&compared(m))).max();
        let Some(want) = manifests.iter().find(|m| Some(count(&compared(m))) == most) else {
            continue;
        };
        if compared(f) == compared(want) {
            continue;
        }
        found.push(Mismatch {
            path: f.path.clone(),
            kind: f.kind,
            of: f.of.clone(),
            says: f.version.clone(),
            want: want.version.clone(),
            others: manifests
                .iter()
                .filter(|m| compared(m) == compared(want))
                .map(|m| m.path.clone())
                .collect(),
            group,
            since: sha.to_string(),
            ..Default::default()
        });
    }
    found
}

#[cfg(test)]
#[path = "versions_tests.rs"]
mod tests;
