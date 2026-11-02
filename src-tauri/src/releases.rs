//! Releases: each connected repository's GitHub releases and version tags,
//! read every `EVERY`, a few repositories a pass, while Watch CI is on. A
//! version tag is one named for a version, such as `v1.2.0` or `1.2.0`.
//! Orbit takes the latest release from GitHub, and works out a newer
//! pre-release, the drafts waiting to be published, the version tags with
//! no release, and how many commits the default branch has had since the
//! latest release. GitHub shows drafts only to accounts that can push to
//! the repository, so Orbit says when its sign-in can't. Orbit reads one
//! page of releases, so when GitHub has more, it doesn't say which tags
//! have none. Nothing here reaches GitHub but reads.
use crate::{
    github::PAGE,
    logs,
    problems::{stamp, time},
    types::*,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;

/// Repositories Orbit reads in one pass.
pub const PER_PASS: usize = 4;
/// How often Orbit reads each repository again.
pub const EVERY: Duration = Duration::minutes(30);
/// The most tags Orbit reads, those on the newest commits first.
pub const TAGS: usize = 100;
/// The most drafts, and tags with no release, Orbit keeps to show.
pub const SHOWN: usize = 10;
/// The most version tags Orbit checks against the version files in one
/// read, and the most checks it keeps for a repository.
pub const CHECKS_PER_READ: usize = 5;
pub const CHECKS_KEPT: usize = 20;

/// One of a repository's releases on GitHub.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Release {
    pub id: u64,
    /// The tag it is for, which a draft may not have made yet.
    pub tag: String,
    /// Its title, or its tag when it has none.
    pub name: String,
    pub prerelease: bool,
    pub draft: bool,
    pub created_at: String,
    pub published_at: Option<String>,
    pub url: String,
    /// How many files are attached to it.
    pub assets: u32,
}
/// A tag and the commit it points at.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tag {
    pub name: String,
    pub sha: String,
}
/// What the version files say at a version tag's commit (see `due`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TagCheck {
    pub name: String,
    pub sha: String,
    /// When Orbit read the files there, by Orbit's clock.
    pub at: String,
    /// The versions the manifests there say, each once, in
    /// `crate::versions::FILES` order. A lockfile only copies its
    /// manifest, so it isn't asked.
    pub says: Vec<String>,
    /// The files there Orbit couldn't read or make out.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unread: Vec<String>,
    /// Why GitHub didn't answer, when it didn't. Orbit tries again at the
    /// next read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl TagCheck {
    /// Whether the tag is on the right commit: true when a manifest there
    /// says its version, false when none does and Orbit read them all, and
    /// None when Orbit can't say, as none says a version there, or Orbit
    /// couldn't read one that may.
    pub fn right(&self) -> Option<bool> {
        if self.error.is_some() {
            return None;
        }
        let wanted = version(&self.name)?;
        // A Python project's files may say it as PEP 440 writes it.
        let python = crate::versions::pep440(&self.name);
        if self.says.iter().any(|s| {
            version(s).as_ref() == Some(&wanted)
                || python.is_some() && crate::versions::pep440(s) == python
        }) {
            return Some(true);
        }
        (self.unread.is_empty() && !self.says.is_empty()).then_some(false)
    }
}

/// What Orbit read of one repository's releases and version tags.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RepoReleases {
    pub repo: String,
    /// When Orbit last read it, by Orbit's clock.
    pub read_at: String,
    /// The default branch, and its latest commit.
    pub branch: String,
    pub head: String,
    /// The latest release, as GitHub marks it: usually the newest made that
    /// is neither a draft nor a pre-release.
    pub latest: Option<Release>,
    /// The newest pre-release made after it, or the newest, when there is
    /// none.
    pub prerelease: Option<Release>,
    /// Drafts waiting to be published, the newest first, at most `SHOWN`,
    /// and how many there are.
    pub drafts: Vec<Release>,
    pub draft_count: u32,
    /// How many releases Orbit read that aren't drafts, and whether GitHub
    /// has more than it read.
    pub published: u32,
    pub more_releases: bool,
    /// How many version tags Orbit read, and whether there are more tags
    /// than it read.
    pub version_tags: u32,
    pub more_tags: bool,
    /// Version tags with no release, not even a draft, newer than the
    /// latest release, or the newest pre-release when there is none, or
    /// any, when nothing is published, the highest version first, at most
    /// `SHOWN`, and how many there are. None when GitHub has more releases
    /// than Orbit read, as one of those may name a tag.
    pub unreleased: Vec<Tag>,
    pub unreleased_count: u32,
    /// How many commits the default branch has had since the latest
    /// release's tag, when Orbit could tell.
    pub since: Option<u32>,
    /// Orbit's sign-in can't push to the repository, so GitHub hides its
    /// drafts.
    pub drafts_hidden: bool,
    /// Why Orbit can't see its releases, when GitHub won't show them.
    pub unseen: Option<String>,
    /// Why the last read failed, how many in a row did, and when Orbit
    /// tries again. What was read before is kept meanwhile.
    pub error: Option<String>,
    pub tries: u32,
    pub retry_at: Option<String>,
    /// The version tags Orbit read, the highest version first, so the next
    /// read can tell which are new. Kept with Orbit: the app isn't sent
    /// them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    /// What the version files say at the commits of the version tags Orbit
    /// checked, the highest version first, at most `CHECKS_KEPT`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<TagCheck>,
}

