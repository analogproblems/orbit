use super::*;
use crate::{
    problems::Problem,
    security::{Alert, Severity},
};
use serde_json::json;

fn day(d: &str) -> NaiveDate {
    NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()
}
fn release(tag: &str, draft: bool) -> Release {
    Release {
        id: 1,
        tag: tag.into(),
        name: tag.into(),
        draft,
        ..Default::default()
    }
}
fn tag(name: &str) -> Tag {
    Tag {
        name: name.into(),
        sha: "1".repeat(40),
    }
}
/// A milestone as GitHub's answer has it.
fn node(number: u64, title: &str, due: Option<&str>, prs: &[u64], issues: &[u64]) -> Value {
    let list = |n: &[u64], total: usize| {
        json!({
            "totalCount": total,
            "nodes": n.iter().map(|n| json!({ "number": n })).collect::<Vec<_>>(),
        })
    };
    json!({
        "number": number,
        "title": title,
        "dueOn": due,
        "url": format!("https://github.com/a/b/milestone/{number}"),
        "pullRequests": list(prs, prs.len()),
        "issues": list(issues, issues.len()),
    })
}
fn milestone(number: u64, due: &str) -> Milestone {
    Milestone {
        number,
        title: "v0.3.0".into(),
        version: "v0.3.0".into(),
        due: due.into(),
        url: format!("https://github.com/a/b/milestone/{number}"),
        ..Default::default()
    }
}
/// `a/b`, connected, whose releases Orbit read, with `milestones`.
fn repo(milestones: Vec<Milestone>) -> Snapshot {
    let mut s = Snapshot::default();
    s.settings.watch_ci = true;
    s.settings.buddy.time_zone = "Etc/UTC".into();
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
        milestones,
        ..Default::default()
    });
    s
}
fn at(time: &str) -> DateTime<Utc> {
    time.parse().unwrap()
}

/// A milestone is a release date when its title, or its last word, names
/// a version and it has a due date. Those due soonest are kept, with their
/// open issues and pull requests by number, and one whose version is out
/// is marked released.
#[test]
fn a_milestone_named_for_a_version_with_a_date_is_a_release_date() {
    assert_eq!(version_of("v0.3.0"), Some("v0.3.0"));
    assert_eq!(version_of(" Release 1.2.0-rc.1 "), Some("1.2.0-rc.1"));
    for other in ["Backlog", "v0.3", "0.3.0 release", "", "Next: app@1.0.0"] {
        assert_eq!(version_of(other), None, "{other}");
    }

    let nodes = json!({ "nodes": [
        node(1, "Backlog", Some("2026-11-03T07:00:00Z"), &[], &[]),
        node(2, "v0.4.0", None, &[], &[]),
        node(3, "Release v0.3.0", Some("2026-11-04T07:00:00Z"), &[12], &[9, 10]),
        node(4, "v0.2.1", Some("2026-11-01T00:00:00Z"), &[], &[]),
        node(5, "v0.5.0", Some("not a day"), &[], &[]),
    ] });
    let out = understand(&nodes, &[release("v0.2.1", false)], &[]);
    assert_eq!(
        out,
        [
            Milestone {
                released: true,
                title: "v0.2.1".into(),
                version: "v0.2.1".into(),
                ..milestone(4, "2026-11-01")
            },
            Milestone {
                title: "Release v0.3.0".into(),
                prs: vec![12],
                pr_count: 1,
                issues: vec![9, 10],
                issue_count: 2,
                ..milestone(3, "2026-11-04")
            },
        ]
    );
    // A draft isn't out; `0.3.0` is the same version as `v0.3.0`.
    let out = understand(&nodes, &[release("v0.3.0", true)], &[]);
    assert!(!out[1].released);
    let out = understand(&nodes, &[release("0.3.0", false)], &[]);
    assert!(out[1].released);
    // With no releases at all, a tag is how a version comes out.
    let out = understand(&nodes, &[], &[tag("v0.3.0")]);
    assert!(out[1].released);
    let out = understand(&nodes, &[release("v0.1.0", false)], &[tag("v0.3.0")]);
    assert!(!out[1].released);

    // Only the first few of each list by number, and at most `KEPT`.
    let many: Vec<u64> = (1..=8).collect();
    let mut big = node(6, "v1.0.0", Some("2026-12-01T00:00:00Z"), &many, &[]);
    big["pullRequests"]["totalCount"] = json!(40);
    let out = understand(&json!({ "nodes": [big] }), &[], &[]);
    assert_eq!(out[0].prs, [1, 2, 3, 4, 5]);
    assert_eq!(out[0].pr_count, 40);
    let lots: Vec<Value> = (1..=9)
        .map(|n| {
            node(
                n,
                &format!("v0.{n}.0"),
                Some("2026-12-01T00:00:00Z"),
                &[],
                &[],
            )
        })
        .collect();
    let out = understand(&json!({ "nodes": lots }), &[], &[]);
    assert_eq!(out.len(), KEPT);
    // No milestones in the answer, or a link off GitHub.
    assert!(understand(&Value::Null, &[], &[]).is_empty());
    let mut odd = node(7, "v1.0.0", Some("2026-12-01T00:00:00Z"), &[], &[]);
    odd["url"] = json!("https://example.com/m/7");
    assert_eq!(understand(&json!({ "nodes": [odd] }), &[], &[])[0].url, "");
}

