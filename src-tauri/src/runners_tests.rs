use super::*;
use serde_json::json;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-30T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
fn ago(minutes: i64) -> String {
    stamp(now() - Duration::minutes(minutes))
}
fn place(name: &str, kind: PlaceKind, read: i64) -> Place {
    Place {
        name: name.into(),
        kind,
        read_at: ago(read),
        ..Default::default()
    }
}
fn runner(name: &str, online: bool, busy: bool) -> Runner {
    Runner {
        name: name.into(),
        online,
        busy,
        ..Default::default()
    }
}
fn repos(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}
fn names(wanted: &[(String, PlaceKind)]) -> Vec<(&str, PlaceKind)> {
    wanted.iter().map(|(n, k)| (n.as_str(), *k)).collect()
}

#[test]
fn each_repository_and_each_account_that_owns_one_is_read() {
    use PlaceKind::*;
    // Two repositories of one owner, and one that isn't a repository.
    let all = repos(&["a/b", "A/c", "nope"]);
    let due = wanted(&all, &[], now());
    assert_eq!(
        names(&due),
        [("a/b", Repository), ("a", Owner), ("A/c", Repository)]
    );
    // An owner Orbit knows is an organization is read as one, after the
    // repository never read.
    let known = [place("a", Organization, 60)];
    assert_eq!(
        names(&wanted(&repos(&["a/b"]), &known, now())),
        [("a/b", Repository), ("a", Organization)]
    );
}

#[test]
fn a_place_is_read_again_every_five_minutes_the_oldest_first() {
    use PlaceKind::*;
    let all = repos(&["a/b", "a/c", "d/e"]);
    let mut places = vec![
        place("a/b", Repository, 4),
        place("a", Organization, 6),
        place("a/c", Repository, 10),
        place("d/e", Repository, 5),
    ];
    // d is never read, so it comes first; a/b was read too lately.
    assert_eq!(
        names(&wanted(&all, &places, now())),
        [
            ("d", Owner),
            ("a/c", Repository),
            ("a", Organization),
            ("d/e", Repository)
        ]
    );
    // At most a few a pass.
    places[0].read_at = ago(20);
    assert_eq!(wanted(&all, &places, now()).len(), PER_PASS);
    // Nothing is due when everything was read lately.
    let fresh: Vec<Place> = places
        .iter()
        .map(|p| Place {
            read_at: ago(1),
            ..p.clone()
        })
        .chain([place("d", Person, 1)])
        .collect();
    assert!(wanted(&all, &fresh, now()).is_empty());
}

#[test]
fn a_persons_account_is_read_once_and_a_failed_read_waits() {
    use PlaceKind::*;
    let all = repos(&["a/b"]);
    let read = [place("a", Person, 60 * 24), place("a/b", Repository, 1)];
    assert!(
        wanted(&all, &read, now()).is_empty(),
        "a person shares no runners"
    );
    // A read that failed is tried once its wait is over, however lately.
    let mut failed = place("a/b", Repository, 1);
    failed.error = Some("down".into());
    failed.retry_at = Some(stamp(now() + Duration::minutes(1)));
    let places = [place("a", Person, 1), failed.clone()];
    assert!(wanted(&all, &places, now()).is_empty());
    failed.retry_at = Some(stamp(now()));
    let places = [place("a", Person, 1), failed];
    assert_eq!(names(&wanted(&all, &places, now())), [("a/b", Repository)]);
}

