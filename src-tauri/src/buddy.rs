//! Bit, Orbit's buddy: its settings, a feed of short notes about the review
//! loops in its own voice, a mood that follows them, the fixes it offers for
//! what needs you, quick answers about Orbit, and the memory Bit keeps
//! between questions. Everything here is worked out from what Orbit already
//! tracks; nothing here calls a model.
use crate::{api::Api, setup::CloneFacts, types::*};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The colours Bit can be, as the app names them. The app maps each to a
/// theme token, except "own": the look's own colours.
pub const COLOURS: [&str; 21] = [
    "own",
    "lime",
    "green",
    "aqua",
    "teal",
    "ice",
    "cyan",
    "sky",
    "blue",
    "lavender",
    "purple",
    "bubblegum",
    "pink",
    "rose",
    "red",
    "peach",
    "orange",
    "gold",
    "lemon",
    "moon",
    "grey",
];
/// The hats Bit can wear, or "none".
pub const HATS: [&str; 21] = [
    "none",
    "party",
    "top-hat",
    "beanie",
    "crown",
    "propeller",
    "wizard",
    "cowboy",
    "chef",
    "cap",
    "graduation",
    "beret",
    "flower",
    "bow",
    "halo",
    "headphones",
    "santa",
    "viking",
    "cat-ears",
    "antenna",
    "space-helmet",
];
/// What Bit looks like: the ringed planet it started as, or one of the orbs.
pub const LOOKS: [&str; 17] = [
    "classic", "pip", "tide", "bloom", "moss", "glim", "nova", "duo", "puff", "jelly", "fizz",
    "poly", "prism", "gem", "twirl", "cubby", "fold",
];
/// How Bit's notes sound.
pub const TONES: [&str; 3] = ["cheerful", "calm", "dry"];
/// Who answers the questions you type: Codex, Claude Code or Antigravity,
/// each with its own sign-in, Ollama on this computer or your network, or
/// OpenAI, Anthropic or Google with an API key of yours.
pub const ANSWERERS: [&str; 7] = [
    "codex",
    "claude",
    "antigravity",
    "ollama",
    "openai",
    "anthropic",
    "google",
];

/// Who answers the questions you type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answerer {
    Codex,
    Claude,
    Antigravity,
    Ollama,
    Api(Api),
}
impl Answerer {
    /// Its name in messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ollama => "Ollama",
            Self::Api(api) => api.label(),
            _ => self.provider().map_or("", Provider::label),
        }
    }
    /// Its name in settings: `buddy.answerer`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Antigravity => "antigravity",
            Self::Ollama => "ollama",
            Self::Api(api) => api.key(),
        }
    }
    /// The reviewer it is, if it is one.
    pub fn provider(self) -> Option<Provider> {
        match self {
            Self::Codex => Some(Provider::Codex),
            Self::Claude => Some(Provider::Claude),
            Self::Antigravity => Some(Provider::Antigravity),
            Self::Ollama | Self::Api(_) => None,
        }
    }
}
/// The longest name, in characters.
pub const MAX_NAME: usize = 24;
/// The longest personality text, in characters.
pub const MAX_PERSONALITY: usize = 4000;
/// Notes kept in the feed.
pub const MAX_NOTES: usize = 50;
/// What Bit does about a fix it offers when something needs you: do it and
/// say so, offer a button, or leave it to you.
pub const RULES: [&str; 3] = ["do_it", "ask_first", "leave_it"];
/// Rounds a fix's Continue loop allows, as the button on the loops page does.
pub const CONTINUE_ROUNDS: u32 = 2;
/// Lines Bit's general memory holds.
pub const MEMORY_GENERAL: usize = 20;
/// Lines Bit's memory holds about each repository.
pub const MEMORY_PER_REPO: usize = 10;
/// The longest memory line, in characters.
pub const MEMORY_LINE: usize = 200;
/// The most changes one answer makes to Bit's memory.
pub const MEMORY_CHANGES: usize = 10;

/// One of Orbit's own controls, offered for something that needs you. A fix
/// only uses Orbit: it never answers a reviewer's request, changes a
/// repository's setup or access, or touches the code under review.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fix {
    /// Allows more automatic rounds, for a loop at its round limit.
    ContinueLoop,
    /// Starts a new review of the head, for an automatic review that failed
    /// or was interrupted. Like the app's Review again, it is not a round.
    ReviewAgain,
    /// Publishes a report waiting for you, unless automatic publication
    /// would hold it: for credentials, or for where the PR comes from.
    Publish,
    /// Syncs with GitHub, for syncing that keeps failing.
    SyncNow,
    /// Turns autopilot back on after a failed save turned it off. A change
    /// to settings, so it always waits for your tap.
    TurnOnAutopilot,
    /// Checks the connections again, for one a repository needs that
    /// isn't connected. It always waits for your tap: what fixes it is
    /// usually signing in on the computer first.
    CheckConnections,
}

