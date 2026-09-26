use super::*;
use crate::types::tests::pr;
use serde_json::Value;

fn finding(id: &str, priority: u8, title: &str) -> Finding {
    Finding {
        id: id.into(),
        title: title.into(),
        priority,
        path: Some("src/a.rs".into()),
        start_line: 1,
        end_line: 2,
        body: String::new(),
        confidence: 0.9,
    }
}
fn prior(id: &str, status: PriorStatus) -> PriorFinding {
    PriorFinding {
        id: id.into(),
        status,
        note: String::new(),
    }
}
fn snapshot() -> (Snapshot, PullRequest) {
    let p = pr();
    let mut s = Snapshot::default();
    s.settings.automatic = true;
    s.settings.auto_post = true;
    s.settings.repositories.push(Repository {
        name: "a/b".into(),
        path: "/tmp/a-b".into(),
        automatic: true,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
    });
    s.prs.push(p.clone());
    (s, p)
}
/// A finished automatic round on `head`, published, with these findings.
fn round(p: &PullRequest, head: &str, minute: u32, findings: Vec<Finding>) -> Review {
    let mut r = Review::new(p, &Settings::default(), true);
    r.head_sha = head.into();
    r.created_at = format!("2026-01-01T00:{minute:02}:00.000Z");
    r.status = "posted".into();
    r.report = "Report".into();
    r.findings = Some(findings);
    decide(&mut r, 1);
    r
}
fn status(s: &Snapshot, p: &PullRequest) -> (LoopStatus, Option<String>) {
    let l = state(s, p).unwrap();
    (l.status, l.reason)
}

