//! GitHub Actions storage: each connected repository's caches and the
//! artifacts it keeps, read every `EVERY`, a few repositories a pass.
//! GitHub drops a repository's least used caches past its limit, usually
//! `CACHE_LIMIT`, and an account whose artifact storage is full can't upload
//! more (see `logs::QUOTA`). Orbit can't see either limit for sure: a
//! repository can have a higher cache limit, and an account's storage is
//! billing, which Orbit doesn't read. Nothing here reaches GitHub but reads.
use crate::{
    github::{Pages, PAGE},
    logs,
    problems::{stamp, time},
    types::*,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Repositories Orbit reads in one pass.
pub const PER_PASS: usize = 4;
/// How often Orbit reads each repository again.
pub const EVERY: Duration = Duration::minutes(30);
/// A gigabyte, as GitHub counts storage.
pub const GB: u64 = 1 << 30;
/// GitHub's usual limit for a repository's caches.
pub const CACHE_LIMIT: u64 = 10 * GB;
/// Caches this big are near the usual limit.
pub const NEAR: u64 = 9 * GB;
/// Pages of a repository's artifacts Orbit reads at most in one go.
pub const ARTIFACT_PAGES: u32 = 3;

/// What Orbit read of one repository's GitHub Actions storage.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Storage {
    pub repo: String,
    /// When Orbit last read it, by Orbit's clock.
    pub read_at: String,
    /// Its caches: how many, and how many bytes they hold.
    pub caches: u32,
    pub cache_bytes: u64,
    /// The artifacts it keeps, that haven't expired, of those Orbit read:
    /// how many, and how many bytes they hold.
    pub artifacts: u32,
    pub artifact_bytes: u64,
    /// GitHub has more artifacts than Orbit read, so it may keep more.
    pub more_artifacts: bool,
    /// Why Orbit can't see its storage, when GitHub won't show it.
    pub unseen: Option<String>,
    /// Why the last read failed, how many in a row did, and when Orbit
    /// tries again. What was read before is kept meanwhile.
    pub error: Option<String>,
    pub tries: u32,
    pub retry_at: Option<String>,
}

/// The repositories Orbit should read now, at most `PER_PASS`: those never
/// read first, then those read longest ago. One is read again `EVERY`
/// after it was, or after its wait once a read failed.
pub fn wanted(repos: &[String], read: &[Storage], now: DateTime<Utc>) -> Vec<String> {
    let mut due: Vec<(String, String)> = repos
        .iter()
        .filter(|r| validate_repo(r).is_ok())
        .filter_map(|repo| {
            let Some(s) = read.iter().find(|s| same_repo(&s.repo, repo)) else {
                return Some((String::new(), repo.clone()));
            };
            let ready = match &s.error {
                Some(_) => s
                    .retry_at
                    .as_deref()
                    .and_then(time)
                    .is_none_or(|retry| now >= retry),
                None => time(&s.read_at).is_none_or(|at| now - at >= EVERY),
            };
            ready.then(|| (s.read_at.clone(), repo.clone()))
        })
        .collect();
    due.sort_by(|a, b| a.0.cmp(&b.0));
    due.into_iter()
        .take(PER_PASS)
        .map(|(_, repo)| repo)
        .collect()
}

