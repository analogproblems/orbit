//! One Orbit at a time on a pull request. Each Orbit has a name of its own,
//! kept in its state, and signs what it posts on a pull request with it in
//! a hidden line: its reviews and its comment about CI and whether the pull
//! request is clear to merge. Before posting there, it reads what was
//! posted since its own last post. When another Orbit posted there later
//! than it did, it steps back from that pull request and tells you, so an
//! agent is never answered twice. Posts from before Orbits signed theirs
//! belong to nobody, so they never make one step back.
//!
//! The name goes with where Orbit runs: its data folder on one computer. A
//! copy of the folder, or the folder moved to another computer, is another
//! Orbit, with a new name. It counts its old names' posts as its own, so it
//! carries on where they left off, and the Orbit it was copied from steps
//! back once the copy posts.
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::sync::LazyLock;

/// How many of its old names an Orbit keeps.
pub const FORMER: usize = 20;

/// Why Orbit didn't post on a pull request it stepped back from.
pub const STEPPED_BACK: &str =
    "Another Orbit posted on this pull request after this one, so this one stepped back from it.";

static SIGNATURE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!--\s*orbit-from\s+([0-9a-f]{32})\s*-->").unwrap());

/// A new name for an Orbit, from the system's secure random source.
pub fn new_name() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
/// Who this Orbit is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Instance {
    /// Its name, which signs what it posts.
    pub name: String,
    /// Where it was named (`home`).
    pub home: String,
    /// Its earlier names, newest last.
    pub former: Vec<String>,
}
impl Instance {
    /// Names this Orbit for `home`: a new name the first time, and once it
    /// runs somewhere else, keeping the old one as a former name. Returns
    /// whether that changed anything.
    pub fn settle(&mut self, home: &str) -> bool {
        if !self.name.is_empty() && self.home == home {
            return false;
        }
        if !self.name.is_empty() {
            self.former.push(std::mem::take(&mut self.name));
            let extra = self.former.len().saturating_sub(FORMER);
            self.former.drain(..extra);
        }
        self.name = new_name();
        self.home = home.to_string();
        true
    }
    /// Whether this Orbit, under its name now or an earlier one, signed as
    /// `by`.
    pub fn is(&self, by: &str) -> bool {
        by == self.name || self.former.iter().any(|f| f == by)
    }
}
/// Where an Orbit runs, as a fingerprint of the computer's name and its
/// data folder. It never leaves the computer.
pub fn home(data: &Path) -> String {
    use sha2::{Digest, Sha256};
    let folder = std::fs::canonicalize(data).unwrap_or_else(|_| data.to_path_buf());
    let place = format!("{}\0{}", host_name(), folder.display());
    Sha256::digest(place.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
#[cfg(unix)]
fn host_name() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its length, and the name ends at the
    // first zero, or at the end when it was cut short.
    let read = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if read != 0 {
        return String::new();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}
#[cfg(windows)]
fn host_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}
/// The hidden line that signs a post as this Orbit's.
pub fn signature(me: &str) -> String {
    format!("<!-- orbit-from {me} -->")
}
/// The post with this Orbit's signature at its end, where it changes
/// nothing a reader or another of Orbit's markers depends on.
pub fn signed(body: &str, me: &str) -> String {
    format!("{body}\n\n{}", signature(me))
}
/// The Orbit that signed a post, if one did.
pub fn signer(body: &str) -> Option<&str> {
    SIGNATURE
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
}

/// Where Orbit stands on one pull request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Posting {
    pub pr_id: String,
    /// When GitHub last took a post from this Orbit there, by GitHub's
    /// clock, as far as Orbit has read.
    pub mine: String,
    /// The newest post there from another Orbit, as far as Orbit has read.
    pub other: Option<Post>,
    /// When the other Orbit's post was that you said to post after anyway.
    pub resumed: String,
}
/// A post on GitHub.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Post {
    pub url: String,
    /// When GitHub last took it, by GitHub's clock.
    pub at: String,
}
impl Posting {
    /// Whether another Orbit posted after this one, and after the post you
    /// said to post after anyway.
    pub fn stepped_back(&self) -> bool {
        self.other
            .as_ref()
            .is_some_and(|o| later(&o.at, &self.mine) && later(&o.at, &self.resumed))
    }
    /// Takes in posts read from GitHub: issue comments (by `updated_at`) or
    /// reviews (by `submitted_at`). `me` is this Orbit.
    pub fn read(&mut self, items: &[Value], me: &Instance) {
        for item in items {
            let Some(by) = item["body"].as_str().and_then(signer) else {
                continue;
            };
            let Some(at) = item["updated_at"]
                .as_str()
                .or_else(|| item["submitted_at"].as_str())
                .filter(|at| !at.is_empty())
            else {
                continue;
            };
            if me.is(by) {
                if later(at, &self.mine) {
                    self.mine = at.to_string();
                }
            } else if self.other.as_ref().is_none_or(|o| later(at, &o.at)) {
                self.other = Some(Post {
                    url: item["html_url"].as_str().unwrap_or_default().to_string(),
                    at: at.to_string(),
                });
            }
        }
    }
    /// Posts here again after the other Orbit's latest post. Returns
    /// whether that changed anything.
    pub fn resume(&mut self) -> bool {
        match &self.other {
            Some(o) if self.stepped_back() => {
                self.resumed = o.at.clone();
                true
            }
            _ => false,
        }
    }
}
/// Whether `a` is later than `b`, which may be empty (before everything).
fn later(a: &str, b: &str) -> bool {
    if b.is_empty() {
        return !a.is_empty();
    }
    match (
        DateTime::parse_from_rfc3339(a),
        DateTime::parse_from_rfc3339(b),
    ) {
        (Ok(a), Ok(b)) => a.with_timezone(&Utc) > b.with_timezone(&Utc),
        _ => a > b,
    }
}

/// Whether Orbit stepped back from a pull request for another Orbit.
pub fn stepped_back(postings: &[Posting], pr_id: &str) -> bool {
    postings
        .iter()
        .any(|p| p.pr_id == pr_id && p.stepped_back())
}

#[cfg(test)]
#[path = "orbits_tests.rs"]
mod tests;
