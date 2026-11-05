use super::*;
use crate::actions_storage::GB;
use serde_json::json;

fn now() -> DateTime<Utc> {
    time("2026-11-05T12:00:00.000Z").unwrap()
}
fn ago(minutes: i64) -> String {
    stamp(now() - Duration::minutes(minutes))
}
fn roomy() -> Option<Disk> {
    Some(Disk {
        free: 200 * GB,
        total: 500 * GB,
    })
}
/// A workspace with one repository reviewed by Codex, connected.
fn snapshot() -> Snapshot {
    let mut s = Snapshot {
        codex_ready: true,
        ..Default::default()
    };
    s.settings
        .repositories
        .push(Repository::new("a/b", "/tmp/a-b"));
    s
}
fn kinds(found: &[Found]) -> Vec<HealthKind> {
    found.iter().map(|f| f.kind).collect()
}

#[test]
fn nothing_is_wrong_with_a_healthy_orbit() {
    assert!(find(&snapshot(), roomy(), None, now()).is_empty());
    // A disk Orbit can't read says nothing.
    assert!(find(&snapshot(), None, None, now()).is_empty());
}

#[test]
fn little_free_disk_space_is_wrong() {
    let s = snapshot();
    let disk = |free, total| find(&s, Some(Disk { free, total }), None, now());
    // Under 2 GB, or under 5% of the disk.
    let low = disk(GB, 500 * GB);
    assert_eq!(kinds(&low), [HealthKind::Disk]);
    assert!(low[0].text.starts_with("Only 1.0 GB of 500.0 GB is free"));
    assert!(low[0].text.contains("Settings → Storage"));
    assert_eq!(kinds(&disk(4 * GB, 100 * GB)), [HealthKind::Disk]);
    assert!(disk(10 * GB, 100 * GB).is_empty());
    assert!(disk(3 * GB, 40 * GB).is_empty());
}

#[test]
fn worktrees_orbit_could_not_remove_are_wrong() {
    let mut s = snapshot();
    let pr = crate::types::tests::pr();
    let mut r = Review::new(&pr, &s.settings, false);
    r.cleanup_error = Some("Permission denied".into());
    s.reviews.push(r.clone());
    let found = find(&s, roomy(), None, now());
    assert_eq!(kinds(&found), [HealthKind::Leftovers]);
    assert_eq!(
        found[0].text,
        "Orbit couldn't remove the worktree of 1 review: Permission denied. Retry under Settings → Storage."
    );
    s.reviews.push(r);
    assert!(find(&s, roomy(), None, now())[0]
        .text
        .contains("of 2 reviews:"));
}

#[test]
fn writes_to_github_that_have_not_settled_after_an_hour_are_wrong() {
    let mut s = snapshot();
    let write = |at: String, state: &str, number: Option<u64>| -> crate::ops::Write {
        serde_json::from_value(json!({
            "id": "w", "thread": "pr:a/b#3", "action": "post_comment",
            "repo": "a/b", "number": number, "state": state, "at": at,
        }))
        .unwrap()
    };
    s.journal.push(write(ago(10), "sending", Some(3)));
    s.journal.push(write(ago(300), "done", Some(3)));
    assert!(find(&s, roomy(), None, now()).is_empty());
    s.journal.push(write(ago(90), "sending", Some(4)));
    s.journal.push(write(ago(61), "sending", None));
    let found = find(&s, roomy(), None, now());
    assert_eq!(kinds(&found), [HealthKind::Journal]);
    assert_eq!(
        found[0].text,
        "2 writes to GitHub haven't settled after an hour, the oldest for a/b#4. Orbit looks for each on GitHub before it tries again, so nothing posts twice; Operations lists them."
    );
}

#[test]
fn posts_the_second_look_held_for_over_a_day_are_wrong() {
    let mut s = snapshot();
    s.settings.buddy.enabled = true;
    s.settings.buddy.second_look = "on".into();
    let thread = |key: &str| crate::ops::Thread {
        key: key.into(),
        problem_id: "p1".into(),
        ..Default::default()
    };
    let look = |key: &str, asked: i64| crate::looks::Look {
        thread: key.into(),
        problem: "p1".into(),
        asked_at: ago(asked),
        ..Default::default()
    };
    s.ops.push(thread("pr:a/b#3"));
    s.looks.push(look("pr:a/b#3", 60));
    assert!(find(&s, roomy(), None, now()).is_empty());
    s.ops.push(thread("pr:a/b#4"));
    s.looks.push(look("pr:a/b#4", 25 * 60));
    let found = find(&s, roomy(), None, now());
    assert_eq!(kinds(&found), [HealthKind::SecondLook]);
    assert_eq!(
        found[0].text,
        "1 post the second look held has waited more than a day for you. Post or drop it on Operations."
    );
    // Watch never holds a post.
    s.settings.buddy.second_look = "watch".into();
    assert!(find(&s, roomy(), None, now()).is_empty());
}

#[test]
fn codex_disconnected_for_a_while_is_wrong_while_something_needs_it() {
    let mut s = snapshot();
    s.codex_ready = false;
    s.codex_error = Some("Codex exited.".into());
    let since = |minutes| Some(now() - Duration::minutes(minutes));
    assert!(find(&s, roomy(), since(2), now()).is_empty());
    let found = find(&s, roomy(), since(6), now());
    assert_eq!(kinds(&found), [HealthKind::Codex]);
    assert_eq!(
        found[0].text,
        "Codex isn't connected, so a/b can't be reviewed: Codex exited."
    );
    assert!(!HealthKind::Codex.needs_you());
    // Nothing reviewed by Codex needs it.
    s.settings.repositories[0].provider = Provider::Claude;
    assert!(find(&s, roomy(), since(60), now()).is_empty());
}

