//! One Orbit at a time on a pull request. Each Orbit has a name of its own,
//! kept in its state, and signs what it posts on a pull request with it in
//! a hidden last line: its reviews and its comment about CI and whether the
//! pull request is clear to merge. Before posting there, it reads what was
//! posted since its own last post. When another Orbit posted there later
//! than it did, it steps back from that pull request and tells you, so an
//! agent is never answered twice. Two posts in the same second are ordered
//! by their signers' names, so every Orbit agrees which came last. Posts
//! from before Orbits signed theirs belong to nobody, and only posts by
//! accounts Orbit trusts count, so neither ever makes one step back.
//!
//! The name goes with where Orbit runs: its data folder on one computer. A
//! copy of the folder, or the folder moved to another computer, is another
//! Orbit, with a new name. It counts what its old names posted until then
//! as its own, so it carries on where they left off, and the Orbit it was
//! copied from steps back once the copy posts. Anything that old name posts
//! afterwards is another Orbit's, so you can hand back to the original.
use chrono::{DateTime, SecondsFormat, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::path::Path;
use std::sync::LazyLock;

/// How many of its old names an Orbit keeps.
pub const FORMER: usize = 20;

/// Why Orbit didn't post on a pull request it stepped back from.
pub const STEPPED_BACK: &str =
    "Another Orbit posted on this pull request after this one, so this one stepped back from it.";

/// A signature, as the whole of a post's last line.
static SIGNATURE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^<!--\s*orbit-from\s+([0-9a-f]{32})\s*-->$").unwrap());

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
    pub former: Vec<Former>,
}
/// One of an Orbit's earlier names.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Former {
    pub name: String,
    /// When this Orbit stopped using it, by this computer's clock. What the
    /// name posted later is another Orbit's: the one it was copied from.
    pub until: String,
}
impl Instance {
    /// Names this Orbit for `home`: a new name the first time, and once it
    /// runs somewhere else, keeping the old one as a former name until
    /// `now`. Returns whether that changed anything.
    pub fn settle(&mut self, home: &str, now: DateTime<Utc>) -> bool {
        if !self.name.is_empty() && self.home == home {
            return false;
        }
        if !self.name.is_empty() {
            self.former.push(Former {
                name: std::mem::take(&mut self.name),
                until: now.to_rfc3339_opts(SecondsFormat::Secs, true),
            });
            let extra = self.former.len().saturating_sub(FORMER);
            self.former.drain(..extra);
        }
        self.name = new_name();
        self.home = home.to_string();
        true
    }
    /// Whether this Orbit signed a post as `by` that GitHub last took `at`:
    /// under its name now, or an earlier one until it stopped using it.
    pub fn is(&self, by: &str, at: &str) -> bool {
        by == self.name
            || self
                .former
                .iter()
                .any(|f| f.name == by && time_order(at, &f.until) != Ordering::Greater)
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
/// The Orbit that signed a post, if one did: only its signature as the
/// post's last line counts, never a marker quoted above it.
pub fn signer(body: &str) -> Option<&str> {
    let last = body.trim_end().lines().last()?.trim();
    SIGNATURE
        .captures(last)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
}
/// Whether a post is by an account whose signature Orbit believes: Orbit's
/// own sign-in (`login`), an owner, member or collaborator of the
/// repository, or an account you trust. Anyone else could copy a
/// signature to make Orbit step back.
pub fn trusted(post: &Value, login: &str, trusted_authors: &[String]) -> bool {
    let by = post["user"]["login"].as_str().unwrap_or_default();
    (!by.is_empty() && by.eq_ignore_ascii_case(login))
        || matches!(
            post["author_association"].as_str(),
            Some("OWNER" | "MEMBER" | "COLLABORATOR")
        )
        || (!by.is_empty() && trusted_authors.iter().any(|a| a.eq_ignore_ascii_case(by)))
}

/// Where Orbit stands on one pull request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Posting {
    pub pr_id: String,
    /// This Orbit's newest post there, as far as Orbit has read.
    pub mine: Option<Post>,
    /// The newest post there from another Orbit, as far as Orbit has read.
    pub other: Option<Post>,
    /// The other Orbit's post you said to post after anyway.
    pub resumed: Option<Post>,
}
/// A signed post on GitHub.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Post {
    pub url: String,
    /// When GitHub last took it, by GitHub's clock.
    pub at: String,
    /// The name that signed it.
    pub by: String,
}
impl Post {
    /// Whether this post came after `other`, or after nothing: by GitHub's
    /// time, then, in the same second, by the signer's name, so every Orbit
    /// orders the two alike.
    fn after(&self, other: Option<&Post>) -> bool {
        other.is_none_or(|o| time_order(&self.at, &o.at).then(self.by.cmp(&o.by)).is_gt())
    }
}
impl Posting {
    /// Whether another Orbit posted after this one, and after the post you
    /// said to post after anyway.
    pub fn stepped_back(&self) -> bool {
        self.other
            .as_ref()
            .is_some_and(|o| o.after(self.mine.as_ref()) && o.after(self.resumed.as_ref()))
    }
    /// When this Orbit last posted there, for reading what came since.
    pub fn since(&self) -> &str {
        self.mine.as_ref().map_or("", |m| m.at.as_str())
    }
    /// Takes in posts read from GitHub, once `trusted` kept those to
    /// believe: issue comments (by `updated_at`) or reviews (by
    /// `submitted_at`). `me` is this Orbit.
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
            let post = Post {
                url: item["html_url"].as_str().unwrap_or_default().to_string(),
                at: at.to_string(),
                by: by.to_string(),
            };
            let newest = if me.is(by, at) {
                &mut self.mine
            } else {
                &mut self.other
            };
            if post.after(newest.as_ref()) {
                *newest = Some(post);
            }
        }
    }
    /// Posts here again after the other Orbit's latest post. Returns
    /// whether that changed anything.
    pub fn resume(&mut self) -> bool {
        if !self.stepped_back() {
            return false;
        }
        self.resumed = self.other.clone();
        true
    }
}
/// How two of GitHub's times compare, by the moment they name.
fn time_order(a: &str, b: &str) -> Ordering {
    match (
        DateTime::parse_from_rfc3339(a),
        DateTime::parse_from_rfc3339(b),
    ) {
        (Ok(a), Ok(b)) => a.with_timezone(&Utc).cmp(&b.with_timezone(&Utc)),
        _ => a.cmp(b),
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
