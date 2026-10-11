//! Bit's setup help: what Orbit needs connected, and each repository's next
//! steps toward automatic reviews, offered as buttons that wait for your
//! tap. Everything here is worked out from Orbit's state and the files in
//! each repository's copy, with no AI.
use crate::{
    ask::{self, Action, Lookups, Proposal},
    setup::{self, CloneFacts, RepositoryPatch},
    types::*,
};

/// Repositories the setup answer offers to connect when none is.
pub const SUGGESTED_REPOS: usize = 3;

/// Something Orbit connects to.
#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    /// Its name in messages.
    pub name: &'static str,
    /// Its part of a Needs you item's key.
    pub key: &'static str,
    pub ready: bool,
    pub error: Option<String>,
    /// The connected repositories that need it.
    pub needed_by: Vec<String>,
    /// How to connect it, when there's no error that says.
    pub how: &'static str,
}

/// What Orbit connects to: GitHub and Codex always, and Claude,
/// Antigravity and dev containers while a repository uses them.
pub fn connections(s: &Snapshot) -> Vec<Connection> {
    let using = |uses: fn(&Repository) -> bool| -> Vec<String> {
        s.settings
            .repositories
            .iter()
            .filter(|r| uses(r))
            .map(|r| r.name.clone())
            .collect()
    };
    let cli = |name, key, status: &CliStatus, needed_by: Vec<String>, how| Connection {
        name,
        key,
        ready: status.ready,
        error: status.error.clone(),
        needed_by,
        how,
    };
    let mut out = vec![
        Connection {
            name: "GitHub",
            key: "github",
            ready: s.github_user.is_some() && s.github_error.is_none(),
            error: s.github_error.clone(),
            needed_by: using(|_| true),
            how: "Run gh auth login in a terminal on the computer running Orbit.",
        },
        Connection {
            name: "Codex",
            key: "codex",
            ready: s.codex_ready,
            error: s.codex_error.clone(),
            needed_by: using(|r| r.provider == Provider::Codex),
            how: "Run codex login in a terminal on the computer running Orbit.",
        },
        cli(
            "Claude",
            "claude",
            &s.claude,
            using(|r| r.provider == Provider::Claude),
            "Install Claude Code, then run claude in a terminal to sign in.",
        ),
        cli(
            "Antigravity",
            "antigravity",
            &s.antigravity,
            using(|r| r.provider == Provider::Antigravity),
            "Install Antigravity, then run agy in a terminal to sign in.",
        ),
        cli(
            "Docker and the Dev Container CLI",
            "dev-containers",
            &s.dev_containers,
            using(|r| r.dev_container.enabled),
            "Install and start Docker, then run npm install -g @devcontainers/cli.",
        ),
    ];
    out.retain(|c| matches!(c.key, "github" | "codex") || !c.needed_by.is_empty());
    out
}

/// Names a few repositories: one, two, or how many.
pub fn some_repositories(names: &[String]) -> String {
    match names {
        [one] => one.clone(),
        [one, two] => format!("{one} and {two}"),
        _ => format!("{} repositories", names.len()),
    }
}

/// Bit's setup help as lines and buttons, with how many things are left
/// to do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guide {
    pub lines: Vec<String>,
    pub proposals: Vec<Proposal>,
    pub to_do: u32,
}
impl Guide {
    fn offer(&mut self, proposal: Option<Proposal>) -> bool {
        let Some(mut p) = proposal else {
            return false;
        };
        p.id = self.proposals.len() as u32;
        self.proposals.push(p);
        true
    }
}

