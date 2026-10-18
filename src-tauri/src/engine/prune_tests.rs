use super::prune::*;
use super::*;

fn review(pr: &PullRequest, n: u32, status: &str, updated: &str) -> Review {
    let mut r = Review::new(pr, &Settings::default(), true);
    r.id = format!("{}-{n}", pr.id);
    r.status = status.into();
    r.created_at = format!("2026-09-0{n}T10:00:00.000Z");
    r.updated_at = updated.into();
    r.messages.push(Message::new("user", "Why?"));
    r
}
fn ids(s: &Snapshot) -> Vec<&str> {
    s.reviews.iter().map(|r| r.id.as_str()).collect()
}
fn at(day: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(day).unwrap().into()
}

#[test]
fn a_closed_pull_request_keeps_its_last_three_reviews() {
    let open = crate::types::tests::pr();
    let mut closed = open.clone();
    closed.id = "a/b#2".into();
    closed.number = 2;
    let mut s = Snapshot {
        prs: vec![open.clone()],
        history_prs: vec![closed.clone()],
        ..Default::default()
    };
    let recent = "2026-10-01T10:00:00.000Z";
    for n in 1..=5 {
        s.reviews.push(review(&open, n, "ready", recent));
        s.reviews.push(review(&closed, n, "ready", recent));
    }
    let now = at("2026-10-02T10:00:00Z");
    assert!(prune(&mut s, now));
    let mut kept = ids(&s);
    kept.sort();
    let mut expected: Vec<String> = (1..=5).map(|n| format!("{}-{n}", open.id)).collect();
    expected.extend((3..=5).map(|n| format!("a/b#2-{n}")));
    expected.sort();
    assert_eq!(kept, expected);
    // Recent chat stays, and a second pass changes nothing.
    assert!(s.reviews.iter().all(|r| !r.messages.is_empty()));
    assert!(!prune(&mut s, now));

    // A month after a closed pull request's review last changed, its chat
    // goes; an open one's stays.
    let later = at("2026-11-01T10:00:00Z");
    assert!(prune(&mut s, later));
    for r in &s.reviews {
        assert_eq!(r.messages.is_empty(), r.pr_id == closed.id, "{}", r.id);
    }
    assert_eq!(s.reviews.len(), 8);
}

#[test]
fn a_review_still_in_use_is_never_pruned() {
    let open = crate::types::tests::pr();
    let mut s = Snapshot {
        history_prs: vec![open.clone()],
        ..Default::default()
    };
    let old = "2026-01-01T10:00:00.000Z";
    for n in 1..=4 {
        s.reviews.push(review(&open, n, "ready", old));
    }
    // The oldest is running, the next has a worktree left to clean up.
    s.reviews[0].status = "reviewing".into();
    s.reviews[1].worktree = Some("/tmp/w".into());
    assert!(prune(&mut s, at("2026-10-01T10:00:00Z")));
    assert_eq!(ids(&s), ["a/b#1-1", "a/b#1-2", "a/b#1-3", "a/b#1-4"]);
    assert!(!s.reviews[0].messages.is_empty() && !s.reviews[1].messages.is_empty());
    assert!(s.reviews[2].messages.is_empty() && s.reviews[3].messages.is_empty());
}
