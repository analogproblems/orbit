use super::{
    claims::{collect, lines, place_commits, Line},
    rounds::{Comment, Comments},
    test_support::*,
    worktrees::git,
};
use crate::types::*;

fn line(numbers: &[u32], kind: ClaimKind, commit: Option<&str>, reason: Option<&str>) -> Line {
    Line {
        numbers: numbers.to_vec(),
        kind,
        commit: commit.map(str::to_owned),
        reason: reason.map(str::to_owned),
    }
}

#[test]
fn reads_fixed_and_declined_replies() {
    use ClaimKind::*;
    let cases = [
        (
            "Fixed F2 in abc1234",
            line(&[2], Fixed, Some("abc1234"), None),
        ),
        (
            "**Fixed F2, F3 and f4** in `ABC1234`.",
            line(&[2, 3, 4], Fixed, Some("abc1234"), None),
        ),
        (
            "Fixed F2 in commit abc1234 by indexing from 0",
            line(&[2], Fixed, Some("abc1234"), None),
        ),
        (
            "Fixed in abc1234def",
            line(&[], Fixed, Some("abc1234def"), None),
        ),
        ("Fixed.", line(&[], Fixed, None, None)),
        ("Fixed F2", line(&[2], Fixed, None, None)),
        (
            "- Declined F5: intended behaviour",
            line(&[5], Declined, None, Some("intended behaviour")),
        ),
        (
            "> Declined: not worth it",
            line(&[], Declined, None, Some("not worth it")),
        ),
        ("Declined F1 & F2.", line(&[1, 2], Declined, None, None)),
    ];
    for (text, expected) in cases {
        assert_eq!(lines(text), [expected], "{text}");
    }
    // Sentences that only mention fixing or declining are not claims.
    for text in [
        "Fixed the tests",
        "Declined because it's fine",
        "I fixed F2 in abc1234",
        "Fixedly yours",
        "Declined F2 since it is intended",
        "Fixed in xyz",
    ] {
        assert_eq!(lines(text), [], "{text}");
    }
    assert_eq!(
        lines("Thanks!\nFixed F1 in abc1234\nDeclined F2: intended"),
        [
            line(&[1], Fixed, Some("abc1234"), None),
            line(&[2], Declined, None, Some("intended"))
        ]
    );
}

fn round() -> PreviousRound {
    let finding = |id: &str, number| Finding {
        id: id.into(),
        number: Some(number),
        ..Default::default()
    };
    PreviousRound {
        review_id: "r1".into(),
        head_sha: "c".repeat(40),
        verdict: None,
        findings: vec![finding("r1-0", 1), finding("r1-1", 2)],
        decisions: vec![],
        claims: vec![],
        comments_included: 0,
        comments_omitted: 0,
    }
}
fn comment(author: &str, at: &str, body: &str, finding: Option<&str>) -> Comment {
    Comment {
        author: author.into(),
        created_at: at.into(),
        body: body.into(),
        finding: finding.map(str::to_owned),
        location: None,
    }
}

#[test]
fn keeps_the_latest_claim_about_each_finding_in_play() {
    let comments = Comments {
        kept: vec![
            comment("alex", "1", "Fixed F1 in abc1234 and F2 in def5678", None),
            comment("bob", "2", "Declined: intended", Some("r1-1")),
            // A finding not in play, a thread no longer carried, and a
            // sentence that isn't a claim are left out.
            comment("alex", "3", "Fixed F9 in abc1234", None),
            comment("alex", "4", "Fixed in abc1234", Some("r0-3")),
            comment("alex", "5", "Fixed the flaky test", Some("r1-0")),
        ],
        ..Default::default()
    };
    let claims = collect(&comments, &round());
    assert_eq!(
        claims,
        [
            AuthorClaim {
                finding_id: "r1-0".into(),
                author: "alex".into(),
                created_at: "1".into(),
                kind: ClaimKind::Fixed,
                commit: Some("abc1234".into()),
                commit_check: None,
                reason: None,
            },
            AuthorClaim {
                finding_id: "r1-1".into(),
                author: "bob".into(),
                created_at: "2".into(),
                kind: ClaimKind::Declined,
                commit: None,
                commit_check: None,
                reason: Some("intended".into()),
            },
        ]
    );
    assert_eq!(claims[0].summary(), "fixed in `abc1234`");
    assert_eq!(claims[1].summary(), "declined: intended");
    // A later claim replaces an earlier one, here or in the thread.
    let mut later = comments;
    later
        .kept
        .push(comment("alex", "6", "**Fixed F2** in `0123abcd`.", None));
    let claims = collect(&later, &round());
    assert_eq!(claims[1].kind, ClaimKind::Fixed);
    assert_eq!(claims[1].commit.as_deref(), Some("0123abcd"));
}

#[tokio::test]
async fn says_where_a_named_commit_is() {
    let (_d, repo) = git_repo().await;
    let previous = commit(&repo, "Reviewed").await;
    let fix = commit(&repo, "Fix").await;
    git(&repo, &["checkout", "-q", "-b", "elsewhere", &previous])
        .await
        .unwrap();
    let elsewhere = commit(&repo, "Elsewhere").await;
    git(&repo, &["checkout", "-q", "-"]).await.unwrap();
    let claim = |sha: &str| AuthorClaim {
        finding_id: "r1-0".into(),
        author: "alex".into(),
        created_at: String::new(),
        kind: ClaimKind::Fixed,
        commit: Some(sha.into()),
        commit_check: None,
        reason: None,
    };
    let mut claims = [
        claim(&fix[..7]),
        claim(&previous),
        claim(&elsewhere[..10]),
        claim("deadbee"),
        AuthorClaim {
            commit: None,
            ..claim("")
        },
    ];
    place_commits(&mut claims, &repo, &previous, &fix).await;
    let places: Vec<_> = claims.iter().map(|c| c.commit_check).collect();
    assert_eq!(
        places,
        [
            Some(CommitPlace::InChange),
            Some(CommitPlace::Earlier),
            Some(CommitPlace::NotInBranch),
            Some(CommitPlace::NotFound),
            None,
        ]
    );
    assert_eq!(
        claims[2].commit_note(),
        Some("the commit is not part of this pull request's head")
    );
}