/// The reminders' steps: from a week before, the day before, on the day,
/// then each day late, each with its own key so each is announced once.
#[test]
fn reminders_come_a_week_before_the_day_before_on_the_day_and_daily_late() {
    let today = day("2026-11-02");
    assert_eq!(when("2026-11-10", today), None);
    assert_eq!(when("2026-11-09", today), Some(When::Soon(7)));
    assert_eq!(when("2026-11-04", today), Some(When::Soon(2)));
    assert_eq!(when("2026-11-03", today), Some(When::Tomorrow));
    assert_eq!(when("2026-11-02", today), Some(When::Today));
    assert_eq!(when("2026-11-01", today), Some(When::Late(1)));
    assert_eq!(when("2026-10-20", today), Some(When::Late(13)));
    assert_eq!(when("soon", today), None);

    let s = repo(vec![milestone(3, "2026-11-04")]);
    let keys = |time: &str| -> Vec<String> { waiting(&s, at(time)).iter().map(key).collect() };
    assert!(keys("2026-10-27T12:00:00Z").is_empty());
    for (time, step) in [
        ("2026-10-28T00:00:00Z", "week"),
        ("2026-11-02T23:59:00Z", "week"),
        ("2026-11-03T08:00:00Z", "tomorrow"),
        ("2026-11-04T08:00:00Z", "today"),
        ("2026-11-05T08:00:00Z", "late-1"),
        ("2026-11-06T08:00:00Z", "late-2"),
    ] {
        assert_eq!(keys(time), [format!("due:a/b:3:{step}")], "{time}");
    }
    // By the day in Bit's time zone: in Auckland, it's the 4th already.
    let mut s = s;
    s.settings.buddy.time_zone = "Pacific/Auckland".into();
    assert_eq!(
        waiting(&s, at("2026-11-03T12:00:00Z"))
            .iter()
            .map(key)
            .collect::<Vec<_>>(),
        ["due:a/b:3:today"]
    );
}

