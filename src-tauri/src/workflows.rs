//! Whether a repository has CI that runs on its pull requests, read from
//! the files on its default branch, and the workflow Orbit proposes when it
//! hasn't, built from what the repository uses, such as its `package.json`
//! scripts or a `Cargo.toml`. Read once for each new commit on the default
//! branch.
//!
//! Orbit only says a repository has no CI when it is sure: a workflow it
//! can't read, another CI service's settings, or checks on a commit Orbit
//! follows all count as CI (see `crate::problems` `see_setup`).
use crate::{
    ci, logs,
    problems::{stamp, time},
    types::*,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Repositories whose files Orbit reads in one pass.
pub const PER_PASS: usize = 2;
/// How long after reading a commit Orbit waits before it says nothing runs
/// CI there, so checks from another service have time to show up.
pub const QUIET: Duration = Duration::minutes(10);
/// Where GitHub runs workflows from.
pub const FOLDER: &str = ".github/workflows";
/// Which checks of workflow files a read made, so a commit read before
/// they were added, or changed, is read again: 1 is Orbit's own
/// (`crate::risks`), 2 adds actionlint and zizmor (`crate::lint`).
pub const CHECKS: u32 = 2;
/// Files and folders at the top of a repository that set up another CI
/// service.
pub const OTHER_CI: [&str; 22] = [
    ".appveyor.yml",
    ".azure-pipelines",
    ".buildkite",
    ".circleci",
    ".cirrus.yml",
    ".drone.yml",
    ".gitlab-ci.yml",
    ".semaphore",
    ".travis.yml",
    ".woodpecker",
    ".woodpecker.yaml",
    ".woodpecker.yml",
    "appveyor.yml",
    "azure-pipelines.yml",
    "bitbucket-pipelines.yml",
    "bitrise.yml",
    "buildspec.yml",
    "cloudbuild.yaml",
    "cloudbuild.yml",
    "codemagic.yaml",
    "Jenkinsfile",
    "wercker.yml",
];
/// The `package.json` scripts a proposal runs, in order, when there are
/// any; otherwise `check`, when there is one.
const SCRIPTS: [&str; 4] = ["lint", "typecheck", "build", "test"];

/// Whether a workflow runs on pull requests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnPulls {
    Yes,
    No,
    /// Orbit can't tell, so it counts as yes.
    #[default]
    Unknown,
}
/// One workflow file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Workflow {
    /// Such as `.github/workflows/ci.yml`.
    pub path: String,
    pub on_pulls: OnPulls,
    /// Whether Orbit checked it for risky settings, the most severe it
    /// found, how many more there are, and why it couldn't check it when it
    /// couldn't (see `crate::risks`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub checked: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<crate::risks::Risk>,
    #[serde(skip_serializing_if = "crate::problems::is_zero")]
    pub more: u32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub why: String,
}
/// What Orbit read about a repository's CI at its default branch's latest
/// commit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Setup {
    pub repo: String,
    pub branch: String,
    /// The commit it was read at.
    pub sha: String,
    /// When Orbit read it, by Orbit's clock.
    pub read_at: String,
    pub workflows: Vec<Workflow>,
    /// Another CI service's files at the top, such as `.circleci`.
    pub others: Vec<String>,
    /// The workflow Orbit proposes, and the files it is built from. Empty
    /// when Orbit can't tell how the repository builds.
    pub proposal: String,
    pub from: Vec<String>,
    /// Why Orbit couldn't read it, how many times in a row it couldn't, and
    /// when it tries again.
    pub error: Option<String>,
    pub tries: u32,
    pub retry_at: Option<String>,
    /// Why Orbit knows something runs checks here, from checks it saw, such
    /// as on a pull request. Kept as Orbit reads newer commits, so it
    /// never says nothing runs them once it has seen otherwise.
    pub ran: Option<String>,
    /// Whether GitHub said the repository is private, read with its files.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
    /// Which checks of its workflow files this read made (`CHECKS`).
    pub checks: u32,
    /// The tools that are installed and ran on its workflow files, and why
    /// one couldn't check them (`crate::lint`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolRun>,
}
/// A tool that ran on a commit's workflow files, and why it couldn't check
/// them when it couldn't.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolRun {
    /// `actionlint` or `zizmor`.
    pub tool: crate::risks::RiskKind,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}
