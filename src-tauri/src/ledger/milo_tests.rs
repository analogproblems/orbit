use super::assemble_tests::fixture;
use super::*;

/// The fixture's comments that are Milo's reviews, oldest first.
fn reviews(n: u64) -> Vec<CommentRef> {
    super::assemble::comment_refs(&fixture(n).comments)
        .into_iter()
        .filter(|c| milo::is_review(&c.body))
        .collect()
}

#[test]
fn verdicts_in_every_phrasing_milo_used() {
    use RoundVerdict::*;
    let cases = [
        ("## Milo PR review\n\n**CLEAR TO MERGE — code review passed**", Clean),
        ("## Milo PR review\n\n**CHANGES REQUESTED**", ChangesRequested),
        (
            "Reviewed `abc`. I found three correctness issues to address before merging:",
            ChangesRequested,
        ),
        ("Re-reviewed `abc`. **Not clear to merge yet.**", ChangesRequested),
        (
            "Re-reviewed `abc`. **Clear to merge from my review. No remaining actionable findings.**",
            Clean,
        ),
        ("Clear to merge. Re-reviewed the update from `a` to `b`.", Clean),
        ("Reviewed `abc`. Two actionable findings:", ChangesRequested),
        ("**The original P3 finding is resolved. No new findings.**", Clean),
        (
            "## Milo PR review\n\n**BLOCKED: TESTBOX is offline.**\n\nNo actionable findings.",
            Unknown,
        ),
        ("```\nCLEAR TO MERGE\n```\nStill looking.", Unknown),
    ];
    for (body, verdict) in cases {
        assert_eq!(milo::verdict(body), verdict, "{body}");
    }
}

#[test]
fn headings_in_every_shape() {
    assert_eq!(
        milo::heading(
            "**F1 · P2 · Preserve the setup-versus-connect intent when a button becomes stale**"
        ),
        Some(milo::Heading {
            number: Some(1),
            priority: 2,
            title: "Preserve the setup-versus-connect intent when a button becomes stale".into(),
            ruling: None,
        })
    );
    let title = |line: &str| milo::heading(line).unwrap().title;
    assert_eq!(
        title("1. **F2 · P1 — Revalidate queued fixes before performing their side effects.** `src-tauri/src/engine/fixes.rs:35–40` drains"),
        "Revalidate queued fixes before performing their side effects"
    );
    assert_eq!(
        milo::heading("1. **F2 remaining · P2 — Recheck the automatic sync after acquiring its lock.** `queue.rs`")
            .unwrap()
            .ruling,
        Some(PriorStatus::StillOpen)
    );
    assert_eq!(
        milo::heading("1. **[P2] Serialize status writes by repository and commit, and let the newest round win.** [`src-tauri/src/engine/status.rs:56–73`](https://x)")
            .unwrap()
            .priority,
        2
    );
    assert_eq!(
        title("**[P2] Claim failed worktrees before allowing follow-ups — [src-tauri/src/engine.rs:547](https://x)**"),
        "Claim failed worktrees before allowing follow-ups"
    );
    assert_eq!(
        milo::heading("**F1 · P2 · [src-tauri/src/engine/settings.rs:100–107](https://x): keep the entire download-and-connect operation alive after request cancellation.**")
            .unwrap()
            .number,
        Some(1)
    );
    let ruling = |line: &str| milo::heading(line).unwrap().ruling;
    assert_eq!(
        ruling("- **F1 fixed:** rules now use snake_case"),
        Some(PriorStatus::Fixed)
    );
    assert_eq!(
        ruling("- **F2 partially fixed:** Continue loop"),
        Some(PriorStatus::StillOpen)
    );
    assert_eq!(
        ruling("**F1 (P2), fixed.** Rules and RulesPatch now use"),
        Some(PriorStatus::Fixed)
    );
    assert_eq!(
        title("### Problem 2: Off-by-one in first()"),
        "Off-by-one in first()"
    );
    assert_eq!(milo::heading("Plain text"), None);
}

#[test]
fn pr_72_one_finding_then_clear_to_merge_with_f1_fixed() {
    let comments = reviews(72);
    assert_eq!(comments.len(), 2);
    let r1 = milo::parse_review(&comments[0]).unwrap();
    assert_eq!(r1.verdict, RoundVerdict::ChangesRequested);
    assert_eq!(
        r1.head_sha.as_deref(),
        Some("82c6ee9533eeb0a34dabaad079c6eb36446edb2e")
    );
    assert_eq!(
        r1.base_sha.as_deref(),
        Some("2af308acb7f6fa69ee062905c86485c0119ef5dd")
    );
    assert_eq!(r1.findings.len(), 1);
    let f = &r1.findings[0];
    assert_eq!(f.short_id.as_deref(), Some("F1"));
    assert_eq!(f.priority, 2);
    assert!(f
        .failure_scenario
        .as_deref()
        .unwrap()
        .starts_with("Bit offers a"));
    assert_eq!(
        f.locations[0].path.as_deref(),
        Some("src-tauri/src/engine/ask.rs")
    );
    assert_eq!(f.confirmed, Some(false));
    let r2 = milo::parse_review(&comments[1]).unwrap();
    assert_eq!(r2.verdict, RoundVerdict::Clean);
    assert_eq!(
        r2.head_sha.as_deref(),
        Some("f2336fce6fd39483d17b3767e2f748f26aa0a5c3")
    );
    let earlier: Vec<(Option<&str>, Option<PriorStatus>)> = r2
        .earlier_findings
        .iter()
        .map(|e| (e.short_id.as_deref(), e.status))
        .collect();
    assert_eq!(earlier, [(Some("F1"), Some(PriorStatus::Fixed))]);
}

#[test]
fn pr_74_colon_separated_marker_still_yields_head_and_base() {
    let r = milo::parse_review(&reviews(74)[0]).unwrap();
    assert_eq!(
        r.head_sha.as_deref(),
        Some("867276bfba41e1b2bcd833e92ac821a94af8ad87")
    );
    assert_eq!(
        r.base_sha.as_deref(),
        Some("b3500ca862651e048f01644f5d7f6efb6e328f85")
    );
    assert_eq!(r.findings[0].number, Some(1));
}

#[test]
fn pr_21_early_prose_three_findings_then_two_then_one() {
    let comments = reviews(21);
    assert_eq!(comments.len(), 4);
    let counts: Vec<usize> = comments
        .iter()
        .map(|c| milo::parse_review(c).unwrap().findings.len())
        .collect();
    assert_eq!(counts, [3, 2, 1, 0]);
    assert_eq!(
        milo::parse_review(&comments[3]).unwrap().verdict,
        RoundVerdict::Clean
    );
    assert_eq!(
        milo::parse_review(&comments[0]).unwrap().findings[0].confirmed,
        Some(true)
    );
}

#[test]
fn what_the_reviewer_did_not_check_is_kept() {
    let c = CommentRef {
        id: 1,
        kind: CommentKind::Issue,
        author: "a".into(),
        role: Role::Reviewer,
        created_at: "2026-10-11T00:00:00Z".into(),
        url: "u".into(),
        body: "## Milo PR review\n\n**CLEAR TO MERGE**\n\nThe Rust tests were not run. Everything else passed.".into(),
        path: None,
        line: None,
        start_line: None,
        in_reply_to: None,
        review_id: None,
        finding_marker: None,
        edit: None,
    };
    let r = milo::parse_review(&c).unwrap();
    assert_eq!(r.not_checked, ["The Rust tests were not run."]);
    assert_eq!(r.reviewer.as_deref(), Some("Milo"));
}
