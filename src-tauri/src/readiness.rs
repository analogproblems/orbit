//! Whether a pull request is clear to merge, as the reviewer of record
//! sees it: Orbit's loop is clean on its current head and base, every check
//! on that head passed, they ran with its base's latest commit, and GitHub
//! says it can merge. Otherwise, what is in the way, first things first.
//! Only pull requests Orbit reviews automatically have an answer. Orbit
//! never merges: this is what it tells you and, later, the agent.
//!
//! GitHub works out whether a pull request merges in the background and
//! only says so when asked about that one pull request, so Orbit asks
//! (`read`) only once nothing else is in the way, a few a pass, and again
//! when its head or its base's latest commit moves. It reads who was asked
//! to review it and their reviews then too, and whether its base branch's
//! rules need approvals: where they do, only approvals of its latest
//! commit count.
use crate::{
    ci::{short, CiState, RepoCi},
    logs,
    problems::{stamp, time},
    types::*,
    workflows,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Pull requests Orbit asks GitHub about in one pass.
pub const PER_PASS: usize = 4;
/// How often Orbit asks again about a pull request that hasn't moved, as a
/// rule or a review on GitHub can change the answer.
pub const EVERY: Duration = Duration::minutes(10);

/// What is in the way of merging.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitKind {
    /// Orbit hasn't finished reviewing the current head and base.
    Review,
    /// Blocking findings wait on the agent.
    Findings,
    /// Orbit's loop needs a person, or was stopped.
    Loop,
    /// A check failed, is still running, or hasn't been read.
    Checks,
    /// The checks ran without the base's latest commit, or Orbit can't
    /// tell.
    Base,
    /// GitHub says it can't merge yet, or hasn't said.
    Merge,
    /// The approvals its base branch's rules need aren't all of its latest
    /// commit, or a reviewer asked for changes.
    Reviews,
}
/// Whose move it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Turn {
    /// The pull request's author, or their agent: answer findings, fix a
    /// check, or bring it up to date with its base.
    Author,
    /// A person: you, or whoever a rule on GitHub waits for.
    Person,
    /// Nobody yet: Orbit, CI or GitHub is still at work.
    Working,
}
/// One thing in the way, in Orbit's words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wait {
    pub kind: WaitKind,
    pub turn: Turn,
    pub text: String,
}
fn wait(kind: WaitKind, turn: Turn, text: impl Into<String>) -> Wait {
    Wait {
        kind,
        turn,
        text: text.into(),
    }
}
/// Whether a pull request is clear to merge at its current head.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Readiness {
    pub pr_id: String,
    pub repo: String,
    pub number: u64,
    /// The head it was worked out for.
    pub head_sha: String,
    /// The base branch, and the latest commit on it the checks ran with,
    /// once Orbit knows it.
    pub base: String,
    pub base_sha: String,
    /// Nothing is in the way.
    pub clear: bool,
    /// What is in the way, first things first.
    pub waits: Vec<Wait>,
    /// Who was asked to review it and where each stands, and what its base
    /// branch's rules need, once Orbit has asked GitHub at this head and
    /// base.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviews: Option<Reviews>,
}
impl Readiness {
    /// The answer in one line: "Clear to merge at abc1234 against main at
    /// def5678." or "Not clear to merge yet. Checks are still running on
    /// abc1234."
    pub fn line(&self) -> String {
        match self.waits.first() {
            None => {
                let line = format!(
                    "Clear to merge at {} against {} at {}.",
                    short(&self.head_sha),
                    self.base,
                    short(&self.base_sha)
                );
                match self.note() {
                    Some(note) => format!("{line} {note}"),
                    None => line,
                }
            }
            Some(w) => format!("Not clear to merge yet. {}", w.text),
        }
    }
    /// What Orbit couldn't check on a pull request that is clear: whether
    /// its approvals are of its latest commit, where Orbit can't see its
    /// base branch's rules.
    pub fn note(&self) -> Option<String> {
        let unseen = self.reviews.as_ref().is_some_and(|r| r.rule.is_none());
        (self.clear && unseen).then(|| {
            format!(
                "Orbit can't see {}'s rules, so it left its reviews to GitHub.",
                self.base
            )
        })
    }
}

