//! Bit's setup help: what Orbit needs connected, and each repository's next
//! steps toward automatic reviews, offered as buttons that wait for your
//! tap. Everything here is worked out from Orbit's state and the files in
//! each repository's copy, with no AI.
use crate::{
    api::Api,
    ask::{self, Action, Lookups, Proposal},
    buddy::Answerer,
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
    /// Whether Bit answers the questions you type with it.
    pub for_bit: bool,
    /// How to connect it, when there's no error that says.
    pub how: &'static str,
    /// What to install, by name, and the page that has it.
    pub program: &'static str,
    pub page: &'static str,
}
impl Connection {
    /// What fixes it, while it isn't ready.
    pub fn remedy(&self) -> Remedy {
        let error = self.error.as_deref().unwrap_or_default();
        let missing = error.starts_with("Cannot start");
        match self.key {
            "github" | "codex" if missing => Remedy::Install,
            // Each says so when signing in would fix it.
            "github" if error.contains("gh auth login") => Remedy::SignIn,
            "codex" if error.contains("not signed in") => Remedy::SignIn,
            "github" | "codex" => Remedy::CheckAgain,
            // The reviewers' pages say how to install them and sign in.
            _ => Remedy::Install,
        }
    }
}

/// What fixes a connection that isn't ready.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remedy {
    /// Installing it, or setting it up, from its own page.
    Install,
    /// Signing in from Orbit, for GitHub and Codex.
    SignIn,
    /// Checking again, once whatever went wrong has passed.
    CheckAgain,
}

/// The pages Bit's setup help links to, which the desktop app may open.
pub const PAGES: [&str; 10] = [
    "https://cli.github.com/",
    "https://github.com/openai/codex",
    "https://code.claude.com/docs/en/setup",
    "https://antigravity.google/docs/cli",
    "https://docs.docker.com/get-started/get-docker/",
    "https://github.com/devcontainers/cli",
    "https://ollama.com/download",
    "https://platform.openai.com/api-keys",
    "https://console.anthropic.com/settings/keys",
    "https://aistudio.google.com/apikey",
];
/// Where a sign-in sends you to enter its code.
pub const SIGN_IN_PAGES: [&str; 2] = [
    "https://github.com/login/device",
    "https://auth.openai.com/",
];
/// Whether the desktop app may open `url` for you: a GitHub link, one of
/// Bit's setup pages, or a sign-in page.
pub fn may_open(url: &str) -> bool {
    !url.chars().any(|c| c.is_control() || c.is_whitespace())
        && (url.starts_with("https://github.com/")
            || PAGES.contains(&url)
            || SIGN_IN_PAGES.iter().any(|p| url.starts_with(p)))
}

/// Whether Ollama can answer with `model`, from its latest check.
fn ollama_ready(s: &Snapshot, model: &str) -> Result<(), String> {
    if !s.ollama.ready {
        return Err(s.ollama.error.clone().unwrap_or_else(|| {
            "Orbit hasn't reached it yet. Check its address in Settings.".into()
        }));
    }
    if model.is_empty() {
        return Err("Choose an Ollama model in Settings.".into());
    }
    if !s.ollama_models.iter().any(|m| m == model) {
        return Err(format!(
            "Ollama doesn't have model '{model}'. Choose one in Settings."
        ));
    }
    Ok(())
}

/// Whether your key for `api` can answer with `model`, from its latest
/// check.
fn api_ready(s: &Snapshot, api: Api, model: &str) -> Result<(), String> {
    let key = s.api_keys.get(api);
    let company = api.company();
    if !key.saved {
        return Err(format!(
            "Save your {company} API key in Settings, under Buddy."
        ));
    }
    if !key.status.ready {
        return Err(key.status.error.clone().unwrap_or_else(|| {
            "Orbit hasn't checked your key yet. Tap Check again in Settings.".into()
        }));
    }
    if model.is_empty() {
        return Err(format!(
            "Choose a model for your {company} key in Settings."
        ));
    }
    if !key.models.iter().any(|m| m == model) {
        return Err(format!(
            "{company} doesn't offer model '{model}' to your key. Choose one in Settings."
        ));
    }
    Ok(())
}

