//! Self-hosted runners: the ones each connected repository has, and the
//! ones the organizations that own connected repositories share, when
//! Orbit's GitHub sign-in can see them, and each repository's runs that
//! have waited a long time for a runner. Read every `EVERY`, a few places a
//! pass. Where the sign-in can't see runners, Orbit says so, never that all
//! is well.
//!
//! A repository's waiting runs open a problem for you (see `crate::problems`
//! `see_waiting`); nothing here reaches GitHub.
use crate::{
    github::{Pages, PAGE},
    logs,
    problems::{stamp, time},
    types::*,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Places Orbit reads in one pass.
pub const PER_PASS: usize = 4;
/// How often Orbit reads each place again.
pub const EVERY: Duration = Duration::minutes(5);
/// How long a run waits for a runner before Orbit says it is stuck.
pub const STUCK: Duration = Duration::minutes(30);
/// Pages of a repository's queued runs Orbit reads at most in one go.
pub const RUN_PAGES: u32 = 3;
/// The longest name Orbit keeps for a runner or a run.
const NAME: usize = 80;

/// What a place is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind {
    #[default]
    Repository,
    /// The account that owns connected repositories, until Orbit knows
    /// whether it is an organization.
    Owner,
    Organization,
    /// A person's account, which shares no runners.
    Person,
}
/// A self-hosted runner.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Runner {
    pub name: String,
    /// Such as `linux`.
    pub os: String,
    pub online: bool,
    pub busy: bool,
    pub labels: Vec<String>,
}
/// A run that has waited for a runner since `since`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Waiting {
    pub id: u64,
    /// Its workflow's name; empty when GitHub gives none.
    pub name: String,
    /// The run on GitHub; empty unless it is a github.com link.
    pub url: String,
    pub since: String,
}
/// What Orbit read about one place's runners.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Place {
    /// `owner/name` for a repository, or an account's login.
    pub name: String,
    pub kind: PlaceKind,
    /// When Orbit last read it, by Orbit's clock.
    pub read_at: String,
    pub runners: Vec<Runner>,
    /// How many runners GitHub says there are, which is more than
    /// `runners` when there are more than Orbit reads.
    pub total: u32,
    /// Why Orbit can't see its runners, when its sign-in may not.
    pub unseen: Option<String>,
    /// For a repository, its runs that have waited for a runner for longer
    /// than `STUCK`, the longest first.
    pub waiting: Vec<Waiting>,
    /// GitHub had more queued runs than Orbit read, so more may wait than
    /// `waiting` lists.
    pub more_queued: bool,
    /// Why the last read failed, how many in a row did, and when Orbit
    /// tries again. What was read before is kept meanwhile.
    pub error: Option<String>,
    pub tries: u32,
    pub retry_at: Option<String>,
}

/// The places Orbit should read now for these connected repositories, at
/// most `PER_PASS`: each repository and each account that owns one, those
/// never read first, then those read longest ago. A place is read again
/// `EVERY` after it was, or after its wait once a read failed. A person's
/// account is read once, to learn that it is one.
pub fn wanted(repos: &[String], places: &[Place], now: DateTime<Utc>) -> Vec<(String, PlaceKind)> {
    let mut all: Vec<(String, PlaceKind)> = vec![];
    for repo in repos {
        if validate_repo(repo).is_err() {
            continue;
        }
        all.push((repo.clone(), PlaceKind::Repository));
        let owner = repo.split_once('/').map_or("", |(o, _)| o);
        if !all
            .iter()
            .any(|(n, k)| *k != PlaceKind::Repository && same_repo(n, owner))
        {
            let kind = places
                .iter()
                .find(|p| p.kind != PlaceKind::Repository && same_repo(&p.name, owner))
                .map_or(PlaceKind::Owner, |p| p.kind);
            all.push((owner.to_string(), kind));
        }
    }
    let mut due: Vec<(String, (String, PlaceKind))> = all
        .into_iter()
        .filter_map(|(name, kind)| {
            // A repository's name has a slash and an account's doesn't, so
            // a name is one place.
            let known = places.iter().find(|p| same_repo(&p.name, &name));
            let Some(p) = known else {
                return Some((String::new(), (name, kind)));
            };
            let at = |t: &Option<String>| t.as_deref().and_then(time);
            let ready = match &p.error {
                Some(_) => at(&p.retry_at).is_none_or(|retry| now >= retry),
                None if p.kind == PlaceKind::Person => false,
                None => time(&p.read_at).is_none_or(|read| now - read >= EVERY),
            };
            ready.then(|| (p.read_at.clone(), (name, kind)))
        })
        .collect();
    due.sort_by(|a, b| a.0.cmp(&b.0));
    due.into_iter()
        .take(PER_PASS)
        .map(|(_, place)| place)
        .collect()
}

