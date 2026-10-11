use super::*;
use serde_json::json;
use std::collections::BTreeMap;

fn repo(name: &str) -> Repository {
    Repository::new(name, &format!("/tmp/{name}"))
}
fn facts(files: &[&str]) -> Result<CloneFacts, String> {
    let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
    Ok(CloneFacts {
        suggested_setup: setup::suggest(&files).map(|(_, c)| c.to_string()),
        files,
        ..Default::default()
    })
}
fn listed(names: &[&str]) -> Vec<RepositorySummary> {
    names
        .iter()
        .map(|n| {
            serde_json::from_value(json!({
                "name": n, "private": true, "description": "", "pushedAt": "2026-10-11T00:00:00Z",
            }))
            .unwrap()
        })
        .collect()
}
/// A snapshot with GitHub and Codex connected and autopilot on.
fn connected_snapshot() -> Snapshot {
    let mut s = Snapshot {
        github_user: Some("jason".into()),
        codex_ready: true,
        ..Default::default()
    };
    s.settings.automatic = true;
    s
}
fn labels(g: &Guide) -> Vec<String> {
    g.proposals
        .iter()
        .map(|p| {
            format!(
                "{:?} {} {}",
                p.action,
                p.repo.as_deref().unwrap_or("-"),
                p.repository
                    .as_ref()
                    .map(|r| serde_json::to_string(r).unwrap())
                    .or_else(|| p.settings.as_ref().map(|v| v.to_string()))
                    .unwrap_or_default()
            )
        })
        .collect()
}

