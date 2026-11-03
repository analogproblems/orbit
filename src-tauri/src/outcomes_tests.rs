use super::*;
use crate::{
    baseline::{Found as SettingFound, RepoSettings},
    ci::{Check, CiState, CommitCi, RepoCi},
    ops::{Answer, Item, Write},
    releases::{Release, RepoReleases, Tag},
    reruns::Rerun,
};
use chrono::TimeZone;

const SHA: &str = "1111111111111111111111111111111111111111";
const OTHER: &str = "2222222222222222222222222222222222222222";

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 3, 12, 0, 0).unwrap()
}
fn ago(minutes: i64) -> String {
    stamp(now() - Duration::minutes(minutes))
}
/// Orbit with a/b connected.
fn snapshot() -> Snapshot {
    let mut s = Snapshot::default();
    s.settings
        .repositories
        .push(Repository::new("a/b", "/tmp/a-b"));
    s
}
fn write(action: Action, minutes: i64) -> Write {
    Write {
        id: format!("w{}", minutes),
        thread: String::new(),
        action,
        repo: "a/b".into(),
        number: None,
        state: WriteState::Done,
        at: ago(minutes + 1),
        done_at: Some(ago(minutes)),
        url: "https://github.com/a/b".into(),
        error: None,
        setting: None,
        head: None,
        tag: None,
        release: None,
    }
}
/// Records and settles, as Orbit does on each save.
fn pass(s: &mut Snapshot) -> Vec<Outcome> {
    record(s, now());
    settle(s, now())
}
fn verdicts(s: &Snapshot) -> Vec<(Kind, Verdict, &str)> {
    s.outcomes
        .iter()
        .map(|o| (o.kind, o.verdict, o.why.as_str()))
        .collect()
}

#[test]
fn a_setting_orbit_turned_on_holds_once_it_reads_as_on() {
    let mut s = snapshot();
    s.journal.push(Write {
        setting: Some(Setting::DependabotAlerts),
        ..write(Action::ChangeSetting, 10)
    });
    // Recorded once, waiting until Orbit reads the settings again.
    assert!(pass(&mut s).is_empty());
    assert!(!record(&mut s, now()));
    let o = &s.outcomes[0];
    assert_eq!(o.id, "write:w10");
    assert_eq!(o.what, "Orbit turned on Dependabot alerts in a/b.");
    assert_eq!(o.expect, "it reads as on");
    assert_eq!(o.verdict, Verdict::Waiting);
    // A read from before it was done doesn't count.
    let read = |minutes: i64, on: Option<bool>| RepoSettings {
        repo: "a/b".into(),
        read_at: ago(minutes),
        found: vec![SettingFound {
            setting: Setting::DependabotAlerts,
            on,
        }],
        ..Default::default()
    };
    s.repo_settings = vec![read(20, Some(false))];
    assert!(pass(&mut s).is_empty());
    s.repo_settings = vec![read(5, Some(true))];
    let settled = pass(&mut s);
    assert_eq!(settled.len(), 1);
    assert_eq!(
        verdicts(&s),
        [(Kind::Setting, Verdict::Held, "It reads as on.")]
    );
    assert_eq!(s.outcomes[0].settled_at, Some(stamp(now())));
    // Settled once.
    assert!(pass(&mut s).is_empty());

    // Still off after GitHub took the change is a miss.
    let mut s = snapshot();
    s.journal.push(Write {
        setting: Some(Setting::DependabotAlerts),
        ..write(Action::ChangeSetting, 10)
    });
    s.repo_settings = vec![read(5, Some(false))];
    let settled = pass(&mut s);
    assert_eq!(settled[0].verdict, Verdict::Missed);
    assert!(settled[0].why.starts_with("It still reads as off"));

    // A setting that isn't a switch says what Orbit changed.
    let mut s = snapshot();
    s.journal.push(Write {
        setting: Some(Setting::ReadOnlyToken),
        ..write(Action::ChangeSetting, 10)
    });
    s.repo_settings = vec![RepoSettings {
        found: vec![SettingFound {
            setting: Setting::ReadOnlyToken,
            on: Some(false),
        }],
        ..read(5, None)
    }];
    let settled = pass(&mut s);
    assert_eq!(
        (settled[0].what.as_str(), settled[0].expect.as_str()),
        (
            "Orbit made the Actions token read-only in a/b.",
            "GitHub shows the change"
        )
    );
    assert_eq!(
        (settled[0].verdict, settled[0].why.as_str()),
        (
            Verdict::Missed,
            "GitHub still shows it as before, though it took the change."
        )
    );
}

