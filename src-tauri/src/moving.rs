//! Moving an Orbit into an always-on Orbit (see `crate::client`): what goes
//! with its state, and where each file may go. The engine packs and takes
//! in the move (`engine/moving.rs`); everything here is plain data
//! handling.
//!
//! Besides its state, an Orbit moves the files it can't read again as they
//! were: each repository's stored pull-request history and write-ups
//! (`ledger/<owner>/<name>/<n>.json` and `.../writeups/<n>.json`) and the
//! files kept from each finished security scan (`scans/<id>/<file>`).
//! Everything else in its data folder belongs to the computer it ran on:
//! review worktrees, its own copies of repositories, scan working files,
//! its access link and token.
use crate::{scans::KEPT_FILES, types::validate_repo};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The largest move an always-on Orbit takes in, in bytes.
pub const LIMIT: usize = 256 * 1024 * 1024;
const LEDGER: &str = "ledger";
const WRITEUPS: &str = "writeups";
const SCANS: &str = "scans";

/// An Orbit on its way to an always-on Orbit.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moved {
    /// Its state, as `state.json` holds it.
    pub state: Value,
    pub files: Vec<MovedFile>,
}
/// One of the files that go with an Orbit's state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovedFile {
    /// Where it is in the data folder, with `/` between folders.
    pub path: String,
    pub text: String,
}

/// Where a moved file goes in the data folder `root`, if it's one an
/// Orbit moves: a pull request's stored history or write-up, or a file
/// kept from one of `scans`.
pub fn destination(root: &Path, path: &str, scans: &[String]) -> Option<PathBuf> {
    let number = |file: &str| {
        file.strip_suffix(".json").is_some_and(|n| {
            !n.is_empty() && n.len() <= 20 && n.bytes().all(|b| b.is_ascii_digit())
        })
    };
    let repo = |owner: &str, name: &str| validate_repo(&format!("{owner}/{name}")).is_ok();
    let parts: Vec<&str> = path.split('/').collect();
    let allowed = match parts.as_slice() {
        [LEDGER, owner, name, file] => repo(owner, name) && number(file),
        [LEDGER, owner, name, WRITEUPS, file] => repo(owner, name) && number(file),
        [SCANS, id, file] => {
            scan_id(id) && scans.iter().any(|s| s == id) && KEPT_FILES.contains(file)
        }
        _ => false,
    };
    allowed.then(|| {
        parts
            .iter()
            .fold(root.to_path_buf(), |p, part| p.join(part))
    })
}
/// Scan ids are Orbit's own, letters, digits and dashes.
fn scan_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The files to move from the data folder `root`, for an Orbit with
/// `scans`.
pub fn collect(root: &Path, scans: &[String]) -> Result<Vec<MovedFile>, String> {
    let mut found = vec![];
    walk(&root.join(LEDGER), 0, &mut found)?;
    for id in scans.iter().filter(|id| scan_id(id)) {
        for file in KEPT_FILES {
            found.push(root.join(SCANS).join(id).join(file));
        }
    }
    let mut files = vec![];
    for path in found {
        let Some(relative) = relative(root, &path) else {
            continue;
        };
        if destination(root, &relative, scans).is_none() || !regular(&path) {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("Orbit couldn't read {}: {e}", path.display()))?;
        files.push(MovedFile {
            path: relative,
            text,
        });
    }
    Ok(files)
}
/// Every regular file under `dir`, at most four folders down, never
/// following a link.
fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > 4 {
        return Ok(());
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("Orbit couldn't read {}: {e}", dir.display())),
    };
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            walk(&entry.path(), depth + 1, found)?;
        } else if kind.is_file() {
            found.push(entry.path());
        }
    }
    Ok(())
}
fn regular(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}
/// `path` inside `root`, with `/` between folders.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let parts: Option<Vec<&str>> = path
        .strip_prefix(root)
        .ok()?
        .components()
        .map(|c| match c {
            std::path::Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect();
    Some(parts?.join("/"))
}

/// Puts moved `files` in place in the data folder `root`, for an Orbit
/// with `scans`. Nothing is written unless every file has its place, and
/// each replaces what was there in one step.
pub fn put(root: &Path, files: &[MovedFile], scans: &[String]) -> Result<(), String> {
    let mut places = vec![];
    for file in files {
        let place = destination(root, &file.path, scans).ok_or_else(|| {
            format!(
                "The moved Orbit sent a file Orbit doesn't move: {}",
                file.path
            )
        })?;
        places.push(place);
    }
    for (file, place) in files.iter().zip(places) {
        let dir = place.parent().ok_or("Invalid folder")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let name = place.file_name().ok_or("Invalid file")?.to_string_lossy();
        let temp = dir.join(format!("{name}.tmp"));
        std::fs::write(&temp, &file.text).map_err(|e| format!("{}: {e}", file.path))?;
        std::fs::rename(&temp, &place).map_err(|e| format!("{}: {e}", file.path))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "moving_tests.rs"]
mod tests;