/// What a repository's files say about its CI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// A workflow runs on pull requests, or another CI service's files are
    /// there: the path or file.
    Covered(String),
    /// Orbit can't tell whether this workflow runs on pull requests.
    Unknown(String),
    /// No workflows at all.
    NoWorkflows,
    /// Workflows, none of which run on pull requests.
    NotOnPulls,
}
impl Setup {
    /// What its files say, once read.
    pub fn verdict(&self) -> Option<Verdict> {
        if self.error.is_some() {
            return None;
        }
        let yes = self.workflows.iter().find(|w| w.on_pulls == OnPulls::Yes);
        if let Some(w) = yes {
            return Some(Verdict::Covered(w.path.clone()));
        }
        if let Some(other) = self.others.first() {
            return Some(Verdict::Covered(other.clone()));
        }
        let unknown = self
            .workflows
            .iter()
            .find(|w| w.on_pulls == OnPulls::Unknown);
        if let Some(w) = unknown {
            return Some(Verdict::Unknown(w.path.clone()));
        }
        Some(if self.workflows.is_empty() {
            Verdict::NoWorkflows
        } else {
            Verdict::NotOnPulls
        })
    }
    /// What a problem about it keeps: the workflows there and the proposal.
    pub fn found(&self) -> Found {
        Found {
            workflows: self.workflows.iter().map(|w| w.path.clone()).collect(),
            proposal: self.proposal.clone(),
            from: self.from.clone(),
        }
    }
}
/// What Orbit found in a repository with no CI on pull requests, for its
/// problem and Orbit's issue.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Found {
    /// The workflows there, none of which run on pull requests; empty when
    /// there are none.
    pub workflows: Vec<String>,
    /// The workflow Orbit proposes, and the files it is built from.
    pub proposal: String,
    pub from: Vec<String>,
}

/// The GraphQL query for a commit's workflow files, the names at its top,
/// and its `package.json`, by `<commit>:<path>` expressions.
pub fn query() -> &'static str {
    "query($owner: String!, $name: String!, $workflows: String!, $root: String!, $package: String!) { \
repository(owner: $owner, name: $name) { isPrivate visibility \
workflows: object(expression: $workflows) { ... on Tree { entries { name type object { ... on Blob { text isTruncated isBinary } } } } } \
root: object(expression: $root) { ... on Tree { entries { name type } } } \
package: object(expression: $package) { ... on Blob { text isTruncated } } } }"
}

/// The repositories whose files Orbit should read now, at most `PER_PASS`:
/// each whose default branch's latest commit, or name, Orbit hasn't read
/// them at, and those due again after an error, the ones tried least
/// first. A rename changes the proposal, which names the branch.
pub fn wanted(
    ci: &[ci::RepoCi],
    setups: &[Setup],
    now: DateTime<Utc>,
) -> Vec<(String, String, String)> {
    let mut due: Vec<(u32, &ci::RepoCi)> = ci
        .iter()
        .filter(|c| valid_sha(&c.head.sha) && !c.branch.is_empty())
        .filter_map(
            |c| match setups.iter().find(|s| same_repo(&s.repo, &c.repo)) {
                Some(s) if s.sha == c.head.sha && s.branch == c.branch => {
                    let due = s.error.is_some()
                        && s.retry_at
                            .as_deref()
                            .and_then(time)
                            .is_none_or(|at| now >= at);
                    // Read before today's checks of its files.
                    let older = s.error.is_none() && s.checks < CHECKS;
                    (due || older).then_some((s.tries, c))
                }
                _ => Some((0, c)),
            },
        )
        .collect();
    due.sort_by_key(|(tries, _)| *tries);
    due.into_iter()
        .take(PER_PASS)
        .map(|(_, c)| (c.repo.clone(), c.branch.clone(), c.head.sha.clone()))
        .collect()
}

