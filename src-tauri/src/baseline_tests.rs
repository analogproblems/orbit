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

    // Internal, which only its enterprise sees: not public either.
    let mut about: Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("repo.json")).unwrap())
            .unwrap();
    about["private"] = json!(false);
    about["visibility"] = json!("internal");
    std::fs::write(d.path().join("repo.json"), about.to_string()).unwrap();
    let _ = std::fs::remove_file(d.path().join("calls"));
    let r = crate::github::GH
        .scope(gh.clone(), read("a/b"))
        .await
        .unwrap();
    assert!(r.private);
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

fn change_entry(state: WriteState, action: Action, setting: Option<Setting>) -> Write {
    Write {
        id: format!("w{}", setting.map_or("", |s| s.key())),
        thread: "settings:acme/api".into(),
        action,
        repo: "acme/api".into(),
        number: None,
        state,
        at: "2026-11-02T00:00:00.000Z".into(),
        done_at: None,
        url: String::new(),
        error: None,
        setting,
    }
}

/// A tap on Fix is noted once, only for a setting Orbit last read off
/// where the baseline wants it on.
#[test]
fn a_tap_asks_for_a_setting_that_drifts_once() {
    use Setting::*;
    let mut read = vec![read_at(&[
        (SecretScanning, Some(true)),
        (DependabotAlerts, Some(false)),
        (PushProtection, Some(false)),
        (ReadOnlyToken, None),
    ])];
    assert_eq!(
        ask(&mut [], &[], &[], "acme/api", DependabotAlerts).unwrap_err(),
        "Orbit hasn't read acme/api's security settings yet."
    );
    for (setting, why) in [
        (SecretScanning, "Secret scanning is already on in acme/api."),
        (
            PushProtection,
            "acme/api's baseline doesn't ask for Push protection for secrets.",
        ),
        (
            ReadOnlyToken,
            "Orbit can't tell whether Read-only Actions token is on in acme/api, so it changes nothing.",
        ),
    ] {
        assert_eq!(
            ask(&mut read, &[], &[], "acme/api", setting).unwrap_err(),
            why
        );
    }
    // Whatever the case of the name, and once.
    ask(&mut read, &[], &[], "ACME/api", DependabotAlerts).unwrap();
    assert_eq!(read[0].asked, [DependabotAlerts]);
    assert!(any_asked(&read));
    let again = "Orbit is already changing Dependabot alerts in acme/api.";
    assert_eq!(
        ask(&mut read, &[], &[], "acme/api", DependabotAlerts).unwrap_err(),
        again
    );
    // Nor while one is on its way.
    read[0].asked.clear();
    let journal = [change_entry(
        WriteState::Sending,
        Action::ChangeSetting,
        Some(DependabotAlerts),
    )];
    assert!(sending(&journal, "acme/API", DependabotAlerts));
    assert_eq!(
        ask(&mut read, &journal, &[], "acme/api", DependabotAlerts).unwrap_err(),
        again
    );
    // A repository Orbit can't see has nothing to change.
    read[0].unseen = Some(unseen_text("acme/api"));
    assert_eq!(
        ask(&mut read, &[], &[], "acme/api", DependabotAlerts).unwrap_err(),
        unseen_text("acme/api")
    );
}

/// A read keeps what you asked for; the look before a change takes the
/// ask, and says whether the change is still needed.
#[test]
fn a_read_keeps_your_asks_and_the_look_takes_them() {
    use Setting::*;
    let now = Utc::now();
    let mut read = vec![read_at(&[(DependabotAlerts, Some(false))])];
    read[0].asked = vec![DependabotAlerts, DependabotUpdates];
    record(
        &mut read,
        "acme/api",
        Ok(read_at(&[
            (DependabotAlerts, Some(false)),
            (DependabotUpdates, Some(true)),
        ])),
        now,
    );
    assert_eq!(read[0].asked, [DependabotAlerts, DependabotUpdates]);
    record(&mut read, "acme/api", Err("HTTP 502".into()), now);
    assert_eq!(read[0].asked.len(), 2);

    assert_eq!(
        next_ask(&mut read, &["acme/api".into()]),
        Some(("acme/api".into(), DependabotAlerts))
    );
    // Still off: it's sent, but only after a read that worked.
    assert!(!take(&mut read, &[], "acme/api", DependabotAlerts, false));
    read[0].asked.insert(0, DependabotAlerts);
    assert!(take(&mut read, &[], "acme/api", DependabotAlerts, true));
    // On by now: nothing to send.
    assert!(!take(&mut read, &[], "acme/api", DependabotUpdates, true));
    assert!(!any_asked(&read));

    // A repository Orbit no longer watches loses its asks.
    read[0].asked = vec![DependabotAlerts];
    assert_eq!(next_ask(&mut read, &[]), None);
    assert!(!any_asked(&read));
}

