use super::*;
use crate::{
    ci::{CiState, CommitCi, RepoCi},
    versions::{Kind, VersionFile},
    workflows::Workflow,
};

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";

fn now() -> DateTime<Utc> {
    "2026-11-02T12:00:00Z".parse().unwrap()
}
fn file(path: &str, kind: Kind, version: &str) -> VersionFile {
    VersionFile {
        path: path.into(),
        kind,
        version: version.into(),
        of: String::new(),
    }
}
/// `a/b`, whose `main` says `version` at `A` in `package.json` and
/// `Cargo.toml`, where CI passed, with these version tags.
fn repo(version: &str, tags: &[&str]) -> Snapshot {
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
    s.releases.push(RepoReleases {
        repo: "a/b".into(),
        read_at: "2026-11-02T11:00:00.000Z".into(),
        branch: "main".into(),
        head: A.into(),
        tags: tags
            .iter()
            .map(|t| Tag {
                name: t.to_string(),
                sha: B.into(),
            })
            .collect(),
        version_tags: tags.len() as u32,
        ..Default::default()
    });
    s.setups.push(Setup {
        repo: "a/b".into(),
        branch: "main".into(),
        sha: A.into(),
        checks: crate::workflows::CHECKS,
        versions: vec![
            file("package.json", Kind::Package, version),
            file("Cargo.toml", Kind::Cargo, version),
        ],
        ..Default::default()
    });
    s.ci.push(RepoCi {
        repo: "a/b".into(),
        branch: "main".into(),
        head: CommitCi {
            sha: A.into(),
            state: CiState::Passing,
            ..Default::default()
        },
        ..Default::default()
    });
    s
}
fn waits(s: &Snapshot) -> Vec<(String, String, String)> {
    waiting(s)
        .into_iter()
        .map(|u| (u.repo, u.tag, u.sha))
        .collect()
}
fn ready() -> Vec<(String, String, String)> {
    vec![("a/b".into(), "v0.3.0".into(), A.into())]
}

#[test]
fn a_green_bump_with_no_tag_yet_waits_for_one() {
    let s = repo("0.3.0", &["v0.2.0", "v0.1.0"]);
    let u = &waiting(&s)[0];
    assert_eq!(
        (u.branch.as_str(), u.version.as_str(), u.tag.as_str()),
        ("main", "0.3.0", "v0.3.0")
    );
    assert_eq!(key(u), "push:a/b:v0.3.0");
    assert_eq!(from_key(&key(u)), Some(("a/b", "v0.3.0")));
    assert_eq!(from_key("problem:x"), None);
    assert_eq!(
        reason(u),
        "a/b says 0.3.0 on main at 1111111, where CI passed, and there's no tag v0.3.0 yet."
    );
    // A lockfile doesn't vote, and a pre-release is a version too.
    let mut s = repo("1.0.0-beta.1", &["v0.9.0"]);
    s.setups[0]
        .versions
        .push(file("Cargo.lock", Kind::CargoLock, "0.9.0"));
    assert_eq!(waiting(&s)[0].tag, "v1.0.0-beta.1");
}

/// A change to what Orbit read, for a case that keeps a version from
/// waiting.
type Change = Box<dyn Fn(&mut Snapshot)>;