/// The rule for each fix Bit can do on its own, keyed by the fix's own name
/// (`continue_loop`, …) as the app's rule maps are, and for telling agents
/// about failing CI (see `crate::ops`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub continue_loop: String,
    pub review_again: String,
    pub publish: String,
    pub sync_now: String,
    /// Posting Orbit's comment on a pull request whose checks fail.
    pub comment_on_prs: String,
    /// Opening an `orbit` issue for a default branch that fails.
    pub open_issues: String,
    /// Rerunning a failed job once when it looks like bad luck.
    pub rerun_flaky: String,
    /// Rerunning the same check a second time on the same commit.
    pub rerun_again: String,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            continue_loop: "ask_first".into(),
            review_again: "ask_first".into(),
            publish: "ask_first".into(),
            sync_now: "ask_first".into(),
            comment_on_prs: "do_it".into(),
            open_issues: "do_it".into(),
            rerun_flaky: "do_it".into(),
            rerun_again: "ask_first".into(),
        }
    }
}
impl Rules {
    /// The rule for a fix. Changing settings always asks first.
    pub fn of(&self, fix: Fix) -> &str {
        match fix {
            Fix::ContinueLoop => &self.continue_loop,
            Fix::ReviewAgain => &self.review_again,
            Fix::Publish => &self.publish,
            Fix::SyncNow => &self.sync_now,
            Fix::TurnOnAutopilot | Fix::CheckConnections => "ask_first",
        }
    }
    fn each(&self) -> [&str; 8] {
        [
            &self.continue_loop,
            &self.review_again,
            &self.publish,
            &self.sync_now,
            &self.comment_on_prs,
            &self.open_issues,
            &self.rerun_flaky,
            &self.rerun_again,
        ]
    }
    fn each_mut(&mut self) -> [&mut String; 8] {
        [
            &mut self.continue_loop,
            &mut self.review_again,
            &mut self.publish,
            &mut self.sync_now,
            &mut self.comment_on_prs,
            &mut self.open_issues,
            &mut self.rerun_flaky,
            &mut self.rerun_again,
        ]
    }
}
/// Only supplied rules change.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesPatch {
    pub continue_loop: Option<String>,
    pub review_again: Option<String>,
    pub publish: Option<String>,
    pub sync_now: Option<String>,
    pub comment_on_prs: Option<String>,
    pub open_issues: Option<String>,
    pub rerun_flaky: Option<String>,
    pub rerun_again: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Buddy {
    pub enabled: bool,
    pub name: String,
    pub colour: String,
    pub hat: String,
    /// One of `LOOKS`.
    pub look: String,
    pub tone: String,
    /// Told to whoever answers your questions, with the tone.
    pub personality: String,
    pub rules: Rules,
    /// One of `ANSWERERS`.
    pub answerer: String,
    /// Where Ollama listens, when it answers.
    pub ollama_url: String,
    /// The Ollama model that answers.
    pub ollama_model: String,
    /// The model that answers with your OpenAI key.
    pub openai_model: String,
    /// The model that answers with your Anthropic key.
    pub anthropic_model: String,
    /// The model that answers with your Google key.
    pub google_model: String,
}
impl Default for Buddy {
    fn default() -> Self {
        Self {
            enabled: true,
            name: "Bit".into(),
            colour: "lime".into(),
            hat: "none".into(),
            look: "glim".into(),
            tone: "cheerful".into(),
            personality: String::new(),
            rules: Rules::default(),
            answerer: "codex".into(),
            ollama_url: crate::ollama::DEFAULT_URL.into(),
            ollama_model: String::new(),
            openai_model: String::new(),
            anthropic_model: String::new(),
            google_model: String::new(),
        }
    }
}
impl Buddy {
    pub fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Give your buddy a name.".into());
        }
        if name.chars().count() > MAX_NAME {
            return Err(format!("Keep your buddy's name to {MAX_NAME} characters."));
        }
        if name.chars().any(char::is_control) {
            return Err("Your buddy's name can't contain line breaks or tabs.".into());
        }
        if !COLOURS.contains(&self.colour.as_str()) {
            return Err(format!(
                "'{}' isn't one of the buddy's colours.",
                self.colour
            ));
        }
        if !HATS.contains(&self.hat.as_str()) {
            return Err(format!("'{}' isn't one of the buddy's hats.", self.hat));
        }
        if !LOOKS.contains(&self.look.as_str()) {
            return Err(format!("'{}' isn't one of the buddy's looks.", self.look));
        }
        if !TONES.contains(&self.tone.as_str()) {
            return Err(format!("'{}' isn't one of the buddy's tones.", self.tone));
        }
        if self.personality.chars().count() > MAX_PERSONALITY {
            return Err(format!(
                "Keep the personality to {MAX_PERSONALITY} characters."
            ));
        }
        if let Some(rule) = self.rules.each().into_iter().find(|r| !RULES.contains(r)) {
            return Err(format!("'{rule}' isn't one of the buddy's rules."));
        }
        if !ANSWERERS.contains(&self.answerer.as_str()) {
            return Err(format!(
                "'{}' can't answer the buddy's questions.",
                self.answerer
            ));
        }
        crate::ollama::validate_url(&self.ollama_url)?;
        crate::ollama::validate_model(&self.ollama_model)?;
        for api in Api::ALL {
            crate::api::validate_model(self.api_model(api))?;
        }
        Ok(())
    }
    /// Who answers the questions you type.
    pub fn answered_by(&self) -> Answerer {
        match self.answerer.as_str() {
            "claude" => Answerer::Claude,
            "antigravity" => Answerer::Antigravity,
            "ollama" => Answerer::Ollama,
            other => Api::from_key(other).map_or(Answerer::Codex, Answerer::Api),
        }
    }
    /// The model that answers with your key for `api`.
    pub fn api_model(&self, api: Api) -> &str {
        match api {
            Api::OpenAi => &self.openai_model,
            Api::Anthropic => &self.anthropic_model,
            Api::Google => &self.google_model,
        }
    }
    fn api_model_mut(&mut self, api: Api) -> &mut String {
        match api {
            Api::OpenAi => &mut self.openai_model,
            Api::Anthropic => &mut self.anthropic_model,
            Api::Google => &mut self.google_model,
        }
    }
    /// Replaces anything saved state holds that this version doesn't know,
    /// such as a hat from a newer Orbit, with the default.
    pub fn normalize(&mut self) {
        let default = Self::default();
        self.name = self.name.trim().to_string();
        if self.name.is_empty()
            || self.name.chars().count() > MAX_NAME
            || self.name.chars().any(char::is_control)
        {
            self.name = default.name;
        }
        if !COLOURS.contains(&self.colour.as_str()) {
            self.colour = default.colour;
        }
        if !HATS.contains(&self.hat.as_str()) {
            self.hat = default.hat;
        }
        if !LOOKS.contains(&self.look.as_str()) {
            self.look = default.look;
        }
        if !TONES.contains(&self.tone.as_str()) {
            self.tone = default.tone;
        }
        if self.personality.chars().count() > MAX_PERSONALITY {
            self.personality = self.personality.chars().take(MAX_PERSONALITY).collect();
        }
        for rule in self.rules.each_mut() {
            if !RULES.contains(&rule.as_str()) {
                *rule = "ask_first".into();
            }
        }
        if !ANSWERERS.contains(&self.answerer.as_str()) {
            self.answerer = default.answerer;
        }
        if crate::ollama::validate_url(&self.ollama_url).is_err() {
            self.ollama_url = default.ollama_url;
        }
        if crate::ollama::validate_model(&self.ollama_model).is_err() {
            self.ollama_model = default.ollama_model;
        }
        for api in Api::ALL {
            if crate::api::validate_model(self.api_model(api)).is_err() {
                self.api_model_mut(api).clear();
            }
        }
    }
}
/// Only supplied fields change.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuddyPatch {
    pub enabled: Option<bool>,
    pub name: Option<String>,
    pub colour: Option<String>,
    pub hat: Option<String>,
    pub look: Option<String>,
    pub tone: Option<String>,
    pub personality: Option<String>,
    pub rules: Option<RulesPatch>,
    pub answerer: Option<String>,
    pub ollama_url: Option<String>,
    pub ollama_model: Option<String>,
    pub openai_model: Option<String>,
    pub anthropic_model: Option<String>,
    pub google_model: Option<String>,
}
impl BuddyPatch {
    pub fn apply(self, buddy: &mut Buddy) {
        if let Some(v) = self.enabled {
            buddy.enabled = v;
        }
        if let Some(v) = self.name {
            buddy.name = v.trim().to_string();
        }
        if let Some(v) = self.colour {
            buddy.colour = v;
        }
        if let Some(v) = self.hat {
            buddy.hat = v;
        }
        if let Some(v) = self.look {
            buddy.look = v;
        }
        if let Some(v) = self.tone {
            buddy.tone = v;
        }
        if let Some(v) = self.personality {
            buddy.personality = v;
        }
        if let Some(v) = self.answerer {
            buddy.answerer = v;
        }
        if let Some(v) = self.ollama_url {
            buddy.ollama_url = v.trim().to_string();
        }
        if let Some(v) = self.ollama_model {
            buddy.ollama_model = v.trim().to_string();
        }
        for (api, v) in [
            (Api::OpenAi, self.openai_model),
            (Api::Anthropic, self.anthropic_model),
            (Api::Google, self.google_model),
        ] {
            if let Some(v) = v {
                *buddy.api_model_mut(api) = v.trim().to_string();
            }
        }
        if let Some(rules) = self.rules {
            let patch = [
                rules.continue_loop,
                rules.review_again,
                rules.publish,
                rules.sync_now,
                rules.comment_on_prs,
                rules.open_issues,
                rules.rerun_flaky,
                rules.rerun_again,
            ];
            for (rule, v) in buddy.rules.each_mut().into_iter().zip(patch) {
                if let Some(v) = v {
                    *rule = v;
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mood {
    Happy,
    Busy,
    Worried,
    Sleepy,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    RoundStarted,
    RoundFinished,
    Clean,
    NeedsYou,
    Stopped,
    Continued,
    /// Bit is doing a fix on its own.
    Fixing,
    /// A fix went through.
    Fixed,
    /// Bit's answer to a question you asked.
    Answer,
    /// Something Orbit posted for an agent about failing CI, or an
    /// agent's answer worth knowing.
    Ci,
    /// A repository was connected, with its next steps.
    Connected,
    /// A pull request's story has something for you to decide, from the
    /// Decisions page.
    Decide,
    /// A sign-in waiting for you to enter its code on a website, or one
    /// that didn't finish.
    SignIn,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Unique and never reused, so the app can key on it.
    pub id: u64,
    pub at: String,
    pub kind: NoteKind,
    pub pr_id: Option<String>,
    pub review_id: Option<String>,
    /// The repository a workspace item is about, such as one failing to sync.
    pub repo: Option<String>,
    pub text: String,
    /// The Needs you item the note is about, so the app can offer its fix
    /// while the item is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// For an answer, the question you asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// For an answer, a line for each thing it lists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<NoteLine>,
    /// For an answer to a question you typed, the buttons it offers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposals: Vec<crate::ask::Proposal>,
    /// For an answer to a question you typed, what it changed in Bit's
    /// memory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory: Vec<MemoryChange>,
    /// A question you typed that Bit is still answering.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}
impl Note {
    fn new(kind: NoteKind, text: String) -> Self {
        Self {
            id: 0,
            at: now(),
            kind,
            pr_id: None,
            review_id: None,
            repo: None,
            text,
            key: None,
            question: None,
            lines: vec![],
            proposals: vec![],
            memory: vec![],
            pending: false,
        }
    }
    /// About a Needs you item: its PR, review and repository.
    fn about(self, item: &Attention) -> Self {
        Self {
            pr_id: item.pr_id.clone(),
            review_id: item.review_id.clone(),
            repo: item.repo.clone(),
            ..self
        }
    }
}
/// A line of an answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteLine {
    pub text: String,
    pub pr_id: Option<String>,
    /// The Needs you item the line is about, as on a note.
    pub key: Option<String>,
}
/// What Bit last saw of a loop, to tell what changed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeenLoop {
    pub pr_id: String,
    pub stopped: bool,
    pub rounds: u32,
    /// Rounds that have finished.
    pub finished: u32,
    pub extra_rounds: u32,
}
/// The feed, saved with the state so a restart neither loses nor repeats a
/// note.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Feed {
    /// Oldest first, at most `MAX_NOTES`.
    pub notes: Vec<Note>,
    pub loops: Vec<SeenLoop>,
    /// Whether the loops were recorded once. Loops seen on the first look,
    /// such as after updating Orbit, get no notes.
    pub seeded: bool,
    pub next_id: u64,
    pub memory: Memory,
    /// What needs a decision that Bit has already seen, by `ToDecide::key`,
    /// with when Bit last saw it in the history (to the day). A key stays
    /// while it is still there, so it is never noted twice, and is
    /// forgotten `FORGET_DECISION_DAYS` after it is gone or as soon as its
    /// repository is disconnected.
    pub decisions_seen: BTreeMap<String, String>,
    /// When Bit began watching each connected repository's decisions, by
    /// lowercased name. What needs a decision in a pull request last
    /// updated before then gets no note, so Bit doesn't retell history.
    pub decisions_since: BTreeMap<String, String>,
}
/// How long Bit remembers a thing to decide after it has left the history.
pub const FORGET_DECISION_DAYS: i64 = 30;

/// What Bit remembers between questions, in its own words: lines about you
/// and Orbit in general, and lines about each repository. Bit adds and
/// forgets lines itself as it answers what you type, within the limits
/// above, and you can forget any line.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Memory {
    pub general: Vec<String>,
    /// By repository, as `owner/name`. A repository with no lines has no
    /// entry.
    pub repositories: BTreeMap<String, Vec<String>>,
}
impl Memory {
    /// The lines about `repo`, or the general ones for none, if there are
    /// any.
    fn lines_mut(&mut self, repo: Option<&str>) -> Option<&mut Vec<String>> {
        match repo {
            None => Some(&mut self.general),
            Some(repo) => self
                .repositories
                .iter_mut()
                .find(|(name, _)| same_repo(name, repo))
                .map(|(_, lines)| lines),
        }
    }
    /// The name memory keeps `repo`'s lines under, if it has any.
    pub fn knows(&self, repo: &str) -> Option<&str> {
        self.repositories
            .keys()
            .find(|name| same_repo(name, repo))
            .map(String::as_str)
    }
    /// Forgets a line, returning it as it was kept.
    fn forget(&mut self, repo: Option<&str>, line: &str) -> Option<String> {
        let lines = self.lines_mut(repo)?;
        let at = lines.iter().position(|l| same_line(l, line))?;
        let line = lines.remove(at);
        self.repositories.retain(|_, lines| !lines.is_empty());
        Some(line)
    }
}
/// Whether two memory lines say the same, ignoring case.
fn same_line(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}
/// A change to Bit's memory: one an answer asks for, or, on the answer's
/// note, one it made.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryChange {
    /// The repository, or none for general memory.
    pub repo: Option<String>,
    pub line: String,
    pub kind: MemoryKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Remember,
    Forget,
    /// Not remembered: its place was full.
    Full,
}
/// Makes an answer's changes to Bit's memory and returns those it made.
/// Lines are forgotten first, so a full place can make room; a line already
/// kept isn't added again, and one that doesn't fit is noted as not kept.
fn change_memory(memory: &mut Memory, changes: Vec<MemoryChange>) -> Vec<MemoryChange> {
    let (forget, remember): (Vec<_>, Vec<_>) = changes
        .into_iter()
        .take(MEMORY_CHANGES)
        .partition(|c| c.kind == MemoryKind::Forget);
    let mut made = vec![];
    for change in forget {
        if let Some(line) = memory.forget(change.repo.as_deref(), &change.line) {
            made.push(MemoryChange { line, ..change });
        }
    }
    for change in remember {
        let (lines, max) = match change.repo.as_deref() {
            None => (&mut memory.general, MEMORY_GENERAL),
            Some(repo) => {
                let name = memory.knows(repo).unwrap_or(repo).to_string();
                (
                    memory.repositories.entry(name).or_default(),
                    MEMORY_PER_REPO,
                )
            }
        };
        if lines.iter().any(|l| same_line(l, &change.line)) {
            continue;
        }
        if lines.len() >= max {
            made.push(MemoryChange {
                kind: MemoryKind::Full,
                ..change
            });
            continue;
        }
        lines.push(change.line.clone());
        made.push(change);
    }
    memory.repositories.retain(|_, lines| !lines.is_empty());
    made
}
/// Forgets a line of Bit's memory you chose, general when `repo` is none.
/// Returns whether it was there.
pub fn forget(s: &mut Snapshot, repo: Option<&str>, line: &str) -> bool {
    s.buddy_feed.memory.forget(repo, line).is_some()
}

/// A loop as Bit sees it now.
struct Current<'a> {
    pr: &'a PullRequest,
    state: LoopState,
    seen: SeenLoop,
    /// Each finished round's review, oldest first.
    finished: Vec<&'a Review>,
}
fn current<'a>(s: &'a Snapshot, pr: &'a PullRequest) -> Option<Current<'a>> {
    let state = crate::review_loop::state(s, pr)?;
    let finished: Vec<&Review> = state
        .round_ids
        .iter()
        .filter_map(|id| s.reviews.iter().find(|r| &r.id == id))
        // A follow-up chat sets a finished round back to running until the
        // chat ends; the round is still finished.
        .filter(|r| !active(&r.status) || r.before_follow_up.is_some())
        .collect();
    let control = s.loops.iter().find(|c| c.pr_id == pr.id);
    let seen = SeenLoop {
        pr_id: pr.id.clone(),
        stopped: state.status == LoopStatus::Stopped,
        rounds: state.rounds,
        finished: finished.len() as u32,
        extra_rounds: control.map_or(0, |c| c.extra_rounds),
    };
    Some(Current {
        pr,
        state,
        seen,
        finished,
    })
}
/// `orbit#41` for `analogproblems/orbit#41`.
fn label(pr: &PullRequest) -> String {
    let name = pr.repo.rsplit('/').next().unwrap_or(&pr.repo);
    format!("{name}#{}", pr.number)
}
/// The label of a PR Orbit knows, in the inbox or not.
fn pr_label(s: &Snapshot, pr_id: Option<&str>) -> Option<String> {
    let id = pr_id?;
    s.prs
        .iter()
        .chain(&s.history_prs)
        .find(|p| p.id == id)
        .map(label)
}
fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
/// What a finished round found, for its note.
enum Outcome {
    Blocking(u32),
    Failed,
    Unstructured,
}
fn outcome(r: &Review) -> Outcome {
    let status = r.before_follow_up.as_ref().map_or(&r.status, |b| &b.status);
    if status == "failed" {
        return Outcome::Failed;
    }
    match r.blocking {
        Some(n) => Outcome::Blocking(n),
        None => Outcome::Unstructured,
    }
}

