use super::*;
use crate::ci::{Check, CommitCi, PullCi, RepoCi};
use crate::security::{Kind, RepoSecurity};
use crate::types::tests::pr;
use chrono::Duration;
use serde_json::json;

pub(crate) fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-02T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
pub(crate) const MAIN: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn bump(name: &str, from: &str, to: &str) -> Bump {
    Bump {
        name: name.into(),
        from: from.into(),
        to: to.into(),
    }
}
pub(crate) fn alert(number: u64, package: &str, path: &str, range: &str, fixed: &str) -> Alert {
    Alert {
        source: Source::Dependabot,
        number,
        severity: Severity::High,
        subject: package.into(),
        path: path.into(),
        ranges: if range.is_empty() {
            vec![]
        } else {
            vec![range.into()]
        },
        fixed_in: fixed.into(),
        url: format!("https://github.com/a/b/security/dependabot/{number}"),
        ..Default::default()
    }
}
/// One of Dependabot's pull requests in `a/b`, against main's latest
/// commit, with this title and head.
pub(crate) fn update(number: u64, title: &str, head: char) -> PullRequest {
    let mut p = pr();
    p.id = pr_id("a/b", number);
    p.number = number;
    p.title = title.into();
    p.author = "dependabot[bot]".into();
    p.head_repo = "a/b".into();
    p.head_sha = head.to_string().repeat(40);
    p.base_sha = MAIN.into();
    p.url = format!("https://github.com/a/b/pull/{number}");
    p
}
/// `a/b` connected and watched, with CI read for main and these pull
/// requests, by number, head and the state of their checks.
pub(crate) fn snapshot(prs: Vec<PullRequest>, checks: &[(u64, CiState)]) -> Snapshot {
    let mut s = Snapshot::default();
    s.settings.watch_ci = true;
    s.settings.watch_security = true;
    s.settings.repositories.push(Repository {
        name: "a/b".into(),
        path: "/tmp/a-b".into(),
        automatic: false,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes: 0,
        keep_updated: false,
        provider: Default::default(),
        dev_container: Default::default(),
    });
    let pulls = checks
        .iter()
        .map(|&(number, state)| {
            let head = prs.iter().find(|p| p.number == number).unwrap();
            PullCi {
                number,
                draft: false,
                ci: CommitCi {
                    sha: head.head_sha.clone(),
                    state,
                    passing: if state == CiState::Passing { 3 } else { 1 },
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
            }
        })
        .collect();
    s.ci.push(RepoCi {
        repo: "a/b".into(),
        branch: "main".into(),
        head: CommitCi {
            sha: MAIN.into(),
            state: CiState::Passing,
            ..Default::default()
        },
        pulls,
        complete: true,
        ..Default::default()
    });
    s.prs = prs;
    s
}
pub(crate) fn merge(s: &Snapshot, number: u64, mergeable: Option<bool>, state: &str) -> Merge {
    let pr = s.prs.iter().find(|p| p.number == number).unwrap();
    Merge {
        pr_id: pr.id.clone(),
        head_sha: pr.head_sha.clone(),
        base_sha: MAIN.into(),
        read_at: crate::problems::stamp(now()),
        mergeable,
        state: state.into(),
        ..Default::default()
    }
}

#[test]
fn dependabots_titles_and_descriptions_say_what_each_update_changes() {
    let p = parse("Bump lodash from 4.17.20 to 4.17.21", "");
    assert_eq!(p.bumps, [bump("lodash", "4.17.20", "4.17.21")]);
    assert_eq!(p.folder.as_deref(), Some(""));
    assert_eq!(
        what_text(&p),
        "Updates lodash from 4.17.20 to 4.17.21, a patch update."
    );

    // A prefix, a folder and a requirement.
    let p = parse(
        "chore(deps): bump actions/checkout from 3 to 4 in /.github/workflows",
        "",
    );
    assert_eq!(p.bumps, [bump("actions/checkout", "3", "4")]);
    assert_eq!(p.folder.as_deref(), Some(".github/workflows"));
    assert_eq!(
        what_text(&p),
        "Updates actions/checkout from 3 to 4 in /.github/workflows, a major update."
    );
    let p = parse("Update serde requirement from ~1.0.100 to ~1.1.0", "");
    assert_eq!(p.bumps, [bump("serde", "~1.0.100", "~1.1.0")]);
    assert_eq!(
        what_text(&p),
        "Updates serde from ~1.0.100 to ~1.1.0, a minor update."
    );

    // A group, with a line per package in the description.
    let body = "Bumps the npm_and_yarn group with 4 updates in the /web directory: [lodash](https://x), [qs](https://y), [react](https://z) and a.\n\nUpdates `lodash` from 4.17.20 to 4.17.21\n- [Release notes](https://x)\n\nUpdates `qs` from 6.10.1 to 6.10.3\nUpdates `react` from 18.3.1 to 19.0.0\nUpdates `left-pad` from 1.0.0 to 1.1.0\nUpdates `qs` from 6.10.1 to 6.10.3\n";
    let p = parse(
        "Bump the npm_and_yarn group across 1 directory with 4 updates",
        body,
    );
    assert_eq!(p.group, "npm_and_yarn");
    assert_eq!(p.count, 4);
    assert_eq!(p.folder.as_deref(), Some("web"));
    assert_eq!(p.bumps.len(), 4, "each package once");
    assert_eq!(
        what_text(&p),
        "Updates 4 packages in the npm_and_yarn group in /web: lodash from 4.17.20 to 4.17.21, qs from 6.10.1 to 6.10.3, react from 18.3.1 to 19.0.0 and 1 more. The biggest is a major update."
    );
    // Across more than one folder, Orbit can't say which.
    let p = parse(
        "Bump the npm_and_yarn group across 2 directories with 1 update",
        "Updates `lodash` from 4.17.20 to 4.17.21",
    );
    assert_eq!(p.folder, None);
    assert_eq!(
        what_text(&p),
        "Updates 1 package in the npm_and_yarn group: lodash from 4.17.20 to 4.17.21, a patch update."
    );
    // Its description names each folder: Orbit can't say which package
    // is in which.
    let p = parse(
        "Bump the npm_and_yarn group across 2 directories with 2 updates",
        "Bumps the npm_and_yarn group with 1 update in the /web directory: lodash.\nBumps the npm_and_yarn group with 1 update in the /api directory: lodash.\n\nUpdates `lodash` from 4.17.20 to 4.17.21",
    );
    assert_eq!(p.folder, None);
    // One folder named twice is that folder; none named across one is
    // unknown, and a title that names none is about the top one.
    let p = parse(
        "Bump the npm_and_yarn group across 1 directory with 2 updates",
        "Bumps the npm_and_yarn group with 2 updates in the /web directory. Updates `qs` from 6.10.1 to 6.10.3 in the /web directory.",
    );
    assert_eq!(p.folder.as_deref(), Some("web"));
    let p = parse(
        "Bump the npm_and_yarn group across 1 directory with 1 update",
        "Updates `qs` from 6.10.1 to 6.10.3",
    );
    assert_eq!(p.folder, None);
    let p = parse(
        "Bump the npm_and_yarn group with 1 update",
        "Updates `qs` from 6.10.1 to 6.10.3",
    );
    assert_eq!(p.folder.as_deref(), Some(""));
    let p = parse("Bump the github-actions group in /ci with 2 updates", "");
    assert_eq!(p.folder.as_deref(), Some("ci"));
    assert_eq!(
        what_text(&p),
        "Updates 2 packages in the github-actions group in /ci."
    );
    assert_eq!(
        what_text(&parse("Release 1.2.0", "")),
        "Orbit can't tell from its title what it updates."
    );
}

#[test]
fn an_update_is_patch_minor_or_major_by_the_first_number_that_changes() {
    assert_eq!(change("4.17.20", "4.17.21"), Some(Change::Patch));
    assert_eq!(change("1.2", "1.3.0"), Some(Change::Minor));
    assert_eq!(change("v1.9.9", "v2.0.0"), Some(Change::Major));
    assert_eq!(change("^1.2.3", "^1.2.4"), Some(Change::Patch));
    assert_eq!(change("1.0.0-beta", "1.0.0"), None);
    assert_eq!(change("1.0.0", "1.0"), None, "nothing changes");
    assert_eq!(change("abc1234", "def5678"), None);
}

#[test]
fn an_update_fixes_an_alert_when_its_new_version_is_outside_the_range() {
    let fixed = bump("lodash", "4.17.20", "4.17.21");
    let a = alert(1, "lodash", "package-lock.json", "< 4.17.21", "4.17.21");
    assert!(fixes(&a, &fixed, Some("")));
    assert!(fixes(&a, &bump("Lodash", "4.17.20", "4.18.0"), Some("")));
    assert!(!fixes(&a, &bump("lodash", "4.17.19", "4.17.20"), Some("")));
    // Another package, folder or kind of alert.
    assert!(!fixes(&a, &bump("qs", "4.17.20", "4.17.21"), Some("")));
    assert!(!fixes(&a, &fixed, Some("web")));
    assert!(!fixes(&a, &fixed, None), "a folder Orbit can't tell");
    let web = alert(2, "lodash", "web/package-lock.json", "< 4.17.21", "");
    assert!(fixes(&web, &fixed, Some("web")));
    assert!(!fixes(&web, &fixed, Some("")));
    let code = Alert {
        source: Source::CodeScanning,
        ..a.clone()
    };
    assert!(!fixes(&code, &fixed, Some("")));
    // A range with a floor and a ceiling.
    let both = alert(3, "qs", "", ">= 6.10.0, < 6.10.3", "6.10.3");
    assert!(!fixes(&both, &bump("qs", "6.9.0", "6.10.1"), Some("")));
    assert!(fixes(&both, &bump("qs", "6.10.1", "6.10.3"), Some("")));
    assert!(fixes(&both, &bump("qs", "6.10.1", "7.0.0"), Some("")));
    // Without every range Orbit can read, only the version that fixes it
    // counts, as another release line may still be affected.
    let fixed_only = alert(4, "lodash", "", "", "4.17.21");
    assert!(fixes(
        &fixed_only,
        &bump("lodash", "4.17.20", "4.17.21"),
        Some("")
    ));
    assert!(!fixes(
        &fixed_only,
        &bump("lodash", "4.17.20", "4.17.22"),
        Some("")
    ));
    assert!(!fixes(
        &fixed_only,
        &bump("lodash", "4.17.19", "4.17.20"),
        Some("")
    ));
    let unread = Alert {
        ranges: vec!["< 4.17.21".into(), "~> 5.0".into()],
        ..fixed_only.clone()
    };
    assert!(!fixes(
        &unread,
        &bump("lodash", "4.17.20", "4.17.22"),
        Some("")
    ));
    assert!(fixes(
        &unread,
        &bump("lodash", "4.17.20", "4.17.21"),
        Some("")
    ));
    let pre = alert(5, "lodash", "", "< 5.0.0-rc.1", "5.0.0-rc.1");
    assert!(fixes(
        &pre,
        &bump("lodash", "4.0.0", "5.0.0-rc.1"),
        Some("")
    ));
    assert!(!fixes(
        &pre,
        &bump("lodash", "4.0.0", "5.0.0-rc.2"),
        Some("")
    ));
    assert!(!fixes(&alert(6, "lodash", "", "", ""), &fixed, Some("")));

    // An advisory that affects two release lines: leaving one isn't a fix
    // while the new version is in the other.
    let django = Alert {
        ranges: vec![">= 1.11.8, < 1.11.10".into(), ">= 2.0.0, < 2.0.2".into()],
        ..alert(7, "django", "requirements.txt", "", "1.11.10")
    };
    assert!(!fixes(
        &django,
        &bump("django", "1.11.9", "2.0.1"),
        Some("")
    ));
    assert!(fixes(&django, &bump("django", "1.11.9", "2.0.2"), Some("")));
    assert!(fixes(
        &django,
        &bump("django", "1.11.9", "1.11.10"),
        Some("")
    ));

    assert_eq!(in_range("< 4.17.21", "4.17.20"), Some(true));
    assert_eq!(in_range("<= 1.0.0", "1.0"), Some(true));
    assert_eq!(in_range("= 1.0.0", "1.0.1"), Some(false));
    assert_eq!(in_range("> 2.0.0", "2.0.1"), Some(true));
    assert_eq!(in_range("", "1.0.0"), None);
    assert_eq!(in_range("~> 1.0", "1.0.1"), None);
}

/// An advisory with more ranges than Orbit keeps: none is kept, so a
/// version in one Orbit would have left out is never called a fix.
#[test]
fn an_advisory_with_more_ranges_than_orbit_keeps_claims_only_the_fixing_version() {
    let mut v = crate::security::tests::dependabot(1, "lodash", "high");
    v["security_vulnerability"]["vulnerable_version_range"] = json!(">= 1.0.0, < 1.0.2");
    v["security_vulnerability"]["first_patched_version"]["identifier"] = json!("1.0.2");
    let lines: Vec<_> = (1..=21)
        .map(|n| {
            json!({
                "package": { "ecosystem": "npm", "name": "lodash" },
                "vulnerable_version_range": format!(">= {n}.0.0, < {n}.0.2"),
            })
        })
        .collect();
    v["security_advisory"]["vulnerabilities"] = json!(lines);
    let a = crate::security::alert(Source::Dependabot, &v).unwrap();
    assert!(a.ranges.is_empty());
    assert!(!fixes(&a, &bump("lodash", "1.0.1", "21.0.1"), Some("")));
    assert!(fixes(&a, &bump("lodash", "1.0.1", "1.0.2"), Some("")));
    // With 20, each is kept and checked.
    v["security_advisory"]["vulnerabilities"]
        .as_array_mut()
        .unwrap()
        .truncate(20);
    let a = crate::security::alert(Source::Dependabot, &v).unwrap();
    assert_eq!(a.ranges.len(), 20);
    assert!(!fixes(&a, &bump("lodash", "1.0.1", "20.0.1"), Some("")));
    assert!(fixes(&a, &bump("lodash", "1.0.1", "21.0.1"), Some("")));
}

/// The three in "done when": a green patch update, a failing major
/// update, and a security fix that conflicts, which comes first.
#[test]
fn each_update_shows_what_it_fixes_its_checks_and_whether_it_merges() {
    let prs = vec![
        update(10, "Bump vite from 5.4.10 to 5.4.11", '1'),
        update(11, "Bump react from 18.3.1 to 19.0.0", '2'),
        update(12, "Bump qs from 6.10.1 to 6.10.3", '3'),
    ];
    let mut s = snapshot(
        prs,
        &[
            (10, CiState::Passing),
            (11, CiState::Failing),
            (12, CiState::Passing),
        ],
    );
    s.security.push(RepoSecurity {
        repo: "a/b".into(),
        read_at: crate::problems::stamp(now()),
        kinds: vec![Kind {
            source: Source::Dependabot,
            alerts: vec![alert(7, "qs", "package-lock.json", "< 6.10.3", "6.10.3")],
            ..Default::default()
        }],
        ..Default::default()
    });
    s.dependabot = vec![
        merge(&s, 10, Some(true), "clean"),
        merge(&s, 11, Some(true), "unstable"),
        merge(&s, 12, Some(false), "dirty"),
    ];
    let all = all(&s);
    let numbers: Vec<u64> = all.iter().map(|u| u.number).collect();
    assert_eq!(numbers, [12, 10, 11], "the security fix first");

    let fix = &all[0];
    assert_eq!(
        fix.fixes,
        [Fix {
            number: 7,
            severity: Severity::High,
            package: "qs".into(),
            name: "qs (high)".into(),
            url: "https://github.com/a/b/security/dependabot/7".into(),
        }]
    );
    assert!(fix.conflicts);
    assert!(!fix.ready);
    assert_eq!(fix.merge_text, "It conflicts with main.");
    assert_eq!(
        brief_text(fix),
        "Dependabot's fix for qs (high) in a/b#12 isn't ready yet. It conflicts with main."
    );

    let patch = &all[1];
    assert_eq!(patch.change, Some(Change::Patch));
    assert!(patch.fixes.is_empty());
    assert!(patch.ready);
    assert_eq!(
        (patch.checks_text.as_str(), patch.merge_text.as_str()),
        ("All 3 of its checks passed.", "GitHub says it can merge.")
    );

    let major = &all[2];
    assert_eq!(major.change, Some(Change::Major));
    assert_eq!(major.checks, CiState::Failing);
    assert_eq!(major.checks_text, "CI / test failed on 2222222.");
    assert!(!major.ready);

    // The fix rebased and passing is ready.
    s.dependabot[2] = merge(&s, 12, Some(true), "clean");
    let fix = &crate::dependabot::all(&s)[0];
    assert!(fix.ready);
    assert_eq!(
        brief_text(fix),
        "Dependabot's fix for qs (high) in a/b#12 is ready to merge."
    );
}

#[test]
fn what_orbit_hasnt_read_yet_is_said_so_and_never_ready() {
    let mut s = snapshot(
        vec![update(10, "Bump vite from 5.4.10 to 5.4.11", '1')],
        &[],
    );
    let u = &all(&s)[0];
    assert_eq!(
        u.checks_text,
        "Orbit hasn't read the checks on 1111111 yet."
    );
    assert_eq!(
        u.merge_text,
        "Orbit hasn't asked GitHub yet whether it merges."
    );
    assert!(!u.ready);
    // Checks on an older head don't count.
    s = snapshot(
        vec![update(10, "Bump vite from 5.4.10 to 5.4.11", '1')],
        &[(10, CiState::Passing)],
    );
    s.dependabot = vec![merge(&s, 10, Some(true), "clean")];
    s.prs[0].head_sha = "9".repeat(40);
    let u = &all(&s)[0];
    assert_eq!(
        u.checks_text,
        "Orbit hasn't read the checks on 9999999 yet."
    );
    assert_eq!(
        u.merge_text,
        "Orbit hasn't asked GitHub yet whether it merges."
    );
    assert!(!u.ready);

    // GitHub still at work, behind, blocked, and a failed question.
    let mut s = snapshot(
        vec![update(10, "Bump vite from 5.4.10 to 5.4.11", '1')],
        &[(10, CiState::Running)],
    );
    let says = |s: &mut Snapshot, mergeable: Option<bool>, state: &str| {
        s.dependabot = vec![merge(s, 10, mergeable, state)];
        all(s)[0].merge_text.clone()
    };
    assert_eq!(
        says(&mut s, None, "unknown"),
        "GitHub is still checking whether it merges."
    );
    assert_eq!(
        says(&mut s, Some(true), "behind"),
        "GitHub wants it brought up to date with main first."
    );
    assert_eq!(
        says(&mut s, Some(true), "blocked"),
        "GitHub says a rule blocks merging it, such as a required review."
    );
    assert_eq!(all(&s)[0].checks_text, "Its checks are still running.");
    s.dependabot[0].error = Some("Orbit couldn't ask GitHub whether it merges: Bad Gateway (HTTP 502). It tries again later.".into());
    s.dependabot[0].read_at.clear();
    let u = &all(&s)[0];
    assert_eq!(u.merge_text, s.dependabot[0].error.clone().unwrap());
    assert!(u.error.is_some());
}

/// A read of CI that failed keeps what Orbit read before, which may be
/// older than the sync: its base isn't trusted, and nothing is ready.
#[test]
fn after_a_failed_read_of_ci_its_base_isnt_trusted_and_nothing_is_ready() {
    let mut s = snapshot(
        vec![update(10, "Bump vite from 5.4.10 to 5.4.11", '1')],
        &[(10, CiState::Passing)],
    );
    // GitHub said it merges cleanly into main at A, the commit CI read.
    s.dependabot = vec![merge(&s, 10, Some(true), "clean")];
    assert!(all(&s)[0].ready);
    // The sync then saw main at B, but CI's next read failed.
    let b = "b".repeat(40);
    s.prs[0].base_sha = b.clone();
    s.ci[0].error = Some("Bad Gateway (HTTP 502)".into());
    let u = &all(&s)[0];
    assert!(!u.ready);
    assert_eq!(
        u.merge_text,
        "Orbit hasn't asked GitHub yet whether it merges."
    );
    assert_eq!(
        u.error.as_deref(),
        Some("Orbit's latest read of CI here failed, so its checks and base may be out of date.")
    );
    assert!(brief_text(u)
        .ends_with(" Orbit's latest question about it failed, so this may be out of date."));
    // So Orbit asks GitHub about B.
    assert_eq!(
        wanted(&s, now()),
        [("a/b".to_string(), 10, "1".repeat(40), b.clone())]
    );
    // Even GitHub's answer for B isn't ready while CI can't be read.
    s.dependabot = vec![Merge {
        base_sha: b,
        ..merge(&s, 10, Some(true), "clean")
    }];
    let u = &all(&s)[0];
    assert_eq!(u.merge_text, "GitHub says it can merge.");
    assert!(!u.ready);
    s.ci[0].error = None;
    assert!(
        !all(&s)[0].ready,
        "CI's main is A again, so B's answer isn't it"
    );
}

/// A group across several folders claims no fix, even for an alert in a
/// folder it doesn't touch.
#[test]
fn a_group_across_folders_claims_no_fix_for_an_untouched_folder() {
    let mut group = update(
        10,
        "Bump the npm_and_yarn group across 2 directories with 2 updates",
        '1',
    );
    group.body = "Bumps the npm_and_yarn group with 1 update in the /web directory: lodash.\nBumps the npm_and_yarn group with 1 update in the /api directory: lodash.\n\nUpdates `lodash` from 4.17.20 to 4.17.21".into();
    let mut s = snapshot(vec![group], &[(10, CiState::Passing)]);
    s.security.push(RepoSecurity {
        repo: "a/b".into(),
        read_at: crate::problems::stamp(now()),
        kinds: vec![Kind {
            source: Source::Dependabot,
            alerts: vec![alert(
                9,
                "lodash",
                "legacy/package-lock.json",
                "< 4.17.21",
                "4.17.21",
            )],
            ..Default::default()
        }],
        ..Default::default()
    });
    assert!(all(&s)[0].fixes.is_empty());
}

#[test]
fn only_dependabots_own_pull_requests_while_both_switches_are_on() {
    let mine = {
        let mut p = update(13, "Bump vite from 5.4.10 to 5.4.11", '4');
        p.author = "someone".into();
        p
    };
    let fork = {
        let mut p = update(14, "Bump vite from 5.4.10 to 5.4.11", '5');
        p.head_repo = "x/b".into();
        p
    };
    let other = {
        let mut p = update(15, "Bump vite from 5.4.10 to 5.4.11", '6');
        p.repo = "a/c".into();
        p.id = pr_id("a/c", 15);
        p.head_repo = "a/c".into();
        p
    };
    let mut s = snapshot(
        vec![
            update(10, "Bump vite from 5.4.10 to 5.4.11", '1'),
            mine,
            fork,
            other,
        ],
        &[],
    );
    let numbers = |s: &Snapshot| all(s).iter().map(|u| u.number).collect::<Vec<_>>();
    assert_eq!(numbers(&s), [10]);
    s.settings.watch_security = false;
    assert!(numbers(&s).is_empty());
    s.settings.watch_security = true;
    s.settings.watch_ci = false;
    assert!(numbers(&s).is_empty());
    assert!(wanted(&s, now()).is_empty());
}

#[test]
fn github_is_asked_once_per_head_and_base_and_again_after_a_wait() {
    let prs = (10..16)
        .map(|n| {
            update(
                n,
                "Bump vite from 5.4.10 to 5.4.11",
                char::from(b'0' + (n - 9) as u8),
            )
        })
        .collect();
    let mut s = snapshot(prs, &[]);
    // A few a pass, those never asked first.
    let asks = wanted(&s, now());
    assert_eq!(asks.len(), PER_PASS);
    assert_eq!(
        asks[0],
        ("a/b".into(), 10, "1".repeat(40), MAIN.to_string())
    );
    for (repo, number, head, base) in asks {
        let answer = Ok(Merge {
            pr_id: pr_id(&repo, number),
            head_sha: head.clone(),
            base_sha: base.clone(),
            read_at: crate::problems::stamp(now()),
            mergeable: Some(true),
            state: "clean".into(),
            ..Default::default()
        });
        record(&mut s, &repo, number, &head, &base, answer, now());
    }
    let left: Vec<u64> = wanted(&s, now()).iter().map(|a| a.1).collect();
    assert_eq!(left, [14, 15]);
    // A definite answer stands for a while.
    let later = now() + readiness::EVERY;
    assert_eq!(wanted(&s, later - Duration::minutes(1)).len(), 2);
    assert_eq!(wanted(&s, later).len(), PER_PASS);

    // Main moves: each is asked again.
    s.prs.truncate(2);
    assert!(prune(&mut s));
    s.ci[0].head.sha = "d".repeat(40);
    let asks = wanted(&s, now());
    assert_eq!(asks.len(), 2);
    assert_eq!(asks[0].3, "d".repeat(40));

    // GitHub still at work: asked again after a minute.
    let (repo, number, head, base) = asks[0].clone();
    let pending = Ok(Merge {
        pr_id: pr_id(&repo, number),
        head_sha: head.clone(),
        base_sha: base.clone(),
        read_at: crate::problems::stamp(now()),
        state: "unknown".into(),
        ..Default::default()
    });
    record(&mut s, &repo, number, &head, &base, pending, now());
    let asked_again = |s: &Snapshot, at| wanted(s, at).iter().any(|a| a.1 == number);
    assert!(!asked_again(&s, now()));
    assert!(asked_again(&s, now() + Duration::minutes(1)));

    // An answer about a head Orbit no longer shows isn't kept, and a pull
    // request that closed is forgotten.
    let gone = s.dependabot.len();
    record(
        &mut s,
        "a/b",
        11,
        &"f".repeat(40),
        &base,
        Err("x".into()),
        now(),
    );
    assert_eq!(s.dependabot.len(), gone);
    s.prs.retain(|p| p.number != 10);
    assert!(prune(&mut s));
    assert!(!s.dependabot.iter().any(|m| m.pr_id == pr_id("a/b", 10)));
    s.settings.watch_security = false;
    prune(&mut s);
    assert!(s.dependabot.is_empty());
}

/// The three updates, where only 12 conflicts.
fn three() -> Snapshot {
    let prs = vec![
        update(10, "Bump vite from 5.4.10 to 5.4.11", '1'),
        update(11, "Bump react from 18.3.1 to 19.0.0", '2'),
        update(12, "Bump qs from 6.10.1 to 6.10.3", '3'),
    ];
    let mut s = snapshot(
        prs,
        &[
            (10, CiState::Passing),
            (11, CiState::Failing),
            (12, CiState::Passing),
        ],
    );
    s.dependabot = vec![
        merge(&s, 10, Some(true), "clean"),
        merge(&s, 11, Some(true), "unstable"),
        merge(&s, 12, Some(false), "dirty"),
    ];
    s
}
fn shown(s: &Snapshot, number: u64) -> Update {
    all(s).into_iter().find(|u| u.number == number).unwrap()
}

#[test]
fn only_one_that_conflicts_and_isnt_asked_about_at_its_commit_may_be_rebased() {
    let mut s = three();
    let head = "3".repeat(40);
    assert_eq!(
        may_rebase(&s, "a/b#12"),
        Ok(("a/b".into(), 12, head.clone(), MAIN.into()))
    );
    let fix = shown(&s, 12);
    assert!(fix.can_rebase);
    assert_eq!(fix.rebase_text, None);
    for n in [10, 11] {
        assert!(!shown(&s, n).can_rebase);
        assert_eq!(
            may_rebase(&s, &pr_id("a/b", n)).unwrap_err(),
            "GitHub doesn't say it conflicts now, so Orbit won't ask Dependabot to rebase it."
        );
    }
    assert!(may_rebase(&s, "a/b#99")
        .unwrap_err()
        .starts_with("Orbit doesn't show this Dependabot pull request now"));

    // Recorded before it is sent: no second ask meanwhile.
    let id = record_rebase(&mut s.journal, "a/b", 12, &head, now());
    let w = s.journal.last().unwrap();
    assert_eq!(
        (w.action, w.state, w.number, w.head.as_deref()),
        (
            Action::AskRebase,
            WriteState::Sending,
            Some(12),
            Some(head.as_str())
        )
    );
    assert_eq!(w.thread, "dependabot:a/b#12");
    assert_eq!(
        may_rebase(&s, "a/b#12").unwrap_err(),
        "Orbit is already asking Dependabot to rebase it."
    );
    let fix = shown(&s, 12);
    assert!(!fix.can_rebase);
    assert_eq!(
        fix.rebase_text.as_deref(),
        Some("Orbit is asking Dependabot to rebase it.")
    );

    // Done: once for each commit.
    let url = "https://github.com/a/b/pull/12#issuecomment-1";
    settle_rebase(&mut s.journal, &id, Ok(url.into()), now());
    assert_eq!(s.journal[0].url, url);
    assert_eq!(
        may_rebase(&s, "a/b#12").unwrap_err(),
        "Orbit already asked Dependabot to rebase it at this commit."
    );
    assert_eq!(
        shown(&s, 12).rebase_text.as_deref(),
        Some("Orbit asked Dependabot to rebase it. Dependabot answers on GitHub, and may take a few minutes.")
    );

    // Dependabot rebased it, and it conflicts again: a new commit, a new ask.
    s.prs[2].head_sha = "4".repeat(40);
    s.ci[0].pulls[2].ci.sha = "4".repeat(40);
    s.dependabot[2] = merge(&s, 12, Some(false), "dirty");
    assert!(may_rebase(&s, "a/b#12").is_ok());
    let fix = shown(&s, 12);
    assert!(fix.can_rebase);
    assert_eq!(fix.rebase_text, None);

    // One that didn't go through may be asked again, and says why.
    let id = record_rebase(&mut s.journal, "a/b", 12, &"4".repeat(40), now());
    settle_rebase(
        &mut s.journal,
        &id,
        Err(rebase_error(
            "gh: Resource not accessible by integration (HTTP 403)",
        )),
        now(),
    );
    assert!(may_rebase(&s, "a/b#12").is_ok());
    let fix = shown(&s, 12);
    assert!(fix.can_rebase);
    assert_eq!(
        fix.rebase_text.as_deref(),
        Some("Orbit couldn't ask Dependabot to rebase it. GitHub didn't let Orbit's sign-in comment on this pull request.")
    );

    // Not while either switch is off.
    s.settings.watch_security = false;
    assert!(may_rebase(&s, "a/b#12").is_err());
}

#[test]
fn githubs_errors_to_an_ask_to_rebase_are_said_plainly() {
    assert_eq!(
        rebase_error("gh: API rate limit exceeded for user ID 1. (HTTP 403)"),
        "GitHub's rate limit held this back. Tap Ask to rebase again later."
    );
    assert_eq!(
        rebase_error("gh: Not Found (HTTP 404)"),
        "GitHub can't find this pull request now."
    );
    assert_eq!(
        rebase_error("gh: Must have admin rights (HTTP 403)"),
        "GitHub didn't let Orbit's sign-in comment on this pull request."
    );
    assert_eq!(
        rebase_error("gh: Bad Gateway (HTTP 502)"),
        "GitHub didn't take Orbit's comment: Bad Gateway (HTTP 502)."
    );
}

#[test]
fn an_ask_to_rebase_cut_off_is_never_sent_again_and_is_looked_for() {
    let mut s = three();
    let head = "3".repeat(40);
    let cut = record_rebase(&mut s.journal, "a/b", 12, &head, now());
    let done = record_rebase(&mut s.journal, "a/b", 10, &"1".repeat(40), now());
    settle_rebase(&mut s.journal, &done, Ok(String::new()), now());

    let later = now() + Duration::minutes(30);
    let found = interrupted(&mut s.journal, later);
    assert_eq!(
        found,
        [(
            cut.clone(),
            "a/b".into(),
            12,
            "2026-11-02T11:55:00.000Z".into()
        )],
        "looked for from a little before it was recorded"
    );
    let w = &s.journal[0];
    assert_eq!(w.state, WriteState::Failed);
    assert_eq!(
        w.error.as_deref(),
        Some("Orbit stopped before GitHub answered, so it didn't ask again. It looks on GitHub for its comment.")
    );
    assert_eq!(s.journal[1].state, WriteState::Done, "the done one stays");
    assert!(interrupted(&mut s.journal, later).is_empty(), "only once");

    // Only Orbit's sign-in's comment that says the command alone counts.
    let comment = |login: &str, body: &str, id: u64| {
        serde_json::json!({
            "user": { "login": login },
            "body": body,
            "html_url": format!("https://github.com/a/b/pull/12#issuecomment-{id}"),
        })
    };
    let comments = vec![
        comment("someone", "@dependabot rebase", 1),
        comment("me", "Could you @dependabot rebase this?", 2),
        comment("ME", "@dependabot rebase\n", 3),
    ];
    assert_eq!(
        find_rebase(&comments, "me").as_deref(),
        Some("https://github.com/a/b/pull/12#issuecomment-3")
    );
    assert_eq!(find_rebase(&comments[..2], "me"), None);
    assert_eq!(find_rebase(&comments, ""), None);

    // Found: it got there.
    let mut journal = s.journal.clone();
    found_rebase(&mut journal, &cut, Ok(find_rebase(&comments, "me")), later);
    assert_eq!(journal[0].state, WriteState::Done);
    assert_eq!(journal[0].error, None);
    assert!(may_rebase(
        &{
            let mut s = s.clone();
            s.journal = journal;
            s
        },
        "a/b#12"
    )
    .is_err());

    // Not found, or the look failed: not gone through, to tap again.
    let mut journal = s.journal.clone();
    found_rebase(&mut journal, &cut, Ok(None), later);
    assert_eq!(journal[0].state, WriteState::Failed);
    assert_eq!(
        journal[0].error.as_deref(),
        Some("Orbit stopped before GitHub answered, and didn't find its comment there. Tap Ask to rebase again if it still conflicts.")
    );
    found_rebase(
        &mut s.journal,
        &cut,
        Err("gh: Bad Gateway (HTTP 502)".into()),
        later,
    );
    assert_eq!(
        s.journal[0].error.as_deref(),
        Some("Orbit stopped before GitHub answered, and couldn't look for its comment there: Bad Gateway (HTTP 502). Tap Ask to rebase again if it still conflicts.")
    );
    assert!(may_rebase(&s, "a/b#12").is_ok());
    assert!(shown(&s, 12).can_rebase);
}

#[test]
fn a_cut_off_write_of_another_kind_isnt_dependabots_to_settle() {
    let mut journal = vec![];
    let id = record_rebase(&mut journal, "a/b", 12, &"3".repeat(40), now());
    let mut threads = vec![];
    assert!(!crate::ops::interrupted(&mut journal, &mut threads, now()));
    assert_eq!(journal[0].state, WriteState::Sending);
    assert_eq!(interrupted(&mut journal, now()).len(), 1);
    assert_eq!(journal[0].id, id);
}
