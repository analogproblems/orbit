use super::*;
use serde_json::json;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-02T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
fn ago(minutes: i64) -> String {
    stamp(now() - Duration::minutes(minutes))
}
fn sha(c: char) -> String {
    c.to_string().repeat(40)
}
fn tag(name: &str, at: char) -> Tag {
    Tag {
        name: name.into(),
        sha: sha(at),
    }
}
fn repos(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}
/// `repo`'s releases, read with these version tags, the highest first.
fn tagged(repo: &str, tags: &[(&str, char)]) -> RepoReleases {
    RepoReleases {
        repo: repo.into(),
        read_at: ago(1),
        tags: tags.iter().map(|(n, c)| tag(n, *c)).collect(),
        ..Default::default()
    }
}
/// What Orbit read of `repo`'s build of `name` at `at`, standing so,
/// first read and last read these many minutes ago.
fn built(repo: &str, name: &str, at: char, state: CiState, first: i64, last: i64) -> Build {
    Build {
        repo: repo.into(),
        tag: name.into(),
        sha: sha(at),
        ci: CommitCi {
            sha: sha(at),
            state,
            ..Default::default()
        },
        first_read_at: ago(first),
        read_at: ago(last),
        ..Default::default()
    }
}
/// A run of the tag `v1.0.0` at commit 1, as GitHub lists it.
fn listed(id: u64, workflow: u64, name: &str, status: &str, conclusion: Option<&str>) -> Value {
    json!({
        "id": id,
        "workflow_id": workflow,
        "name": name,
        "head_branch": "v1.0.0",
        "head_sha": sha('1'),
        "event": "push",
        "status": status,
        "conclusion": conclusion,
        "html_url": format!("https://github.com/a/b/actions/runs/{id}"),
    })
}

/// Each repository's newest version tag is read once it is new or moved,
/// again every few minutes while its build runs or fails, or while it has
/// no runs yet for a day, and every half hour once it passed; a failed
/// read waits.
/// Those not read for their tag come first, then those read longest ago.
#[test]
fn a_new_tag_is_read_and_then_again_until_its_build_passes() {
    let releases = vec![
        tagged("a/new", &[("v1.0.0", '1'), ("v0.9.0", '9')]),
        tagged("a/moved", &[("v1.0.0", '2')]),
        tagged("a/passed", &[("v1.0.0", '1')]),
        tagged("a/running", &[("v1.0.0", '1')]),
        tagged("a/failing", &[("v1.0.0", '1')]),
        tagged("a/fresh", &[("v1.0.0", '1')]),
        tagged("a/quiet", &[("v1.0.0", '1')]),
        tagged("a/old", &[("v1.0.0", '1')]),
        tagged("a/none", &[]),
    ];
    let builds = vec![
        built("a/new", "v0.9.0", '9', CiState::Passing, 60, 60),
        built("a/moved", "v1.0.0", '1', CiState::Passing, 60, 60),
        built("a/passed", "v1.0.0", '1', CiState::Passing, 60, 20),
        built("a/running", "v1.0.0", '1', CiState::Running, 60, 6),
        built("a/failing", "v1.0.0", '1', CiState::Failing, 60, 10),
        built("a/fresh", "v1.0.0", '1', CiState::Failing, 60, 2),
        built("a/quiet", "v1.0.0", '1', CiState::None, 60, 8),
        built("a/old", "v1.0.0", '1', CiState::None, 2 * 24 * 60, 8),
    ];
    let all = repos(&[
        "a/new",
        "a/moved",
        "a/passed",
        "a/running",
        "a/failing",
        "a/fresh",
        "a/quiet",
        "a/old",
        "a/none",
        "a/unread",
    ]);
    let wanted = |repos: &[String], builds: &[Build]| -> Vec<String> {
        wanted(repos, &releases, builds, now())
            .into_iter()
            .map(|(r, t)| format!("{r} {} {}", t.name, &t.sha[..1]))
            .collect()
    };
    assert_eq!(
        wanted(&all, &builds),
        [
            "a/new v1.0.0 1",
            "a/moved v1.0.0 2",
            "a/failing v1.0.0 1",
            "a/quiet v1.0.0 1"
        ]
    );
    // At most `PER_PASS`, the oldest read first.
    let some = repos(&["a/running", "a/failing", "a/quiet", "a/fresh"]);
    assert_eq!(
        wanted(&some, &builds),
        [
            "a/failing v1.0.0 1",
            "a/quiet v1.0.0 1",
            "a/running v1.0.0 1"
        ]
    );
    // A passing build is read again every half hour, as a later run at the
    // tag, such as one a published release starts, may fail.
    let passed = built("a/passed", "v1.0.0", '1', CiState::Passing, 60, 31);
    assert_eq!(
        wanted(&repos(&["a/passed"]), &[passed]),
        ["a/passed v1.0.0 1"]
    );
    // A failed read waits for its time.
    let mut failed = built("a/passed", "v1.0.0", '1', CiState::None, 60, 60);
    failed.error = Some("Orbit couldn't read the release build yet".into());
    failed.retry_at = Some(stamp(now() + Duration::minutes(1)));
    let one = repos(&["a/passed"]);
    assert!(wanted(&one, std::slice::from_ref(&failed)).is_empty());
    failed.retry_at = Some(ago(1));
    assert_eq!(wanted(&one, &[failed]), ["a/passed v1.0.0 1"]);
    // Tags Orbit can't see, or hasn't read, aren't asked about.
    let mut unseen = tagged("a/new", &[("v1.0.0", '1')]);
    unseen.unseen = Some("Orbit's sign-in can't see this repository's tags.".into());
    assert!(super::wanted(&repos(&["a/new"]), &[unseen], &[], now()).is_empty());
}

