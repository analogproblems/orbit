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
            if let Some(label) = &p.label {
                return format!("{:?} {label}", p.action);
            }
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
        github_error: Some("Could not sync a/b: HTTP 502".into()),
        codex_error: Some("Codex is not signed in. Run codex login, then reconnect.".into()),
        ..Default::default()
    };
    let lookups = Lookups {
        clones: BTreeMap::new(),
        connectable: Ok(listed(&["acme/api", "acme/web", "acme/docs", "acme/old"])),
        ..Default::default()
    };
    let g = setup(&s, &lookups);
    assert_eq!(
        g.lines,
        [
            "GitHub isn't connected. Could not sync a/b: HTTP 502",
            "Codex isn't signed in. Tap Sign in to Codex and I'll give you a code to enter on its website.",
            "No repositories are connected yet. Tap one of yours below and Orbit downloads its own copy and connects it, with automatic reviews off until you turn them on.",
        ]
    );
    assert_eq!(
        labels(&g),
        [
            "SignIn Sign in to Codex",
            "CheckConnections - ",
            "SetUpRepository acme/api {}",
            "SetUpRepository acme/web {}",
            "SetUpRepository acme/docs {}",
        ]
    );
    assert!(g.proposals[2..].iter().all(|p| p.connect));
    assert_eq!(g.proposals[0].service.as_deref(), Some("codex"));
    assert_eq!(
        g.proposals.iter().map(|p| p.id).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
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
    // While GitHub isn't connected, the list waits on it.
    assert_eq!(
        setup(&s, &failed).lines[2],
        "No repositories are connected yet. Once GitHub is connected, I'll list yours here to pick from."
    );
    s.github_user = Some("jason".into());
    assert_eq!(
        setup(&s, &failed).lines[2],
        "No repositories are connected yet, and Orbit couldn't list yours on GitHub: GitHub took too long to list your repositories."
    );
}

