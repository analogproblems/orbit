use super::*;
use crate::ci::{Check, CommitCi, PullCi, RepoCi};
use crate::problems::{Problem, ProblemKind};
use crate::review_loop::decide;
use crate::types::tests::pr;
use crate::workflows::Setup;
use serde_json::json;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-30T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
fn ago(minutes: i64) -> String {
    stamp(now() - Duration::minutes(minutes))
}
const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MAIN: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn repository(automatic: bool) -> Repository {
    Repository {
        name: "a/b".into(),
        path: "/tmp/a-b".into(),
        automatic,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes: 0,
        keep_updated: false,
        provider: Default::default(),
        dev_container: Default::default(),
    }
}
/// A pull request Orbit trusts, opened against main's latest commit.
fn trusted() -> PullRequest {
    let mut p = pr();
    p.head_repo = "a/b".into();
    p.author_association = "OWNER".into();
    p.base_sha = MAIN.into();
    p
}
/// A finished automatic round of the pull request as it is, with
/// `blocking` blocking findings.
fn round(p: &PullRequest, blocking: usize) -> Review {
    let mut r = Review::new(p, &Settings::default(), true);
    r.created_at = ago(30);
    r.status = "posted".into();
    r.report = "Report".into();
    r.findings = Some(
        (0..blocking)
            .map(|i| Finding {
                id: format!("f-{i}"),
                title: "Bug".into(),
                priority: 0,
                ..Default::default()
            })
            .collect(),
    );
    decide(&mut r, Some(1));
    r
}
fn commit(sha: &str, state: CiState) -> CommitCi {
    CommitCi {
        sha: sha.into(),
        state,
        passing: u32::from(state == CiState::Passing),
        ..Default::default()
    }
}
/// A pull request Orbit reviews that is clear to merge: a clean round on
/// its head and base, its checks passed after main's latest commit was
/// made, and GitHub says it merges.
fn ready() -> (Snapshot, PullRequest) {
    let p = trusted();
    let mut s = Snapshot::default();
    s.settings.automatic = true;
    s.settings.auto_post = true;
    s.settings.watch_ci = true;
    s.settings.repositories.push(repository(true));
    s.prs.push(p.clone());
    s.reviews.push(round(&p, 0));
    s.ci = vec![RepoCi {
        repo: "a/b".into(),
        branch: "main".into(),
        head: CommitCi {
            committed_at: ago(60),
            ..commit(MAIN, CiState::Passing)
        },
        pulls: vec![PullCi {
            number: 1,
            draft: false,
            ci: CommitCi {
                ran_at: ago(40),
                ..commit(HEAD, CiState::Passing)
            },
        }],
        complete: true,
        ..Default::default()
    }];
    s.merges = vec![merge(Some(true), "clean")];
    (s, p)
}
fn merge(mergeable: Option<bool>, state: &str) -> Merge {
    Merge {
        pr_id: "a/b#1".into(),
        head_sha: HEAD.into(),
        base_sha: MAIN.into(),
        read_at: ago(1),
        mergeable,
        state: state.into(),
        ..Default::default()
    }
}
fn waits(s: &Snapshot) -> Vec<(WaitKind, String)> {
    of(s, &s.prs[0])
        .waits
        .into_iter()
        .map(|w| (w.kind, w.text))
        .collect()
}
fn first(s: &Snapshot) -> String {
    of(s, &s.prs[0]).line()
}
fn pull(s: &mut Snapshot) -> &mut CommitCi {
    &mut s.ci[0].pulls[0].ci
}

#[test]
fn a_pull_request_is_clear_only_with_nothing_in_the_way() {
    let (s, p) = ready();
    let answer = of(&s, &p);
    assert!(answer.clear);
    assert!(answer.waits.is_empty());
    assert_eq!(
        (answer.base.as_str(), answer.base_sha.as_str()),
        ("main", MAIN)
    );
    assert_eq!(
        answer.line(),
        "Clear to merge at aaaaaaa against main at ccccccc."
    );
    assert_eq!(all(&s), [answer]);
}

#[test]
fn orbits_loop_comes_first() {
    let (mut s, _) = ready();
    s.reviews.clear();
    assert!(!of(&s, &s.prs[0]).clear);
    assert_eq!(
        waits(&s),
        [(
            WaitKind::Review,
            "Orbit hasn't reviewed aaaaaaa yet.".into()
        )]
    );
    // A round on this head that is still running.
    let (mut s, p) = ready();
    s.reviews[0].status = "reviewing".into();
    assert_eq!(
        first(&s),
        "Not clear to merge yet. Orbit is reviewing aaaaaaa."
    );
    // Findings on their way to the author.
    let mut r = round(&p, 2);
    r.status = "ready".into();
    s.reviews = vec![r];
    assert_eq!(
        first(&s),
        "Not clear to merge yet. Orbit is publishing its review of aaaaaaa."
    );
    s.reviews[0].status = "posted".into();
    assert_eq!(
        waits(&s),
        [(
            WaitKind::Findings,
            "2 blocking findings from Orbit's review of aaaaaaa wait on the author.".into()
        )]
    );
    s.reviews = vec![round(&p, 1)];
    assert_eq!(
        waits(&s)[0].1,
        "1 blocking finding from Orbit's review of aaaaaaa waits on the author."
    );
    // A newer head than the latest round.
    let (mut s, _) = ready();
    s.prs[0].head_sha = "d".repeat(40);
    assert_eq!(
        waits(&s),
        [
            (
                WaitKind::Review,
                "Orbit hasn't reviewed ddddddd yet.".into()
            ),
            (
                WaitKind::Checks,
                "Orbit hasn't read the checks on ddddddd yet.".into()
            ),
        ]
    );
    // A newer base than the latest round, at the same head.
    let (mut s, _) = ready();
    s.prs[0].base_sha = "e".repeat(40);
    assert_eq!(
        waits(&s)[0],
        (
            WaitKind::Review,
            "Orbit hasn't reviewed aaaaaaa yet.".into()
        )
    );
    // A clean review with an older main than the checks ran with, as when
    // Orbit read CI after main moved on but before GitHub gave the pull
    // request the newer base: never clear against the newer one.
    let (mut s, p) = ready();
    let mut older = p.clone();
    older.base_sha = "e".repeat(40);
    s.prs = vec![older.clone()];
    s.reviews = vec![round(&older, 0)];
    let answer = of(&s, &s.prs[0]);
    assert!(!answer.clear);
    assert_eq!(
        answer.waits,
        [Wait {
            kind: WaitKind::Review,
            turn: Turn::Working,
            text: "Orbit reviewed it with main at eeeeeee, and main is at ccccccc now, so it waits for a review with the latest main.".into()
        }]
    );
    assert!(wanted(&s, now()).is_empty());
    // Findings in the way say so alone, whatever main the review was with.
    s.reviews = vec![round(&older, 2)];
    assert_eq!(
        waits(&s),
        [(
            WaitKind::Findings,
            "2 blocking findings from Orbit's review of aaaaaaa wait on the author.".into()
        )]
    );
    // A loop that needs you, or that you stopped.
    let (mut s, p) = ready();
    s.reviews[0].status = "failed".into();
    assert_eq!(
        waits(&s),
        [(
            WaitKind::Loop,
            "Orbit's review loop needs you: The review failed.".into()
        )]
    );
    s.reviews = vec![round(&p, 0)];
    s.loops.push(LoopControl {
        pr_id: p.id.clone(),
        stopped: true,
        ..Default::default()
    });
    assert_eq!(
        waits(&s),
        [(
            WaitKind::Loop,
            "Orbit's review loop on it is stopped.".into()
        )]
    );
}

