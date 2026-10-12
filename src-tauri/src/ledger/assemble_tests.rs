use super::*;

/// One of Orbit's own pull requests as GitHub returned it, trimmed to the
/// fields Orbit reads (recorded by PRism).
pub(super) fn fixture(n: u64) -> StoredPr {
    let text = match n {
        21 => include_str!("../../../test-fixtures/ledger/orbit-pr-21.json"),
        64 => include_str!("../../../test-fixtures/ledger/orbit-pr-64.json"),
        72 => include_str!("../../../test-fixtures/ledger/orbit-pr-72.json"),
        74 => include_str!("../../../test-fixtures/ledger/orbit-pr-74.json"),
        75 => include_str!("../../../test-fixtures/ledger/orbit-pr-75.json"),
        _ => panic!("no fixture for #{n}"),
    };
    let v: Value = serde_json::from_str(text).unwrap();
    let list = |key: &str| v[key].as_array().cloned().unwrap_or_default();
    from_github(
        "analogproblems/orbit",
        &v["pr"],
        &list("issueComments"),
        &list("reviews"),
        &list("reviewComments"),
        &list("commits"),
        &list("files"),
        None,
        "2026-10-12T00:00:00Z",
    )
}
fn by_short_id(h: &History, id: &str) -> Trajectory {
    h.trajectories
        .iter()
        .find(|t| t.short_id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("no {id}"))
        .clone()
}

#[test]
fn pr_21_four_rounds_each_fix_claim_attached_to_the_round_it_answered() {
    let h = derive(&fixture(21));
    let rounds: Vec<(Option<u32>, RoundVerdict, usize)> = h
        .rounds
        .iter()
        .map(|r| (r.round, r.verdict, r.findings.len()))
        .collect();
    assert_eq!(
        rounds,
        [
            (Some(1), RoundVerdict::ChangesRequested, 3),
            (Some(2), RoundVerdict::ChangesRequested, 2),
            (Some(3), RoundVerdict::ChangesRequested, 1),
            (Some(4), RoundVerdict::Clean, 0),
        ]
    );
    let commits: Vec<Option<&str>> = h
        .trajectories
        .iter()
        .map(|t| t.claim.as_ref().and_then(|c| c.commit.as_deref()))
        .collect();
    assert_eq!(
        commits,
        [
            Some("1e8b66f"),
            Some("1e8b66f"),
            Some("1e8b66f"),
            Some("0ab9947"),
            Some("0ab9947"),
            Some("acf81ae")
        ]
    );
    assert!(h
        .trajectories
        .iter()
        .all(|t| t.final_status == FinalStatus::Fixed));
    assert_eq!(h.metrics.rounds, 4);
    assert_eq!(h.metrics.open_at_merge, 0);
    assert_eq!(h.metrics.merged_by_loop, Some(true));
    assert!(h.flags.is_empty(), "{:?}", h.flags);
}

#[test]
fn pr_64_f1_to_f4_across_four_rounds_with_rulings_and_named_claims() {
    let h = derive(&fixture(64));
    let mut ids: Vec<&str> = h
        .trajectories
        .iter()
        .filter_map(|t| t.short_id.as_deref())
        .collect();
    ids.sort();
    assert_eq!(ids, ["F1", "F2", "F3", "F4"]);
    let commit = |id: &str| by_short_id(&h, id).claim.and_then(|c| c.commit);
    assert_eq!(commit("F1").as_deref(), Some("3a235dc"));
    assert_eq!(commit("F2").as_deref(), Some("3e0a5fa"));
    let f2: Vec<PriorStatus> = by_short_id(&h, "F2")
        .rulings
        .iter()
        .map(|r| r.status)
        .collect();
    assert_eq!(
        f2,
        [
            PriorStatus::StillOpen,
            PriorStatus::StillOpen,
            PriorStatus::Fixed
        ]
    );
    assert_eq!(by_short_id(&h, "F4").opened_round, 3);
    assert!(h
        .flags
        .iter()
        .any(|f| f.kind == FlagKind::RepeatBlocking && f.finding_id.as_deref() == Some("F2")));
    assert_eq!(h.metrics.fixed, 4);
}

#[test]
fn prs_72_and_74_one_finding_fixed_in_a_named_commit_then_clean() {
    for (n, sha) in [(72, "f2336fc"), (74, "0db1179")] {
        let h = derive(&fixture(n));
        assert_eq!(h.rounds.len(), 2, "#{n}");
        let t = &h.trajectories[0];
        assert_eq!(
            t.claim.as_ref().and_then(|c| c.commit.as_deref()),
            Some(sha),
            "#{n}"
        );
        assert_eq!(t.final_status, FinalStatus::Fixed, "#{n}");
        assert!(h.narrative.epic.is_some() || h.narrative.before.is_some());
    }
}

#[test]
fn pr_75_is_clean_on_the_first_round() {
    let h = derive(&fixture(75));
    let verdicts: Vec<RoundVerdict> = h.rounds.iter().map(|r| r.verdict).collect();
    assert_eq!(verdicts, [RoundVerdict::Clean]);
    assert_eq!(h.metrics.first_verdict, Some(RoundVerdict::Clean));
    assert_eq!(h.metrics.minutes_to_clean, Some(5));
    assert_eq!(
        h.title,
        "Show the inbox list and an opened pull request one at a time on phones"
    );
    assert!(h.merged);
    assert_eq!(h.changed_files, 3);
}