/// Where setup stands: each connection, a few of your repositories to
/// connect when none is, each connected repository's next steps, and
/// autopilot.
pub fn setup(s: &Snapshot, lookups: &Lookups) -> Guide {
    let mut g = Guide::default();
    let mut off = false;
    for c in connections(s) {
        if c.ready {
            g.lines.push(match (&s.github_user, c.key) {
                (Some(user), "github") => format!("GitHub: connected as @{user}."),
                _ => format!("{}: connected.", c.name),
            });
        } else {
            off = true;
            g.to_do += 1;
            let why = c.error.as_deref().unwrap_or(c.how);
            g.lines
                .push(format!("{} isn't connected. {}", c.name, why.trim()));
        }
    }
    if off {
        g.offer(Some(Proposal::new(Action::CheckConnections)));
    }
    if s.settings.repositories.is_empty() {
        g.to_do += 1;
        let line = match &lookups.connectable {
            Ok(repos) if !repos.is_empty() => {
                let names = lookups.connectable_names();
                for name in names.iter().take(SUGGESTED_REPOS) {
                    let connect = ask::set_up_proposal(s, name, RepositoryPatch::default(), &names);
                    g.offer(connect);
                }
                "No repositories are connected yet. Tap one of yours below and Orbit downloads its own copy and connects it, with automatic reviews off until you turn them on.".into()
            }
            Ok(_) => "No repositories are connected yet, and GitHub lists none of yours to connect. You can add one under Repositories.".into(),
            Err(e) => format!(
                "No repositories are connected yet, and Orbit couldn't list yours on GitHub: {}",
                e.trim()
            ),
        };
        g.lines.push(line);
    }
    for repo in &s.settings.repositories {
        next_steps(&mut g, s, repo, lookups.clones.get(&repo.name));
    }
    autopilot(&mut g, s);
    g
}

/// The next steps for a repository that was just connected.
pub fn connected(s: &Snapshot, name: &str, facts: Option<&Result<CloneFacts, String>>) -> Guide {
    let mut g = Guide::default();
    if let Some(repo) = s
        .settings
        .repositories
        .iter()
        .find(|r| same_repo(&r.name, name))
    {
        next_steps(&mut g, s, repo, facts);
        autopilot(&mut g, s);
    }
    g
}

/// Autopilot for all of Orbit, which automatic reviews need too.
fn autopilot(g: &mut Guide, s: &Snapshot) {
    if s.settings.automatic || s.settings.repositories.is_empty() {
        return;
    }
    if g.offer(ask::autopilot_proposal(s)) {
        g.to_do += 1;
        g.lines.push(
            "Autopilot is off for all of Orbit, so nothing is reviewed automatically.".into(),
        );
    }
}

/// A repository's setup command and automatic reviews: a line about each,
/// and a button for what's missing.
fn next_steps(
    g: &mut Guide,
    s: &Snapshot,
    repo: &Repository,
    facts: Option<&Result<CloneFacts, String>>,
) {
    let name = &repo.name;
    let suggestion = match facts {
        Some(Ok(facts)) => setup::suggest(&facts.files),
        _ => None,
    };
    let setup_allowed = repo.provider == Provider::Codex || repo.dev_container.enabled;
    let mut setup = if !repo.setup_command.trim().is_empty() {
        format!(
            "{name} sets up with `{}`.",
            ask::shown_command(repo.setup_command.trim())
        )
    } else if let Some(Err(e)) = facts {
        g.to_do += 1;
        format!("Orbit can't read {name}'s copy: {}", e.trim())
    } else {
        match suggestion {
            Some((file, command)) if setup_allowed => {
                let patch = RepositoryPatch {
                    setup_command: Some(command.into()),
                    network_access: (!repo.network_access).then_some(true),
                    ..Default::default()
                };
                if g.offer(ask::set_up_proposal(s, name, patch, &[])) {
                    g.to_do += 1;
                    format!(
                        "{name} has no setup command yet. Its copy has {file}, so `{command}` would install what reviews need. Installing needs network access, which lets setup and the reviewer reach the internet, including code from the pull request."
                    )
                } else {
                    format!("{name} has no setup command.")
                }
            }
            Some(_) => format!(
                "{name} has no setup command: {} reviews can only run one in a dev container, so they use only what's already installed on the computer running Orbit.",
                repo.provider.label()
            ),
            None => format!(
                "{name} has no setup command, and nothing in its copy suggests one, so reviews use only what's already installed on the computer running Orbit."
            ),
        }
    };
    let automatic = RepositoryPatch {
        autopilot: Some(true),
        ..Default::default()
    };
    if repo.automatic {
        setup.push_str(" Its pull requests are reviewed automatically.");
    } else {
        if g.offer(ask::set_up_proposal(s, name, automatic, &[])) {
            g.to_do += 1;
        }
        setup.push_str(" Automatic reviews are off.");
    }
    g.lines.push(setup);
}

#[cfg(test)]
#[path = "guide_tests.rs"]
mod tests;
