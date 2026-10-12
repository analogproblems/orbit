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
#[cfg(test)]
tokio::task_local! {
    /// A stand-in for `gh` in tests, so no test reaches GitHub.
    pub static GH: std::path::PathBuf;
}
fn gh() -> String {
    #[cfg(test)]
    return GH
        .try_with(|p| p.to_string_lossy().into_owned())
        // A test that sets no stand-in can't reach GitHub either.
        .unwrap_or_else(|_| "gh-is-not-available-in-tests".into());
    #[cfg(not(test))]
    "gh".into()
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
    let out = run(&gh(), &args, None, body.map(|v| v.to_string())).await?;
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
        let (items, more) = repositories_page(page).await?;
        repos.extend(items);
        if !more {
            break;
        }
    }
    Ok(repos)
}
/// The 100 repositories the user pushed to most recently, as `repositories`
/// lists them.
pub async fn recent_repositories() -> Result<Vec<RepositorySummary>, String> {
    Ok(repositories_page(1).await?.0)
}
/// A page of `repositories`, and whether there may be more.
async fn repositories_page(page: u32) -> Result<(Vec<RepositorySummary>, bool), String> {
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
    Ok((summarize_repositories(items), items.len() == 100))
}
/// Downloads `repo` into `folder`, which must not exist yet, with the
/// GitHub CLI signed in as you.
pub async fn clone(repo: &str, folder: &Path) -> Result<(), String> {
    validate_repo(repo)?;
    let args = [
        "repo".into(),
        "clone".into(),
        repo.into(),
        folder.to_string_lossy().into_owned(),
    ];
    // A first clone of a large repository can take many minutes.
    run_with_timeout(&gh(), &args, None, None, Duration::from_secs(30 * 60))
        .await
        .map(|_| ())
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
/// A repository's 100 most recently updated pull requests in any state,
/// newest first.
pub async fn recent_pulls(repo: &str) -> Result<Vec<Value>, String> {
    validate_repo(repo)?;
    let v = api(
        &format!("repos/{repo}/pulls"),
        "GET",
        &[
            ("state", "all".into()),
            ("sort", "updated".into()),
            ("direction", "desc".into()),
            ("per_page", "100".into()),
            ("page", "1".into()),
        ],
        None,
    )
    .await?;
    v.as_array()
        .cloned()
        .ok_or("Invalid pull request list".into())
}
/// Every item of a list GitHub pages, up to 1,000.
async fn every_page(endpoint: &str) -> Result<Vec<Value>, String> {
    let mut all = vec![];
    for page in 1..=10 {
        let v = api(
            endpoint,
            "GET",
            &[("per_page", "100".into()), ("page", page.to_string())],
            None,
        )
        .await?;
        let items = v.as_array().ok_or("Invalid list")?;
        all.extend(items.iter().cloned());
        if items.len() < 100 {
            break;
        }
    }
    Ok(all)
}
/// Everything Orbit keeps of a pull request's history, as GitHub returns it.
pub struct PullHistory {
    pub pr: Value,
    pub issue_comments: Vec<Value>,
    pub reviews: Vec<Value>,
    pub review_comments: Vec<Value>,
    pub commits: Vec<Value>,
    pub files: Vec<Value>,
}
/// A pull request in any state with its conversation, reviews, comments on
/// changed lines, commits and changed files.
pub async fn pull_history(repo: &str, number: u64) -> Result<PullHistory, String> {
    validate_repo(repo)?;
    let pull = format!("repos/{repo}/pulls/{number}");
    let (comments, reviews, line_comments, commits) = (
        format!("repos/{repo}/issues/{number}/comments"),
        format!("{pull}/reviews"),
        format!("{pull}/comments"),
        format!("{pull}/commits"),
    );
    let (pr, issue_comments, reviews, review_comments, commits, files) = tokio::try_join!(
        api(&pull, "GET", &[], None),
        every_page(&comments),
        every_page(&reviews),
        every_page(&line_comments),
        every_page(&commits),
        files(repo, number),
    )?;
    if pr["number"].as_u64() != Some(number) {
        return Err("GitHub returned a different pull request.".into());
    }
    Ok(PullHistory {
        pr,
        issue_comments,
        reviews,
        review_comments,
        commits,
        files: files.as_array().cloned().unwrap_or_default(),
    })
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
/// The PR's comments on the diff updated at or after `since`, up to 1,000.
/// Replies in a thread carry `in_reply_to_id`, the thread's first comment.
pub async fn review_comments_since(pr: &PullRequest, since: &str) -> Result<Vec<Value>, String> {
    validate_repo(&pr.repo)?;
    let mut all = vec![];
    for page in 1..=10 {
        let v = api(
            &format!("repos/{}/pulls/{}/comments", pr.repo, pr.number),
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
/// A comment or review with a body containing `marker`, by its URL and ID.
fn find_marker(items: &[Value], marker: &str) -> Result<Option<(String, u64)>, String> {
    let Some(item) = items
        .iter()
        .find(|c| c["body"].as_str().is_some_and(|b| b.contains(marker)))
    else {
        return Ok(None);
    };
    let url = item["html_url"].as_str().ok_or("Missing comment URL")?;
    Ok(Some((url.to_string(), item["id"].as_u64().unwrap_or(0))))
}
/// Pages through a list looking for Orbit's marker, up to 10,000 items.
async fn search_marker(endpoint: &str, marker: &str) -> Result<Option<(String, u64)>, String> {
    for page in 1..=100 {
        let v = api(
            endpoint,
            "GET",
            &[("per_page", "100".into()), ("page", page.to_string())],
            None,
        )
        .await?;
        let items = v.as_array().ok_or("Invalid comment list")?;
        if let Some(found) = find_marker(items, marker)? {
            return Ok(Some(found));
        }
        if items.len() < 100 {
            return Ok(None);
        }
    }
    Err("Too many comments to safely check for duplicate publication.".into())
}
/// Where an earlier, uncertain attempt published the review, if it did: a PR
/// review (with its ID) or an issue comment (ID None).
pub async fn existing_publication(
    pr: &PullRequest,
    r: &Review,
) -> Result<Option<(String, Option<u64>)>, String> {
    validate_repo(&pr.repo)?;
    let marker = format!("<!-- orbit-review:{} -->", r.id);
    let reviews = format!("repos/{}/pulls/{}/reviews", pr.repo, pr.number);
    if let Some((url, id)) = search_marker(&reviews, &marker).await? {
        return Ok(Some((url, Some(id))));
    }
    let comments = format!("repos/{}/issues/{}/comments", pr.repo, pr.number);
    Ok(search_marker(&comments, &marker)
        .await?
        .map(|(url, _)| (url, None)))
}
/// A hidden line with Orbit's verdict that agents can read from the comment.
pub(crate) fn verdict_marker(r: &Review, round: Option<u32>) -> String {
    let Some(verdict) = r.orbit_verdict else {
        return String::new();
    };
    let verdict = match verdict {
        Verdict::Clean => "clean",
        Verdict::ChangesRequested => "changes_requested",
    };
    let round = round.map(|n| format!(" round={n}")).unwrap_or_default();
    format!(
        "\n<!-- orbit-verdict: {verdict}{round} blocking={} -->",
        r.blocking.unwrap_or(0)
    )
}
/// The PR comment for a review, including the marker used to reconcile retries
/// and, when the review has one, Orbit's verdict.
fn comment_body(r: &Review, round: Option<u32>) -> String {
    let marker = format!("<!-- orbit-review:{} -->", r.id);
    let marker = marker + &verdict_marker(r, round);
    let selection = crate::models::report_selection_label(r);
    let declined: String = r
        .declined_approvals
        .iter()
        .map(|note| format!("{note}\n\n"))
        .collect();
    format!(
        "{marker}\n## Orbit · {} review\n\n\
         Reviewed head `{}` against base `{}`.\n\n{}\n\n{declined}---\n{selection}",
        r.provider.label(),
        r.head_sha,
        r.base_sha,
        r.report
    )
}
pub async fn publish(pr: &PullRequest, r: &Review, round: Option<u32>) -> Result<String, String> {
    let body = comment_body(r, round);
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
/// Posts a PR review of `sha` and returns its URL and ID. The event is always
/// COMMENT: GitHub refuses approvals from the PR's author, and the verdict is
/// in the commit status.
pub async fn create_review(
    pr: &PullRequest,
    sha: &str,
    body: &str,
    comments: &[Value],
) -> Result<(String, u64), String> {
    validate_repo(&pr.repo)?;
    let v = api(
        &format!("repos/{}/pulls/{}/reviews", pr.repo, pr.number),
        "POST",
        &[],
        Some(json!({"commit_id": sha, "event": "COMMENT", "body": body, "comments": comments})),
    )
    .await?;
    let url = v["html_url"]
        .as_str()
        .ok_or("GitHub did not return a review URL.")?;
    let id = v["id"]
        .as_u64()
        .ok_or("GitHub did not return a review ID.")?;
    Ok((url.to_string(), id))
}
/// Whether GitHub refused a request as unprocessable, which for a review means
/// it posted nothing, usually because a comment targets a line outside the diff.
pub fn unprocessable(error: &str) -> bool {
    error.contains("HTTP 422")
}
/// The inline comments of one review, up to 3,000.
pub async fn review_comments(pr: &PullRequest, review_id: u64) -> Result<Vec<Value>, String> {
    validate_repo(&pr.repo)?;
    let mut all = vec![];
    for page in 1..=30 {
        let v = api(
            &format!(
                "repos/{}/pulls/{}/reviews/{review_id}/comments",
                pr.repo, pr.number
            ),
            "GET",
            &[("per_page", "100".into()), ("page", page.to_string())],
            None,
        )
        .await?;
        let items = v.as_array().ok_or("Invalid comment list")?;
        all.extend(items.iter().cloned());
        if items.len() < 100 {
            break;
        }
    }
    Ok(all)
}
/// Sets the `orbit/review` status on a commit.
pub async fn set_status(repo: &str, sha: &str, status: &CommitStatus) -> Result<(), String> {
    api(
        &format!("repos/{repo}/statuses/{sha}"),
        "POST",
        &[],
        Some(json!({
            "state": status.state,
            "description": status.description,
            "context": "orbit/review",
        })),
    )
    .await
    .map(|_| ())
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
        assert_eq!(comment_body(&r, None), expected);
        r.model = "gpt-x".into();
        r.effort = "high".into();
        assert!(comment_body(&r, None).ends_with(
            "Model: gpt-x (requested, unverified) · Effort: high (requested, unverified)"
        ));
        r.legacy_chat_selection = Some(ModelSelection {
            model: "chat-only".into(),
            effort: "low".into(),
        });
        assert!(!comment_body(&r, None).contains("chat-only"));
        r.confirmed_review_selection = Some(ModelSelection {
            model: "confirmed-report".into(),
            effort: "medium".into(),
        });
        assert!(comment_body(&r, None)
            .ends_with("Model: confirmed-report (confirmed) · Effort: medium (confirmed)"));
        // Other reviewers are named in the heading, not just the footer.
        r.provider = Provider::Antigravity;
        assert!(comment_body(&r, None).contains("\n## Orbit · Antigravity review\n"));
    }
    #[test]
    fn adds_orbits_verdict_as_a_hidden_line() {
        let mut r = Review::new(&crate::types::tests::pr(), &Settings::default(), true);
        r.id = "rid".into();
        assert!(comment_body(&r, Some(2)).starts_with("<!-- orbit-review:rid -->\n## Orbit"));
        r.orbit_verdict = Some(Verdict::ChangesRequested);
        r.blocking = Some(1);
        assert!(comment_body(&r, Some(2)).starts_with(
            "<!-- orbit-review:rid -->\n<!-- orbit-verdict: changes_requested round=2 blocking=1 -->\n## Orbit"
        ));
        r.orbit_verdict = Some(Verdict::Clean);
        r.blocking = Some(0);
        assert!(comment_body(&r, None).contains("<!-- orbit-verdict: clean blocking=0 -->"));
    }
    #[test]
    fn finds_orbits_marker_in_reviews_and_comments() {
        let marker = "<!-- orbit-review:rid -->";
        let items = json!([
            {"id": 1, "body": null, "html_url": "https://github.com/me/app/pull/7#pullrequestreview-1"},
            {"id": 2, "body": "<!-- orbit-review:other -->", "html_url": "u2"},
            {"id": 3, "body": format!("{marker}\n## Orbit"), "html_url": "u3"}
        ]);
        let items = items.as_array().unwrap();
        assert_eq!(find_marker(items, marker).unwrap(), Some(("u3".into(), 3)));
        assert_eq!(find_marker(&items[..2], marker).unwrap(), None);
        assert!(find_marker(&[json!({"id": 4, "body": marker})], marker).is_err());
        assert!(unprocessable("gh: Unprocessable Entity (HTTP 422)"));
        assert!(!unprocessable("gh: Bad Gateway (HTTP 502)"));
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
