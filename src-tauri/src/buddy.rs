//! Bit, Orbit's buddy: its settings, a feed of short notes about the review
//! loops in its own voice, a mood that follows them, the fixes it offers for
//! what needs you, and quick answers about Orbit. Everything here is worked
//! out from what Orbit already tracks; nothing here calls a model.
use crate::types::*;
use serde::{Deserialize, Serialize};

/// The colours Bit can be, as the app names them. The app maps each to a
/// theme token.
pub const COLOURS: [&str; 20] = [
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
/// How Bit's notes sound.
pub const TONES: [&str; 3] = ["cheerful", "calm", "dry"];
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
}

/// The rule for each fix Bit can do on its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rules {
    pub continue_loop: String,
    pub review_again: String,
    pub publish: String,
    pub sync_now: String,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            continue_loop: "ask_first".into(),
            review_again: "ask_first".into(),
            publish: "ask_first".into(),
            sync_now: "ask_first".into(),
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
            Fix::TurnOnAutopilot => "ask_first",
        }
    }
    fn each(&self) -> [&str; 4] {
        [
            &self.continue_loop,
            &self.review_again,
            &self.publish,
            &self.sync_now,
        ]
    }
    fn each_mut(&mut self) -> [&mut String; 4] {
        [
            &mut self.continue_loop,
            &mut self.review_again,
            &mut self.publish,
            &mut self.sync_now,
        ]
    }
}
/// Only supplied rules change.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RulesPatch {
    pub continue_loop: Option<String>,
    pub review_again: Option<String>,
    pub publish: Option<String>,
    pub sync_now: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Buddy {
    pub enabled: bool,
    pub name: String,
    pub colour: String,
    pub hat: String,
    pub tone: String,
    /// Saved now, used once Bit can chat.
    pub personality: String,
    pub rules: Rules,
}
impl Default for Buddy {
    fn default() -> Self {
        Self {
            enabled: true,
            name: "Bit".into(),
            colour: "lime".into(),
            hat: "none".into(),
            tone: "cheerful".into(),
            personality: String::new(),
            rules: Rules::default(),
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
        Ok(())
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
    pub tone: Option<String>,
    pub personality: Option<String>,
    pub rules: Option<RulesPatch>,
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
        if let Some(v) = self.tone {
            buddy.tone = v;
        }
        if let Some(v) = self.personality {
            buddy.personality = v;
        }
        if let Some(rules) = self.rules {
            let patch = [
                rules.continue_loop,
                rules.review_again,
                rules.publish,
                rules.sync_now,
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
            ("dry", Fix::ContinueLoop) => format!("{pr} gets two more rounds."),
            ("dry", Fix::ReviewAgain) => format!("{pr} is queued for another go."),
            ("dry", Fix::Publish) => format!("{pr}'s report is out."),
            ("dry", Fix::SyncNow) => "Synced. Finally.".into(),
            ("dry", Fix::TurnOnAutopilot) => "Autopilot: on.".into(),
            (_, Fix::ContinueLoop) => format!("Gave {pr} two more rounds!"),
            (_, Fix::ReviewAgain) => format!("Queued a new review of {pr}. Fingers crossed!"),
            (_, Fix::Publish) => format!("Published the report on {pr}! 📬"),
            (_, Fix::SyncNow) => "Synced with GitHub. All good again!".into(),
            (_, Fix::TurnOnAutopilot) => "Autopilot is back on!".into(),
        }
    }
    pub fn not_done(tone: &str, fix: Fix, pr: &str, error: &str) -> String {
        let what = match fix {
            Fix::ContinueLoop => format!("continue the loop on {pr}"),
            Fix::ReviewAgain => format!("review {pr} again"),
            Fix::Publish => format!("publish the report on {pr}"),
            Fix::SyncNow => "sync with GitHub".into(),
            Fix::TurnOnAutopilot => "turn autopilot back on".into(),
        };
        let error = error.trim();
        match tone {
            "calm" => format!("Couldn't {what}: {error}"),
            "dry" => format!("Tried to {what}. No luck: {error}"),
            _ => format!("I tried to {what}, but it didn't work: {error}"),
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
pub(crate) const FIXES: [Fix; 5] = [
    Fix::ContinueLoop,
    Fix::ReviewAgain,
    Fix::Publish,
    Fix::SyncNow,
    Fix::TurnOnAutopilot,
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
    let excess = feed.notes.len().saturating_sub(MAX_NOTES);
    feed.notes.drain(..excess);
}

/// The fix Bit does on its own for an item that just opened: one whose rule
/// is Do it, while Bit is on.
pub fn will_fix(s: &Snapshot, item: &Attention) -> Option<Fix> {
    let buddy = &s.settings.buddy;
    item.fix
        .filter(|&fix| buddy.enabled && buddy.rules.of(fix) == "do_it")
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

/// A question Bit answers straight from Orbit's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Question {
    NeedsYou,
    Running,
    Loops,
    Repositories,
}
impl Question {
    pub fn text(self) -> &'static str {
        match self {
            Question::NeedsYou => "What needs me?",
            Question::Running => "What's running?",
            Question::Loops => "How are my loops?",
            Question::Repositories => "How are my repositories?",
        }
    }
}
/// Answers a question and adds the answer to the feed, where every window
/// sees it.
pub fn ask(s: &mut Snapshot, question: Question) {
    let (text, lines) = answer(s, question);
    append(
        &mut s.buddy_feed,
        vec![Note {
            question: Some(question.text().into()),
            lines,
            ..Note::new(NoteKind::Answer, text)
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
/// What Bit says, and a line for each thing it lists.
fn answer(s: &Snapshot, question: Question) -> (String, Vec<NoteLine>) {
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
            (say::needs_you_answer(tone, lines.len() as u32), lines)
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
            (autopilot_off(s, text), lines)
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
            (say::loops_answer(tone, summary.as_deref()), lines)
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
                let item = items.iter().find(|a| {
                    a.kind == AttentionKind::SyncFailing
                        && a.repo.as_deref().is_some_and(|r| same_repo(r, &repo.name))
                });
                lines.push(line(text, None, item));
            }
            let text = say::repositories_answer(tone, s.settings.repositories.len() as u32);
            (autopilot_off(s, text), lines)
        }
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