#[test]
fn a_first_run_checks_connections_and_offers_your_repositories() {
    let mut s = Snapshot {
        github_error: Some("gh is not signed in.".into()),
        codex_error: Some("Codex is not signed in. Run codex login, then reconnect.".into()),
        ..Default::default()
    };
    let lookups = Lookups {
        clones: BTreeMap::new(),
        connectable: Ok(listed(&["acme/api", "acme/web", "acme/docs", "acme/old"])),
    };
    let g = setup(&s, &lookups);
    assert_eq!(
        g.lines,
        [
            "GitHub isn't connected. gh is not signed in.",
            "Codex isn't connected. Codex is not signed in. Run codex login, then reconnect.",
            "No repositories are connected yet. Tap one of yours below and Orbit downloads its own copy and connects it, with automatic reviews off until you turn them on.",
        ]
    );
    assert_eq!(
        labels(&g),
        [
            "CheckConnections - ",
            "SetUpRepository acme/api {}",
            "SetUpRepository acme/web {}",
            "SetUpRepository acme/docs {}",
        ]
    );
    assert!(g.proposals[1..].iter().all(|p| p.connect));
    assert_eq!(
        g.proposals.iter().map(|p| p.id).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert_eq!(g.to_do, 3);

    // Without an error yet, it says how to connect.
    s.github_error = None;
    let g = setup(&s, &Lookups::default());
    assert_eq!(
        g.lines[0],
        "GitHub isn't connected. Run gh auth login in a terminal on the computer running Orbit."
    );
    assert_eq!(
        g.lines[2],
        "No repositories are connected yet, and GitHub lists none of yours to connect. You can add one under Repositories."
    );
    let failed = Lookups {
        connectable: Err("GitHub took too long to list your repositories.".into()),
        ..Default::default()
    };
    assert_eq!(
        setup(&s, &failed).lines[2],
        "No repositories are connected yet, and Orbit couldn't list yours on GitHub: GitHub took too long to list your repositories."
    );
}

#[test]
fn each_repository_gets_its_next_steps() {
    let mut s = connected_snapshot();
    s.settings.automatic = false;
    let mut done = repo("a/done");
    done.setup_command = "npm ci".into();
    done.automatic = true;
    let mut claude = repo("a/claude");
    claude.provider = Provider::Claude;
    s.claude.error = Some("claude isn't installed.".into());
    s.settings.repositories = vec![repo("a/npm"), done, claude, repo("a/plain"), repo("a/gone")];
    let lookups = Lookups {
        clones: BTreeMap::from([
            (
                "a/npm".into(),
                facts(&["package.json", "package-lock.json"]),
            ),
            ("a/done".into(), facts(&["package-lock.json"])),
            ("a/claude".into(), facts(&["Cargo.lock"])),
            ("a/plain".into(), facts(&["Makefile"])),
            (
                "a/gone".into(),
                Err("Orbit can't find this repository's copy.".into()),
            ),
        ]),
        connectable: Ok(listed(&["acme/api"])),
    };
    let g = setup(&s, &lookups);
    assert_eq!(
        g.lines,
        [
            "GitHub: connected as @jason.",
            "Codex: connected.",
            "Claude isn't connected. claude isn't installed.",
            "a/npm has no setup command yet. Its copy has package-lock.json, so `npm ci --ignore-scripts` would install what reviews need. Installing needs network access, which lets setup and the reviewer reach the internet, including code from the pull request. Automatic reviews are off.",
            "a/done sets up with `npm ci`. Its pull requests are reviewed automatically.",
            "a/claude has no setup command: Claude reviews can only run one in a dev container, so they use only what's already installed on the computer running Orbit. Automatic reviews are off.",
            "a/plain has no setup command, and nothing in its copy suggests one, so reviews use only what's already installed on the computer running Orbit. Automatic reviews are off.",
            "Orbit can't read a/gone's copy: Orbit can't find this repository's copy. Automatic reviews are off.",
            "Autopilot is off for all of Orbit, so nothing is reviewed automatically.",
        ]
    );
    assert_eq!(
        labels(&g),
        [
            "CheckConnections - ",
            r#"SetUpRepository a/npm {"setupCommand":"npm ci --ignore-scripts","networkAccess":true}"#,
            r#"SetUpRepository a/npm {"autopilot":true}"#,
            r#"SetUpRepository a/claude {"autopilot":true}"#,
            r#"SetUpRepository a/plain {"autopilot":true}"#,
            r#"SetUpRepository a/gone {"autopilot":true}"#,
            r#"ChangeSettings - {"automatic":true}"#,
        ]
    );
    assert!(g.proposals.iter().all(|p| !p.connect));
    // Claude, a/npm's two, a/claude, a/plain, a/gone's copy and its
    // autopilot, and autopilot for everything.
    assert_eq!(g.to_do, 8);
}

#[test]
fn a_setup_command_already_set_or_network_already_on_is_kept() {
    let mut s = connected_snapshot();
    let mut r = repo("a/web");
    r.network_access = true;
    r.automatic = true;
    s.settings.repositories = vec![r];
    let lookups = Lookups {
        clones: BTreeMap::from([("a/web".into(), facts(&["pnpm-lock.yaml"]))]),
        ..Default::default()
    };
    let g = setup(&s, &lookups);
    // Network is on already, so only the command changes.
    assert_eq!(
        labels(&g),
        [
            r#"SetUpRepository a/web {"setupCommand":"pnpm install --frozen-lockfile --ignore-scripts"}"#
        ]
    );
    assert_eq!(g.to_do, 1);

    s.settings.repositories[0].setup_command = "make deps".into();
    let g = setup(&s, &lookups);
    assert!(g.proposals.is_empty());
    assert_eq!(g.to_do, 0);
    assert_eq!(
        g.lines[2],
        "a/web sets up with `make deps`. Its pull requests are reviewed automatically."
    );
}

#[test]
fn a_new_repository_gets_only_its_own_next_steps() {
    let mut s = connected_snapshot();
    let mut other = repo("a/other");
    other.automatic = true;
    s.settings.repositories = vec![other, repo("acme/api")];
    let g = connected(&s, "ACME/API", Some(&facts(&["go.mod"])));
    assert_eq!(
        g.lines,
        ["acme/api has no setup command yet. Its copy has go.mod, so `go mod download` would install what reviews need. Installing needs network access, which lets setup and the reviewer reach the internet, including code from the pull request. Automatic reviews are off."]
    );
    assert_eq!(
        labels(&g),
        [
            r#"SetUpRepository acme/api {"setupCommand":"go mod download","networkAccess":true}"#,
            r#"SetUpRepository acme/api {"autopilot":true}"#,
        ]
    );
    assert_eq!(connected(&s, "gone/repo", None), Guide::default());
}

#[test]
fn reviewers_are_only_checked_while_a_repository_uses_them() {
    let mut s = connected_snapshot();
    s.claude.error = Some("claude isn't installed.".into());
    s.dev_containers.error = Some("Docker isn't running.".into());
    let names = |s: &Snapshot| -> Vec<&str> { connections(s).iter().map(|c| c.name).collect() };
    assert_eq!(names(&s), ["GitHub", "Codex"]);
    let mut r = repo("a/b");
    r.provider = Provider::Claude;
    r.dev_container.enabled = true;
    s.settings.repositories.push(r);
    assert_eq!(
        names(&s),
        [
            "GitHub",
            "Codex",
            "Claude",
            "Docker and the Dev Container CLI"
        ]
    );
    assert!(connections(&s)[1].needed_by.is_empty());
    assert_eq!(connections(&s)[2].needed_by, ["a/b"]);
}

#[test]
fn every_suggested_command_is_one_an_answer_could_set() {
    for (file, command) in setup::SETUP_COMMANDS {
        assert!(setup::TELLTALE_FILES.contains(&file), "{file}");
        assert_eq!(
            setup::patch([("setupCommand", command)]).and_then(|p| p.setup_command),
            Some(command.to_string()),
            "{command}"
        );
    }
    // The first lock file wins.
    assert_eq!(
        setup::suggest(&["yarn.lock".into(), "package-lock.json".into()]),
        Some(("package-lock.json", "npm ci --ignore-scripts"))
    );
    assert_eq!(setup::suggest(&["Makefile".into()]), None);
}