/// The answerers you sign in to, which Bit may pick on its own.
pub const SIGN_INS: [Answerer; 3] = [Answerer::Codex, Answerer::Claude, Answerer::Antigravity];

/// The model, and effort, Bit answers with when `a` answers.
pub fn bit_selection(s: &Snapshot, a: Answerer) -> ModelSelection {
    let only = |model: &str| ModelSelection {
        model: model.to_string(),
        effort: String::new(),
    };
    match a {
        Answerer::Ollama => only(&s.settings.buddy.ollama_model),
        Answerer::Api(api) => only(s.settings.buddy.api_model(api)),
        _ => s
            .settings
            .selection(a.provider().unwrap_or(Provider::Codex)),
    }
}

/// Whether Bit can answer the questions you type with `a`, from the
/// latest checks.
pub fn bit_ready(s: &Snapshot, a: Answerer) -> Result<(), String> {
    let selection = bit_selection(s, a);
    match (a, a.provider()) {
        (_, Some(p)) => selection.validate_for(p, s),
        (Answerer::Api(api), None) => api_ready(s, api, &selection.model),
        _ => ollama_ready(s, &selection.model),
    }
}

/// Who Bit switches to on its own: the one sign-in that's ready, while Bit
/// is on and the one you chose to answer isn't ready. Ollama and an API
/// key are never switched away from, since you chose where your questions
/// go.
pub fn pick_answerer(s: &Snapshot) -> Option<Answerer> {
    let buddy = &s.settings.buddy;
    let by = buddy.answered_by();
    if !buddy.enabled || by.provider().is_none() || bit_ready(s, by).is_ok() {
        return None;
    }
    match SIGN_INS
        .into_iter()
        .filter(|&a| bit_ready(s, a).is_ok())
        .collect::<Vec<_>>()[..]
    {
        [one] => Some(one),
        _ => None,
    }
}

/// What Orbit connects to: GitHub and Codex always, Claude, Antigravity
/// and dev containers while a repository or Bit uses them, and Ollama or an
/// API key while Bit answers with it.
pub fn connections(s: &Snapshot) -> Vec<Connection> {
    let mut out = all_connections(s);
    out.retain(|c| matches!(c.key, "github" | "codex") || !c.needed_by.is_empty() || c.for_bit);
    out
}