#[test]
fn what_the_app_receives_leaves_out_the_conversation() {
    let h = derive(&fixture(72));
    let v = serde_json::to_value(&h).unwrap();
    assert!(v.get("comments").is_none());
    assert!(v.get("commits").is_none());
    assert!(v["rounds"][0].get("body").is_none());
    assert_eq!(v["rounds"][0]["verdict"], "changes_requested");
    assert_eq!(v["trajectories"][0]["finalStatus"], "fixed");
    assert!(!h.comments.is_empty() && !h.commits.is_empty());
}

fn comment(id: u64, at: &str, body: &str) -> StoredComment {
    StoredComment {
        id,
        kind: CommentKind::Issue,
        author: "jason".into(),
        body: body.into(),
        url: format!("https://github.com/a/b/pull/1#issuecomment-{id}"),
        created_at: at.into(),
        updated_at: at.into(),
        ..Default::default()
    }
}
fn merged_pr(comments: Vec<StoredComment>) -> StoredPr {
    StoredPr {
        version: STORED_VERSION,
        repo: "a/b".into(),
        pr: PrInfo {
            number: 1,
            title: "Add login".into(),
            url: "https://github.com/a/b/pull/1".into(),
            state: "closed".into(),
            merged: true,
            created_at: "2026-10-11T09:00:00Z".into(),
            merged_at: Some("2026-10-11T12:00:00Z".into()),
            ..Default::default()
        },
        comments,
        ..Default::default()
    }
}

#[test]
fn a_review_edited_in_place_for_a_new_head_keeps_both_rounds() {
    let first = "## Milo PR review\n\n**CHANGES REQUESTED**\n\n- Full head: `aaaaaaa`\n\n**F1 · P2 · Guard the empty list**\n\nIt panics.";
    let second = "## Milo PR review\n\n**CLEAR TO MERGE**\n\n- Full head: `bbbbbbb`\n\n- **F1 fixed:** guarded.";
    let mut review = comment(10, "2026-10-11T10:00:00Z", second);
    review.updated_at = "2026-10-11T11:00:00Z".into();
    review.revisions = vec![Revision {
        body: first.into(),
        updated_at: "2026-10-11T10:00:00Z".into(),
    }];
    let reply = comment(11, "2026-10-11T10:30:00Z", "Fixed F1 in 1234567");
    let h = derive(&merged_pr(vec![review, reply]));
    let rounds: Vec<(RoundVerdict, &str)> = h
        .rounds
        .iter()
        .map(|r| (r.verdict, r.created_at.as_str()))
        .collect();
    assert_eq!(
        rounds,
        [
            (RoundVerdict::ChangesRequested, "2026-10-11T10:00:00Z"),
            (RoundVerdict::Clean, "2026-10-11T11:00:00Z"),
        ]
    );
    let t = by_short_id(&h, "F1");
    assert_eq!(t.final_status, FinalStatus::Fixed);
    assert_eq!(t.claim.unwrap().commit.as_deref(), Some("1234567"));
    assert_eq!(t.minutes_to_claim, Some(30));
}

#[test]
fn an_edit_for_the_same_head_stays_one_round_with_the_latest_text() {
    let first = "## Milo PR review\n\nBLOCKED: required isolated testing could not start because JUPITER is offline.\n\n- Full head: `aaaaaaa`";
    let second = "## Milo PR review\n\n**CLEAR TO MERGE**\n\n- Full head: `aaaaaaa`";
    let mut review = comment(10, "2026-10-11T10:00:00Z", second);
    review.updated_at = "2026-10-11T11:00:00Z".into();
    review.revisions = vec![Revision {
        body: first.into(),
        updated_at: "2026-10-11T10:00:00Z".into(),
    }];
    let h = derive(&merged_pr(vec![review]));
    assert_eq!(h.rounds.len(), 1);
    assert_eq!(h.rounds[0].verdict, RoundVerdict::Clean);
    assert_eq!(h.rounds[0].created_at, "2026-10-11T10:00:00Z");
}

#[test]
fn flags_a_merge_with_an_open_finding_and_a_merge_with_no_review() {
    let review = comment(
        10,
        "2026-10-11T10:00:00Z",
        "## Milo PR review\n\n**CHANGES REQUESTED**\n\n**F1 · P1 · Token logged on failure**\n\nThe token reaches the log.",
    );
    let h = derive(&merged_pr(vec![review]));
    assert_eq!(h.metrics.open_at_merge, 1);
    assert_eq!(h.metrics.merged_by_loop, Some(false));
    let flag = &h.flags[0];
    assert_eq!(flag.kind, FlagKind::MergedWithOpenFindings);
    assert_eq!(flag.attention, Glance::Act);
    assert_eq!(
        flag.text,
        "Merged while F1 (Token logged on failure) had no ruling."
    );

    let h = derive(&merged_pr(vec![]));
    assert_eq!(h.flags[0].kind, FlagKind::NoReviewBeforeMerge);
    assert_eq!(h.metrics.merged_by_loop, Some(false));
}

#[test]
fn a_blocked_review_has_no_verdict() {
    let review = comment(
        10,
        "2026-10-11T10:00:00Z",
        "## Milo PR review\n\n**BLOCKED: required isolated testing could not start because JUPITER is offline.**\n\nNo actionable findings in the static review.",
    );
    let h = derive(&merged_pr(vec![review]));
    assert_eq!(h.rounds[0].verdict, RoundVerdict::Unknown);
    assert_eq!(h.rounds[0].blocking, None);
}

#[test]
fn comments_by_the_builder_are_never_rounds() {
    let reply = comment(
        11,
        "2026-10-11T10:30:00Z",
        "Reviewed `abcdef0` myself; no findings.\n\n---\n_Generated by [Claude Code](https://claude.ai/code)_",
    );
    let h = derive(&merged_pr(vec![reply]));
    assert!(h.rounds.is_empty());
    assert_eq!(h.comments[0].role, Role::Builder);
}