/// Orbit reads only the tags of the newest commits, so a build it knows of
/// a higher version than any it read, and missing from them, stays the
/// one watched: only a higher version read, or a full list without its
/// tag, takes its place. Its tag moving forgets it, to be read afresh.
#[test]
fn a_partial_list_of_tags_doesnt_replace_the_build_orbit_knows() {
    let failing = built("a/b", "v2.0.0", '2', CiState::Failing, 60, 10);
    let kept = |r: &RepoReleases| {
        let mut builds = vec![failing.clone()];
        keep_current(&mut builds, r);
        !builds.is_empty()
    };
    let partial = |tags: &[(&str, char)]| RepoReleases {
        more_tags: true,
        ..tagged("a/b", tags)
    };
    // Backports newer by commit date, or no version tags at all, on the
    // newest commits.
    for r in [partial(&[("v1.9.1", '3'), ("v1.9.0", '4')]), partial(&[])] {
        assert_eq!(target(&r, Some(&failing)), Some(tag("v2.0.0", '2')));
        assert!(kept(&r));
        assert_eq!(
            super::wanted(
                &repos(&["a/b"]),
                &[r],
                std::slice::from_ref(&failing),
                now()
            )
            .into_iter()
            .map(|(_, t)| t)
            .collect::<Vec<_>>(),
            [tag("v2.0.0", '2')]
        );
    }
    // The same version spelled another way doesn't replace it either.
    for r in [
        partial(&[("2.0.0", '3'), ("v1.9.1", '4')]),
        partial(&[("v2.0.0+build.7", '3')]),
    ] {
        assert_eq!(target(&r, Some(&failing)), Some(tag("v2.0.0", '2')));
        assert!(kept(&r));
    }
    // With no build known, the highest read is the one.
    let r = partial(&[("v1.9.1", '3')]);
    assert_eq!(target(&r, None), Some(tag("v1.9.1", '3')));
    // A higher version read takes its place.
    let r = partial(&[("v2.1.0", '5'), ("v1.9.1", '3')]);
    assert_eq!(target(&r, Some(&failing)), Some(tag("v2.1.0", '5')));
    assert!(!kept(&r));
    // A full list without its tag: gone.
    let r = tagged("a/b", &[("v1.9.1", '3')]);
    assert_eq!(target(&r, Some(&failing)), Some(tag("v1.9.1", '3')));
    assert!(!kept(&r));
    let r = tagged("a/b", &[]);
    assert_eq!(target(&r, Some(&failing)), None);
    assert!(!kept(&r));
    // The tag moved.
    assert!(!kept(&tagged("a/b", &[("v2.0.0", '6')])));
    // Still the same, or a read that didn't show the tags: kept.
    assert!(kept(&tagged("a/b", &[("v2.0.0", '2'), ("v1.9.1", '3')])));
    let mut unseen = tagged("a/b", &[]);
    unseen.unseen = Some("Not found".into());
    assert!(kept(&unseen));
    let mut unread = tagged("a/b", &[]);
    unread.read_at.clear();
    assert!(kept(&unread));
    // Another repository's read leaves it be.
    assert!(kept(&tagged("a/c", &[])));
}

