use super::*;
use crate::ci::Check;
use crate::types::{PullRequest, Snapshot};
use chrono::TimeZone;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap()
}
fn check(name: &str) -> Check {
    Check {
        name: name.into(),
        url: "https://github.com/a/b/actions/runs/1/job/2".into(),
    }
}
fn sha(c: char) -> String {
    c.to_string().repeat(40)
}
/// A default branch failing these checks at `at`.
fn branch(failing: &[&str], at: char) -> Problem {
    Problem {
        id: "p1".into(),
        key: "ci:a/b".into(),
        kind: ProblemKind::DefaultBranch,
        repo: "a/b".into(),
        branch: "main".into(),
        sha: sha(at),
        since_sha: sha(at),
        failing: failing.iter().map(|n| check(n)).collect(),
        opened_at: stamp(t0()),
        ..Default::default()
    }
}
fn pull(id: &str, failing: &[&str], at: char, opened: DateTime<Utc>) -> Problem {
    Problem {
        id: id.into(),
        key: "ci:a/b#12".into(),
        kind: ProblemKind::PullRequest,
        repo: "a/b".into(),
        pr_id: Some("a/b#12".into()),
        branch: "fix-it".into(),
        sha: sha(at),
        since_sha: sha(at),
        failing: failing.iter().map(|n| check(n)).collect(),
        opened_at: stamp(opened),
        ..Default::default()
    }
}
fn pass(p: &mut Problem, at: char, when: DateTime<Utc>) {
    p.closed_at = Some(stamp(when));
    p.closed_reason = Some(format!("Passed at {}.", &sha(at)[..7]));
    p.closed_sha = sha(at);
}
/// Sends what the thread needs as GitHub would take it, and returns what
/// was sent.
fn send(
    threads: &mut [Thread],
    journal: &mut Vec<Write>,
    p: &Problem,
    may: bool,
    now: DateTime<Utc>,
) -> Option<Planned> {
    tell(threads, journal, Some(p), may, now)
}
/// The same, for a thread that may have no problem.
fn tell(
    threads: &mut [Thread],
    journal: &mut Vec<Write>,
    p: Option<&Problem>,
    may: bool,
    now: DateTime<Utc>,
) -> Option<Planned> {
    let planned = next(&threads[0], p, may, now)?;
    let id = record(journal, &planned, now);
    let posted = Posted {
        id: 7,
        number: Some(
            if planned.action == Action::PostComment || planned.action == Action::EditComment {
                12
            } else {
                45
            },
        ),
        url: "https://github.com/a/b/issues/45".into(),
        updated_at: "2026-10-29T12:00:00Z".into(),
        body: planned.body.clone(),
    };
    settle(threads, journal, &id, &planned, Ok(posted), now);
    Some(planned)
}

/// A failing default branch gets one issue, with an item for each failing
/// check, and nothing more while nothing changes.
#[test]
fn a_failing_default_branch_gets_one_issue() {
    let p = branch(&["CI / Rust", "CI / Lint"], '1');
    let mut threads = vec![];
    let mut journal = vec![];
    assert!(refresh(&mut threads, std::slice::from_ref(&p), t0()));
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].key, "issue:p1");
    assert_eq!(threads[0].kind, ThreadKind::Issue);
    let open = send(&mut threads, &mut journal, &p, true, t0()).unwrap();
    assert_eq!(open.action, Action::OpenIssue);
    assert_eq!(open.title, "CI is failing on main");
    assert!(open.body.contains("2 checks fail on `main` at `1111111`."));
    assert!(open.body.contains("- **I1** · `CI / Rust` · failing at `1111111` · [details](https://github.com/a/b/actions/runs/1/job/2)"));
    assert!(open
        .body
        .contains("- **I2** · `CI / Lint` · failing at `1111111`"));
    let hidden = open.body.lines().last().unwrap();
    assert!(hidden.starts_with("<!-- orbit-ops thread=issue:p1 {"));
    assert!(hidden.contains("\"version\":1"));
    assert!(hidden.contains("\"id\":\"I2\""));
    assert_eq!(threads[0].number, Some(45));
    assert_eq!(journal[0].state, WriteState::Done);
    assert!(!refresh(&mut threads, std::slice::from_ref(&p), t0()));
    assert_eq!(next(&threads[0], Some(&p), true, t0()), None);
}

/// Runs waiting for a runner are a person's to fix, so no agent hears of
/// them.
#[test]
fn runs_waiting_for_a_runner_get_no_thread() {
    let mut problems = vec![];
    let run = crate::runners::Waiting {
        id: 1,
        name: "CI".into(),
        ..Default::default()
    };
    crate::problems::see_waiting(&mut problems, "a/b", &[run], true, t0());
    let mut threads = vec![];
    assert!(!refresh(&mut threads, &problems, t0()));
    assert!(threads.is_empty());
}

/// Items keep their numbers: a check that stops failing keeps its item, and
/// one that fails again gets it back with when it started failing again.
#[test]
fn items_keep_their_numbers() {
    let mut p = branch(&["A"], '1');
    let mut threads = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    p.sha = sha('2');
    p.failing = vec![check("B")];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    p.sha = sha('3');
    p.failing = vec![check("A"), check("B")];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let items = &threads[0].items;
    let line = |i: usize| (items[i].number, items[i].check.as_str(), items[i].failing);
    assert_eq!(line(0), (1, "A", true));
    assert_eq!(line(1), (2, "B", true));
    assert_eq!(items[0].since, sha('3'), "failing again since 3");
    assert_eq!(items[1].since, sha('2'), "still failing since 2");
    let body = issue_body(&threads[0], &p, false);
    assert!(body.contains("- **I2** · `B` · failing since `2222222`, still at `3333333`"));
    p.failing = vec![check("B")];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let body = issue_body(&threads[0], &p, false);
    // Failing items come first.
    let b = body.find("**I2**").unwrap();
    let a = body
        .find("**I1** · `A` · not failing at `3333333`")
        .unwrap();
    assert!(b < a);
}

/// A green default branch's issue says so at once, and closes with a note
/// only once it has stayed green for an hour, as it then can't come back as
/// the same problem.
#[test]
fn an_issue_closes_with_a_note_after_an_hour_of_green() {
    let mut p = branch(&["A"], '1');
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    send(&mut threads, &mut journal, &p, true, t0());
    let green = t0() + Duration::minutes(5);
    pass(&mut p, '2', green);
    refresh(&mut threads, std::slice::from_ref(&p), green);
    let edit = send(&mut threads, &mut journal, &p, true, green).unwrap();
    assert_eq!(edit.action, Action::EditIssue);
    assert!(edit.body.contains(
        "Every check passes on `main` at `2222222`. Orbit closes this issue once it stays green for an hour."
    ));
    assert!(edit.body.contains("- **I1** · `A` · passes at `2222222`"));
    let soon = green + Duration::minutes(59);
    assert_eq!(next(&threads[0], Some(&p), true, soon), None);
    let later = green + CLOSE_AFTER;
    let note = send(&mut threads, &mut journal, &p, true, later).unwrap();
    assert_eq!(note.action, Action::NoteOnIssue);
    assert!(note.body.starts_with(
        "Every check passed on `main` at `2222222` and stayed green for an hour, so Orbit is closing this issue."
    ));
    assert_eq!(note.marker, note_tag("issue:p1", &stamp(green)));
    let close = send(&mut threads, &mut journal, &p, true, later).unwrap();
    assert_eq!(close.action, Action::CloseIssue);
    assert!(close.completed);
    assert!(close.body.contains("and stayed green for an hour."));
    assert!(threads[0].closed);
    assert_eq!(next(&threads[0], Some(&p), true, later), None);
    let kinds: Vec<Action> = journal.iter().map(|w| w.action).collect();
    assert_eq!(
        kinds,
        [
            Action::OpenIssue,
            Action::EditIssue,
            Action::NoteOnIssue,
            Action::CloseIssue
        ]
    );
}

/// One that closes for another reason closes its issue straight away, as
/// not planned, and reopens it if the same problem comes back.
#[test]
fn an_issue_closed_for_another_reason_reopens_if_it_comes_back() {
    let mut p = branch(&["A"], '1');
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    send(&mut threads, &mut journal, &p, true, t0());
    p.closed_at = Some(stamp(t0()));
    p.closed_reason = Some("Orbit stopped watching CI.".into());
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let note = send(&mut threads, &mut journal, &p, true, t0()).unwrap();
    assert!(note
        .body
        .starts_with("Orbit is closing this issue. Orbit stopped watching CI."));
    let close = send(&mut threads, &mut journal, &p, true, t0()).unwrap();
    assert_eq!(close.action, Action::CloseIssue);
    assert!(!close.completed);
    p.closed_at = None;
    p.closed_reason = None;
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let reopen = send(&mut threads, &mut journal, &p, true, t0()).unwrap();
    assert_eq!(reopen.action, Action::ReopenIssue);
    assert!(!threads[0].closed);
}

/// Bit posts nothing new without Do it, or Ask first and your yes, but
/// keeps what it already posted true.
#[test]
fn the_rule_decides_what_is_posted_but_not_what_is_kept_true() {
    let mut p = pull("p1", &["A"], '1', t0());
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    assert!(!rule_allows("leave_it", true, true));
    assert!(!rule_allows("ask_first", false, true));
    assert!(!rule_allows("do_it", true, false), "Bit is off");
    assert!(rule_allows("ask_first", true, true));
    assert_eq!(next(&threads[0], Some(&p), false, t0()), None);
    let post = send(&mut threads, &mut journal, &p, true, t0()).unwrap();
    assert_eq!(post.action, Action::PostComment);
    assert_eq!(post.number, Some(12));
    assert!(post
        .body
        .contains("1 check fails on this pull request at `1111111`."));
    pass(&mut p, '2', t0());
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let edit = send(&mut threads, &mut journal, &p, false, t0()).unwrap();
    assert_eq!(edit.action, Action::EditComment);
    assert_eq!(edit.comment_id, Some(7));
    assert!(edit
        .body
        .contains("Every check passes at `2222222`. Nothing is left to fix here."));
}

