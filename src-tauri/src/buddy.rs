//! Bit, Orbit's buddy: its settings, a feed of short notes about the review
//! loops in its own voice, and a mood that follows them. Notes are written
//! from changes Orbit already tracks; nothing here calls a model.
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
    use super::{plural, Outcome};

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
    ]
}

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
        let mut next_id = s.buddy_feed.next_id;
        let mut add = |kind, pr_id, review_id, repo, text| {
            written.push(Note {
                id: next_id,
                at: now(),
                kind,
                pr_id,
                review_id,
                repo,
                text,
            });
            next_id += 1;
        };
        for (kind, pr, review_id, text) in notes {
            add(kind, Some(pr.id.clone()), review_id, None, text);
        }
        for item in fresh {
            let pr = s
                .prs
                .iter()
                .chain(&s.history_prs)
                .find(|p| Some(&p.id) == item.pr_id.as_ref())
                .map(label);
            add(
                NoteKind::NeedsYou,
                item.pr_id.clone(),
                item.review_id.clone(),
                item.repo.clone(),
                say::needs_you(&tone, pr.as_deref(), &item.reason),
            );
        }
        s.buddy_feed.next_id = next_id;
    }
    let changed = !written.is_empty() || loops != s.buddy_feed.loops || !s.buddy_feed.seeded;
    s.buddy_feed.loops = loops;
    s.buddy_feed.seeded = true;
    s.buddy_feed.notes.extend(written);
    let excess = s.buddy_feed.notes.len().saturating_sub(MAX_NOTES);
    s.buddy_feed.notes.drain(..excess);
    changed
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
