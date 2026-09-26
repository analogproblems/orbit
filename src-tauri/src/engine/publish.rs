use super::{inline, Engine};
use crate::{github, types::*};
use std::sync::atomic::Ordering;

impl Engine {
    pub async fn publish(
        &self,
        id: &str,
        automatic: bool,
        allow_sensitive: bool,
    ) -> Result<(), String> {
        let _guard = self.publish_lock.lock().await;
        // Claimed under the state lock, so a follow-up cannot start while GitHub is
        // being checked. Anything short of a confirmed comment releases the claim.
        let r = {
            let mut s = self.state.lock().await;
            let r = s
                .reviews
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or("Unknown review")?;
            if r.status != "ready" || r.before_follow_up.is_some() || r.cleanup_pending {
                return Err(
                    "Only a completed review of the current head and base can be published.".into(),
                );
            }
            r.status = "publishing".into();
            r.clone()
        };
        self.dirty.store(true, Ordering::SeqCst);
        let result = self.publish_claimed(&r, automatic, allow_sensitive).await;
        self.update(|s| {
            if let Some(r) = s
                .reviews
                .iter_mut()
                .find(|r| r.id == id && r.status == "publishing")
            {
                r.status = "ready".into();
            }
        })
        .await;
        result
    }
    pub(super) async fn publish_claimed(
        &self,
        r: &Review,
        automatic: bool,
        allow_sensitive: bool,
    ) -> Result<(), String> {
        let id = r.id.as_str();
        let s = self.snapshot().await;
        // A PR from a disconnected repository may still be open; the fresh fetch
        // below refuses closed ones.
        let cached = s
            .prs
            .iter()
            .chain(&s.history_prs)
            .find(|p| p.id == r.pr_id)
            .ok_or("Unknown pull request. Sync first.")?;
        if !publishable(r, cached) {
            return Err(
                "Only a completed review of the current head and base can be published.".into(),
            );
        }
        let sensitive = r.credential_scan();
        if automatic {
            if !(s.settings.automatic
                && s.settings.auto_post
                && r.automatic
                && s.settings
                    .repositories
                    .iter()
                    .any(|repo| same_repo(&repo.name, &cached.repo) && repo.automatic))
            {
                return Ok(());
            }
            if let Some(reason) = automatic_hold(cached, &sensitive, &s.settings.trusted_authors) {
                return Err(reason);
            }
        } else if !sensitive.is_empty() && !allow_sensitive {
            return Err(format!(
                "The report contains text that looks like credentials ({}). Check it, then confirm to publish.",
                sensitive.join(", ")
            ));
        }
        // Reconcile before fresh-head and automation checks, so slow pagination
        // cannot leave a long gap between the final checks and the POST.
        if let Some((url, review_id)) = github::existing_publication(cached, r).await? {
            return self.record_publication(cached, id, url, review_id).await;
        }
        let fresh = github::pull(
            &cached.repo,
            cached.number,
            s.github_user.as_deref().unwrap_or(""),
        )
        .await?;
        if !publishable(r, &fresh) {
            return Err("The pull request changed since this review. Sync and review the new commits before publishing.".into());
        }
        // Recheck switches immediately before the write; disabling autopost cancels pending publication.
        if automatic {
            let s = self.snapshot().await;
            if !(s.settings.automatic
                && s.settings.auto_post
                && s.settings
                    .repositories
                    .iter()
                    .any(|repo| same_repo(&repo.name, &fresh.repo) && repo.automatic))
            {
                return Ok(());
            }
            if let Some(reason) = automatic_hold(&fresh, &sensitive, &s.settings.trusted_authors) {
                return Err(reason);
            }
        }
        // Persisted first, so a crash mid-request is recovered as an interrupted publication.
        let _ = self.flush().await;
        // A stable marker reconciles network timeouts or crashes without duplicate comments.
        let round = crate::review_loop::round_number(&self.snapshot().await, r);
        let published = if r.findings.is_some() {
            self.publish_review(&fresh, r, round).await
        } else {
            github::publish(&fresh, r, round)
                .await
                .map(|url| (url, None))
        };
        match published {
            Ok((url, review_id)) => self.record_publication(&fresh, id, url, review_id).await,
            Err(error) => {
                self.update(|s| {
                    if let Some(r) = s.reviews.iter_mut().find(|r| r.id == id) {
                        r.error = Some(format!(
                            "Comment not confirmed: {error}. Retry will check for an existing comment."
                        ));
                    }
                })
                .await;
                Err(error)
            }
        }
    }
    /// Publishes a structured run as a PR review, with each finding inline
    /// on its lines when they are in the diff. Returns its URL and ID.
    async fn publish_review(
        &self,
        pr: &PullRequest,
        r: &Review,
        round: Option<u32>,
    ) -> Result<(String, Option<u64>), String> {
        let files = github::files(&pr.repo, pr.number).await?;
        let hunks = inline::hunks(&files);
        let plan = inline::plan(r, &hunks, round, true);
        inline::check_size(&plan)?;
        match github::create_review(pr, &r.head_sha, &plan.body, &plan.comments).await {
            // GitHub refuses the whole review if it disagrees about a line, and
            // posts nothing, so posting again with every finding in the body is safe.
            Err(error) if github::unprocessable(&error) && !plan.comments.is_empty() => {
                let plan = inline::plan(r, &hunks, round, false);
                inline::check_size(&plan)?;
                github::create_review(pr, &r.head_sha, &plan.body, &[]).await
            }
            result => result,
        }
        .map(|(url, id)| (url, Some(id)))
    }
    /// Marks a review posted. For a PR review, also saves which comment holds
    /// each finding; that lookup is best effort, since the markers in the
    /// comments identify the findings anyway.
    async fn record_publication(
        &self,
        pr: &PullRequest,
        id: &str,
        url: String,
        review_id: Option<u64>,
    ) -> Result<(), String> {
        let inline_comments = match review_id {
            Some(review_id) => github::review_comments(pr, review_id)
                .await
                .map(|items| inline::inline_comments(&items))
                .unwrap_or_default(),
            None => vec![],
        };
        self.update(|s| {
            if let Some(r) = s.reviews.iter_mut().find(|r| r.id == id) {
                r.status = "posted".into();
                r.comment_url = Some(url);
                r.review_id = review_id;
                r.inline_comments = inline_comments;
                r.updated_at = now();
                r.error = None;
            }
        })
        .await;
        self.flush().await
    }
}
/// Why an automatic review's report must wait for a person, if it must.
/// The report stays ready for manual publication either way.
pub(super) fn automatic_hold(
    pr: &PullRequest,
    sensitive: &[String],
    trusted_authors: &[String],
) -> Option<String> {
    if !trusted_source(pr, trusted_authors) {
        return Some("Not published automatically: automatic publication is limited to pull requests opened from this repository by its owners, members, collaborators, or trusted accounts. Check the report, then publish it manually.".into());
    }
    if !sensitive.is_empty() {
        return Some(format!(
            "Not published automatically: the report contains text that looks like credentials ({}). Check it before publishing manually.",
            sensitive.join(", ")
        ));
    }
    None
}
