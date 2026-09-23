use crate::types::*;
use serde_json::{json, Value};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

pub async fn run(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    input: Option<String>,
) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Ignore Git hooks and prevent credential prompts from hanging the background worker.
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GH_PROMPT_DISABLED", "1");
    if let Some(p) = cwd {
        cmd.current_dir(p);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Cannot start {program}: {e}"))?;
    if let Some(body) = input {
        child
            .stdin
            .take()
            .ok_or("Missing stdin")?
            .write_all(body.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
    }
    let out = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output())
        .await
        .map_err(|_| format!("{program} timed out after 120 seconds."))?
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "{program}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
pub async fn api(
    endpoint: &str,
    method: &str,
    fields: &[(&str, String)],
    body: Option<Value>,
) -> Result<Value, String> {
    let mut args = vec![
        "api".into(),
        "--hostname".into(),
        "github.com".into(),
        "--method".into(),
        method.into(),
        endpoint.into(),
    ];
    for (key, value) in fields {
        args.extend(["-f".into(), format!("{key}={value}")]);
    }
    if body.is_some() {
        args.extend(["--input".into(), "-".into()]);
    }
    let out = run("gh", &args, None, body.map(|v| v.to_string())).await?;
    serde_json::from_str(&out).map_err(|e| format!("Invalid GitHub response: {e}"))
}
pub async fn user() -> Result<String, String> {
    let v = api("user", "GET", &[], None).await?;
    v["login"]
        .as_str()
        .map(str::to_owned)
        .ok_or("GitHub did not return a username.".into())
}
pub const CLOSED: &str = "This pull request is no longer open.";
pub async fn pull(repo: &str, number: u64, login: &str) -> Result<PullRequest, String> {
    validate_repo(repo)?;
    let v = api(&format!("repos/{repo}/pulls/{number}"), "GET", &[], None).await?;
    if v["state"] != "open" {
        return Err(CLOSED.into());
    }
    parse_pull(&v, repo, login)
}
/// Open pull requests in one repository. The list endpoint omits the size
/// statistics (additions, deletions, changed files), which stay zero here.
pub async fn open_pulls(repo: &str, login: &str) -> Result<Vec<PullRequest>, String> {
    validate_repo(repo)?;
    let mut prs = vec![];
    for page in 1..=10 {
        let v = api(
            &format!("repos/{repo}/pulls"),
            "GET",
            &[
                ("state", "open".into()),
                ("per_page", "100".into()),
                ("page", page.to_string()),
            ],
            None,
        )
        .await?;
        let items = v.as_array().ok_or("Invalid pull request list")?;
        for item in items {
            prs.push(parse_pull(item, repo, login)?);
        }
        if items.len() < 100 {
            break;
        }
    }
    Ok(prs)
}
fn parse_pull(v: &Value, repo: &str, login: &str) -> Result<PullRequest, String> {
    let number = v["number"].as_u64().ok_or("Invalid PR number")?;
    let text = |key: &str| v[key].as_str().unwrap_or("").to_string();
    let pr = PullRequest {
        id: format!("{repo}#{number}"),
        repo: repo.into(),
        number,
        title: text("title"),
        author: v["user"]["login"].as_str().unwrap_or("").into(),
        body: text("body"),
        url: text("html_url"),
        head_sha: v["head"]["sha"].as_str().unwrap_or("").into(),
        base_sha: v["base"]["sha"].as_str().unwrap_or("").into(),
        head_ref: v["head"]["ref"].as_str().unwrap_or("").into(),
        base_ref: v["base"]["ref"].as_str().unwrap_or("").into(),
        updated_at: text("updated_at"),
        draft: v["draft"].as_bool().unwrap_or(false),
        additions: v["additions"].as_u64().unwrap_or(0),
        deletions: v["deletions"].as_u64().unwrap_or(0),
        changed_files: v["changed_files"].as_u64().unwrap_or(0),
        review_requested: v["requested_reviewers"]
            .as_array()
            .is_some_and(|a| a.iter().any(|u| u["login"] == login)),
        labels: v["labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|l| l["name"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        head_repo: v["head"]["repo"]["full_name"]
            .as_str()
            .unwrap_or("")
            .into(),
        author_association: text("author_association"),
    };
    if !valid_sha(&pr.head_sha) || !valid_sha(&pr.base_sha) {
        return Err("GitHub returned an invalid commit SHA.".into());
    }
    Ok(pr)
}
/// Repositories the signed-in user owns, collaborates on, or reaches through an
/// organization, most recently pushed first.
pub async fn repositories() -> Result<Vec<RepositorySummary>, String> {
    let mut repos = vec![];
    for page in 1..=10 {
        let v = api(
            "user/repos",
            "GET",
            &[
                (
                    "affiliation",
                    "owner,collaborator,organization_member".into(),
                ),
                ("sort", "pushed".into()),
                ("per_page", "100".into()),
                ("page", page.to_string()),
            ],
            None,
        )
        .await?;
        let items = v.as_array().ok_or("Invalid repository list")?;
        repos.extend(summarize_repositories(items));
        if items.len() < 100 {
            break;
        }
    }
    Ok(repos)
}
/// Archived repositories are omitted because they cannot receive new pull requests.
fn summarize_repositories(items: &[Value]) -> Vec<RepositorySummary> {
    items
        .iter()
        .filter(|r| r["archived"] != true)
        .filter_map(|r| {
            let name = r["full_name"].as_str()?;
            validate_repo(name).ok()?;
            Some(RepositorySummary {
                name: name.into(),
                // Unknown visibility is shown as public, the more cautious label.
                private: r["private"].as_bool().unwrap_or(false),
                description: r["description"].as_str().unwrap_or("").into(),
                pushed_at: r["pushed_at"].as_str().unwrap_or("").into(),
            })
        })
        .collect()
}
pub async fn files(repo: &str, number: u64) -> Result<Value, String> {
    validate_repo(repo)?;
    let mut files = vec![];
    for page in 1..=30 {
        let v = api(
            &format!("repos/{repo}/pulls/{number}/files"),
            "GET",
            &[("per_page", "100".into()), ("page", page.to_string())],
            None,
        )
        .await?;
        let page = v.as_array().ok_or("Invalid file list")?;
        files.extend(page.iter().cloned());
        if page.len() < 100 {
            break;
        }
    }
    Ok(json!(files))
}
pub async fn existing_comment(pr: &PullRequest, r: &Review) -> Result<Option<String>, String> {
    let marker = format!("<!-- orbit-review:{} -->", r.id);
    // Resolve an uncertain previous POST by looking for our stable marker before retrying.
    for page in 1..=100 {
        let v = api(
            &format!("repos/{}/issues/{}/comments", pr.repo, pr.number),
            "GET",
            &[("per_page", "100".into()), ("page", page.to_string())],
            None,
        )
        .await?;
        let comments = v.as_array().ok_or("Invalid comment list")?;
        for c in comments {
            if c["body"].as_str().is_some_and(|b| b.contains(&marker)) {
                return c["html_url"]
                    .as_str()
                    .map(|url| Some(url.to_string()))
                    .ok_or("Missing comment URL".into());
            }
        }
        if comments.len() < 100 {
            break;
        }
        if page == 100 {
            return Err("Too many comments to safely check for duplicate publication.".into());
        }
    }
    Ok(None)
}
pub async fn publish(pr: &PullRequest, r: &Review) -> Result<String, String> {
    let marker = format!("<!-- orbit-review:{} -->", r.id);
    let body=format!("{marker}\n## Orbit · Codex review\n\nReviewed head `{}` against base `{}`.\n\n{}\n\n---\nModel: {} · Effort: {}",r.head_sha,r.base_sha,r.report,if r.model.is_empty(){"Codex default"}else{&r.model},if r.effort.is_empty(){"default"}else{&r.effort});
    if body.len() > 60000 {
        return Err(
            "Review exceeds GitHub’s comment size limit. Copy it from the chat instead.".into(),
        );
    }
    let v = api(
        &format!("repos/{}/issues/{}/comments", pr.repo, pr.number),
        "POST",
        &[],
        Some(json!({"body":body})),
    )
    .await?;
    v["html_url"]
        .as_str()
        .map(str::to_owned)
        .ok_or("GitHub did not return a comment URL.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_listed_pull_requests() {
        let item = json!({
            "number": 7, "title": "Fix", "body": null, "html_url": "https://github.com/me/app/pull/7",
            "user": {"login": "claude[bot]"}, "author_association": "NONE",
            "head": {"sha": "a".repeat(40), "ref": "fix", "repo": {"full_name": "me/app"}},
            "base": {"sha": "b".repeat(40), "ref": "main"},
            "updated_at": "2026-09-22T00:00:00Z", "draft": false,
            "requested_reviewers": [{"login": "me"}], "labels": [{"name": "bug"}]
        });
        let pr = parse_pull(&item, "me/app", "me").unwrap();
        assert_eq!(pr.id, "me/app#7");
        assert_eq!(pr.author, "claude[bot]");
        assert_eq!(pr.head_repo, "me/app");
        assert_eq!(pr.author_association, "NONE");
        assert!(pr.review_requested);
        assert_eq!(pr.labels, ["bug"]);
        assert_eq!((pr.additions, pr.changed_files), (0, 0));
        let mut bad = item.clone();
        bad["head"]["sha"] = json!("not-a-sha");
        assert!(parse_pull(&bad, "me/app", "me").is_err());
    }
    #[test]
    fn summarizes_accessible_repositories() {
        let items = json!([
            {"full_name":"me/app","private":true,"description":"App","pushed_at":"2026-09-01T00:00:00Z"},
            {"full_name":"me/site","private":false,"description":null},
            {"full_name":"me/old","private":true,"archived":true},
            {"full_name":"me/bad;name","private":true},
            {"full_name":"org/unknown"}
        ]);
        let repos = summarize_repositories(items.as_array().unwrap());
        let names: Vec<_> = repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["me/app", "me/site", "org/unknown"]);
        assert!(repos[0].private);
        assert_eq!(repos[0].description, "App");
        assert!(!repos[1].private);
        assert_eq!(repos[1].description, "");
        assert!(!repos[2].private);
    }
}