/// What GitHub last said about whether a pull request can merge.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Merge {
    pub pr_id: String,
    /// The head Orbit asked about, and its base branch's latest commit
    /// then, which can make a conflict without the pull request changing.
    pub head_sha: String,
    pub base_sha: String,
    /// When Orbit last asked, by Orbit's clock.
    pub read_at: String,
    /// GitHub's `mergeable`: None while it is still working it out.
    pub mergeable: Option<bool>,
    /// GitHub's `mergeable_state`, such as clean, dirty, blocked or behind.
    pub state: String,
    /// Why the last read failed, how many reads in a row failed or found
    /// GitHub still working it out, and when Orbit asks again.
    pub error: Option<String>,
    pub tries: u32,
    pub retry_at: Option<String>,
    /// Who was asked to review it and their reviews, as GitHub said then.
    /// None in answers from before Orbit read them.
    pub reviews: Option<Reviews>,
}
impl Merge {
    /// GitHub hasn't worked it out yet.
    fn pending(&self) -> bool {
        self.mergeable.is_none() || self.state == "unknown"
    }
    /// GitHub says a rule blocks merging it.
    fn blocked(&self) -> bool {
        self.state == "blocked"
    }
}

/// Who was asked to review a pull request, or reviewed it, and what its
/// base branch's rules need.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Reviews {
    pub reviewers: Vec<Reviewer>,
    /// What the rules need, or None when Orbit can't see them, with why in
    /// `unseen`.
    pub rule: Option<Rule>,
    pub unseen: Option<String>,
    /// Why Orbit couldn't read the reviews, as GitHub said, so `reviewers`
    /// is empty.
    pub unread: Option<String>,
    /// It has more reviews than Orbit reads, so `reviewers` is empty.
    pub too_many: bool,
}
/// Someone asked to review a pull request, or who reviewed it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Reviewer {
    /// A login, or `owner/slug` for a team. Someone else's words.
    pub name: String,
    pub team: bool,
    pub stand: Stand,
    /// The commit their latest approval or request for changes was of.
    pub commit: String,
    /// For an approval of the latest commit or a request for changes,
    /// whether they have write access, without which GitHub's rules don't
    /// count it. None when Orbit couldn't tell, or didn't need to ask.
    pub may_approve: Option<bool>,
}
impl Reviewer {
    /// An approval of the latest commit that counts toward the rules.
    fn counts(&self) -> bool {
        self.stand == Stand::Approved && self.may_approve == Some(true)
    }
    /// A request for changes that can hold it back: from someone with
    /// write access, or someone Orbit can't tell about.
    fn blocks(&self) -> bool {
        self.stand == Stand::ChangesRequested && self.may_approve != Some(false)
    }
}
/// Where a reviewer stands on a pull request's latest commit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stand {
    /// Asked, and hasn't reviewed since.
    #[default]
    Asked,
    /// Approved the latest commit.
    Approved,
    /// Approved an older commit, which doesn't count.
    ApprovedOlder,
    /// Asked for changes, which stand until they review again.
    ChangesRequested,
}
/// What a base branch's rules need before a pull request merges.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rule {
    /// Approvals, which Orbit counts only of the latest commit.
    pub approvals: u32,
    /// A code owner's approval, which Orbit leaves to GitHub.
    pub code_owners: bool,
}
impl Rule {
    fn needs_reviews(&self) -> bool {
        self.approvals > 0 || self.code_owners
    }
}