/// A version, as a version tag names it: `1.2.0`, or `v1.2.0`, maybe with
/// a pre-release such as `-beta.2`. Build data after a `+` is left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<String>,
}
/// The version a tag names, or None for any other tag, such as `latest`,
/// `v1.2` or `app@1.2.0`.
pub fn version(tag: &str) -> Option<Version> {
    // Letters, digits and dashes, between the dots of a pre-release or
    // build data.
    let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let v = tag.strip_prefix('v').unwrap_or(tag);
    let v = match v.split_once('+') {
        Some((v, build)) if build.split('.').all(word) => v,
        Some(_) => return None,
        None => v,
    };
    let (core, pre) = match v.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (v, None),
    };
    let numbers: Vec<u64> = core
        .split('.')
        .map(|n| {
            (!n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
                .then(|| n.parse().ok())
                .flatten()
        })
        .collect::<Option<_>>()?;
    let [major, minor, patch] = numbers[..] else {
        return None;
    };
    let pre: Vec<String> = match pre {
        Some(pre) if pre.split('.').all(word) => pre.split('.').map(str::to_string).collect(),
        Some(_) => return None,
        None => vec![],
    };
    Some(Version {
        major,
        minor,
        patch,
        pre,
    })
}
impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let core =
            (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        if core.is_ne() {
            return core;
        }
        // A pre-release comes before the version itself.
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        for (a, b) in self.pre.iter().zip(&other.pre) {
            let order = match (a.parse::<u64>(), b.parse::<u64>()) {
                (Ok(a), Ok(b)) => a.cmp(&b),
                // Numbers come before words.
                (Ok(_), Err(_)) => Ordering::Less,
                (Err(_), Ok(_)) => Ordering::Greater,
                _ => a.cmp(b),
            };
            if order.is_ne() {
                return order;
            }
        }
        self.pre.len().cmp(&other.pre.len())
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The repositories Orbit should read now, at most `PER_PASS`: those never
/// read first, then those read longest ago. One is read again `EVERY`
/// after it was, or after its wait once a read failed.
pub fn wanted(repos: &[String], read: &[RepoReleases], now: DateTime<Utc>) -> Vec<String> {
    let mut due: Vec<(String, String)> = repos
        .iter()
        .filter(|r| validate_repo(r).is_ok())
        .filter_map(|repo| {
            let Some(r) = read.iter().find(|r| same_repo(&r.repo, repo)) else {
                return Some((String::new(), repo.clone()));
            };
            let ready = match &r.error {
                Some(_) => r
                    .retry_at
                    .as_deref()
                    .and_then(time)
                    .is_none_or(|retry| now >= retry),
                None => time(&r.read_at).is_none_or(|at| now - at >= EVERY),
            };
            ready.then(|| (r.read_at.clone(), repo.clone()))
        })
        .collect();
    due.sort_by(|a, b| a.0.cmp(&b.0));
    due.into_iter()
        .take(PER_PASS)
        .map(|(_, repo)| repo)
        .collect()
}

/// What Orbit asks GitHub about a repository's tags: those on the newest
/// commits, each with the commit it points at, through an annotated tag
/// when it is one, the default branch, and whether Orbit's sign-in can
/// push there.
fn query() -> String {
    format!(
        "query($owner: String!, $name: String!) {{
  repository(owner: $owner, name: $name) {{
    viewerPermission
    defaultBranchRef {{ name target {{ oid }} }}
    refs(refPrefix: \"refs/tags/\", first: {TAGS}, orderBy: {{field: TAG_COMMIT_DATE, direction: DESC}}) {{
      totalCount
      nodes {{ name target {{ __typename oid ... on Tag {{ target {{ __typename oid }} }} }} }}
    }}
  }}
}}"
    )
}