#[test]
fn nothing_waits_until_every_part_of_it_is_ready() {
    let cases: Vec<(&str, Change)> = vec![
        ("Watch CI off", Box::new(|s| s.settings.watch_ci = false)),
        (
            "not connected",
            Box::new(|s| s.settings.repositories.clear()),
        ),
        (
            "CI still running",
            Box::new(|s| s.ci[0].head.state = CiState::Running),
        ),
        (
            "CI failing",
            Box::new(|s| s.ci[0].head.state = CiState::Failing),
        ),
        (
            "CI read at another commit",
            Box::new(|s| s.ci[0].head.sha = B.into()),
        ),
        (
            "CI's latest read failed, so what passed may not hold",
            Box::new(|s| s.ci[0].error = Some("gh: Bad Gateway (HTTP 502)".into())),
        ),
        (
            "more tags on GitHub than Orbit read",
            Box::new(|s| s.releases[0].more_tags = true),
        ),
        (
            "version tags Orbit didn't keep",
            Box::new(|s| s.releases[0].version_tags += 1),
        ),
        (
            "a release workflow its tag wouldn't start",
            Box::new(|s| {
                s.setups[0].workflows.push(Workflow {
                    tags: vec!["release-*".into()],
                    ..Default::default()
                })
            }),
        ),
        (
            "files read before Orbit read tags",
            Box::new(|s| s.setups[0].checks = crate::workflows::VERSIONS),
        ),
        (
            "the files' read failed",
            Box::new(|s| s.setups[0].error = Some("down".into())),
        ),
        (
            "a version file Orbit couldn't read",
            Box::new(|s| s.setups[0].unread_versions = vec!["Cargo.toml".into()]),
        ),
        (
            "manifests that disagree",
            Box::new(|s| s.setups[0].versions[1].version = "0.2.0".into()),
        ),
        (
            "an open problem about the version files",
            Box::new(|s| {
                s.problems.push(crate::problems::Problem {
                    key: crate::problems::versions_key("a/b"),
                    kind: crate::problems::ProblemKind::Versions,
                    ..Default::default()
                })
            }),
        ),
        (
            "no version at all",
            Box::new(|s| s.setups[0].versions.clear()),
        ),
        (
            "a version that isn't one a tag names",
            Box::new(|s| {
                for f in &mut s.setups[0].versions {
                    f.version = "0.3".into();
                }
            }),
        ),
        ("its tag is there", Box::new(|s| tag(s, "v0.3.0"))),
        (
            "its tag is there without a v",
            Box::new(|s| tag(s, "0.3.0")),
        ),
        ("a higher tag is there", Box::new(|s| tag(s, "v0.4.0"))),
        (
            "no version tags and no workflow that runs on tags",
            Box::new(|s| s.releases[0].tags.clear()),
        ),
        (
            "Orbit's sign-in can't push",
            Box::new(|s| s.releases[0].drafts_hidden = true),
        ),
        (
            "releases not read yet",
            Box::new(|s| s.releases[0].read_at.clear()),
        ),
        (
            "a push on its way",
            Box::new(|s| push_of(s, WriteState::Sending)),
        ),
        ("a push done", Box::new(|s| push_of(s, WriteState::Done))),
    ];
    for (why, change) in cases {
        let mut s = repo("0.3.0", &["v0.2.0"]);
        assert_eq!(waits(&s), ready());
        change(&mut s);
        assert!(waits(&s).is_empty(), "{why}");
    }
    // A push that failed doesn't hold it back.
    let mut s = repo("0.3.0", &["v0.2.0"]);
    push_of(&mut s, WriteState::Failed);
    assert_eq!(waits(&s), ready());
}
fn tag(s: &mut Snapshot, name: &str) {
    seen(&mut s.releases[0], name, B);
}
fn push_of(s: &mut Snapshot, state: WriteState) {
    let u = waiting(s).remove(0);
    let id = record(&mut s.journal, &u, now());
    s.journal.iter_mut().find(|w| w.id == id).unwrap().state = state;
}

#[test]
fn a_tag_is_named_as_the_repository_names_them_and_starts_its_release() {
    // As its highest version tag does.
    let s = repo("0.3.0", &["0.2.0"]);
    assert_eq!(waiting(&s)[0].tag, "0.3.0");
    let workflow = |tags: &[&str], ignore: &[&str]| Workflow {
        path: ".github/workflows/release.yml".into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        tags_ignore: ignore.iter().map(|t| t.to_string()).collect(),
        ..Default::default()
    };
    // With none yet: v and the version, else the version alone, whichever
    // starts the release workflow, as GitHub reads its filter.
    let none: Vec<&str> = vec![];
    for (tags, ignore, name) in [
        (vec!["v*"], none.clone(), Some("v0.3.0")),
        (vec!["**"], none.clone(), Some("v0.3.0")),
        (vec!["v[0-9]+.[0-9]+.[0-9]+"], none.clone(), Some("v0.3.0")),
        (vec!["[0-9]+.[0-9]+.[0-9]+"], none.clone(), Some("0.3.0")),
        (vec!["1.*", "0.*"], none.clone(), Some("0.3.0")),
        (none.clone(), vec!["v*"], Some("0.3.0")),
        // A range the version is outside, another prefix, or left out.
        (vec!["1.*"], none.clone(), None),
        (vec!["release-*"], none.clone(), None),
        (vec!["v*", "!v0.*"], none.clone(), None),
        (none.clone(), vec!["*"], None),
    ] {
        let mut s = repo("0.3.0", &[]);
        s.setups[0].workflows.push(workflow(&tags, &ignore));
        assert_eq!(
            waiting(&s).first().map(|u| u.tag.as_str()),
            name,
            "{tags:?} {ignore:?}"
        );
    }
    // With version tags, the release workflow must take the name they give.
    let mut s = repo("0.3.0", &["v0.2.0"]);
    s.setups[0].workflows.push(workflow(&["v*", "!v*-*"], &[]));
    assert_eq!(waiting(&s)[0].tag, "v0.3.0");
    let mut s = repo("0.3.0-rc.1", &["v0.2.0"]);
    s.setups[0].workflows.push(workflow(&["v*", "!v*-*"], &[]));
    assert!(waiting(&s).is_empty());
    let mut s = repo("0.3.0", &["v0.2.0"]);
    s.setups[0].workflows.push(workflow(&["[0-9]*"], &[]));
    assert!(waiting(&s).is_empty());
}