/// Each thing in the way says whose move it is: the author's when they
/// have something to fix, a person's when only you or a rule on GitHub can
/// move it on, and nobody's while Orbit, CI or GitHub is still at work.
#[test]
fn each_wait_says_whose_move_it_is() {
    use Turn::*;
    fn failing(s: &mut Snapshot) {
        pull(s).state = CiState::Failing;
        pull(s).failing = vec![Check {
            name: "Test".into(),
            url: String::new(),
        }];
    }
    fn answered(s: &mut Snapshot, mergeable: Option<bool>, state: &str) {
        s.merges = vec![merge(mergeable, state)];
    }
    type Change = fn(&mut Snapshot);
    let cases: Vec<(Turn, Change)> = vec![
        (Working, |s| s.reviews.clear()),
        (Working, |s| s.reviews[0].status = "reviewing".into()),
        (Working, |s| {
            let mut r = round(&s.prs[0], 2);
            r.status = "ready".into();
            s.reviews = vec![r];
        }),
        (Author, |s| s.reviews = vec![round(&s.prs[0], 2)]),
        (Person, |s| s.reviews[0].status = "failed".into()),
        (Person, |s| {
            s.loops.push(LoopControl {
                pr_id: s.prs[0].id.clone(),
                stopped: true,
                ..Default::default()
            })
        }),
        (Author, failing),
        (Working, |s| {
            pull(s).state = CiState::Running;
            pull(s).running = 1;
        }),
        (Working, |s| {
            pull(s).state = CiState::Running;
            pull(s).incomplete = true;
        }),
        (Working, |s| pull(s).sha = "e".repeat(40)),
        (Person, |s| s.settings.watch_ci = false),
        (Working, |s| *pull(s) = commit(HEAD, CiState::None)),
        (Person, |s| s.prs[0].base_ref = "release".into()),
        (Author, |s| s.ci[0].head.committed_at = ago(20)),
        (Person, |s| {
            s.ci[0].head.committed_at = ago(20);
            pull(s).ran_at.clear();
        }),
        (Working, |s| {
            s.prs[0].base_sha = "e".repeat(40);
            s.reviews = vec![round(&s.prs[0], 0)];
        }),
        (Working, |s| s.merges.clear()),
        (Working, |s| answered(s, None, "unknown")),
        (Working, |s| {
            s.merges = vec![Merge {
                read_at: String::new(),
                error: Some("down".into()),
                ..merge(None, "")
            }]
        }),
        (Author, |s| answered(s, Some(false), "dirty")),
        (Author, |s| answered(s, Some(true), "behind")),
        (Person, |s| answered(s, Some(true), "blocked")),
        (Author, |s| answered(s, Some(true), "unstable")),
        (Author, |s| answered(s, Some(true), "draft")),
        (Person, |s| answered(s, Some(true), "odd")),
        (Person, |s| {
            reviewed_by(s, 1, vec![person("alice", Stand::ApprovedOlder, OLD)])
        }),
        (Author, |s| {
            reviewed_by(s, 1, vec![person("alice", Stand::ChangesRequested, HEAD)])
        }),
        (Person, |s| {
            reviewed_by(s, 1, vec![person("alice", Stand::ChangesRequested, OLD)])
        }),
    ];
    for (turn, change) in cases {
        let (mut s, _) = ready();
        change(&mut s);
        let first = of(&s, &s.prs[0]).waits.remove(0);
        assert_eq!(first.turn, turn, "{}", first.text);
    }
}

#[test]
fn every_check_on_the_head_must_pass_and_be_read() {
    let (mut s, _) = ready();
    pull(&mut s).state = CiState::Failing;
    pull(&mut s).failing = vec![Check {
        name: "Test".into(),
        url: String::new(),
    }];
    assert_eq!(
        waits(&s),
        [(WaitKind::Checks, "Test fails on aaaaaaa.".into())]
    );
    pull(&mut s).failing.push(Check {
        name: "Lint".into(),
        url: String::new(),
    });
    assert_eq!(waits(&s)[0].1, "Test and Lint fail on aaaaaaa.");
    pull(&mut s).state = CiState::Running;
    pull(&mut s).running = 1;
    assert_eq!(waits(&s)[0].1, "Checks are still running on aaaaaaa.");
    // Running only because some weren't read.
    pull(&mut s).running = 0;
    pull(&mut s).incomplete = true;
    assert_eq!(
        waits(&s)[0].1,
        "Orbit couldn't read every check on aaaaaaa."
    );
    // GitHub's answer is about an older head, or Orbit read none.
    let (mut s, _) = ready();
    pull(&mut s).sha = "e".repeat(40);
    assert_eq!(
        waits(&s)[0].1,
        "Orbit hasn't read the checks on aaaaaaa yet."
    );
    s.ci.clear();
    assert_eq!(
        waits(&s)[0].1,
        "Orbit hasn't read the checks on aaaaaaa yet."
    );
    s.settings.watch_ci = false;
    assert_eq!(
        waits(&s),
        [(
            WaitKind::Checks,
            "Orbit can't see its checks while Watch CI is off.".into()
        )]
    );
}