#[test]
fn only_recent_work_gets_checked() {
    let mut s = snapshot();
    let old = 60 * (RECENT_HOURS + 1);
    s.journal.push(Write {
        setting: Some(Setting::DependabotAlerts),
        ..write(Action::ChangeSetting, old)
    });
    // Not done, or failed.
    s.journal.push(Write {
        state: WriteState::Sending,
        done_at: None,
        setting: Some(Setting::SecretScanning),
        ..write(Action::ChangeSetting, 5)
    });
    s.journal.push(Write {
        state: WriteState::Failed,
        setting: Some(Setting::SecretScanning),
        ..write(Action::ChangeSetting, 6)
    });
    // Comments and issues have nothing to check.
    s.journal.push(write(Action::PostComment, 7));
    assert!(!record(&mut s, now()));
    assert!(s.outcomes.is_empty());
}

#[test]
fn a_tag_holds_at_its_commit_and_a_release_once_published() {
    let mut s = snapshot();
    s.journal.push(Write {
        head: Some(SHA.into()),
        tag: Some("v1.2.0".into()),
        ..write(Action::PushTag, 10)
    });
    s.journal.push(Write {
        tag: Some("v1.2.0".into()),
        release: Some(7),
        ..write(Action::PublishRelease, 9)
    });
    pass(&mut s);
    assert_eq!(s.outcomes[0].what, "Orbit pushed the tag v1.2.0 in a/b.");
    assert_eq!(s.outcomes[0].expect, "it points at 1111111");
    assert_eq!(
        s.outcomes[1].what,
        "Orbit published the draft release for v1.2.0 in a/b."
    );
    let release = |draft: bool| Release {
        id: 7,
        tag: "v1.2.0".into(),
        draft,
        ..Default::default()
    };
    let mut read = RepoReleases {
        repo: "a/b".into(),
        read_at: ago(5),
        tags: vec![Tag {
            name: "v1.2.0".into(),
            sha: SHA.into(),
        }],
        latest: Some(release(false)),
        ..Default::default()
    };
    s.releases = vec![read.clone()];
    pass(&mut s);
    assert_eq!(
        verdicts(&s),
        [
            (Kind::Tag, Verdict::Held, "It points at 1111111."),
            (Kind::Release, Verdict::Held, "It reads as published."),
        ]
    );

    // A tag at another commit, or missing, and a release still a draft.
    let mut s = snapshot();
    s.journal = vec![
        Write {
            head: Some(SHA.into()),
            tag: Some("v1.2.0".into()),
            ..write(Action::PushTag, 10)
        },
        Write {
            id: "w2".into(),
            head: Some(SHA.into()),
            tag: Some("v1.3.0".into()),
            ..write(Action::PushTag, 10)
        },
        Write {
            tag: Some("v1.2.0".into()),
            release: Some(7),
            ..write(Action::PublishRelease, 9)
        },
    ];
    read.tags[0].sha = OTHER.into();
    read.latest = None;
    read.drafts = vec![release(true)];
    read.draft_count = 1;
    s.releases = vec![read.clone()];
    pass(&mut s);
    assert_eq!(
        verdicts(&s),
        [
            (
                Kind::Tag,
                Verdict::Missed,
                "It points at 2222222, not 1111111."
            ),
            (Kind::Tag, Verdict::Missed, "GitHub doesn't list the tag."),
            (Kind::Release, Verdict::Missed, "It's still a draft."),
        ]
    );
    // While GitHub has more tags than Orbit read, a missing one waits.
    let mut s = snapshot();
    s.journal.push(Write {
        head: Some(SHA.into()),
        tag: Some("v1.3.0".into()),
        ..write(Action::PushTag, 10)
    });
    read.more_tags = true;
    s.releases = vec![read];
    pass(&mut s);
    assert_eq!(s.outcomes[0].verdict, Verdict::Waiting);

    // A read GitHub refused says nothing of either: both wait.
    let mut s = snapshot();
    s.journal = vec![
        Write {
            head: Some(SHA.into()),
            tag: Some("v1.2.0".into()),
            ..write(Action::PushTag, 10)
        },
        Write {
            tag: Some("v1.2.0".into()),
            release: Some(7),
            ..write(Action::PublishRelease, 9)
        },
    ];
    s.releases = vec![RepoReleases {
        repo: "a/b".into(),
        read_at: ago(5),
        unseen: Some("GitHub won't show a/b's releases.".into()),
        ..Default::default()
    }];
    assert!(pass(&mut s).is_empty());
    assert_eq!(s.outcomes.len(), 2);
    // A release that is neither a draft nor the latest may have been
    // deleted: Orbit can't tell.
    s.releases = vec![RepoReleases {
        repo: "a/b".into(),
        read_at: ago(4),
        tags: vec![Tag {
            name: "v1.2.0".into(),
            sha: SHA.into(),
        }],
        latest: Some(Release {
            id: 6,
            tag: "v1.1.0".into(),
            ..Default::default()
        }),
        ..Default::default()
    }];
    pass(&mut s);
    assert_eq!(
        verdicts(&s),
        [
            (Kind::Tag, Verdict::Held, "It points at 1111111."),
            (
                Kind::Release,
                Verdict::Unclear,
                "It isn't a draft any more, but it isn't the latest release either, so Orbit can't tell whether it was published or deleted."
            ),
        ]
    );
}