#[test]
fn codex_usage_near_a_limit_is_wrong() {
    let mut s = snapshot();
    let usage = |used: f64, reached: Option<&str>| -> crate::usage::Usage {
        serde_json::from_value(json!({
            "rateLimits": {
                "primary": {"usedPercent": used, "resetsAt": (now() + Duration::hours(3)).timestamp()},
                "rateLimitReachedType": reached,
            },
            "otherLimits": [],
            "updatedAt": stamp(now()),
        }))
        .unwrap()
    };
    s.usage = Some(usage(80.0, None));
    assert!(find(&s, roomy(), None, now()).is_empty());
    s.usage = Some(usage(93.4, None));
    let found = find(&s, roomy(), None, now());
    assert_eq!(kinds(&found), [HealthKind::Usage]);
    assert_eq!(
        found[0].text,
        "Codex has used 93% of a usage limit, which starts over in 3 hours. Write-ups and Bit's answers with Codex wait or may fail until then."
    );
    assert!(!HealthKind::Usage.needs_you());
    s.usage = Some(usage(100.0, Some("rate_limit_reached")));
    assert!(find(&s, roomy(), None, now())[0]
        .text
        .starts_with("Codex has reached a usage limit,"));
}

fn disk_found(text: &str) -> Vec<Found> {
    vec![Found {
        kind: HealthKind::Disk,
        text: text.into(),
    }]
}

#[test]
fn an_issue_opens_tells_you_once_and_closes_when_it_clears() {
    let mut issues = vec![];
    let t0 = now();
    assert!(see(&mut issues, &disk_found("Only 1.0 GB"), t0));
    assert_eq!(issues.len(), 1);
    let first = issues[0].clone();
    assert!(first.is_open() && first.alerts == 1);
    assert_eq!(first.item_key(), format!("health:{}:1", first.id));
    // Still wrong, nothing new: no change, no second alert.
    assert!(!see(
        &mut issues,
        &disk_found("Only 1.0 GB"),
        t0 + Duration::minutes(1)
    ));
    // It changed: the text and timeline follow, without telling you again.
    assert!(see(
        &mut issues,
        &disk_found("Only 0.8 GB"),
        t0 + Duration::minutes(2)
    ));
    assert_eq!(issues[0].text, "Only 0.8 GB");
    assert_eq!(issues[0].timeline.len(), 2);
    assert_eq!(issues[0].alerts, 1);
    // A day later it reminds you once.
    see(
        &mut issues,
        &disk_found("Only 0.8 GB"),
        t0 + Duration::hours(25),
    );
    assert_eq!(issues[0].alerts, 2);
    // It clears.
    assert!(see(&mut issues, &[], t0 + Duration::hours(26)));
    assert!(!issues[0].is_open());
    assert_eq!(
        issues[0].closed_reason.as_deref(),
        Some("There's room on the disk again.")
    );
    assert!(!see(&mut issues, &[], t0 + Duration::hours(27)));
}

#[test]
fn an_issue_that_comes_straight_back_is_the_same_one_and_stays_quiet() {
    let mut issues = vec![];
    let t0 = now();
    see(&mut issues, &disk_found("Low"), t0);
    see(&mut issues, &[], t0 + Duration::minutes(5));
    // Its Needs you item stays announced while it could come back.
    assert_eq!(
        quiet_keys(&issues, t0 + Duration::minutes(10)),
        [issues[0].item_key()]
    );
    see(&mut issues, &disk_found("Low"), t0 + Duration::minutes(10));
    assert_eq!(issues.len(), 1);
    assert!(issues[0].is_open());
    assert_eq!(issues[0].alerts, 1);
    assert_eq!(issues[0].timeline.last().unwrap().text, "Again: Low");
    // An hour after its alert it tells you, if still open.
    see(&mut issues, &disk_found("Low"), t0 + Duration::minutes(61));
    assert_eq!(issues[0].alerts, 2);
    // Later than that, it's a new one.
    see(&mut issues, &[], t0 + Duration::minutes(70));
    see(&mut issues, &disk_found("Low"), t0 + Duration::minutes(200));
    assert_eq!(issues.len(), 2);
    assert!(quiet_keys(&issues, t0 + Duration::minutes(200)).is_empty());
}

#[test]
fn closed_issues_are_kept_for_two_weeks_and_fifty_at_most() {
    let t0 = now();
    let closed = |n: i64, days: i64| HealthIssue {
        id: format!("i{n}"),
        closed_at: Some(stamp(t0 - Duration::days(days) - Duration::minutes(n))),
        ..Default::default()
    };
    let mut issues: Vec<HealthIssue> = (0..60).map(|n| closed(n, 1)).collect();
    issues.push(closed(100, 15));
    issues.push(HealthIssue {
        id: "open".into(),
        ..Default::default()
    });
    assert!(prune(&mut issues, t0));
    assert_eq!(issues.len(), MAX_CLOSED + 1);
    assert!(issues.iter().any(|i| i.id == "open"));
    assert!(issues.iter().any(|i| i.id == "i0"));
    assert!(!issues.iter().any(|i| i.id == "i59" || i.id == "i100"));
    assert!(!prune(&mut issues, t0));
}

#[test]
fn reads_the_free_space_where_orbit_keeps_its_data() {
    let d = tempfile::tempdir().unwrap();
    if cfg!(unix) {
        let disk = disk(d.path()).unwrap();
        assert!(disk.total > 0 && disk.free <= disk.total);
    } else {
        assert_eq!(disk(d.path()), None);
    }
    assert_eq!(disk(&d.path().join("missing")), None);
}