#[test]
fn verdict_follows_the_blocking_priority_not_the_model() {
    let p = pr();
    let mut r = round(&p, "h", 0, vec![finding("r-0", 2, "Minor")]);
    r.verdict = Some(Verdict::ChangesRequested);
    decide(&mut r, 1);
    assert_eq!(
        (r.blocking, r.orbit_verdict),
        (Some(0), Some(Verdict::Clean))
    );
    assert_eq!(r.verdict, Some(Verdict::ChangesRequested));
    decide(&mut r, 2);
    assert_eq!(
        (r.blocking, r.orbit_verdict),
        (Some(1), Some(Verdict::ChangesRequested))
    );
    r.findings = Some(vec![finding("r-0", 0, "Severe"), finding("r-1", 1, "Bad")]);
    decide(&mut r, 0);
    assert_eq!(r.blocking, Some(1));
    // Earlier findings count while they are still open or disputed.
    r.findings = Some(vec![]);
    r.previous_round = Some(PreviousRound {
        review_id: "old".into(),
        head_sha: "old".into(),
        verdict: None,
        findings: vec![
            finding("old-0", 1, "Open"),
            finding("old-1", 0, "Disputed"),
            finding("old-2", 0, "Accepted"),
            finding("old-3", 3, "Low"),
        ],
        decisions: vec![],
        comments_included: 0,
        comments_omitted: 0,
    });
    r.prior_findings = vec![
        prior("old-0", PriorStatus::StillOpen),
        prior("old-1", PriorStatus::DeclinedDisputed),
        prior("old-2", PriorStatus::DeclinedAccepted),
        prior("old-3", PriorStatus::StillOpen),
    ];
    decide(&mut r, 1);
    assert_eq!(
        (r.blocking, r.orbit_verdict),
        (Some(2), Some(Verdict::ChangesRequested))
    );
    r.findings = None;
    decide(&mut r, 1);
    assert_eq!((r.blocking, r.orbit_verdict), (None, None));
}
#[test]
fn states_follow_the_latest_automatic_round() {
    let (mut s, p) = snapshot();
    assert!(state(&s, &p).is_none());
    // Manual reviews are not part of the loop.
    let mut manual = round(&p, &p.head_sha, 0, vec![]);
    manual.automatic = false;
    s.reviews.push(manual);
    assert!(state(&s, &p).is_none());

    s.reviews
        .push(round(&p, &p.head_sha, 1, vec![finding("x-0", 1, "Bug")]));
    assert_eq!(status(&s, &p), (LoopStatus::WaitingForAuthor, None));
    let l = state(&s, &p).unwrap();
    assert_eq!((l.rounds, l.round_limit), (1, 4));

    s.reviews.last_mut().unwrap().findings = Some(vec![finding("x-0", 2, "Nit")]);
    decide(s.reviews.last_mut().unwrap(), 1);
    assert_eq!(status(&s, &p), (LoopStatus::Clean, None));
    // A clean round on an older head means the new head is next.
    s.reviews.last_mut().unwrap().head_sha = "old".into();
    assert_eq!(status(&s, &p), (LoopStatus::Reviewing, None));

    let mut running = round(&p, &p.head_sha, 2, vec![]);
    running.status = "reviewing".into();
    running.findings = None;
    let running_id = running.id.clone();
    s.reviews.push(running);
    assert_eq!(status(&s, &p), (LoopStatus::Reviewing, None));
    s.approvals.push(Approval {
        id: 1.into(),
        review_id: running_id,
        method: "item/commandExecution/requestApproval".into(),
        params: Value::Null,
    });
    assert_eq!(
        status(&s, &p),
        (LoopStatus::NeedsYou, Some("An approval is pending.".into()))
    );
}
#[test]
fn needs_a_person_when_a_round_cannot_continue_on_its_own() {
    let (mut s, p) = snapshot();
    let bug = || vec![finding("x-0", 1, "Bug")];
    let needs = |s: &Snapshot| status(s, &p).1.unwrap_or_default();

    s.reviews.push(round(&p, &p.head_sha, 0, bug()));
    let r = s.reviews.last_mut().unwrap();
    r.status = "failed".into();
    assert_eq!(needs(&s), "The review failed.");

    let r = s.reviews.last_mut().unwrap();
    r.status = "ready".into();
    r.findings = None;
    decide(r, 1);
    assert_eq!(needs(&s), "The review has no structured findings.");

    let r = s.reviews.last_mut().unwrap();
    r.findings = Some(bug());
    decide(r, 1);
    r.error = Some("Not published automatically: …".into());
    assert_eq!(needs(&s), "Publication was held.");
    s.reviews.last_mut().unwrap().error = None;
    assert_eq!(status(&s, &p).0, LoopStatus::Reviewing);
    s.settings.auto_post = false;
    assert_eq!(needs(&s), "The report is waiting for you to publish it.");

    let r = s.reviews.last_mut().unwrap();
    r.status = "posted".into();
    r.prior_findings = vec![prior("y-0", PriorStatus::DeclinedDisputed)];
    assert_eq!(needs(&s), "The reviewer disputes a declined finding.");
}
#[test]
fn the_round_limit_stops_new_rounds_until_continued() {
    let (mut s, p) = snapshot();
    s.settings.repositories[0].round_limit = 2;
    s.reviews
        .push(round(&p, "one", 0, vec![finding("a-0", 1, "A")]));
    assert!(allows_new_round(&p, &s));
    s.reviews
        .push(round(&p, &p.head_sha, 1, vec![finding("b-0", 1, "B")]));
    assert!(!allows_new_round(&p, &s));
    assert_eq!(
        status(&s, &p),
        (LoopStatus::NeedsYou, Some("Round limit reached.".into()))
    );
    // Superseded, stopped and manual runs are not rounds.
    let mut superseded = round(&p, "x", 2, vec![]);
    superseded.superseded = true;
    let mut cancelled = round(&p, "y", 3, vec![]);
    cancelled.status = "cancelled".into();
    let mut manual = round(&p, "z", 4, vec![]);
    manual.automatic = false;
    s.reviews.extend([superseded, cancelled, manual]);
    assert_eq!(state(&s, &p).unwrap().rounds, 2);
    // Only this PR is limited.
    let mut other = p.clone();
    other.id = "a/b#2".into();
    assert!(allows_new_round(&other, &s));

    s.loops.push(LoopControl {
        pr_id: p.id.clone(),
        stopped: false,
        extra_rounds: 1,
    });
    assert!(allows_new_round(&p, &s));
    assert_eq!(status(&s, &p), (LoopStatus::WaitingForAuthor, None));
    s.loops[0].stopped = true;
    assert!(!allows_new_round(&p, &s));
    assert_eq!(status(&s, &p), (LoopStatus::Stopped, None));
}
#[test]
fn a_loop_that_keeps_the_same_blocking_findings_is_not_converging() {
    let (mut s, p) = snapshot();
    let first = round(&p, "one", 0, vec![finding("one-0", 1, "Race")]);
    let mut second = round(&p, &p.head_sha, 1, vec![finding("two-0", 3, "Nit")]);
    second.previous_round = Some(PreviousRound {
        review_id: first.id.clone(),
        head_sha: "one".into(),
        verdict: None,
        findings: first.findings.clone().unwrap(),
        decisions: vec![],
        comments_included: 0,
        comments_omitted: 0,
    });
    second.prior_findings = vec![prior("one-0", PriorStatus::StillOpen)];
    decide(&mut second, 1);
    s.reviews.extend([first, second]);
    assert_eq!(
        status(&s, &p),
        (
            LoopStatus::NeedsYou,
            Some("The loop is not converging.".into())
        )
    );
    // Progress on the blocking finding means it is converging.
    s.reviews[1].prior_findings = vec![prior("one-0", PriorStatus::Fixed)];
    decide(&mut s.reviews[1], 1);
    assert_eq!(status(&s, &p).0, LoopStatus::Clean);
    // Without statuses, the same blocking path and title in both rounds.
    let r = &mut s.reviews[1];
    r.previous_round = None;
    r.prior_findings.clear();
    r.findings = Some(vec![finding("two-0", 1, "race")]);
    decide(r, 1);
    assert_eq!(
        status(&s, &p),
        (
            LoopStatus::NeedsYou,
            Some("The loop is not converging.".into())
        )
    );
    s.reviews[1].findings = Some(vec![finding("two-0", 1, "Another bug")]);
    assert_eq!(status(&s, &p).0, LoopStatus::WaitingForAuthor);
}
#[test]
fn a_new_head_waits_one_poll_interval_before_its_round() {
    let (mut s, mut p) = snapshot();
    p.head_seen_at = now();
    assert!(!allows_new_round(&p, &s));
    p.head_seen_at = (chrono::Utc::now() - chrono::Duration::seconds(121)).to_rfc3339();
    assert!(allows_new_round(&p, &s));
    p.head_seen_at = now();
    s.settings.settle_pushes = false;
    assert!(allows_new_round(&p, &s));
    // Older state has no time: settled.
    s.settings.settle_pushes = true;
    p.head_seen_at.clear();
    assert!(allows_new_round(&p, &s));
}