#[test]
fn no_checks_wait_only_where_ci_runs() {
    let (mut s, _) = ready();
    *pull(&mut s) = commit(HEAD, CiState::None);
    let none = || {
        vec![(
            WaitKind::Checks,
            "No checks have reported on aaaaaaa yet.".to_string(),
        )]
    };
    assert_eq!(waits(&s), none());
    // Orbit read at main's latest commit that no workflow runs on pull
    // requests here, but hasn't yet waited for another service's checks.
    let mut setup = Setup {
        repo: "a/b".into(),
        branch: "main".into(),
        sha: MAIN.into(),
        ..Default::default()
    };
    s.setups = vec![setup.clone()];
    assert_eq!(waits(&s), none());
    // Once it has, it opens a problem about it, and nothing waits on checks.
    let mut problem = Problem {
        key: crate::problems::setup_key("a/b"),
        kind: ProblemKind::NoCi,
        repo: "a/b".into(),
        branch: "main".into(),
        sha: MAIN.into(),
        ..Default::default()
    };
    s.problems = vec![problem.clone()];
    assert!(of(&s, &s.prs[0]).clear);
    // Or its workflows don't run on them.
    let mut not_on_pulls = setup.clone();
    not_on_pulls.workflows = vec![workflows::Workflow {
        path: ".github/workflows/release.yml".into(),
        on_pulls: workflows::OnPulls::No,
    }];
    s.setups = vec![not_on_pulls];
    assert!(of(&s, &s.prs[0]).clear);
    s.setups = vec![setup.clone()];
    // Into another branch, whose workflows Orbit didn't read, it can't tell.
    s.prs[0].base_ref = "release".into();
    assert_eq!(
        waits(&s),
        [(
            WaitKind::Base,
            "Orbit can't tell whether its checks ran with the latest release: it only follows the default branch.".into()
        )]
    );
    s.prs[0].base_ref = "main".into();
    // What was read, or the problem, at an older commit of main tells
    // nothing, as main may have added CI since.
    let older = |sha: &mut String| *sha = "e".repeat(40);
    let mut stale = setup.clone();
    older(&mut stale.sha);
    s.setups = vec![stale];
    assert_eq!(waits(&s), none());
    s.setups = vec![setup.clone()];
    older(&mut problem.sha);
    s.problems = vec![problem.clone()];
    assert_eq!(waits(&s), none());
    problem.sha = MAIN.into();
    // Nor a closed problem, or one about another branch.
    problem.closed_at = Some(ago(1));
    s.problems = vec![problem.clone()];
    assert_eq!(waits(&s), none());
    problem.closed_at = None;
    problem.branch = "trunk".into();
    s.problems = vec![problem.clone()];
    assert_eq!(waits(&s), none());
    problem.branch = "main".into();
    s.problems = vec![problem];
    assert!(of(&s, &s.prs[0]).clear);
    // Checks seen there before mean CI runs after all, and a failed read
    // tells nothing.
    setup.ran = Some("Checks ran on a pull request.".into());
    s.setups = vec![setup.clone()];
    assert_eq!(waits(&s), none());
    setup.ran = None;
    setup.error = Some("down".into());
    s.setups = vec![setup];
    assert_eq!(waits(&s), none());
}

#[test]
fn the_checks_must_have_run_with_the_bases_latest_commit() {
    let (mut s, _) = ready();
    s.ci[0].head.committed_at = ago(20);
    assert_eq!(
        waits(&s),
        [(
            WaitKind::Base,
            "main moved on since its checks ran: merge main in and let them run again.".into()
        )]
    );
    pull(&mut s).ran_at.clear();
    assert_eq!(
        waits(&s)[0].1,
        "Orbit can't tell whether its checks ran with the latest main, as GitHub didn't say when they began."
    );
    let (mut s, _) = ready();
    s.prs[0].base_ref = "release".into();
    assert_eq!(
        waits(&s),
        [(
            WaitKind::Base,
            "Orbit can't tell whether its checks ran with the latest release: it only follows the default branch.".into()
        )]
    );
    assert_eq!(of(&s, &s.prs[0]).base_sha, "");
}

#[test]
fn github_must_say_it_merges() {
    let (mut s, _) = ready();
    let text = |s: &Snapshot| {
        let w = waits(s);
        assert!(w.iter().all(|(k, _)| *k == WaitKind::Merge), "{w:?}");
        w.into_iter().map(|(_, t)| t).collect::<Vec<_>>()
    };
    for (mergeable, state, said) in [
        (
            None,
            "unknown",
            "GitHub is still checking whether it merges.",
        ),
        (None, "clean", "GitHub is still checking whether it merges."),
        (
            Some(true),
            "unknown",
            "GitHub is still checking whether it merges.",
        ),
        (
            Some(false),
            "dirty",
            "It conflicts with main: merge main in and fix the conflicts.",
        ),
        (
            Some(false),
            "clean",
            "It conflicts with main: merge main in and fix the conflicts.",
        ),
        (
            Some(true),
            "dirty",
            "It conflicts with main: merge main in and fix the conflicts.",
        ),
        (
            Some(true),
            "behind",
            "GitHub wants it brought up to date with main first.",
        ),
        (
            Some(true),
            "blocked",
            "GitHub says a rule blocks merging it, such as a required review.",
        ),
        (
            Some(true),
            "unstable",
            "GitHub says a check on it isn't passing.",
        ),
        (Some(true), "draft", "GitHub says it is a draft."),
        (Some(true), "odd", "GitHub says it can't merge yet (odd)."),
    ] {
        s.merges = vec![merge(mergeable, state)];
        assert_eq!(text(&s), [said], "{mergeable:?} {state}");
    }
    s.merges = vec![merge(Some(true), "has_hooks")];
    assert!(of(&s, &s.prs[0]).clear);
    // An answer about another head or base, or none, isn't one.
    let asked = "Orbit hasn't asked GitHub yet whether it merges.";
    s.merges = vec![Merge {
        base_sha: "f".repeat(40),
        ..merge(Some(true), "clean")
    }];
    assert_eq!(text(&s), [asked]);
    s.merges = vec![Merge {
        head_sha: "f".repeat(40),
        ..merge(Some(true), "clean")
    }];
    assert_eq!(text(&s), [asked]);
    s.merges.clear();
    assert_eq!(text(&s), [asked]);
    // A failed read says so; after an earlier answer, that answer stands.
    let failed = "Orbit couldn't ask GitHub whether it merges: HTTP 502. It tries again later.";
    s.merges = vec![Merge {
        read_at: String::new(),
        error: Some(failed.into()),
        ..merge(None, "")
    }];
    assert_eq!(text(&s), [failed]);
    s.merges = vec![Merge {
        error: Some(failed.into()),
        ..merge(Some(true), "clean")
    }];
    assert!(of(&s, &s.prs[0]).clear);
    // GitHub isn't asked about one with something else in the way.
    s.merges = vec![merge(Some(false), "dirty")];
    s.reviews.clear();
    assert!(waits(&s).iter().all(|(k, _)| *k != WaitKind::Merge));
}

