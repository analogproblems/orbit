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
fn a_tag_is_named_as_the_repository_names_them() {
    // As its highest version tag does.
    let s = repo("0.3.0", &["0.2.0"]);
    assert_eq!(waiting(&s)[0].tag, "0.3.0");
    // With none, as its release workflow's filter takes them: with a v,
    // unless every filter starts with a digit.
    let workflow = |tags: &[&str]| Workflow {
        path: ".github/workflows/release.yml".into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        ..Default::default()
    };
    for (tags, name) in [
        (vec!["v*"], "v0.3.0"),
        (vec!["**"], "v0.3.0"),
        (vec!["[0-9]+.[0-9]+.[0-9]+"], "0.3.0"),
        (vec!["1.*", "0.*"], "0.3.0"),
        (vec!["0.*", "v*"], "v0.3.0"),
    ] {
        let mut s = repo("0.3.0", &[]);
        s.setups[0].workflows.push(workflow(&tags));
        assert_eq!(waiting(&s)[0].tag, name, "{tags:?}");
    }
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