/// Reads a repository's releases and tags, and how many commits its
/// default branch has had since the latest release.
pub async fn read(repo: &str) -> Result<RepoReleases, String> {
    validate_repo(repo)?;
    let (owner, name) = repo.split_once('/').ok_or("Invalid repository")?;
    let now = Utc::now();
    // GitHub lists the newest first.
    let listed = crate::github::api(
        &format!("repos/{repo}/releases"),
        "GET",
        &[("per_page", PAGE.to_string())],
        None,
    )
    .await;
    let listed = match listed {
        Ok(v) => v,
        Err(e) if logs::refused(&e) => {
            return Ok(RepoReleases {
                repo: repo.to_string(),
                read_at: stamp(now),
                unseen: Some(unseen(repo)),
                ..Default::default()
            })
        }
        Err(e) => return Err(e),
    };
    // The one GitHub marks latest, which may be off the page Orbit read,
    // or not the newest when someone chose another. None has none.
    let marked = format!("repos/{repo}/releases/latest");
    let latest = match crate::github::api(&marked, "GET", &[], None).await {
        Ok(v) => v,
        Err(e) if e.contains("HTTP 404") => Value::Null,
        Err(e) => return Err(e),
    };
    let refs = crate::github::api(
        "graphql",
        "POST",
        &[
            ("query", query()),
            ("owner", owner.to_string()),
            ("name", name.to_string()),
        ],
        None,
    )
    .await?;
    let (mut read, base) = understand(repo, &listed, &latest, &refs, now)?;
    if let Some(base) = base {
        read.since = crate::github::ahead_by(repo, &base, &read.head).await?;
    }
    Ok(read)
}
/// Why Orbit can't see a repository's releases, in Orbit's words.
pub fn unseen(repo: &str) -> String {
    format!("Orbit can't see the releases of {repo}: GitHub doesn't show them to Orbit's sign-in.")
}

/// What GitHub's list of releases, the release it marks latest (null for
/// none) and its answer to `query` say, and the commit of the latest
/// release's tag, to count the commits since, when Orbit read that tag and
/// the default branch has a commit.
pub fn understand(
    repo: &str,
    listed: &Value,
    latest: &Value,
    refs: &Value,
    now: DateTime<Utc>,
) -> Result<(RepoReleases, Option<String>), String> {
    let list = listed
        .as_array()
        .ok_or("GitHub didn't return the releases.")?;
    let releases: Vec<Release> = list.iter().filter_map(release).collect();
    crate::ci::refused(refs)?;
    let r = &refs["data"]["repository"];
    if r.is_null() {
        return Err("GitHub didn't return the repository.".into());
    }
    let nodes = r["refs"]["nodes"]
        .as_array()
        .ok_or("GitHub didn't return the tags.")?;
    let tags: Vec<Tag> = nodes.iter().filter_map(tag).collect();
    let total = r["refs"]["totalCount"].as_u64().unwrap_or(0);
    let branch = r["defaultBranchRef"]["name"].as_str().unwrap_or_default();
    let head = r["defaultBranchRef"]["target"]["oid"]
        .as_str()
        .filter(|s| valid_sha(s))
        .unwrap_or_default();
    // Only those who can push see drafts.
    let can_push = matches!(
        r["viewerPermission"].as_str(),
        Some("ADMIN" | "MAINTAIN" | "WRITE")
    );

    let made = |r: &Release| time(&r.created_at);
    let newest = |a: &&Release, b: &&Release| made(b).cmp(&made(a));
    let mut published: Vec<&Release> = releases.iter().filter(|r| !r.draft).collect();
    published.sort_by(newest);
    let latest = release(latest).filter(|l| !l.draft && !l.prerelease);
    let prerelease = published
        .iter()
        .filter(|r| r.prerelease)
        .find(|r| latest.as_ref().is_none_or(|l| made(r) > made(l)))
        .map(|r| (*r).clone());
    // GitHub may have more than the page Orbit read.
    let more_releases = list.len() >= PAGE;
    let mut drafts: Vec<Release> = releases.iter().filter(|r| r.draft).cloned().collect();
    drafts.sort_by(|a, b| newest(&a, &b));
    let draft_count = drafts.len() as u32;
    drafts.truncate(SHOWN);

    let mut versioned: Vec<(&Tag, Version)> = tags
        .iter()
        .filter_map(|t| Some((t, version(&t.name)?)))
        .collect();
    // The highest first, and for one version named twice, such as `1.2.0`
    // and `v1.2.0`, by name.
    versioned.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));
    // Newer than what is out: the latest release, or with none, the newest
    // pre-release.
    let out = latest.as_ref().or(prerelease.as_ref());
    let floor = out.and_then(|l| version(&l.tag));
    let mut unreleased: Vec<&(&Tag, Version)> = versioned
        .iter()
        .filter(|_| !more_releases)
        .filter(|(t, _)| !releases.iter().any(|r| r.tag == t.name))
        .filter(|(_, v)| floor.as_ref().is_none_or(|f| v > f))
        .collect();
    unreleased.sort_by(|a, b| b.1.cmp(&a.1));
    let unreleased_count = unreleased.len() as u32;

    let base = latest
        .as_ref()
        .and_then(|l| tags.iter().find(|t| t.name == l.tag))
        .map(|t| t.sha.clone())
        .filter(|_| !head.is_empty());
    let read = RepoReleases {
        repo: repo.to_string(),
        read_at: stamp(now),
        branch: branch.to_string(),
        head: head.to_string(),
        latest,
        prerelease,
        drafts,
        draft_count,
        published: published.len() as u32,
        more_releases,
        version_tags: versioned.len() as u32,
        more_tags: total > nodes.len() as u64,
        unreleased: unreleased
            .into_iter()
            .take(SHOWN)
            .map(|(t, _)| (*t).clone())
            .collect(),
        unreleased_count,
        since: None,
        drafts_hidden: !can_push,
        tags: versioned.iter().map(|(t, _)| (*t).clone()).collect(),
        ..Default::default()
    };
    Ok((read, base))
}