#[test]
fn only_pull_requests_orbit_reviews_have_an_answer() {
    let (mut s, _) = ready();
    let mut draft = trusted();
    draft.id = "a/b#2".into();
    draft.number = 2;
    draft.draft = true;
    let mut fork = trusted();
    fork.id = "a/b#3".into();
    fork.number = 3;
    fork.head_repo = "c/b".into();
    s.prs.push(draft);
    s.prs.push(fork);
    let ids = |s: &Snapshot| all(s).into_iter().map(|r| r.pr_id).collect::<Vec<_>>();
    assert_eq!(ids(&s), ["a/b#1"]);
    s.settings.repositories = vec![repository(false)];
    assert!(ids(&s).is_empty());
}

#[test]
fn github_is_asked_once_nothing_else_is_in_the_way() {
    let (mut s, _) = ready();
    let ask = |n: u64| ("a/b".to_string(), n, HEAD.to_string(), MAIN.to_string());
    s.merges.clear();
    assert_eq!(wanted(&s, now()), [ask(1)]);
    // A definite answer stands for `EVERY`.
    s.merges = vec![merge(Some(true), "clean")];
    assert!(wanted(&s, now()).is_empty());
    s.merges[0].read_at = ago(10);
    assert_eq!(wanted(&s, now()), [ask(1)]);
    // While GitHub works it out, or after an error, Orbit waits its turn.
    s.merges = vec![Merge {
        retry_at: Some(stamp(now() + Duration::minutes(1))),
        ..merge(None, "unknown")
    }];
    assert!(wanted(&s, now()).is_empty());
    s.merges[0].retry_at = Some(stamp(now()));
    assert_eq!(wanted(&s, now()), [ask(1)]);
    s.merges = vec![Merge {
        error: Some("down".into()),
        retry_at: Some(stamp(now() + Duration::minutes(1))),
        ..merge(Some(true), "clean")
    }];
    assert!(wanted(&s, now()).is_empty());
    // An answer kept while GitHub works it out again waits its turn too.
    s.merges = vec![Merge {
        retry_at: Some(stamp(now() + Duration::minutes(1))),
        ..merge(Some(true), "clean")
    }];
    s.merges[0].read_at = ago(30);
    assert!(wanted(&s, now()).is_empty());
    s.merges[0].retry_at = Some(stamp(now()));
    assert_eq!(wanted(&s, now()), [ask(1)]);
    // A new base commit is a new question, once the pull request is on it
    // and reviewed with it.
    s.ci[0].head.sha = "9".repeat(40);
    assert!(wanted(&s, now()).is_empty());
    s.prs[0].base_sha = "9".repeat(40);
    s.reviews = vec![round(&s.prs[0], 0)];
    assert_eq!(
        wanted(&s, now()),
        [("a/b".to_string(), 1, HEAD.to_string(), "9".repeat(40))]
    );
    // Something else in the way: nothing to ask.
    s.reviews.clear();
    assert!(wanted(&s, now()).is_empty());
}

#[test]
fn a_few_are_asked_a_pass_never_asked_first_then_the_oldest() {
    let (mut s, _) = ready();
    s.prs.clear();
    s.reviews.clear();
    s.ci[0].pulls.clear();
    s.merges.clear();
    for n in 1..=6u64 {
        let mut pr = trusted();
        pr.number = n;
        pr.id = pr_id("a/b", n);
        pr.head_sha = format!("{n}").repeat(40);
        s.reviews.push(round(&pr, 0));
        s.ci[0].pulls.push(PullCi {
            number: n,
            draft: false,
            ci: CommitCi {
                ran_at: ago(40),
                ..commit(&pr.head_sha, CiState::Passing)
            },
        });
        if n <= 3 {
            s.merges.push(Merge {
                pr_id: pr.id.clone(),
                head_sha: pr.head_sha.clone(),
                read_at: ago(10 + n as i64),
                ..merge(Some(true), "clean")
            });
        }
        s.prs.push(pr);
    }
    let numbers: Vec<u64> = wanted(&s, now()).into_iter().map(|w| w.1).collect();
    assert_eq!(numbers, [4, 5, 6, 3]);
    assert_eq!(numbers.len(), PER_PASS);
}

