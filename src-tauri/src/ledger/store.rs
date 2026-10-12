//! Reading GitHub's answers into the stored form, and keeping one file per
//! pull request under `ledger/<owner>/<name>/` in Orbit's data folder.
use super::*;
use crate::types::validate_repo;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// How many earlier texts of an edited comment are kept.
pub const REVISIONS: usize = 10;
const DIR: &str = "ledger";
/// Each repository's write-ups, beside its pull requests.
const WRITEUPS: &str = "writeups";

/// The stored form of a pull request from GitHub's answers: the pull
/// request, its conversation comments, reviews, comments on changed lines,
/// commits and changed files. A comment whose text changed since `previous`
/// keeps its earlier text as a revision.
#[allow(clippy::too_many_arguments)]
pub fn from_github(
    repo: &str,
    pr: &Value,
    issue_comments: &[Value],
    reviews: &[Value],
    review_comments: &[Value],
    commits: &[Value],
    files: &[Value],
    previous: Option<&StoredPr>,
    fetched_at: &str,
) -> StoredPr {
    let login = |v: &Value| v["user"]["login"].as_str().unwrap_or("").to_string();
    let number = |v: &Value, key: &str| v[key].as_u64().and_then(|n| u32::try_from(n).ok());
    let mut comments = vec![];
    for c in issue_comments {
        comments.push(StoredComment {
            id: c["id"].as_u64().unwrap_or(0),
            kind: CommentKind::Issue,
            author: login(c),
            body: text(c, "body"),
            url: text(c, "html_url"),
            created_at: text(c, "created_at"),
            updated_at: text(c, "updated_at"),
            ..Default::default()
        });
    }
    for r in reviews {
        // A review still being written has no submitted time and no reader.
        let Some(at) = r["submitted_at"].as_str() else {
            continue;
        };
        comments.push(StoredComment {
            id: r["id"].as_u64().unwrap_or(0),
            kind: CommentKind::Review,
            author: login(r),
            body: text(r, "body"),
            url: text(r, "html_url"),
            created_at: at.into(),
            updated_at: at.into(),
            ..Default::default()
        });
    }
    for c in review_comments {
        comments.push(StoredComment {
            id: c["id"].as_u64().unwrap_or(0),
            kind: CommentKind::ReviewComment,
            author: login(c),
            body: text(c, "body"),
            url: text(c, "html_url"),
            created_at: text(c, "created_at"),
            updated_at: text(c, "updated_at"),
            path: c["path"].as_str().map(str::to_owned),
            line: number(c, "line").or_else(|| number(c, "original_line")),
            start_line: number(c, "start_line").or_else(|| number(c, "original_start_line")),
            in_reply_to: c["in_reply_to_id"].as_u64(),
            review_id: c["pull_request_review_id"].as_u64(),
            ..Default::default()
        });
    }
    comments.retain(|c| c.id != 0);
    if let Some(previous) = previous {
        for c in &mut comments {
            let Some(old) = previous
                .comments
                .iter()
                .find(|o| o.id == c.id && o.kind == c.kind)
            else {
                continue;
            };
            c.revisions = old.revisions.clone();
            if old.body != c.body && !old.body.trim().is_empty() {
                c.revisions.push(Revision {
                    body: old.body.clone(),
                    updated_at: old.updated_at.clone(),
                });
            }
            let extra = c.revisions.len().saturating_sub(REVISIONS);
            c.revisions.drain(..extra);
        }
    }
    StoredPr {
        version: STORED_VERSION,
        repo: repo.into(),
        fetched_at: fetched_at.into(),
        pr: PrInfo {
            number: pr["number"].as_u64().unwrap_or(0),
            title: text(pr, "title"),
            url: text(pr, "html_url"),
            state: text(pr, "state"),
            merged: pr["merged"].as_bool().unwrap_or(false) || pr["merged_at"].is_string(),
            draft: pr["draft"].as_bool().unwrap_or(false),
            author: login(pr),
            body: text(pr, "body"),
            created_at: text(pr, "created_at"),
            updated_at: text(pr, "updated_at"),
            closed_at: pr["closed_at"].as_str().map(str::to_owned),
            merged_at: pr["merged_at"].as_str().map(str::to_owned),
            merged_by: pr["merged_by"]["login"].as_str().map(str::to_owned),
            head_ref: text(&pr["head"], "ref"),
            head_sha: text(&pr["head"], "sha"),
            base_ref: text(&pr["base"], "ref"),
            additions: pr["additions"].as_u64().unwrap_or(0),
            deletions: pr["deletions"].as_u64().unwrap_or(0),
            changed_files: pr["changed_files"].as_u64().unwrap_or(0),
        },
        comments,
        commits: commits
            .iter()
            .map(|c| CommitRef {
                sha: text(c, "sha"),
                message: text(&c["commit"], "message"),
                author: text(&c["commit"]["author"], "name"),
                date: text(&c["commit"]["author"], "date"),
                url: text(c, "html_url"),
            })
            .collect(),
        files: files
            .iter()
            .filter_map(|f| f["filename"].as_str().map(str::to_owned))
            .collect(),
    }
}