/// The version tags Orbit should check against the version files now, the
/// highest version first, at most `CHECKS_PER_READ`: the newest version
/// tag, each that is new since the read `before` or points at another
/// commit, and each whose check GitHub didn't answer, unless Orbit checked
/// it at that commit already. With no list from before, only the newest
/// counts as new, as Orbit can't tell which others are.
pub fn due(before: Option<&RepoReleases>, now: &RepoReleases) -> Vec<Tag> {
    let listed = before.is_some_and(|b| {
        !b.tags.is_empty() || (b.version_tags == 0 && !b.read_at.is_empty() && b.unseen.is_none())
    });
    let known =
        |t: &Tag| before.is_some_and(|b| b.tags.iter().any(|k| k.name == t.name && k.sha == t.sha));
    let checked = |t: &Tag, failed: bool| {
        now.checks
            .iter()
            .any(|c| c.name == t.name && c.sha == t.sha && c.error.is_some() == failed)
    };
    now.tags
        .iter()
        .enumerate()
        .filter(|(i, t)| *i == 0 || (listed && !known(t)) || checked(t, true))
        .map(|(_, t)| t)
        .filter(|t| !checked(t, false))
        .take(CHECKS_PER_READ)
        .cloned()
        .collect()
}
/// Keeps the checks of the read `before` whose tags still point at the
/// same commits.
pub fn carry(before: Option<&RepoReleases>, now: &mut RepoReleases) {
    let Some(before) = before else {
        return;
    };
    now.checks = before
        .checks
        .iter()
        .filter(|c| now.tags.iter().any(|t| t.name == c.name && t.sha == c.sha))
        .cloned()
        .collect();
}
/// Adds a check, in place of an earlier one of its tag, keeping the
/// highest versions, at most `CHECKS_KEPT`.
pub fn add(now: &mut RepoReleases, check: TagCheck) {
    now.checks.retain(|c| c.name != check.name);
    now.checks.push(check);
    now.checks
        .sort_by_key(|c| std::cmp::Reverse(version(&c.name)));
    now.checks.truncate(CHECKS_KEPT);
}
/// Reads what the version files say at a tag's commit.
pub async fn check(repo: &str, tag: &Tag, now: DateTime<Utc>) -> TagCheck {
    let mut c = TagCheck {
        name: tag.name.clone(),
        sha: tag.sha.clone(),
        at: stamp(now),
        ..Default::default()
    };
    match crate::versions::read_at(repo, &tag.sha).await {
        Ok((files, unread)) => {
            c.says = crate::versions::manifests_say(&files);
            c.unread = unread;
        }
        Err(e) => c.error = Some(e),
    }
    c
}
/// A release from GitHub's list, or None when it isn't whole.
fn release(v: &Value) -> Option<Release> {
    let tag = v["tag_name"].as_str().filter(|t| !t.is_empty())?;
    let name = v["name"]
        .as_str()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(tag);
    Some(Release {
        id: v["id"].as_u64()?,
        tag: tag.to_string(),
        name: name.to_string(),
        prerelease: v["prerelease"] == true,
        draft: v["draft"] == true,
        created_at: v["created_at"].as_str().unwrap_or_default().to_string(),
        published_at: v["published_at"].as_str().map(str::to_string),
        url: v["html_url"]
            .as_str()
            .filter(|u| u.starts_with("https://github.com/"))
            .unwrap_or_default()
            .to_string(),
        assets: v["assets"]
            .as_array()
            .map_or(0, |a| a.len().min(u32::MAX as usize) as u32),
    })
}
/// A tag from GitHub's answer to `query`, with the commit it points at, or
/// None when that isn't a commit, such as a tag of a tag.
fn tag(v: &Value) -> Option<Tag> {
    let name = v["name"].as_str().filter(|n| !n.is_empty())?;
    let target = &v["target"];
    let sha = match target["__typename"].as_str()? {
        "Commit" => target["oid"].as_str()?,
        "Tag" if target["target"]["__typename"] == "Commit" => target["target"]["oid"].as_str()?,
        _ => return None,
    };
    valid_sha(sha).then(|| Tag {
        name: name.to_string(),
        sha: sha.to_string(),
    })
}