/// Orbit offers a tag only when it read every version tag: a higher one,
/// or the same version without its `v`, may be among those it didn't.
#[test]
fn nothing_waits_while_some_tags_are_unread() {
    // 100 recent backport tags, and v2.0.0 on an older commit past them.
    let backports: Vec<String> = (0..100).map(|i| format!("v1.9.{i}")).collect();
    let names: Vec<&str> = backports.iter().map(String::as_str).collect();
    let mut s = repo("1.10.0", &names);
    assert_eq!(waiting(&s)[0].tag, "v1.10.0");
    s.releases[0].more_tags = true;
    assert!(waiting(&s).is_empty());
    // 0.3.0, among the tags Orbit didn't read.
    let mut s = repo("0.3.0", &["v0.2.0"]);
    s.releases[0].more_tags = true;
    assert!(waiting(&s).is_empty());
    // A tag Orbit finds is kept and counted.
    let mut s = repo("0.3.0", &["v0.2.0"]);
    seen(&mut s.releases[0], "0.3.0", A);
    assert_eq!(s.releases[0].version_tags, 2);
    assert!(waiting(&s).is_empty());
}

/// A tap pushes the tag on its button, at the commit Orbit showed, or
/// nothing; GitHub is asked for the version under either spelling.
#[test]
fn a_tap_pushes_only_what_it_offered() {
    let mut s = repo("0.3.0", &["v0.2.0"]);
    assert_eq!(offered(&s, "a/b", "v0.3.0").unwrap(), waiting(&s)[0]);
    assert_eq!(
        offered(&s, "a/b", "v0.2.9").unwrap_err(),
        "a/b waits for v0.3.0 now, not v0.2.9, so Orbit didn't push v0.2.9."
    );
    s.ci[0].head.state = CiState::Failing;
    assert_eq!(
        offered(&s, "a/b", "v0.3.0").unwrap_err(),
        "a/b doesn't wait for v0.3.0 any more, so Orbit didn't push it."
    );
    let tagged = |tag: &str| Untagged {
        tag: tag.into(),
        ..Default::default()
    };
    assert_eq!(spellings(&tagged("v0.3.0")), ["v0.3.0", "0.3.0"]);
    assert_eq!(spellings(&tagged("0.3.0")), ["0.3.0", "v0.3.0"]);
}

#[test]
fn only_one_push_at_a_time_and_none_while_orbit_doesn_t_know() {
    let mut s = repo("0.3.0", &["v0.2.0"]);
    assert_eq!(may_push(&s, "A/B").unwrap(), waiting(&s)[0]);
    let u = waiting(&s).remove(0);
    let id = record(&mut s.journal, &u, now());
    assert_eq!(
        may_push(&s, "a/b").unwrap_err(),
        "Orbit is already pushing a tag here."
    );
    settle(
        &mut s.journal,
        &id,
        Err("gh: Bad Gateway (HTTP 502)".into()),
        now(),
    );
    assert_eq!(
        may_push(&s, "a/b").unwrap_err(),
        "Orbit doesn't know yet whether GitHub took its last tag here, so it won't push one until it has looked."
    );
    assert_eq!(
        unsettled(&s.journal),
        [(id, "a/b".into(), "v0.3.0".into(), A.into())]
    );
    let s = repo("0.3.0", &["v0.3.0"]);
    assert_eq!(
        may_push(&s, "a/b").unwrap_err(),
        "a/b has no version waiting for a tag now, so Orbit won't push one."
    );
}