/// A change Orbit stopped in the middle of is marked as cut off and never
/// sent again; other writes are left to the journal's own pass, and the
/// journal's pass leaves changes alone.
#[test]
fn a_change_cut_off_by_orbit_stopping_is_never_sent_again() {
    use Setting::*;
    let now = Utc::now();
    let mut journal = vec![
        change_entry(
            WriteState::Sending,
            Action::ChangeSetting,
            Some(ReadOnlyToken),
        ),
        change_entry(WriteState::Sending, Action::PostComment, None),
        change_entry(
            WriteState::Done,
            Action::ChangeSetting,
            Some(DependabotAlerts),
        ),
    ];
    let mut threads = vec![];
    assert!(crate::ops::interrupted(&mut journal, &mut threads, now));
    assert_eq!(journal[0].state, WriteState::Sending);
    assert_eq!(journal[1].state, WriteState::Failed);

    assert_eq!(interrupted(&mut journal, now), ["acme/api"]);
    assert_eq!(journal[0].state, WriteState::Failed);
    assert_eq!(journal[0].error.as_deref(), Some(INTERRUPTED));
    assert_eq!(journal[2].state, WriteState::Done);
    assert!(interrupted(&mut journal, now).is_empty());
}

/// A change is recorded as on its way before it is sent, then done, with
/// where GitHub shows it, or failed, with why.
#[test]
fn a_change_is_recorded_before_it_is_sent_and_settled_after() {
    use Setting::*;
    let now = Utc::now();
    let mut journal = vec![];
    let id = record_change(&mut journal, "Acme/Api", ReadOnlyToken, now);
    assert_eq!(
        (
            journal[0].action,
            journal[0].state,
            journal[0].setting,
            journal[0].thread.as_str()
        ),
        (
            Action::ChangeSetting,
            WriteState::Sending,
            Some(ReadOnlyToken),
            "settings:acme/api"
        )
    );
    assert!(sending(&journal, "acme/api", ReadOnlyToken));
    settle(&mut journal, &id, Ok(()), now);
    assert_eq!(journal[0].state, WriteState::Done);
    assert_eq!(
        journal[0].url,
        "https://github.com/Acme/Api/settings/actions"
    );
    assert!(!sending(&journal, "acme/api", ReadOnlyToken));

    let id = record_change(&mut journal, "acme/api", SecretScanning, now);
    settle(&mut journal, &id, Err(change_error("HTTP 409")), now);
    assert_eq!(journal[1].state, WriteState::Failed);
    assert!(journal[1].url.is_empty());
    assert_eq!(
        journal[1].error.as_deref(),
        Some("The organization's own settings don't let a repository change this, so an owner of the organization changes it there.")
    );
    assert_eq!(
        page("acme/api", DependabotAlerts),
        "https://github.com/acme/api/settings/security_analysis"
    );
    // The journal's file keeps which setting, and older entries load.
    let saved = serde_json::to_value(&journal[1]).unwrap();
    assert_eq!(saved["setting"], "secret_scanning");
    let older: Write = serde_json::from_value(json!({
        "id": "w", "thread": "pr:a/b#1", "action": "post_comment",
        "repo": "a/b", "at": "2026-11-02T00:00:00.000Z",
    }))
    .unwrap();
    assert_eq!(older.setting, None);
    assert!(serde_json::to_value(&older)
        .unwrap()
        .get("setting")
        .is_none());
}

#[test]
fn why_github_didnt_make_a_change_is_said_plainly() {
    assert_eq!(
        change_error("gh: API rate limit exceeded (HTTP 403)"),
        "GitHub's rate limit held this back. Tap Fix again later."
    );
    for refused in ["gh: Forbidden (HTTP 403)", "gh: Not Found (HTTP 404)"] {
        assert_eq!(
            change_error(refused),
            "GitHub didn't let Orbit change this. Orbit's sign-in needs to be an admin of the repository."
        );
    }
    assert!(change_error("gh: Validation Failed (HTTP 422)")
        .starts_with("GitHub didn't make the change: "));
}

