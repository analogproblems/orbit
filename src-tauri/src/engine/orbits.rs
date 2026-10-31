//! Reading, before posting on a pull request, whether another Orbit posts
//! there (see `crate::orbits`).
use super::Engine;
use crate::{github, orbits::Posting};
use serde_json::Value;

impl Engine {
    /// Reads what was posted on pull request `number` of `repo` since
    /// Orbit's own last post there, comments first and then reviews, and
    /// returns whether Orbit may post: not when another Orbit posted later.
    /// What one read shows counts even when the next fails. An error means
    /// GitHub couldn't say, so nothing new should be posted yet.
    pub(super) async fn clear_to_post(
        &self,
        repo: &str,
        number: u64,
        pr_id: &str,
    ) -> Result<bool, String> {
        let since = {
            let s = self.state.lock().await;
            s.posting
                .iter()
                .find(|p| p.pr_id == pr_id)
                .map(|p| p.since().to_string())
                .unwrap_or_default()
        };
        let comments = github::comments_since(repo, number, &since).await?;
        if !self.take_in(pr_id, &comments).await {
            return Ok(false);
        }
        let reviews = github::reviews(repo, number).await?;
        Ok(self.take_in(pr_id, &reviews).await)
    }
    /// Takes posts read on a pull request into what Orbit knows there,
    /// those from accounts it trusts (`crate::orbits::trusted`), and
    /// returns whether Orbit may still post.
    async fn take_in(&self, pr_id: &str, posts: &[Value]) -> bool {
        let mut clear = true;
        self.update(|s| {
            let login = s.github_user.clone().unwrap_or_default();
            let believed: Vec<Value> = posts
                .iter()
                .filter(|p| crate::orbits::trusted(p, &login, &s.settings.trusted_authors))
                .cloned()
                .collect();
            let i = match s.posting.iter().position(|p| p.pr_id == pr_id) {
                Some(i) => i,
                None => {
                    s.posting.push(Posting {
                        pr_id: pr_id.to_string(),
                        ..Posting::default()
                    });
                    s.posting.len() - 1
                }
            };
            let me = s.instance.clone();
            s.posting[i].read(&believed, &me);
            clear = !s.posting[i].stepped_back();
        })
        .await;
        clear
    }
    /// Whether GitHub's copy of comment `id` in `repo` was last written by
    /// this Orbit, so editing it overwrites nothing another Orbit wrote.
    pub(super) async fn own_comment(&self, repo: &str, id: u64, marker: &str) -> bool {
        let Ok(copy) = github::get_comment(repo, id, marker).await else {
            return false;
        };
        let me = self.state.lock().await.instance.clone();
        crate::orbits::signer(&copy.body).is_some_and(|by| me.is(by, &copy.updated_at))
    }
    /// Posts on a pull request again after the other Orbit's latest post
    /// there, read now so one it made since doesn't count: this Orbit's
    /// next post makes the other one step back.
    pub(super) async fn post_here_again(&self, pr_id: &str) -> Result<(), String> {
        let (repo, number) = {
            let s = self.state.lock().await;
            s.prs
                .iter()
                .find(|p| p.id == pr_id)
                .map(|p| (p.repo.clone(), p.number))
                .ok_or("This pull request isn't in the inbox any more.")?
        };
        self.clear_to_post(&repo, number, pr_id).await?;
        self.update(|s| {
            if let Some(p) = s.posting.iter_mut().find(|p| p.pr_id == pr_id) {
                p.resume();
            }
        })
        .await;
        self.flush().await
    }
}