/// The pull requests Orbit is the reviewer of record for: open, not
/// drafts, from a source Orbit trusts, in a repository it reviews
/// automatically.
fn reviewed(s: &Snapshot) -> impl Iterator<Item = &PullRequest> {
    s.prs.iter().filter(|pr| {
        !pr.draft
            && trusted_source(pr, &s.settings.trusted_authors)
            && s.settings
                .repositories
                .iter()
                .any(|r| same_repo(&r.name, &pr.repo) && r.automatic)
    })
}
/// Every reviewed pull request's answer.
pub fn all(s: &Snapshot) -> Vec<Readiness> {
    reviewed(s).map(|pr| of(s, pr)).collect()
}
/// A pull request's answer.
pub fn of(s: &Snapshot, pr: &PullRequest) -> Readiness {
    let mut answer = Readiness {
        pr_id: pr.id.clone(),
        repo: pr.repo.clone(),
        number: pr.number,
        head_sha: pr.head_sha.clone(),
        base: pr.base_ref.clone(),
        base_sha: String::new(),
        clear: false,
        waits: vec![],
        reviews: None,
    };
    let reviewed = review(s, pr);
    let checked = checks(s, pr);
    let clean = reviewed.is_none() && checked.waits.is_empty();
    answer.waits.extend(reviewed);
    answer.waits.extend(checked.waits);
    let base_sha = checked.base_sha;
    // A clean loop reviewed the pull request with the base commit GitHub
    // gave Orbit for it; the checks must have run with that same commit.
    if clean && base_sha != pr.base_sha {
        let base = &pr.base_ref;
        answer.waits.push(wait(
            WaitKind::Review,
            Turn::Working,
            format!(
                "Orbit reviewed it with {base} at {}, and {base} is at {} now, so it waits for a review with the latest {base}.",
                short(&pr.base_sha),
                short(&base_sha)
            ),
        ));
    }
    if answer.waits.is_empty() {
        let merge = s.merges.iter().find(|m| asked(m, pr, &base_sha));
        let reviews = merge.and_then(|m| m.reviews.as_ref());
        let github = merging(merge, pr);
        let people = reviews.and_then(|r| reviewing(r, pr, merge.is_some_and(Merge::blocked)));
        // GitHub still at work, a conflict or being behind come first, and
        // reviews in the way say more than that a rule blocks it, unless
        // Orbit couldn't read them.
        let blocked = merge.is_some_and(Merge::blocked);
        match (github, people) {
            (Some((Turn::Person, _)), Some((turn, text))) if blocked && turn != Turn::Working => {
                answer.waits.push(wait(WaitKind::Reviews, turn, text));
            }
            (Some((turn, text)), _) => answer.waits.push(wait(WaitKind::Merge, turn, text)),
            (None, Some((turn, text))) => answer.waits.push(wait(WaitKind::Reviews, turn, text)),
            (None, None) => {}
        }
        answer.reviews = reviews.cloned();
    }
    answer.base_sha = base_sha;
    answer.clear = answer.waits.is_empty();
    answer
}

/// What Orbit's loop says, when it is in the way.
fn review(s: &Snapshot, pr: &PullRequest) -> Option<Wait> {
    let head = short(&pr.head_sha);
    let reviewing = |text: String| Some(wait(WaitKind::Review, Turn::Working, text));
    let Some(state) = crate::review_loop::state(s, pr) else {
        return reviewing(format!("Orbit hasn't reviewed {head} yet."));
    };
    let latest = state
        .round_ids
        .last()
        .and_then(|id| s.reviews.iter().find(|r| &r.id == id));
    let current = latest.is_some_and(|r| r.head_sha == pr.head_sha && r.base_sha == pr.base_sha);
    match state.status {
        LoopStatus::Clean => None,
        LoopStatus::Reviewing if current && latest.is_some_and(|r| active(&r.status)) => {
            reviewing(format!("Orbit is reviewing {head}."))
        }
        LoopStatus::Reviewing if current => {
            reviewing(format!("Orbit is publishing its review of {head}."))
        }
        LoopStatus::Reviewing => reviewing(format!("Orbit hasn't reviewed {head} yet.")),
        LoopStatus::WaitingForAuthor => {
            let n = latest.and_then(|r| r.blocking).unwrap_or(0);
            let findings = match n {
                1 => "1 blocking finding".to_string(),
                n => format!("{n} blocking findings"),
            };
            let verb = if n == 1 { "waits" } else { "wait" };
            Some(wait(
                WaitKind::Findings,
                Turn::Author,
                format!("{findings} from Orbit's review of {head} {verb} on the author."),
            ))
        }
        LoopStatus::NeedsYou => Some(wait(
            WaitKind::Loop,
            Turn::Person,
            match &state.reason {
                Some(reason) => format!("Orbit's review loop needs you: {reason}"),
                None => "Orbit's review loop needs you.".into(),
            },
        )),
        LoopStatus::Stopped => Some(wait(
            WaitKind::Loop,
            Turn::Person,
            "Orbit's review loop on it is stopped.",
        )),
    }
}