/// A pull request keeps one comment across its problems, and its item
/// numbers are never reused; a closed pull request isn't edited.
#[test]
fn a_pull_request_keeps_one_comment_and_its_numbers() {
    let mut first = pull("p1", &["A"], '1', t0());
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&first), t0());
    send(&mut threads, &mut journal, &first, true, t0());
    pass(&mut first, '2', t0());
    let second = pull("p2", &["B"], '3', t0() + Duration::days(1));
    let both = [first.clone(), second.clone()];
    refresh(&mut threads, &both, t0());
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].problem_id, "p2");
    let numbers: Vec<(u32, &str, bool)> = threads[0]
        .items
        .iter()
        .map(|i| (i.number, i.check.as_str(), i.failing))
        .collect();
    assert_eq!(numbers, [(1, "A", false), (2, "B", true)]);
    let mut second = second;
    second.closed_at = Some(stamp(t0()));
    second.closed_reason = Some("The pull request closed.".into());
    refresh(&mut threads, &[first, second.clone()], t0());
    assert_eq!(next(&threads[0], Some(&second), true, t0()), None);
}

/// Pull request 12's checks pass at `at`, but `main` moved on since.
fn moved(id: &str, at: char, opened: DateTime<Utc>) -> Problem {
    Problem {
        key: "base:a/b#12".into(),
        kind: ProblemKind::BaseMoved,
        base: "main".into(),
        base_sha: sha('9'),
        ..pull(id, &[], at, opened)
    }
}
/// The item in the hidden line of a body, by its number.
fn data_item(body: &str, number: u32) -> serde_json::Value {
    let json = body
        .split_once("<!-- orbit-ops thread=pr:a/b#12 ")
        .unwrap()
        .1;
    let v: serde_json::Value = serde_json::from_str(json.split_once(" -->").unwrap().0).unwrap();
    let id = format!("I{number}");
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id.as_str())
        .unwrap()
        .clone()
}

/// A pull request whose checks passed before `main` moved on gets an item
/// on its one comment, numbered after its checks, until CI runs again.
#[test]
fn a_base_that_moved_on_is_an_item_on_the_pull_requests_comment() {
    let mut failed = pull("p1", &["A"], '1', t0());
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&failed), t0());
    send(&mut threads, &mut journal, &failed, true, t0());
    pass(&mut failed, '2', t0());
    let mut base = moved("p2", '2', t0() + Duration::minutes(5));
    refresh(&mut threads, &[failed.clone(), base.clone()], t0());
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].problem_id, "p2");
    let items: Vec<(u32, ItemKind, &str, bool)> = threads[0]
        .items
        .iter()
        .map(|i| (i.number, i.kind, i.check.as_str(), i.failing))
        .collect();
    assert_eq!(
        items,
        [
            (1, ItemKind::Check, "A", false),
            (2, ItemKind::BaseMoved, "main", true)
        ]
    );
    let body = comment_body(&threads[0], Some(&base));
    assert!(body.contains("\n\nEvery check passes at `2222222`, but `main` has moved on since they began. Merge `main` in and let CI run again, so they run with its latest changes.\n\n"), "{body}");
    assert!(body.contains(
        "- **I2** · `main` moved on since the checks at `2222222` began · merge it in and let CI run again\n- **I1** · `A` · passes at `2222222`\n"
    ));
    let item = data_item(&body, 2);
    assert_eq!(item["kind"], "base_moved");
    assert_eq!(item["base"], "main");
    assert_eq!(item["failing"], true);
    assert!(item.get("check").is_none());
    assert!(data_item(&body, 1).get("kind").is_none(), "as before");
    let planned = send(&mut threads, &mut journal, &base, true, t0()).unwrap();
    assert_eq!(planned.action, Action::EditComment);
    let posted = event_text(
        &threads,
        std::slice::from_ref(&base),
        &Event::Posted(threads[0].key.clone()),
    );
    assert_eq!(
        posted.unwrap().2,
        "Orbit told the agent on a/b #12 that main moved on since its checks began."
    );

    // Agents answer it like any item, but CI settles it, not a check.
    on_github(&mut threads[0]);
    let heard = hear(
        &mut threads[0],
        &[reply(20, "Fixed I2 in abc1234\nDeclined I1: no")],
        None,
    );
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].check, "main moved on");
    assert!(to_verify(&threads, &[base.clone()]).is_empty());
    assert!(comment_body(&threads[0], Some(&base))
        .contains("merge it in and let CI run again · said to be fixed in `abc1234`; Orbit is waiting for its checks there"));

    // CI runs again with main's latest.
    pass(&mut base, '3', t0());
    refresh(&mut threads, &[failed.clone(), base.clone()], t0());
    let body = comment_body(&threads[0], Some(&base));
    assert!(body.contains("Every check passes at `3333333`. Nothing is left to fix here."));
    assert!(body.contains(
        "- **I1** · `A` · passes at `3333333`\n- **I2** · `main` moved on · done at `3333333`\n"
    ));

    // main moves on again later: the same item, from the start, named for
    // the base as it is now.
    let again = Problem {
        base: "trunk".into(),
        ..moved("p3", '3', t0() + Duration::hours(1))
    };
    refresh(&mut threads, &[failed, base, again.clone()], t0());
    let i2 = &threads[0].items[1];
    assert_eq!(
        (i2.number, i2.failing, i2.since.as_str()),
        (2, true, sha('3').as_str())
    );
    assert_eq!(i2.answer, None);
    assert_eq!(i2.check, "trunk");
    assert_eq!(threads[0].items.len(), 2);
}

/// When the checks fail instead, the failure is what the comment is about,
/// and the base's item is over.
#[test]
fn a_failure_takes_over_from_a_base_that_moved_on() {
    let mut base = moved("p1", '1', t0());
    let mut threads = vec![];
    refresh(&mut threads, std::slice::from_ref(&base), t0());
    base.closed_at = Some(stamp(t0()));
    base.closed_reason = Some("Its checks fail at 2222222 now.".into());
    let failing = pull("p2", &["A"], '2', t0() + Duration::minutes(5));
    refresh(&mut threads, &[base, failing.clone()], t0());
    assert_eq!(threads[0].problem_id, "p2");
    let items: Vec<(u32, ItemKind, bool)> = threads[0]
        .items
        .iter()
        .map(|i| (i.number, i.kind, i.failing))
        .collect();
    assert_eq!(
        items,
        [(1, ItemKind::BaseMoved, false), (2, ItemKind::Check, true)]
    );
    let body = comment_body(&threads[0], Some(&failing));
    assert!(body.contains("1 check fails on this pull request at `2222222`."));
    assert!(body.contains("- **I1** · `main` moved on · done at `2222222`"));
}

/// A check's name can't break out of its code span, mention anyone, or end
/// the hidden line early; only GitHub links are linked.
#[test]
fn names_and_links_from_github_stay_plain() {
    let mut p = branch(&[], '1');
    p.failing = vec![Check {
        name: "Ping @everyone `now` --> <b>".into(),
        url: "javascript:alert(1)".into(),
    }];
    let mut threads = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let body = issue_body(&threads[0], &p, false);
    assert!(body.contains("`Ping @everyone 'now' --> <b>` · failing at `1111111`\n"));
    assert!(!body.contains("javascript"));
    let hidden = body.lines().last().unwrap();
    assert!(hidden.starts_with("<!-- orbit-ops thread=issue:p1 "));
    assert_eq!(hidden.matches("-->").count(), 1);
    assert!(hidden.ends_with(" -->"));
}

/// A failed write waits longer after each failure, up to an hour;
/// something deleted from GitHub is left alone.
#[test]
fn failed_writes_wait_and_deleted_ones_are_left_alone() {
    let p = pull("p1", &["A"], '1', t0());
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let mut now = t0();
    let mut waits = vec![];
    for tries in 1..=8 {
        let planned = next(&threads[0], Some(&p), true, now).unwrap();
        assert_eq!(planned.action, Action::PostComment);
        let id = record(&mut journal, &planned, now);
        settle(
            &mut threads,
            &mut journal,
            &id,
            &planned,
            Err("HTTP 502".into()),
            now,
        );
        assert_eq!(threads[0].error.as_deref(), Some("HTTP 502"));
        let retry = time(threads[0].retry_at.as_deref().unwrap()).unwrap();
        waits.push((retry - now).num_minutes());
        assert_eq!(
            next(&threads[0], Some(&p), true, retry - Duration::seconds(1)),
            None
        );
        now = retry;
        // GitHub may have taken it, so Orbit looks first and finds nothing,
        // which doesn't count as the post going through.
        let find = next(&threads[0], Some(&p), true, now).unwrap();
        assert_eq!(find.action, Action::Find);
        looked(&mut threads, &find, Ok(None), now);
        assert!(!threads[0].unsure);
        assert_eq!(threads[0].tries, tries);
    }
    assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60]);
    send(&mut threads, &mut journal, &p, true, now);
    assert_eq!(threads[0].tries, 0);
    let mut moved = p.clone();
    moved.sha = sha('2');
    refresh(&mut threads, std::slice::from_ref(&moved), now);
    let planned = next(&threads[0], Some(&moved), true, now).unwrap();
    let id = record(&mut journal, &planned, now);
    settle(
        &mut threads,
        &mut journal,
        &id,
        &planned,
        Err("HTTP 404: Not Found".into()),
        now,
    );
    assert!(threads[0].gone);
    assert_eq!(
        next(&threads[0], Some(&moved), true, now + Duration::days(1)),
        None
    );
    // It stays deleted while the pull request is open, even once its
    // problem has gone and the pull request fails again.
    let later = now + KEEP_CLOSED + Duration::days(1);
    let prs = [open_pr()];
    prune(&mut threads, &mut journal, &[], &prs, later);
    assert_eq!(threads.len(), 1);
    let again = pull("p2", &["B"], '3', later);
    refresh(&mut threads, std::slice::from_ref(&again), later);
    assert_eq!(threads.len(), 1);
    assert_eq!(next(&threads[0], Some(&again), true, later), None);
    prune(&mut threads, &mut journal, &[], &[], later);
    assert!(threads.is_empty(), "and goes once the pull request closes");
}

/// Threads go with their problems, but an issue Orbit hasn't closed stays;
/// the journal keeps its newest entries for as long as closed problems.
#[test]
fn prune_keeps_open_issues_and_the_newest_journal() {
    let p = branch(&["A"], '1');
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    send(&mut threads, &mut journal, &p, true, t0());
    let q = pull("p2", &["A"], '1', t0());
    refresh(&mut threads, &[p.clone(), q], t0());
    assert_eq!(threads.len(), 2);
    assert!(prune(&mut threads, &mut journal, &[], &[], t0()));
    assert_eq!(threads.len(), 1, "the open issue stays");
    threads[0].closed = true;
    prune(&mut threads, &mut journal, &[], &[], t0());
    assert!(threads.is_empty());

    for i in 0..(MAX_JOURNAL + 5) {
        let mut w = journal[0].clone();
        w.id = i.to_string();
        journal.push(w);
    }
    prune(&mut threads, &mut journal, &[], &[], t0());
    assert_eq!(journal.len(), MAX_JOURNAL);
    assert_eq!(journal.last().unwrap().id, (MAX_JOURNAL + 4).to_string());
    prune(&mut threads, &mut journal, &[], &[], t0() + KEEP_CLOSED);
    assert!(journal.is_empty());
}