/// The folder holding a repository's pull requests. Names are lowercased,
/// as GitHub's are case-insensitive.
fn repo_dir(root: &Path, repo: &str) -> Result<PathBuf, String> {
    validate_repo(repo)?;
    let (owner, name) = repo.split_once('/').ok_or("Invalid repository name.")?;
    Ok(root
        .join(DIR)
        .join(owner.to_ascii_lowercase())
        .join(name.to_ascii_lowercase()))
}
/// Where a pull request is stored.
pub fn stored_path(root: &Path, repo: &str, number: u64) -> Result<PathBuf, String> {
    Ok(repo_dir(root, repo)?.join(format!("{number}.json")))
}
/// Saves a pull request, replacing the file in one step.
pub fn save(root: &Path, stored: &StoredPr) -> Result<(), String> {
    let path = stored_path(root, &stored.repo, stored.pr.number)?;
    let dir = path.parent().ok_or("Invalid history path.")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let data = serde_json::to_vec(stored).map_err(|e| e.to_string())?;
    let temp = dir.join(format!("{}.json.tmp", stored.pr.number));
    std::fs::write(&temp, data).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, &path).map_err(|e| e.to_string())
}
/// Saves a pull request's write-up, replacing the file in one step.
pub fn save_writeup(root: &Path, w: &writeup::WriteUp) -> Result<(), String> {
    let dir = repo_dir(root, &w.repo)?.join(WRITEUPS);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let data = serde_json::to_vec(w).map_err(|e| e.to_string())?;
    let temp = dir.join(format!("{}.json.tmp", w.number));
    std::fs::write(&temp, data).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, dir.join(format!("{}.json", w.number))).map_err(|e| e.to_string())
}
/// A repository's write-ups in the current form, by pull request. One
/// that can't be read, or is of another form, is made again.
pub fn load_writeups(root: &Path, repo: &str) -> Result<HashMap<u64, writeup::WriteUp>, String> {
    let dir = repo_dir(root, repo)?.join(WRITEUPS);
    let mut out = HashMap::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let read = std::fs::read(&path)
            .ok()
            .and_then(|data| serde_json::from_slice::<writeup::WriteUp>(&data).ok());
        if let Some(w) = read.filter(|w| w.version == writeup::WRITEUP_VERSION && w.number > 0) {
            out.insert(w.number, w);
        }
    }
    Ok(out)
}
/// A stored pull request in the current stored form, or None when there is
/// none or it can't be read, so it is fetched again.
pub fn load(root: &Path, repo: &str, number: u64) -> Result<Option<StoredPr>, String> {
    let path = stored_path(root, repo, number)?;
    Ok(std::fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice::<StoredPr>(&data).ok())
        .filter(|s| s.version == STORED_VERSION && s.pr.number == number))
}
/// Every stored pull request of a repository in the current stored form,
/// and a note for each file that couldn't be read.
pub fn load_repo(root: &Path, repo: &str) -> Result<(Vec<StoredPr>, Vec<String>), String> {
    let dir = repo_dir(root, repo)?;
    let mut found = vec![];
    let mut problems = vec![];
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((found, problems)),
        Err(e) => return Err(e.to_string()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|data| serde_json::from_slice::<StoredPr>(&data).map_err(|e| e.to_string()))
        {
            Ok(stored) if stored.version == STORED_VERSION && stored.pr.number > 0 => {
                found.push(stored)
            }
            // An older form is read again from GitHub on the next refresh.
            Ok(_) => {}
            Err(e) => problems.push(format!("{}: {e}", path.display())),
        }
    }
    Ok((found, problems))
}
/// Removes the stored history of repositories that are no longer connected.
pub fn prune(root: &Path, connected: &[String]) -> Result<(), String> {
    let keep: Vec<(String, String)> = connected
        .iter()
        .filter_map(|r| r.split_once('/'))
        .map(|(o, n)| (o.to_ascii_lowercase(), n.to_ascii_lowercase()))
        .collect();
    let Ok(owners) = std::fs::read_dir(root.join(DIR)) else {
        return Ok(());
    };
    for owner in owners.flatten() {
        let owner_name = owner.file_name().to_string_lossy().into_owned();
        let Ok(names) = std::fs::read_dir(owner.path()) else {
            continue;
        };
        let mut left = 0;
        for name in names.flatten() {
            let name_text = name.file_name().to_string_lossy().into_owned();
            if keep
                .iter()
                .any(|(o, n)| *o == owner_name && *n == name_text)
            {
                left += 1;
                continue;
            }
            // Never follows a link out of Orbit's folder: a link is removed
            // itself.
            let is_dir = name.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let removed = if is_dir {
                std::fs::remove_dir_all(name.path())
            } else {
                std::fs::remove_file(name.path())
            };
            removed.map_err(|e| e.to_string())?;
        }
        if left == 0 {
            let _ = std::fs::remove_dir(owner.path());
        }
    }
    Ok(())
}