/// Each setting is changed with the call GitHub has for it; the Actions
/// token and approvals are set together, so the one not asked about is
/// sent as Orbit read it, or left out when Orbit couldn't tell.
#[test]
fn each_setting_is_changed_with_its_own_call() {
    use Setting::*;
    let read = read_at(&[(ReadOnlyToken, Some(false)), (NoActionsApproval, None)]);
    let call = |s| {
        let (path, method, body) = change(s, &read);
        (path, method, body.unwrap_or(Value::Null))
    };
    assert_eq!(
        call(SecretScanning),
        (
            "",
            "PATCH",
            json!({"security_and_analysis": {"secret_scanning": {"status": "enabled"}}})
        )
    );
    assert_eq!(
        call(PushProtection).2,
        json!({"security_and_analysis": {"secret_scanning_push_protection": {"status": "enabled"}}})
    );
    assert_eq!(
        call(DependabotAlerts),
        ("/vulnerability-alerts", "PUT", Value::Null)
    );
    assert_eq!(
        call(DependabotUpdates),
        ("/automated-security-fixes", "PUT", Value::Null)
    );
    assert_eq!(
        call(PrivateReporting),
        ("/private-vulnerability-reporting", "PUT", Value::Null)
    );
    assert_eq!(
        call(ReadOnlyToken),
        (
            "/actions/permissions/workflow",
            "PUT",
            json!({"default_workflow_permissions": "read"})
        )
    );
    assert_eq!(
        call(NoActionsApproval).2,
        json!({"default_workflow_permissions": "write", "can_approve_pull_request_reviews": false})
    );
    assert_eq!(
        call(ForkApproval),
        (
            "/actions/permissions/fork-pr-contributor-approval",
            "PUT",
            json!({"approval_policy": "first_time_contributors"})
        )
    );
}

/// Against GitHub as the fake has it: with every setting off, turning
/// each on leaves a read that finds them all on.
#[cfg(unix)]
#[tokio::test]
async fn turning_each_setting_on_leaves_it_on() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    std::fs::write(
        d.path().join("repo.json"),
        json!({
            "full_name": "a/b",
            "private": false,
            "visibility": "public",
            "permissions": {"admin": true},
            "security_and_analysis": {
                "secret_scanning": {"status": "disabled"},
                "secret_scanning_push_protection": {"status": "disabled"},
            },
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        d.path().join("settings.json"),
        json!({
            "vulnerability-alerts": false,
            "automated-security-fixes": false,
            "private-vulnerability-reporting": {"enabled": false},
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
    let r = crate::github::GH
        .scope(gh.clone(), read("a/b"))
        .await
        .unwrap();
    assert!(r.found.iter().all(|f| f.on == Some(false)), "{r:?}");
    for setting in Setting::ALL {
        // As the engine does: each change from a read just before it.
        let before = crate::github::GH
            .scope(gh.clone(), read("a/b"))
            .await
            .unwrap();
        crate::github::GH
            .scope(gh.clone(), turn_on("a/b", setting, &before))
            .await
            .unwrap();
    }
    let r = crate::github::GH
        .scope(gh.clone(), read("a/b"))
        .await
        .unwrap();
    assert!(r.found.iter().all(|f| f.on == Some(true)), "{r:?}");
    assert_eq!(r.found.len(), 8);

    // A change GitHub refuses fails.
    std::fs::write(d.path().join("change-error"), "gh: Conflict (HTTP 409)\n").unwrap();
    let e = crate::github::GH
        .scope(gh.clone(), turn_on("a/b", Setting::ReadOnlyToken, &r))
        .await
        .unwrap_err();
    assert!(e.contains("HTTP 409"), "{e}");
    assert!(turn_on("a b", Setting::ReadOnlyToken, &r).await.is_err());
}

#[test]
fn only_a_repository_everyone_sees_is_public() {
    for (about, is) in [
        (json!({"private": false, "visibility": "public"}), true),
        (json!({"private": true, "visibility": "private"}), false),
        (json!({"private": false, "visibility": "internal"}), false),
        (json!({"private": false}), true),
        (json!({"private": true}), false),
        (json!({}), false),
    ] {
        assert_eq!(public(&about), is, "{about}");
    }
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
