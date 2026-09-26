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
    run_with_timeout(program, args, cwd, input, Duration::from_secs(120)).await
}
pub async fn run_with_timeout(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    input: Option<String>,
    timeout: Duration,
) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(not(unix))]
    cmd.kill_on_drop(true);
    // Ignore Git hooks and prevent credential prompts from hanging the background worker.
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GH_PROMPT_DISABLED", "1");
    if let Some(p) = cwd {
        cmd.current_dir(p);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Cannot start {program}: {e}"))?;
    #[cfg(unix)]
    let mut terminate = TerminateOnDrop(child.id());
    if let Some(body) = input {
        child
            .stdin
            .take()
            .ok_or("Missing stdin")?
            .write_all(body.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
    }
    let waited = tokio::time::timeout(timeout, child.wait_with_output()).await;
    #[cfg(unix)]
    if matches!(waited, Ok(Ok(_))) {
        // The command exited and was reaped; there is nothing left to stop.
        terminate.0 = None;
    }
    let out = waited
        .map_err(|_| format!("{program} timed out after {} seconds.", timeout.as_secs()))?
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "{program}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
/// Stops a command that is abandoned before it finishes (a timeout or a stopped
/// review) with SIGTERM rather than SIGKILL, so git can remove its lock files.
#[cfg(unix)]
struct TerminateOnDrop(Option<u32>);
#[cfg(unix)]
impl Drop for TerminateOnDrop {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }
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
    let items = list_pages(|page| async move {
        let fields = [
            ("state", "open".to_string()),
            ("per_page", "100".into()),
            ("page", page.to_string()),
        ];
        api(&format!("repos/{repo}/pulls"), "GET", &fields, None).await
    })
    .await?;
    items
        .iter()
        .map(|item| parse_pull(item, repo, login))
        .collect()
}
/// Collects up to ten pages of 100 items. Ten full pages are only complete when
/// an eleventh page is empty.
async fn list_pages<F, Fut>(fetch: F) -> Result<Vec<Value>, String>
where
    F: Fn(u32) -> Fut,
    Fut: std::future::Future<Output = Result<Value, String>>,
{
    let mut all = vec![];
    for page in 1..=11 {
        let v = fetch(page).await?;
        let items = v.as_array().ok_or("Invalid pull request list")?;
        if page == 11 {
            if items.is_empty() {
                break;
            }
            return Err(
                "More than 1,000 open pull requests; Orbit lists at most 1,000 per repository."
                    .into(),
            );
        }
        all.extend(items.iter().cloned());
        if items.len() < 100 {
            break;
        }
    }
    Ok(all)
}
fn parse_pull(v: &Value, repo: &str, login: &str) -> Result<PullRequest, String> {
    let number = v["number"].as_u64().ok_or("Invalid PR number")?;
    let text = |key: &str| v[key].as_str().unwrap_or("").to_string();
    let pr = PullRequest {
        id: pr_id(repo, number),
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
        head_repo: v["head"]["repo"]["full_name"].as_str().unwrap_or("").into(),
        author_association: text("author_association"),
        base_repo: v["base"]["repo"]["full_name"].as_str().unwrap_or("").into(),
        head_seen_at: String::new(),
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
/// The PR's conversation comments updated at or after `since`, oldest first,
/// up to 1,000.
pub async fn comments(pr: &PullRequest, since: &str) -> Result<Vec<Value>, String> {
    validate_repo(&pr.repo)?;
    let mut all = vec![];
    for page in 1..=10 {
        let v = api(
            &format!("repos/{}/issues/{}/comments", pr.repo, pr.number),
            "GET",
            &[
                ("since", since.to_string()),
                ("per_page", "100".into()),
                ("page", page.to_string()),
            ],
            None,
        )
        .await?;
        let comments = v.as_array().ok_or("Invalid comment list")?;
        all.extend(comments.iter().cloned());
        if comments.len() < 100 {
            break;
        }
    }
    Ok(all)
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
/// The PR comment for a review, including the marker used to reconcile retries.
fn comment_body(r: &Review) -> String {
    let marker = format!("<!-- orbit-review:{} -->", r.id);
    let selection = crate::models::report_selection_label(r);
    format!(
        "{marker}\n## Orbit · Codex review\n\n\
         Reviewed head `{}` against base `{}`.\n\n{}\n\n---\n{selection}",
        r.head_sha, r.base_sha, r.report
    )
}
pub async fn publish(pr: &PullRequest, r: &Review) -> Result<String, String> {
    let body = comment_body(r);
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
    #[tokio::test]
    async fn enforces_the_given_timeout() {
        let args = ["-c".to_string(), "sleep 5".to_string()];
        let error = run_with_timeout("sh", &args, None, None, Duration::from_millis(50))
            .await
            .unwrap_err();
        assert_eq!(error, "sh timed out after 0 seconds.");
        let args = ["-c".to_string(), "echo ok".to_string()];
        assert_eq!(
            run_with_timeout("sh", &args, None, None, Duration::from_secs(5))
                .await
                .unwrap(),
            "ok\n"
        );
    }
    #[tokio::test]
    async fn asks_abandoned_commands_to_exit_with_sigterm() {
        let d = tempfile::tempdir().unwrap();
        let marker = d.path().join("terminated");
        let script = format!(
            "trap 'touch {}; exit 1' TERM; sleep 5 & wait",
            marker.display()
        );
        let args = ["-c".to_string(), script];
        assert!(
            run_with_timeout("sh", &args, None, None, Duration::from_millis(200))
                .await
                .is_err()
        );
        for _ in 0..50 {
            if marker.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the command did not receive SIGTERM");
    }
    #[test]
    fn formats_the_published_comment() {
        let mut r = Review::new(&crate::types::tests::pr(), &Settings::default(), false);
        r.id = "rid".into();
        r.report = "No findings.".into();
        let expected = format!(
            "<!-- orbit-review:rid -->\n## Orbit · Codex review\n\nReviewed head `{}` against base `{}`.\n\nNo findings.\n\n---\nModel: not recorded · Effort: not recorded",
            "a".repeat(40),
            "b".repeat(40)
        );
        assert_eq!(comment_body(&r), expected);
        r.model = "gpt-x".into();
        r.effort = "high".into();
        assert!(comment_body(&r).ends_with(
            "Model: gpt-x (requested, unverified) · Effort: high (requested, unverified)"
        ));
        r.legacy_chat_selection = Some(ModelSelection {
            model: "chat-only".into(),
            effort: "low".into(),
        });
        assert!(!comment_body(&r).contains("chat-only"));
        r.confirmed_review_selection = Some(ModelSelection {
            model: "confirmed-report".into(),
            effort: "medium".into(),
        });
        assert!(comment_body(&r)
            .ends_with("Model: confirmed-report (confirmed) · Effort: medium (confirmed)"));
    }
    #[tokio::test]
    async fn lists_up_to_exactly_a_thousand_items() {
        let pages = |total: usize| {
            move |page: u32| async move {
                let start = (page as usize - 1) * 100;
                let count = total.saturating_sub(start).min(100);
                Ok(json!(vec![json!({}); count]))
            }
        };
        assert_eq!(list_pages(pages(0)).await.unwrap().len(), 0);
        assert_eq!(list_pages(pages(250)).await.unwrap().len(), 250);
        assert_eq!(list_pages(pages(1000)).await.unwrap().len(), 1000);
        assert!(list_pages(pages(1001))
            .await
            .unwrap_err()
            .contains("More than 1,000"));
    }
    #[test]
    fn parses_listed_pull_requests() {
        let item = json!({
            "number": 7, "title": "Fix", "body": null, "html_url": "https://github.com/me/app/pull/7",
            "user": {"login": "claude[bot]"}, "author_association": "NONE",
            "head": {"sha": "a".repeat(40), "ref": "fix", "repo": {"full_name": "me/app"}},
            "base": {"sha": "b".repeat(40), "ref": "main", "repo": {"full_name": "me/app"}},
            "updated_at": "2026-09-22T00:00:00Z", "draft": false,
            "requested_reviewers": [{"login": "me"}], "labels": [{"name": "bug"}]
        });
        let pr = parse_pull(&item, "me/app", "me").unwrap();
        assert_eq!(pr.id, "me/app#7");
        assert_eq!(pr.author, "claude[bot]");
        assert_eq!(pr.head_repo, "me/app");
        assert_eq!(pr.author_association, "NONE");
        assert_eq!(pr.base_repo, "me/app");
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
