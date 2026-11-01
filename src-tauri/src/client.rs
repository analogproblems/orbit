//! The desktop app showing an always-on Orbit instead of running its own.
//! The window then works as a browser on another device does (see
//! `crate::remote`): each action goes to that Orbit's `/api/<command>` with
//! its access token, and its snapshots arrive on one event stream, which the
//! app passes to the window as they come. The notifications it sent
//! (`Snapshot.notices`) are shown here too, each once.
//!
//! The link is kept in the desktop app's data folder, in `always-on.json`,
//! which only Settings writes. The app's own Orbit isn't started while it's
//! there, and its state stays as it was, for when you switch back. The
//! notifications already shown are kept apart, in `always-on-seen.json`,
//! so keeping them never rewrites the link.
use crate::attention::SentNotice;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path, time::Duration};

const FILE: &str = "always-on.json";
const SEEN_FILE: &str = "always-on-seen.json";
/// How long reaching the always-on Orbit may take.
const CONNECT: Duration = Duration::from_secs(15);
/// How long Orbit without a window on this computer may take to answer
/// while the desktop app starts.
const HERE: Duration = Duration::from_secs(3);

/// How long Orbit waits on the always-on Orbit before giving up.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// How long the event stream may stay silent, its answer included:
    /// the always-on Orbit writes at least every 15 seconds.
    pub silence: Duration,
    /// For its snapshot, which it has to hand.
    pub snapshot: Duration,
    /// For anything else, which may run as long as a sync or a publish.
    pub action: Duration,
    /// For moving this app's Orbit there, which may download each of its
    /// repositories.
    pub moving: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Limits {
            silence: Duration::from_secs(45),
            snapshot: Duration::from_secs(20),
            action: Duration::from_secs(10 * 60),
            moving: Duration::from_secs(30 * 60),
        }
    }
}

pub const NOT_A_LINK: &str =
    "That isn't an Orbit access link. Copy the whole link, including the part after #token=.";
pub const NOT_PRIVATE: &str = "That link isn't private: use the https link through Tailscale, or an http link only on this computer.";
pub const EXPIRED: &str = "The always-on Orbit's link no longer works. Paste its current link in Settings, Other devices.";
pub const LOST: &str = "Lost connection to the always-on Orbit. Reconnecting…";
pub const OWN_LINK: &str = "That's this app's own link. Paste the always-on Orbit's link: `orbit-server link` shows it on that computer.";
pub const NOT_EMPTY: &str = "The always-on Orbit already has repositories or reviews of its own, so this one can't move there. Use it as it is with Use it instead.";
pub const NOT_SIGNED_IN: &str = "The always-on Orbit isn't signed in to GitHub yet. On that computer, run gh auth login, then try again.";
pub const TOO_OLD: &str =
    "The always-on Orbit is too old to take this one in. Update it, then try again.";

