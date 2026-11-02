//! Pushing a release tag: once a repository's default branch says a
//! version that has no tag yet, at a commit where CI passed, Orbit offers to
//! make the tag there on GitHub with your sign-in, named the way the
//! repository names its tags (`waiting`). Only in a repository that already
//! has version tags, or a workflow that runs on tags, so Orbit never starts
//! tagging one that doesn't release that way. The rule `push_tag` (**Push a
//! release tag**) decides whether Bit asks first, does it, or leaves it to
//! you. Each push is recorded in the journal and saved before it is sent,
//! after GitHub says just then that the tag is still missing and the branch
//! is still at that commit. One GitHub may or may not have taken is looked
//! for on GitHub before anything else (`unsettled`, `found`), so a tag is
//! never made twice. Orbit never moves or deletes a tag.
use crate::{
    logs,
    ops::{Action, Write, WriteState},
    problems::stamp,
    releases::{version, RepoReleases, Tag},
    types::*,
    workflows::Setup,
};
use chrono::{DateTime, Utc};

/// A version the default branch says, with no tag yet, ready to tag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Untagged {
    pub repo: String,
    pub branch: String,
    /// The default branch's latest commit, where the files say `version`
    /// and CI passed.
    pub sha: String,
    pub version: String,
    /// The tag to push, such as `v0.3.0`.
    pub tag: String,
}

/// The Needs you item's key for a version waiting for its tag:
/// `push:<repo>:<tag>`.
pub fn key(u: &Untagged) -> String {
    format!("push:{}:{}", u.repo, u.tag)
}
/// The repository and tag a Needs you item's key names, for a version
/// waiting for its tag.
pub fn from_key(key: &str) -> Option<(&str, &str)> {
    key.strip_prefix("push:")?.rsplit_once(':')
}
/// What the Needs you item says.
pub fn reason(u: &Untagged) -> String {
    format!(
        "{} says {} on {} at {}, where CI passed, and there's no tag {} yet.",
        u.repo,
        u.version,
        u.branch,
        crate::ci::short(&u.sha),
        u.tag
    )
}

/// The tag for `version`, named the way the repository names its version
/// tags: as its highest one does, with or without a `v`, or with none yet,
/// `v` and the version, else the version alone. Where workflows run on
/// tags, the tag must start one of them, as GitHub reads their filters
/// (`Workflow::runs_on_tag`), so a tag never misses the release workflow.
/// None when no name fits.
pub fn name(r: &RepoReleases, setup: &Setup, version: &str) -> Option<String> {
    let release: Vec<&crate::workflows::Workflow> =
        setup.workflows.iter().filter(|w| w.on_tags()).collect();
    let starts = |tag: &String| release.is_empty() || release.iter().any(|w| w.runs_on_tag(tag));
    let names = match r.tags.first() {
        Some(t) if t.name.starts_with('v') => vec![format!("v{version}")],
        Some(_) => vec![version.to_string()],
        None => vec![format!("v{version}"), version.to_string()],
    };
    names.into_iter().find(starts)
}

/// The versions waiting for a tag, one at most for each repository Orbit
/// watches (`of`).
pub fn waiting(s: &Snapshot) -> Vec<Untagged> {
    if !s.settings.watch_ci {
        return vec![];
    }
    s.releases
        .iter()
        .filter(|r| {
            s.settings
                .repositories
                .iter()
                .any(|c| same_repo(&c.name, &r.repo))
        })
        .filter_map(|r| of(s, r))
        .collect()
}