/// What the checks say, and the base branch's latest commit they ran
/// with, when Orbit knows it.
struct Checked {
    waits: Vec<Wait>,
    base_sha: String,
}
fn checks(s: &Snapshot, pr: &PullRequest) -> Checked {
    let head = short(&pr.head_sha);
    let waiting = |kind, turn, text: String| Checked {
        waits: vec![wait(kind, turn, text)],
        base_sha: String::new(),
    };
    if !s.settings.watch_ci {
        return waiting(
            WaitKind::Checks,
            Turn::Person,
            "Orbit can't see its checks while Watch CI is off.".into(),
        );
    }
    let repo = s.ci.iter().find(|c| same_repo(&c.repo, &pr.repo));
    let pull = repo
        .and_then(|c| c.pull(pr.number))
        .filter(|p| p.ci.sha == pr.head_sha);
    let (Some(repo), Some(pull)) = (repo, pull) else {
        return waiting(
            WaitKind::Checks,
            Turn::Working,
            format!("Orbit hasn't read the checks on {head} yet."),
        );
    };
    let ci = &pull.ci;
    let base = &pr.base_ref;
    let on_default = *base == repo.branch;
    let waived = match ci.state {
        CiState::Failing => {
            let verb = if ci.failing.len() == 1 {
                "fails"
            } else {
                "fail"
            };
            return waiting(
                WaitKind::Checks,
                Turn::Author,
                format!("{} {verb} on {head}.", crate::problems::names(&ci.failing)),
            );
        }
        CiState::Running if ci.incomplete && ci.running == 0 => {
            return waiting(
                WaitKind::Checks,
                Turn::Working,
                format!("Orbit couldn't read every check on {head}."),
            );
        }
        CiState::Running => {
            return waiting(
                WaitKind::Checks,
                Turn::Working,
                format!("Checks are still running on {head}."),
            );
        }
        // Without CI on pull requests there are no checks to wait for, nor
        // a run to have missed the base.
        CiState::None if no_ci(s, repo) => true,
        CiState::None => {
            return waiting(
                WaitKind::Checks,
                Turn::Working,
                format!("No checks have reported on {head} yet."),
            );
        }
        CiState::Passing => false,
    };
    // What Orbit knows of CI is the default branch's.
    if !on_default {
        return waiting(
            WaitKind::Base,
            Turn::Person,
            format!(
                "Orbit can't tell whether its checks ran with the latest {base}: it only follows the default branch."
            ),
        );
    }
    if waived {
        return Checked {
            waits: vec![],
            base_sha: repo.head.sha.clone(),
        };
    }
    match crate::problems::behind(repo, pr, ci) {
        Some(false) => Checked {
            waits: vec![],
            base_sha: repo.head.sha.clone(),
        },
        Some(true) => waiting(
            WaitKind::Base,
            Turn::Author,
            format!("{base} moved on since its checks ran: merge {base} in and let them run again."),
        ),
        None => waiting(
            WaitKind::Base,
            Turn::Person,
            format!(
                "Orbit can't tell whether its checks ran with the latest {base}, as GitHub didn't say when they began."
            ),
        ),
    }
}
/// Whether Orbit is sure no CI runs on the repository's pull requests: it
/// read so at the default branch's latest commit, has never seen checks
/// run there, and its problem about it is open at that commit, which
/// `problems::see_setup` opens only once other services have had time to
/// report checks.
fn no_ci(s: &Snapshot, ci: &RepoCi) -> bool {
    let now = |sha: &str, branch: &str| sha == ci.head.sha && branch == ci.branch;
    let read = s.setups.iter().any(|w| {
        same_repo(&w.repo, &ci.repo)
            && now(&w.sha, &w.branch)
            && w.ran.is_none()
            && matches!(
                w.verdict(),
                Some(workflows::Verdict::NoWorkflows | workflows::Verdict::NotOnPulls)
            )
    });
    let key = crate::problems::setup_key(&ci.repo);
    read && s
        .problems
        .iter()
        .any(|p| p.key == key && p.is_open() && now(&p.sha, &p.branch))
}

/// Whether Orbit's question to GitHub was about this head and base.
fn asked(m: &Merge, pr: &PullRequest, base_sha: &str) -> bool {
    m.pr_id == pr.id && m.head_sha == pr.head_sha && m.base_sha == base_sha
}
/// What GitHub's answer says, and whose move it is, when it is in the
/// way.
fn merging(merge: Option<&Merge>, pr: &PullRequest) -> Option<(Turn, String)> {
    let base = &pr.base_ref;
    let working = |text: &str| Some((Turn::Working, text.to_string()));
    let Some(m) = merge else {
        return working("Orbit hasn't asked GitHub yet whether it merges.");
    };
    if m.read_at.is_empty() {
        if let Some(error) = &m.error {
            return working(error);
        }
    }
    if m.pending() {
        return working("GitHub is still checking whether it merges.");
    }
    if m.mergeable == Some(false) || m.state == "dirty" {
        return Some((
            Turn::Author,
            format!("It conflicts with {base}: merge {base} in and fix the conflicts."),
        ));
    }
    let person = |text: String| Some((Turn::Person, text));
    match m.state.as_str() {
        "clean" | "has_hooks" => None,
        "behind" => Some((
            Turn::Author,
            format!("GitHub wants it brought up to date with {base} first."),
        )),
        "blocked" => {
            person("GitHub says a rule blocks merging it, such as a required review.".into())
        }
        "unstable" => Some((
            Turn::Author,
            "GitHub says a check on it isn't passing.".into(),
        )),
        "draft" => Some((Turn::Author, "GitHub says it is a draft.".into())),
        other => person(format!("GitHub says it can't merge yet ({other}).")),
    }
}