#[test]
fn githubs_answer_is_read_for_the_head_orbit_asked_about() {
    let v = json!({
        "head": { "sha": HEAD },
        "base": { "sha": MAIN },
        "mergeable": true,
        "mergeable_state": "clean"
    });
    let m = understand(&v, "a/b#1", HEAD, MAIN, now()).unwrap();
    let read = Merge {
        read_at: stamp(now()),
        ..merge(Some(true), "clean")
    };
    assert_eq!(m, read);
    let pending = json!({ "head": { "sha": HEAD }, "base": { "sha": MAIN }, "mergeable": null });
    let m = understand(&pending, "a/b#1", HEAD, MAIN, now()).unwrap();
    assert_eq!((m.mergeable, m.state.as_str()), (None, "unknown"));
    // The pull request moved on, or its base did, since Orbit synced.
    let moved =
        json!({ "head": { "sha": "d".repeat(40) }, "base": { "sha": MAIN }, "mergeable": true });
    assert_eq!(
        understand(&moved, "a/b#1", HEAD, MAIN, now()),
        Err("GitHub's answer was about another commit.".into())
    );
    let rebased =
        json!({ "head": { "sha": HEAD }, "base": { "sha": "e".repeat(40) }, "mergeable": true });
    assert_eq!(
        understand(&rebased, "a/b#1", HEAD, MAIN, now()),
        Err("GitHub's answer was about another commit.".into())
    );
}
#[test]
fn github_still_working_or_failing_is_asked_again_later_and_longer() {
    let mut merges = vec![];
    let pending = || {
        Ok(Merge {
            read_at: stamp(now()),
            ..merge(None, "unknown")
        })
    };
    let mut waits = vec![];
    for _ in 0..8 {
        record(&mut merges, "a/b#1", HEAD, MAIN, pending(), now());
        let retry = time(merges[0].retry_at.as_deref().unwrap()).unwrap();
        waits.push((retry - now()).num_minutes());
    }
    assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60]);
    assert_eq!(merges.len(), 1);
    // Still working it out on a later read: that read is the latest.
    let later = stamp(now() + Duration::minutes(5));
    let still = Merge {
        read_at: later.clone(),
        ..merge(None, "unknown")
    };
    record(&mut merges, "a/b#1", HEAD, MAIN, Ok(still), now());
    assert_eq!(merges[0].read_at, later);
    // A definite answer starts over.
    record(
        &mut merges,
        "a/b#1",
        HEAD,
        MAIN,
        Ok(merge(Some(true), "clean")),
        now(),
    );
    assert_eq!(merges, [merge(Some(true), "clean")]);
    // GitHub working it out again keeps it too, so a pull request clear to
    // merge stays clear meanwhile, and Orbit asks again after a wait.
    record(&mut merges, "a/b#1", HEAD, MAIN, pending(), now());
    assert_eq!(
        merges,
        [Merge {
            tries: 1,
            retry_at: Some(stamp(now() + Duration::minutes(1))),
            ..merge(Some(true), "clean")
        }]
    );
    record(
        &mut merges,
        "a/b#1",
        HEAD,
        MAIN,
        Ok(merge(Some(true), "clean")),
        now(),
    );
    // An error keeps it, and says why.
    record(
        &mut merges,
        "a/b#1",
        HEAD,
        MAIN,
        Err("gh: HTTP 502".into()),
        now(),
    );
    assert_eq!((merges[0].mergeable, merges[0].tries), (Some(true), 1));
    assert_eq!(
        merges[0].error.as_deref(),
        Some("Orbit couldn't ask GitHub whether it merges: HTTP 502. It tries again later.")
    );
    // GitHub then still working it out: the error is over.
    record(&mut merges, "a/b#1", HEAD, MAIN, pending(), now());
    assert_eq!(
        (
            merges[0].mergeable,
            merges[0].error.as_deref(),
            merges[0].tries
        ),
        (Some(true), None, 2)
    );
    // About a new head, an error keeps nothing of the old answer, and the
    // waits start over.
    let new = "d".repeat(40);
    record(
        &mut merges,
        "a/b#1",
        &new,
        MAIN,
        Err("HTTP 502".into()),
        now(),
    );
    assert_eq!(merges.len(), 1);
    let m = &merges[0];
    assert_eq!(
        (
            m.head_sha.as_str(),
            m.mergeable,
            m.read_at.as_str(),
            m.tries
        ),
        (new.as_str(), None, "", 1)
    );
    assert_eq!(m.retry_at, Some(stamp(now() + Duration::minutes(1))));
}

#[test]
fn answers_go_with_pull_requests_orbit_stops_reviewing() {
    let (mut s, _) = ready();
    s.merges.push(Merge {
        pr_id: "a/b#9".into(),
        ..merge(Some(true), "clean")
    });
    prune(&mut s);
    assert_eq!(s.merges, [merge(Some(true), "clean")]);
    s.prs[0].draft = true;
    prune(&mut s);
    assert!(s.merges.is_empty());
}

const OLD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
/// Someone where `stand`, with write access where it matters.
fn person(name: &str, stand: Stand, commit: &str) -> Reviewer {
    Reviewer {
        may_approve: matches!(stand, Stand::Approved | Stand::ChangesRequested).then_some(true),
        ..read_as(name, stand, commit)
    }
}
/// Someone where `stand`, as GitHub's reviews say, before Orbit asks
/// about their access.
fn read_as(name: &str, stand: Stand, commit: &str) -> Reviewer {
    Reviewer {
        name: name.into(),
        team: false,
        stand,
        commit: commit.into(),
        may_approve: None,
    }
}
/// GitHub's answer says main needs `approvals` and who stands where.
fn reviewed_by(s: &mut Snapshot, approvals: u32, reviewers: Vec<Reviewer>) {
    s.merges[0].reviews = Some(Reviews {
        reviewers,
        rule: Some(Rule {
            approvals,
            code_owners: false,
        }),
        ..Default::default()
    });
}
fn wait_of(s: &Snapshot) -> (WaitKind, Turn, String) {
    let w = of(s, &s.prs[0]).waits.remove(0);
    (w.kind, w.turn, w.text)
}