/// Everything Orbit can connect to, used or not.
pub fn all_connections(s: &Snapshot) -> Vec<Connection> {
    let buddy = &s.settings.buddy;
    let for_bit = |a: Answerer| buddy.enabled && buddy.answered_by() == a;
    let using = |uses: fn(&Repository) -> bool| -> Vec<String> {
        s.settings
            .repositories
            .iter()
            .filter(|r| uses(r))
            .map(|r| r.name.clone())
            .collect()
    };
    let cli = |name, key, status: &CliStatus, needed_by, for_bit, how, program, page| Connection {
        name,
        key,
        ready: status.ready,
        error: status.error.clone(),
        needed_by,
        for_bit,
        how,
        program,
        page,
    };
    let mut out = vec![
        Connection {
            name: "GitHub",
            key: "github",
            ready: s.github_user.is_some() && s.github_error.is_none(),
            error: s.github_error.clone(),
            needed_by: using(|_| true),
            for_bit: false,
            how: "Run gh auth login in a terminal on the computer running Orbit.",
            program: "GitHub CLI",
            page: PAGES[0],
        },
        Connection {
            name: "Codex",
            key: "codex",
            ready: s.codex_ready,
            error: s.codex_error.clone(),
            needed_by: using(|r| r.provider == Provider::Codex),
            for_bit: for_bit(Answerer::Codex),
            how: "Run codex login in a terminal on the computer running Orbit.",
            program: "Codex CLI",
            page: PAGES[1],
        },
        cli(
            "Claude",
            "claude",
            &s.claude,
            using(|r| r.provider == Provider::Claude),
            for_bit(Answerer::Claude),
            "Install Claude Code, then run claude in a terminal to sign in.",
            "Claude Code",
            PAGES[2],
        ),
        cli(
            "Antigravity",
            "antigravity",
            &s.antigravity,
            using(|r| r.provider == Provider::Antigravity),
            for_bit(Answerer::Antigravity),
            "Install Antigravity, then run agy in a terminal to sign in.",
            "Antigravity",
            PAGES[3],
        ),
        cli(
            "Docker and the Dev Container CLI",
            "dev-containers",
            &s.dev_containers,
            using(|r| r.dev_container.enabled),
            false,
            "Install and start Docker, then run npm install -g @devcontainers/cli.",
            "Docker",
            PAGES[4],
        ),
    ];
    // Docker answered, so what's missing is the Dev Container CLI.
    if let Some(c) = out.last_mut().filter(|c| {
        c.error
            .as_deref()
            .is_some_and(|e| e.contains("Dev Container CLI"))
    }) {
        c.program = "Dev Container CLI";
        c.page = PAGES[5];
    }
    out.push(cli(
        "Ollama",
        "ollama",
        &s.ollama,
        vec![],
        for_bit(Answerer::Ollama),
        "Start Ollama on this computer or another one on your network, then check its address in Settings, under Buddy.",
        "Ollama",
        PAGES[6],
    ));
    for (api, page) in Api::ALL.into_iter().zip(&PAGES[7..]) {
        let key = s.api_keys.get(api);
        let (name, key_name, program, how) = match api {
            Api::OpenAi => (
                "OpenAI API",
                "openai-api",
                "an OpenAI API key",
                "Get an API key from OpenAI's website, then save it in Settings, under Buddy.",
            ),
            Api::Anthropic => (
                "Anthropic API",
                "anthropic-api",
                "an Anthropic API key",
                "Get an API key from Anthropic's website, then save it in Settings, under Buddy.",
            ),
            Api::Google => (
                "Gemini API",
                "google-api",
                "a Gemini API key",
                "Get an API key from Google AI Studio, then save it in Settings, under Buddy.",
            ),
        };
        out.push(cli(
            name,
            key_name,
            &key.status,
            vec![],
            for_bit(Answerer::Api(api)),
            how,
            program,
            page,
        ));
    }
    out
}

/// The connection Bit answers with when `a` answers.
fn connection_of(all: &[Connection], a: Answerer) -> Option<&Connection> {
    let key = match a {
        Answerer::Api(api) => format!("{}-api", api.key()),
        _ => a.key().to_string(),
    };
    all.iter().find(|c| c.key == key)
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
    /// Adds a button, unless the same one is there already. Says whether
    /// it's there now.
    fn offer(&mut self, proposal: Option<Proposal>) -> bool {
        let Some(mut p) = proposal else {
            return false;
        };
        p.id = 0;
        if self
            .proposals
            .iter()
            .any(|q| Proposal { id: 0, ..q.clone() } == p)
        {
            return true;
        }
        p.id = self.proposals.len() as u32;
        self.proposals.push(p);
        true
    }
}

