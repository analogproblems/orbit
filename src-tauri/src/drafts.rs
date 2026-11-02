//! Publishing a draft release: once a repository's newest version tag has
//! a draft release, and every run the tag started passed (`crate::builds`),
//! Orbit offers to publish the draft on GitHub with your sign-in
//! (`waiting`). The rule `publish_release` (**Publish a draft release**)
//! decides whether Bit asks first, does it, or leaves it to you. Each
//! publish is recorded in the journal and saved before it is sent, after
//! GitHub says just then that the release is still a draft for that tag,
//! as its button showed it, that the tag still points at the commit its
//! build passed at, and that the runs the tag started still pass. One
//! GitHub may or may not have taken is looked up on GitHub before anything
//! else (`unsettled`, `found`), so a release is never published twice.
//! Orbit only turns the draft into a release: it never changes its text,
//! files or tag, never makes a tag this way, and never deletes a release.
use crate::{
    logs,
    ops::{Action, Write, WriteState},
    problems::stamp,
    releases::{Release, RepoReleases},
    types::*,
};
use chrono::{DateTime, Utc};

/// A draft release for a repository's newest version tag, whose build
/// passed, ready to publish.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    pub repo: String,
    /// The tag, and the commit it points at, where its build passed.
    pub tag: String,
    pub sha: String,
    /// The draft's id on GitHub, and its title.
    pub id: u64,
    pub name: String,
    pub prerelease: bool,
}

/// The Needs you item's key for a draft ready to publish:
/// `release:<repo>:<tag>:<id>:<look>`, with the draft's id on GitHub and
/// `look`, a short hash of its title and whether it is a pre-release, so a
/// tap on a button for a draft that was replaced, or changed, since Orbit
/// showed it finds no item, and publishes nothing.
pub fn key(d: &Draft) -> String {
    use sha2::{Digest, Sha256};
    let look = Sha256::digest(format!("{}\n{}", d.prerelease, d.name).as_bytes());
    let look: String = look[..4].iter().map(|b| format!("{b:02x}")).collect();
    format!("release:{}:{}:{}:{look}", d.repo, d.tag, d.id)
}
/// The repository and tag a Needs you item's key names, for a draft ready
/// to publish.
pub fn from_key(key: &str) -> Option<(&str, &str)> {
    let mut parts = key.strip_prefix("release:")?.split(':');
    let (repo, tag) = (parts.next()?, parts.next()?);
    let id = parts.next()?;
    (parts.next().is_some() && parts.next().is_none() && id.parse::<u64>().is_ok())
        .then_some((repo, tag))
}
/// What the Needs you item says.
pub fn reason(d: &Draft) -> String {
    let kind = if d.prerelease {
        "pre-release"
    } else {
        "release"
    };
    let title = if d.name == d.tag {
        String::new()
    } else {
        format!(" ({})", d.name)
    };
    format!(
        "{} has a draft {kind} for {}{title}, and every run the tag started passed at {}.",
        d.repo,
        d.tag,
        crate::ci::short(&d.sha)
    )
}