/// The text of a note, in the buddy's tone.
mod say {
    use super::{plural, Fix, Outcome};

    pub fn started(tone: &str, pr: &str, round: u32) -> String {
        match tone {
            "calm" => format!("Round {round} started on {pr}."),
            "dry" => format!("Round {round} on {pr}. Here we go again."),
            _ => format!("On it! Round {round} of {pr} is underway."),
        }
    }
    pub fn finished(tone: &str, pr: &str, round: u32, outcome: &Outcome) -> String {
        match (tone, outcome) {
            ("calm", Outcome::Blocking(n)) => format!(
                "Round {round} on {pr} finished with {}.",
                plural(*n, "blocking finding", "blocking findings")
            ),
            ("calm", Outcome::Failed) => format!("Round {round} on {pr} failed."),
            ("calm", Outcome::Unstructured) => {
                format!("Round {round} on {pr} finished without structured findings.")
            }
            ("dry", Outcome::Blocking(n)) => format!(
                "Round {round} on {pr}: {}. The author has homework.",
                plural(*n, "blocking finding", "blocking findings")
            ),
            ("dry", Outcome::Failed) => format!("Round {round} on {pr} fell over."),
            ("dry", Outcome::Unstructured) => {
                format!("Round {round} on {pr} finished. Its findings didn't come back tidy.")
            }
            (_, Outcome::Blocking(n)) => format!(
                "Round {round} of {pr} found {}. Over to the author!",
                plural(*n, "blocking finding", "blocking findings")
            ),
            (_, Outcome::Failed) => format!("Oh no, round {round} of {pr} didn't finish."),
            (_, Outcome::Unstructured) => {
                format!("Round {round} of {pr} finished, but I couldn't read its findings.")
            }
        }
    }
    pub fn clean(tone: &str, pr: &str, rounds: u32, history: &str) -> String {
        let after = plural(rounds, "round", "rounds");
        match tone {
            "calm" => format!("{pr} is clean after {after} ({history})."),
            "dry" => format!("{pr}: clean in {after} ({history}). Not bad."),
            _ => format!("{pr} is clean after {after}! 🎉 {history}"),
        }
    }
    pub fn needs_you(tone: &str, pr: Option<&str>, reason: &str) -> String {
        let reason = reason.trim();
        match (tone, pr) {
            ("calm", Some(pr)) => format!("{pr} needs you: {reason}"),
            ("calm", None) => reason.to_string(),
            ("dry", Some(pr)) => format!("{pr} wants you. {reason}"),
            ("dry", None) => format!("{reason} Thought you'd want to know."),
            (_, Some(pr)) => format!("Could you take a look at {pr}? {reason}"),
            (_, None) => format!("Heads up! {reason}"),
        }
    }
    pub fn told(tone: &str, what: &str) -> String {
        match tone {
            "calm" => what.to_string(),
            "dry" => format!("{what} Done."),
            _ => format!("{what} 📮"),
        }
    }
    pub fn answered(tone: &str, what: &str) -> String {
        match tone {
            "calm" => what.to_string(),
            "dry" => format!("{what} Of course."),
            _ => format!("{what} Worth a look."),
        }
    }
    pub fn stopped(tone: &str, pr: &str) -> String {
        match tone {
            "calm" => format!("The loop on {pr} is paused."),
            "dry" => format!("{pr}: paused. Fine."),
            _ => format!("Pausing the loop on {pr}. I'll be here."),
        }
    }
    pub fn continued(tone: &str, pr: &str) -> String {
        match tone {
            "calm" => format!("The loop on {pr} is running again."),
            "dry" => format!("{pr}: back on."),
            _ => format!("Back to work on {pr}!"),
        }
    }
    pub fn needs_you_answer(tone: &str, n: u32) -> String {
        let things = plural(n, "thing needs you", "things need you");
        match (tone, n) {
            ("calm", 0) => "Nothing needs you right now.".into(),
            ("dry", 0) => "Nothing. Enjoy it while it lasts.".into(),
            (_, 0) => "Nothing needs you right now. 🎉".into(),
            ("calm", _) => format!("{things}:"),
            ("dry", _) => format!("{things}. Naturally."),
            _ => format!("{things}. Here's the list:"),
        }
    }
    pub fn running_answer(tone: &str, n: u32) -> String {
        let things = plural(n, "thing is running", "things are running");
        match (tone, n) {
            ("calm", 0) => "Nothing is running.".into(),
            ("dry", 0) => "Nothing. Very restful.".into(),
            (_, 0) => "Nothing is running. All quiet!".into(),
            ("calm", _) => format!("{things}:"),
            ("dry", _) => format!("{things}. Busy, busy."),
            _ => format!("{things}. Here's what:"),
        }
    }
    pub fn loops_answer(tone: &str, summary: Option<&str>) -> String {
        match (tone, summary) {
            ("calm", None) => "No loops are open.".into(),
            ("dry", None) => "No loops. Nothing to go round.".into(),
            (_, None) => "No loops are open right now.".into(),
            ("calm", Some(summary)) => summary.into(),
            ("dry", Some(summary)) => format!("{summary} The details:"),
            (_, Some(summary)) => format!("{summary} Here's each one:"),
        }
    }
    pub fn repositories_answer(tone: &str, n: u32) -> String {
        let repositories = plural(n, "repository", "repositories");
        match (tone, n) {
            ("calm", 0) => "No repositories yet. Add one under Repositories.".into(),
            ("dry", 0) => "No repositories. Hard to review nothing.".into(),
            (_, 0) => {
                "No repositories yet. Add one under Repositories and I'll keep an eye on it!".into()
            }
            ("calm", _) => format!("{repositories}:"),
            ("dry", _) => format!("{repositories}. Here goes."),
            _ => format!("You have {repositories}. Here's how they're doing:"),
        }
    }
    pub fn fixing(tone: &str, pr: Option<&str>, reason: &str) -> String {
        let reason = reason.trim();
        let about = pr.map_or_else(|| reason.to_string(), |pr| format!("{pr}: {reason}"));
        match tone {
            "calm" => format!("{about} Handling it."),
            "dry" => format!("{about} I'll deal with it."),
            _ => format!("{about} I'm on it!"),
        }
    }
    pub fn done(tone: &str, fix: Fix, pr: &str) -> String {
        match (tone, fix) {
            ("calm", Fix::ContinueLoop) => format!("The loop on {pr} has two more rounds."),
            ("calm", Fix::ReviewAgain) => format!("Queued a new review of {pr}."),
            ("calm", Fix::Publish) => format!("Published the report on {pr}."),
            ("calm", Fix::SyncNow) => "Syncing works again.".into(),
            ("calm", Fix::TurnOnAutopilot) => "Autopilot is on again.".into(),
            ("calm", Fix::CheckConnections) => "That's connected again.".into(),
            ("dry", Fix::ContinueLoop) => format!("{pr} gets two more rounds."),
            ("dry", Fix::ReviewAgain) => format!("{pr} is queued for another go."),
            ("dry", Fix::Publish) => format!("{pr}'s report is out."),
            ("dry", Fix::SyncNow) => "Synced. Finally.".into(),
            ("dry", Fix::TurnOnAutopilot) => "Autopilot: on.".into(),
            ("dry", Fix::CheckConnections) => "Reconnected. Good.".into(),
            (_, Fix::ContinueLoop) => format!("Gave {pr} two more rounds!"),
            (_, Fix::ReviewAgain) => format!("Queued a new review of {pr}. Fingers crossed!"),
            (_, Fix::Publish) => format!("Published the report on {pr}! 📬"),
            (_, Fix::SyncNow) => "Synced with GitHub. All good again!".into(),
            (_, Fix::TurnOnAutopilot) => "Autopilot is back on!".into(),
            (_, Fix::CheckConnections) => "It's connected again. All good!".into(),
        }
    }
    pub fn not_done(tone: &str, fix: Fix, pr: &str, error: &str) -> String {
        let what = match fix {
            Fix::ContinueLoop => format!("continue the loop on {pr}"),
            Fix::ReviewAgain => format!("review {pr} again"),
            Fix::Publish => format!("publish the report on {pr}"),
            Fix::SyncNow => "sync with GitHub".into(),
            Fix::TurnOnAutopilot => "turn autopilot back on".into(),
            Fix::CheckConnections => "reconnect".into(),
        };
        let error = error.trim();
        match tone {
            "calm" => format!("Couldn't {what}: {error}"),
            "dry" => format!("Tried to {what}. No luck: {error}"),
            _ => format!("I tried to {what}, but it didn't work: {error}"),
        }
    }
    pub fn setup_answer(tone: &str, to_do: u32) -> String {
        let things = plural(to_do, "thing", "things");
        match (tone, to_do) {
            ("calm", 0) => "Everything is set up.".into(),
            ("dry", 0) => "All set. Nothing for me to do.".into(),
            (_, 0) => "You're all set! Everything's connected and ready. 🎉".into(),
            ("calm", _) => format!("{things} left to set up:"),
            ("dry", _) => format!("{things} to sort out:"),
            _ => format!("Let's get you set up! {things} to do. Tap a button when you're ready:"),
        }
    }
    pub fn connected(tone: &str, repo: &str, to_do: u32) -> String {
        match (tone, to_do) {
            ("calm", 0) => format!("{repo} is connected and ready."),
            ("dry", 0) => format!("{repo} is in. Nothing else to do."),
            (_, 0) => format!("{repo} is connected and ready to go!"),
            ("calm", _) => format!("{repo} is connected. Next steps:"),
            ("dry", _) => format!("{repo} is in. A couple of things:"),
            _ => format!("{repo} is connected! Here's what I'd set up next:"),
        }
    }
    /// A decision waiting on a pull request, from its write-up.
    pub fn decide(tone: &str, pr: &str, text: &str) -> String {
        let text = text.trim();
        match tone {
            "calm" => format!("{pr} has something to decide. {text}"),
            "dry" => format!("{pr}: {text} Your call."),
            _ => format!("Something to decide on {pr}: {text}"),
        }
    }
    /// A question you typed that Bit couldn't answer.
    pub fn no_answer(tone: &str, error: &str) -> String {
        let error = error.trim();
        match tone {
            "calm" => format!("I couldn't answer that. {error}"),
            "dry" => format!("No answer this time. {error}"),
            _ => format!("Sorry, I couldn't answer that. {error} Try asking again?"),
        }
    }
    pub fn switched(tone: &str, from: &str, to: &str) -> String {
        match tone {
            "calm" => format!("{from} isn't ready, so I'll answer your questions with {to}, which is. You can change that in Settings, under Buddy."),
            "dry" => format!("{from} wasn't answering, so I switched to {to}. Change it back in Settings, under Buddy, if you like."),
            _ => format!("{from} isn't ready, but {to} is, so I'll answer your questions with {to} from now on! You can change that in Settings, under Buddy."),
        }
    }
    pub fn cant_answer(tone: &str) -> String {
        match tone {
            "calm" => "I can't answer yet. First, connect something for me to answer with:".into(),
            "dry" => "Can't answer that. Nothing to answer with yet:".into(),
            _ => "I'd love to answer that, but I need something to answer with first. Tap a button to connect one:".into(),
        }
    }
}