/// Reads what a repository's files at `sha` say about its CI.
/// Reads what `query` asks at a commit, with the text of each workflow file
/// Orbit checked, for `crate::lint`.
pub async fn read(
    repo: &str,
    branch: &str,
    sha: &str,
) -> Result<(Setup, Vec<crate::lint::File>), String> {
    validate_repo(repo)?;
    if !valid_sha(sha) {
        return Err("Invalid commit".into());
    }
    let (owner, name) = repo.split_once('/').ok_or("Invalid repository")?;
    let v = crate::github::api(
        "graphql",
        "POST",
        &[
            ("query", query().to_string()),
            ("owner", owner.to_string()),
            ("name", name.to_string()),
            ("workflows", format!("{sha}:{FOLDER}")),
            ("root", format!("{sha}:")),
            ("package", format!("{sha}:package.json")),
        ],
        None,
    )
    .await?;
    let setup = understand(repo, branch, sha, &v, Utc::now())?;
    let files = texts(&v)
        .filter(|(path, _)| setup.workflows.iter().any(|w| w.checked && w.path == *path))
        .collect();
    Ok((setup, files))
}
/// Each workflow file's path and text in GitHub's answer to `query`, when
/// GitHub sent all of it.
fn texts(v: &Value) -> impl Iterator<Item = crate::lint::File> + '_ {
    v["data"]["repository"]["workflows"]["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["type"] == "blob")
        .filter_map(|e| {
            let blob = &e["object"];
            let complete = blob["isTruncated"] != true && blob["isBinary"] != true;
            let text = blob["text"].as_str().filter(|_| complete)?;
            Some((
                format!("{FOLDER}/{}", e["name"].as_str()?),
                text.to_string(),
            ))
        })
}