/// Reads a repository's caches and the artifacts it keeps.
pub async fn read(repo: &str) -> Result<Storage, String> {
    validate_repo(repo)?;
    let now = Utc::now();
    let mut storage = Storage {
        repo: repo.to_string(),
        read_at: stamp(now),
        ..Default::default()
    };
    let usage = crate::github::api(
        &format!("repos/{repo}/actions/cache/usage"),
        "GET",
        &[],
        None,
    )
    .await;
    let Some(usage) = seen(&mut storage, usage)? else {
        return Ok(storage);
    };
    (storage.caches, storage.cache_bytes) = caches(&usage)?;
    // GitHub lists the newest first.
    let mut pages = Pages::new("artifacts", "artifacts");
    for n in 1..=ARTIFACT_PAGES {
        let fields = [("per_page", PAGE.to_string()), ("page", n.to_string())];
        let answer = crate::github::api(
            &format!("repos/{repo}/actions/artifacts"),
            "GET",
            &fields,
            None,
        )
        .await;
        let Some(page) = seen(&mut storage, answer)? else {
            return Ok(storage);
        };
        if !pages.add(&page)? {
            break;
        }
    }
    (storage.artifacts, storage.artifact_bytes) = kept(&pages.items);
    storage.more_artifacts = !pages.all();
    Ok(storage)
}
/// GitHub's answer, or None when it refused or wouldn't show it, which
/// Orbit keeps as why it can't see the storage; anything else is tried
/// again.
fn seen(storage: &mut Storage, answer: Result<Value, String>) -> Result<Option<Value>, String> {
    match answer {
        Ok(v) => Ok(Some(v)),
        Err(e) if logs::refused(&e) => {
            storage.unseen = Some(unseen(&storage.repo));
            Ok(None)
        }
        Err(e) => Err(e),
    }
}
/// Why Orbit can't see a repository's storage, in Orbit's words.
pub fn unseen(repo: &str) -> String {
    format!(
        "Orbit can't see the Actions storage of {repo}: GitHub doesn't show it to Orbit's sign-in."
    )
}
/// How many caches GitHub's answer says a repository has, and their bytes.
pub fn caches(v: &Value) -> Result<(u32, u64), String> {
    let bytes = v["active_caches_size_in_bytes"]
        .as_u64()
        .ok_or("GitHub didn't say how much the caches hold.")?;
    let count = v["active_caches_count"].as_u64().unwrap_or(0);
    Ok((count.min(u32::MAX as u64) as u32, bytes))
}
/// How many of these artifacts haven't expired, and their bytes.
pub fn kept(artifacts: &[Value]) -> (u32, u64) {
    let kept: Vec<&Value> = artifacts.iter().filter(|a| a["expired"] != true).collect();
    let bytes = kept
        .iter()
        .filter_map(|a| a["size_in_bytes"].as_u64())
        .sum();
    (kept.len().min(u32::MAX as usize) as u32, bytes)
}

/// Bytes in gigabytes, to one place: "9.4 GB".
pub fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / GB as f64)
}
/// What Bit says of a repository's storage when its caches are near the
/// usual limit, starting with a space, or nothing.
pub fn sentence(s: &Storage) -> String {
    if s.unseen.is_some() || s.cache_bytes < NEAR {
        return String::new();
    }
    format!(
        " Its caches hold {}, near GitHub's usual {} GB limit, so GitHub may soon drop the ones used least.",
        gb(s.cache_bytes),
        CACHE_LIMIT / GB
    )
}

/// Records what Orbit read about a repository's storage. After an error it
/// keeps what it read before and tries again after a wait, a minute
/// doubling up to `logs::MAX_RETRY`.
pub fn record(
    read: &mut Vec<Storage>,
    repo: &str,
    result: Result<Storage, String>,
    now: DateTime<Utc>,
) {
    let known = read.iter().position(|s| same_repo(&s.repo, repo));
    let storage = match result {
        Ok(storage) => storage,
        Err(error) => {
            let mut before = known.map(|i| read[i].clone()).unwrap_or_else(|| Storage {
                repo: repo.to_string(),
                ..Default::default()
            });
            before.tries += 1;
            let wait = Duration::minutes(1i64 << (before.tries - 1).min(6)).min(logs::MAX_RETRY);
            before.error = Some(format!(
                "Orbit couldn't read the storage yet: {}. It tries again later.",
                logs::short(&error)
            ));
            before.retry_at = Some(stamp(now + wait));
            before
        }
    };
    match known {
        Some(i) => read[i] = storage,
        None => read.push(storage),
    }
}

/// Forgets repositories that are no longer connected.
pub fn prune(read: &mut Vec<Storage>, connected: &[String]) {
    read.retain(|s| connected.iter().any(|r| same_repo(r, &s.repo)));
}

#[cfg(test)]
#[path = "actions_storage_tests.rs"]
mod tests;