/// Every note kind in a tone, for tests.
#[cfg(test)]
pub(crate) fn samples(tone: &str) -> Vec<(NoteKind, String)> {
    vec![
        (NoteKind::RoundStarted, say::started(tone, "orbit#41", 2)),
        (
            NoteKind::RoundFinished,
            say::finished(tone, "orbit#41", 2, &Outcome::Blocking(1)),
        ),
        (
            NoteKind::RoundFinished,
            say::finished(tone, "orbit#41", 2, &Outcome::Failed),
        ),
        (
            NoteKind::RoundFinished,
            say::finished(tone, "orbit#41", 2, &Outcome::Unstructured),
        ),
        (
            NoteKind::Clean,
            say::clean(tone, "orbit#41", 3, "4 → 1 → 0"),
        ),
        (
            NoteKind::NeedsYou,
            say::needs_you(tone, Some("orbit#41"), "Round limit reached."),
        ),
        (
            NoteKind::NeedsYou,
            say::needs_you(tone, None, "Orbit can't sync."),
        ),
        (NoteKind::Stopped, say::stopped(tone, "orbit#41")),
        (NoteKind::Continued, say::continued(tone, "orbit#41")),
        (
            NoteKind::Fixing,
            say::fixing(tone, Some("orbit#41"), "Round limit reached."),
        ),
        (
            NoteKind::Fixing,
            say::fixing(tone, None, "Orbit can't sync."),
        ),
    ]
    .into_iter()
    .chain(FIXES.into_iter().flat_map(|fix| {
        [
            (NoteKind::Fixed, say::done(tone, fix, "orbit#41")),
            (
                NoteKind::NeedsYou,
                say::not_done(tone, fix, "orbit#41", "It broke."),
            ),
        ]
    }))
    .collect()
}
/// Every fix, for tests.
#[cfg(test)]
pub(crate) const FIXES: [Fix; 6] = [
    Fix::ContinueLoop,
    Fix::ReviewAgain,
    Fix::Publish,
    Fix::SyncNow,
    Fix::TurnOnAutopilot,
    Fix::CheckConnections,
];