/// What GitHub's answer to `query` says.
pub fn understand(
    repo: &str,
    branch: &str,
    sha: &str,
    v: &Value,
    now: DateTime<Utc>,
) -> Result<Setup, String> {
    ci::refused(v)?;
    let r = &v["data"]["repository"];
    if r.is_null() {
        return Err("GitHub didn't return the repository.".into());
    }
    let Some(top) = r["root"]["entries"].as_array() else {
        return Err(format!(
            "GitHub didn't return the files at {}.",
            ci::short(sha)
        ));
    };
    let names: Vec<String> = top
        .iter()
        .filter_map(|e| e["name"].as_str())
        .map(str::to_string)
        .collect();
    let owner = repo.split_once('/').map_or(repo, |(owner, _)| owner);
    // Only files directly in the folder are workflows; GitHub runs no other.
    let workflows = r["workflows"]["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["type"] == "blob")
        .filter_map(|e| {
            let file = e["name"].as_str()?;
            if !(file.ends_with(".yml") || file.ends_with(".yaml")) {
                return None;
            }
            let blob = &e["object"];
            let text = blob["text"]
                .as_str()
                .filter(|_| blob["isTruncated"] != true && blob["isBinary"] != true);
            let path = format!("{FOLDER}/{file}");
            let checked = match text {
                Some(text) => crate::risks::check(&path, text, owner),
                None => Err("GitHub didn't send all of it".into()),
            };
            let (checked, found, why) = match checked {
                Ok(found) => (true, found, String::new()),
                Err(why) => (false, Default::default(), why),
            };
            Some(Workflow {
                on_pulls: text.map_or(OnPulls::Unknown, on_pulls),
                path,
                checked,
                risks: found.risks,
                more: found.more,
                why,
            })
        })
        .collect();
    let others = OTHER_CI
        .iter()
        .filter(|f| names.iter().any(|n| n == *f))
        .map(|f| f.to_string())
        .collect();
    let package = &r["package"];
    let package = package["text"]
        .as_str()
        .filter(|_| package["isTruncated"] != true);
    let (proposal, from) = propose(&names, package, branch);
    // Internal counts as not private, as more people than the repository's
    // own can read it.
    let private = r["isPrivate"] == true && r["visibility"].as_str().is_none_or(|v| v == "PRIVATE");
    Ok(Setup {
        repo: repo.to_string(),
        branch: branch.to_string(),
        sha: sha.to_string(),
        read_at: stamp(now),
        workflows,
        others,
        proposal,
        from,
        private,
        checks: CHECKS,
        ..Default::default()
    })
}

/// Records what Orbit read for a repository at `sha`, keeping the checks
/// it saw there before (`Setup::ran`). After an error, it tries again
/// after a wait, a minute doubling up to `logs::MAX_RETRY`.
pub fn record(
    setups: &mut Vec<Setup>,
    repo: &str,
    branch: &str,
    sha: &str,
    result: Result<Setup, String>,
    now: DateTime<Utc>,
) {
    let known = setups.iter().position(|s| same_repo(&s.repo, repo));
    let ran = known.and_then(|i| setups[i].ran.clone());
    let mut setup = match result {
        Ok(setup) => setup,
        Err(error) => {
            let before = known.map(|i| &setups[i]).filter(|s| s.sha == sha);
            let tries = before.map_or(0, |s| s.tries) + 1;
            let wait = Duration::minutes(1i64 << (tries - 1).min(6)).min(logs::MAX_RETRY);
            Setup {
                repo: repo.to_string(),
                branch: branch.to_string(),
                sha: sha.to_string(),
                error: Some(format!(
                    "Orbit couldn't read the workflows yet: {}. It tries again later.",
                    logs::short(&error)
                )),
                tries,
                retry_at: Some(stamp(now + wait)),
                ..Default::default()
            }
        }
    };
    setup.ran = ran;
    match known {
        Some(i) => setups[i] = setup,
        None => setups.push(setup),
    }
}

/// Whether a workflow runs on pull requests, from its `on:` setting: when
/// it names `pull_request` or `pull_request_target`, or `push` for any
/// branch a pull request may come from. Anything Orbit can't read for sure
/// is `Unknown`, never `No`.
pub fn on_pulls(text: &str) -> OnPulls {
    let Some(events) = events(text) else {
        return OnPulls::Unknown;
    };
    let runs: Vec<Option<bool>> = events.iter().map(Event::on_pulls).collect();
    if runs.contains(&Some(true)) {
        OnPulls::Yes
    } else if runs.contains(&None) {
        OnPulls::Unknown
    } else {
        OnPulls::No
    }
}

/// An event a workflow's `on:` names, with its settings: on its line, or
/// the lines under it.
#[derive(Debug)]
struct Event<'a> {
    name: String,
    value: &'a str,
    under: Vec<(usize, &'a str)>,
}
impl Event<'_> {
    /// Whether it runs on pull requests, or None when Orbit can't tell.
    fn on_pulls(&self) -> Option<bool> {
        match self.name.as_str() {
            "pull_request" | "pull_request_target" => Some(true),
            "push" => self.push(),
            _ => Some(false),
        }
    }
    /// Whether a `push` runs for a pull request's branch: unless it is only
    /// for tags, or for branches named outright, which Orbit takes to be
    /// where pull requests go rather than where they come from.
    fn push(&self) -> Option<bool> {
        if !self.under.is_empty() {
            if !self.value.is_empty() {
                return None;
            }
        } else {
            return matches!(self.value, "" | "{}" | "null" | "~").then_some(true);
        }
        let indent = self.under[0].0;
        let mut filters: Vec<(&str, Vec<String>)> = vec![];
        for (i, (at, line)) in self.under.iter().enumerate() {
            if *at < indent {
                return None;
            }
            if *at > indent {
                continue;
            }
            let (key, value) = split_key(line)?;
            let mut list = match value {
                "" => vec![],
                value => flow_list(value)?,
            };
            if value.is_empty() {
                for (_, item) in self.under[i + 1..].iter().take_while(|(d, _)| *d > indent) {
                    let item = item.strip_prefix('-')?.trim();
                    if item.is_empty() {
                        return None;
                    }
                    list.push(unquote(item).to_string());
                }
            }
            filters.push((key, list));
        }
        let has = |name: &str| filters.iter().any(|(k, _)| *k == name);
        for (key, _) in &filters {
            let known = [
                "branches",
                "branches-ignore",
                "tags",
                "tags-ignore",
                "paths",
                "paths-ignore",
            ];
            if !known.contains(key) {
                return None;
            }
        }
        if has("branches-ignore") {
            return Some(true);
        }
        if let Some((_, branches)) = filters.iter().find(|(k, _)| *k == "branches") {
            let glob = |b: &String| b.chars().any(|c| matches!(c, '*' | '?' | '+' | '[' | '!'));
            return Some(branches.iter().any(glob));
        }
        // Only tags, or every branch.
        Some(!(has("tags") || has("tags-ignore")))
    }
}