/// An always-on Orbit's access link.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    /// Where it is, such as `https://box.example.ts.net/`.
    pub base: String,
    pub token: String,
}
impl Link {
    /// Reads an access link as Orbit shows it, `https://<host>/#token=<token>`.
    /// Plain `http` is taken only for this computer, so the token never
    /// crosses a network unencrypted.
    pub fn parse(text: &str) -> Result<Link, String> {
        let (address, fragment) = text.trim().split_once('#').ok_or(NOT_A_LINK)?;
        let token = fragment
            .strip_prefix("token=")
            .filter(|t| crate::remote::valid_token(t))
            .ok_or(NOT_A_LINK)?;
        let url = reqwest::Url::parse(address).map_err(|_| NOT_A_LINK)?;
        if !url.username().is_empty() || url.password().is_some() || url.host().is_none() {
            return Err(NOT_A_LINK.into());
        }
        match url.scheme() {
            "https" => {}
            "http" if local(&url) => {}
            "http" => return Err(NOT_PRIVATE.into()),
            _ => return Err(NOT_A_LINK.into()),
        }
        Ok(Link {
            base: format!("{}/", url.origin().ascii_serialization()),
            token: token.to_lowercase(),
        })
    }
    /// The always-on Orbit's address, as the app shows it.
    pub fn host(&self) -> &str {
        self.base
            .split_once("://")
            .map_or(self.base.as_str(), |(_, rest)| rest)
            .trim_end_matches('/')
    }
}
/// Whether `url` is on this computer.
fn local(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// What `always-on.json` holds.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    link: Option<Link>,
}
fn write_file(root: &Path, name: &str, value: &impl Serialize) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    crate::remote::write_private(&root.join(name), &bytes)
}
fn read_file<T: serde::de::DeserializeOwned + Default>(root: &Path, name: &str) -> T {
    std::fs::read(root.join(name))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
/// The always-on Orbit this app uses, if it uses one.
pub fn load(root: &Path) -> Option<Link> {
    read_file::<Saved>(root, FILE).link
}
/// Uses the always-on Orbit at `link` from now on. Its notifications are
/// shown from the next one it sends.
pub fn save(root: &Path, link: &Link) -> Result<(), String> {
    write_file(
        root,
        FILE,
        &Saved {
            link: Some(link.clone()),
        },
    )
}
/// Stops using an always-on Orbit, so the app runs its own again.
pub fn forget(root: &Path) -> Result<(), String> {
    match std::fs::remove_file(root.join(FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!(
            "Orbit couldn't stop using the always-on Orbit: {e}"
        )),
        _ => Ok(()),
    }
}

/// What `always-on-seen.json` holds: the notifications shown here from
/// the Orbit at one link, named by a hash of it, so what a follower writes
/// late, after the link changed, counts for nothing.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Seen {
    link: String,
    ids: Vec<String>,
}
fn link_id(link: &Link) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(format!("{}#{}", link.base, link.token).as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
/// The notifications already shown from the Orbit at `link`, if any
/// snapshot from it arrived.
fn seen(root: &Path, link: &Link) -> Option<Vec<String>> {
    let seen: Seen = read_file(root, SEEN_FILE);
    (seen.link == link_id(link)).then_some(seen.ids)
}
fn remember(root: &Path, link: &Link, ids: Vec<String>) -> Result<(), String> {
    let seen = Seen {
        link: link_id(link),
        ids,
    };
    write_file(root, SEEN_FILE, &seen)
}

/// Orbit without a window running on this computer with the data folder
/// `root`, if it answers within a few seconds. Only one Orbit can use a
/// data folder, so the desktop app shows that one instead of its own.
pub async fn running_here(root: &Path) -> Option<Link> {
    let token = crate::remote::saved_token(root)?;
    let port = crate::remote::config(root).port;
    let link = Link::parse(&crate::remote::local_link(port, &token)).ok()?;
    let client = Client::new(link.clone()).ok()?;
    let answer = tokio::time::timeout(HERE, client.call("get_snapshot", &json!({}))).await;
    matches!(answer, Ok(Ok(_))).then_some(link)
}

/// Whether the always-on Orbit whose snapshot is `there` can take in this
/// app's Orbit, signed in to GitHub as `login` when known: only when it has
/// nothing of its own yet, and is signed in as the same account.
pub fn can_take_in(there: &Value, login: Option<&str>) -> Result<(), String> {
    let has = |list: &Value| list.as_array().is_some_and(|l| !l.is_empty());
    if has(&there["settings"]["repositories"]) || has(&there["reviews"]) || has(&there["scans"]) {
        return Err(NOT_EMPTY.into());
    }
    match (there["githubUser"].as_str(), login) {
        (None, _) => Err(NOT_SIGNED_IN.into()),
        (Some(there), Some(here)) if !there.eq_ignore_ascii_case(here) => Err(format!(
            "The always-on Orbit is signed in to GitHub as {there}, and this one as {here}. Sign it in as {here} on that computer, then try again."
        )),
        _ => Ok(()),
    }
}
/// Whether the always-on Orbit took in the Orbit named `name`: its
/// snapshot `after` counts that name as one of its own, and `before`
/// didn't. Used when a move's answer was lost.
pub fn took_in(before: &Value, after: &Value, name: &str) -> bool {
    let counts = |s: &Value| {
        s["instance"]["former"]
            .as_array()
            .is_some_and(|f| f.iter().any(|f| f["name"] == name))
    };
    !name.is_empty() && !counts(before) && counts(after)
}

/// Talks to an always-on Orbit.
pub struct Client {
    link: Link,
    http: reqwest::Client,
    limits: Limits,
}
impl Client {
    pub fn new(link: Link) -> Result<Client, String> {
        Client::with_limits(link, Limits::default())
    }
    pub fn with_limits(link: Link, limits: Limits) -> Result<Client, String> {
        // Not through a proxy, which can't reach your tailnet, and no
        // redirects, which would carry the token on to another site.
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Client { link, http, limits })
    }
    pub fn link(&self) -> &Link {
        &self.link
    }
    /// Runs `command` there, as the desktop app's command of that name
    /// runs here. It gives up after `Limits`, though the always-on Orbit
    /// still finishes what it started.
    pub async fn call(&self, command: &str, args: &Value) -> Result<Value, String> {
        let limit = if command == "get_snapshot" {
            self.limits.snapshot
        } else {
            self.limits.action
        };
        let response = self
            .http
            .post(format!("{}api/{command}", self.link.base))
            .bearer_auth(&self.link.token)
            .json(args)
            .timeout(limit)
            .send()
            .await
            .map_err(|e| self.unreachable(e))?;
        self.answer(response).await
    }
    /// What the always-on Orbit answered, or what went wrong.
    async fn answer(&self, response: reqwest::Response) -> Result<Value, String> {
        let status = response.status();
        let body: Value = match response.json().await {
            Ok(body) => body,
            Err(e) if e.is_timeout() => return Err(self.unreachable(e)),
            Err(_) => Value::Null,
        };
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(EXPIRED.into());
        }
        if !status.is_success() {
            return Err(body["error"].as_str().map_or_else(
                || format!("The always-on Orbit could not do that ({status})."),
                String::from,
            ));
        }
        Ok(body)
    }
    fn unreachable(&self, e: reqwest::Error) -> String {
        let host = self.link.host();
        if e.is_connect() {
            format!("Orbit can't reach the always-on Orbit at {host}. Check that it's running and that Tailscale is on.")
        } else if e.is_timeout() {
            format!("The always-on Orbit at {host} didn't answer in time. If it was busy, it may still finish: look again in a moment.")
        } else {
            format!("Orbit lost the always-on Orbit at {host}.")
        }
    }
    /// Checks that the always-on Orbit answers, before the app uses it.
    pub async fn check(&self) -> Result<(), String> {
        self.snapshot().await.map(drop)
    }
    /// The always-on Orbit's snapshot, as it shows it.
    pub async fn snapshot(&self) -> Result<Value, String> {
        self.call("get_snapshot", &json!({})).await
    }
    /// Moves this app's Orbit there (see `crate::moving`). An always-on
    /// Orbit from before moving was possible doesn't know the request.
    pub async fn move_in(&self, moved: &crate::moving::Moved) -> Result<(), String> {
        let response = self
            .http
            .post(format!("{}api/move-in", self.link.base))
            .bearer_auth(&self.link.token)
            .json(moved)
            .timeout(self.limits.moving)
            .send()
            .await
            .map_err(|e| self.unreachable(e))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(TOO_OLD.into());
        }
        self.answer(response).await.map(drop)
    }
    /// Follows the always-on Orbit's snapshots, giving each to `show`, and
    /// the notifications to show here to `notify`. `problem` hears what's
    /// wrong while it can't follow, and None once it can again. It tries
    /// again after a lost connection, waiting longer each time up to 15
    /// seconds, and gives up only when the link stops working. What was
    /// shown is remembered in `root`, so a restart shows nothing twice.
    pub async fn follow(
        &self,
        root: &Path,
        mut show: impl FnMut(&Value),
        mut notify: impl FnMut(&SentNotice),
        mut problem: impl FnMut(Option<&str>),
    ) {
        let mut relay = Relay::new(seen(root, &self.link));
        let mut wait = Duration::from_secs(1);
        loop {
            match self
                .stream(|snapshot| {
                    wait = Duration::from_secs(1);
                    let (new, remembered) = relay.take(snapshot);
                    if let Some(ids) = remembered {
                        let _ = remember(root, &self.link, ids);
                    }
                    show(snapshot);
                    new.iter().for_each(&mut notify);
                    problem(None);
                })
                .await
            {
                Ended::Expired => {
                    problem(Some(EXPIRED));
                    return;
                }
                Ended::Lost => problem(Some(LOST)),
            }
            tokio::time::sleep(wait).await;
            wait = (wait * 2).min(Duration::from_secs(15));
        }
    }
    /// Reads one event stream until it ends. Silence, before its answer
    /// or between its lines, ends it as lost; nothing limits how long it
    /// runs.
    async fn stream(&self, mut each: impl FnMut(&Value)) -> Ended {
        let silence = self.limits.silence;
        let response = self
            .http
            .get(format!("{}api/events", self.link.base))
            .bearer_auth(&self.link.token)
            .send();
        let mut response = match tokio::time::timeout(silence, response).await {
            Ok(Ok(r)) if r.status() == reqwest::StatusCode::UNAUTHORIZED => return Ended::Expired,
            Ok(Ok(r)) if r.status().is_success() => r,
            _ => return Ended::Lost,
        };
        let mut buffered: Vec<u8> = vec![];
        loop {
            let chunk = match tokio::time::timeout(silence, response.chunk()).await {
                Ok(Ok(Some(chunk))) => chunk,
                _ => return Ended::Lost,
            };
            buffered.extend_from_slice(&chunk);
            while let Some(end) = buffered.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffered.drain(..=end).collect();
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                match serde_json::from_slice::<Value>(&line) {
                    Ok(snapshot) => each(&snapshot),
                    Err(_) => return Ended::Lost,
                }
            }
        }
    }
}
/// Why an event stream ended.
enum Ended {
    Lost,
    Expired,
}