#[test]
fn only_approvals_of_the_latest_commit_count() {
    use Stand::*;
    let (mut s, _) = ready();
    reviewed_by(&mut s, 1, vec![person("alice", ApprovedOlder, OLD)]);
    let r = of(&s, &s.prs[0]);
    assert_eq!(r.waits.len(), 1);
    assert_eq!(
        (r.waits[0].kind, r.waits[0].turn),
        (WaitKind::Reviews, Turn::Person)
    );
    assert_eq!(
        r.line(),
        "Not clear to merge yet. main needs 1 approval of aaaaaaa and it has none: alice approved bbbbbbb but not aaaaaaa."
    );
    // The answer says who stands where.
    assert_eq!(r.reviews, s.merges[0].reviews);
    // Approving the latest commit makes it clear.
    reviewed_by(&mut s, 1, vec![person("alice", Approved, HEAD)]);
    let r = of(&s, &s.prs[0]);
    assert!(r.clear);
    assert_eq!(r.reviews, s.merges[0].reviews);
    assert_eq!(
        r.line(),
        "Clear to merge at aaaaaaa against main at ccccccc."
    );
    // It names everyone it waits for.
    let team = Reviewer {
        name: "a/devs".into(),
        team: true,
        ..Default::default()
    };
    reviewed_by(
        &mut s,
        2,
        vec![
            person("alice", Approved, HEAD),
            person("bob", Asked, ""),
            team,
            person("carol", ApprovedOlder, OLD),
        ],
    );
    assert_eq!(
        first(&s),
        "Not clear to merge yet. main needs 2 approvals of aaaaaaa and it has 1: bob hasn't reviewed aaaaaaa yet, the a/devs team hasn't reviewed aaaaaaa yet and carol approved bbbbbbb but not aaaaaaa."
    );
    reviewed_by(
        &mut s,
        2,
        vec![
            person("alice", Approved, HEAD),
            person("bob", Approved, HEAD),
        ],
    );
    assert!(of(&s, &s.prs[0]).clear);
    // Nobody left to ask.
    reviewed_by(&mut s, 2, vec![person("alice", Approved, HEAD)]);
    assert_eq!(
        first(&s),
        "Not clear to merge yet. main needs 2 approvals of aaaaaaa and it has 1: nobody else is asked to review it."
    );
    reviewed_by(&mut s, 1, vec![]);
    assert_eq!(
        first(&s),
        "Not clear to merge yet. main needs 1 approval of aaaaaaa and it has none: nobody is asked to review it."
    );
    // Rules that need no reviews leave them be.
    reviewed_by(&mut s, 0, vec![person("alice", ApprovedOlder, OLD)]);
    assert!(of(&s, &s.prs[0]).clear);
    // A code owner's approval is left to GitHub.
    s.merges[0].reviews.as_mut().unwrap().rule = Some(Rule {
        approvals: 0,
        code_owners: true,
    });
    assert!(of(&s, &s.prs[0]).clear);
    // An answer from before Orbit read reviews says nothing of them.
    s.merges[0].reviews = None;
    let r = of(&s, &s.prs[0]);
    assert!(r.clear && r.reviews.is_none());
}

#[test]
fn a_request_for_changes_stands_until_its_reviewer_reviews_again() {
    use Stand::*;
    let (mut s, _) = ready();
    let changes = |commit: &str| {
        vec![
            person("alice", Approved, HEAD),
            person("bob", ChangesRequested, commit),
        ]
    };
    // On the latest commit, it's the author's move.
    reviewed_by(&mut s, 1, changes(HEAD));
    assert_eq!(
        wait_of(&s),
        (
            WaitKind::Reviews,
            Turn::Author,
            "bob asked for changes on aaaaaaa: answer their review.".into()
        )
    );
    // On an older one, it's the reviewer's.
    reviewed_by(&mut s, 1, changes(OLD));
    assert_eq!(
        wait_of(&s),
        (
            WaitKind::Reviews,
            Turn::Person,
            "bob asked for changes on bbbbbbb and hasn't reviewed aaaaaaa since.".into()
        )
    );
    // Where only a code owner's approval is needed, it stands too.
    s.merges[0].reviews.as_mut().unwrap().rule = Some(Rule {
        approvals: 0,
        code_owners: true,
    });
    assert_eq!(wait_of(&s).0, WaitKind::Reviews);
    // Rules that need no reviews leave it to GitHub.
    reviewed_by(&mut s, 0, changes(HEAD));
    assert!(of(&s, &s.prs[0]).clear);
}

#[test]
fn rules_orbit_cant_see_are_left_to_github_and_said_so() {
    use Stand::*;
    let (mut s, _) = ready();
    let unseen = |s: &mut Snapshot, reviewers: Vec<Reviewer>| {
        s.merges[0].reviews = Some(Reviews {
            reviewers,
            rule: None,
            unseen: Some("Orbit's GitHub sign-in can't see main's rules.".into()),
            ..Default::default()
        });
    };
    unseen(&mut s, vec![person("alice", ApprovedOlder, OLD)]);
    let r = of(&s, &s.prs[0]);
    assert!(r.clear);
    assert_eq!(
        r.line(),
        "Clear to merge at aaaaaaa against main at ccccccc. Orbit can't see main's rules, so it left its reviews to GitHub."
    );
    // GitHub says a rule blocks it: those not approving the latest commit.
    s.merges[0].state = "blocked".into();
    assert_eq!(
        wait_of(&s),
        (
            WaitKind::Reviews,
            Turn::Person,
            "GitHub says a rule blocks merging it, and Orbit can't see main's rules to tell which. So far, alice approved bbbbbbb but not aaaaaaa.".into()
        )
    );
    // Or a request for changes.
    unseen(&mut s, vec![person("bob", ChangesRequested, HEAD)]);
    assert_eq!(wait_of(&s).1, Turn::Author);
    // With everyone approving, or nobody asked, it's GitHub's rule.
    for reviewers in [vec![person("alice", Approved, HEAD)], vec![]] {
        unseen(&mut s, reviewers);
        assert_eq!(
            wait_of(&s),
            (
                WaitKind::Merge,
                Turn::Person,
                "GitHub says a rule blocks merging it, such as a required review.".into()
            )
        );
    }
    // Not clear: no note, though Orbit can't see the rules.
    let r = of(&s, &s.prs[0]);
    assert!(r.reviews.as_ref().is_some_and(|r| r.rule.is_none()));
    assert_eq!(r.note(), None);
}