#[test]
fn runners_are_read_from_githubs_answer() {
    let v = json!({ "total_count": 3, "runners": [
        { "id": 1, "name": "build-1", "os": "linux", "status": "online", "busy": true,
          "labels": [{ "name": "self-hosted" }, { "name": "gpu" }] },
        { "id": 2, "name": "  build\n2 ", "os": "macOS", "status": "offline", "busy": false, "labels": [] },
    ] });
    let (list, total) = runners(&v).unwrap();
    assert_eq!(total, 3, "GitHub has more than Orbit read");
    assert_eq!(
        list,
        [
            Runner {
                name: "build-1".into(),
                os: "linux".into(),
                online: true,
                busy: true,
                labels: vec!["self-hosted".into(), "gpu".into()],
            },
            Runner {
                name: "build 2".into(),
                os: "macOS".into(),
                ..Default::default()
            },
        ]
    );
    // A total less than the list is the list.
    let (_, total) = runners(&json!({ "runners": [{ "name": "x" }] })).unwrap();
    assert_eq!(total, 1);
    assert!(runners(&json!([])).is_err());
    // Long names are cut short.
    let long = "r".repeat(200);
    let (list, _) = runners(&json!({ "runners": [{ "name": long }] })).unwrap();
    assert_eq!(list[0].name.chars().count(), 80);
    assert!(list[0].name.ends_with('…'));
}

#[test]
fn runs_that_waited_half_an_hour_for_a_runner_are_kept_the_longest_first() {
    let v = json!([
        { "id": 1, "name": "CI", "status": "queued", "created_at": ago(45),
          "html_url": "https://github.com/a/b/actions/runs/1" },
        // Its try began later than the run: that's when it began waiting.
        { "id": 2, "name": "Release", "status": "queued", "created_at": ago(300),
          "run_started_at": ago(10), "html_url": "https://github.com/a/b/actions/runs/2" },
        { "id": 3, "name": " ", "status": "queued", "run_started_at": ago(90),
          "html_url": "https://example.com/runs/3" },
        { "id": 4, "name": "Lint", "status": "in_progress", "created_at": ago(90) },
        { "id": 5, "name": "Docs", "status": "queued" },
        { "id": 6, "name": "CI", "status": "queued", "created_at": ago(30),
          "html_url": "https://github.com/a/b/actions/runs/6" },
    ]);
    let list = waiting(v.as_array().unwrap(), now());
    assert_eq!(
        list,
        [
            Waiting {
                id: 3,
                name: String::new(),
                url: String::new(),
                since: ago(90),
            },
            Waiting {
                id: 1,
                name: "CI".into(),
                url: "https://github.com/a/b/actions/runs/1".into(),
                since: ago(45),
            },
            Waiting {
                id: 6,
                name: "CI".into(),
                url: "https://github.com/a/b/actions/runs/6".into(),
                since: ago(30),
            },
        ]
    );
}

#[test]
fn runners_orbit_may_not_see_are_said_so_and_other_errors_are_tried_again() {
    let mut p = place("a/b", PlaceKind::Repository, 0);
    let refused = Err("gh: Must have admin rights to Repository. (HTTP 403)".to_string());
    assert!(seen(&mut p, refused).is_ok());
    assert_eq!(
        p.unseen.as_deref(),
        Some("Orbit can't see the self-hosted runners of a/b: that needs admin rights on it.")
    );
    let mut org = place("a", PlaceKind::Organization, 0);
    assert!(seen(&mut org, Err("gh: Not Found (HTTP 404)".into())).is_ok());
    assert_eq!(
        org.unseen.as_deref(),
        Some("Orbit can't see the runners the a organization shares: that needs an owner of a signed in with the admin:org scope.")
    );
    for error in [
        "gh: API rate limit exceeded for user ID 1. (HTTP 403)",
        "gh: Bad Gateway (HTTP 502)",
    ] {
        let mut p = place("a/b", PlaceKind::Repository, 0);
        assert!(seen(&mut p, Err(error.into())).is_err(), "{error}");
        assert!(p.unseen.is_none());
    }
    let mut p = place("a/b", PlaceKind::Repository, 0);
    let answer = json!({ "total_count": 1, "runners": [{ "name": "x", "status": "online" }] });
    seen(&mut p, Ok(answer)).unwrap();
    assert_eq!((p.runners.len(), p.total, p.unseen), (1, 1, None));
}

