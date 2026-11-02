use super::*;
use serde_json::json;

fn read_at(found: &[(Setting, Option<bool>)]) -> RepoSettings {
    RepoSettings {
        repo: "acme/api".into(),
        read_at: "2026-11-02T00:00:00.000Z".into(),
        found: found
            .iter()
            .map(|&(setting, on)| Found { setting, on })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn orbits_baseline_leaves_push_protection_out_and_an_owner_can_choose() {
    use Setting::*;
    assert_eq!(
        wanted_for(&[], "acme"),
        [
            SecretScanning,
            DependabotAlerts,
            DependabotUpdates,
            PrivateReporting,
            ReadOnlyToken,
            NoActionsApproval,
            ForkApproval
        ]
    );
    let chosen = [Baseline {
        owner: "Acme".into(),
        settings: vec![ReadOnlyToken, PushProtection, ReadOnlyToken],
    }];
    // In the usual order, once each, whatever the case of the name.
    assert_eq!(wanted_for(&chosen, "acme"), [PushProtection, ReadOnlyToken]);
    assert_eq!(wanted_for(&chosen, "other").len(), 7);
    // An empty one asks for nothing.
    let none = [Baseline {
        owner: "acme".into(),
        settings: vec![],
    }];
    assert!(wanted_for(&none, "acme").is_empty());
    assert_eq!(owner("acme/api"), "acme");
}

#[test]
fn a_baseline_is_for_a_real_owner_once() {
    let b = |owner: &str| Baseline {
        owner: owner.into(),
        settings: vec![],
    };
    assert!(validate(&[b("acme"), b("my-name-2")]).is_ok());
    for bad in ["", "-acme", "acme/api", "a b", "me[bot]", &"a".repeat(40)] {
        assert!(validate(&[b(bad)]).is_err(), "{bad}");
    }
    assert_eq!(
        validate(&[b("acme"), b("ACME")]).unwrap_err(),
        "ACME has more than one baseline."
    );
}

#[test]
fn a_setting_is_on_off_or_unknown() {
    let enabled = |v: &Value| v["enabled"].as_bool();
    let read = |answer: Result<Value, String>, missing| setting(answer, missing, enabled);
    assert_eq!(read(Ok(json!({"enabled": true})), None), Ok(Some(true)));
    assert_eq!(read(Ok(json!({"enabled": false})), None), Ok(Some(false)));
    assert_eq!(read(Ok(json!({})), None), Ok(None));
    // A 404 is off only where GitHub says it means that.
    let not_found = || Err("gh: Not Found (HTTP 404)".to_string());
    assert_eq!(read(not_found(), Some(false)), Ok(Some(false)));
    assert_eq!(read(not_found(), None), Ok(None));
    // GitHub refusing, or not offering it here, leaves it unknown.
    for e in [
        "gh: Forbidden (HTTP 403)",
        "gh: Validation Failed (HTTP 422)",
    ] {
        assert_eq!(read(Err(e.into()), Some(false)), Ok(None), "{e}");
    }
    // Anything else fails the read, to try again later.
    for e in [
        "gh: API rate limit exceeded (HTTP 403)",
        "gh: Bad Gateway (HTTP 502)",
    ] {
        assert_eq!(read(Err(e.into()), Some(false)), Err(e.into()), "{e}");
    }
}

#[test]
fn a_repository_drifts_where_the_baseline_wants_what_github_has_off() {
    use Setting::*;
    let mut read = read_at(&[
        (SecretScanning, None),
        (PushProtection, Some(false)),
        (DependabotAlerts, Some(false)),
        (DependabotUpdates, Some(true)),
        (PrivateReporting, Some(false)),
        (ReadOnlyToken, Some(false)),
        (NoActionsApproval, Some(true)),
        (ForkApproval, None),
    ]);
    let wanted = wanted_for(&[], "acme");
    // Push protection is off, but Orbit's baseline doesn't ask for it.
    assert_eq!(
        drift(&read, &wanted),
        [DependabotAlerts, PrivateReporting, ReadOnlyToken]
    );
    assert_eq!(unknown(&read, &wanted), [SecretScanning, ForkApproval]);
    assert_eq!(
        drift_text("acme/api", &drift(&read, &wanted)),
        "acme/api drifts from your security settings: Dependabot alerts are off. Private vulnerability reporting is off. The Actions token can write by default."
    );
    // A private repository doesn't have the public ones.
    read.private = true;
    assert_eq!(drift(&read, &wanted), [DependabotAlerts, ReadOnlyToken]);
    assert_eq!(unknown(&read, &wanted), [SecretScanning]);
    // Nothing drifts that Orbit can't see, or hasn't read.
    read.unseen = Some(unseen_text("acme/api"));
    assert!(drift(&read, &wanted).is_empty() && unknown(&read, &wanted).is_empty());
    assert!(drift(&RepoSettings::default(), &wanted).is_empty());
}

#[test]
fn repositories_are_read_never_read_first_then_the_oldest_each_hour() {
    let now = time("2026-11-02T12:00:00.000Z").unwrap();
    let at = |repo: &str, read_at: &str| RepoSettings {
        repo: repo.into(),
        read_at: read_at.into(),
        ..Default::default()
    };
    let repos: Vec<String> = ["a/new", "a/old", "a/older", "a/fresh"]
        .map(String::from)
        .into();
    let read = vec![
        at("a/old", "2026-11-02T10:00:00.000Z"),
        at("a/older", "2026-11-02T09:00:00.000Z"),
        at("a/fresh", "2026-11-02T11:30:00.000Z"),
    ];
    assert_eq!(wanted(&repos, &read, now), ["a/new", "a/older"]);
    // A failed read waits its turn.
    let mut failed = at("a/new", "");
    failed.error = Some("no".into());
    failed.retry_at = Some("2026-11-02T12:05:00.000Z".into());
    let read = [read, vec![failed]].concat();
    assert_eq!(wanted(&repos, &read, now), ["a/older", "a/old"]);
}

#[test]
fn a_failed_read_keeps_what_was_read_and_waits_longer_each_time() {
    let now = time("2026-11-02T12:00:00.000Z").unwrap();
    let mut read = vec![];
    let first = read_at(&[(Setting::ReadOnlyToken, Some(false))]);
    record(&mut read, "acme/api", Ok(first.clone()), now);
    for (tries, wait) in [(1, 1), (2, 2), (3, 4)] {
        record(
            &mut read,
            "ACME/api",
            Err("gh: Bad Gateway (HTTP 502)".into()),
            now,
        );
        assert_eq!(read.len(), 1);
        let r = &read[0];
        assert_eq!((r.tries, r.found.clone()), (tries, first.found.clone()));
        assert_eq!(
            r.retry_at.as_deref().and_then(time),
            Some(now + Duration::minutes(wait))
        );
        assert!(r
            .error
            .as_deref()
            .unwrap()
            .starts_with("Orbit couldn't read the security settings yet:"));
    }
    record(&mut read, "acme/api", Ok(first.clone()), now);
    assert_eq!(read, [first]);
    prune(&mut read, &["acme/web".into()]);
    assert!(read.is_empty());
}

/// GitHub as `gh_fixture.cjs` fakes it, in `dir`.
#[cfg(unix)]
fn fake_gh(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let gh = dir.join("gh");
    let script = format!(
        "#!/bin/sh\nexec node {} {} \"$@\"\n",
        concat!(env!("CARGO_MANIFEST_DIR"), "/src/gh_fixture.cjs"),
        dir.display()
    );
    std::fs::write(&gh, script).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    gh
}
#[cfg(unix)]
fn calls(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// An admin's read of a public repository asks for each setting; a
/// private one's leaves out those only public repositories have, and
/// someone who isn't an admin sees none.
#[cfg(unix)]
#[tokio::test]
async fn reads_each_setting_as_github_has_it() {
    use Setting::*;
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let repo = |private: bool, admin: bool| {
        let visibility = if private { "private" } else { "public" };
        std::fs::write(
            d.path().join("repo.json"),
            json!({
                "full_name": "a/b",
                "private": private,
                "visibility": visibility,
                "permissions": {"admin": admin, "push": true},
                "security_and_analysis": {
                    "secret_scanning": {"status": "enabled"},
                    "secret_scanning_push_protection": {"status": "disabled"},
                    "dependabot_security_updates": {"status": "enabled"},
                },
            })
            .to_string(),
        )
        .unwrap();
        let _ = std::fs::remove_file(d.path().join("calls"));
    };
    std::fs::write(
        d.path().join("settings.json"),
        json!({
            "vulnerability-alerts": false,
            "automated-security-fixes": {"enabled": true, "paused": true},
            "actions/permissions/workflow": {
                "default_workflow_permissions": "write",
                "can_approve_pull_request_reviews": true,
            },
            "actions/permissions/fork-pr-contributor-approval": {
                "approval_policy": "first_time_contributors_new_to_github",
            },
        })
        .to_string(),
    )
    .unwrap();
    let ons = |r: &RepoSettings| -> Vec<(Setting, Option<bool>)> {
        r.found.iter().map(|f| (f.setting, f.on)).collect()
    };

    repo(false, true);
    let r = crate::github::GH
        .scope(gh.clone(), read("a/b"))
        .await
        .unwrap();
    assert!(!r.private && r.unseen.is_none() && !r.read_at.is_empty());
    assert_eq!(
        ons(&r),
        [
            (SecretScanning, Some(true)),
            (PushProtection, Some(false)),
            (DependabotAlerts, Some(false)),
            // Paused is still on.
            (DependabotUpdates, Some(true)),
            (PrivateReporting, Some(true)),
            (ReadOnlyToken, Some(false)),
            (NoActionsApproval, Some(false)),
            (ForkApproval, Some(false)),
        ]
    );
    assert_eq!(
        calls(d.path()),
        [
            "GET repos/a/b",
            "GET repos/a/b/vulnerability-alerts",
            "GET repos/a/b/automated-security-fixes",
            "GET repos/a/b/private-vulnerability-reporting",
            "GET repos/a/b/actions/permissions/workflow",
            "GET repos/a/b/actions/permissions/fork-pr-contributor-approval",
        ]
    );
    assert_eq!(
        drift(&r, &wanted_for(&[], "a")),
        [
            DependabotAlerts,
            ReadOnlyToken,
            NoActionsApproval,
            ForkApproval
        ]
    );

    // Private: the public ones aren't asked.
    repo(true, true);
    let r = crate::github::GH
        .scope(gh.clone(), read("a/b"))
        .await
        .unwrap();
    assert!(r.private);
    assert_eq!(r.found.len(), 6);
    assert!(!calls(d.path())
        .iter()
        .any(|c| c.contains("private-vulnerability-reporting") || c.contains("fork-pr")));

    // Not an admin: nothing more is asked.
    repo(false, false);
    let r = crate::github::GH
        .scope(gh.clone(), read("a/b"))
        .await
        .unwrap();
    assert_eq!(
        r.unseen.as_deref(),
        Some("GitHub shows a/b's security settings only to its admins, and Orbit's sign-in isn't one.")
    );
    assert!(r.found.is_empty());
    assert_eq!(calls(d.path()), ["GET repos/a/b"]);

    // A setting GitHub won't give fails the read.
    repo(false, true);
    std::fs::write(
        d.path().join("settings-error"),
        "gh: Bad Gateway (HTTP 502)\n",
    )
    .unwrap();
    let e = crate::github::GH
        .scope(gh.clone(), read("a/b"))
        .await
        .unwrap_err();
    assert!(e.contains("HTTP 502"), "{e}");
}

#[test]
fn the_app_knows_the_same_settings() {
    let fixture: Vec<Value> =
        serde_json::from_str(include_str!("../../test-fixtures/security-settings.json")).unwrap();
    let ours: Vec<Value> = Setting::ALL
        .iter()
        .map(|&s| {
            json!({
                "key": s.key(),
                "name": s.name(),
                "publicOnly": s.public_only(),
                "orbit": Setting::ORBIT.contains(&s),
            })
        })
        .collect();
    assert_eq!(fixture, ours);
    for s in Setting::ALL {
        assert_eq!(serde_json::to_value(s).unwrap(), s.key());
    }
}