/// Compares the loops with what Bit last saw and writes a note for each
/// change, and one for each item in `fresh`: the Needs you items that just
/// opened. With the buddy off, loops are still recorded but nothing is
/// written. Returns whether the feed changed.
pub fn observe(s: &mut Snapshot, fresh: &[Attention]) -> bool {
    let tone = s.settings.buddy.tone.clone();
    let enabled = s.settings.buddy.enabled;
    let mut notes: Vec<(NoteKind, &PullRequest, Option<String>, String)> = vec![];
    let loops_now: Vec<Current> = s.prs.iter().filter_map(|pr| current(s, pr)).collect();
    if s.buddy_feed.seeded {
        for c in &loops_now {
            let pr = label(c.pr);
            let seen = match s.buddy_feed.loops.iter().find(|l| l.pr_id == c.pr.id) {
                Some(seen) => seen.clone(),
                // A loop that has just begun. One with more behind it, such
                // as a PR back in the inbox, is only recorded.
                None if c.seen.rounds <= 1 => SeenLoop::default(),
                None => continue,
            };
            let latest = c.state.round_ids.last().cloned();
            if c.seen.stopped && !seen.stopped {
                notes.push((NoteKind::Stopped, c.pr, None, say::stopped(&tone, &pr)));
            }
            if (seen.stopped && !c.seen.stopped) || c.seen.extra_rounds > seen.extra_rounds {
                notes.push((NoteKind::Continued, c.pr, None, say::continued(&tone, &pr)));
            }
            if c.seen.rounds > seen.rounds {
                notes.push((
                    NoteKind::RoundStarted,
                    c.pr,
                    latest.clone(),
                    say::started(&tone, &pr, c.seen.rounds),
                ));
            }
            // A round that leaves the loop clean says so, with how the
            // blocking findings went down round by round.
            if let Some(r) = c
                .finished
                .last()
                .filter(|_| c.seen.finished > seen.finished)
            {
                let (kind, text) = if c.state.status == LoopStatus::Clean {
                    let history: Vec<String> = c
                        .finished
                        .iter()
                        .map(|r| r.blocking.map_or("?".into(), |n| n.to_string()))
                        .collect();
                    (
                        NoteKind::Clean,
                        say::clean(&tone, &pr, c.seen.finished, &history.join(" → ")),
                    )
                } else {
                    (
                        NoteKind::RoundFinished,
                        say::finished(&tone, &pr, c.seen.finished, &outcome(r)),
                    )
                };
                notes.push((kind, c.pr, Some(r.id.clone()), text));
            }
        }
    }
    let loops: Vec<SeenLoop> = loops_now.into_iter().map(|c| c.seen).collect();
    let mut written: Vec<Note> = vec![];
    if enabled {
        for (kind, pr, review_id, text) in notes {
            written.push(Note {
                pr_id: Some(pr.id.clone()),
                review_id,
                ..Note::new(kind, text)
            });
        }
        // An item Bit fixes on its own gets a note saying so, and the fix's
        // outcome follows. Any other item gets a note that can offer its fix.
        for item in fresh {
            let pr = pr_label(s, item.pr_id.as_deref());
            let pr = pr.as_deref();
            written.push(match will_fix(s, item) {
                Some(_) => {
                    Note::new(NoteKind::Fixing, say::fixing(&tone, pr, &item.reason)).about(item)
                }
                None => Note {
                    key: Some(item.key.clone()),
                    ..Note::new(NoteKind::NeedsYou, say::needs_you(&tone, pr, &item.reason))
                        .about(item)
                },
            });
        }
    }
    let changed = !written.is_empty() || loops != s.buddy_feed.loops || !s.buddy_feed.seeded;
    s.buddy_feed.loops = loops;
    s.buddy_feed.seeded = true;
    append(&mut s.buddy_feed, written);
    changed
}
/// Adds notes to the feed with new IDs, keeping the newest `MAX_NOTES`.
fn append(feed: &mut Feed, notes: Vec<Note>) {
    for mut note in notes {
        note.id = feed.next_id;
        feed.next_id += 1;
        feed.notes.push(note);
    }
    // The oldest notes go first, but never a question Bit is still
    // answering, or its answer would have nowhere to land.
    let mut excess = feed.notes.len().saturating_sub(MAX_NOTES);
    feed.notes.retain(|n| {
        let drop = excess > 0 && !n.pending;
        if drop {
            excess -= 1;
        }
        !drop
    });
}

