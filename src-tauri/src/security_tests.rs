use super::*;
use serde_json::json;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-01T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
fn ago(minutes: i64) -> String {
    stamp(now() - Duration::minutes(minutes))
}
fn read(repo: &str, minutes: i64) -> RepoSecurity {
    RepoSecurity {
        repo: repo.into(),
        read_at: ago(minutes),
        ..Default::default()
    }
}
fn repos(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}
/// A Dependabot alert as GitHub lists it.
pub(crate) fn dependabot(number: u64, package: &str, severity: &str) -> Value {
    json!({
        "number": number,
        "state": "open",
        "dependency": {
            "package": { "ecosystem": "npm", "name": package },
            "manifest_path": "package-lock.json",
            "scope": "runtime"
        },
        "security_advisory": {
            "ghsa_id": "GHSA-xxxx-yyyy-zzzz",
            "summary": format!("Prototype pollution in {package}"),
            "severity": severity
        },
        "security_vulnerability": {
            "package": { "ecosystem": "npm", "name": package },
            "severity": severity,
            "vulnerable_version_range": "< 4.17.21",
            "first_patched_version": { "identifier": "4.17.21" }
        },
        "html_url": format!("https://github.com/a/b/security/dependabot/{number}"),
        "created_at": "2026-10-30T10:00:00Z"
    })
}
/// A code scanning alert as GitHub lists it.
pub(crate) fn code_scanning(number: u64, severity: Option<&str>) -> Value {
    json!({
        "number": number,
        "state": "open",
        "rule": {
            "id": "js/sql-injection",
            "severity": "error",
            "security_severity_level": severity,
            "description": "Database query built from user-controlled sources",
            "name": "js/sql-injection"
        },
        "tool": { "name": "CodeQL" },
        "most_recent_instance": {
            "ref": "refs/heads/main",
            "location": { "path": "src/db.ts", "start_line": 12 },
            "message": { "text": "This query depends on a user-provided value." }
        },
        "html_url": format!("https://github.com/a/b/security/code-scanning/{number}"),
        "created_at": "2026-10-31T10:00:00Z"
    })
}
/// A secret scanning alert as GitHub lists it, with the secret's value as
/// GitHub sends it when it isn't asked to hide it.
pub(crate) fn secret(number: u64) -> Value {
    json!({
        "number": number,
        "state": "open",
        "secret_type": "github_personal_access_token",
        "secret_type_display_name": "GitHub Personal Access Token",
        "secret": "ghp_THISISTHESECRETVALUE0123456789abcd",
        "validity": "active",
        "push_protection_bypassed": true,
        "first_location_detected": { "path": "src/config.ts", "start_line": 4 },
        "html_url": format!("https://github.com/a/b/security/secret-scanning/{number}"),
        "created_at": "2026-11-01T09:00:00Z"
    })
}

#[test]
fn repositories_are_read_never_read_first_then_the_oldest_every_half_hour() {
    let all = repos(&["a/new", "a/old", "a/fresh", "a/older", "bad name"]);
    let known = [read("a/old", 40), read("A/Fresh", 29), read("a/older", 90)];
    assert_eq!(wanted(&all, &known, now()), ["a/new", "a/older"]);
    let many = repos(&["a/1", "a/2", "a/3", "a/4", "a/5"]);
    assert_eq!(wanted(&many, &[], now()).len(), PER_PASS);
    // One that failed waits for its retry, however long ago it was read.
    let mut failed = read("a/old", 90);
    failed.error = Some("down".into());
    failed.retry_at = Some(stamp(now() + Duration::minutes(2)));
    assert!(wanted(&repos(&["a/old"]), &[failed.clone()], now()).is_empty());
    failed.retry_at = Some(ago(1));
    assert_eq!(wanted(&repos(&["a/old"]), &[failed], now()), ["a/old"]);
}

#[test]
fn a_dependabot_alert_names_its_package_file_and_the_version_that_fixes_it() {
    let a = alert(Source::Dependabot, &dependabot(12, "lodash", "critical")).unwrap();
    assert_eq!(a.number, 12);
    assert_eq!(a.severity, Severity::Critical);
    assert_eq!(a.subject, "lodash");
    assert_eq!(a.title, "Prototype pollution in lodash");
    assert_eq!(a.path, "package-lock.json");
    assert_eq!(a.fixed_in, "4.17.21");
    assert_eq!(a.url, "https://github.com/a/b/security/dependabot/12");
    assert!(a.needs_you());
    assert_eq!(a.name(), "lodash (critical)");
    assert_eq!(a.label(), "Dependabot alert 12, lodash");
    // GitHub's pages call medium "moderate"; neither needs you.
    let a = alert(Source::Dependabot, &dependabot(3, "minimist", "moderate")).unwrap();
    assert_eq!(a.severity, Severity::Medium);
    assert!(!a.needs_you());
    // Without a number it isn't an alert.
    assert!(alert(Source::Dependabot, &json!({"state": "open"})).is_none());
}