/// A post cut off when Orbit stopped leaves its thread unsure, and the
/// thread looks for it on GitHub before anything else, even once CI passes
/// or Bit may no longer post; what it finds is brought up to date.
#[test]
fn a_cut_off_post_is_looked_for_whatever_happens_next() {
    let mut p = branch(&["A"], '1');
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let planned = next(&threads[0], Some(&p), true, t0()).unwrap();
    assert_eq!(planned.action, Action::OpenIssue);
    record(&mut journal, &planned, t0());
    assert!(interrupted(&mut journal, &mut threads, t0()));
    assert_eq!(journal[0].state, WriteState::Failed);
    assert!(threads[0].unsure);
    assert!(!interrupted(&mut journal, &mut threads, t0()));
    // CI passes, and posting is off.
    pass(&mut p, '2', t0());
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let find = next(&threads[0], Some(&p), false, t0()).unwrap();
    assert_eq!(find.action, Action::Find);
    assert!(looks_first(find.action));
    let found = Posted {
        id: 9,
        number: Some(45),
        url: "https://github.com/a/b/issues/45".into(),
        updated_at: String::new(),
        body: String::new(),
    };
    looked(&mut threads, &find, Ok(Some(found)), t0());
    assert!(!threads[0].unsure);
    assert_eq!(threads[0].number, Some(45));
    let edit = next(&threads[0], Some(&p), false, t0()).unwrap();
    assert_eq!(edit.action, Action::EditIssue);
    assert!(edit
        .body
        .contains("Every check passes on `main` at `2222222`."));
    // Found nothing: nothing got there, and nothing new is posted.
    threads[0].number = None;
    threads[0].unsure = true;
    let find = next(&threads[0], Some(&p), false, t0()).unwrap();
    looked(&mut threads, &find, Ok(None), t0());
    assert!(!threads[0].unsure);
    assert_eq!(next(&threads[0], Some(&p), true, t0()), None);
    // A failed search waits like a failed write.
    threads[0].unsure = true;
    looked(&mut threads, &find, Err("HTTP 502".into()), t0());
    assert!(threads[0].unsure);
    assert_eq!(next(&threads[0], Some(&p), true, t0()), None);
    // And a post GitHub didn't answer may have got there too.
    let mut threads = vec![];
    let mut q = branch(&["A"], '1');
    q.id = "p2".into();
    refresh(&mut threads, std::slice::from_ref(&q), t0());
    let open = next(&threads[0], Some(&q), true, t0()).unwrap();
    let id = record(&mut journal, &open, t0());
    let timeout = Err("timed out".into());
    settle(&mut threads, &mut journal, &id, &open, timeout, t0());
    assert!(threads[0].unsure);
}

fn open_pr() -> PullRequest {
    PullRequest {
        id: "a/b#12".into(),
        repo: "a/b".into(),
        number: 12,
        title: String::new(),
        author: "agent".into(),
        body: String::new(),
        url: String::new(),
        head_sha: sha('1'),
        base_sha: sha('0'),
        head_ref: "fix-it".into(),
        base_ref: "main".into(),
        updated_at: String::new(),
        draft: false,
        additions: 0,
        deletions: 0,
        changed_files: 0,
        review_requested: false,
        labels: vec![],
        head_repo: "a/b".into(),
        author_association: "MEMBER".into(),
        base_repo: "a/b".into(),
        head_seen_at: String::new(),
    }
}

/// An open pull request keeps its comment and item numbers however long it
/// stays green, and problems an unclosed issue needs are kept, even when
/// many close at once; once Orbit is done with them, both go as usual.
#[test]
fn threads_keep_what_they_still_need() {
    let mut p = pull("p1", &["A"], '1', t0());
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    send(&mut threads, &mut journal, &p, true, t0());
    pass(&mut p, '2', t0());
    let mut problems = vec![p];
    let later = t0() + KEEP_CLOSED + Duration::days(1);
    let prs = [open_pr()];
    let pins = pinned(&threads, &prs);
    assert!(pins.contains("p1"));
    assert!(!crate::problems::prune(&mut problems, &pins, later));
    prune(&mut threads, &mut journal, &problems, &prs, later);
    assert_eq!(threads.len(), 1, "kept while the pull request is open");
    // It fails again: the next check is I2.
    problems.push(pull("p2", &["B"], '3', later));
    refresh(&mut threads, &problems, later);
    assert_eq!(threads.len(), 1);
    let numbers: Vec<u32> = threads[0].items.iter().map(|i| i.number).collect();
    assert_eq!(numbers, [1, 2]);
    // Once the pull request closes, it goes with its problems.
    assert!(pinned(&threads, &[]).is_empty());
    prune(&mut threads, &mut journal, &[], &[], later);
    assert!(threads.is_empty());

    // 150 default branches pass together, each with an issue to close.
    let mut problems = vec![];
    let mut threads = vec![];
    for i in 0..150 {
        let mut p = branch(&["A"], '1');
        p.id = format!("b{i:03}");
        p.repo = format!("a/b{i}");
        problems.push(p);
    }
    refresh(&mut threads, &problems, t0());
    for t in &mut threads {
        t.number = Some(1);
    }
    for p in &mut problems {
        pass(p, '2', t0());
    }
    let pins = pinned(&threads, &[]);
    assert_eq!(pins.len(), 150);
    crate::problems::prune(&mut problems, &pins, t0() + Duration::minutes(5));
    assert_eq!(problems.len(), 150, "all kept until their issues close");
    for t in &mut threads {
        t.closed = true;
    }
    let pins = pinned(&threads, &[]);
    crate::problems::prune(&mut problems, &pins, t0() + CLOSE_AFTER);
    assert_eq!(problems.len(), crate::problems::MAX_CLOSED);
}

/// When Orbit stops watching for a reason other than a pass, what it last
/// saw still stands, in the text and the hidden line.
#[test]
fn stopping_watching_keeps_what_was_last_seen() {
    let mut p = branch(&["A"], '1');
    let mut threads = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    p.closed_at = Some(stamp(t0()));
    p.closed_reason = Some("Orbit stopped watching CI.".into());
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    assert!(threads[0].items[0].failing);
    let body = issue_body(&threads[0], &p, true);
    assert!(body.contains("Orbit has stopped watching CI here. Orbit stopped watching CI."));
    assert!(body.contains("- **I1** · `A` · failing at `1111111` when Orbit stopped watching"));
    let hidden = body.lines().last().unwrap();
    assert!(hidden.contains("\"failing\":true"));
    assert!(hidden.contains("\"open\":false"));
    assert!(hidden.contains("\"passed\":false"));
    assert!(!comment_body(&threads[0], Some(&p)).contains("not failing"));
}

/// Bit posts something new only while Watch CI is on, the repository is
/// connected, and for a comment, Orbit follows the pull request.
#[test]
fn may_post_checks_where_things_stand_now() {
    let p = pull("p1", &["A"], '1', t0());
    let mut threads = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let t = threads[0].clone();
    let mut s = Snapshot::default();
    s.settings.watch_ci = true;
    s.settings.repositories.push(crate::types::Repository {
        name: "a/b".into(),
        path: String::new(),
        automatic: false,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes: 0,
        keep_updated: false,
        provider: Default::default(),
        dev_container: Default::default(),
    });
    s.prs = vec![open_pr()];
    assert!(may_post(&s, &t));
    s.prs[0].draft = true;
    assert!(!may_post(&s, &t), "a draft");
    s.prs[0].draft = false;
    s.prs[0].author_association = "NONE".into();
    assert!(!may_post(&s, &t), "not trusted");
    s.prs[0].author_association = "MEMBER".into();
    s.settings.watch_ci = false;
    assert!(!may_post(&s, &t), "Watch CI is off");
    s.settings.watch_ci = true;
    s.settings.repositories.clear();
    assert!(!may_post(&s, &t), "disconnected");
}

/// A reply from the agent, as the engine reads it, posted `id` seconds
/// after noon.
fn reply(id: u64, body: &str) -> Reply {
    let at = format!("2026-10-29T12:00:{:02}Z", id % 60);
    Reply {
        id,
        author: "agent".into(),
        created_at: at.clone(),
        updated_at: at,
        url: format!("https://github.com/a/b/issues/12#issuecomment-{id}"),
        said: crate::engine::claims::ci_lines(body),
    }
}
/// GitHub has Orbit's text, showing the items as they are now, from noon.
fn on_github(t: &mut Thread) {
    shown(t, "2026-10-29T12:00:00Z");
}

/// Each reply is read once, the latest answer about a failing item counts,
/// and an item that fails again starts without one.
#[test]
fn answers_count_while_their_item_fails() {
    let p = pull("p1", &["A", "B"], '1', t0());
    let mut threads = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let t = &mut threads[0];
    // Orbit's comment is reply 10; older replies aren't about its items.
    t.read_to = 10;
    on_github(t);
    let long = "x".repeat(MAX_ANSWER + 100);
    let heard = hear(
        t,
        &[
            reply(9, "Taking I2"),
            reply(11, &format!("Taking I1\nDeclined I2: {long}")),
            reply(12, "Fixed I1 in abc1234\nTaking I9"),
        ],
        None,
    );
    let numbers: Vec<u32> = heard.iter().map(|h| h.number).collect();
    assert_eq!(numbers, [2, 1]);
    let a = t.items[0].answer.clone().unwrap();
    assert_eq!(
        (a.kind, a.commit.as_deref(), a.check, a.reply_id),
        (
            AnswerKind::Fixed,
            Some("abc1234"),
            Some(FixCheck::Waiting),
            12
        )
    );
    let declined = t.items[1].answer.clone().unwrap();
    assert_eq!(declined.kind, AnswerKind::Declined);
    assert_eq!(declined.text.unwrap().chars().count(), MAX_ANSWER);
    assert_eq!(
        (t.read_to, t.read_since.as_str()),
        (12, "2026-10-29T12:00:12Z")
    );
    assert!(
        hear(t, &[reply(12, "Declined I1: no")], None).is_empty(),
        "read once"
    );
    assert!(hear(t, &[reply(13, "Thanks")], None).is_empty());
    assert_eq!(t.read_to, 13);

    // B passes: an answer about it now is left alone. It fails again: what
    // was said before is over.
    let mut q = p.clone();
    q.failing = vec![check("A")];
    q.sha = sha('2');
    refresh(&mut threads, std::slice::from_ref(&q), t0());
    assert!(hear(&mut threads[0], &[reply(14, "Taking I2")], None).is_empty());
    assert_eq!(threads[0].items[1].answer.as_ref().unwrap().reply_id, 11);
    q.failing = vec![check("A"), check("B")];
    q.sha = sha('3');
    refresh(&mut threads, std::slice::from_ref(&q), t0());
    assert_eq!(threads[0].items[1].answer, None);
    assert_eq!(threads[0].items[1].heard_from, "", "until Orbit says so");
    assert!(
        threads[0].items[0].answer.is_some(),
        "A never stopped failing"
    );
    assert_eq!(threads[0].items[0].heard_from, "2026-10-29T12:00:00Z");
}

