use super::*;
use crate::{
    builds::Build,
    ci::{CiState, CommitCi},
    releases::Tag,
};

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";

fn now() -> DateTime<Utc> {
    "2026-11-02T12:00:00Z".parse().unwrap()
}
fn release(id: u64, tag: &str, draft: bool) -> Release {
    Release {
        id,
        tag: tag.into(),
        name: tag.into(),
        draft,
        created_at: "2026-11-02T10:00:00Z".into(),
        url: format!("https://github.com/a/b/releases/tag/{tag}"),
        ..Default::default()
    }
}
/// `a/b`, whose newest version tag `v0.3.0` at `A` has a draft, titled
/// "Orbit 0.3.0", and a build that passed; `v0.2.0` is out.
fn repo() -> Snapshot {
    let mut s = Snapshot::default();
    s.settings.watch_ci = true;
    s.settings.repositories.push(Repository {
        name: "a/b".into(),
        path: "/tmp/a-b".into(),
        automatic: true,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes: 0,
        keep_updated: false,
        provider: Default::default(),
        dev_container: Default::default(),
    });
    let draft = Release {
        name: "Orbit 0.3.0".into(),
        ..release(7, "v0.3.0", true)
    };
    s.releases.push(RepoReleases {
        repo: "a/b".into(),
        read_at: "2026-11-02T11:00:00.000Z".into(),
        branch: "main".into(),
        head: A.into(),
        latest: Some(release(5, "v0.2.0", false)),
        drafts: vec![draft],
        draft_count: 1,
        published: 1,
        tags: vec![
            Tag {
                name: "v0.3.0".into(),
                sha: A.into(),
            },
            Tag {
                name: "v0.2.0".into(),
                sha: B.into(),
            },
        ],
        version_tags: 2,
        ..Default::default()
    });
    s.builds.push(Build {
        repo: "A/B".into(),
        tag: "v0.3.0".into(),
        sha: A.into(),
        ci: CommitCi {
            sha: A.into(),
            state: CiState::Passing,
            passing: 2,
            ..Default::default()
        },
        read_at: "2026-11-02T11:00:00.000Z".into(),
        ..Default::default()
    });
    s
}
fn ready(s: &Snapshot) -> Vec<(String, String, String, u64)> {
    waiting(s)
        .into_iter()
        .map(|d| (d.repo, d.tag, d.sha, d.id))
        .collect()
}
fn the_draft() -> Vec<(String, String, String, u64)> {
    vec![("a/b".into(), "v0.3.0".into(), A.into(), 7)]
}

#[test]
fn a_draft_for_the_newest_tag_whose_build_passed_is_ready() {
    let s = repo();
    assert_eq!(ready(&s), the_draft());
    let d = &waiting(&s)[0];
    assert_eq!(key(d), "release:a/b:v0.3.0");
    assert_eq!(from_key(&key(d)), Some(("a/b", "v0.3.0")));
    assert_eq!(from_key("push:a/b:v0.3.0"), None);
    assert_eq!(
        reason(d),
        "a/b has a draft release for v0.3.0 (Orbit 0.3.0), and every run the tag started passed at 1111111."
    );
    // Untitled, and a pre-release.
    let mut s = repo();
    s.releases[0].drafts[0].name = "v0.3.0".into();
    s.releases[0].drafts[0].prerelease = true;
    assert_eq!(
        reason(&waiting(&s)[0]),
        "a/b has a draft pre-release for v0.3.0, and every run the tag started passed at 1111111."
    );
    // Other drafts, for older tags, don't count.
    let mut s = repo();
    s.releases[0].drafts.push(release(3, "v0.1.0", true));
    s.releases[0].draft_count = 2;
    assert_eq!(ready(&s), the_draft());
}

/// A change to what Orbit read, for a case that keeps a draft from being
/// ready.
type Change = Box<dyn Fn(&mut Snapshot)>;