/// What the reviews say, and whose move it is, when they are in the way.
/// Only where the base branch's rules need reviews, or, where Orbit can't
/// see them, once GitHub says a rule blocks merging it. Only approvals of
/// the latest commit count, and a request for changes stands until its
/// reviewer reviews again.
fn reviewing(r: &Reviews, pr: &PullRequest, blocked: bool) -> Option<(Turn, String)> {
    let head = short(&pr.head_sha);
    let base = &pr.base_ref;
    match &r.rule {
        Some(rule) if !rule.needs_reviews() => return None,
        None if !blocked => return None,
        _ => {}
    }
    if r.too_many {
        return Some((
            Turn::Person,
            format!("It has more reviews than Orbit reads, so Orbit can't tell whether enough people approved {head}."),
        ));
    }
    if let Some(why) = &r.unread {
        return Some((
            Turn::Working,
            format!("Orbit couldn't read its reviews, so it can't tell yet whether enough people approved {head}: {why}."),
        ));
    }
    let changes = r.reviewers.iter().find(|x| x.blocks());
    if let Some(x) = changes {
        if x.commit == pr.head_sha {
            return Some((
                Turn::Author,
                format!(
                    "{} asked for changes on {head}: answer their review.",
                    x.name
                ),
            ));
        }
        return Some((Turn::Person, format!("{}.", who(x, head))));
    }
    let pending: Vec<String> = r
        .reviewers
        .iter()
        .filter(|x| !x.counts())
        .map(|x| who(x, head))
        .collect();
    let Some(rule) = &r.rule else {
        return (!pending.is_empty()).then(|| {
            (
                Turn::Person,
                format!(
                    "GitHub says a rule blocks merging it, and Orbit can't see {base}'s rules to tell which. So far, {}.",
                    and(&pending)
                ),
            )
        });
    };
    let approved = r.reviewers.iter().filter(|x| x.counts()).count();
    let needed = rule.approvals as usize;
    if approved >= needed {
        return None;
    }
    let approvals = match needed {
        1 => "1 approval".to_string(),
        n => format!("{n} approvals"),
    };
    let has = match approved {
        0 => "has none".to_string(),
        n => format!("has {n}"),
    };
    let rest = match (pending.is_empty(), approved) {
        (false, _) => and(&pending),
        (true, 0) => "nobody is asked to review it".to_string(),
        (true, _) => "nobody else is asked to review it".to_string(),
    };
    Some((
        Turn::Person,
        format!("{base} needs {approvals} of {head} and it {has}: {rest}."),
    ))
}
/// Where someone whose approval of the latest commit doesn't count yet
/// stands, in a clause.
fn who(x: &Reviewer, head: &str) -> String {
    let name = if x.team {
        format!("the {} team", x.name)
    } else {
        x.name.clone()
    };
    let older = short(&x.commit);
    match (x.stand, x.may_approve) {
        (Stand::Asked, _) => format!("{name} hasn't reviewed {head} yet"),
        (Stand::ApprovedOlder, _) => format!("{name} approved {older} but not {head}"),
        (Stand::ChangesRequested, Some(false)) => format!(
            "{name} asked for changes on {older}, but without write access that doesn't hold it back"
        ),
        (Stand::ChangesRequested, _) => {
            format!("{name} asked for changes on {older} and hasn't reviewed {head} since")
        }
        (Stand::Approved, Some(true)) => format!("{name} approved {head}"),
        (Stand::Approved, Some(false)) => {
            format!("{name} approved {head}, but without write access that doesn't count")
        }
        (Stand::Approved, None) => format!(
            "{name} approved {head}, but Orbit can't tell whether they have write access"
        ),
    }
}
/// "a", "a and b", "a, b and c".
fn and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The pull requests Orbit should ask GitHub about now, at most
/// `PER_PASS`, with the base branch's latest commit to ask at: those
/// with nothing else in the way whose answer is for another head or base,
/// or `EVERY` old, or, while GitHub was still working it out or the read
/// failed, whose wait is over. Never asked first, then the oldest.
pub fn wanted(s: &Snapshot, now: DateTime<Utc>) -> Vec<(String, u64, String, String)> {
    let mut due: Vec<(String, (String, u64, String, String))> = reviewed(s)
        .filter_map(|pr| {
            let answer = of(s, pr);
            // Reviews in the way come from GitHub's answer too.
            let github = |w: &Wait| matches!(w.kind, WaitKind::Merge | WaitKind::Reviews);
            if !answer.waits.iter().all(github) {
                return None;
            }
            let ask = (
                pr.repo.clone(),
                pr.number,
                pr.head_sha.clone(),
                answer.base_sha.clone(),
            );
            let Some(m) = s.merges.iter().find(|m| asked(m, pr, &answer.base_sha)) else {
                return Some((String::new(), ask));
            };
            // Only a definite answer has no wait set.
            let ready = match m.retry_at.as_deref().and_then(time) {
                Some(retry) => now >= retry,
                None => time(&m.read_at).is_none_or(|at| now - at >= EVERY),
            };
            ready.then(|| (m.read_at.clone(), ask))
        })
        .collect();
    due.sort_by(|a, b| a.0.cmp(&b.0));
    due.into_iter().take(PER_PASS).map(|(_, ask)| ask).collect()
}