/// Writes a note for each event about telling agents that Bit tells (see
/// `ops::event_text`), leading to Operations. With the buddy off nothing is
/// written. Returns whether the feed changed.
pub fn observe_ci(s: &mut Snapshot, events: &[crate::ops::Event]) -> bool {
    if !s.settings.buddy.enabled {
        return false;
    }
    let tone = s.settings.buddy.tone.clone();
    let notes: Vec<Note> = events
        .iter()
        .filter_map(|e| crate::ops::event_text(&s.ops, &s.problems, e).map(|told| (e, told)))
        .map(|(e, (problem, repo, text))| {
            let text = match e {
                crate::ops::Event::Heard(_) => say::answered(&tone, &text),
                _ => say::told(&tone, &text),
            };
            Note {
                repo: Some(repo),
                key: Some(format!("problem:{problem}")),
                ..Note::new(NoteKind::Ci, text)
            }
        })
        .collect();
    let changed = !notes.is_empty();
    append(&mut s.buddy_feed, notes);
    changed
}

/// Something in a pull request's story that needs a decision: a flag that
/// asks for one, or a decision its write-up marks so.
#[derive(Clone, Debug, PartialEq)]
pub struct ToDecide {
    /// Stable while the thing stays the same.
    pub key: String,
    pub repo: String,
    pub number: u64,
    /// When the pull request last changed.
    pub at: String,
    pub text: String,
}
/// What needs a decision in the Decisions page's pull requests.
pub fn to_decide(prs: &[crate::ledger::History]) -> Vec<ToDecide> {
    use crate::ledger::Glance;
    use sha2::{Digest, Sha256};
    let mut out = vec![];
    for h in prs {
        let item = |what: String, text: String| ToDecide {
            key: format!("{}#{}:{what}", h.repo.to_ascii_lowercase(), h.number),
            repo: h.repo.clone(),
            number: h.number,
            at: h.updated_at.clone(),
            text,
        };
        for f in h.flags.iter().filter(|f| f.attention == Glance::Act) {
            let kind = serde_json::to_value(f.kind).unwrap_or_default();
            let what = format!(
                "{}:{}",
                kind.as_str().unwrap_or_default(),
                f.finding_id.as_deref().unwrap_or_default()
            );
            out.push(item(what, f.text.clone()));
        }
        for d in h.writeup.iter().flat_map(|w| &w.decisions) {
            if d.attention != Some(Glance::Act) {
                continue;
            }
            let digest = format!(
                "{:x}",
                Sha256::digest(d.statement.trim().to_lowercase().as_bytes())
            );
            let what = format!("writeup:{}", &digest[..12]);
            let text = match d.attention_reason.as_deref() {
                Some(why) => format!("{} {}", d.statement.trim(), why.trim()),
                None => d.statement.trim().to_string(),
            };
            out.push(item(what, text));
        }
    }
    out
}
/// Writes a note for each thing to decide Bit hasn't seen, in a pull
/// request that changed since Bit began watching its repository. A
/// repository Bit sees for the first time is only recorded. With the buddy
/// off, things are still recorded but nothing is written. Returns whether
/// the feed changed.
pub fn observe_decisions(s: &mut Snapshot, items: &[ToDecide]) -> bool {
    let tone = s.settings.buddy.tone.clone();
    let enabled = s.settings.buddy.enabled;
    let connected: Vec<String> = s
        .settings
        .repositories
        .iter()
        .map(|r| r.name.to_ascii_lowercase())
        .collect();
    let feed = &mut s.buddy_feed;
    let mut changed = false;
    let before = feed.decisions_since.len();
    feed.decisions_since
        .retain(|repo, _| connected.contains(repo));
    changed |= feed.decisions_since.len() != before;
    let at = now();
    for repo in &connected {
        if !feed.decisions_since.contains_key(repo) {
            feed.decisions_since.insert(repo.clone(), at.clone());
            changed = true;
        }
    }
    let ago = |days: i64| {
        (chrono::Utc::now() - chrono::Duration::days(days))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };
    // Last seen is kept to the day, so an unchanged history isn't saved
    // again on every read.
    let yesterday = ago(1);
    let mut written = vec![];
    for item in items {
        if let Some(seen) = feed.decisions_seen.get_mut(&item.key) {
            if *seen < yesterday {
                *seen = at.clone();
                changed = true;
            }
            continue;
        }
        feed.decisions_seen.insert(item.key.clone(), at.clone());
        changed = true;
        let since = feed.decisions_since.get(&item.repo.to_ascii_lowercase());
        if enabled && since.is_some_and(|since| item.at.as_str() > since.as_str()) {
            let name = item.repo.rsplit('/').next().unwrap_or(&item.repo);
            let pr = format!("{name}#{}", item.number);
            written.push(Note {
                pr_id: Some(pr_id(&item.repo, item.number)),
                repo: Some(item.repo.clone()),
                ..Note::new(NoteKind::Decide, say::decide(&tone, &pr, &item.text))
            });
        }
    }
    // Forget what left the history long ago, and every key of a repository
    // no longer connected (its keys start `owner/name#`).
    let forget = ago(FORGET_DECISION_DAYS);
    let before = feed.decisions_seen.len();
    feed.decisions_seen.retain(|key, seen| {
        let repo = key.split('#').next().unwrap_or_default();
        *seen >= forget && connected.iter().any(|c| c == repo)
    });
    changed |= feed.decisions_seen.len() != before;
    append(feed, written);
    changed
}

/// The fix Bit does on its own for an item that just opened: one whose rule
/// is Do it, while Bit is on. A loop is continued on its own only once: a
/// loop that has been given more rounds and used them up again waits for
/// you, so a builder and reviewer that never agree can't go round forever.
pub fn will_fix(s: &Snapshot, item: &Attention) -> Option<Fix> {
    let buddy = &s.settings.buddy;
    let continued = || {
        s.loops
            .iter()
            .any(|c| Some(&*c.pr_id) == item.pr_id.as_deref() && c.extra_rounds > 0)
    };
    item.fix.filter(|&fix| {
        buddy.enabled
            && buddy.rules.of(fix) == "do_it"
            && !(fix == Fix::ContinueLoop && continued())
    })
}
fn pr_or_this(s: &Snapshot, item: &Attention) -> String {
    pr_label(s, item.pr_id.as_deref()).unwrap_or_else(|| "this pull request".into())
}
/// Notes that a fix went through. A loop Bit continued is recorded as seen,
/// so the next look doesn't note it again as continued.
pub fn fixed(s: &mut Snapshot, item: &Attention, fix: Fix) {
    if !s.settings.buddy.enabled {
        return;
    }
    if fix == Fix::ContinueLoop {
        let id = item.pr_id.as_deref();
        let seen = s
            .buddy_feed
            .loops
            .iter_mut()
            .find(|l| Some(&*l.pr_id) == id);
        let control = s.loops.iter().find(|c| Some(&*c.pr_id) == id);
        if let (Some(seen), Some(control)) = (seen, control) {
            seen.stopped = control.stopped;
            seen.extra_rounds = control.extra_rounds;
        }
    }
    let text = say::done(&s.settings.buddy.tone, fix, &pr_or_this(s, item));
    append(
        &mut s.buddy_feed,
        vec![Note::new(NoteKind::Fixed, text).about(item)],
    );
}
/// Notes that a fix Bit tried on its own didn't work. The item needs you
/// after all, so the note offers the fix while the item is open.
pub fn not_fixed(s: &mut Snapshot, item: &Attention, fix: Fix, error: &str) {
    if !s.settings.buddy.enabled {
        return;
    }
    let text = say::not_done(&s.settings.buddy.tone, fix, &pr_or_this(s, item), error);
    append(
        &mut s.buddy_feed,
        vec![Note {
            key: Some(item.key.clone()),
            ..Note::new(NoteKind::NeedsYou, text).about(item)
        }],
    );
}