/// The events a workflow's `on:` names, or None when Orbit can't read it.
fn events(text: &str) -> Option<Vec<Event<'_>>> {
    let lines = lines(text)?;
    let tops: Vec<usize> = (0..lines.len()).filter(|i| lines[*i].0 == 0).collect();
    let on: Vec<usize> = tops
        .iter()
        .copied()
        .filter(|i| split_key(lines[*i].1).is_some_and(|(key, _)| key == "on"))
        .collect();
    let [start] = on[..] else {
        return None;
    };
    let end = tops
        .iter()
        .copied()
        .find(|i| *i > start)
        .unwrap_or(lines.len());
    let (_, value) = split_key(lines[start].1)?;
    let block = &lines[start + 1..end];
    if !value.is_empty() {
        if !block.is_empty() {
            return None;
        }
        let names = if value.starts_with('[') {
            flow_list(value)?
        } else {
            vec![unquote(value).to_string()]
        };
        return names
            .into_iter()
            .map(|name| {
                valid_event(&name).then_some(Event {
                    name,
                    value: "",
                    under: vec![],
                })
            })
            .collect();
    }
    let indent = block.first()?.0;
    let mut events = vec![];
    let (mut items, mut keys) = (false, false);
    for (i, (at, line)) in block.iter().enumerate() {
        if *at < indent {
            return None;
        }
        if *at > indent {
            continue;
        }
        if let Some(item) = line.strip_prefix('-') {
            items = true;
            let name = unquote(item.trim()).to_string();
            if !valid_event(&name) {
                return None;
            }
            events.push(Event {
                name,
                value: "",
                under: vec![],
            });
        } else {
            keys = true;
            let (name, value) = split_key(line)?;
            if !valid_event(name) {
                return None;
            }
            let under = block[i + 1..]
                .iter()
                .take_while(|(d, _)| *d > indent)
                .copied()
                .collect();
            events.push(Event {
                name: name.to_string(),
                value,
                under,
            });
        }
    }
    // A list item with settings under it, or a list and keys together,
    // isn't a shape Orbit reads.
    let nested = items && block.iter().any(|(at, _)| *at > indent);
    (!(items && keys) && !nested && !events.is_empty()).then_some(events)
}
/// The lines that hold something, without comments, each with how far it
/// is indented. None when one is indented with tabs or the file holds more
/// than one document.
fn lines(text: &str) -> Option<Vec<(usize, &str)>> {
    let mut out = vec![];
    for line in text.lines() {
        let line = line.trim_end();
        let content = line.trim_start();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let indent = line.len() - content.len();
        if line[..indent].contains('\t') {
            return None;
        }
        if content == "---" || content == "..." {
            if out.is_empty() {
                continue;
            }
            return None;
        }
        out.push((indent, uncomment(content)));
    }
    Some(out)
}
/// A line without the comment at its end: from a `#` after a space,
/// outside quotes.
fn uncomment(line: &str) -> &str {
    let mut quote = None;
    let mut space = false;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '#') if space => return line[..i].trim_end(),
            _ => {}
        }
        space = c.is_whitespace();
    }
    line
}
/// A `key: value` line's key, without quotes, and its value; the value is
/// empty for `key:`.
fn split_key(line: &str) -> Option<(&str, &str)> {
    let (key, value) = if let Some(key) = line.strip_suffix(':') {
        (key, "")
    } else {
        let (key, value) = line.split_once(": ")?;
        (key, value.trim())
    };
    let key = unquote(key.trim());
    (!key.is_empty() && !key.contains(['"', '\'', ':', ' '])).then_some((key, value))
}
/// `[a, "b"]` as its items; None for anything else, such as a list that
/// goes on to the next line or holds another list.
fn flow_list(value: &str) -> Option<Vec<String>> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    if inner.contains(['[', ']', '{', '}']) {
        return None;
    }
    if inner.trim().is_empty() {
        return Some(vec![]);
    }
    inner
        .split(',')
        .map(|item| {
            let item = unquote(item.trim());
            (!item.is_empty()).then(|| item.to_string())
        })
        .collect()
}
fn unquote(text: &str) -> &str {
    for q in ['"', '\''] {
        if let Some(inner) = text.strip_prefix(q).and_then(|t| t.strip_suffix(q)) {
            return inner;
        }
    }
    text
}
fn valid_event(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// The workflow Orbit proposes for a repository with these names at its
/// top and this `package.json`, and the files it is built from: one job on
/// GitHub's Ubuntu runner, for every pull request and each push to
/// `branch`, using only GitHub's own actions. Its commands come from
/// Orbit's own lists, never from the repository's text. Empty when Orbit
/// can't tell how the repository builds.
pub fn propose(names: &[String], package: Option<&str>, branch: &str) -> (String, Vec<String>) {
    let has = |name: &str| names.iter().any(|n| n == name);
    let mut from: Vec<String> = vec![];
    let mut steps: Vec<String> = vec![];
    if has("package.json") {
        let locks = [
            ("package-lock.json", "npm"),
            ("npm-shrinkwrap.json", "npm"),
            ("pnpm-lock.yaml", "pnpm"),
            ("yarn.lock", "yarn"),
        ];
        let lock = locks.iter().find(|(file, _)| has(file));
        // Bun isn't on GitHub's runner.
        let bun = has("bun.lock") || has("bun.lockb");
        if !bun || lock.is_some() {
            from.push("package.json".into());
            let manager = lock.map_or("npm", |(_, m)| *m);
            let mut node = vec![
                "      - uses: actions/setup-node@v5".to_string(),
                "        with:".into(),
            ];
            match [".nvmrc", ".node-version"].into_iter().find(|f| has(f)) {
                Some(file) => {
                    node.push(format!("          node-version-file: {file}"));
                    from.push(file.into());
                }
                None => node.push("          node-version: lts/*".into()),
            }
            let mut cached = false;
            if let Some((file, _)) = lock {
                from.push(file.to_string());
                if manager == "npm" {
                    node.push("          cache: npm".into());
                    cached = true;
                }
            }
            // Otherwise setup-node caches on its own when `package.json`
            // names a package manager, which fails without a lock file, or
            // before Corepack has set up pnpm or Yarn.
            if !cached {
                node.push("          package-manager-cache: false".into());
            }
            steps.extend(node);
            if manager != "npm" {
                steps.push("      - run: corepack enable".into());
            }
            let install = match (manager, lock.is_some()) {
                ("npm", true) => "npm ci",
                ("pnpm", _) => "pnpm install --frozen-lockfile",
                ("yarn", _) => "yarn install --frozen-lockfile",
                _ => "npm install",
            };
            steps.push(format!("      - run: {install}"));
            for script in scripts(package) {
                let run = if script == "test" {
                    format!("{manager} test")
                } else {
                    format!("{manager} run {script}")
                };
                steps.push(format!("      - run: {run}"));
            }
        }
    }
    if has("Cargo.toml") {
        from.push("Cargo.toml".into());
        let locked = if has("Cargo.lock") {
            from.push("Cargo.lock".into());
            " --locked"
        } else {
            ""
        };
        steps.push(format!("      - run: cargo test{locked}"));
    }
    if has("go.mod") {
        from.push("go.mod".into());
        steps.extend([
            "      - uses: actions/setup-go@v5".to_string(),
            "        with:".into(),
            "          go-version-file: go.mod".into(),
            "      - run: go test ./...".into(),
        ]);
    }
    if steps.is_empty() {
        return (String::new(), vec![]);
    }
    // Only a branch name that needs no quoting goes in, and with any other
    // the workflow runs on pull requests alone.
    let plain = branch.starts_with(|c: char| c.is_ascii_alphabetic())
        && branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'));
    let push = if plain {
        format!("  push:\n    branches:\n      - {branch}\n")
    } else {
        String::new()
    };
    let workflow = format!(
        "name: CI\n\non:\n  pull_request:\n{push}\npermissions:\n  contents: read\n\njobs:\n  check:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v5\n{}\n",
        steps.join("\n")
    );
    (workflow, from)
}
/// The scripts a proposal runs from a `package.json`: those of `SCRIPTS`
/// it has, or `check`. npm's placeholder `test` script doesn't count.
fn scripts(package: Option<&str>) -> Vec<&'static str> {
    let Some(v) = package.and_then(|p| serde_json::from_str::<Value>(p).ok()) else {
        return vec![];
    };
    let has = |name: &str| {
        v["scripts"][name]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty() && !s.contains("no test specified"))
    };
    let found: Vec<&str> = SCRIPTS.into_iter().filter(|s| has(s)).collect();
    if found.is_empty() && has("check") {
        return vec!["check"];
    }
    found
}

/// Forgets what Orbit read for repositories it no longer watches.
pub fn prune(setups: &mut Vec<Setup>, connected: &[String]) {
    setups.retain(|s| connected.iter().any(|r| same_repo(r, &s.repo)));
}

#[cfg(test)]
#[path = "workflows_tests.rs"]
mod tests;