/// A fix in a named commit is checked against CI until CI settles it; one
/// not on the branch is checked again once the branch moves. Bit hears of
/// each settled check once.
#[test]
fn fixes_are_checked_until_ci_settles_them() {
    let p = pull("p1", &["A"], '1', t0());
    let mut threads = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    on_github(&mut threads[0]);
    hear(&mut threads[0], &[reply(1, "Fixed I1 in abc1234")], None);
    let v = to_verify(&threads, std::slice::from_ref(&p));
    assert_eq!(
        v,
        [Verify {
            thread: "pr:a/b#12".into(),
            number: 1,
            repo: "a/b".into(),
            commit: "abc1234".into(),
            head: sha('1'),
            check: "A".into(),
        }]
    );
    assert_eq!(
        verified(&mut threads, &v[0], Ok(FixCheck::Waiting), t0()),
        None
    );
    assert_eq!(to_verify(&threads, std::slice::from_ref(&p)).len(), 1);
    let told = verified(&mut threads, &v[0], Ok(FixCheck::NotOnBranch), t0()).unwrap();
    assert_eq!(told.answer.check, Some(FixCheck::NotOnBranch));
    assert!(to_verify(&threads, std::slice::from_ref(&p)).is_empty());

    let mut moved = p.clone();
    moved.sha = sha('2');
    refresh(&mut threads, std::slice::from_ref(&moved), t0());
    let v = to_verify(&threads, std::slice::from_ref(&moved));
    assert_eq!(v[0].head, sha('2'));
    assert_eq!(
        verified(&mut threads, &v[0], Ok(FixCheck::NotOnBranch), t0()),
        None,
        "already told"
    );
    assert!(to_verify(&threads, std::slice::from_ref(&moved)).is_empty());
    let told = verified(&mut threads, &v[0], Ok(FixCheck::Fails), t0()).unwrap();
    assert_eq!(told.answer.head, sha('2'));
    assert!(to_verify(&threads, std::slice::from_ref(&moved)).is_empty());

    // A newer answer replaces it, and the old one's check is dropped.
    hear(&mut threads[0], &[reply(2, "Fixed I1 in def5678")], None);
    assert_eq!(
        verified(&mut threads, &v[0], Ok(FixCheck::Passes), t0()),
        None
    );
    let a = threads[0].items[0].answer.clone().unwrap();
    assert_eq!(a.check, Some(FixCheck::Waiting));
    // Nothing is checked on a problem that closed.
    let mut passed = moved.clone();
    pass(&mut passed, '3', t0());
    assert!(to_verify(&threads, std::slice::from_ref(&passed)).is_empty());
}