/// A run passes, fails, or still runs as GitHub says, as a check does.
#[test]
fn a_run_stands_as_github_says() {
    let state = |status: &str, conclusion: Option<&str>| {
        run_state(&listed(1, 1, "Release", status, conclusion))
    };
    assert_eq!(state("completed", Some("success")), CiState::Passing);
    for failed in ["failure", "timed_out", "startup_failure"] {
        assert_eq!(state("completed", Some(failed)), CiState::Failing);
    }
    for neither in ["cancelled", "skipped", "neutral", "stale"] {
        assert_eq!(state("completed", Some(neither)), CiState::None);
    }
    assert_eq!(
        state("completed", Some("action_required")),
        CiState::Running
    );
    for status in ["queued", "in_progress", "waiting", "requested", "pending"] {
        assert_eq!(state(status, None), CiState::Running);
    }
}

/// Only runs for the tag at its commit count, the newest of each
/// workflow, by name; a failed run's failed jobs are its checks.
#[test]
fn the_newest_run_of_each_workflow_at_the_tag_counts() {
    let mut elsewhere = listed(9, 3, "Docs", "completed", Some("failure"));
    elsewhere["head_sha"] = json!(sha('2'));
    let mut branch = listed(8, 3, "Docs", "completed", Some("failure"));
    branch["head_branch"] = json!("main");
    let items = vec![
        listed(5, 1, "Release", "completed", Some("failure")),
        listed(3, 1, "Release", "completed", Some("success")),
        listed(4, 2, "Publish", "in_progress", None),
        elsewhere,
        branch,
    ];
    let runs = runs_of(&items, &tag("v1.0.0", '1'));
    let seen: Vec<(u64, &str, CiState)> = runs
        .iter()
        .map(|(r, _)| (r.id, r.name.as_str(), r.state))
        .collect();
    assert_eq!(
        seen,
        [
            (4, "Publish", CiState::Running),
            (5, "Release", CiState::Failing)
        ]
    );
    assert_eq!(runs[1].0.url, "https://github.com/a/b/actions/runs/5");
    assert_eq!(runs[1].0.event, "push");
    // How each ended, once it has.
    assert_eq!(runs[0].0.conclusion, "");
    assert_eq!(runs[1].0.conclusion, "failure");

    let job = |name: &str, status: &str, conclusion: Option<&str>| {
        json!({
            "name": name,
            "status": status,
            "conclusion": conclusion,
            "html_url": format!("https://github.com/a/b/actions/runs/5/job/{}", name.len()),
        })
    };
    let jobs = [
        job("build (macos)", "completed", Some("failure")),
        job("build (linux)", "completed", Some("cancelled")),
        job("sign", "completed", Some("timed_out")),
        job("upload", "completed", Some("success")),
        job("notes", "in_progress", None),
    ];
    assert_eq!(
        failed_jobs(&runs[1].0, &jobs),
        [
            Check {
                name: "Release / build (macos)".into(),
                url: "https://github.com/a/b/actions/runs/5/job/13".into()
            },
            Check {
                name: "Release / sign".into(),
                url: "https://github.com/a/b/actions/runs/5/job/4".into()
            }
        ]
    );
}

/// A read of the same tag keeps when Orbit first read it, and a new tag
/// starts again. A failed read keeps what was read of that tag, with why,
/// and waits longer each time, up to an hour.
#[test]
fn a_read_is_kept_and_a_failed_one_waits() {
    let mut builds = vec![built("a/b", "v1.0.0", '1', CiState::Running, 30, 5)];
    let fresh = built("a/b", "v1.0.0", '1', CiState::Failing, 0, 0);
    record(&mut builds, "a/b", &tag("v1.0.0", '1'), Ok(fresh), now());
    assert_eq!(builds.len(), 1);
    assert_eq!(builds[0].ci.state, CiState::Failing);
    assert_eq!(builds[0].first_read_at, ago(30));
    assert_eq!(builds[0].read_at, ago(0));

    record(
        &mut builds,
        "a/b",
        &tag("v1.0.0", '1'),
        Err("gh: Bad Gateway (HTTP 502)".into()),
        now(),
    );
    assert_eq!(builds[0].ci.state, CiState::Failing);
    assert_eq!(builds[0].tries, 1);
    assert_eq!(
        builds[0].error.as_deref(),
        Some("Orbit couldn't read the release build yet: Bad Gateway (HTTP 502). It tries again later.")
    );
    assert_eq!(
        builds[0].retry_at.as_deref(),
        Some(&*stamp(now() + Duration::minutes(1)))
    );
    for _ in 0..10 {
        record(
            &mut builds,
            "a/b",
            &tag("v1.0.0", '1'),
            Err("boom".into()),
            now(),
        );
    }
    assert_eq!(
        builds[0].retry_at.as_deref(),
        Some(&*stamp(now() + Duration::hours(1)))
    );

    // A new tag that GitHub didn't answer for: nothing of the old one is
    // kept, and Orbit tries again later.
    record(
        &mut builds,
        "a/b",
        &tag("v1.1.0", '2'),
        Err("boom".into()),
        now(),
    );
    assert_eq!(
        (builds[0].tag.as_str(), builds[0].sha.as_str()),
        ("v1.1.0", &*sha('2'))
    );
    assert_eq!(builds[0].ci.state, CiState::None);
    assert_eq!(
        (builds[0].tries, builds[0].first_read_at.as_str()),
        (1, &*ago(0))
    );
    // Read at last, it starts from then.
    let fresh = built("a/b", "v1.1.0", '2', CiState::Passing, 0, 0);
    record(&mut builds, "a/b", &tag("v1.1.0", '2'), Ok(fresh), now());
    assert!(builds[0].error.is_none() && builds[0].tries == 0);

    // Disconnected repositories are forgotten.
    prune(&mut builds, &repos(&["c/d"]));
    assert!(builds.is_empty());
}