#[test]
fn github_in_the_way_comes_before_reviews() {
    use Stand::*;
    let (mut s, _) = ready();
    let cases = [
        (None, "unknown", Turn::Working),
        (Some(false), "dirty", Turn::Author),
        (Some(true), "behind", Turn::Author),
        (Some(true), "unstable", Turn::Author),
        (Some(true), "draft", Turn::Author),
        (Some(true), "odd", Turn::Person),
    ];
    for (mergeable, state, turn) in cases {
        s.merges = vec![merge(mergeable, state)];
        reviewed_by(&mut s, 1, vec![person("alice", ApprovedOlder, OLD)]);
        let r = of(&s, &s.prs[0]);
        assert_eq!(r.waits.len(), 1, "{state}");
        assert_eq!((r.waits[0].kind, r.waits[0].turn), (WaitKind::Merge, turn));
    }
    // Reviews say more than that a rule blocks it.
    s.merges = vec![merge(Some(true), "blocked")];
    reviewed_by(&mut s, 1, vec![person("alice", ApprovedOlder, OLD)]);
    assert_eq!(
        first(&s),
        "Not clear to merge yet. main needs 1 approval of aaaaaaa and it has none: alice approved bbbbbbb but not aaaaaaa."
    );
    // Anything else in the way comes first, and reviews aren't given.
    s.reviews.clear();
    let r = of(&s, &s.prs[0]);
    assert_eq!(r.waits[0].kind, WaitKind::Review);
    assert!(r.reviews.is_none());
}

#[test]
fn github_is_asked_again_while_reviews_are_in_the_way() {
    let (mut s, _) = ready();
    let ask = ("a/b".to_string(), 1, HEAD.to_string(), MAIN.to_string());
    reviewed_by(&mut s, 1, vec![person("alice", Stand::ApprovedOlder, OLD)]);
    assert!(wanted(&s, now()).is_empty());
    s.merges[0].read_at = ago(10);
    assert_eq!(wanted(&s, now()), [ask]);
}

#[test]
fn fresh_reviews_are_kept_while_github_works_out_whether_it_merges() {
    let older = Reviews {
        reviewers: vec![person("alice", Stand::ApprovedOlder, OLD)],
        ..Default::default()
    };
    let newer = Reviews {
        reviewers: vec![person("alice", Stand::Approved, HEAD)],
        ..Default::default()
    };
    let mut merges = vec![Merge {
        reviews: Some(older),
        ..merge(Some(true), "clean")
    }];
    let pending = Merge {
        read_at: stamp(now()),
        reviews: Some(newer.clone()),
        ..merge(None, "unknown")
    };
    record(&mut merges, "a/b#1", HEAD, MAIN, Ok(pending), now());
    assert_eq!(
        (merges[0].mergeable, merges[0].reviews.as_ref()),
        (Some(true), Some(&newer))
    );
}

#[test]
fn reviewers_are_read_from_the_pull_request_and_its_reviews() {
    use Stand::*;
    let pull = json!({
        "user": { "login": "author" },
        "base": { "repo": { "owner": { "login": "a" } } },
        "requested_reviewers": [
            { "login": "bob" }, { "login": "dave" }, { "login": "Bob" },
            { "login": "gina" }, { "login": "hank" }, {}
        ],
        "requested_teams": [{ "slug": "devs" }, { "slug": "" }, {}]
    });
    let review = |login: &str, state: &str, commit: &str| json!({ "user": { "login": login }, "state": state, "commit_id": commit });
    let reviews = [
        review("alice", "APPROVED", OLD),
        review("carol", "APPROVED", HEAD),
        // Asked again: a request for changes stands, an older approval
        // is still older.
        review("bob", "CHANGES_REQUESTED", OLD),
        review("dave", "APPROVED", OLD),
        review("erin", "CHANGES_REQUESTED", OLD),
        // A comment changes nothing.
        review("erin", "COMMENTED", HEAD),
        review("erin", "PENDING", HEAD),
        // A dismissed review counts as none.
        review("frank", "APPROVED", HEAD),
        review("frank", "DISMISSED", HEAD),
        review("hank", "APPROVED", HEAD),
        review("hank", "DISMISSED", HEAD),
        // The author's own, and one from an account that's gone, don't.
        review("Author", "APPROVED", HEAD),
        json!({ "user": null, "state": "APPROVED", "commit_id": HEAD }),
        // The latest review is the one that stands, whatever the case.
        review("ALICE", "APPROVED", HEAD),
    ];
    let team = Reviewer {
        name: "a/devs".into(),
        team: true,
        ..Default::default()
    };
    assert_eq!(
        reviewers(&pull, &reviews, HEAD),
        [
            read_as("bob", ChangesRequested, OLD),
            read_as("dave", ApprovedOlder, OLD),
            read_as("gina", Asked, ""),
            read_as("hank", Asked, ""),
            team,
            read_as("carol", Approved, HEAD),
            read_as("erin", ChangesRequested, OLD),
            read_as("ALICE", Approved, HEAD),
        ]
    );
    // Asked again after approving the latest commit, that still counts.
    let asked = json!({ "requested_reviewers": [{ "login": "carol" }] });
    assert_eq!(
        reviewers(&asked, &reviews[1..2], HEAD),
        [read_as("carol", Approved, HEAD)]
    );
    // A team without its owner is named by its slug.
    let teams = json!({ "requested_teams": [{ "slug": "devs" }] });
    assert_eq!(reviewers(&teams, &[], HEAD)[0].name, "devs");
}

#[test]
fn a_branchs_rules_say_how_many_approvals_it_needs() {
    let none = json!([]);
    let needs = |approvals, code_owners| Rule {
        approvals,
        code_owners,
    };
    assert_eq!(rule(None, &none), needs(0, false));
    let classic = json!({ "required_pull_request_reviews": {
        "required_approving_review_count": 2,
        "require_code_owner_reviews": false
    } });
    assert_eq!(rule(Some(&classic), &none), needs(2, false));
    let checks_only = json!({ "required_status_checks": { "contexts": [] } });
    assert_eq!(rule(Some(&checks_only), &none), needs(0, false));
    // A rule that doesn't say how many asks for one.
    let unsaid = json!({ "required_pull_request_reviews": {} });
    assert_eq!(rule(Some(&unsaid), &none), needs(1, false));
    let owners = json!({ "required_pull_request_reviews": {
        "required_approving_review_count": 0,
        "require_code_owner_reviews": true
    } });
    assert_eq!(rule(Some(&owners), &none), needs(0, true));
    // The most any asks for, and a code owner if any asks.
    let sets = json!([
        { "type": "required_linear_history" },
        { "type": "pull_request", "parameters": {
            "required_approving_review_count": 0, "require_code_owner_review": true
        } },
        { "type": "pull_request", "parameters": {
            "required_approving_review_count": 3, "require_code_owner_review": false
        } },
        { "type": "required_status_checks", "parameters": {
            "required_approving_review_count": 9
        } }
    ]);
    assert_eq!(rule(Some(&classic), &sets), needs(3, true));
    assert_eq!(rule(None, &sets), needs(3, true));
    let fewer = json!([{ "type": "pull_request", "parameters": {
        "required_approving_review_count": 1
    } }]);
    assert_eq!(rule(Some(&classic), &fewer), needs(2, false));
    let zero = json!([{ "type": "pull_request", "parameters": {
        "required_approving_review_count": 0
    } }]);
    assert_eq!(rule(None, &zero), needs(0, false));
}