/// Notes a sign-in to `name` waiting for you: the code to enter, and a
/// button for the page that takes it. Returns the note's id.
pub fn signing_in(s: &mut Snapshot, name: &str, code: &str, page: &str) -> u64 {
    let id = s.buddy_feed.next_id;
    let text = format!(
        "Let's sign you in to {name}. Open its sign-in page and enter the code below. I'll check again as soon as you're done."
    );
    let host = page
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_string();
    append(
        &mut s.buddy_feed,
        vec![Note {
            lines: vec![line(format!("Your code: `{code}`"), None, None)],
            proposals: vec![crate::ask::Proposal {
                link: Some(page.into()),
                label: Some(format!("Open {host}")),
                ..crate::ask::Proposal::new(crate::ask::Action::OpenPage)
            }],
            ..Note::new(NoteKind::SignIn, text)
        }],
    );
    id
}
/// Notes how a sign-in to `name` ended. Its code, in the note `code`, is
/// no use any more, so that note loses it and its button.
pub fn signed_in(s: &mut Snapshot, name: &str, code: Option<u64>, result: Result<(), String>) {
    if let Some(note) = s.buddy_feed.notes.iter_mut().find(|n| Some(n.id) == code) {
        note.lines = vec![line("That code is no longer needed.".into(), None, None)];
        note.proposals.clear();
    }
    let note = match result {
        Ok(()) => Note::new(
            NoteKind::Fixed,
            format!("You're signed in to {name}. All good!"),
        ),
        Err(e) => Note::new(
            NoteKind::SignIn,
            format!(
                "Signing in to {name} didn't finish: {} Tap Help me set up to try again.",
                e.trim()
            ),
        ),
    };
    append(&mut s.buddy_feed, vec![note]);
}

/// Adds a question you typed, which Bit answers once Codex has, and
/// returns its note's id.
pub fn asked(s: &mut Snapshot, question: &str) -> u64 {
    let id = s.buddy_feed.next_id;
    append(
        &mut s.buddy_feed,
        vec![Note {
            question: Some(question.into()),
            pending: true,
            ..Note::new(NoteKind::Answer, String::new())
        }],
    );
    id
}
/// Answers a question you typed that Bit can't answer yet: what it would
/// answer with, and buttons to connect something.
pub fn cant_answer(s: &mut Snapshot, question: &str) {
    let guide = crate::guide::bit_setup(s);
    let text = say::cant_answer(&s.settings.buddy.tone);
    append(
        &mut s.buddy_feed,
        vec![Note {
            question: Some(question.into()),
            lines: guide
                .lines
                .into_iter()
                .map(|t| line(t, None, None))
                .collect(),
            proposals: guide.proposals,
            ..Note::new(NoteKind::Answer, text)
        }],
    );
}
/// Notes that Bit switched to answering with `to`, the one sign-in that's
/// ready, since `from` wasn't.
pub fn switched(s: &mut Snapshot, from: Answerer, to: Answerer) {
    let text = say::switched(&s.settings.buddy.tone, from.label(), to.label());
    append(&mut s.buddy_feed, vec![Note::new(NoteKind::Fixed, text)]);
}
/// Fills in the answer to a question you typed, and makes the changes it
/// asks for in Bit's memory, while the question is still waiting.
pub fn answered(
    s: &mut Snapshot,
    id: u64,
    text: String,
    proposals: Vec<crate::ask::Proposal>,
    memory: Vec<MemoryChange>,
) {
    let feed = &mut s.buddy_feed;
    if let Some(note) = feed.notes.iter_mut().find(|n| n.id == id && n.pending) {
        note.at = now();
        note.text = text;
        note.proposals = proposals;
        note.memory = change_memory(&mut feed.memory, memory);
        note.pending = false;
    }
}
/// Says why Bit couldn't answer a question you typed.
pub fn not_answered(s: &mut Snapshot, id: u64, error: &str) {
    let text = say::no_answer(&s.settings.buddy.tone, error);
    answered(s, id, text, vec![], vec![]);
}
/// A question Orbit was answering when it closed gets no answer.
pub fn recover(feed: &mut Feed) {
    for note in feed.notes.iter_mut().filter(|n| n.pending) {
        note.pending = false;
        note.text = "Orbit closed before I could answer. Ask me again?".into();
    }
}
/// The button `proposal` of the answer `note`.
pub fn proposal(s: &Snapshot, note: u64, proposal: u32) -> Option<&crate::ask::Proposal> {
    s.buddy_feed
        .notes
        .iter()
        .find(|n| n.id == note)?
        .proposals
        .iter()
        .find(|p| p.id == proposal)
}
/// Marks a button you tapped as done.
pub fn proposal_done(s: &mut Snapshot, note: u64, proposal: u32) {
    if let Some(p) = s
        .buddy_feed
        .notes
        .iter_mut()
        .find(|n| n.id == note)
        .and_then(|n| n.proposals.iter_mut().find(|p| p.id == proposal))
    {
        p.done = true;
    }
}