#[test]
fn a_rerun_holds_once_its_commit_passes_and_misses_if_it_fails_again() {
    let job = "https://github.com/a/b/actions/runs/9/job/5";
    let thread = |failed_again: bool, failing: bool| Thread {
        key: "issue:p1".into(),
        kind: crate::ops::ThreadKind::Issue,
        repo: "a/b".into(),
        branch: "main".into(),
        items: vec![Item {
            number: 1,
            check: "CI / test".into(),
            url: job.into(),
            failing,
            at: SHA.into(),
            reruns: vec![Rerun {
                url: job.into(),
                at: ago(10),
                done: true,
                failed_again,
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    // The checks at the commit, each by name, workflow run, job and where
    // it stands, as Orbit read them 5 minutes ago, after the rerun.
    let ci = |checks: &[(&str, u32, u32, CiState)]| {
        let check = |(name, run, job, _): &(&str, u32, u32, CiState)| Check {
            name: name.to_string(),
            url: format!("https://github.com/a/b/actions/runs/{run}/job/{job}"),
        };
        let count = |state| checks.iter().filter(|c| c.3 == state).count() as u32;
        let failing: Vec<_> = checks
            .iter()
            .filter(|c| c.3 == CiState::Failing)
            .map(check)
            .collect();
        RepoCi {
            repo: "a/b".into(),
            branch: "main".into(),
            checked_at: ago(5),
            head: CommitCi {
                sha: SHA.into(),
                state: if failing.is_empty() {
                    CiState::Passing
                } else {
                    CiState::Failing
                },
                failing,
                running: count(CiState::Running),
                passing: count(CiState::Passing),
                checks: checks.iter().map(|c| (check(c), c.3)).collect(),
                named: true,
                ..Default::default()
            },
            ..Default::default()
        }
    };
    use CiState::{Failing, None as Neither, Passing, Running};
    let lint = ("CI / lint", 9, 7, Failing);
    let mut s = snapshot();
    s.ops = vec![thread(false, true)];
    s.ci = vec![ci(&[("CI / test", 9, 5, Failing)])];
    pass(&mut s);
    let o = &s.outcomes[0];
    assert_eq!(o.id, format!("rerun:issue:p1:1:{job}"));
    assert_eq!(o.what, "Orbit reran CI / test at 1111111 on main in a/b.");
    assert_eq!(o.expect, "the job passes the second time");
    // While it runs again, it isn't failing, but it hasn't passed either.
    s.ops = vec![thread(false, false)];
    s.ci = vec![ci(&[("CI / test", 9, 50, Running)])];
    assert!(pass(&mut s).is_empty());
    // Nor while the same job passed in another run at the commit, such as
    // a pull request's run of the same workflow.
    s.ci = vec![ci(&[
        ("CI / test", 9, 50, Running),
        ("CI / test", 8, 3, Passing),
        lint,
    ])];
    assert!(pass(&mut s).is_empty());
    // Nor while a job in its run by the same name passed: two jobs there
    // by one name can't be told apart, even once both finish.
    s.ci = vec![ci(&[
        ("CI / test", 9, 50, Running),
        ("CI / test", 9, 6, Passing),
    ])];
    assert!(pass(&mut s).is_empty());
    s.ci = vec![ci(&[
        ("CI / test", 9, 50, Passing),
        ("CI / test", 9, 6, Passing),
    ])];
    assert_eq!(
        pass(&mut s)[0].why,
        "Two jobs in its run share its name, so Orbit can't tell which one was rerun."
    );
    // Nor from a read that missed some checks: one it missed may be the
    // rerun, beside a job in its run by the same name that passed.
    s.outcomes.clear();
    s.ci = vec![ci(&[("CI / test", 9, 6, Passing), lint])];
    s.ci[0].head.incomplete = true;
    assert!(pass(&mut s).is_empty());
    s.ci[0].head.incomplete = false;
    s.ci[0].head.cursor = Some("page-2".into());
    assert!(pass(&mut s).is_empty());
    // Cancelled, with another check failing and the same job passed in
    // another run: it didn't pass.
    s.outcomes.clear();
    s.ci = vec![ci(&[
        ("CI / test", 9, 50, Neither),
        ("CI / test", 8, 3, Passing),
        lint,
    ])];
    let settled = pass(&mut s);
    assert_eq!(
        (settled[0].verdict, settled[0].why.as_str()),
        (
            Verdict::Unclear,
            "It didn't pass or fail after the rerun: GitHub may have cancelled or skipped it."
        )
    );
    // It holds when GitHub says it passed in its run, whatever else fails.
    s.outcomes.clear();
    s.ci = vec![ci(&[
        ("CI / test", 9, 50, Passing),
        ("CI / build", 9, 51, Running),
        lint,
    ])];
    let settled = pass(&mut s);
    assert_eq!(settled[0].verdict, Verdict::Held);
    assert_eq!(settled[0].why, "It passed after the rerun.");
    // Not from CI read before the rerun, nor from before a restart, which
    // has no names of its checks.
    s.outcomes.clear();
    s.ci[0].checked_at = ago(20);
    assert!(pass(&mut s).is_empty());
    s.ci[0].checked_at = ago(5);
    s.ci[0].head.checks.clear();
    s.ci[0].head.named = false;
    assert!(pass(&mut s).is_empty());

    // Failing again is a miss.
    let mut s = snapshot();
    s.ops = vec![thread(true, true)];
    s.ci = vec![ci(&[("CI / test", 9, 50, Failing)])];
    let settled = pass(&mut s);
    assert_eq!(
        (settled[0].verdict, settled[0].why.as_str()),
        (Verdict::Missed, "It failed again after the rerun.")
    );
    assert_eq!(
        missed_text(&settled[0]),
        "Orbit reran CI / test at 1111111 on main in a/b. It failed again after the rerun."
    );

    // A newer commit first: Orbit can't tell.
    let mut s = snapshot();
    s.ops = vec![thread(false, true)];
    record(&mut s, now());
    s.ops[0].items[0].at = OTHER.into();
    assert_eq!(settle(&mut s, now())[0].verdict, Verdict::Unclear);
}

#[test]
fn a_release_build_rerun_is_told_by_its_run_not_the_default_branch() {
    let job = "https://github.com/a/b/actions/runs/9/job/5";
    let mut s = snapshot();
    s.problems.push(crate::problems::Problem {
        id: "p1".into(),
        kind: ProblemKind::ReleaseBuild,
        repo: "a/b".into(),
        branch: "v1.2.0".into(),
        sha: SHA.into(),
        ..Default::default()
    });
    s.ops = vec![Thread {
        key: "issue:p1".into(),
        kind: crate::ops::ThreadKind::Issue,
        repo: "a/b".into(),
        branch: "v1.2.0".into(),
        problem_id: "p1".into(),
        items: vec![Item {
            number: 1,
            check: "Release / build (macos)".into(),
            url: job.into(),
            at: SHA.into(),
            reruns: vec![Rerun {
                url: job.into(),
                at: ago(10),
                done: true,
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    }];
    // The default branch at the same commit, passing, says nothing of it.
    s.ci = vec![RepoCi {
        repo: "a/b".into(),
        branch: "main".into(),
        head: CommitCi {
            sha: SHA.into(),
            state: CiState::Passing,
            passing: 1,
            checks: vec![(
                Check {
                    name: "Release / build (macos)".into(),
                    url: "https://github.com/a/b/actions/runs/9/job/6".into(),
                },
                CiState::Passing,
            )],
            named: true,
            ..Default::default()
        },
        ..Default::default()
    }];
    let run = |state: CiState, conclusion: &str| crate::builds::Run {
        id: 9,
        name: "Release".into(),
        state,
        conclusion: conclusion.into(),
        ..Default::default()
    };
    let build = |tag: &str, read: i64, run: crate::builds::Run| crate::builds::Build {
        repo: "a/b".into(),
        tag: tag.into(),
        sha: SHA.into(),
        runs: vec![run],
        read_at: ago(read),
        ..Default::default()
    };
    assert!(pass(&mut s).is_empty());
    assert_eq!(
        s.outcomes[0].what,
        "Orbit reran Release / build (macos) at 1111111 on v1.2.0 in a/b."
    );
    // A build read before the rerun, or still running, waits.
    s.builds = vec![build("v1.2.0", 20, run(CiState::Passing, "success"))];
    assert!(pass(&mut s).is_empty());
    s.builds = vec![build("v1.2.0", 5, run(CiState::Running, ""))];
    assert!(pass(&mut s).is_empty());
    // Its run passing holds, though the default branch moved on.
    s.ci[0].head.sha = OTHER.into();
    s.builds = vec![build("v1.2.0", 5, run(CiState::Passing, "success"))];
    let settled = pass(&mut s);
    assert_eq!(
        (settled[0].verdict, settled[0].why.as_str()),
        (Verdict::Held, "Its run passed after the rerun.")
    );
    // A cancelled run can't be told, nor one Orbit stopped watching.
    s.outcomes.clear();
    s.builds = vec![build("v1.2.0", 5, run(CiState::None, "cancelled"))];
    assert_eq!(
        pass(&mut s)[0].why,
        "Its run didn't pass or fail after the rerun: GitHub says it was cancelled."
    );
    s.outcomes.clear();
    s.builds = vec![build("v1.3.0", 5, run(CiState::Passing, "success"))];
    assert_eq!(
        pass(&mut s)[0].why,
        "Orbit watches the build of v1.3.0 now."
    );
}

#[test]
fn a_rebase_holds_once_its_pull_request_moves() {
    let mut s = snapshot();
    s.journal.push(Write {
        number: Some(4),
        head: Some(SHA.into()),
        ..write(Action::AskRebase, 10)
    });
    let mut pr = crate::types::tests::pr();
    pr.id = "a/b#4".into();
    pr.number = 4;
    pr.head_sha = SHA.into();
    s.prs.push(pr);
    pass(&mut s);
    assert_eq!(
        s.outcomes[0].what,
        "Orbit asked Dependabot to rebase a/b #4."
    );
    assert_eq!(s.outcomes[0].verdict, Verdict::Waiting);
    assert_eq!(s.outcomes[0].expect, "Dependabot rebases it within a day");
    assert_eq!(
        s.outcomes[0].due,
        Some(stamp(
            now() - Duration::minutes(10) + Duration::hours(REBASE_HOURS)
        ))
    );
    let mut missed = s.clone();
    s.prs[0].head_sha = OTHER.into();
    let settled = pass(&mut s);
    assert_eq!(settled[0].verdict, Verdict::Held);
    assert_eq!(settled[0].why, "Dependabot rebased it, to 2222222.");
    // Still at the same commit a day on, by a sync after then, is a miss.
    let day = now() + Duration::hours(REBASE_HOURS);
    missed.last_poll = Some(stamp(day - Duration::minutes(20)));
    assert!(settle(&mut missed, day).is_empty(), "not synced since");
    missed.last_poll = Some(stamp(day));
    // Nor by a sync that failed for its repository, which keeps what
    // Orbit read before.
    missed.sync_failures = vec![crate::types::SyncFailure {
        repo: "a/b".into(),
        count: 1,
        error: "HTTP 502".into(),
    }];
    assert!(settle(&mut missed, day).is_empty(), "its sync failed");
    missed.sync_failures.clear();
    let settled = settle(&mut missed, day);
    assert_eq!(
        (settled[0].verdict, settled[0].why.as_str()),
        (Verdict::Missed, "Dependabot didn't rebase it within a day.")
    );
    // One that closed first can't be told.
    let mut s = snapshot();
    s.journal.push(Write {
        number: Some(4),
        head: Some(SHA.into()),
        ..write(Action::AskRebase, 10)
    });
    assert_eq!(pass(&mut s)[0].verdict, Verdict::Unclear);
}

#[test]
fn an_agents_fix_holds_or_misses_by_its_commits_checks() {
    let thread = |check: Option<FixCheck>, commit: &str| Thread {
        key: "pr:a/b#3".into(),
        repo: "a/b".into(),
        number: Some(3),
        pr_id: Some("a/b#3".into()),
        items: vec![Item {
            number: 2,
            check: "CI / test".into(),
            failing: true,
            at: SHA.into(),
            answer: Some(Answer {
                kind: AnswerKind::Fixed,
                author: "agent".into(),
                at: ago(10),
                url: "https://github.com/a/b/pull/3#issuecomment-1".into(),
                commit: Some(commit.into()),
                check,
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    for (check, verdict, why) in [
        (
            FixCheck::Passes,
            Verdict::Held,
            "Its checks pass at 2222222.",
        ),
        (
            FixCheck::Fails,
            Verdict::Missed,
            "Its checks fail at 2222222.",
        ),
        (
            FixCheck::NotOnBranch,
            Verdict::Missed,
            "2222222 isn't on the branch Orbit reads.",
        ),
        (
            FixCheck::NotFound,
            Verdict::Missed,
            "GitHub doesn't know the commit 2222222.",
        ),
    ] {
        let mut s = snapshot();
        s.ops = vec![thread(Some(FixCheck::Waiting), OTHER)];
        pass(&mut s);
        let o = &s.outcomes[0];
        assert_eq!(o.id, format!("fix:pr:a/b#3:2:{OTHER}"));
        assert_eq!(
            o.what,
            "agent said I2 (CI / test) is fixed in 2222222 on a/b #3."
        );
        assert_eq!(o.expect, "its checks pass at 2222222");
        assert_eq!(o.verdict, Verdict::Waiting);
        s.ops = vec![thread(Some(check), OTHER)];
        let settled = pass(&mut s);
        assert_eq!(
            (settled[0].verdict, settled[0].why.as_str()),
            (verdict, why)
        );
    }
    // A newer answer about it makes the older one moot.
    let mut s = snapshot();
    s.ops = vec![thread(Some(FixCheck::Waiting), OTHER)];
    record(&mut s, now());
    s.ops = vec![thread(Some(FixCheck::Waiting), SHA)];
    let settled = settle(&mut s, now());
    assert_eq!(settled[0].verdict, Verdict::Unclear);
    // And it gets a check of its own.
    assert!(record(&mut s, now()));
}

#[test]
fn an_outcome_orbit_cant_tell_gives_up_and_old_ones_go() {
    let mut s = snapshot();
    s.journal.push(Write {
        number: Some(4),
        head: Some(SHA.into()),
        ..write(Action::AskRebase, 10)
    });
    let mut pr = crate::types::tests::pr();
    pr.id = "a/b#4".into();
    pr.number = 4;
    pr.head_sha = SHA.into();
    s.prs.push(pr);
    pass(&mut s);
    // Done 10 minutes ago, due a day later: Orbit waits 7 days from then.
    let later = now() - Duration::minutes(10)
        + Duration::hours(REBASE_HOURS)
        + Duration::days(GIVE_UP_DAYS);
    assert!(settle(&mut s, later).is_empty());
    let settled = settle(&mut s, later + Duration::minutes(1));
    assert_eq!(
        (settled[0].verdict, settled[0].why.as_str()),
        (Verdict::Unclear, "Orbit couldn't tell within 7 days.")
    );
    // Waiting ones stay; settled ones go after a while, misses later.
    let at = |days: i64| Some(stamp(now() - Duration::days(days)));
    s.outcomes = vec![
        Outcome {
            id: "waiting".into(),
            ..Default::default()
        },
        Outcome {
            id: "held".into(),
            verdict: Verdict::Held,
            settled_at: at(KEEP_DAYS + 1),
            ..Default::default()
        },
        Outcome {
            id: "missed".into(),
            verdict: Verdict::Missed,
            settled_at: at(KEEP_DAYS + 1),
            ..Default::default()
        },
        Outcome {
            id: "old miss".into(),
            verdict: Verdict::Missed,
            settled_at: at(KEEP_MISSED_DAYS + 1),
            ..Default::default()
        },
    ];
    assert!(prune(&mut s.outcomes, now()));
    let kept: Vec<&str> = s.outcomes.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(kept, ["waiting", "missed"]);
    assert!(!prune(&mut s.outcomes, now()));
}

#[test]
fn a_miss_is_a_note_from_bit() {
    let mut s = snapshot();
    s.settings.buddy.enabled = true;
    let missed = Outcome {
        id: "write:w1".into(),
        kind: Kind::Tag,
        repo: "a/b".into(),
        what: "Orbit pushed the tag v1.2.0 in a/b.".into(),
        why: "GitHub doesn't list the tag.".into(),
        verdict: Verdict::Missed,
        ..Default::default()
    };
    let held = Outcome {
        verdict: Verdict::Held,
        ..missed.clone()
    };
    assert!(crate::buddy::observe_outcomes(
        &mut s,
        &[missed.clone(), held]
    ));
    let notes = &s.buddy_feed.notes;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].kind, crate::buddy::NoteKind::Ci);
    assert_eq!(notes[0].key.as_deref(), Some("outcome:write:w1"));
    assert!(notes[0]
        .text
        .contains("Orbit pushed the tag v1.2.0 in a/b. GitHub doesn't list the tag."));
    // A rerun or a fix that missed is noted from its CI thread already.
    for kind in [Kind::Rerun, Kind::Fix] {
        let elsewhere = Outcome {
            kind,
            id: "rerun:x".into(),
            ..missed.clone()
        };
        assert!(!crate::buddy::observe_outcomes(&mut s, &[elsewhere]));
    }
    assert_eq!(s.buddy_feed.notes.len(), 1);
    // Nothing while Bit is off.
    s.settings.buddy.enabled = false;
    assert!(!crate::buddy::observe_outcomes(&mut s, &[missed]));
}

/// GitHub's CI for a/b's main at `sha`, read at `at`.
fn main_at(sha: &str, state: CiState, at: DateTime<Utc>) -> RepoCi {
    RepoCi {
        repo: "a/b".into(),
        branch: "main".into(),
        head: CommitCi {
            sha: sha.into(),
            state,
            failing: if state == CiState::Failing {
                vec![Check {
                    name: "CI / test".into(),
                    url: String::new(),
                }]
            } else {
                vec![]
            },
            ..Default::default()
        },
        complete: true,
        checked_at: stamp(at),
        ..Default::default()
    }
}
/// a/b's main failed at 3333333, then passed at 1111111 at `now()`.
fn fixed() -> Snapshot {
    let mut s = snapshot();
    let read = |s: &mut Snapshot, ci: RepoCi, at| {
        crate::problems::observe(&mut s.problems, &ci, &[], at);
        s.ci = vec![ci];
    };
    let before = now() - Duration::minutes(30);
    read(
        &mut s,
        main_at(&"3".repeat(40), CiState::Failing, before),
        before,
    );
    read(&mut s, main_at(SHA, CiState::Passing, now()), now());
    s
}

#[test]
fn a_fixed_default_branch_holds_3_days_on_once() {
    let mut s = fixed();
    assert!(pass(&mut s).is_empty());
    let problem = s.problems[0].id.clone();
    let o = s.outcomes[0].clone();
    assert_eq!(o.id, format!("holds:{problem}:{SHA}"));
    assert_eq!(o.kind, Kind::Holds);
    assert_eq!(o.what, "CI on main in a/b passed at 1111111 after failing.");
    assert_eq!(o.expect, "it stays fixed for 3 days");
    assert_eq!(o.url, format!("https://github.com/a/b/commit/{SHA}"));
    let due = now() + Duration::days(HOLDS_DAYS);
    assert_eq!(o.due, Some(stamp(due)));
    // Still green 2 days on: waiting.
    let two = now() + Duration::days(2);
    s.ci = vec![main_at(SHA, CiState::Passing, two)];
    assert!(settle(&mut s, two).is_empty());
    // Due, but Orbit hasn't read CI since then.
    assert!(settle(&mut s, due).is_empty());
    // Read on the date while a newer commit's checks run: not yet.
    s.ci = vec![main_at(&"5".repeat(40), CiState::Running, due)];
    assert!(settle(&mut s, due).is_empty());
    // Read on the date passing: it held, once.
    s.ci = vec![main_at(SHA, CiState::Passing, due)];
    let settled = settle(&mut s, due);
    assert_eq!(
        (settled[0].verdict, settled[0].why.as_str()),
        (Verdict::Held, "It stayed fixed for 3 days.")
    );
    assert!(settle(&mut s, due).is_empty());
    assert!(!record(&mut s, due));
    assert_eq!(s.outcomes.len(), 1);
}

#[test]
fn a_fixed_default_branch_that_breaks_again_misses_at_once() {
    // 2 days later: a new problem, which says when it was fixed before.
    let mut s = fixed();
    pass(&mut s);
    let two = now() + Duration::days(2);
    let red = main_at(&"4".repeat(40), CiState::Failing, two);
    crate::problems::observe(&mut s.problems, &red, &[], two);
    let settled = settle(&mut s, two);
    assert_eq!(
        (settled[0].verdict, settled[0].why.as_str()),
        (Verdict::Missed, "It failed again 2 days after it passed.")
    );
    let back = s.problems.iter().find(|p| p.is_open()).unwrap();
    assert_eq!(
        back.fixed_before,
        Some(crate::problems::FixedBefore {
            at: stamp(now()),
            sha: SHA.into()
        })
    );
    assert!(back
        .timeline
        .iter()
        .any(|e| e.text == "It was fixed before: it passed at 1111111 2 days earlier."));
    // Bit's note is the new problem's, not a second one.
    s.settings.buddy.enabled = true;
    assert!(!crate::buddy::observe_outcomes(&mut s, &settled));

    // Back within the hour: the same problem, failing again, also a miss.
    let mut s = fixed();
    pass(&mut s);
    let soon = now() + Duration::minutes(30);
    let red = main_at(&"4".repeat(40), CiState::Failing, soon);
    crate::problems::observe(&mut s.problems, &red, &[], soon);
    assert_eq!(s.problems.len(), 1);
    let settled = settle(&mut s, soon);
    assert_eq!(
        settled[0].why,
        "It failed again 30 minutes after it passed."
    );
    // Passing again at another commit is a new check.
    let green = now() + Duration::minutes(40);
    let ci = main_at(OTHER, CiState::Passing, green);
    crate::problems::observe(&mut s.problems, &ci, &[], green);
    assert!(record(&mut s, green));
    assert_eq!(
        s.outcomes[1].id,
        format!("holds:{}:{OTHER}", s.problems[0].id)
    );
}

#[test]
fn a_setting_that_held_is_checked_again_a_week_later() {
    let mut s = snapshot();
    s.journal.push(Write {
        setting: Some(Setting::DependabotAlerts),
        ..write(Action::ChangeSetting, 10)
    });
    let read = |at: DateTime<Utc>, on: bool| RepoSettings {
        repo: "a/b".into(),
        read_at: stamp(at),
        found: vec![SettingFound {
            setting: Setting::DependabotAlerts,
            on: Some(on),
        }],
        ..Default::default()
    };
    s.repo_settings = vec![read(now(), true)];
    assert_eq!(pass(&mut s)[0].verdict, Verdict::Held);
    // The check due later comes once it held.
    assert!(record(&mut s, now()));
    let later = s.outcomes[1].clone();
    assert_eq!(later.id, "later:write:w10");
    assert_eq!(later.kind, Kind::StillOn);
    assert_eq!(later.what, "Orbit turned on Dependabot alerts in a/b.");
    assert_eq!(later.expect, "it's still on a week later");
    let week = now() - Duration::minutes(10) + Duration::days(STILL_ON_DAYS);
    assert_eq!(later.due, Some(stamp(week)));
    assert!(!record(&mut s, now()));
    let mut off = s.clone();
    // Still on before the date: waiting; on, read on the date: held.
    s.repo_settings = vec![read(now() + Duration::days(3), true)];
    assert!(settle(&mut s, now() + Duration::days(3)).is_empty());
    s.repo_settings = vec![read(week, true)];
    let settled = settle(&mut s, week);
    assert_eq!(
        (settled[0].verdict, settled[0].why.as_str()),
        (Verdict::Held, "It's still on a week later.")
    );
    // Read as off before the date: a miss at once.
    off.repo_settings = vec![read(now() + Duration::days(2), false)];
    let settled = settle(&mut off, now() + Duration::days(2));
    assert_eq!(
        (settled[0].verdict, settled[0].why.as_str()),
        (Verdict::Missed, "It reads as off again.")
    );
}

#[test]
fn a_tag_that_held_has_a_release_build_that_passes_within_a_day() {
    use crate::builds::Build;
    let start = || {
        let mut s = snapshot();
        s.settings.watch_ci = true;
        s.journal.push(Write {
            head: Some(SHA.into()),
            tag: Some("v1.2.0".into()),
            ..write(Action::PushTag, 10)
        });
        s.releases = vec![RepoReleases {
            repo: "a/b".into(),
            read_at: stamp(now()),
            tags: vec![Tag {
                name: "v1.2.0".into(),
                sha: SHA.into(),
            }],
            ..Default::default()
        }];
        assert_eq!(pass(&mut s)[0].verdict, Verdict::Held);
        assert!(record(&mut s, now()));
        s
    };
    let build = |tag: &str, state: CiState| Build {
        repo: "a/b".into(),
        tag: tag.into(),
        sha: SHA.into(),
        read_at: stamp(now()),
        runs: vec![crate::builds::Run {
            id: 9,
            name: "Release".into(),
            state,
            conclusion: match state {
                CiState::Passing => "success",
                CiState::Failing => "failure",
                _ => "",
            }
            .into(),
            ..Default::default()
        }],
        ci: CommitCi {
            sha: SHA.into(),
            state,
            failing: if state == CiState::Failing {
                vec![Check {
                    name: "Release / build".into(),
                    url: String::new(),
                }]
            } else {
                vec![]
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let mut s = start();
    let o = &s.outcomes[1];
    assert_eq!((o.id.as_str(), o.kind), ("later:write:w10", Kind::Build));
    assert_eq!(o.expect, "its release build passes within a day");
    let day = now() - Duration::minutes(10) + Duration::hours(BUILD_HOURS);
    assert_eq!(o.due, Some(stamp(day)));
    // Failing before the day is up may still pass after a rerun.
    s.builds = vec![build("v1.2.0", CiState::Failing)];
    assert!(settle(&mut s, now()).is_empty());
    s.builds = vec![build("v1.2.0", CiState::Passing)];
    assert_eq!(settle(&mut s, now())[0].why, "Its release build passed.");
    for (state, verdict, why) in [
        (
            CiState::Failing,
            Verdict::Missed,
            "Its release build still fails: Release / build.",
        ),
        (
            CiState::Running,
            Verdict::Missed,
            "Its release build hadn't passed within a day.",
        ),
        (
            CiState::None,
            Verdict::Unclear,
            "No release build ran for it within a day.",
        ),
    ] {
        let mut s = start();
        s.builds = vec![build("v1.2.0", state)];
        if state == CiState::None {
            s.builds[0].runs.clear();
        }
        let settled = settle(&mut s, day);
        assert_eq!(
            (settled[0].verdict, settled[0].why.as_str()),
            (verdict, why)
        );
    }
    // A cancelled run beside one that passed isn't a pass: by the day,
    // Orbit can't tell.
    let mut s = start();
    let mut b = build("v1.2.0", CiState::Passing);
    b.runs.push(crate::builds::Run {
        id: 10,
        name: "Docs".into(),
        state: CiState::None,
        conclusion: "cancelled".into(),
        ..Default::default()
    });
    s.builds = vec![b];
    assert!(settle(&mut s, now()).is_empty());
    assert_eq!(
        settle(&mut s, day)[0].why,
        "Orbit couldn't tell whether its release build passed within a day: Docs was cancelled."
    );
    // A read from before the tag was pushed waits.
    let mut s = start();
    s.builds = vec![build("v1.2.0", CiState::Passing)];
    s.builds[0].read_at = ago(20);
    assert!(settle(&mut s, now()).is_empty());
    // A newer tag's build is what Orbit watches now.
    let mut s = start();
    s.builds = vec![build("v1.3.0", CiState::Passing)];
    assert_eq!(
        settle(&mut s, now())[0].why,
        "Orbit watches the build of v1.3.0 now."
    );
}