/// Picks out the notifications to show here from each snapshot: those not
/// shown before whose item still needs you, while notifications are on.
/// The first snapshot ever only marks what's there as seen, as it was sent
/// before this app followed that Orbit.
pub struct Relay {
    seen: Option<HashSet<String>>,
}
impl Relay {
    pub fn new(seen: Option<Vec<String>>) -> Relay {
        Relay {
            seen: seen.map(|ids| ids.into_iter().collect()),
        }
    }
    /// The notifications to show for `snapshot`, and what to remember as
    /// seen when that changed.
    pub fn take(&mut self, snapshot: &Value) -> (Vec<SentNotice>, Option<Vec<String>>) {
        let sent: Vec<SentNotice> = snapshot["notices"]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter_map(|n| serde_json::from_value(n.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();
        let ids: Vec<String> = sent.iter().map(|n| n.id.clone()).collect();
        let fresh = match &self.seen {
            None => vec![],
            Some(seen) => {
                let open: HashSet<&str> = snapshot["attention"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|a| a["key"].as_str())
                    .collect();
                let on = snapshot["settings"]["notify"].as_bool().unwrap_or(true);
                sent.into_iter()
                    .filter(|n| on && !seen.contains(&n.id) && open.contains(n.item.key.as_str()))
                    .collect()
            }
        };
        let now: HashSet<String> = ids.iter().cloned().collect();
        if self.seen.as_ref() == Some(&now) {
            return (fresh, None);
        }
        self.seen = Some(now);
        (fresh, Some(ids))
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
