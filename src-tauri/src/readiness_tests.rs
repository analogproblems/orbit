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