/// What Bit says of a build.
#[test]
fn a_build_in_a_sentence() {
    let mut b = built("a/b", "v1.0.0", '1', CiState::Passing, 30, 5);
    b.ci.passing = 2;
    assert_eq!(
        sentence(&b, now()).as_deref(),
        Some("The release build for v1.0.0 passed (2 runs).")
    );
    b.ci.state = CiState::Running;
    assert_eq!(
        sentence(&b, now()).as_deref(),
        Some("The release build for v1.0.0 is still running.")
    );
    b.ci.state = CiState::Failing;
    b.ci.failing = vec![Check {
        name: "Release / sign".into(),
        url: String::new(),
    }];
    assert_eq!(
        sentence(&b, now()).as_deref(),
        Some("The release build for v1.0.0 is failing: Release / sign.")
    );
    b.ci.state = CiState::None;
    assert_eq!(
        sentence(&b, now()).as_deref(),
        Some("Nothing has run for v1.0.0 yet.")
    );
    // An old tag's runs may be long gone.
    b.first_read_at = stamp(now() - LOOK_FOR);
    assert_eq!(sentence(&b, now()), None);
    b.read_at.clear();
    b.ci.state = CiState::Passing;
    assert_eq!(sentence(&b, now()), None);
}

/// A release may be published from a build only when every workflow's
/// newest run passed, or was skipped by its own conditions, with at least
/// one passing: a cancelled run counts against it, though `ci.state`
/// leaves it out.
#[test]
fn a_release_is_published_only_from_a_build_whose_every_run_passed() {
    let run = |id: u64, name: &str, state: CiState, conclusion: &str| Run {
        id,
        name: name.into(),
        state,
        conclusion: conclusion.into(),
        ..Default::default()
    };
    let with = |runs: Vec<Run>| Build {
        runs,
        ..built("a/b", "v1.0.0", '1', CiState::Passing, 60, 10)
    };
    let release = run(1, "Release", CiState::Passing, "success");
    assert_eq!(publishable(&with(vec![release.clone()])), Ok(()));
    let docs_skipped = run(2, "Docs", CiState::None, "skipped");
    assert_eq!(
        publishable(&with(vec![release.clone(), docs_skipped.clone()])),
        Ok(())
    );
    for (other, why) in [
        (
            run(2, "Package", CiState::None, "cancelled"),
            "Package was cancelled",
        ),
        (
            run(2, "Package", CiState::Running, ""),
            "Package is still running",
        ),
        (
            run(2, "Package", CiState::Failing, "failure"),
            "Package failed",
        ),
        (
            run(2, "Package", CiState::None, "neutral"),
            "Package ended as neutral",
        ),
        (run(2, "", CiState::None, "stale"), "a run ended as stale"),
        (
            run(2, "Package", CiState::Passing, ""),
            "Orbit hasn't read how Package ended",
        ),
    ] {
        assert_eq!(
            publishable(&with(vec![release.clone(), other])),
            Err(why.to_string())
        );
    }
    assert_eq!(
        publishable(&with(vec![docs_skipped])),
        Err("no run v1.0.0 started has passed".into())
    );
    assert_eq!(
        publishable(&with(vec![])),
        Err("no run v1.0.0 started has passed".into())
    );
    let mut failed_read = with(vec![release.clone()]);
    failed_read.error = Some("Bad Gateway".into());
    assert_eq!(
        publishable(&failed_read),
        Err("Orbit's latest read of the runs v1.0.0 started failed".into())
    );
    let mut more = with(vec![release]);
    more.ci.incomplete = true;
    assert_eq!(
        publishable(&more),
        Err("v1.0.0 started more runs than Orbit reads".into())
    );
}