#[test]
fn what_github_says_to_a_push_is_recorded() {
    let s = repo("0.3.0", &["v0.2.0"]);
    let u = waiting(&s).remove(0);
    let entry = |result: Result<(), String>| {
        let mut journal = vec![];
        let id = record(&mut journal, &u, now());
        settle(&mut journal, &id, result, now());
        journal.remove(0)
    };
    let done = entry(Ok(()));
    assert_eq!(
        (
            done.state,
            done.url.as_str(),
            done.tag.as_deref(),
            done.head.as_deref()
        ),
        (
            WriteState::Done,
            "https://github.com/a/b/releases/tag/v0.3.0",
            Some("v0.3.0"),
            Some(A)
        )
    );
    assert_eq!(done.action, Action::PushTag);
    for (error, state, says) in [
        (
            "gh: Reference already exists (HTTP 422)",
            WriteState::Failed,
            "GitHub has v0.3.0 already, so Orbit didn't push it.",
        ),
        (
            "gh: Resource not accessible by integration (HTTP 403)",
            WriteState::Failed,
            "GitHub didn't let Orbit's sign-in push v0.3.0: Resource not accessible by integration (HTTP 403).",
        ),
        (
            "gh: API rate limit exceeded (HTTP 403)",
            WriteState::Failed,
            "GitHub's rate limit held this back. Tap it again later.",
        ),
        (
            "gh: Bad Gateway (HTTP 502)",
            WriteState::Sending,
            "GitHub didn't answer clearly (Bad Gateway (HTTP 502)), so Orbit looks on GitHub for v0.3.0 before it pushes it again.",
        ),
    ] {
        let w = entry(Err(error.into()));
        assert_eq!((w.state, w.error.as_deref()), (state, Some(says)), "{error}");
    }
}

#[test]
fn a_push_orbit_isn_t_sure_of_is_settled_by_what_is_on_github() {
    let start = || {
        let mut s = repo("0.3.0", &["v0.2.0"]);
        let u = waiting(&s).remove(0);
        let id = record(&mut s.journal, &u, now());
        (s, id)
    };
    // There, at its commit: it went, and Orbit keeps the tag.
    let (mut s, id) = start();
    found(
        &mut s.journal,
        &mut s.releases,
        &id,
        Ok(Some(A.into())),
        now(),
    );
    assert_eq!(s.journal[0].state, WriteState::Done);
    assert_eq!(s.journal[0].url, page("a/b", "v0.3.0"));
    assert_eq!(s.releases[0].tags[0].name, "v0.3.0");
    assert_eq!(s.releases[0].tags[0].sha, A);
    // Not there: it didn't go, so Orbit may offer it again.
    let (mut s, id) = start();
    found(&mut s.journal, &mut s.releases, &id, Ok(None), now());
    assert_eq!(
        (s.journal[0].state, s.journal[0].error.as_deref()),
        (WriteState::Failed, Some("v0.3.0 didn't get to GitHub."))
    );
    assert_eq!(waits(&s), ready());
    // Somewhere else: someone else made it, and Orbit leaves it.
    let (mut s, id) = start();
    found(
        &mut s.journal,
        &mut s.releases,
        &id,
        Ok(Some(B.into())),
        now(),
    );
    assert_eq!(
        (s.journal[0].state, s.journal[0].error.as_deref()),
        (
            WriteState::Failed,
            Some("Someone else pushed v0.3.0, at 2222222, so Orbit left it.")
        )
    );
    assert_eq!(s.releases[0].tags[0].sha, B);
    assert!(waits(&s).is_empty());
    // Couldn't look: still unsure.
    let (mut s, id) = start();
    found(
        &mut s.journal,
        &mut s.releases,
        &id,
        Err("gh: Bad Gateway (HTTP 502)".into()),
        now(),
    );
    assert_eq!(s.journal[0].state, WriteState::Sending);
    assert_eq!(unsettled(&s.journal).len(), 1);
    assert!(waits(&s).is_empty());
}