/// The drafts ready to publish, one at most for each repository Orbit
/// watches (`of`).
pub fn waiting(s: &Snapshot) -> Vec<Draft> {
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

/// The draft a repository is ready to publish, if any:
///
/// - Orbit read its releases, and its sign-in can push there, so GitHub
///   shows it the drafts;
/// - Orbit read every draft, and exactly one names the tag whose build
///   Orbit watches, its newest version tag (`crate::builds::target`);
/// - no release Orbit read for that tag is out already;
/// - Orbit's latest read of the runs the tag started, at the commit it
///   points at, worked and says each passed, or was skipped by its own
///   workflow, with none cancelled (`crate::builds::publishable`);
/// - no publish of that draft is on its way or done.
pub fn of(s: &Snapshot, r: &RepoReleases) -> Option<Draft> {
    if r.read_at.is_empty() || r.unseen.is_some() || r.drafts_hidden {
        return None;
    }
    // A draft Orbit didn't read may name the tag too.
    if r.drafts.len() != r.draft_count as usize {
        return None;
    }
    let known = s.builds.iter().find(|b| same_repo(&b.repo, &r.repo));
    let tag = crate::builds::target(r, known)?;
    let out = [&r.latest, &r.prerelease]
        .into_iter()
        .flatten()
        .any(|l| l.tag == tag.name);
    if out {
        return None;
    }
    let [draft] = &r
        .drafts
        .iter()
        .filter(|d| d.tag == tag.name)
        .collect::<Vec<_>>()[..]
    else {
        return None;
    };
    let passed = s.builds.iter().any(|b| {
        same_repo(&b.repo, &r.repo)
            && b.tag == tag.name
            && b.sha == tag.sha
            && b.ci.state == crate::ci::CiState::Passing
            && crate::builds::publishable(b).is_ok()
    });
    if !passed || publishing(&s.journal, &r.repo, draft.id) {
        return None;
    }
    Some(Draft {
        repo: r.repo.clone(),
        tag: tag.name.clone(),
        sha: tag.sha.clone(),
        id: draft.id,
        name: draft.name.clone(),
        prerelease: draft.prerelease,
    })
}

/// Whether a publish of the draft `id` in `repo` is on its way to GitHub,
/// or done.
pub fn publishing(journal: &[Write], repo: &str, id: u64) -> bool {
    journal.iter().any(|w| {
        w.action == Action::PublishRelease
            && w.state != WriteState::Failed
            && same_repo(&w.repo, repo)
            && w.release == Some(id)
    })
}

/// The draft you tapped, or Bit took on, while it is still ready: the one
/// the item's `key` shows, if its repository is ready to publish just that
/// one now, the same release, title and pre-release mark. Kept as it is
/// then, commit and all, so a draft that takes its place while Orbit waits
/// for its turn isn't published instead.
pub fn offered(s: &Snapshot, key_shown: &str) -> Result<Draft, String> {
    let (repo, tag) = from_key(key_shown).ok_or("This isn't about a draft release.")?;
    match waiting(s).into_iter().find(|d| same_repo(&d.repo, repo)) {
        Some(d) if key(&d) == key_shown => Ok(d),
        Some(d) if d.tag == tag => Err(format!(
            "The draft for {tag} changed since Orbit showed it, so Orbit didn't publish it. Look at it again."
        )),
        Some(d) => Err(format!(
            "{repo} has a draft for {} ready now, not {tag}, so Orbit didn't publish {tag}.",
            d.tag
        )),
        None => Err(format!(
            "{repo} has no draft for {tag} ready any more, so Orbit didn't publish it."
        )),
    }
}

/// The draft `repo` is ready to publish, for your tap or Bit's, or why
/// there is none in a sentence.
pub fn may_publish(s: &Snapshot, repo: &str) -> Result<Draft, String> {
    if let Some(w) = s.journal.iter().find(|w| {
        w.action == Action::PublishRelease
            && w.state == WriteState::Sending
            && same_repo(&w.repo, repo)
    }) {
        return Err(match &w.error {
            Some(_) => "Orbit doesn't know yet whether GitHub published its last release here, so it won't publish one until it has looked.".into(),
            None => "Orbit is already publishing a release here.".into(),
        });
    }
    waiting(s)
        .into_iter()
        .find(|d| same_repo(&d.repo, repo))
        .ok_or_else(|| {
            format!("{repo} has no draft ready to publish now, so Orbit won't publish one.")
        })
}

/// Records a publish before it is sent.
pub fn record(journal: &mut Vec<Write>, d: &Draft, now: DateTime<Utc>) -> String {
    let id = crate::types::id();
    journal.push(Write {
        id: id.clone(),
        thread: format!("release:{}:{}", d.repo.to_lowercase(), d.tag),
        action: Action::PublishRelease,
        repo: d.repo.clone(),
        number: None,
        state: WriteState::Sending,
        at: stamp(now),
        done_at: None,
        url: String::new(),
        error: None,
        setting: None,
        head: Some(d.sha.clone()),
        tag: Some(d.tag.clone()),
        release: Some(d.id),
    });
    id
}

/// Records what GitHub said to a publish, from the release it sent back, or
/// the raw error: done, or, when GitHub refused it, why it didn't go. Any
/// other failure may have published it, so the publish stays unsettled,
/// with why, until Orbit looks the release up (`unsettled`), and none is
/// sent meanwhile.
pub fn settle(
    journal: &mut [Write],
    id: &str,
    result: Result<Release, String>,
    now: DateTime<Utc>,
) {
    let Some(w) = journal.iter_mut().find(|w| w.id == id) else {
        return;
    };
    let tag = w.tag.clone().unwrap_or_default();
    match result {
        Ok(release) => {
            w.state = WriteState::Done;
            w.done_at = Some(stamp(now));
            w.url = release.url;
            w.error = None;
        }
        Err(error) => {
            if crate::dependabot::refused(&error) {
                w.state = WriteState::Failed;
                w.done_at = Some(stamp(now));
            }
            w.error = Some(publish_error(&error, &tag));
        }
    }
}

/// What became of a publish that failed, from its raw error, in a
/// sentence.
pub fn publish_error(error: &str, tag: &str) -> String {
    if !crate::dependabot::refused(error) {
        format!(
            "GitHub didn't answer clearly ({}), so Orbit looks on GitHub at the draft for {tag} before it publishes it again.",
            logs::short(error)
        )
    } else if logs::rate_limited(error) {
        "GitHub's rate limit held this back. Tap it again later.".into()
    } else if error.contains("HTTP 422") {
        format!(
            "GitHub refused to publish the draft for {tag}, such as when a release for it is out already: {}.",
            logs::short(error)
        )
    } else if error.contains("HTTP 403") || error.contains("HTTP 404") {
        format!(
            "GitHub didn't let Orbit's sign-in publish the draft for {tag}: {}.",
            logs::short(error)
        )
    } else {
        format!(
            "GitHub refused to publish the draft for {tag}: {}.",
            logs::short(error)
        )
    }
}

/// Publishes GitHub may or may not have taken: still marked as sending.
/// Run only while none is being sent, so each was cut off by Orbit stopping
/// or GitHub's answer was unclear. Returns each one's id, repository and
/// release id.
pub fn unsettled(journal: &[Write]) -> Vec<(String, String, u64)> {
    journal
        .iter()
        .filter(|w| w.action == Action::PublishRelease && w.state == WriteState::Sending)
        .filter_map(|w| Some((w.id.clone(), w.repo.clone(), w.release?)))
        .collect()
}

/// Records what Orbit found on GitHub for a publish that may or may not
/// have gone: the release as it is now, or None when it is gone. Out, it
/// went; still a draft, it didn't, so it may be offered again; gone,
/// someone deleted it. A failed look keeps it unsettled. A release Orbit
/// found out is kept with the repository's releases, so it isn't offered
/// again before the next read.
pub fn found(
    journal: &mut [Write],
    releases: &mut [RepoReleases],
    id: &str,
    result: Result<Option<Release>, String>,
    now: DateTime<Utc>,
) {
    let Some(w) = journal.iter_mut().find(|w| w.id == id) else {
        return;
    };
    let tag = w.tag.clone().unwrap_or_default();
    let release = match result {
        Err(error) => {
            w.error = Some(format!(
                "Orbit couldn't look on GitHub at the draft for {tag} yet ({}), so it won't publish it until it has.",
                logs::short(&error)
            ));
            return;
        }
        Ok(release) => release,
    };
    w.done_at = Some(stamp(now));
    match &release {
        None => {
            w.state = WriteState::Failed;
            w.error = Some(format!("The draft for {tag} is gone from GitHub."));
        }
        Some(r) if r.draft => {
            w.state = WriteState::Failed;
            w.error = Some(format!("The draft for {tag} didn't get published."));
        }
        Some(r) => {
            w.state = WriteState::Done;
            w.url = r.url.clone();
            w.error = None;
        }
    }
    if let (Some(release), Some(r)) = (
        release.filter(|r| !r.draft),
        releases.iter_mut().find(|r| same_repo(&r.repo, &w.repo)),
    ) {
        published(r, release);
    }
}

/// Keeps a draft as GitHub says it is now, in place of what Orbit read of
/// it, so what is ready to publish is worked out from it until the next
/// read: a new title or pre-release mark gives a new item and button.
pub fn changed(r: &mut RepoReleases, release: Release) {
    if let Some(d) = r.drafts.iter_mut().find(|d| d.id == release.id) {
        *d = release;
    }
}

/// Keeps a release Orbit published, or found published, with the
/// repository's releases until the next read: it is no longer a draft,
/// and, as GitHub marks a newly published release that isn't a
/// pre-release, the latest. Its version's milestones are done with.
pub fn published(r: &mut RepoReleases, release: Release) {
    crate::milestones::out(r, &release.tag);
    let before = r.drafts.len();
    r.drafts.retain(|d| d.id != release.id);
    if r.drafts.len() < before {
        r.draft_count = r.draft_count.saturating_sub(1);
        r.published += 1;
    }
    if release.prerelease {
        r.prerelease = Some(release);
    } else {
        r.since = None;
        r.latest = Some(release);
    }
}

/// The release on GitHub, or None when there's no such release.
pub async fn look_up(repo: &str, id: u64) -> Result<Option<Release>, String> {
    match crate::github::api(&format!("repos/{repo}/releases/{id}"), "GET", &[], None).await {
        Ok(v) => crate::releases::release(&v)
            .map(Some)
            .ok_or_else(|| "GitHub's answer had no release".into()),
        Err(e) if e.contains("HTTP 404") => Ok(None),
        Err(e) => Err(e),
    }
}

/// Publishes the draft `id`, changing nothing else about it, and returns
/// the release GitHub sends back.
pub async fn publish(repo: &str, id: u64) -> Result<Release, String> {
    let v = crate::github::api(
        &format!("repos/{repo}/releases/{id}"),
        "PATCH",
        &[],
        Some(serde_json::json!({ "draft": false })),
    )
    .await?;
    crate::releases::release(&v).ok_or_else(|| "GitHub's answer had no release".into())
}

#[cfg(test)]
#[path = "drafts_tests.rs"]
mod tests;