/// A question Bit answers straight from Orbit's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Question {
    NeedsYou,
    Running,
    Loops,
    Repositories,
    /// Where setup stands, with buttons for what's left: the one question
    /// that needs `Lookups`.
    Setup,
}
impl Question {
    pub fn text(self) -> &'static str {
        match self {
            Question::NeedsYou => "What needs me?",
            Question::Running => "What's running?",
            Question::Loops => "How are my loops?",
            Question::Repositories => "How are my repositories?",
            Question::Setup => "Help me set up",
        }
    }
}
/// Answers a question and adds the answer to the feed, where every window
/// sees it.
pub fn ask(s: &mut Snapshot, question: Question, lookups: &crate::ask::Lookups) {
    let (text, lines, proposals) = answer(s, question, lookups);
    append(
        &mut s.buddy_feed,
        vec![Note {
            question: Some(question.text().into()),
            lines,
            proposals,
            ..Note::new(NoteKind::Answer, text)
        }],
    );
}
/// Notes that `repo` was connected, with buttons for its next steps, while
/// Bit is on.
pub fn connected(s: &mut Snapshot, repo: &str, facts: Option<&Result<CloneFacts, String>>) {
    if !s.settings.buddy.enabled {
        return;
    }
    let guide = crate::guide::connected(s, repo, facts);
    let Some(name) = s
        .settings
        .repositories
        .iter()
        .find(|r| same_repo(&r.name, repo))
        .map(|r| r.name.clone())
    else {
        return;
    };
    let text = say::connected(&s.settings.buddy.tone, &name, guide.to_do);
    append(
        &mut s.buddy_feed,
        vec![Note {
            repo: Some(name),
            lines: guide
                .lines
                .into_iter()
                .map(|t| line(t, None, None))
                .collect(),
            proposals: guide.proposals,
            ..Note::new(NoteKind::Connected, text)
        }],
    );
}
fn line(text: String, pr_id: Option<&str>, key: Option<&Attention>) -> NoteLine {
    NoteLine {
        text,
        pr_id: pr_id.map(str::to_owned),
        key: key.map(|a| a.key.clone()),
    }
}
/// An answer that starts by saying autopilot is off, when it is.
fn autopilot_off(s: &Snapshot, text: String) -> String {
    if s.settings.automatic {
        text
    } else {
        format!("Autopilot is off, so nothing is reviewed automatically. {text}")
    }
}
/// What Bit says, a line for each thing it lists, and its buttons.
fn answer(
    s: &Snapshot,
    question: Question,
    lookups: &crate::ask::Lookups,
) -> (String, Vec<NoteLine>, Vec<crate::ask::Proposal>) {
    let tone = s.settings.buddy.tone.as_str();
    let items = crate::attention::items(s);
    let pr_or_id = |id: &str| pr_label(s, Some(id)).unwrap_or_else(|| id.to_string());
    match question {
        Question::NeedsYou => {
            let lines: Vec<NoteLine> = items
                .iter()
                .map(|a| {
                    let text = match &a.pr_id {
                        Some(id) => format!("{}: {}", pr_or_id(id), a.reason),
                        None => a.reason.clone(),
                    };
                    line(text, a.pr_id.as_deref(), Some(a))
                })
                .collect();
            (
                say::needs_you_answer(tone, lines.len() as u32),
                lines,
                vec![],
            )
        }
        Question::Running => {
            let mut lines = vec![];
            if s.polling {
                lines.push(line("Syncing with GitHub.".into(), None, None));
            }
            for r in s.reviews.iter().filter(|r| active(&r.status)) {
                let what = if r.before_follow_up.is_some() {
                    "a follow-up chat".to_string()
                } else {
                    crate::review_loop::round_number(s, r)
                        .map_or_else(|| "a review".into(), |n| format!("round {n}"))
                };
                let state = match r.status.as_str() {
                    "queued" => "is queued",
                    "preparing" => "is getting ready",
                    "awaitingApproval" => "is waiting for your approval",
                    _ => "is running",
                };
                let text = format!("{}: {what} {state}.", pr_or_id(&r.pr_id));
                lines.push(line(text, Some(&r.pr_id), None));
            }
            for scan in &s.scans {
                let state = match scan.status.as_str() {
                    "preparing" | "running" => "is running",
                    "cancelling" => "is stopping",
                    "importing" => "is finishing",
                    _ => continue,
                };
                let text = format!("{}: a security scan {state}.", scan.repo);
                lines.push(line(text, None, None));
            }
            let text = say::running_answer(tone, lines.len() as u32);
            (autopilot_off(s, text), lines, vec![])
        }
        Question::Loops => {
            let open: Vec<LoopState> = crate::review_loop::states(s)
                .into_iter()
                .filter(|l| !l.closed && l.rounds > 0)
                .collect();
            let status = |l: &LoopState| match l.status {
                LoopStatus::NeedsYou => "needs you",
                LoopStatus::Reviewing => "reviewing",
                LoopStatus::WaitingForAuthor => "waiting for the author",
                LoopStatus::Clean => "clean",
                LoopStatus::Stopped => "paused",
            };
            let lines = open
                .iter()
                .map(|l| {
                    let mut text = format!(
                        "{}: round {} of {}, {}.",
                        pr_or_id(&l.pr_id),
                        l.rounds,
                        l.round_limit,
                        status(l)
                    );
                    if let Some(reason) = l
                        .reason
                        .as_deref()
                        .filter(|_| l.status == LoopStatus::NeedsYou)
                    {
                        text.push(' ');
                        text.push_str(reason);
                    }
                    // The item that offers a fix, if the PR has one.
                    let about = |a: &&Attention| a.pr_id.as_deref() == Some(&*l.pr_id);
                    let item = items
                        .iter()
                        .filter(about)
                        .find(|a| a.fix.is_some())
                        .or_else(|| items.iter().find(about));
                    line(text, Some(&l.pr_id), item)
                })
                .collect();
            let mut counts = vec![];
            for kind in [
                LoopStatus::NeedsYou,
                LoopStatus::Reviewing,
                LoopStatus::WaitingForAuthor,
                LoopStatus::Clean,
                LoopStatus::Stopped,
            ] {
                let n = open.iter().filter(|l| l.status == kind).count();
                if n > 0 {
                    let label = open.iter().find(|l| l.status == kind).map(status);
                    counts.push(format!("{n} {}", label.unwrap_or_default()));
                }
            }
            let summary = (!open.is_empty()).then(|| {
                format!(
                    "{}: {}.",
                    plural(open.len() as u32, "loop is open", "loops are open"),
                    counts.join(", ")
                )
            });
            (say::loops_answer(tone, summary.as_deref()), lines, vec![])
        }
        Question::Repositories => {
            let mut lines = vec![];
            if let Some(github) = items.iter().find(|a| a.key == "sync:github") {
                lines.push(line(github.reason.clone(), None, Some(github)));
            }
            for repo in &s.settings.repositories {
                let open = s
                    .prs
                    .iter()
                    .filter(|p| same_repo(&p.repo, &repo.name))
                    .count();
                let mut text = format!(
                    "{}: {}, autopilot {}.",
                    repo.name,
                    plural(open as u32, "open pull request", "open pull requests"),
                    if repo.automatic { "on" } else { "off" }
                );
                if let Some(f) = s
                    .sync_failures
                    .iter()
                    .find(|f| same_repo(&f.repo, &repo.name))
                {
                    let syncs = match f.count {
                        1 => "sync".to_string(),
                        n => format!("{n} syncs"),
                    };
                    text.push_str(&format!(" The last {syncs} failed: {}", f.error));
                }
                let ci =
                    s.ci.iter()
                        .find(|c| same_repo(&c.repo, &repo.name))
                        .filter(|_| s.settings.watch_ci);
                if let Some(ci) = ci {
                    text.push_str(&ci_sentence(ci));
                }
                // A failing sync first: CI can't be read without one.
                let about = |kind: AttentionKind| {
                    items.iter().find(|a| {
                        a.kind == kind
                            && a.repo.as_deref().is_some_and(|r| same_repo(r, &repo.name))
                    })
                };
                let item = about(AttentionKind::SyncFailing).or(about(AttentionKind::CiFailing));
                lines.push(line(text, None, item));
            }
            let text = say::repositories_answer(tone, s.settings.repositories.len() as u32);
            (autopilot_off(s, text), lines, vec![])
        }
        Question::Setup => {
            let guide = crate::guide::setup(s, lookups);
            let lines = guide
                .lines
                .into_iter()
                .map(|t| line(t, None, None))
                .collect();
            (say::setup_answer(tone, guide.to_do), lines, guide.proposals)
        }
    }
}

/// How a repository's default branch's CI stands, as a sentence to follow
/// its line in How are my repositories, or nothing when nothing ran.
fn ci_sentence(ci: &crate::ci::RepoCi) -> String {
    use crate::ci::CiState;
    let branch = if ci.branch.is_empty() {
        "the default branch"
    } else {
        &ci.branch
    };
    match ci.head.state {
        _ if ci.head.sha.is_empty() => String::new(),
        CiState::Running if ci.head.incomplete => {
            format!(" Orbit couldn't read every check on {branch}.")
        }
        CiState::Failing => format!(
            " CI is failing on {branch}: {}.",
            crate::problems::names(&ci.head.failing)
        ),
        CiState::Running => format!(" CI is running on {branch}."),
        CiState::Passing => format!(" CI is passing on {branch}."),
        CiState::None => String::new(),
    }
}

/// How Bit feels about the loops: worried while anything needs you, busy
/// while a review runs, sleepy with autopilot off or nothing open, and happy
/// otherwise.
pub fn mood(s: &Snapshot, attention: &[Attention], loops: &[LoopState]) -> Mood {
    if !attention.is_empty() {
        Mood::Worried
    } else if s.reviews.iter().any(|r| active(&r.status))
        || loops
            .iter()
            .any(|l| !l.closed && l.status == LoopStatus::Reviewing)
    {
        Mood::Busy
    } else if !s.settings.automatic || s.prs.is_empty() {
        Mood::Sleepy
    } else {
        Mood::Happy
    }
}

#[cfg(test)]
#[path = "buddy_tests.rs"]
mod tests;