/// Records what Orbit read about a repository's releases. After an error
/// it keeps what it read before and tries again after a wait, a minute
/// doubling up to `logs::MAX_RETRY`.
pub fn record(
    read: &mut Vec<RepoReleases>,
    repo: &str,
    result: Result<RepoReleases, String>,
    now: DateTime<Utc>,
) {
    let known = read.iter().position(|r| same_repo(&r.repo, repo));
    let releases = match result {
        Ok(releases) => releases,
        Err(error) => {
            let mut before = known
                .map(|i| read[i].clone())
                .unwrap_or_else(|| RepoReleases {
                    repo: repo.to_string(),
                    ..Default::default()
                });
            before.tries += 1;
            let wait = Duration::minutes(1i64 << (before.tries - 1).min(6)).min(logs::MAX_RETRY);
            before.error = Some(format!(
                "Orbit couldn't read the releases yet: {}. It tries again later.",
                logs::short(&error)
            ));
            before.retry_at = Some(stamp(now + wait));
            before
        }
    };
    match known {
        Some(i) => read[i] = releases,
        None => read.push(releases),
    }
}

/// Forgets repositories that are no longer connected.
pub fn prune(read: &mut Vec<RepoReleases>, connected: &[String]) {
    read.retain(|r| connected.iter().any(|c| same_repo(c, &r.repo)));
}

/// What Bit says of a repository's releases, starting with a space, or
/// nothing when Orbit hasn't read them.
pub fn sentence(r: &RepoReleases) -> String {
    if r.unseen.is_some() || r.read_at.is_empty() {
        return String::new();
    }
    let mut text = match &r.latest {
        Some(l) => {
            let since = match r.since {
                Some(0) => format!(", and {} has had no commits since", r.branch),
                Some(1) => format!(", and {} has had 1 commit since", r.branch),
                Some(n) => format!(", and {} has had {n} commits since", r.branch),
                None => String::new(),
            };
            format!(" Its latest release is {}{since}.", l.tag)
        }
        None if r.published > 0 => " It has no full release yet, only pre-releases.".to_string(),
        None if r.draft_count > 0 => " It has nothing published yet.".to_string(),
        None if r.unreleased_count > 0 => format!(
            " It has no releases on GitHub; its newest version tag is {}.",
            r.unreleased[0].name
        ),
        None if r.version_tags > 0 => " It has no releases on GitHub yet.".to_string(),
        None => " It has no releases or version tags yet.".to_string(),
    };
    match (&r.prerelease, &r.latest) {
        (Some(p), Some(_)) => text.push_str(&format!(" A newer pre-release, {}, is out.", p.tag)),
        (Some(p), None) => text.push_str(&format!(" Its newest pre-release is {}.", p.tag)),
        _ => {}
    }
    match r.draft_count {
        0 => {}
        1 => text.push_str(&format!(
            " A draft, {}, waits to be published.",
            r.drafts[0].tag
        )),
        n => text.push_str(&format!(" {n} drafts wait to be published.")),
    }
    // With nothing out, tags without a release are how work goes.
    if r.latest.is_some() || r.published > 0 {
        match r.unreleased_count {
            0 => {}
            1 => text.push_str(&format!(
                " Its tag {} has no release.",
                r.unreleased[0].name
            )),
            n => text.push_str(&format!(" {n} newer version tags have no release.")),
        }
    }
    text
}

#[cfg(test)]
#[path = "releases_tests.rs"]
mod tests;