#[test]
fn a_new_install_gets_a_button_for_each_problem() {
    // As on a computer with neither gh nor Codex installed.
    let s = Snapshot {
        github_error: Some("Cannot start gh: program not found".into()),
        codex_error: Some(
            "Cannot start Codex. Install the Codex CLI and run codex login. program not found"
                .into(),
        ),
        ..Default::default()
    };
    let failed = Lookups {
        connectable: Err("Cannot start gh: program not found".into()),
        ..Default::default()
    };
    let g = setup(&s, &failed);
    assert_eq!(
        g.lines,
        [
            "GitHub CLI isn't installed on the computer running Orbit. Get the GitHub CLI from its page, then tap Check again.",
            "Codex CLI isn't installed on the computer running Orbit. Get the Codex CLI from its page, then tap Check again.",
            "No repositories are connected yet. Once GitHub is connected, I'll list yours here to pick from.",
        ]
    );
    assert_eq!(
        labels(&g),
        [
            "OpenPage Get GitHub CLI",
            "OpenPage Get Codex CLI",
            "CheckConnections - ",
        ]
    );
    assert_eq!(
        g.proposals[..2]
            .iter()
            .map(|p| p.link.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["https://cli.github.com/", "https://github.com/openai/codex"]
    );
    assert_eq!(g.to_do, 3);

    // Installed but signed out, as gh and Codex say it.
    let s = Snapshot {
        github_error: Some("gh: To get started with GitHub CLI, please run:  gh auth login".into()),
        codex_error: Some("Codex is not signed in. Run codex login, then reconnect.".into()),
        ..Default::default()
    };
    let g = setup(&s, &failed);
    assert_eq!(
        g.lines[..2],
        [
            "GitHub isn't signed in. Tap Sign in to GitHub and I'll give you a code to enter on its website.",
            "Codex isn't signed in. Tap Sign in to Codex and I'll give you a code to enter on its website.",
        ]
    );
    assert_eq!(
        labels(&g),
        [
            "SignIn Sign in to GitHub",
            "SignIn Sign in to Codex",
            "CheckConnections - ",
        ]
    );
}

#[test]
fn every_connection_bit_flags_has_its_own_button() {
    let errors = [
        None,
        Some("Cannot start it: program not found"),
        Some("gh: To get started with GitHub CLI, please run:  gh auth login"),
        Some("Codex is not signed in."),
        Some("Install the Dev Container CLI (npm install -g @devcontainers/cli) to run checks in dev containers: missing"),
        Some("Something else went wrong."),
    ];
    for error in errors {
        let mut s = Snapshot {
            github_error: error.map(Into::into),
            codex_error: error.map(Into::into),
            ..Default::default()
        };
        let mut uses = repo("a/b");
        uses.provider = Provider::Claude;
        uses.dev_container.enabled = true;
        let mut other = repo("a/c");
        other.provider = Provider::Antigravity;
        s.settings.repositories = vec![uses, other];
        for status in [&mut s.claude, &mut s.antigravity, &mut s.dev_containers] {
            status.error = error.map(Into::into);
        }
        let all = connections(&s);
        assert_eq!(all.len(), 5);
        for c in &all {
            // Something that goes away by itself has Check again, which
            // every note with a problem has; anything else has its own.
            let fix = fix(c);
            assert_eq!(
                fix.is_none(),
                c.remedy() == Remedy::CheckAgain,
                "{} with {error:?}",
                c.name
            );
            if let Some(link) = fix.and_then(|p| p.link) {
                assert!(may_open(&link), "{link}");
            }
            if !matches!(c.key, "github" | "codex") {
                assert_eq!(c.remedy(), Remedy::Install, "{}", c.name);
            }
        }
        let g = setup(&s, &Lookups::default());
        assert_eq!(
            g.proposals
                .iter()
                .filter(|p| p.action == Action::CheckConnections)
                .count(),
            1
        );
    }
    let docker = Snapshot {
        dev_containers: CliStatus {
            error: Some("Install Docker and start it to run checks in dev containers: no".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut cli = docker.clone();
    cli.dev_containers.error =
        Some("Install the Dev Container CLI (npm install -g @devcontainers/cli) to run checks in dev containers: no".into());
    for (s, page) in [
        (docker, "https://docs.docker.com/get-started/get-docker/"),
        (cli, "https://github.com/devcontainers/cli"),
    ] {
        let mut s = s;
        let mut uses = repo("a/b");
        uses.dev_container.enabled = true;
        s.settings.repositories = vec![uses];
        let c = connections(&s).pop().unwrap();
        assert_eq!(fix(&c).unwrap().link.as_deref(), Some(page));
    }
}

#[test]
fn only_github_links_and_setup_pages_open() {
    for url in PAGES {
        assert!(may_open(url));
    }
    assert!(may_open("https://github.com/login/device"));
    assert!(may_open("https://auth.openai.com/codex/device"));
    assert!(may_open("https://github.com/acme/api/pull/1"));
    for url in [
        "https://example.com/",
        "http://github.com/",
        "https://github.com.evil.example/",
        "https://auth.openai.com.evil.example/",
        "https://github.com/a b",
        "file:///etc/passwd",
        "https://cli.github.com/x",
    ] {
        assert!(!may_open(url), "{url}");
    }
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
        ..Default::default()
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
            "OpenPage Get Claude Code",
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
fn bits_answerer_is_checked_even_when_no_repository_uses_it() {
    let mut s = connected_snapshot();
    let names = |s: &Snapshot| -> Vec<&str> { connections(s).iter().map(|c| c.name).collect() };
    assert!(connections(&s)[1].for_bit);
    s.settings.buddy.answerer = "antigravity".into();
    assert_eq!(names(&s), ["GitHub", "Codex", "Antigravity"]);
    let c = &connections(&s)[2];
    assert!(c.for_bit && c.needed_by.is_empty());
    assert!(!connections(&s)[1].for_bit);
    // Ollama too, with a button to get it.
    s.settings.buddy.answerer = "ollama".into();
    s.ollama.error = Some("Nothing answered at http://localhost:11434.".into());
    assert_eq!(names(&s), ["GitHub", "Codex", "Ollama"]);
    let ollama = connections(&s).pop().unwrap();
    assert_eq!(
        fix(&ollama).unwrap().link.as_deref(),
        Some("https://ollama.com/download")
    );
    assert!(may_open("https://ollama.com/download"));
    // Not while Bit is off.
    s.settings.buddy.enabled = false;
    assert_eq!(names(&s), ["GitHub", "Codex"]);
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
