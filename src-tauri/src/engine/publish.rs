use super::{inline, Engine};
use crate::{github, types::*};
use std::sync::atomic::Ordering;

/// Who is publishing a report, which decides the checks before each write.
#[derive(Clone, Copy)]
pub(super) enum By<'a> {
    /// You, from the app: a report with credentials waits for your say-so.
    You { allow_sensitive: bool },
    /// Autopilot, for an automatic review, while its switches allow it.
    Autopilot,
    /// Bit, for something that needs you: only a report automatic
    /// publication wouldn't hold, judged on the fresh PR before each write,
    /// whatever the publication switch says. The check runs then too, with
    /// the report as it was before this publication claimed it.
    Bit(&'a (dyn Fn(&Snapshot) -> Result<(), String> + Sync)),
}

impl Engine {
    pub async fn publish(
        &self,
        id: &str,
        automatic: bool,
        allow_sensitive: bool,
    ) -> Result<(), String> {
        let by = if automatic {
            By::Autopilot
        } else {
            By::You { allow_sensitive }
        };
        self.publish_by(id, by).await
    }
    pub(super) async fn publish_by(&self, id: &str, by: By<'_>) -> Result<(), String> {
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
        let result = self.publish_claimed(&r, by).await;
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
    pub(super) async fn publish_claimed(&self, r: &Review, by: By<'_>) -> Result<(), String> {
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
        match by {
            By::Autopilot => {
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
                if let Some(reason) =
                    automatic_hold(cached, &sensitive, &s.settings.trusted_authors)
                {
                    return Err(reason);
                }
            }
            By::You { allow_sensitive } => {
                if !sensitive.is_empty() && !allow_sensitive {
                    return Err(format!(
                        "The report contains text that looks like credentials ({}). Check it, then confirm to publish.",
                        sensitive.join(", ")
                    ));
                }
            }
            By::Bit(still) => {
                still(&unclaimed(s.clone(), id))?;
                if let Some(reason) = bit_hold(cached, &sensitive, &s.settings.trusted_authors) {
                    return Err(reason);
                }
            }
        }
        // Reconcile before fresh-head and automation checks, so slow pagination
        // cannot leave a long gap between the final checks and the POST.
        if let Some((url, review_id)) = github::existing_publication(cached, r).await? {
            return self.record_publication(cached, id, url, review_id).await;
        }
        // Anything slow (the diff, the placement) is prepared before the final
        // checks, which run again immediately before each write.
        let round = crate::review_loop::round_number(&self.snapshot().await, r);
        let hunks = match r.findings {
            Some(_) => Some(inline::hunks(
                &github::files(&cached.repo, cached.number).await?,
            )),
            None => None,
        };
        let plan = hunks
            .as_ref()
            .map(|hunks| inline::plan(r, hunks, round, true));
        if let Some(plan) = &plan {
            inline::check_size(plan)?;
        }
        let Some(fresh) = self.final_checks(cached, r, by, &sensitive).await? else {
            return Ok(());
        };
        // Signed, so another Orbit following this pull request steps back.
        let me = s.instance.name.as_str();
        // Persisted first, so a crash mid-request is recovered as an interrupted publication.
        let _ = self.flush().await;
        // A stable marker reconciles network timeouts or crashes without duplicate comments.
        let published = match (&plan, &hunks) {
            (Some(plan), Some(hunks)) => {
                let body = crate::orbits::signed(&plan.body, me);
                match github::create_review(&fresh, &r.head_sha, &body, &plan.comments).await {
                    // GitHub refuses the whole review if it disagrees about a line, and
                    // posts nothing, so posting again with every finding in the body is safe.
                    Err(error) if github::unprocessable(&error) && !plan.comments.is_empty() => {
                        let plan = inline::plan(r, hunks, round, false);
                        inline::check_size(&plan)?;
                        let Some(fresh) = self.final_checks(cached, r, by, &sensitive).await?
                        else {
                            return Ok(());
                        };
                        let body = crate::orbits::signed(&plan.body, me);
                        github::create_review(&fresh, &r.head_sha, &body, &[]).await
                    }
                    result => result,
                }
                .map(|(url, id)| (url, Some(id)))
            }
            _ => github::publish(&fresh, r, round, me)
                .await
                .map(|url| (url, None)),
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
    /// The checks run immediately before each write: the PR is still open at
    /// the reviewed head and base; for automatic publication, the switches
    /// still allow it; and for Bit, its check still passes and the fresh PR
    /// is one automatic publication would trust. Neither posts where another
    /// Orbit posted later (`crate::orbits`); you can. That read comes first,
    /// as it may take a while, so the rest is checked on things as they are
    /// after it. Returns the fresh PR, or None when automatic publication
    /// was switched off or stepped back, which cancels it.
    async fn final_checks(
        &self,
        cached: &PullRequest,
        r: &Review,
        by: By<'_>,
        sensitive: &[String],
    ) -> Result<Option<PullRequest>, String> {
        let mine = matches!(by, By::You { .. })
            || self
                .clear_to_post(&cached.repo, cached.number, &cached.id)
                .await?;
        if !mine {
            return match by {
                By::Bit(_) => Err(crate::orbits::STEPPED_BACK.into()),
                _ => Ok(None),
            };
        }
        let login = self.snapshot().await.github_user.unwrap_or_default();
        let fresh = github::pull(&cached.repo, cached.number, &login).await?;
        if !publishable(r, &fresh) {
            return Err("The pull request changed since this review. Sync and review the new commits before publishing.".into());
        }
        match by {
            By::Autopilot => {
                let s = self.snapshot().await;
                if !(s.settings.automatic
                    && s.settings.auto_post
                    && s.settings
                        .repositories
                        .iter()
                        .any(|repo| same_repo(&repo.name, &fresh.repo) && repo.automatic))
                {
                    return Ok(None);
                }
                if let Some(reason) = automatic_hold(&fresh, sensitive, &s.settings.trusted_authors)
                {
                    return Err(reason);
                }
            }
            By::You { .. } => {}
            By::Bit(still) => {
                let s = unclaimed(self.snapshot().await, &r.id);
                still(&s)?;
                if let Some(reason) = bit_hold(&fresh, sensitive, &s.settings.trusted_authors) {
                    return Err(reason);
                }
            }
        }
        Ok(Some(fresh))
    }
    /// Where a structured run's findings will go when it is published, from
    /// the PR's current diff. None for an unstructured run, which is posted
    /// as one comment.
    pub async fn publish_plan(&self, id: &str) -> Result<Option<PublishPlan>, String> {
        let s = self.snapshot().await;
        let r = s
            .reviews
            .iter()
            .find(|r| r.id == id)
            .ok_or("Unknown review")?;
        if r.findings.is_none() {
            return Ok(None);
        }
        let pr = s
            .prs
            .iter()
            .chain(&s.history_prs)
            .find(|p| p.id == r.pr_id)
            .ok_or("Unknown pull request. Sync first.")?;
        let hunks = inline::hunks(&github::files(&pr.repo, pr.number).await?);
        let round = crate::review_loop::round_number(&s, r);
        Ok(Some(inline::preview(r, &hunks, round)))
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
/// Orbit's state as it was before a publication claimed review `id`, so a
/// check made while publishing sees the report still waiting.
fn unclaimed(mut s: Snapshot, id: &str) -> Snapshot {
    if let Some(r) = s
        .reviews
        .iter_mut()
        .find(|r| r.id == id && r.status == "publishing")
    {
        r.status = "ready".into();
    }
    s
}
/// Why Bit must leave a report to you, if it must: for the same reasons
/// automatic publication would hold it.
fn bit_hold(pr: &PullRequest, sensitive: &[String], trusted_authors: &[String]) -> Option<String> {
    if !trusted_source(pr, trusted_authors) {
        return Some("This pull request doesn't come from a source Orbit trusts. Check the report, then publish it yourself.".into());
    }
    if !sensitive.is_empty() {
        return Some(format!(
            "The report contains text that looks like credentials ({}). Check it, then publish it yourself.",
            sensitive.join(", ")
        ));
    }
    None
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