#[test]
fn a_code_scanning_alert_takes_its_security_severity_or_counts_by_its_rule() {
    let a = alert(Source::CodeScanning, &code_scanning(5, Some("high"))).unwrap();
    assert_eq!(a.severity, Severity::High);
    assert_eq!(a.subject, "js/sql-injection");
    assert_eq!(a.place(), "src/db.ts line 12");
    assert_eq!(
        a.name(),
        "Database query built from user-controlled sources in src/db.ts line 12 (high)"
    );
    assert!(a.needs_you());
    // A rule that isn't about security: an error is medium, else low.
    let a = alert(Source::CodeScanning, &code_scanning(6, None)).unwrap();
    assert_eq!(a.severity, Severity::Medium);
    let mut v = code_scanning(7, None);
    v["rule"]["severity"] = json!("note");
    assert_eq!(
        alert(Source::CodeScanning, &v).unwrap().severity,
        Severity::Low
    );
}

#[test]
fn a_secrets_value_is_never_kept_even_when_github_sends_it() {
    let a = alert(Source::SecretScanning, &secret(3)).unwrap();
    assert_eq!(a.title, "GitHub Personal Access Token");
    assert_eq!(a.subject, "github_personal_access_token");
    assert_eq!(a.place(), "src/config.ts line 4");
    assert_eq!(a.validity, "active");
    assert!(a.bypassed);
    assert!(a.needs_you(), "every secret needs you");
    assert_eq!(
        a.name(),
        "a GitHub Personal Access Token in src/config.ts line 4"
    );
    let kept = serde_json::to_string(&a).unwrap();
    assert!(!kept.contains("THISISTHESECRETVALUE"), "{kept}");
    let kind = kind("a/b", Source::SecretScanning, Ok(json!([secret(3)]))).unwrap();
    assert!(!serde_json::to_string(&kind)
        .unwrap()
        .contains("THISISTHESECRETVALUE"));
    // Orbit asks GitHub to hide it, for a list and for one alert.
    for list in [true, false] {
        assert!(fields(Source::SecretScanning, list).contains(&("hide_secret", "true".into())));
    }
    assert!(!fields(Source::Dependabot, true)
        .iter()
        .any(|(k, _)| *k == "hide_secret"));
    let article = alert(
        Source::SecretScanning,
        &json!({"number": 1, "secret_type_display_name": "AWS Access Key ID"}),
    )
    .unwrap();
    assert_eq!(article.name(), "an AWS Access Key ID");
}

#[test]
fn text_from_github_stays_on_one_line_and_only_links_to_github_are_kept() {
    let mut v = dependabot(1, "left\u{7}\npad", "low");
    v["security_advisory"]["summary"] = json!(format!("{}\n\tend", "x".repeat(300)));
    v["html_url"] = json!("https://evil.example/a");
    let a = alert(Source::Dependabot, &v).unwrap();
    assert_eq!(a.subject, "left pad");
    assert!(!a.title.contains('\n'));
    assert_eq!(
        a.title.chars().count(),
        MAX_TEXT + 1,
        "cut with an ellipsis"
    );
    assert_eq!(a.url, "");
}

#[test]
fn a_kind_off_for_the_repository_or_hidden_from_orbit_is_said_so() {
    let refused = |text: &str| Err(format!("gh: {text}\n"));
    let k = kind(
        "a/b",
        Source::Dependabot,
        refused("Dependabot alerts are disabled for this repository. (HTTP 403)"),
    )
    .unwrap();
    assert_eq!(
        k.unseen.as_deref(),
        Some("Dependabot alerts are off for a/b.")
    );
    assert!(k.off);
    for text in [
        "no analysis found (HTTP 404)",
        "Advanced Security must be enabled for this repository to use code scanning. (HTTP 403)",
    ] {
        let k = kind("a/b", Source::CodeScanning, refused(text)).unwrap();
        assert_eq!(k.unseen.as_deref(), Some("No code scanning runs on a/b."));
        assert!(k.off);
    }
    let k = kind(
        "a/b",
        Source::SecretScanning,
        refused("Secret scanning is disabled on this repository. (HTTP 404)"),
    )
    .unwrap();
    assert_eq!(k.unseen.as_deref(), Some("Secret scanning is off for a/b."));
    let k = kind(
        "a/b",
        Source::SecretScanning,
        refused("Resource not accessible by integration (HTTP 403)"),
    )
    .unwrap();
    assert_eq!(
        k.unseen.as_deref(),
        Some("GitHub doesn't show a/b's secret scanning alerts to Orbit's sign-in. It shows them to admins and security managers.")
    );
    assert!(!k.off);
    // A rate limit or GitHub failing is tried again, not taken as hidden.
    for text in [
        "API rate limit exceeded for user ID 1. (HTTP 403)",
        "Bad Gateway (HTTP 502)",
    ] {
        assert!(kind("a/b", Source::Dependabot, refused(text)).is_err());
    }
    assert!(kind("a/b", Source::Dependabot, Ok(json!({"message": "?"}))).is_err());
}

