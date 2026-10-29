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
    let planned = next(&threads[0], Some(p), may, now)?;
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
    assert!(!comment_body(&threads[0], &p).contains("not failing"));
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