/// The version a repository's default branch says with no tag yet, if it
/// is ready to tag:
///
/// - the repository has version tags, or a workflow that runs on tags, and
///   Orbit's sign-in can push there;
/// - Orbit read every version tag (none beyond the newest it reads), and
///   the version is higher than each;
/// - Orbit read the version files and the tags workflows run on at the
///   branch's latest commit, every manifest there says the same version,
///   and no version files disagree;
/// - CI passed at that commit, as Orbit's latest read of it says;
/// - a name fits (`name`);
/// - no push of that tag is on its way or done.
pub fn of(s: &Snapshot, r: &RepoReleases) -> Option<Untagged> {
    if r.read_at.is_empty() || r.unseen.is_some() || r.drafts_hidden {
        return None;
    }
    // A tag Orbit didn't read may be the version, or a higher one.
    if r.more_tags || r.tags.len() != r.version_tags as usize {
        return None;
    }
    let setup = s.setups.iter().find(|x| same_repo(&x.repo, &r.repo))?;
    let read = setup.error.is_none()
        && !setup.sha.is_empty()
        && setup.checks >= crate::workflows::TAGS
        && setup.unread_versions.is_empty();
    if !read {
        return None;
    }
    let ci = s.ci.iter().find(|c| same_repo(&c.repo, &r.repo))?;
    // What passed before a read that failed may not hold now.
    let passed = ci.error.is_none() && ci.head.state == crate::ci::CiState::Passing;
    if ci.head.sha != setup.sha || !passed {
        return None;
    }
    let disagree = s
        .problems
        .iter()
        .any(|p| p.is_open() && p.key == crate::problems::versions_key(&r.repo));
    if disagree {
        return None;
    }
    let [said] = &crate::versions::manifests_say(&setup.versions)[..] else {
        return None;
    };
    let wanted = version(said)?;
    let tagged = setup.workflows.iter().any(|w| w.on_tags());
    if r.tags.is_empty() && !tagged {
        return None;
    }
    if r.tags
        .iter()
        .any(|t| version(&t.name).is_none_or(|v| v >= wanted))
    {
        return None;
    }
    let tag = name(r, setup, said)?;
    if pushing(&s.journal, &r.repo, &tag) {
        return None;
    }
    Some(Untagged {
        repo: r.repo.clone(),
        branch: setup.branch.clone(),
        sha: setup.sha.clone(),
        version: said.clone(),
        tag,
    })
}

/// Whether a push of `tag` to `repo` is on its way to GitHub, or done.
pub fn pushing(journal: &[Write], repo: &str, tag: &str) -> bool {
    journal.iter().any(|w| {
        w.action == Action::PushTag
            && w.state != WriteState::Failed
            && same_repo(&w.repo, repo)
            && w.tag.as_deref() == Some(tag)
    })
}

/// The tag you tapped, or Bit took on, while it still waits: the version
/// `repo` waits to tag now, if it is `tag`. Kept as it is then, commit and
/// all, so a version that takes its place while Orbit waits for its turn
/// isn't pushed instead.
pub fn offered(s: &Snapshot, repo: &str, tag: &str) -> Result<Untagged, String> {
    match waiting(s).into_iter().find(|u| same_repo(&u.repo, repo)) {
        Some(u) if u.tag == tag => Ok(u),
        Some(u) => Err(format!(
            "{repo} waits for {} now, not {tag}, so Orbit didn't push {tag}.",
            u.tag
        )),
        None => Err(format!(
            "{repo} doesn't wait for {tag} any more, so Orbit didn't push it."
        )),
    }
}
/// The names GitHub may have the version under: the tag, and the same
/// version with or without its `v`.
pub fn spellings(u: &Untagged) -> [String; 2] {
    let other = match u.tag.strip_prefix('v') {
        Some(bare) => bare.to_string(),
        None => format!("v{}", u.tag),
    };
    [u.tag.clone(), other]
}

/// The version `repo` waits to tag, for your tap or Bit's, or why there is
/// none in a sentence.
pub fn may_push(s: &Snapshot, repo: &str) -> Result<Untagged, String> {
    if let Some(w) = s.journal.iter().find(|w| {
        w.action == Action::PushTag && w.state == WriteState::Sending && same_repo(&w.repo, repo)
    }) {
        return Err(match &w.error {
            Some(_) => "Orbit doesn't know yet whether GitHub took its last tag here, so it won't push one until it has looked.".into(),
            None => "Orbit is already pushing a tag here.".into(),
        });
    }
    waiting(s)
        .into_iter()
        .find(|u| same_repo(&u.repo, repo))
        .ok_or_else(|| {
            format!("{repo} has no version waiting for a tag now, so Orbit won't push one.")
        })
}