/// Reads a place: a repository's runners and its runs waiting for one, an
/// account's kind and, for an organization, the runners it shares.
pub async fn read(name: &str, kind: PlaceKind) -> Result<Place, String> {
    let now = Utc::now();
    let mut place = Place {
        name: name.to_string(),
        kind,
        read_at: stamp(now),
        ..Default::default()
    };
    let page = [("per_page", PAGE.to_string())];
    match kind {
        PlaceKind::Repository => {
            validate_repo(name)?;
            let runners =
                crate::github::api(&format!("repos/{name}/actions/runners"), "GET", &page, None)
                    .await;
            seen(&mut place, runners)?;
            // Only a run made `STUCK` ago or earlier can have waited that
            // long. GitHub lists the newest first, so those that waited
            // longest come last.
            let before = format!("<{}", (now - STUCK).format("%Y-%m-%dT%H:%M:%S+00:00"));
            let mut queue = Pages::new("workflow_runs", "queued runs");
            for n in 1..=RUN_PAGES {
                let fields = [
                    ("status", "queued".to_string()),
                    ("created", before.clone()),
                    ("per_page", PAGE.to_string()),
                    ("page", n.to_string()),
                ];
                let runs =
                    crate::github::api(&format!("repos/{name}/actions/runs"), "GET", &fields, None)
                        .await?;
                if !queue.add(&runs)? {
                    break;
                }
            }
            place.waiting = waiting(&queue.items, now);
            place.more_queued = !queue.all();
        }
        PlaceKind::Owner | PlaceKind::Organization | PlaceKind::Person => {
            if !valid_login(name) || name.ends_with("[bot]") {
                return Err("Invalid account".into());
            }
            if kind != PlaceKind::Organization {
                let account =
                    crate::github::api(&format!("users/{name}"), "GET", &[], None).await?;
                place.kind = match account["type"].as_str() {
                    Some("Organization") => PlaceKind::Organization,
                    Some("User") => PlaceKind::Person,
                    _ => return Err(format!("GitHub didn't say what kind of account {name} is.")),
                };
            }
            if place.kind == PlaceKind::Organization {
                let runners =
                    crate::github::api(&format!("orgs/{name}/actions/runners"), "GET", &page, None)
                        .await;
                seen(&mut place, runners)?;
            }
        }
    }
    Ok(place)
}
/// Keeps what GitHub said about a place's runners, or why Orbit can't see
/// them. A refusal that isn't a rate limit, or a place GitHub won't show,
/// means Orbit's sign-in can't see them; anything else is tried again.
fn seen(place: &mut Place, answer: Result<Value, String>) -> Result<(), String> {
    match answer {
        Ok(v) => {
            (place.runners, place.total) = runners(&v)?;
            Ok(())
        }
        Err(e) if logs::refused(&e) => {
            place.unseen = Some(unseen(&place.name, place.kind));
            Ok(())
        }
        Err(e) => Err(e),
    }
}
/// Why Orbit can't see a place's runners, in Orbit's words.
pub fn unseen(name: &str, kind: PlaceKind) -> String {
    match kind {
        PlaceKind::Repository => {
            format!("Orbit can't see the self-hosted runners of {name}: that needs admin rights on it.")
        }
        _ => format!(
            "Orbit can't see the runners the {name} organization shares: that needs an owner of {name} signed in with the admin:org scope."
        ),
    }
}
/// The runners in GitHub's answer, and how many it says there are.
pub fn runners(v: &Value) -> Result<(Vec<Runner>, u32), String> {
    let list = v["runners"]
        .as_array()
        .ok_or("GitHub didn't return the runners.")?;
    let runners: Vec<Runner> = list
        .iter()
        .map(|r| Runner {
            name: cut(r["name"].as_str().unwrap_or("")),
            os: cut(r["os"].as_str().unwrap_or("")),
            online: r["status"] == "online",
            busy: r["busy"] == true,
            labels: r["labels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| l["name"].as_str())
                .map(cut)
                .collect(),
        })
        .collect();
    let total = v["total_count"]
        .as_u64()
        .unwrap_or(0)
        .max(runners.len() as u64);
    Ok((runners, total.min(u32::MAX as u64) as u32))
}
/// The queued runs among these that have waited for longer than `STUCK`
/// since their latest try began, the longest first. A run GitHub gives no
/// time for is left out.
pub fn waiting(runs: &[Value], now: DateTime<Utc>) -> Vec<Waiting> {
    let mut out: Vec<(DateTime<Utc>, Waiting)> = runs
        .iter()
        .filter(|r| r["status"] == "queued")
        .filter_map(|r| {
            // A rerun keeps its run's first time; its try began later.
            let since = r["run_started_at"]
                .as_str()
                .or(r["created_at"].as_str())
                .and_then(time)?;
            (now - since >= STUCK).then(|| {
                let url = r["html_url"].as_str().unwrap_or("");
                (
                    since,
                    Waiting {
                        id: r["id"].as_u64().unwrap_or(0),
                        name: cut(r["name"].as_str().unwrap_or("")),
                        url: if url.starts_with("https://github.com/") {
                            url.to_string()
                        } else {
                            String::new()
                        },
                        since: stamp(since),
                    },
                )
            })
        })
        .collect();
    out.sort_by_key(|(since, w)| (*since, w.id));
    out.into_iter().map(|(_, w)| w).collect()
}
/// A name on one line, cut short.
fn cut(text: &str) -> String {
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > NAME {
        let short: String = line.chars().take(NAME - 1).collect();
        format!("{short}…")
    } else {
        line
    }
}

/// What Orbit sees of what a repository's waiting runs could use, in a few
/// sentences: whether it read all of its queued runs, its own self-hosted
/// runners, and those its organization shares, which Orbit can't tell it may
/// use, as a runner group can leave it out. None before Orbit has read the
/// repository.
pub fn seen_text(places: &[Place], repo: &str) -> Option<String> {
    let owner = repo.split_once('/').map_or("", |(o, _)| o);
    // A repository's name has a slash and an account's doesn't.
    let own = places
        .iter()
        .find(|p| same_repo(&p.name, repo) && !p.read_at.is_empty())?;
    let account = places.iter().find(|p| same_repo(&p.name, owner));
    let mut out = vec![];
    if own.more_queued {
        out.push("Orbit couldn't read every queued run, so more may wait.".to_string());
    }
    // A place read without trouble that has no runners.
    let none = |p: &Place| p.unseen.is_none() && p.error.is_none() && p.total == 0;
    if none(own) && account.is_some_and(none) {
        out.push("Orbit sees no self-hosted runners for it, so its runs wait for GitHub's own runners or for one it doesn't have.".into());
    } else {
        out.push(own_text(own));
        out.extend(shared_text(account, owner, repo));
    }
    Some(out.join(" "))
}
/// A repository's own self-hosted runners in a sentence or two.
fn own_text(p: &Place) -> String {
    let n = p.runners.len();
    let online = p.runners.iter().filter(|r| r.online).count();
    let free = p.runners.iter().filter(|r| r.online && !r.busy).count();
    let mut text = match (n, online, free) {
        _ if p.unseen.is_some() => "Orbit can't see its own self-hosted runners.".to_string(),
        (0, _, _) => "It has no self-hosted runners of its own.".into(),
        (1, 0, _) => "Its one self-hosted runner is offline.".into(),
        (n, 0, _) => format!("Its {n} self-hosted runners are all offline."),
        (_, _, 0) => "Every one of its online self-hosted runners is busy.".into(),
        (_, _, 1) => "1 of its self-hosted runners is online and free, so the runs may ask for a label it doesn't have.".into(),
        (_, _, k) => format!("{k} of its self-hosted runners are online and free, so the runs may ask for a label none of them has."),
    };
    if p.total as usize > n {
        text.push_str(&format!(" Orbit read only {n} of its {}.", p.total));
    }
    if p.error.is_some() {
        text.push_str(" Orbit's latest read of them failed, so this may be out of date.");
    }
    text
}
/// The self-hosted runners the account that owns a repository shares, in a
/// sentence or two, or None for a person's account, which shares none.
fn shared_text(account: Option<&Place>, owner: &str, repo: &str) -> Option<String> {
    let Some(a) = account else {
        return Some(format!(
            "Orbit hasn't read what the {owner} account shares yet."
        ));
    };
    // Only a read that failed leaves no time.
    if a.read_at.is_empty() {
        return Some(format!(
            "Orbit couldn't read what the {owner} account shares yet."
        ));
    }
    let n = a.runners.len();
    let free = a.runners.iter().filter(|r| r.online && !r.busy).count();
    let mut text = match a.kind {
        PlaceKind::Person => return None,
        _ if a.unseen.is_some() => {
            format!("Orbit can't see the runners the {owner} organization shares.")
        }
        _ if a.total == 0 => format!("The {owner} organization shares no self-hosted runners."),
        _ => {
            let (count, them) = match a.total {
                1 => ("1 self-hosted runner".to_string(), "it"),
                t => (format!("{t} self-hosted runners"), "them"),
            };
            format!(
                "The {owner} organization shares {count} ({free} online and free), but Orbit can't tell whether {repo} may use {them}."
            )
        }
    };
    if a.total as usize > n {
        text.push_str(&format!(" Orbit read only {n} of them."));
    }
    if a.error.is_some() {
        text.push_str(&format!(
            " Orbit's latest read of what {owner} shares failed, so this may be out of date."
        ));
    }
    Some(text)
}

/// Records what Orbit read about a place. After an error it keeps what it
/// read before and tries again after a wait, a minute doubling up to
/// `logs::MAX_RETRY`.
pub fn record(
    places: &mut Vec<Place>,
    name: &str,
    kind: PlaceKind,
    result: Result<Place, String>,
    now: DateTime<Utc>,
) {
    let known = places.iter().position(|p| same_repo(&p.name, name));
    let place = match result {
        Ok(place) => place,
        Err(error) => {
            let mut before = known.map(|i| places[i].clone()).unwrap_or_else(|| Place {
                name: name.to_string(),
                kind,
                ..Default::default()
            });
            before.tries += 1;
            let wait = Duration::minutes(1i64 << (before.tries - 1).min(6)).min(logs::MAX_RETRY);
            before.error = Some(format!(
                "Orbit couldn't read the runners yet: {}. It tries again later.",
                logs::short(&error)
            ));
            before.retry_at = Some(stamp(now + wait));
            before
        }
    };
    match known {
        Some(i) => places[i] = place,
        None => places.push(place),
    }
}

/// Forgets places that are no longer a connected repository or the
/// account that owns one.
pub fn prune(places: &mut Vec<Place>, connected: &[String]) {
    places.retain(|p| {
        connected.iter().any(|r| match p.kind {
            PlaceKind::Repository => same_repo(r, &p.name),
            _ => r
                .split_once('/')
                .is_some_and(|(owner, _)| same_repo(owner, &p.name)),
        })
    });
}

#[cfg(test)]
#[path = "runners_tests.rs"]
mod tests;