#[test]
fn a_list_keeps_open_alerts_the_most_severe_first_and_says_when_theres_more() {
    let mut closed = dependabot(9, "old", "critical");
    closed["state"] = json!("fixed");
    let list = json!([
        dependabot(1, "a", "low"),
        dependabot(2, "b", "high"),
        closed,
        dependabot(3, "c", "high")
    ]);
    let k = kind("a/b", Source::Dependabot, Ok(list)).unwrap();
    let numbers: Vec<u64> = k.alerts.iter().map(|a| a.number).collect();
    assert_eq!(numbers, [3, 2, 1]);
    assert!(!k.more);
    let full: Vec<Value> = (1..=PAGE as u64)
        .map(|n| dependabot(n, "x", "low"))
        .collect();
    assert!(
        kind("a/b", Source::Dependabot, Ok(json!(full)))
            .unwrap()
            .more
    );
}

#[test]
fn what_became_of_an_alert_is_said_in_plain_words() {
    let a = alert(Source::Dependabot, &dependabot(12, "lodash", "critical")).unwrap();
    let fixed = outcome(Source::Dependabot, &json!({"state": "fixed"}));
    assert_eq!(
        outcome_text(&a, &fixed),
        "Dependabot alert 12, lodash: fixed, GitHub says."
    );
    let dismissed = outcome(
        Source::Dependabot,
        &json!({"state": "dismissed", "dismissed_by": {"login": "sam"}, "dismissed_reason": "tolerable_risk"}),
    );
    assert_eq!(
        dismissed,
        Outcome::Closed {
            fixed: false,
            by: "sam".into(),
            why: "tolerable risk".into()
        }
    );
    assert_eq!(
        outcome_text(&a, &dismissed),
        "Dependabot alert 12, lodash: dismissed by @sam as tolerable risk."
    );
    let auto = outcome(Source::Dependabot, &json!({"state": "auto_dismissed"}));
    assert_eq!(
        outcome_text(&a, &auto),
        "Dependabot alert 12, lodash: dismissed by a Dependabot rule."
    );
    assert_eq!(
        outcome(Source::Dependabot, &json!({"state": "open"})),
        Outcome::Open
    );
    assert_eq!(
        outcome_text(&a, &Outcome::Gone),
        "Dependabot alert 12, lodash: GitHub no longer has it."
    );

    let c = alert(Source::CodeScanning, &code_scanning(5, Some("high"))).unwrap();
    let wont = outcome(
        Source::CodeScanning,
        &json!({"state": "dismissed", "dismissed_by": {"login": "x\"y"}, "dismissed_reason": "won't fix"}),
    );
    assert_eq!(
        outcome_text(&c, &wont),
        "Code scanning alert 5, js/sql-injection: dismissed as won't fix.",
        "a login GitHub wouldn't give is left out"
    );

    let s = alert(Source::SecretScanning, &secret(3)).unwrap();
    let revoked = outcome(
        Source::SecretScanning,
        &json!({"state": "resolved", "resolution": "revoked", "resolved_by": {"login": "sam"}}),
    );
    assert_eq!(
        outcome_text(&s, &revoked),
        "Secret scanning alert 3, GitHub Personal Access Token: closed as revoked by @sam."
    );
    let false_positive = outcome(
        Source::SecretScanning,
        &json!({"state": "resolved", "resolution": "false_positive"}),
    );
    assert_eq!(
        outcome_text(&s, &false_positive),
        "Secret scanning alert 3, GitHub Personal Access Token: dismissed as false positive."
    );
}