/// Records a push before it is sent.
pub fn record(journal: &mut Vec<Write>, u: &Untagged, now: DateTime<Utc>) -> String {
    let id = crate::types::id();
    journal.push(Write {
        id: id.clone(),
        thread: format!("tag:{}:{}", u.repo.to_lowercase(), u.tag),
        action: Action::PushTag,
        repo: u.repo.clone(),
        number: None,
        state: WriteState::Sending,
        at: stamp(now),
        done_at: None,
        url: String::new(),
        error: None,
        setting: None,
        head: Some(u.sha.clone()),
        tag: Some(u.tag.clone()),
    });
    id
}

/// The tag's page on GitHub.
pub fn page(repo: &str, tag: &str) -> String {
    format!("https://github.com/{repo}/releases/tag/{tag}")
}

/// Records what GitHub said to a push, from the raw `result` of making the
/// tag: done, or, when GitHub refused it, why it didn't go. Any other
/// failure may have made it, so the push stays unsettled, with why, until
/// Orbit looks for the tag (`unsettled`), and none is sent meanwhile.
pub fn settle(journal: &mut [Write], id: &str, result: Result<(), String>, now: DateTime<Utc>) {
    let Some(w) = journal.iter_mut().find(|w| w.id == id) else {
        return;
    };
    let tag = w.tag.clone().unwrap_or_default();
    match result {
        Ok(()) => {
            w.state = WriteState::Done;
            w.done_at = Some(stamp(now));
            w.url = page(&w.repo, &tag);
            w.error = None;
        }
        Err(error) => {
            if crate::dependabot::refused(&error) {
                w.state = WriteState::Failed;
                w.done_at = Some(stamp(now));
            }
            w.error = Some(push_error(&error, &tag));
        }
    }
}

/// What became of a push that failed, from its raw error, in a sentence.
pub fn push_error(error: &str, tag: &str) -> String {
    if !crate::dependabot::refused(error) {
        format!(
            "GitHub didn't answer clearly ({}), so Orbit looks on GitHub for {tag} before it pushes it again.",
            logs::short(error)
        )
    } else if logs::rate_limited(error) {
        "GitHub's rate limit held this back. Tap it again later.".into()
    } else if error.contains("HTTP 422") {
        format!("GitHub has {tag} already, so Orbit didn't push it.")
    } else if error.contains("HTTP 403") || error.contains("HTTP 404") {
        format!(
            "GitHub didn't let Orbit's sign-in push {tag}: {}.",
            logs::short(error)
        )
    } else {
        format!("GitHub refused {tag}: {}.", logs::short(error))
    }
}

/// Pushes GitHub may or may not have taken: still marked as sending. Run
/// only while none is being sent, so each was cut off by Orbit stopping or
/// GitHub's answer was unclear. Returns each one's id, repository, tag and
/// commit.
pub fn unsettled(journal: &[Write]) -> Vec<(String, String, String, String)> {
    journal
        .iter()
        .filter(|w| w.action == Action::PushTag && w.state == WriteState::Sending)
        .filter_map(|w| {
            Some((
                w.id.clone(),
                w.repo.clone(),
                w.tag.clone()?,
                w.head.clone()?,
            ))
        })
        .collect()
}