#[test]
fn nothing_is_ready_until_every_part_of_it_is() {
    let cases: Vec<(&str, Change)> = vec![
        ("Watch CI off", Box::new(|s| s.settings.watch_ci = false)),
        (
            "not connected",
            Box::new(|s| s.settings.repositories.clear()),
        ),
        (
            "releases not read yet",
            Box::new(|s| s.releases[0].read_at.clear()),
        ),
        (
            "GitHub won't show the releases",
            Box::new(|s| s.releases[0].unseen = Some("Not found".into())),
        ),
        (
            "Orbit's sign-in can't push, so drafts are hidden",
            Box::new(|s| s.releases[0].drafts_hidden = true),
        ),
        (
            "more drafts than Orbit read",
            Box::new(|s| s.releases[0].draft_count = 11),
        ),
        (
            "no draft for the newest tag",
            Box::new(|s| s.releases[0].drafts[0].tag = "v0.2.1".into()),
        ),
        (
            "two drafts for the newest tag",
            Box::new(|s| {
                s.releases[0].drafts.push(release(8, "v0.3.0", true));
                s.releases[0].draft_count = 2;
            }),
        ),
        (
            "a release for the tag is out already",
            Box::new(|s| s.releases[0].latest = Some(release(6, "v0.3.0", false))),
        ),
        (
            "a pre-release for the tag is out already",
            Box::new(|s| s.releases[0].prerelease = Some(release(6, "v0.3.0", false))),
        ),
        ("no build read", Box::new(|s| s.builds.clear())),
        (
            "build still running",
            Box::new(|s| s.builds[0].ci.state = CiState::Running),
        ),
        (
            "build failing",
            Box::new(|s| s.builds[0].ci.state = CiState::Failing),
        ),
        (
            "nothing ran",
            Box::new(|s| s.builds[0].ci.state = CiState::None),
        ),
        (
            "build read at another commit, so the tag moved",
            Box::new(|s| s.builds[0].sha = B.into()),
        ),
        (
            "build of another tag",
            Box::new(|s| s.builds[0].tag = "v0.2.0".into()),
        ),
        (
            "build's latest read failed",
            Box::new(|s| s.builds[0].error = Some("Bad Gateway".into())),
        ),
        (
            "a newer tag with no draft",
            Box::new(|s| {
                s.releases[0].tags.insert(
                    0,
                    Tag {
                        name: "v0.4.0".into(),
                        sha: B.into(),
                    },
                )
            }),
        ),
        (
            "being published",
            Box::new(|s| {
                let d = waiting(s).remove(0);
                record(&mut s.journal, &d, now());
            }),
        ),
        (
            "published by Orbit",
            Box::new(|s| {
                let d = waiting(s).remove(0);
                let id = record(&mut s.journal, &d, now());
                settle(&mut s.journal, &id, Ok(release(7, "v0.3.0", false)), now());
            }),
        ),
    ];
    for (why, change) in cases {
        let mut s = repo();
        change(&mut s);
        assert_eq!(ready(&s), vec![], "{why}");
    }
    // A publish GitHub refused may be tried again.
    let mut s = repo();
    let d = waiting(&s).remove(0);
    let id = record(&mut s.journal, &d, now());
    settle(
        &mut s.journal,
        &id,
        Err("gh: Validation Failed (HTTP 422)".into()),
        now(),
    );
    assert_eq!(ready(&s), the_draft());
}

#[test]
fn the_draft_tapped_is_the_one_published() {
    let s = repo();
    assert_eq!(offered(&s, "a/b", "v0.3.0").unwrap().id, 7);
    assert_eq!(
        offered(&s, "a/b", "v0.2.0").unwrap_err(),
        "a/b has a draft for v0.3.0 ready now, not v0.2.0, so Orbit didn't publish v0.2.0."
    );
    let mut gone = repo();
    gone.builds.clear();
    assert_eq!(
        offered(&gone, "a/b", "v0.3.0").unwrap_err(),
        "a/b has no draft for v0.3.0 ready any more, so Orbit didn't publish it."
    );
    assert_eq!(
        may_publish(&gone, "a/b").unwrap_err(),
        "a/b has no draft ready to publish now, so Orbit won't publish one."
    );
    // One on its way holds back any other in the repository.
    let mut s = repo();
    let d = may_publish(&s, "a/b").unwrap();
    let id = record(&mut s.journal, &d, now());
    let w = &s.journal[0];
    assert_eq!(
        (
            w.action,
            w.thread.as_str(),
            w.tag.as_deref(),
            w.head.as_deref(),
            w.release,
            w.number
        ),
        (
            Action::PublishRelease,
            "release:a/b:v0.3.0",
            Some("v0.3.0"),
            Some(A),
            Some(7),
            None
        )
    );
    assert_eq!(
        may_publish(&s, "a/b").unwrap_err(),
        "Orbit is already publishing a release here."
    );
    settle(
        &mut s.journal,
        &id,
        Err("gh: Bad Gateway (HTTP 502)".into()),
        now(),
    );
    assert_eq!(
        may_publish(&s, "a/b").unwrap_err(),
        "Orbit doesn't know yet whether GitHub published its last release here, so it won't publish one until it has looked."
    );
    assert_eq!(unsettled(&s.journal), [(id, "a/b".to_string(), 7)]);
}