/// Where setup stands: first what Bit answers your questions with, then
/// each other connection, a few of your repositories to connect when none
/// is, each connected repository's next steps, and autopilot.
pub fn setup(s: &Snapshot, lookups: &Lookups) -> Guide {
    let mut g = Guide::default();
    let mut off = bit_connection(&mut g, s);
    let mut github = true;
    for c in connections(s) {
        // Bit's own lines cover what Bit answers with, unless a
        // repository's reviews use it too and it's connected.
        if c.for_bit && (!c.ready || (c.needed_by.is_empty() && c.key != "codex")) {
            continue;
        }
        g.lines.push(connection_line(s, &c));
        if c.ready {
            continue;
        }
        off = true;
        github &= c.key != "github";
        g.to_do += 1;
        g.offer(fix(&c));
    }
    if off {
        g.offer(Some(Proposal::new(Action::CheckConnections)));
    }
    if s.settings.repositories.is_empty() {
        g.to_do += 1;
        let line = match &lookups.connectable {
            // GitHub's own line says how to connect it.
            Err(_) if !github => "No repositories are connected yet. Once GitHub is connected, I'll list yours here to pick from.".into(),
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

/// A connection's line in Bit's setup help: that it's connected, or what
/// to do about it.
fn connection_line(s: &Snapshot, c: &Connection) -> String {
    if c.ready {
        return match (&s.github_user, c.key) {
            (Some(user), "github") => format!("GitHub: connected as @{user}."),
            _ => format!("{}: connected.", c.name),
        };
    }
    let why = c.error.as_deref().unwrap_or(c.how).trim();
    match c.remedy() {
        Remedy::Install if matches!(c.key, "github" | "codex") => format!(
            "{} isn't installed on the computer running Orbit. Get the {} from its page, then tap Check again.",
            c.program, c.program
        ),
        Remedy::SignIn => format!(
            "{} isn't signed in. Tap Sign in to {} and I'll give you a code to enter on its website.",
            c.name, c.name
        ),
        Remedy::Install | Remedy::CheckAgain => format!("{} isn't connected. {why}", c.name),
    }
}

/// Bit's answer when it can't answer a question you typed: what it would
/// answer with, and buttons to connect something.
pub fn bit_setup(s: &Snapshot) -> Guide {
    let mut g = Guide::default();
    if bit_connection(&mut g, s) {
        g.offer(Some(Proposal::new(Action::CheckConnections)));
    }
    g
}

/// What Bit answers your questions with: a line saying so, and while it
/// isn't ready, buttons for each way to fix that, from switching to a
/// sign-in that's ready, to connecting one, Ollama or an API key. Says
/// whether anything is off.
fn bit_connection(g: &mut Guide, s: &Snapshot) -> bool {
    let buddy = &s.settings.buddy;
    if !buddy.enabled {
        return false;
    }
    let by = buddy.answered_by();
    let error = match bit_ready(s, by) {
        Ok(()) => {
            g.lines
                .push(format!("I answer your questions with {}.", by.label()));
            return false;
        }
        Err(e) => e,
    };
    g.to_do += 1;
    let all = all_connections(s);
    let own = connection_of(&all, by).filter(|c| !c.ready);
    let problem = match own {
        Some(c) => connection_line(s, c),
        None => format!(
            "I answer your questions with {}, which isn't ready: {}",
            by.label(),
            error.trim()
        ),
    };
    let ready: Vec<Answerer> = SIGN_INS
        .into_iter()
        .filter(|&a| a != by && bit_ready(s, a).is_ok())
        .collect();
    let names: Vec<&str> = ready.iter().map(|a| a.label()).collect();
    match names[..] {
        [] => g.lines.extend([
            "First, I need something to answer your questions with: Codex, Claude, Antigravity, Ollama or your own API key. Tap one below to set it up.".into(),
            problem,
        ]),
        [one] => g.lines.extend([
            problem,
            format!("{one} is ready, so tap Answer with {one} to use it instead."),
        ]),
        _ => g.lines.extend([
            problem,
            format!(
                "{} are ready, so tap one to answer with it instead.",
                and_list(&names)
            ),
        ]),
    }
    if let Some(c) = own {
        g.offer(fix(c));
    }
    if !ready.is_empty() {
        for a in ready {
            g.offer(ask::answerer_proposal(
                s,
                a,
                format!("Answer with {}", a.label()),
            ));
        }
        return true;
    }
    for a in SIGN_INS.into_iter().filter(|&a| a != by) {
        if let Some(c) = connection_of(&all, a).filter(|c| !c.ready) {
            g.offer(fix(c));
        }
    }
    let others = [Answerer::Ollama]
        .into_iter()
        .chain(Api::ALL.map(Answerer::Api))
        .filter(|&a| a != by);
    for a in others {
        if let Some(c) = connection_of(&all, a) {
            g.offer(ask::answerer_proposal(s, a, format!("Use {}", c.program)));
        }
    }
    true
}

/// Names a few things: one, two, or a list ending in "and".
fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The button that fixes a connection, besides Check again.
pub fn fix(c: &Connection) -> Option<Proposal> {
    match c.remedy() {
        Remedy::SignIn => Some(Proposal {
            service: Some(c.key.into()),
            label: Some(format!("Sign in to {}", c.name)),
            ..Proposal::new(Action::SignIn)
        }),
        Remedy::Install => Some(Proposal {
            link: Some(c.page.into()),
            label: Some(format!("Get {}", c.program)),
            ..Proposal::new(Action::OpenPage)
        }),
        Remedy::CheckAgain => None,
    }
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