/// Records what Orbit found on GitHub for a push that may or may not have
/// gone: the commit the tag points at now, or None when there's no tag.
/// At the push's commit, it went. Anywhere else, someone else made it, and
/// Orbit leaves it. None means it didn't go, so it may be offered again.
/// A failed look keeps it unsettled. A tag Orbit found is kept with the
/// repository's tags, so it isn't offered again before the next read.
pub fn found(
    journal: &mut [Write],
    releases: &mut [RepoReleases],
    id: &str,
    result: Result<Option<String>, String>,
    now: DateTime<Utc>,
) {
    let Some(w) = journal.iter_mut().find(|w| w.id == id) else {
        return;
    };
    let (tag, sha) = (
        w.tag.clone().unwrap_or_default(),
        w.head.clone().unwrap_or_default(),
    );
    let at = match result {
        Err(error) => {
            w.error = Some(format!(
                "Orbit couldn't look on GitHub for {tag} yet ({}), so it won't push it until it has.",
                logs::short(&error)
            ));
            return;
        }
        Ok(at) => at,
    };
    w.done_at = Some(stamp(now));
    match at.as_deref() {
        None => {
            w.state = WriteState::Failed;
            w.error = Some(format!("{tag} didn't get to GitHub."));
        }
        Some(at) if at == sha => {
            w.state = WriteState::Done;
            w.url = page(&w.repo, &tag);
            w.error = None;
        }
        Some(at) => {
            w.state = WriteState::Failed;
            w.error = Some(format!(
                "Someone else pushed {tag}, at {}, so Orbit left it.",
                crate::ci::short(at)
            ));
        }
    }
    if let (Some(at), Some(r)) = (
        at,
        releases.iter_mut().find(|r| same_repo(&r.repo, &w.repo)),
    ) {
        seen(r, &tag, &at);
    }
}

/// Keeps a tag Orbit found on GitHub with the repository's version tags.
pub fn seen(r: &mut RepoReleases, tag: &str, sha: &str) {
    if r.tags.iter().any(|t| t.name == tag) || version(tag).is_none() {
        return;
    }
    r.tags.push(Tag {
        name: tag.to_string(),
        sha: sha.to_string(),
    });
    r.version_tags += 1;
    r.tags.sort_by_key(|t| std::cmp::Reverse(version(&t.name)));
}

/// The commit a tag points at on GitHub, through an annotated tag when it
/// is one, or None when there's no such tag.
pub async fn tag_sha(repo: &str, tag: &str) -> Result<Option<String>, String> {
    let read = crate::github::api(
        &format!("repos/{repo}/git/ref/tags/{tag}"),
        "GET",
        &[],
        None,
    )
    .await;
    let v = match read {
        Ok(v) => v,
        Err(e) if e.contains("HTTP 404") => return Ok(None),
        Err(e) => return Err(e),
    };
    let object = &v["object"];
    let sha = object["sha"]
        .as_str()
        .ok_or("GitHub's answer had no commit")?;
    if object["type"].as_str() != Some("tag") {
        return Ok(Some(sha.to_string()));
    }
    let annotated =
        crate::github::api(&format!("repos/{repo}/git/tags/{sha}"), "GET", &[], None).await?;
    annotated["object"]["sha"]
        .as_str()
        .map(|s| Some(s.to_string()))
        .ok_or_else(|| "GitHub's answer had no commit".into())
}

/// The commit a branch is at on GitHub now.
pub async fn branch_sha(repo: &str, branch: &str) -> Result<String, String> {
    let v = crate::github::api(
        &format!("repos/{repo}/git/ref/heads/{branch}"),
        "GET",
        &[],
        None,
    )
    .await?;
    v["object"]["sha"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "GitHub's answer had no commit".into())
}

/// Makes the tag on GitHub, pointing at `sha`.
pub async fn push(repo: &str, tag: &str, sha: &str) -> Result<(), String> {
    crate::github::api(
        &format!("repos/{repo}/git/refs"),
        "POST",
        &[],
        Some(serde_json::json!({ "ref": format!("refs/tags/{tag}"), "sha": sha })),
    )
    .await
    .map(|_| ())
}

#[cfg(test)]
#[path = "tagging_tests.rs"]
mod tests;