/// Orbit's comment says what the agent answered and links the reply, but
/// never repeats a reason or question, or links anywhere but GitHub. Once
/// Orbit stops watching, answers aren't shown.
#[test]
fn bodies_say_what_agents_answered_without_repeating_them() {
    let mut p = pull("p1", &["A", "B"], '1', t0());
    let mut threads = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    on_github(&mut threads[0]);
    hear(
        &mut threads[0],
        &[reply(1, "Declined I1: ask @someone <b>now</b>\nFixed I2")],
        None,
    );
    let body = comment_body(&threads[0], Some(&p));
    assert!(body.contains(
        "- **I1** · `A` · failing at `1111111` · [details](https://github.com/a/b/actions/runs/1/job/2) · declined ([reply](https://github.com/a/b/issues/12#issuecomment-1))\n"
    ), "{body}");
    assert!(body.contains("· said to be fixed; Orbit is waiting to see it pass ([reply]("));
    assert!(!body.contains("someone") && !body.contains("<b>"));
    let hidden = body.lines().last().unwrap();
    assert!(hidden.contains(r#""answer":{"check":null,"commit":null,"kind":"declined"}"#));
    assert_eq!(hidden.matches("\"answer\"").count(), 2, "{hidden}");
    threads[0].items[1].answer = None;
    let hidden = comment_body(&threads[0], Some(&p));
    assert_eq!(hidden.matches("\"answer\"").count(), 1, "none without one");
    threads[0].items[0].answer.as_mut().unwrap().url = "https://example.com/x".into();
    assert!(comment_body(&threads[0], Some(&p)).contains("job/2) · declined\n"));

    p.closed_at = Some(stamp(t0()));
    p.closed_reason = Some("Orbit stopped watching CI.".into());
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let body = comment_body(&threads[0], Some(&p));
    let shown = body.rsplit_once('\n').unwrap().0;
    assert!(!shown.contains("declined") && !shown.contains("said to be fixed"));
}

/// Bit tells you what Orbit posted and closed, and the answers worth your
/// attention: a declined item, or a fix CI doesn't bear out.
#[test]
fn bit_hears_what_matters() {
    let mut p = branch(&["A"], '1');
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    send(&mut threads, &mut journal, &p, true, t0());
    let key = threads[0].key.clone();
    let text = |e: &Event, threads: &[Thread], p: &Problem| {
        event_text(threads, std::slice::from_ref(p), e).map(|(_, _, text)| text)
    };
    assert_eq!(
        text(&Event::Posted(key.clone()), &threads, &p).as_deref(),
        Some("Orbit opened issue #45 to tell the agent that A fails on main in a/b.")
    );
    let heard = hear(
        &mut threads[0],
        &[reply(1, "Taking I1"), reply(2, "Declined I1: the runner")],
        None,
    );
    assert_eq!(heard.len(), 1);
    assert_eq!(
        text(&Event::Heard(Box::new(heard[0].clone())), &threads, &p).as_deref(),
        Some("The agent on main in a/b declined I1 (A): the runner")
    );
    let taking = hear(&mut threads[0], &[reply(3, "Taking I1")], None);
    assert_eq!(
        text(&Event::Heard(Box::new(taking[0].clone())), &threads, &p),
        None
    );
    pass(&mut p, '2', t0());
    assert_eq!(
        text(&Event::Closed(key.clone()), &threads, &p).as_deref(),
        Some("Orbit closed issue #45: main in a/b stayed green for an hour.")
    );
    p.closed_sha.clear();
    assert_eq!(
        text(&Event::Closed(key), &threads, &p).as_deref(),
        Some("Orbit closed issue #45, as it stopped watching main in a/b.")
    );
}

/// Replies count from Orbit's own post on. GitHub lists comments by when
/// they were posted, so one edited later comes before a newer reply: Orbit
/// goes on through the list a page at a time, and asks only for comments
/// from after the latest it saw once it has read the list to the end.
#[test]
fn replies_are_read_from_orbits_post_on() {
    let p = pull("p1", &["A"], '1', t0());
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    send(&mut threads, &mut journal, &p, true, t0());
    let t = &mut threads[0];
    assert_eq!(
        (t.read_to, t.read_since.as_str(), t.read_page),
        (7, "2026-10-29T12:00:00Z", 0)
    );
    // An old comment edited at 12:30, on a list GitHub has more of.
    let mut old = reply(5, "Taking I1");
    old.created_at = "2026-10-29T11:00:00Z".into();
    old.updated_at = "2026-10-29T12:30:00Z".into();
    assert!(hear(t, &[old], Some(10)).is_empty());
    assert_eq!(
        (t.read_since.as_str(), t.read_page),
        ("2026-10-29T12:00:00Z", 10),
        "nothing on the rest of the list is skipped"
    );
    // The rest of it holds a reply posted before that edit.
    let heard = hear(t, &[reply(8, "Taking I1")], None);
    assert_eq!(heard.len(), 1);
    assert_eq!(
        (t.read_to, t.read_since.as_str(), t.read_page),
        (8, "2026-10-29T12:30:00Z", 0)
    );
}

/// A check that fails again hears only replies from when Orbit's text
/// shows it failing again: one left while it was green, even edited since,
/// is about the failure before, though it comes on the same page as one
/// about this failure.
#[test]
fn replies_from_before_a_failure_came_back_are_skipped() {
    let mut p = pull("p1", &["A", "B"], '1', t0());
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    send(&mut threads, &mut journal, &p, true, t0());
    pass(&mut p, '2', t0());
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    // Both fail again, before Orbit's comment says so.
    let again = pull("p2", &["A", "B"], '3', t0());
    refresh(&mut threads, &[p, again.clone()], t0());
    let t = &mut threads[0];
    assert!(t.items.iter().all(|i| i.failing && i.heard_from.is_empty()));
    assert!(hear(t, &[reply(20, "Fixed I1 in abc1234")], None).is_empty());
    // Orbit's edit is on GitHub at 12:00:30, so it reads on from there.
    shown(t, "2026-10-29T12:00:30Z");
    assert_eq!(
        (t.read_since.as_str(), t.read_page),
        ("2026-10-29T12:00:30Z", 0)
    );
    // The first page after it: a reply from the green spell, edited since,
    // and one about this failure.
    let mut edited = reply(25, "Declined I1: the runner was down");
    edited.updated_at = "2026-10-29T12:00:40Z".into();
    let heard = hear(t, &[edited, reply(35, "Taking I2")], Some(10));
    let numbers: Vec<u32> = heard.iter().map(|h| h.number).collect();
    assert_eq!(numbers, [2]);
    assert_eq!(t.items[0].answer, None);
    // The next page goes on from there.
    let heard = hear(t, &[reply(36, "Declined I1: still down")], None);
    assert_eq!(heard.len(), 1);
    assert_eq!(t.items[0].answer.as_ref().unwrap().reply_id, 36);
}

/// A failure that comes back without changing Orbit's text, as when a check
/// is run again at the same commit before Orbit could say it passed, has
/// been on GitHub since Orbit's last write, so it hears replies from then.
#[test]
fn a_failure_back_without_new_text_counts_from_orbits_last_write() {
    let p = pull("p1", &["A"], '1', t0());
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    send(&mut threads, &mut journal, &p, true, t0());
    let mut green = p.clone();
    pass(&mut green, '1', t0());
    refresh(&mut threads, std::slice::from_ref(&green), t0());
    let again = pull("p2", &["A"], '1', t0());
    refresh(&mut threads, &[green, again.clone()], t0());
    assert_eq!(next(&threads[0], Some(&again), true, t0()), None);
    assert_eq!(threads[0].items[0].heard_from, "2026-10-29T12:00:00Z");
    assert_eq!(
        hear(&mut threads[0], &[reply(9, "Taking I1")], None).len(),
        1
    );
}

/// Fixes take turns: the one tried longest ago is checked first, so fixes
/// whose checks never finish don't keep another from being checked.
#[test]
fn fixes_are_checked_in_turn() {
    let names = ["A", "B", "C", "D", "E", "F"];
    let p = pull("p1", &names, '1', t0());
    let mut threads = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    on_github(&mut threads[0]);
    hear(
        &mut threads[0],
        &[reply(1, "Fixed I1, I2, I3, I4, I5, I6 in abc1234")],
        None,
    );
    let first = to_verify(&threads, std::slice::from_ref(&p));
    assert_eq!(first.len(), 6);
    for (i, v) in first.iter().take(CHECKS_PER_PASS).enumerate() {
        let result = if i == 0 {
            Err("HTTP 502".to_string())
        } else {
            Ok(FixCheck::Waiting)
        };
        verified(&mut threads, v, result, t0());
    }
    let next: Vec<u32> = to_verify(&threads, std::slice::from_ref(&p))
        .iter()
        .map(|v| v.number)
        .collect();
    assert_eq!(next[0], 6, "never tried, so first");
    assert_eq!(next.len(), 6, "a failed try leaves it to check again");
}

/// A post that reached GitHub without Orbit hearing back is found again,
/// and its items have been on GitHub since it was posted, not since the
/// edit that brings it up to date: a reply in between counts, and one from
/// before it doesn't. An item the copy shows from an earlier failure waits
/// for Orbit's text about this one.
#[test]
fn a_post_found_again_hears_from_when_it_was_posted() {
    let mut p = pull("p1", &["A", "B"], '1', t0());
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let post = next(&threads[0], Some(&p), true, t0()).unwrap();
    let id = record(&mut journal, &post, t0());
    settle(
        &mut threads,
        &mut journal,
        &id,
        &post,
        Err("timed out".into()),
        t0(),
    );
    // B passes and fails again at 2 before Orbit finds it.
    p.failing = vec![check("A")];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    p.failing = vec![check("A"), check("B")];
    p.sha = sha('2');
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let later = t0() + Duration::hours(1);
    let find = next(&threads[0], Some(&p), true, later).unwrap();
    assert_eq!(find.action, Action::Find);
    let copy = Posted {
        id: 7,
        number: Some(12),
        url: "https://github.com/a/b/issues/12#issuecomment-7".into(),
        updated_at: "2026-10-29T12:00:05Z".into(),
        body: post.body.clone(),
    };
    looked(&mut threads, &find, Ok(Some(copy)), later);
    let t = &mut threads[0];
    assert_eq!(t.items[0].heard_from, "2026-10-29T12:00:05Z");
    assert_eq!(t.items[1].heard_from, "", "failing since 2 now, not 1");
    assert_eq!(t.read_since, "2026-10-29T12:00:05Z");
    let heard = hear(
        t,
        &[
            reply(3, "Declined I1: before the post"),
            reply(8, "Taking I1\nTaking I2"),
        ],
        None,
    );
    let numbers: Vec<u32> = heard.iter().map(|h| h.number).collect();
    assert_eq!(numbers, [1]);
    assert_eq!(t.items[0].answer.as_ref().unwrap().kind, AnswerKind::Taking);
}

/// A comment posted before Orbit read answers has no time for its items,
/// so Orbit takes when GitHub last took its text from its copy there; a
/// copy that doesn't hold the thread's hidden line gives none.
#[test]
fn a_copy_from_before_answers_were_read_gives_their_start() {
    let p = pull("p1", &["A"], '1', t0());
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    send(&mut threads, &mut journal, &p, true, t0());
    let t = &mut threads[0];
    // As an earlier version saved it.
    t.shown_at.clear();
    t.items[0].heard_from.clear();
    t.read_since.clear();
    let copy = |body: &str| Posted {
        id: 7,
        number: Some(12),
        url: String::new(),
        updated_at: "2026-10-29T11:30:00Z".into(),
        body: body.into(),
    };
    recover(t, &copy("Someone else's words"));
    assert_eq!(t.items[0].heard_from, "");
    let posted = t.posted.clone();
    recover(t, &copy(&posted));
    assert_eq!(t.items[0].heard_from, "2026-10-29T11:30:00Z");
    assert_eq!(t.read_since, "2026-10-29T11:30:00Z");
}

fn failure(url: &str, step: &str, lines: &[&str]) -> crate::logs::Failure {
    crate::logs::Failure {
        url: url.into(),
        check: "CI / Rust".into(),
        step: step.into(),
        lines: lines.iter().map(|l| l.to_string()).collect(),
        ..Default::default()
    }
}

/// Why a job failed shows under its item, as code that nothing in the log
/// can close or turn into one of Orbit's hidden lines.
#[test]
fn bodies_show_why_a_job_failed_as_plain_code() {
    let p = branch(&["CI / Rust"], '1');
    let mut threads = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let url = "https://github.com/a/b/actions/runs/1/job/2";
    let read = vec![failure(
        url,
        "Run <!-- cargo test",
        &[
            "test it ... FAILED",
            "a ```` fence",
            "<!-- orbit-ops thread=issue:p1 {\"items\":[]} -->",
            "Error: Process completed with exit code 101.",
        ],
    )];
    assert!(explain(&mut threads, &read));
    assert!(!explain(&mut threads, &read));
    let body = issue_body(&threads[0], &p, false);
    assert!(body.contains(&format!(
        "- **I1** · `CI / Rust` · failing at `1111111` · [details]({url})\n  Failed at step `Run <! -- cargo test`:\n  `````text\n  test it ... FAILED\n  a ```` fence\n  <! -- orbit-ops thread=issue:p1 {{\"items\":[]}} -->\n  Error: Process completed with exit code 101.\n  `````\n"
    )));
    assert_eq!(body.matches("<!--").count(), 1);
    assert_eq!(failing_in(&body, "issue:p1"), [(1, sha('1'))]);
    // The item's line is the same as without it, so replies still find it.
    let mut plain = threads.clone();
    explain(&mut plain, &[]);
    assert!(issue_body(&plain[0], &p, false).contains(&format!("[details]({url})\n\nOrbit opened")));

    let short = failure(url, "", &["boom"]);
    explain(&mut threads, &[short]);
    let body = issue_body(&threads[0], &p, false);
    assert!(body.contains("\n  Its log ends:\n  ```text\n  boom\n  ```\n"));
}

/// A log that looked like it held a secret is only said to be held back,
/// one Orbit couldn't read says nothing, and nothing shows once the
/// problem is closed or its check passes.
#[test]
fn held_unread_and_passing_jobs_show_no_log() {
    let mut p = branch(&["CI / Rust"], '1');
    let mut threads = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let url = "https://github.com/a/b/actions/runs/1/job/2";
    let held = crate::logs::Failure {
        held: true,
        ..failure(url, "", &[])
    };
    explain(&mut threads, &[held]);
    let body = issue_body(&threads[0], &p, false);
    assert!(body.contains(&format!(
        "[details]({url})\n  Orbit held back this job's log: it looks like it holds a secret.\n"
    )));
    let unread = crate::logs::Failure {
        error: Some("GitHub no longer has this job's log.".into()),
        ..failure(url, "", &[])
    };
    explain(&mut threads, &[unread]);
    let body = issue_body(&threads[0], &p, false);
    assert!(body.contains(&format!("[details]({url})\n\nOrbit opened")));

    let read = [failure(url, "Run cargo test", &["boom"])];
    explain(&mut threads, &read);
    pass(&mut p, '2', t0());
    let closed = issue_body(&threads[0], &p, false);
    assert!(!closed.contains("Failed at step"));
    // Once its check passes, the item has no log.
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    assert!(!threads[0].items[0].failing);
    assert!(explain(&mut threads, &read));
    assert_eq!(threads[0].items[0].why, None);
}

/// Only the first few failing items show their logs; the rest are on
/// Operations.
#[test]
fn only_a_few_logs_show() {
    let mut p = branch(&[], '1');
    p.failing = (1..=MAX_WHY + 2)
        .map(|n| Check {
            name: format!("Job {n}"),
            url: format!("https://github.com/a/b/actions/runs/1/job/{n}"),
        })
        .collect();
    let read: Vec<crate::logs::Failure> = p
        .failing
        .iter()
        .map(|c| failure(&c.url, "Run it", &["boom"]))
        .collect();
    let mut threads = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    explain(&mut threads, &read);
    let body = issue_body(&threads[0], &p, false);
    assert_eq!(body.matches("Failed at step").count(), MAX_WHY);
    assert!(body.contains(&format!("`Job {MAX_WHY}`")));
    let last = body
        .split(&format!("`Job {}`", MAX_WHY + 1))
        .nth(1)
        .unwrap();
    assert!(!last
        .split("\n- ")
        .next()
        .unwrap()
        .contains("Failed at step"));
}

/// A repository where no CI runs on pull requests.
fn no_ci(workflows: &[&str], proposal: &str) -> Problem {
    Problem {
        id: "p9".into(),
        key: "setup:a/b".into(),
        kind: ProblemKind::NoCi,
        repo: "a/b".into(),
        branch: "main".into(),
        sha: sha('1'),
        since_sha: sha('1'),
        opened_at: stamp(t0()),
        found: Some(crate::workflows::Found {
            workflows: workflows.iter().map(|w| w.to_string()).collect(),
            proposal: proposal.into(),
            from: if proposal.is_empty() {
                vec![]
            } else {
                vec!["package.json".into(), "Cargo.toml".into()]
            },
        }),
        ..Default::default()
    }
}
fn hidden(body: &str, key: &str) -> serde_json::Value {
    let json = body
        .split_once(&format!("<!-- orbit-ops thread={key} "))
        .unwrap()
        .1;
    serde_json::from_str(json.split_once(" -->").unwrap().0).unwrap()
}

/// Orbit's proposal is one issue with a workflow to start from and one
/// item, edited as what Orbit finds changes, and closed at once as done
/// once a workflow runs on pull requests.
#[test]
fn a_repository_with_no_ci_gets_an_issue_with_a_workflow_to_start_from() {
    let mut p = no_ci(&[], "name: CI\non:\n  pull_request:\n");
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let t = &threads[0];
    assert_eq!((t.key.as_str(), t.kind), ("issue:p9", ThreadKind::Issue));
    assert_eq!((t.number, t.branch.as_str()), (None, "main"));
    assert_eq!(t.items.len(), 1);
    assert_eq!(t.items[0].kind, ItemKind::NoCi);
    assert_eq!(t.items[0].name(), "CI on pull requests");
    assert!(t.items[0].failing);
    let open = send(&mut threads, &mut journal, &p, true, t0()).unwrap();
    assert_eq!(open.action, Action::OpenIssue);
    assert_eq!(open.title, "Add CI that runs on pull requests");
    let (text, data) = open.body.split_once("<!-- orbit-ops").unwrap();
    assert_eq!(
        text,
        "## Orbit · CI\n\nThis repository has no workflows on `main`, so nothing checks a change before it merges.\n\nHere is a workflow to start from, as `.github/workflows/ci.yml`. Orbit built it from `package.json` and `Cargo.toml` at `1111111`, so check its commands are the ones this project uses.\n\n```yaml\nname: CI\non:\n  pull_request:\n```\n\n- **I1** · add a workflow that runs on pull requests\n\nOrbit opened this issue because no CI runs on pull requests here. It edits this issue as it reads new commits on `main`, and closes it once a workflow there runs on pull requests.\n\n"
    );
    assert!(data.starts_with(" thread=issue:p9 "));
    let v = hidden(&open.body, "issue:p9");
    assert_eq!(v["kind"], "no_ci");
    assert_eq!(
        v["items"][0],
        json!({ "id": "I1", "kind": "no_ci", "branch": "main", "failing": true, "since": sha('1'), "at": sha('1') })
    );
    assert_eq!(next(&threads[0], Some(&p), true, t0()), None);

    // Workflows that don't run on pull requests are named, as code; with
    // nothing to propose, it says what would do.
    p.sha = sha('2');
    p.found = no_ci(
        &[".github/workflows/de`ploy.yml", ".github/workflows/x.yml"],
        "",
    )
    .found;
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let edit = send(&mut threads, &mut journal, &p, true, t0()).unwrap();
    assert_eq!(edit.action, Action::EditIssue);
    assert!(edit.body.contains(
        "None of the workflows on `main` runs on pull requests (`.github/workflows/de'ploy.yml`, `.github/workflows/x.yml`), so nothing checks a change before it merges.\n\nOrbit couldn't tell from the files at the top of this repository how it builds and tests, so it has no workflow to suggest. One in `.github/workflows` that runs the project's tests on `pull_request` is enough.\n\n"
    ), "{}", edit.body);
    assert!(!edit.body.contains("```"));
    // Its item and text follow a renamed branch.
    p.branch = "trunk".into();
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    assert_eq!(
        (
            threads[0].branch.as_str(),
            threads[0].items[0].check.as_str()
        ),
        ("trunk", "trunk")
    );
    let edit = send(&mut threads, &mut journal, &p, true, t0()).unwrap();
    assert!(
        edit.body.contains("None of the workflows on `trunk`"),
        "{}",
        edit.body
    );
    assert_eq!(
        hidden(&edit.body, "issue:p9")["items"][0]["branch"],
        "trunk"
    );
    p.branch = "main".into();
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    send(&mut threads, &mut journal, &p, true, t0()).unwrap();

    // Done: the note and the close go at once, as completed.
    let later = t0() + Duration::minutes(5);
    p.timeline.clear();
    p.closed_at = Some(stamp(later));
    p.closed_reason = Some("CI runs on pull requests at 3333333.".into());
    p.closed_sha = sha('3');
    refresh(&mut threads, std::slice::from_ref(&p), later);
    assert!(!threads[0].items[0].failing);
    let note = send(&mut threads, &mut journal, &p, true, later).unwrap();
    assert_eq!(note.action, Action::NoteOnIssue);
    assert!(note.body.starts_with(
        "A workflow on `main` runs on pull requests at `3333333`, so Orbit is closing this issue."
    ));
    let close = send(&mut threads, &mut journal, &p, true, later).unwrap();
    assert_eq!(close.action, Action::CloseIssue);
    assert!(close.completed);
    assert!(close.body.contains(
        "A workflow on `main` runs on pull requests at `3333333`. Nothing is left to do here.\n\n- **I1** · a workflow runs on pull requests · done at `3333333`\n"
    ), "{}", close.body);
    assert_eq!(hidden(&close.body, "issue:p9")["passed"], true);
    assert_eq!(next(&threads[0], Some(&p), true, later), None);
}

/// One that closes for another reason closes as not planned, with Orbit's
/// reason; agents answer its item like any other, and no fix is checked.
#[test]
fn a_proposal_that_ends_otherwise_says_why_and_takes_answers() {
    let mut p = no_ci(&[], "name: CI\n");
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    send(&mut threads, &mut journal, &p, true, t0());
    on_github(&mut threads[0]);
    // A fix isn't checked on its commit: the proposal closes once a
    // workflow runs on pull requests.
    let fixed = reply(1, "Fixed I1 in abcdef1");
    hear(&mut threads[0], std::slice::from_ref(&fixed), None);
    assert!(to_verify(&threads, std::slice::from_ref(&p)).is_empty());
    let edit = send(&mut threads, &mut journal, &p, true, t0()).unwrap();
    assert!(edit.body.contains("- **I1** · add a workflow that runs on pull requests · said to be added in `abcdef1`; Orbit is waiting to see a workflow that runs on pull requests on `main` ([reply]"), "{}", edit.body);
    let heard = hear(
        &mut threads[0],
        &[fixed, reply(2, "Declined I1: we use Jenkins")],
        None,
    );
    assert_eq!(heard.len(), 1);
    let (_, _, text) = event_text(
        &threads,
        std::slice::from_ref(&p),
        &Event::Heard(Box::new(heard[0].clone())),
    )
    .unwrap();
    assert_eq!(
        text,
        "The agent on main in a/b declined I1 (CI on pull requests): we use Jenkins"
    );
    let edit = send(&mut threads, &mut journal, &p, true, t0()).unwrap();
    assert!(edit.body.contains("- **I1** · add a workflow that runs on pull requests · declined ([reply](https://github.com/a/b/issues/12#issuecomment-2))"), "{}", edit.body);

    p.closed_at = Some(stamp(t0()));
    p.closed_reason = Some("Checks ran on #7, so something runs them.".into());
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let note = send(&mut threads, &mut journal, &p, true, t0()).unwrap();
    assert_eq!(
        note.body.split_once("\n\n").unwrap().0,
        "Orbit is closing this issue. Checks ran on #7, so something runs them."
    );
    let close = send(&mut threads, &mut journal, &p, true, t0()).unwrap();
    assert!(!close.completed);
    assert!(close.body.contains(
        "Orbit has stopped proposing CI here. Checks ran on #7, so something runs them.\n\n- **I1** · add a workflow that runs on pull requests, when Orbit stopped watching\n"
    ), "{}", close.body);
}

fn watched() -> Snapshot {
    let mut s = Snapshot::default();
    s.settings.watch_ci = true;
    s.settings.buddy.enabled = true;
    s.settings.repositories.push(crate::types::Repository {
        name: "a/b".into(),
        path: String::new(),
        automatic: false,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes: 0,
        keep_updated: false,
        provider: Default::default(),
        dev_container: Default::default(),
    });
    s
}

/// Proposing CI has its own rule, Ask first by default, and Bit offers
/// each proposal once.
#[test]
fn bit_offers_a_proposal_once_and_posts_it_by_its_own_rule() {
    let mut s = watched();
    s.problems = vec![no_ci(&[], "name: CI\n"), branch(&["A"], '1')];
    refresh(&mut s.ops, &s.problems, t0());
    let key = "issue:p9".to_string();
    let proposal = s.ops.iter().find(|t| t.key == key).unwrap().clone();
    let failing = s.ops.iter().find(|t| t.key == "issue:p1").unwrap().clone();
    assert!(proposes(&s, &proposal) && !proposes(&s, &failing));
    assert_eq!(rule_of(&s, &proposal), "ask_first");
    assert_eq!(rule_of(&s, &failing), "do_it");
    assert!(!may_post(&s, &proposal));
    assert!(may_post(&s, &failing));

    let events = offer(&mut s);
    assert_eq!(events, [Event::Offered(key.clone())]);
    let (problem, repo, text) = event_text(&s.ops, &s.problems, &events[0]).unwrap();
    assert_eq!((problem.as_str(), repo.as_str()), ("p9", "a/b"));
    assert_eq!(
        text,
        "No CI runs on a/b's pull requests. Tap Propose CI on Operations and Orbit opens an issue for its agent, with a workflow to start from."
    );
    assert!(offer(&mut s).is_empty(), "once");

    // You said yes.
    s.ops.iter_mut().find(|t| t.key == key).unwrap().allowed = true;
    let proposal = s.ops.iter().find(|t| t.key == key).unwrap().clone();
    assert!(may_post(&s, &proposal));
    s.settings.buddy.rules.propose_ci = "leave_it".into();
    assert!(!may_post(&s, &proposal));
    s.settings.buddy.rules.propose_ci = "do_it".into();
    s.settings.buddy.rules.open_issues = "leave_it".into();
    assert!(may_post(&s, &proposal));

    // Nothing is offered under another rule, with Bit off, once posted or
    // allowed, while unwatched, or once the problem closed.
    let fresh = |s: &mut Snapshot| {
        for t in &mut s.ops {
            t.offered = false;
            t.allowed = false;
        }
    };
    fresh(&mut s);
    assert!(offer(&mut s).is_empty(), "Do it posts instead");
    s.settings.buddy.rules.propose_ci = "ask_first".into();
    s.settings.buddy.enabled = false;
    assert!(offer(&mut s).is_empty(), "Bit is off");
    s.settings.buddy.enabled = true;
    s.settings.watch_ci = false;
    assert!(offer(&mut s).is_empty(), "not watching");
    s.settings.watch_ci = true;
    s.ops.iter_mut().find(|t| t.key == key).unwrap().allowed = true;
    assert!(offer(&mut s).is_empty(), "allowed");
    fresh(&mut s);
    s.problems[0].closed_at = Some(stamp(t0()));
    assert!(offer(&mut s).is_empty(), "closed");
    s.problems[0].closed_at = None;
    s.ops.iter_mut().find(|t| t.key == key).unwrap().number = Some(45);
    assert!(offer(&mut s).is_empty(), "posted");
    s.ops.iter_mut().find(|t| t.key == key).unwrap().number = None;
    assert_eq!(offer(&mut s).len(), 1);
}

#[test]
fn bit_says_what_orbit_did_about_a_proposal() {
    let mut p = no_ci(&[], "name: CI\n");
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    send(&mut threads, &mut journal, &p, true, t0());
    let key = threads[0].key.clone();
    let text = |e: &Event, threads: &[Thread], p: &Problem| {
        event_text(threads, std::slice::from_ref(p), e).map(|(_, _, text)| text)
    };
    assert_eq!(
        text(&Event::Posted(key.clone()), &threads, &p).as_deref(),
        Some("Orbit opened issue #45 for the agent on a/b, with a workflow to start from, as no CI runs on its pull requests.")
    );
    p.closed_at = Some(stamp(t0()));
    p.closed_reason = Some("CI runs on pull requests at 2222222.".into());
    p.closed_sha = sha('2');
    assert_eq!(
        text(&Event::Closed(key.clone()), &threads, &p).as_deref(),
        Some("Orbit closed issue #45: a workflow runs on a/b's pull requests now.")
    );
    p.closed_sha.clear();
    p.closed_reason = Some("The repository was disconnected.".into());
    assert_eq!(
        text(&Event::Closed(key), &threads, &p).as_deref(),
        Some("Orbit closed issue #45 on a/b. The repository was disconnected.")
    );
}

/// Pull request 12 of `a/b` at `head`, into main.
fn open(head: char) -> PullRequest {
    let mut p = crate::types::tests::pr();
    p.id = "a/b#12".into();
    p.number = 12;
    p.head_sha = sha(head);
    p
}
/// Its answer at `head`: clear, or the first thing in the way and whose
/// move it is.
fn answer(head: char, wait: Option<(Turn, &str)>) -> Readiness {
    Readiness {
        pr_id: "a/b#12".into(),
        repo: "a/b".into(),
        number: 12,
        head_sha: sha(head),
        base: "main".into(),
        base_sha: sha('c'),
        clear: wait.is_none(),
        waits: wait
            .map(|(turn, text)| crate::readiness::Wait {
                kind: crate::readiness::WaitKind::Checks,
                turn,
                text: text.into(),
            })
            .into_iter()
            .collect(),
    }
}
fn say(threads: &mut Vec<Thread>, head: char, a: Readiness, problems: &[Problem]) -> bool {
    ready(threads, &[a], &[open(head)], problems, &[], true, t0())
}

/// A pull request Orbit reviews gets a comment once its answer tells the
/// agent something: it is clear to merge, or it waits on someone. Not
/// while Orbit, CI or GitHub is still at work, nor while Orbit doesn't
/// watch CI, nor where Bit may not post. Bit tells you nothing about it.
#[test]
fn the_merge_answer_gets_a_comment_once_it_tells_something() {
    let prs = [open('a')];
    let mut threads = vec![];
    let mut journal = vec![];
    let working = answer('a', Some((Turn::Working, "Checks are still running.")));
    let answers = std::slice::from_ref(&working);
    assert!(!ready(&mut threads, answers, &prs, &[], &[], false, t0()));
    assert!(threads.is_empty());
    assert!(ready(&mut threads, answers, &prs, &[], &[], true, t0()));
    let t = &threads[0];
    assert_eq!(
        (t.key.as_str(), t.kind, t.number, t.pr_id.as_deref()),
        ("pr:a/b#12", ThreadKind::Comment, Some(12), Some("a/b#12"))
    );
    assert_eq!(t.updated_at, stamp(t0()));
    assert_eq!(next(t, None, true, t0()), None);
    for turn in [Turn::Author, Turn::Person] {
        let mut threads = threads.clone();
        say(&mut threads, 'a', answer('a', Some((turn, "x"))), &[]);
        let post = next(&threads[0], None, true, t0()).map(|p| p.action);
        assert_eq!(post, Some(Action::PostComment), "{turn:?}");
        assert_eq!(next(&threads[0], None, false, t0()), None);
    }
    assert!(say(&mut threads, 'a', answer('a', None), &[]));
    let post = tell(&mut threads, &mut journal, None, true, t0()).unwrap();
    assert_eq!(post.action, Action::PostComment);
    assert_eq!(
        post.body,
        format!(
            "## Orbit\n\n**Clear to merge** at `aaaaaaa` against `main` at `ccccccc`.\n\n<!-- orbit-ready {{\"base\":\"main\",\"baseSha\":\"{}\",\"clear\":true,\"head\":\"{}\",\"turn\":null,\"version\":1}} -->\n\nOrbit works out after each sync whether this is clear to merge, and edits this comment when that or CI changes. It never pushes to this branch.\n\n<!-- orbit-ops thread=pr:a/b#12 {{\"items\":[],\"kind\":\"pull_request\",\"open\":false,\"repo\":\"a/b\",\"version\":1}} -->",
            sha('c'),
            sha('a')
        )
    );
    let posted = Event::Posted("pr:a/b#12".into());
    assert_eq!(event_text(&threads, &[], &posted), None);
    // Nothing more while nothing changes.
    assert!(!say(&mut threads, 'a', answer('a', None), &[]));
    assert_eq!(next(&threads[0], None, true, t0()), None);
}

/// The answer on the comment changes only when it turns clear or not, its
/// head or base moves, or it becomes someone else's move, so each change
/// wakes the agent once. Its words catch up when the comment changes
/// anyway.
#[test]
fn the_comment_changes_only_when_the_answer_does() {
    let mut threads = vec![];
    let mut journal = vec![];
    say(&mut threads, 'a', answer('a', None), &[]);
    tell(&mut threads, &mut journal, None, true, t0()).unwrap();
    let mut edit = |threads: &mut Vec<Thread>, p: Option<&Problem>| {
        let e = tell(threads, &mut journal, p, true, t0()).map(|e| (e.action, e.body));
        e.map(|(action, body)| {
            assert_eq!(action, Action::EditComment);
            body
        })
    };
    // A push takes it back: one edit.
    let working = |text: &str| answer('b', Some((Turn::Working, text)));
    say(
        &mut threads,
        'b',
        working("Orbit hasn't reviewed bbbbbbb yet."),
        &[],
    );
    let body = edit(&mut threads, None).unwrap();
    assert!(body.contains(&format!(
        "\n\n**Not clear to merge yet.** Orbit hasn't reviewed bbbbbbb yet.\n\n<!-- orbit-ready {{\"base\":\"main\",\"baseSha\":\"\",\"clear\":false,\"head\":\"{}\",\"turn\":\"working\",\"version\":1}} -->\n\n",
        sha('b')
    )));
    // Orbit, CI and GitHub at work: no edit, and the words wait.
    for text in [
        "Orbit is reviewing bbbbbbb.",
        "Checks are still running on bbbbbbb.",
        "GitHub is still checking whether it merges.",
    ] {
        assert!(!say(&mut threads, 'b', working(text), &[]), "{text}");
        assert_eq!(edit(&mut threads, None), None, "{text}");
    }
    let text = |threads: &[Thread]| threads[0].ready.as_ref().unwrap().text.clone();
    assert_eq!(text(&threads), "Orbit hasn't reviewed bbbbbbb yet.");
    // The author's move: one edit.
    let conflict = "It conflicts with main: merge main in and fix the conflicts.";
    say(
        &mut threads,
        'b',
        answer('b', Some((Turn::Author, conflict))),
        &[],
    );
    let body = edit(&mut threads, None).unwrap();
    assert!(body.contains(&format!("**Not clear to merge yet.** {conflict}\n\n")));
    assert!(body.contains("\"turn\":\"author\""));
    // Clear, then clear against a newer base: an edit each.
    say(&mut threads, 'b', answer('b', None), &[]);
    let body = edit(&mut threads, None).unwrap();
    assert!(body.contains("**Clear to merge** at `bbbbbbb` against `main` at `ccccccc`."));
    let newer = Readiness {
        base_sha: sha('d'),
        ..answer('b', None)
    };
    say(&mut threads, 'b', newer, &[]);
    let body = edit(&mut threads, None).unwrap();
    assert!(body.contains("**Clear to merge** at `bbbbbbb` against `main` at `ddddddd`."));
    // Its words catch up when CI changes the comment anyway.
    let fails = |text: &str| answer('b', Some((Turn::Author, text)));
    say(&mut threads, 'b', fails("CI / Test fails on bbbbbbb."), &[]);
    edit(&mut threads, None).unwrap();
    let both = "CI / Test and CI / Lint fail on bbbbbbb.";
    assert!(!say(&mut threads, 'b', fails(both), &[]));
    assert_eq!(text(&threads), "CI / Test fails on bbbbbbb.");
    let p = pull("p1", &["CI / Test", "CI / Lint"], 'b', t0());
    let problems = std::slice::from_ref(&p);
    refresh(&mut threads, problems, t0());
    assert!(say(&mut threads, 'b', fails(both), problems));
    let body = edit(&mut threads, Some(&p)).unwrap();
    assert!(body.starts_with(&format!(
        "## Orbit\n\n**Not clear to merge yet.** {both}\n\n<!-- orbit-ready "
    )));
    assert!(body.contains(
        " -->\n\n### CI\n\n2 checks fail on this pull request at `bbbbbbb`.\n\n- **I1** · `CI / Test`"
    ));
    assert!(body.ends_with("\"open\":true,\"passed\":false,\"repo\":\"a/b\",\"version\":1} -->"));
    // With CI unchanged, new words wait again.
    let reworded = "CI / Lint and CI / Test fail on bbbbbbb.";
    assert!(!say(&mut threads, 'b', fails(reworded), problems));
    assert_eq!(edit(&mut threads, Some(&p)), None);
}

/// A pull request still open that Orbit stopped working out says so, once;
/// a closed one is left as it was. A comment only about CI never gets the
/// line, and a thread with neither has nothing to say.
#[test]
fn a_pull_request_orbit_stops_working_out_says_so() {
    let mut threads = vec![];
    let mut journal = vec![];
    say(&mut threads, 'a', answer('a', None), &[]);
    tell(&mut threads, &mut journal, None, true, t0()).unwrap();
    let draft = PullRequest {
        draft: true,
        ..open('a')
    };
    let stop = |threads: &mut Vec<Thread>, pr: &PullRequest| {
        ready(threads, &[], std::slice::from_ref(pr), &[], &[], true, t0())
    };
    assert!(stop(&mut threads, &draft));
    let body = tell(&mut threads, &mut journal, None, true, t0())
        .unwrap()
        .body;
    assert!(body.contains(&format!(
        "\n\n**Not clear to merge.** Orbit stopped working out whether it is clear to merge while it is a draft.\n\n<!-- orbit-ready {{\"base\":\"main\",\"baseSha\":\"\",\"clear\":false,\"head\":\"{}\",\"turn\":null,\"version\":1}} -->",
        sha('a')
    )));
    assert!(!stop(&mut threads, &draft));
    // Out of draft but no longer reviewed: the same answer, so no edit.
    stop(&mut threads, &open('a'));
    assert_eq!(next(&threads[0], None, true, t0()), None);
    // Closed: left as it was.
    let before = threads.clone();
    assert!(!ready(&mut threads, &[], &[], &[], &[], true, t0()));
    assert_eq!(threads, before);
    // Reviewed again.
    say(&mut threads, 'a', answer('a', None), &[]);
    let body = tell(&mut threads, &mut journal, None, true, t0())
        .unwrap()
        .body;
    assert!(body.contains("**Clear to merge** at `aaaaaaa`"));

    // One never posted isn't posted to say Orbit stopped.
    let mut quiet = vec![];
    say(
        &mut quiet,
        'a',
        answer('a', Some((Turn::Working, "x"))),
        &[],
    );
    stop(&mut quiet, &open('a'));
    assert_eq!(
        quiet[0].ready.as_ref().unwrap().text,
        "Orbit stopped working out whether it is clear to merge, as it no longer reviews it automatically."
    );
    assert_eq!(next(&quiet[0], None, true, t0()), None);
    // A comment only about CI.
    let p = pull("p1", &["CI / Rust"], 'a', t0());
    let mut ci = vec![];
    refresh(&mut ci, std::slice::from_ref(&p), t0());
    assert!(!ready(
        &mut ci,
        &[],
        &[open('a')],
        std::slice::from_ref(&p),
        &[],
        true,
        t0()
    ));
    assert_eq!(ci[0].ready, None);
    assert!(comment_body(&ci[0], Some(&p)).starts_with("## Orbit · CI\n\n1 check fails"));
    // Neither.
    ci[0].comment_id = Some(7);
    assert_eq!(next(&ci[0], None, true, t0()), None);
}

/// A pull request that closes keeps its comment as it was, and once it is
/// reopened the comment says what is true now: the CI problem from before
/// it closed is over, so only the answer is left. Reopened means CI lists
/// it again, whatever the times say.
#[test]
fn a_reopened_pull_request_gets_its_answer() {
    let mut p = pull("p1", &["CI / Rust"], 'a', t0());
    let mut threads = vec![];
    let mut journal = vec![];
    refresh(&mut threads, std::slice::from_ref(&p), t0());
    let failing = answer('a', Some((Turn::Author, "CI / Rust fails on aaaaaaa.")));
    say(&mut threads, 'a', failing.clone(), std::slice::from_ref(&p));
    let body = send(&mut threads, &mut journal, &p, true, t0())
        .unwrap()
        .body;
    assert!(body.contains("### CI\n\n1 check fails on this pull request at `aaaaaaa`."));
    // It closes: nothing is edited, though Orbit stopped watching its CI.
    let closed = t0() + Duration::hours(1);
    p.closed_at = Some(stamp(closed));
    p.closed_reason = Some("The pull request closed.".into());
    let problems = std::slice::from_ref(&p);
    refresh(&mut threads, problems, closed);
    let read = |repo: &str, numbers: &[u64]| crate::ci::RepoCi {
        repo: repo.into(),
        pulls: numbers
            .iter()
            .map(|&number| crate::ci::PullCi {
                number,
                ..Default::default()
            })
            .collect(),
        complete: true,
        ..Default::default()
    };
    let gone = [read("a/b", &[])];
    assert!(!ready(
        &mut threads,
        &[],
        &[],
        problems,
        &gone,
        true,
        closed
    ));
    let t = &threads[0];
    assert_eq!(next(t, problem_of(t, problems, &gone), true, closed), None);
    // Still listed in Orbit's pull requests from before it closed, or
    // another pull request or repository listed: the same.
    let prs = [open('a')];
    assert!(!ready(
        &mut threads,
        &[failing],
        &prs,
        problems,
        &gone,
        true,
        closed
    ));
    let t = &threads[0];
    assert_eq!(next(t, problem_of(t, problems, &gone), true, closed), None);
    for others in [[read("a/b", &[13])], [read("c/d", &[12])]] {
        assert_eq!(problem_of(&threads[0], problems, &others), Some(&p));
    }
    // CI lists it again, even before its update time: reopened. Its checks
    // pass and Orbit's review is clean, so it is clear to merge.
    let listed = [read("A/B", &[12])];
    let pr = PullRequest {
        updated_at: stamp(t0()),
        ..open('a')
    };
    let prs = std::slice::from_ref(&pr);
    let clear = [answer('a', None)];
    assert!(ready(
        &mut threads,
        &clear,
        prs,
        problems,
        &listed,
        true,
        closed
    ));
    let problem = problem_of(&threads[0], problems, &listed);
    assert_eq!(problem, None);
    let edit = tell(&mut threads, &mut journal, problem, true, closed).unwrap();
    assert_eq!(edit.action, Action::EditComment);
    assert!(edit
        .body
        .starts_with("## Orbit\n\n**Clear to merge** at `aaaaaaa`"));
    assert!(!edit.body.contains("### CI"));
    assert!(edit.body.contains("\"items\":[]"));
    assert!(!ready(
        &mut threads,
        &clear,
        prs,
        problems,
        &listed,
        true,
        closed
    ));
    assert_eq!(next(&threads[0], problem, true, closed), None);
    // It waits on someone, then the same in new words, which wait as usual.
    let waits = |text| [answer('a', Some((Turn::Person, text)))];
    let review = waits("A review is required.");
    assert!(ready(
        &mut threads,
        &review,
        prs,
        problems,
        &listed,
        true,
        closed
    ));
    tell(&mut threads, &mut journal, problem, true, closed).unwrap();
    let reworded = waits("A required review is missing.");
    assert!(!ready(
        &mut threads,
        &reworded,
        prs,
        problems,
        &listed,
        true,
        closed
    ));
    assert_eq!(next(&threads[0], problem, true, closed), None);
    // Only a problem that closed with its pull request is over that way.
    let mut passed = p.clone();
    pass(&mut passed, 'a', closed);
    let passed = std::slice::from_ref(&passed);
    assert_eq!(problem_of(&threads[0], passed, &listed), passed.first());
}

/// Others' words in the answer, such as a check's name or GitHub's error,
/// can't mention anyone, link anywhere or read as Markdown or HTML.
#[test]
fn others_words_in_the_answer_stay_words() {
    let said = "CI / Rust (ubuntu-22.04) fails on abc1234: it's 100% \"odd\", a=b; c+d?";
    assert_eq!(plain(said), said);
    for (word, shown) in [
        ("@team", "`@team`"),
        ("*Lint*", "`*Lint*`"),
        ("a_b", "`a_b`"),
        ("#12", "`#12`"),
        ("<b>", "`<b>`"),
        ("[a](b)", "`[a](b)`"),
        ("~~x~~", "`~~x~~`"),
        ("$x$", "`$x$`"),
        ("&amp;", "`&amp;`"),
        ("a\\b", "`a\\b`"),
        ("`x`", "`'x'`"),
        ("<!--x", "`<! --x`"),
        ("a\nb", "`a b`"),
        ("https://evil.example", "`https://evil.example`"),
        ("WWW.evil.example", "`WWW.evil.example`"),
    ] {
        assert_eq!(
            plain(&format!("{word} fails.")),
            format!("{shown} fails."),
            "{word}"
        );
    }
    // On the comment, whatever the answer.
    let said = |turn| Ready {
        turn,
        text: "@team fails.".into(),
        ..Default::default()
    };
    for turn in [Some(Turn::Author), None] {
        assert!(
            ready_text(&said(turn)).contains(".** `@team` fails.\n"),
            "{turn:?}"
        );
    }
}

/// A thread saved before Orbit said whether pull requests are clear to
/// merge loads, and one without an answer saves as before.
#[test]
fn threads_without_an_answer_load_and_save_as_before() {
    let t: Thread = serde_json::from_str(r#"{"key":"pr:a/b#12","kind":"comment"}"#).unwrap();
    assert_eq!(t.ready, None);
    assert!(!serde_json::to_string(&t).unwrap().contains("ready"));
}

/// The reply skill for coding agents shows the answer as Orbit writes it,
/// and names each field of its hidden line.
#[test]
fn the_reply_skill_shows_the_answer_as_orbit_writes_it() {
    let skill = include_str!("../../.claude/skills/answer-orbit-reviews/SKILL.md");
    let clear = Ready {
        clear: true,
        head_sha: format!("abc1234{}", "0".repeat(33)),
        base: "main".into(),
        base_sha: format!("def5678{}", "0".repeat(33)),
        ..Default::default()
    };
    let conflict = Ready {
        turn: Some(Turn::Author),
        text: "It conflicts with main: merge main in and fix the conflicts.".into(),
        ..clear.clone()
    };
    for r in [clear, conflict] {
        let text = ready_text(&r);
        let (line, hidden) = text.split_once("\n\n<!-- orbit-ready ").unwrap();
        assert!(skill.contains(&format!("\n- {line}\n")), "{line}");
        let v: serde_json::Value = serde_json::from_str(hidden.trim_end_matches(" -->")).unwrap();
        for key in v.as_object().unwrap().keys() {
            let named = skill.contains(&format!("`{key}`"))
                || skill.contains(&format!("`\"{key}\": {}`", v[key]));
            assert!(named, "{key}");
        }
    }
    for turn in ["author", "person", "working"] {
        assert!(skill.contains(&format!("`{turn}`")), "{turn}");
    }
    // It names the hidden line that tells Orbit's own comment apart.
    let id = crate::types::pr_id("owner/repo", 12);
    let a = Readiness {
        pr_id: id.clone(),
        ..answer('a', None)
    };
    let pr = PullRequest { id, ..open('a') };
    let mut threads = vec![];
    ready(&mut threads, &[a], &[pr], &[], &[], true, t0());
    let marker = format!("`{}{{json}} -->`", threads[0].tag());
    assert!(skill.contains(&marker), "{marker}");
}
