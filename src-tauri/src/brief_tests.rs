use super::*;
use crate::attention::{announce, items, notice};
use crate::buddy::NoteKind;
use crate::problems::Problem;
use crate::readiness::tests as r;
use crate::runners::{Place, PlaceKind, Runner};

fn at(day: &str, time: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(&format!("{day} {time}"), "%Y-%m-%d %H:%M").unwrap()
}
fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-31T09:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
fn problem(id: &str, kind: ProblemKind) -> Problem {
    Problem {
        id: id.into(),
        kind,
        repo: "a/b".into(),
        branch: "main".into(),
        sha: "1".repeat(40),
        since_sha: "2".repeat(40),
        failing: vec![crate::ci::Check {
            name: "build".into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}
/// A day with one of everything a brief lists but stuck pull requests: a
/// pull request clear to merge, a CI problem of each kind, runs waiting,
/// runners all offline and caches near GitHub's limit.
fn busy() -> Snapshot {
    let (mut s, _) = r::ready();
    s.problems = vec![
        problem("main", ProblemKind::DefaultBranch),
        Problem {
            pr_id: Some("a/b#7".into()),
            ..problem("pull", ProblemKind::PullRequest)
        },
        Problem {
            pr_id: Some("a/b#1".into()),
            base: "main".into(),
            base_sha: "3".repeat(40),
            ..problem("base", ProblemKind::BaseMoved)
        },
        problem("setup", ProblemKind::NoCi),
        Problem {
            waiting: vec![crate::runners::Waiting {
                id: 1,
                name: "deploy".into(),
                ..Default::default()
            }],
            ..problem("wait", ProblemKind::Waiting)
        },
    ];
    let runner = |online| Runner {
        name: "r".into(),
        online,
        ..Default::default()
    };
    s.runners = vec![
        Place {
            name: "a/b".into(),
            kind: PlaceKind::Repository,
            read_at: "2026-10-31T08:00:00.000Z".into(),
            runners: vec![runner(false), runner(false)],
            total: 2,
            ..Default::default()
        },
        Place {
            name: "a".into(),
            kind: PlaceKind::Organization,
            read_at: "2026-10-31T08:00:00.000Z".into(),
            runners: vec![runner(false)],
            total: 1,
            ..Default::default()
        },
    ];
    s.actions_storage = vec![crate::actions_storage::Storage {
        repo: "a/b".into(),
        read_at: "2026-10-31T08:00:00.000Z".into(),
        cache_bytes: crate::actions_storage::NEAR,
        ..Default::default()
    }];
    s
}
fn texts(b: &Brief) -> Vec<(&str, bool)> {
    b.lines.iter().map(|l| (l.text.as_str(), l.new)).collect()
}
fn latest(s: &Snapshot) -> &Brief {
    s.brief.latest.as_ref().unwrap()
}

#[test]
fn a_brief_time_is_hours_and_minutes_on_a_24_hour_clock() {
    for good in ["00:00", "09:00", "23:59"] {
        assert!(time_of(good).is_some(), "{good}");
    }
    for bad in [
        "", "9:00", "09:0", "0900", "09.00", "24:00", "09:60", "09:00:00", "ab:cd", " 9:00",
    ] {
        assert!(time_of(bad).is_none(), "{bad}");
    }
}

#[test]
fn a_brief_is_due_once_a_day_once_its_time_has_come() {
    let day = "2026-10-31";
    assert_eq!(due("", "09:00", at(day, "08:59")), None);
    assert_eq!(due("", "09:00", at(day, "09:00")), Some(day.into()));
    assert_eq!(due("", "09:00", at(day, "23:30")), Some(day.into()));
    assert_eq!(
        due("2026-10-30", "09:00", at(day, "09:00")),
        Some(day.into())
    );
    // Once that day, and not again if the clock goes back a day.
    assert_eq!(due(day, "09:00", at(day, "10:00")), None);
    assert_eq!(due(day, "09:00", at("2026-10-30", "10:00")), None);
    // Not at a time that isn't one.
    assert_eq!(due("", "9", at(day, "10:00")), None);
}

/// The part's Done when, without the engine: a day with changes gives one
/// brief, in Needs you and in Bit's conversation, and a quiet day gives
/// none.
#[test]
fn a_day_with_changes_gives_one_brief_and_a_quiet_day_none() {
    let mut s = busy();
    let day = "2026-10-31";
    assert!(!look(&mut s, at(day, "08:59"), now()));
    assert_eq!(s.brief, Briefs::default());
    assert!(look(&mut s, at(day, "09:00"), now()));
    let b = latest(&s).clone();
    assert_eq!(
        (b.day.as_str(), b.at.as_str()),
        (day, "2026-10-31T09:00:00.000Z")
    );
    assert_eq!(
        b.text,
        "Here's your morning brief! 1 pull request is clear to merge, 4 CI problems are open and 4 notes are about runners or storage."
    );
    assert_eq!(
        texts(&b),
        [
            ("b#1: Clear to merge at aaaaaaa against main at ccccccc.", true),
            ("CI is failing on main in a/b since 2222222: build.", true),
            ("CI is failing on a/b#7 at 1111111: build.", true),
            ("b#1's checks ran without main's latest commit, 3333333.", true),
            ("No CI runs on the pull requests of a/b.", true),
            (crate::problems::waiting_text("a/b", &s.problems[4].waiting).as_str(), true),
            ("a/b's 2 self-hosted runners are all offline.", true),
            ("The a organization's one self-hosted runner is offline.", true),
            ("a/b's caches hold 9.0 GB, near GitHub's usual 10 GB limit, so GitHub may soon drop the ones used least.", true),
        ]
    );
    assert_eq!(
        b.lines
            .iter()
            .map(|l| l.pr_id.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("a/b#1"),
            None,
            Some("a/b#7"),
            Some("a/b#1"),
            None,
            None,
            None,
            None,
            None
        ]
    );
    assert_eq!((b.settled, b.open), (0, true));
    // In Needs you first, told once, and in Bit's conversation with its
    // lines.
    s.problems.push(Problem {
        alerts: 1,
        ..problem("other", ProblemKind::DefaultBranch)
    });
    let all = items(&s);
    assert_eq!(
        (all[0].kind, all[0].key.as_str(), all[0].reason.as_str()),
        (AttentionKind::Brief, "brief:2026-10-31", b.text.as_str())
    );
    s.problems.pop();
    assert_eq!(notice(&s, all[0].clone()).title, "Your morning brief");
    let (notices, _) = announce(&mut s);
    let fresh: Vec<Attention> = notices.into_iter().map(|n| n.item).collect();
    assert!(fresh.iter().any(|a| a.kind == AttentionKind::Brief));
    crate::buddy::observe(&mut s, &fresh);
    let notes: Vec<_> = s
        .buddy_feed
        .notes
        .iter()
        .filter(|n| n.kind == NoteKind::Brief)
        .collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].text, b.text);
    assert_eq!(notes[0].key.as_deref(), Some("brief:2026-10-31"));
    assert_eq!(
        notes[0]
            .lines
            .iter()
            .map(|l| (l.text.as_str(), l.pr_id.as_deref(), l.key.as_deref(), l.new))
            .collect::<Vec<_>>(),
        b.lines
            .iter()
            .map(|l| (l.text.as_str(), l.pr_id.as_deref(), None, true))
            .collect::<Vec<_>>()
    );
    assert!(!announce(&mut s)
        .0
        .iter()
        .any(|n| n.item.kind == AttentionKind::Brief));
    // Once a day.
    assert!(!look(&mut s, at(day, "18:00"), now()));
    // A quiet day: nothing new and nothing settled, so no brief, and
    // yesterday's leaves Needs you.
    assert!(look(&mut s, at("2026-11-01", "09:30"), now()));
    assert_eq!(s.brief.looked, "2026-11-01");
    assert_eq!(latest(&s).day, day);
    assert!(!latest(&s).open);
    assert!(!items(&s).iter().any(|a| a.kind == AttentionKind::Brief));
    assert!(!look(&mut s, at("2026-11-01", "23:00"), now()));
}

#[test]
fn a_brief_marks_whats_new_and_counts_what_settled() {
    let mut s = busy();
    look(&mut s, at("2026-10-31", "09:00"), now());
    // CI on main passes, and a pull request's fails.
    s.problems.retain(|p| p.id != "main");
    s.problems[0].closed_at = Some("2026-10-31T10:00:00.000Z".into());
    s.problems.push(Problem {
        pr_id: Some("a/b#8".into()),
        ..problem("eight", ProblemKind::PullRequest)
    });
    assert!(look(&mut s, at("2026-11-01", "09:00"), now()));
    let b = latest(&s);
    assert_eq!(b.day, "2026-11-01");
    assert_eq!(b.settled, 2);
    assert_eq!(
        b.text,
        "Here's your morning brief! 1 pull request is clear to merge, 3 CI problems are open and 4 notes are about runners or storage. 1 is new since the last brief. 2 things from the last brief have settled."
    );
    let new: Vec<&str> = b
        .lines
        .iter()
        .filter(|l| l.new)
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(new, ["CI is failing on a/b#8 at 1111111: build."]);
    assert_eq!(items(&s)[0].key, "brief:2026-11-01");
    // Something only settling is a change too.
    s.problems.clear();
    s.runners.clear();
    s.actions_storage.clear();
    s.merges[0].state = "behind".into();
    assert!(look(&mut s, at("2026-11-02", "09:00"), now()));
    let b = latest(&s);
    assert!(b.lines.is_empty());
    assert_eq!(
        b.text,
        "Here's your morning brief! Nothing is clear to merge, stuck or failing. 8 things from the last brief have settled."
    );
    // Then nothing to say.
    assert!(look(&mut s, at("2026-11-03", "09:00"), now()));
    assert_eq!(latest(&s).day, "2026-11-02");
    // Each line new, but not on the first brief.
    let mut s = busy();
    s.brief.latest = Some(Brief {
        lines: vec![line("old".into(), "Old.".into(), None)],
        ..Default::default()
    });
    look(&mut s, at("2026-10-31", "09:00"), now());
    assert!(latest(&s).text.ends_with(
        "All of it is new since the last brief. 1 thing from the last brief has settled."
    ));
    let (mut one, _) = r::ready();
    one.brief.latest = Some(Brief::default());
    look(&mut one, at("2026-10-31", "09:00"), now());
    assert_eq!(
        latest(&one).text,
        "Here's your morning brief! 1 pull request is clear to merge. It is new since the last brief."
    );
}

#[test]
fn nothing_to_say_on_the_first_day_sends_nothing() {
    let mut s = Snapshot::default();
    assert!(look(&mut s, at("2026-10-31", "09:00"), now()));
    assert_eq!(s.brief.looked, "2026-10-31");
    assert_eq!(s.brief.latest, None);
    assert!(items(&s).is_empty());
}

#[test]
fn no_brief_while_bit_or_the_brief_is_off_or_orbit_cant_sync() {
    let day = "2026-10-31";
    for off in [
        |s: &mut Snapshot| s.settings.buddy.enabled = false,
        |s: &mut Snapshot| s.settings.buddy.brief = false,
        |s: &mut Snapshot| s.poll_failures = 1,
    ] {
        let mut s = busy();
        off(&mut s);
        assert!(!look(&mut s, at(day, "09:00"), now()));
        assert_eq!(s.brief, Briefs::default());
    }
    // At the time you choose.
    let mut s = busy();
    s.settings.buddy.brief_at = "17:30".into();
    assert!(!look(&mut s, at(day, "17:29"), now()));
    assert!(look(&mut s, at(day, "17:30"), now()));
    assert!(latest(&s).open);
}

/// Part 4b's Done when: the brief comes at your time on a server set to
/// another zone. Whatever this computer's clock says, Orbit looks at the
/// time in the zone you chose.
#[test]
fn the_brief_comes_at_your_time_in_the_zone_you_chose() {
    let utc = |at: &str| {
        DateTime::parse_from_rfc3339(at)
            .unwrap()
            .with_timezone(&Utc)
    };
    let look_at = |s: &mut Snapshot, now: &str| {
        let now = utc(now);
        let local = crate::clock::local(&s.settings.buddy.time_zone, now);
        look(s, local, now)
    };
    let mut s = busy();
    s.settings.buddy.time_zone = "America/New_York".into();
    // 9:00 in New York is 14:00 in UTC in November: not at 9:00 UTC.
    assert!(!look_at(&mut s, "2026-11-02T09:00:00Z"));
    assert!(!look_at(&mut s, "2026-11-02T13:59:00Z"));
    assert!(look_at(&mut s, "2026-11-02T14:00:00Z"));
    assert_eq!(latest(&s).day, "2026-11-02");
    // A zone a day ahead of UTC gives that day's brief the evening before,
    // by UTC.
    let mut s = busy();
    s.settings.buddy.time_zone = "Pacific/Auckland".into();
    assert!(!look_at(&mut s, "2026-11-01T19:59:00Z"));
    assert!(look_at(&mut s, "2026-11-01T20:00:00Z"));
    assert_eq!(latest(&s).day, "2026-11-02");
}

/// A read the brief sums up failed: no brief from what Orbit kept from
/// before, and the day isn't used up, so the brief comes once a read works
/// that day.
#[test]
fn a_failed_read_holds_the_brief_until_a_read_works() {
    let day = "2026-10-31";
    let fails: [fn(&mut Snapshot); 4] = [
        |s| s.poll_failures = 1,
        // One repository didn't sync, so its pull requests are as before.
        |s| s.github_error = Some("Could not sync a/b: gh: Bad Gateway (HTTP 502)".into()),
        |s| s.ci[0].error = Some("gh: Bad Gateway (HTTP 502)".into()),
        |s| {
            s.merges[0].error =
                Some("Orbit couldn't ask GitHub whether it merges: Bad Gateway.".into())
        },
    ];
    for fail in fails {
        let mut s = busy();
        fail(&mut s);
        assert!(!look(&mut s, at(day, "09:00"), now()));
        assert_eq!(s.brief, Briefs::default());
        s.poll_failures = 0;
        s.github_error = None;
        s.ci[0].error = None;
        s.merges[0].error = None;
        assert!(look(&mut s, at(day, "09:05"), now()));
        assert_eq!((latest(&s).day.as_str(), latest(&s).open), (day, true));
    }
    // A failed question about whether an older head merges no longer
    // counts: here the author pushed again and its checks fail, so Orbit
    // won't ask GitHub about the new head until they pass.
    let mut s = busy();
    s.merges[0].error = Some("Orbit couldn't ask GitHub whether it merges: Bad Gateway.".into());
    assert!(!look(&mut s, at(day, "09:00"), now()));
    let pushed = "d".repeat(40);
    s.prs[0].head_sha = pushed.clone();
    let pull = r::pull(&mut s);
    pull.sha = pushed.clone();
    pull.state = crate::ci::CiState::Failing;
    pull.passing = 0;
    s.problems.push(Problem {
        pr_id: Some("a/b#1".into()),
        sha: pushed,
        ..problem("one", ProblemKind::PullRequest)
    });
    assert!(look(&mut s, at(day, "09:05"), now()));
    assert!(latest(&s)
        .lines
        .iter()
        .any(|l| l.text == "CI is failing on b#1 at ddddddd: build."));
    // Nor does one about another base, or an older head once the new one
    // is reviewed and passes, which Orbit asks about on its next pass.
    let failed = |s: &mut Snapshot| {
        s.merges[0].error =
            Some("Orbit couldn't ask GitHub whether it merges: Bad Gateway.".into());
    };
    let mut s = busy();
    failed(&mut s);
    s.merges[0].base_sha = "e".repeat(40);
    assert!(look(&mut s, at(day, "09:00"), now()));
    let mut s = busy();
    failed(&mut s);
    let pushed = "d".repeat(40);
    s.prs[0].head_sha = pushed.clone();
    s.reviews[0].head_sha = pushed.clone();
    r::pull(&mut s).sha = pushed;
    let waits: Vec<WaitKind> = crate::readiness::of(&s, &s.prs[0])
        .waits
        .iter()
        .map(|w| w.kind)
        .collect();
    assert_eq!(waits, [WaitKind::Merge]);
    assert!(look(&mut s, at(day, "09:00"), now()));
    // Nor one about this head and base while something else is in the way
    // now, such as checks that failed when they ran again, or a loop you
    // stopped.
    let mut s = busy();
    failed(&mut s);
    let pull = r::pull(&mut s);
    pull.state = crate::ci::CiState::Failing;
    pull.passing = 0;
    assert!(look(&mut s, at(day, "09:00"), now()));
    let mut s = busy();
    failed(&mut s);
    s.loops.push(LoopControl {
        pr_id: "a/b#1".into(),
        stopped: true,
        ..Default::default()
    });
    let answer = crate::readiness::of(&s, &s.prs[0]);
    assert_eq!(
        (answer.waits[0].kind, answer.base_sha.as_str()),
        (WaitKind::Loop, s.merges[0].base_sha.as_str())
    );
    assert!(look(&mut s, at(day, "09:00"), now()));
    // Nor one about another pull request at the same commits.
    let mut s = busy();
    failed(&mut s);
    s.merges[0].pr_id = "a/b#9".into();
    assert!(look(&mut s, at(day, "09:00"), now()));
}

#[test]
fn runners_and_storage_are_in_it_only_while_watch_ci_is_on() {
    let mut s = busy();
    s.settings.watch_ci = false;
    let keys: Vec<String> = lines(&s).into_iter().map(|l| l.key).collect();
    assert!(keys.contains(&"runs:wait".to_string()));
    assert!(!keys
        .iter()
        .any(|k| k.starts_with("offline:") || k.starts_with("caches:")));
    // Only runners Orbit sees, of a repository or an organization, all
    // offline; caches only once near the limit.
    let mut s = busy();
    s.runners[0].runners[1].online = true;
    s.runners[1].unseen = Some("Orbit can't see them.".into());
    s.actions_storage[0].cache_bytes = crate::actions_storage::NEAR - 1;
    s.actions_storage.push(crate::actions_storage::Storage {
        repo: "a/c".into(),
        cache_bytes: crate::actions_storage::NEAR,
        unseen: Some(crate::actions_storage::unseen("a/c")),
        ..Default::default()
    });
    let mut person = s.runners[1].clone();
    person.kind = PlaceKind::Person;
    person.unseen = None;
    s.runners.push(person);
    s.runners.push(Place {
        name: "a/c".into(),
        ..Default::default()
    });
    let keys: Vec<String> = lines(&s).into_iter().map(|l| l.key).collect();
    assert!(!keys
        .iter()
        .any(|k| k.starts_with("offline:") || k.starts_with("caches:")));
    s.runners[0].runners[1].online = false;
    let keys: Vec<String> = lines(&s).into_iter().map(|l| l.key).collect();
    assert!(keys.contains(&"offline:a/b".to_string()));
    s.runners[0].runners.truncate(1);
    // As GitHub counts them, or as read before Orbit kept the count.
    for total in [1, 0] {
        s.runners[0].total = total;
        let texts: Vec<String> = lines(&s).into_iter().map(|l| l.text).collect();
        assert!(texts.contains(&"a/b's one self-hosted runner is offline.".to_string()));
    }
    // Only the runners Orbit read are said to be offline, and a read that
    // failed may be out of date.
    let mut s = busy();
    s.runners[0].total = 3;
    s.runners[1].total = 101;
    s.runners[1].error = Some("gh: Bad Gateway (HTTP 502)".into());
    s.actions_storage[0].error = Some("gh: Bad Gateway (HTTP 502)".into());
    let notes: Vec<String> = lines(&s)
        .into_iter()
        .filter(|l| l.key.starts_with("offline:") || l.key.starts_with("caches:"))
        .map(|l| l.text)
        .collect();
    assert_eq!(
        notes,
        [
            "The 2 self-hosted runners Orbit read of a/b's 3 are all offline.",
            "The one self-hosted runner Orbit read of the a organization's 101 is offline. Orbit's latest read of them failed, so this may be out of date.",
            "a/b's caches hold 9.0 GB, near GitHub's usual 10 GB limit, so GitHub may soon drop the ones used least. Orbit's latest read of its storage failed, so this may be out of date.",
        ]
    );
    // A closed problem isn't in it.
    let mut s = busy();
    for p in &mut s.problems {
        p.closed_at = Some("2026-10-31T08:00:00.000Z".into());
    }
    assert!(!lines(&s).iter().any(|l| l.key.starts_with("problem:")));
}

/// A pull request Orbit nudged someone about, while the first thing in the
/// way is still theirs at the same head.
#[test]
fn a_stuck_pull_request_says_whose_move_it_is() {
    use crate::ops::{Ready, Thread};
    let stuck = |state: &str, turn: Turn| {
        let (mut s, _) = r::ready();
        s.merges[0].state = state.into();
        s.ops.push(Thread {
            key: "pr:a/b#1".into(),
            pr_id: Some("a/b#1".into()),
            ready: Some(Ready {
                head_sha: r::HEAD.into(),
                base: "main".into(),
                turn: Some(turn),
                since: "2026-10-30T09:00:00.000Z".into(),
                nudged: Some("2026-10-30T11:00:00.000Z".into()),
                ..Default::default()
            }),
            ..Default::default()
        });
        s
    };
    let texts = |s: &Snapshot| -> Vec<(String, String)> {
        lines(s).into_iter().map(|l| (l.key, l.text)).collect()
    };
    let key = "stuck:a/b#1:2026-10-30T09:00:00.000Z".to_string();
    assert_eq!(
        texts(&stuck("behind", Turn::Author)),
        [(
            key.clone(),
            "b#1 has waited on its author for more than 2 hours: GitHub wants it brought up to date with main first.".into()
        )]
    );
    let mut s = stuck("blocked", Turn::Person);
    assert_eq!(
        texts(&s),
        [(
            key.clone(),
            "b#1 has waited on a person for more than a day: GitHub says a rule blocks merging it, such as a required review.".into()
        )]
    );
    // Its reviews say more.
    r::reviewed_by(
        &mut s,
        1,
        vec![r::person("bob", crate::readiness::Stand::Asked, "")],
    );
    assert_eq!(
        texts(&s)[0].1,
        "b#1 has waited on a person for more than a day: main needs 1 approval of aaaaaaa and it has none: bob hasn't reviewed aaaaaaa yet."
    );
    let looked = look(&mut s, at("2026-10-31", "09:00"), now());
    assert!(looked);
    assert_eq!(
        latest(&s).text,
        "Here's your morning brief! 1 pull request is stuck."
    );
    // Not before anyone was nudged, once it is someone else's move or
    // another head, nor for a person's move Needs you doesn't nudge about.
    let none = |s: Snapshot| assert!(lines(&s).is_empty(), "{:?}", lines(&s));
    // Only the first thing in the way counts, as on the comment: here
    // Orbit's own review, though the failing checks after it are the
    // author's.
    let mut s = stuck("clean", Turn::Author);
    s.reviews.clear();
    let pull = r::pull(&mut s);
    pull.state = crate::ci::CiState::Failing;
    pull.passing = 0;
    pull.failing = vec![crate::ci::Check {
        name: "build".into(),
        ..Default::default()
    }];
    let turns: Vec<(WaitKind, Turn)> = crate::readiness::of(&s, &s.prs[0])
        .waits
        .iter()
        .map(|w| (w.kind, w.turn))
        .collect();
    assert_eq!(
        turns,
        [
            (WaitKind::Review, Turn::Working),
            (WaitKind::Checks, Turn::Author)
        ]
    );
    none(s);
    let mut s = stuck("behind", Turn::Author);
    s.ops[0].ready.as_mut().unwrap().nudged = None;
    none(s);
    none(stuck("behind", Turn::Person));
    none(stuck("blocked", Turn::Author));
    let mut s = stuck("behind", Turn::Author);
    s.ops[0].ready.as_mut().unwrap().head_sha = "f".repeat(40);
    none(s);
    let mut s = stuck("behind", Turn::Author);
    s.ops[0].ready.as_mut().unwrap().clear = true;
    none(s);
    let mut s = stuck("blocked", Turn::Person);
    s.ops[0].key = "pr:a/b#2".into();
    none(s);
    let mut s = stuck("blocked", Turn::Person);
    s.ops[0].ready = None;
    none(s);
    // A loop that needs you is a person's move, but already in Needs you.
    let mut s = stuck("clean", Turn::Person);
    s.loops.push(LoopControl {
        pr_id: "a/b#1".into(),
        stopped: true,
        ..Default::default()
    });
    assert_eq!(
        crate::readiness::of(&s, &s.prs[0]).waits[0].turn,
        Turn::Person
    );
    none(s);
}

#[test]
fn you_can_mark_a_brief_read_and_turning_it_off_does_too() {
    let mut s = busy();
    look(&mut s, at("2026-10-31", "09:00"), now());
    assert!(!close(&mut s, "2026-10-30"));
    assert!(latest(&s).open);
    assert!(close(&mut s, "2026-10-31"));
    assert!(!latest(&s).open);
    assert!(!close(&mut s, "2026-10-31"));
    assert!(items(&s).is_empty() || items(&s)[0].kind != AttentionKind::Brief);
    for off in [
        |s: &mut Snapshot| s.settings.buddy.enabled = false,
        |s: &mut Snapshot| s.settings.buddy.brief = false,
    ] {
        let mut s = busy();
        look(&mut s, at("2026-10-31", "09:00"), now());
        settings_changed(&mut s);
        assert!(latest(&s).open);
        off(&mut s);
        settings_changed(&mut s);
        assert!(!latest(&s).open);
    }
    let mut s = Snapshot::default();
    s.settings.buddy.brief = false;
    settings_changed(&mut s);
    assert_eq!(s.brief.latest, None);
}

#[test]
fn a_brief_sounds_like_bit() {
    let (mut s, _) = r::ready();
    for (tone, text) in [
        (
            "calm",
            "Your morning brief. 1 pull request is clear to merge.",
        ),
        (
            "dry",
            "Morning brief. 1 pull request is clear to merge. That's the lot.",
        ),
    ] {
        s.settings.buddy.tone = tone.into();
        s.brief = Briefs::default();
        look(&mut s, at("2026-10-31", "09:00"), now());
        assert_eq!(latest(&s).text, text);
    }
    // And it doesn't worry Bit.
    let all = items(&s);
    assert_eq!(all[0].kind, AttentionKind::Brief);
    assert_ne!(
        crate::buddy::mood(&s, &all, &[]),
        crate::buddy::Mood::Worried
    );
}

/// Clear to merge first, then stuck, CI problems, and runners and storage,
/// each keyed by what makes it the same thing next time.
#[test]
fn a_brief_lists_clear_then_stuck_then_problems_then_notes() {
    use crate::ops::{Ready, Thread};
    let mut s = busy();
    // Another pull request, first in the inbox, stuck on its author.
    let mut two = s.prs[0].clone();
    two.number = 2;
    two.id = "a/b#2".into();
    s.prs.insert(0, two);
    let mut review = s.reviews[0].clone();
    review.id = "two".into();
    review.pr_id = "a/b#2".into();
    s.reviews.push(review);
    let mut pull = s.ci[0].pulls[0].clone();
    pull.number = 2;
    s.ci[0].pulls.push(pull);
    s.merges.push(crate::readiness::Merge {
        pr_id: "a/b#2".into(),
        state: "behind".into(),
        ..s.merges[0].clone()
    });
    s.ops.push(Thread {
        key: "pr:a/b#2".into(),
        pr_id: Some("a/b#2".into()),
        ready: Some(Ready {
            head_sha: r::HEAD.into(),
            base: "main".into(),
            turn: Some(Turn::Author),
            since: "2026-10-30T09:00:00.000Z".into(),
            nudged: Some("2026-10-30T11:00:00.000Z".into()),
            ..Default::default()
        }),
        ..Default::default()
    });
    let keys: Vec<String> = lines(&s).into_iter().map(|l| l.key).collect();
    assert_eq!(
        keys,
        [
            format!("clear:a/b#1:{}:{}", r::HEAD, "c".repeat(40)).as_str(),
            "stuck:a/b#2:2026-10-30T09:00:00.000Z",
            "problem:main",
            "problem:pull",
            "problem:base",
            "problem:setup",
            "runs:wait",
            "offline:a/b",
            "offline:a",
            "caches:a/b",
        ]
    );
    // A problem with no branch or pull request named says what it can.
    let s = Snapshot {
        problems: vec![
            Problem {
                branch: String::new(),
                ..problem("main", ProblemKind::DefaultBranch)
            },
            problem("pull", ProblemKind::PullRequest),
        ],
        ..Default::default()
    };
    let texts: Vec<String> = lines(&s).into_iter().map(|l| l.text).collect();
    assert_eq!(
        texts,
        [
            "CI is failing on the default branch in a/b since 2222222: build.",
            "CI is failing on a/b at 1111111: build.",
        ]
    );
    // GitHub's names don't mind case, so neither does what is new.
    let mut s = busy();
    look(&mut s, at("2026-10-31", "09:00"), now());
    s.runners[0].name = "A/B".into();
    s.actions_storage[0].repo = "A/B".into();
    s.problems.retain(|p| p.id != "main");
    assert!(look(&mut s, at("2026-11-01", "09:00"), now()));
    assert_eq!(latest(&s).settled, 1);
    assert!(latest(&s).lines.iter().all(|l| !l.new));
}

#[test]
fn what_orbit_keeps_about_briefs_saves_and_loads() {
    let mut s = busy();
    look(&mut s, at("2026-10-31", "09:00"), now());
    let saved = serde_json::to_value(&s.brief).unwrap();
    assert_eq!(saved["looked"], "2026-10-31");
    assert_eq!(saved["latest"]["lines"][0]["new"], true);
    assert_eq!(saved["latest"]["lines"][1].get("prId"), None);
    let back: Briefs = serde_json::from_value(saved).unwrap();
    assert_eq!(back, s.brief);
    // An older state has none.
    let old: Snapshot = serde_json::from_str("{}").unwrap();
    assert_eq!(old.brief, Briefs::default());
    assert_eq!(
        serde_json::to_value(Briefs::default()).unwrap(),
        serde_json::json!({})
    );
}