#[test]
fn branch_names_are_escaped_in_api_paths() {
    assert_eq!(branch_path("main"), "main");
    assert_eq!(branch_path("feature/a-b_c.d~e"), "feature/a-b_c.d~e");
    assert_eq!(branch_path("x y#1?é"), "x%20y%231%3F%C3%A9");
}

#[test]
fn reviews_load_from_older_state() {
    let m: Merge = serde_json::from_value(json!({ "prId": "a/b#1" })).unwrap();
    assert_eq!(m.reviews, None);
    let r: Reviews = serde_json::from_value(json!({
        "reviewers": [{ "name": "alice", "stand": "approved_older", "commit": OLD }],
        "rule": { "approvals": 1 }
    }))
    .unwrap();
    assert_eq!(r.reviewers, [person("alice", Stand::ApprovedOlder, OLD)]);
    assert!(!r.rule.unwrap().code_owners);
    let v = serde_json::to_value(Rule {
        approvals: 1,
        code_owners: true,
    })
    .unwrap();
    assert_eq!(v, json!({ "approvals": 1, "codeOwners": true }));
}

#[test]
fn only_approvals_by_people_with_write_access_count() {
    use Stand::*;
    let (mut s, _) = ready();
    let without = |name: &str, stand: Stand, commit: &str, access: Option<bool>| Reviewer {
        may_approve: access,
        ..person(name, stand, commit)
    };
    reviewed_by(
        &mut s,
        1,
        vec![
            person("carol", ApprovedOlder, OLD),
            without("mallory", Approved, HEAD, Some(false)),
            without("trent", Approved, HEAD, None),
        ],
    );
    assert_eq!(
        wait_of(&s),
        (
            WaitKind::Reviews,
            Turn::Person,
            "main needs 1 approval of aaaaaaa and it has none: carol approved bbbbbbb but not aaaaaaa, mallory approved aaaaaaa, but without write access that doesn't count and trent approved aaaaaaa, but Orbit can't tell whether they have write access.".into()
        )
    );
    // A request for changes without write access doesn't hold it back;
    // one Orbit can't tell about does.
    reviewed_by(
        &mut s,
        1,
        vec![
            person("alice", Approved, HEAD),
            without("mallory", ChangesRequested, HEAD, Some(false)),
        ],
    );
    assert!(of(&s, &s.prs[0]).clear);
    reviewed_by(
        &mut s,
        2,
        vec![
            person("alice", Approved, HEAD),
            without("mallory", ChangesRequested, OLD, Some(false)),
        ],
    );
    assert_eq!(
        first(&s),
        "Not clear to merge yet. main needs 2 approvals of aaaaaaa and it has 1: mallory asked for changes on bbbbbbb, but without write access that doesn't hold it back."
    );
    reviewed_by(
        &mut s,
        1,
        vec![
            person("alice", Approved, HEAD),
            without("trent", ChangesRequested, HEAD, None),
        ],
    );
    assert_eq!(wait_of(&s).1, Turn::Author);
    // GitHub's word on someone's permission.
    for (permission, write) in [
        ("admin", Some(true)),
        ("maintain", Some(true)),
        ("write", Some(true)),
        ("triage", Some(false)),
        ("read", Some(false)),
        ("none", Some(false)),
        ("odd", None),
    ] {
        assert_eq!(access(&json!({ "permission": permission })), write);
    }
    assert_eq!(access(&json!({})), None);
    assert!(plain_login("alice-b2"));
    for odd in ["", "dependabot[bot]", "a/b", "a b", "../x"] {
        assert!(!plain_login(odd), "{odd}");
    }
}

#[test]
fn reviews_orbit_couldnt_read_never_make_it_clear() {
    let (mut s, _) = ready();
    let unreadable = |s: &mut Snapshot, too_many: bool, rule: Option<Rule>| {
        s.merges[0].reviews = Some(Reviews {
            rule,
            unread: (!too_many).then(|| "Bad Gateway (HTTP 502)".into()),
            too_many,
            ..Default::default()
        });
    };
    let needs = Some(Rule {
        approvals: 1,
        code_owners: false,
    });
    unreadable(&mut s, true, needs.clone());
    assert_eq!(
        wait_of(&s),
        (
            WaitKind::Reviews,
            Turn::Person,
            "It has more reviews than Orbit reads, so Orbit can't tell whether enough people approved aaaaaaa.".into()
        )
    );
    unreadable(&mut s, false, needs.clone());
    assert_eq!(
        wait_of(&s),
        (
            WaitKind::Reviews,
            Turn::Working,
            "Orbit couldn't read its reviews, so it can't tell yet whether enough people approved aaaaaaa: Bad Gateway (HTTP 502).".into()
        )
    );
    // GitHub says a rule blocks it: that stands, as Orbit couldn't read
    // more.
    s.merges[0].state = "blocked".into();
    assert_eq!(
        wait_of(&s),
        (
            WaitKind::Merge,
            Turn::Person,
            "GitHub says a rule blocks merging it, such as a required review.".into()
        )
    );
    unreadable(&mut s, true, needs);
    assert_eq!(wait_of(&s).0, WaitKind::Reviews);
    // Unread reviews don't matter where the rules need none, or where
    // Orbit can't see the rules and GitHub says it merges.
    s.merges[0].state = "clean".into();
    unreadable(&mut s, false, Some(Rule::default()));
    assert!(of(&s, &s.prs[0]).clear);
    unreadable(&mut s, true, None);
    let r = of(&s, &s.prs[0]);
    assert!(r.clear && r.note().is_some());
}