/// Asks GitHub whether a pull request can merge, who was asked to review
/// it, its reviews, and what its base branch's rules need. An answer about
/// another head or base commit is an error, as the pull request moved
/// since Orbit last synced, and so is a list of reviews Orbit can't read
/// in full. Rules Orbit can't see are said so instead.
pub async fn read(
    repo: &str,
    number: u64,
    head_sha: &str,
    base_sha: &str,
) -> Result<Merge, String> {
    validate_repo(repo)?;
    let v = crate::github::api(&format!("repos/{repo}/pulls/{number}"), "GET", &[], None).await?;
    let mut m = understand(&v, &pr_id(repo, number), head_sha, base_sha, Utc::now())?;
    // GitHub's answer stands whatever else Orbit can't read.
    let reviews = read_pages(&format!("repos/{repo}/pulls/{number}/reviews"), PAGES).await;
    let rule = match v["base"]["ref"].as_str() {
        Some(branch) => read_rule(repo, branch).await,
        None => Err(
            "GitHub didn't say its base branch, so Orbit can't tell whether it needs reviews."
                .into(),
        ),
    };
    let mut found = Reviews {
        unseen: rule.as_ref().err().cloned(),
        rule: rule.ok(),
        ..Default::default()
    };
    match reviews {
        Ok(Some(reviews)) => {
            found.reviewers = reviewers(&v, &reviews, head_sha);
            if found.rule.as_ref().is_none_or(Rule::needs_reviews) {
                check_access(repo, &mut found.reviewers).await;
            }
        }
        Ok(None) => found.too_many = true,
        Err(e) => found.unread = Some(logs::short(&e)),
    }
    m.reviews = Some(found);
    Ok(m)
}
/// Pages of a list Orbit reads, 100 to a page.
const PAGES: u32 = 3;
/// Every page of a list, or None when it has more than `pages` full ones.
async fn read_pages(endpoint: &str, pages: u32) -> Result<Option<Vec<Value>>, String> {
    let mut all = vec![];
    for page in 1..=pages {
        let v = crate::github::api(
            endpoint,
            "GET",
            &[("per_page", "100".into()), ("page", page.to_string())],
            None,
        )
        .await?;
        let items = v.as_array().ok_or("Invalid list")?;
        all.extend(items.iter().cloned());
        if items.len() < 100 {
            return Ok(Some(all));
        }
    }
    Ok(None)
}
/// Reviewers whose write access Orbit asks GitHub about, at most.
const ACCESS_CHECKS: usize = 10;
/// Asks GitHub whether each reviewer whose approval of the latest commit
/// or request for changes the rules may count has write access. Those
/// Orbit can't ask about, or GitHub doesn't answer for, stay unknown.
async fn check_access(repo: &str, reviewers: &mut [Reviewer]) {
    let asked = reviewers
        .iter_mut()
        .filter(|x| matches!(x.stand, Stand::Approved | Stand::ChangesRequested))
        .filter(|x| plain_login(&x.name))
        .take(ACCESS_CHECKS);
    for x in asked {
        let endpoint = format!("repos/{repo}/collaborators/{}/permission", x.name);
        let v = crate::github::api(&endpoint, "GET", &[], None).await;
        x.may_approve = v.ok().and_then(|v| access(&v));
    }
}
/// A GitHub login, safe in a path.
fn plain_login(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
/// Whether GitHub's answer about someone's permission gives them write
/// access, which an approval needs to count toward a branch's rules.
pub fn access(v: &Value) -> Option<bool> {
    match v["permission"].as_str()? {
        "admin" | "maintain" | "write" => Some(true),
        "triage" | "read" | "none" => Some(false),
        _ => None,
    }
}
/// What a base branch's rules need, from its protection and the rulesets
/// on it, or why Orbit can't see them. Its protection takes an admin's
/// sign-in to read.
async fn read_rule(repo: &str, branch: &str) -> Result<Rule, String> {
    let path = format!("repos/{repo}/branches/{}", branch_path(branch));
    let get = |endpoint: String| async move {
        crate::github::api(&endpoint, "GET", &[("per_page", "100".into())], None).await
    };
    let unseen = |e: String| {
        if e.contains("HTTP 403") || e.contains("HTTP 404") {
            format!("Orbit's GitHub sign-in can't see {branch}'s rules, so Orbit can't tell whether it needs reviews.")
        } else {
            format!(
                "Orbit couldn't read {branch}'s rules, so it can't tell whether it needs reviews: {}.",
                logs::short(&e)
            )
        }
    };
    let protected = get(path.clone()).await.map_err(unseen)?["protected"].as_bool();
    let protection = match protected {
        Some(false) => None,
        Some(true) => match get(format!("{path}/protection")).await {
            Ok(v) => Some(v),
            // Protected by rulesets alone.
            Err(e) if e.contains("Branch not protected") => None,
            Err(e) => return Err(unseen(e)),
        },
        None => return Err(unseen("GitHub didn't say whether it is protected".into())),
    };
    let endpoint = format!("repos/{repo}/rules/branches/{}", branch_path(branch));
    let Some(rulesets) = read_pages(&endpoint, PAGES).await.map_err(unseen)? else {
        return Err(format!(
            "{branch} has more rules than Orbit reads, so Orbit can't tell whether it needs reviews."
        ));
    };
    Ok(rule(protection.as_ref(), &Value::Array(rulesets)))
}
/// A branch name as part of an API path: slashes stay, and anything else
/// that isn't plain is escaped.
fn branch_path(branch: &str) -> String {
    branch
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                (b as char).to_string()
            }
            b => format!("%{b:02X}"),
        })
        .collect()
}
/// Who was asked to review a pull request, or reviewed it, from GitHub's
/// pull request and its reviews, oldest first, as of its latest commit
/// `head`: those asked now, then teams asked, then the rest by when they
/// last reviewed. Comments don't change where anyone stands, a dismissed
/// review counts as none, and the author's own reviews don't count.
/// Someone asked again stands where their latest review left them, as a
/// request for changes stands until they review again, and an approval of
/// the latest commit still counts.
pub fn reviewers(pull: &Value, reviews: &[Value], head: &str) -> Vec<Reviewer> {
    let login = |v: &Value| v["login"].as_str().unwrap_or_default().to_string();
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let author = login(&pull["user"]);
    // Each reviewer's latest stand, None once dismissed.
    let mut latest: Vec<(String, Option<Stand>, String)> = vec![];
    for r in reviews {
        let name = login(&r["user"]);
        let commit = r["commit_id"].as_str().unwrap_or_default().to_string();
        let stand = match r["state"].as_str().unwrap_or_default() {
            "APPROVED" if commit == head => Some(Stand::Approved),
            "APPROVED" => Some(Stand::ApprovedOlder),
            "CHANGES_REQUESTED" => Some(Stand::ChangesRequested),
            "DISMISSED" => None,
            _ => continue,
        };
        if name.is_empty() || same(&name, &author) {
            continue;
        }
        latest.retain(|(n, ..)| !same(n, &name));
        latest.push((name, stand, commit));
    }
    let mut out: Vec<Reviewer> = vec![];
    let listed = |out: &[Reviewer], name: &str| out.iter().any(|x| !x.team && same(&x.name, name));
    let reviewer = |name: String| {
        let found = latest.iter().find(|(n, ..)| same(n, &name));
        match found {
            Some((_, Some(stand), commit)) => Reviewer {
                name,
                stand: *stand,
                commit: commit.clone(),
                ..Default::default()
            },
            _ => Reviewer {
                name,
                ..Default::default()
            },
        }
    };
    for u in pull["requested_reviewers"].as_array().into_iter().flatten() {
        let name = login(u);
        if !name.is_empty() && !listed(&out, &name) {
            out.push(reviewer(name));
        }
    }
    let owner = pull["base"]["repo"]["owner"]["login"]
        .as_str()
        .unwrap_or_default();
    for t in pull["requested_teams"].as_array().into_iter().flatten() {
        let Some(slug) = t["slug"].as_str().filter(|s| !s.is_empty()) else {
            continue;
        };
        out.push(Reviewer {
            name: if owner.is_empty() {
                slug.to_string()
            } else {
                format!("{owner}/{slug}")
            },
            team: true,
            ..Default::default()
        });
    }
    for (name, stand, _) in &latest {
        if stand.is_some() && !listed(&out, name) {
            out.push(reviewer(name.clone()));
        }
    }
    out
}
/// What a base branch's rules need, from its classic protection, when it
/// has one Orbit can see, and the rulesets on it: the most approvals any
/// asks for, and whether any asks for a code owner's.
pub fn rule(protection: Option<&Value>, rulesets: &Value) -> Rule {
    let mut rule = Rule::default();
    let mut add = |count: &Value, owners: &Value| {
        // A rule that doesn't say how many asks for one.
        let count = count
            .as_u64()
            .map_or(1, |n| u32::try_from(n).unwrap_or(u32::MAX));
        rule.approvals = rule.approvals.max(count);
        rule.code_owners |= owners.as_bool().unwrap_or(false);
    };
    let classic = protection
        .map(|p| &p["required_pull_request_reviews"])
        .filter(|r| r.is_object());
    if let Some(r) = classic {
        add(
            &r["required_approving_review_count"],
            &r["require_code_owner_reviews"],
        );
    }
    let sets = rulesets.as_array().into_iter().flatten();
    for r in sets.filter(|r| r["type"] == "pull_request") {
        let p = &r["parameters"];
        add(
            &p["required_approving_review_count"],
            &p["require_code_owner_review"],
        );
    }
    rule
}
/// GitHub's answer about one pull request, as a `Merge`.
pub fn understand(
    v: &Value,
    pr_id: &str,
    head_sha: &str,
    base_sha: &str,
    now: DateTime<Utc>,
) -> Result<Merge, String> {
    if v["head"]["sha"] != head_sha || v["base"]["sha"] != base_sha {
        return Err("GitHub's answer was about another commit.".into());
    }
    Ok(Merge {
        pr_id: pr_id.to_string(),
        head_sha: head_sha.to_string(),
        base_sha: base_sha.to_string(),
        read_at: stamp(now),
        mergeable: v["mergeable"].as_bool(),
        state: v["mergeable_state"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        ..Default::default()
    })
}

/// Records what GitHub said about a pull request, or that asking failed.
/// While GitHub is still working it out, or after an error, Orbit asks
/// again after a wait, a minute doubling up to `logs::MAX_RETRY`, and
/// keeps what GitHub said before about the same head and base, so a
/// pull request clear to merge doesn't flicker while GitHub works it out
/// again.
pub fn record(
    merges: &mut Vec<Merge>,
    pr_id: &str,
    head_sha: &str,
    base_sha: &str,
    result: Result<Merge, String>,
    now: DateTime<Utc>,
) {
    let known = merges.iter().position(|m| m.pr_id == pr_id);
    let same = known
        .map(|i| &merges[i])
        .filter(|m| m.head_sha == head_sha && m.base_sha == base_sha);
    let tries = same.map_or(0, |m| m.tries) + 1;
    let wait = Duration::minutes(1i64 << (tries - 1).min(6)).min(logs::MAX_RETRY);
    let answered = same.filter(|m| !m.pending());
    let merge = match result {
        // Reviews don't wait for GitHub to work out whether it merges.
        Ok(m) if m.pending() => Merge {
            tries,
            retry_at: Some(stamp(now + wait)),
            error: None,
            reviews: m.reviews.clone(),
            ..answered.cloned().unwrap_or(m)
        },
        Ok(m) => m,
        Err(error) => {
            let before = same.cloned().unwrap_or_else(|| Merge {
                pr_id: pr_id.to_string(),
                head_sha: head_sha.to_string(),
                base_sha: base_sha.to_string(),
                ..Default::default()
            });
            Merge {
                error: Some(format!(
                    "Orbit couldn't ask GitHub whether it merges: {}. It tries again later.",
                    logs::short(&error)
                )),
                tries,
                retry_at: Some(stamp(now + wait)),
                ..before
            }
        }
    };
    match known {
        Some(i) => merges[i] = merge,
        None => merges.push(merge),
    }
}

/// Forgets what GitHub said about pull requests Orbit no longer reviews.
pub fn prune(s: &mut Snapshot) {
    let kept: Vec<String> = reviewed(s).map(|pr| pr.id.clone()).collect();
    s.merges.retain(|m| kept.contains(&m.pr_id));
}

#[cfg(test)]
#[path = "readiness_tests.rs"]
mod tests;