#[test]
fn what_github_says_to_a_publish_is_kept() {
    let mut s = repo();
    let d = waiting(&s).remove(0);
    let id = record(&mut s.journal, &d, now());
    settle(&mut s.journal, &id, Ok(release(7, "v0.3.0", false)), now());
    let w = &s.journal[0];
    assert_eq!(w.state, WriteState::Done);
    assert_eq!(w.url, "https://github.com/a/b/releases/tag/v0.3.0");
    assert!(unsettled(&s.journal).is_empty());

    let said = |error: &str| {
        let mut s = repo();
        let id = record(&mut s.journal, &d, now());
        settle(&mut s.journal, &id, Err(error.into()), now());
        let w = s.journal[0].clone();
        (w.state, w.error.unwrap())
    };
    assert_eq!(
        said("gh: Validation Failed (HTTP 422)"),
        (
            WriteState::Failed,
            "GitHub refused to publish the draft for v0.3.0, such as when a release for it is out already: Validation Failed (HTTP 422).".into()
        )
    );
    assert_eq!(
        said("gh: Not Found (HTTP 404)"),
        (
            WriteState::Failed,
            "GitHub didn't let Orbit's sign-in publish the draft for v0.3.0: Not Found (HTTP 404)."
                .into()
        )
    );
    assert_eq!(
        said("gh: API rate limit exceeded (HTTP 403)").1,
        "GitHub's rate limit held this back. Tap it again later."
    );
    assert_eq!(
        said("gh: Bad Gateway (HTTP 502)"),
        (
            WriteState::Sending,
            "GitHub didn't answer clearly (Bad Gateway (HTTP 502)), so Orbit looks on GitHub at the draft for v0.3.0 before it publishes it again.".into()
        )
    );
}

#[test]
fn a_publish_cut_off_is_settled_by_what_github_has() {
    let cut_off = || {
        let mut s = repo();
        let d = waiting(&s).remove(0);
        let id = record(&mut s.journal, &d, now());
        (s, id)
    };
    // Out: it went, and is kept as the latest release.
    let (mut s, id) = cut_off();
    let out = release(7, "v0.3.0", false);
    found(&mut s.journal, &mut s.releases, &id, Ok(Some(out)), now());
    assert_eq!(s.journal[0].state, WriteState::Done);
    assert_eq!(
        s.journal[0].url,
        "https://github.com/a/b/releases/tag/v0.3.0"
    );
    let r = &s.releases[0];
    assert_eq!(r.latest.as_ref().map(|l| l.tag.as_str()), Some("v0.3.0"));
    assert_eq!((r.drafts.len(), r.draft_count, r.published), (0, 0, 2));
    assert!(waiting(&s).is_empty());

    // Still a draft: it didn't go, so it is ready again.
    let (mut s, id) = cut_off();
    let draft = release(7, "v0.3.0", true);
    found(&mut s.journal, &mut s.releases, &id, Ok(Some(draft)), now());
    assert_eq!(s.journal[0].state, WriteState::Failed);
    assert_eq!(
        s.journal[0].error.as_deref(),
        Some("The draft for v0.3.0 didn't get published.")
    );
    assert_eq!(ready(&s), the_draft());

    // Gone.
    let (mut s, id) = cut_off();
    found(&mut s.journal, &mut s.releases, &id, Ok(None), now());
    assert_eq!(
        (s.journal[0].state, s.journal[0].error.as_deref()),
        (
            WriteState::Failed,
            Some("The draft for v0.3.0 is gone from GitHub.")
        )
    );

    // A failed look keeps it unsettled, and nothing is sent meanwhile.
    let (mut s, id) = cut_off();
    found(
        &mut s.journal,
        &mut s.releases,
        &id,
        Err("gh: Bad Gateway (HTTP 502)".into()),
        now(),
    );
    assert_eq!(s.journal[0].state, WriteState::Sending);
    assert_eq!(
        s.journal[0].error.as_deref(),
        Some("Orbit couldn't look on GitHub at the draft for v0.3.0 yet (Bad Gateway (HTTP 502)), so it won't publish it until it has.")
    );
    assert!(may_publish(&s, "a/b").is_err());
}

#[test]
fn a_published_pre_release_is_kept_as_the_newest_pre_release() {
    let mut s = repo();
    let mut out = release(7, "v0.3.0", false);
    out.prerelease = true;
    published(&mut s.releases[0], out);
    let r = &s.releases[0];
    assert_eq!(r.latest.as_ref().map(|l| l.tag.as_str()), Some("v0.2.0"));
    assert_eq!(
        r.prerelease.as_ref().map(|l| l.tag.as_str()),
        Some("v0.3.0")
    );
    assert!(r.drafts.is_empty());
    // Found out again, nothing is counted twice.
    published(&mut s.releases[0], release(7, "v0.3.0", false));
    assert_eq!((s.releases[0].published, s.releases[0].draft_count), (2, 0));
}