#[test]
fn alerts_that_need_you_are_named_in_a_sentence() {
    let alerts: Vec<Alert> = [
        dependabot(1, "lodash", "critical"),
        dependabot(2, "minimist", "high"),
        dependabot(3, "qs", "high"),
        dependabot(4, "left-pad", "low"),
    ]
    .iter()
    .filter_map(|v| alert(Source::Dependabot, v))
    .collect();
    assert_eq!(
        needs_you_text("a/b", Source::Dependabot, &alerts).unwrap(),
        "a/b has 3 high or critical Dependabot alerts: lodash (critical), minimist (high) and 1 more."
    );
    assert_eq!(
        needs_you_text("a/b", Source::Dependabot, &alerts[..1]).unwrap(),
        "a/b has a high or critical Dependabot alert: lodash (critical)."
    );
    assert!(needs_you_text("a/b", Source::Dependabot, &alerts[3..]).is_none());
    assert_eq!(
        open_text(Source::Dependabot, &alerts[2..]),
        "2 open Dependabot alerts: qs (high) and left-pad (low)."
    );
    let secrets: Vec<Alert> = [secret(3), secret(4)]
        .iter()
        .filter_map(|v| alert(Source::SecretScanning, v))
        .collect();
    assert_eq!(
        needs_you_text("a/b", Source::SecretScanning, &secrets[..1]).unwrap(),
        "GitHub found a secret in a/b: a GitHub Personal Access Token in src/config.ts line 4. Revoke it, then close its alert on GitHub."
    );
    assert!(needs_you_text("a/b", Source::SecretScanning, &secrets)
        .unwrap()
        .starts_with("GitHub found 2 secrets in a/b:"));
}

#[test]
fn a_failed_read_keeps_what_was_read_and_waits_longer_each_time() {
    let mut all = vec![];
    let mut first = read("a/b", 0);
    first.kinds.push(Kind {
        source: Source::Dependabot,
        alerts: vec![alert(Source::Dependabot, &dependabot(1, "x", "low")).unwrap()],
        ..Default::default()
    });
    record(&mut all, "a/b", Ok(first.clone()), now());
    assert_eq!(all, [first.clone()]);
    record(
        &mut all,
        "a/b",
        Err("gh: Bad Gateway (HTTP 502)".into()),
        now(),
    );
    assert_eq!(all[0].kinds, first.kinds);
    assert_eq!(all[0].tries, 1);
    assert_eq!(
        all[0].error.as_deref(),
        Some("Orbit couldn't read the security alerts yet: Bad Gateway (HTTP 502). It tries again later.")
    );
    assert_eq!(
        all[0].retry_at.as_deref(),
        Some(stamp(now() + Duration::minutes(1)).as_str())
    );
    record(&mut all, "a/b", Err("down".into()), now());
    assert_eq!(
        all[0].retry_at.as_deref(),
        Some(stamp(now() + Duration::minutes(2)).as_str())
    );
    record(&mut all, "a/b", Ok(first.clone()), now());
    assert_eq!(all[0].error, None);
    assert_eq!(all[0].tries, 0);
    prune(&mut all, &repos(&["a/c"]));
    assert!(all.is_empty());
}

#[test]
fn alerts_that_left_their_list_are_looked_up_only_for_kinds_orbit_sees() {
    use crate::problems::{security_key, Problem, ProblemKind};
    let listed = |numbers: &[u64]| -> Vec<Alert> {
        numbers
            .iter()
            .map(|&n| alert(Source::Dependabot, &dependabot(n, "x", "low")).unwrap())
            .collect()
    };
    let problem = Problem {
        key: security_key("a/b", Source::Dependabot),
        kind: ProblemKind::Security,
        repo: "a/b".into(),
        source: Some(Source::Dependabot),
        security_alerts: listed(&[1, 2, 3]),
        ..Default::default()
    };
    let mut now_read = read("a/b", 0);
    now_read.kinds = vec![Kind {
        source: Source::Dependabot,
        alerts: listed(&[2]),
        ..Default::default()
    }];
    assert_eq!(
        to_look_up(std::slice::from_ref(&problem), &now_read),
        [(Source::Dependabot, 1), (Source::Dependabot, 3)]
    );
    now_read.kinds[0].unseen = Some("off".into());
    assert!(to_look_up(&[problem], &now_read).is_empty());
}

/// Only a repository GitHub says is private counts as one: an internal
/// one, or one GitHub didn't say, counts as public; a rate limit is tried
/// again.
#[test]
fn only_a_repository_github_says_is_private_counts_as_one() {
    assert_eq!(
        private(Ok(json!({"private": true, "visibility": "private"}))),
        Ok(true)
    );
    assert_eq!(private(Ok(json!({"private": true}))), Ok(true));
    assert_eq!(
        private(Ok(json!({"private": true, "visibility": "internal"}))),
        Ok(false)
    );
    assert_eq!(
        private(Ok(json!({"private": false, "visibility": "public"}))),
        Ok(false)
    );
    assert_eq!(private(Ok(json!([]))), Ok(false));
    // Not knowing fails the read, so it is never taken as either.
    assert!(private(Err("gh: Not Found (HTTP 404)".into())).is_err());
    assert!(private(Err("gh: Bad Gateway (HTTP 502)".into())).is_err());
    assert!(private(Err(
        "gh: API rate limit exceeded for user ID 1. (HTTP 403)".into()
    ))
    .is_err());
}