/// What the reminder says, with what stands in the release's way: the
/// milestone's open pull requests and issues, version files that
/// disagree, a failing default branch and high or critical security
/// alerts. Nothing is due once the release is out, the milestone is gone,
/// or Orbit doesn't watch the repository's releases.
#[test]
fn a_release_due_says_what_stands_in_its_way() {
    let now = at("2026-11-02T12:00:00Z");
    let s = repo(vec![milestone(3, "2026-11-04")]);
    let due = waiting(&s, now);
    assert_eq!(
        reason(&due[0]),
        "v0.3.0 of a/b is due in 2 days. Nothing Orbit watches stands in its way."
    );

    let mut s = repo(vec![Milestone {
        prs: vec![12],
        pr_count: 1,
        issues: vec![9, 10, 11, 13, 14],
        issue_count: 7,
        ..milestone(3, "2026-11-03")
    }]);
    let problem = |kind: ProblemKind, repo: &str| Problem {
        id: format!("{kind:?}"),
        kind,
        repo: repo.into(),
        branch: "main".into(),
        ..Default::default()
    };
    let alert = |severity: Severity| Alert {
        severity,
        ..Default::default()
    };
    s.problems.push(problem(ProblemKind::Versions, "a/b"));
    s.problems.push(problem(ProblemKind::DefaultBranch, "a/b"));
    s.problems.push(Problem {
        security_alerts: vec![alert(Severity::Critical), alert(Severity::Medium)],
        ..problem(ProblemKind::Security, "a/b")
    });
    s.problems.push(Problem {
        security_alerts: vec![alert(Severity::High)],
        ..problem(ProblemKind::Security, "a/b")
    });
    // Another repository's, and a closed one, aren't in its way.
    s.problems.push(problem(ProblemKind::Versions, "a/c"));
    s.problems.push(Problem {
        closed_at: Some("2026-11-01T00:00:00Z".into()),
        ..problem(ProblemKind::Tag, "a/b")
    });
    let due = waiting(&s, now);
    assert_eq!(
        reason(&due[0]),
        "v0.3.0 of a/b is due tomorrow. In its way: 1 open pull request (#12), \
         7 open issues (#9, #10, #11, #13, #14 and 2 more), version files that disagree, \
         CI failing on main and 2 high or critical security alerts."
    );
    s.problems.retain(|p| p.kind != ProblemKind::Security);
    s.problems[0].closed_at = Some("2026-11-02T00:00:00Z".into());
    s.releases[0].milestones[0].issue_count = 0;
    s.releases[0].milestones[0].issues.clear();
    let late = waiting(&s, at("2026-11-05T12:00:00Z"));
    assert_eq!(
        reason(&late[0]),
        "v0.3.0 of a/b was due 2 days ago. In its way: 1 open pull request (#12) and CI failing on main."
    );
    let yesterday = waiting(&s, at("2026-11-04T12:00:00Z"));
    assert!(reason(&yesterday[0]).starts_with("v0.3.0 of a/b was due yesterday."));

    // Out already, or nothing more to watch.
    let quiet = |change: &dyn Fn(&mut Snapshot)| {
        let mut s = repo(vec![milestone(3, "2026-11-03")]);
        change(&mut s);
        waiting(&s, now).is_empty()
    };
    assert!(!quiet(&|_| {}));
    assert!(quiet(&|s| s.releases[0].milestones[0].released = true));
    assert!(quiet(&|s| s.releases[0].milestones.clear()));
    assert!(quiet(&|s| s.settings.watch_ci = false));
    assert!(quiet(&|s| s.settings.repositories.clear()));
    assert!(quiet(&|s| s.releases[0].unseen = Some("Hidden".into())));
    assert!(quiet(&|s| s.releases[0].read_at.clear()));
}

/// Through the Needs you list: one item a step, which opens Operations.
#[test]
fn a_release_due_is_on_needs_you() {
    let today = crate::clock::local("Etc/UTC", Utc::now()).date();
    let due = (today + chrono::Duration::days(2))
        .format("%Y-%m-%d")
        .to_string();
    let s = repo(vec![Milestone {
        prs: vec![12],
        pr_count: 1,
        ..milestone(3, &due)
    }]);
    let items: Vec<_> = crate::attention::items(&s)
        .into_iter()
        .filter(|a| a.key.starts_with("due:"))
        .collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].key, "due:a/b:3:week");
    assert_eq!(items[0].kind, AttentionKind::Release);
    assert_eq!(items[0].repo.as_deref(), Some("a/b"));
    assert_eq!(items[0].fix, None);
    assert_eq!(
        items[0].reason,
        "v0.3.0 of a/b is due in 2 days. In its way: 1 open pull request (#12)."
    );
}