#[test]
fn a_failed_read_keeps_what_was_read_and_waits_longer_each_time() {
    let kind = PlaceKind::Repository;
    let mut places = vec![];
    let mut first = place("a/b", kind, 0);
    first.runners = vec![runner("x", true, false)];
    first.total = 1;
    record(&mut places, "a/b", kind, Ok(first.clone()), now());
    assert_eq!(places, [first.clone()]);
    let mut waits = vec![];
    for _ in 0..8 {
        record(
            &mut places,
            "A/B",
            kind,
            Err("gh: Bad Gateway (HTTP 502)".into()),
            now(),
        );
        let retry = time(places[0].retry_at.as_deref().unwrap()).unwrap();
        waits.push((retry - now()).num_minutes());
    }
    assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60]);
    assert_eq!(places.len(), 1);
    assert_eq!(places[0].runners, first.runners, "what was read is kept");
    assert_eq!(places[0].tries, 8);
    assert_eq!(
        places[0].error.as_deref(),
        Some("Orbit couldn't read the runners yet: Bad Gateway (HTTP 502). It tries again later.")
    );
    // A read that works starts over.
    record(&mut places, "a/b", kind, Ok(first.clone()), now());
    assert_eq!(places, [first]);
    // Another place is added.
    record(
        &mut places,
        "a",
        PlaceKind::Owner,
        Err("down".into()),
        now(),
    );
    assert_eq!(places.len(), 2);
    assert_eq!(places[1].kind, PlaceKind::Owner);
    assert_eq!(places[1].tries, 1);
}

#[test]
fn places_go_with_the_repositories_that_brought_them() {
    use PlaceKind::*;
    let mut places = vec![
        place("a/b", Repository, 0),
        place("a", Organization, 0),
        place("c/d", Repository, 0),
        place("c", Person, 0),
        place("e", Owner, 0),
    ];
    prune(&mut places, &repos(&["A/b", "e/f"]));
    let left: Vec<&str> = places.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(left, ["a/b", "a", "e"]);
}

#[test]
fn what_orbit_sees_of_the_runners_a_repository_could_use() {
    use PlaceKind::*;
    let with = |name: &str, kind: PlaceKind, list: Vec<Runner>| Place {
        total: list.len() as u32,
        runners: list,
        ..place(name, kind, 0)
    };
    let text = |places: &[Place]| seen_text(places, "a/b");
    let person = place("a", Person, 0);
    assert_eq!(text(&[]), None, "nothing read yet");
    let failed = Place {
        error: Some("down".into()),
        ..place("a/b", Repository, 0)
    };
    let never = Place {
        read_at: String::new(),
        ..failed.clone()
    };
    assert_eq!(
        text(&[never, person.clone()]),
        None,
        "never read, only tried"
    );
    let other = [place("a/c", Repository, 0), person.clone()];
    assert_eq!(text(&other), None, "another repository's");
    let none = place("a/b", Repository, 0);
    assert_eq!(
        text(std::slice::from_ref(&none)).as_deref(),
        Some("It has no self-hosted runners of its own. Orbit hasn't read what the a account shares yet.")
    );
    assert_eq!(
        text(&[none.clone(), person.clone()]).as_deref(),
        Some("Orbit sees no self-hosted runners for it, so its runs wait for GitHub's own runners or for one it doesn't have.")
    );
    let shares_none = place("a", Organization, 0);
    assert_eq!(
        text(&[none.clone(), person.clone()]),
        text(&[none.clone(), shares_none.clone()])
    );
    let more = Place {
        more_queued: true,
        ..none.clone()
    };
    assert_eq!(
        text(&[more, person.clone()]).as_deref(),
        Some("Orbit couldn't read every queued run, so more may wait. Orbit sees no self-hosted runners for it, so its runs wait for GitHub's own runners or for one it doesn't have.")
    );
    // Its own runners.
    let own = |list: Vec<Runner>| text(&[with("a/b", Repository, list), person.clone()]).unwrap();
    assert_eq!(
        own(vec![runner("x", false, false)]),
        "Its one self-hosted runner is offline."
    );
    assert_eq!(
        own(vec![runner("x", false, false), runner("y", false, false)]),
        "Its 2 self-hosted runners are all offline."
    );
    assert_eq!(
        own(vec![runner("x", false, false), runner("y", true, true)]),
        "Every one of its online self-hosted runners is busy."
    );
    assert_eq!(
        own(vec![runner("x", true, false), runner("y", true, true)]),
        "1 of its self-hosted runners is online and free, so the runs may ask for a label it doesn't have."
    );
    assert_eq!(
        own(vec![runner("x", true, false), runner("y", true, false)]),
        "2 of its self-hosted runners are online and free, so the runs may ask for a label none of them has."
    );
    let hidden = Place {
        unseen: Some(unseen("a/b", Repository)),
        ..none.clone()
    };
    assert_eq!(
        text(&[hidden, person.clone()]).as_deref(),
        Some("Orbit can't see its own self-hosted runners.")
    );
    let cut = Place {
        total: 150,
        ..with("a/b", Repository, vec![runner("x", false, false)])
    };
    assert_eq!(
        text(&[cut, person.clone()]).as_deref(),
        Some("Its one self-hosted runner is offline. Orbit read only 1 of its 150.")
    );
    assert_eq!(
        text(&[failed, person.clone()]).as_deref(),
        Some("It has no self-hosted runners of its own. Orbit's latest read of them failed, so this may be out of date.")
    );
    // What its organization shares, which a runner group may keep from it.
    let mine = with("a/b", Repository, vec![runner("x", false, false)]);
    let org = |p: Option<Place>| {
        let mut places = vec![mine.clone()];
        places.extend(p);
        text(&places).unwrap()
    };
    let offline = "Its one self-hosted runner is offline.";
    assert_eq!(
        org(None),
        format!("{offline} Orbit hasn't read what the a account shares yet.")
    );
    let tried = Place {
        read_at: String::new(),
        error: Some("down".into()),
        ..place("a", Owner, 0)
    };
    assert_eq!(
        org(Some(tried)),
        format!("{offline} Orbit couldn't read what the a account shares yet.")
    );
    assert_eq!(org(Some(person.clone())), offline);
    assert_eq!(
        org(Some(shares_none)),
        format!("{offline} The a organization shares no self-hosted runners.")
    );
    let free = with("a", Organization, vec![runner("y", true, false)]);
    assert_eq!(
        org(Some(free)),
        format!("{offline} The a organization shares 1 self-hosted runner (1 online and free), but Orbit can't tell whether a/b may use it.")
    );
    let two = with(
        "a",
        Organization,
        vec![runner("y", true, false), runner("z", true, true)],
    );
    assert_eq!(
        org(Some(two.clone())),
        format!("{offline} The a organization shares 2 self-hosted runners (1 online and free), but Orbit can't tell whether a/b may use them.")
    );
    let stale = Place {
        total: 300,
        error: Some("down".into()),
        ..two
    };
    assert_eq!(
        org(Some(stale)),
        format!("{offline} The a organization shares 300 self-hosted runners (1 online and free), but Orbit can't tell whether a/b may use them. Orbit read only 2 of them. Orbit's latest read of what a shares failed, so this may be out of date.")
    );
    let hidden = Place {
        unseen: Some(unseen("a", Organization)),
        ..place("a", Organization, 0)
    };
    assert_eq!(
        org(Some(hidden)),
        format!("{offline} Orbit can't see the runners the a organization shares.")
    );
}

/// A runner group can keep an organization's free runners from a
/// repository, so they never count as its own.
#[test]
fn an_organizations_free_runners_are_never_said_to_be_the_repositorys() {
    let mine = Place {
        total: 1,
        runners: vec![runner("x", false, false)],
        ..place("a/b", PlaceKind::Repository, 0)
    };
    let shared = Place {
        total: 1,
        runners: vec![runner("y", true, false)],
        ..place("a", PlaceKind::Organization, 0)
    };
    let text = seen_text(&[mine, shared], "a/b").unwrap();
    assert!(text.starts_with("Its one self-hosted runner is offline."));
    assert!(!text.contains("so the runs may ask for a label"));
    assert!(text.contains("Orbit can't tell whether a/b may use it."));
}
