//! Reading, before posting on a pull request, whether another Orbit posts
//! there (see `crate::orbits`).
use super::Engine;
use crate::{github, orbits::Posting};
use serde_json::Value;

impl Engine {
    /// Reads what was posted on pull request `number` of `repo` since
    /// Orbit's own last post there, and returns whether Orbit may post: not
    /// when another Orbit posted later. An error means GitHub couldn't say,
    /// so nothing should be posted yet.
    pub(super) async fn clear_to_post(
        &self,
        repo: &str,
        number: u64,
        pr_id: &str,
    ) -> Result<bool, String> {
        self.clear_to_post_with(pr_id, |since| async move {
            let mut posts = github::comments_since(repo, number, &since).await?;
            posts.extend(github::reviews(repo, number).await?);
            Ok(posts)
        })
        .await
    }
    pub(super) async fn clear_to_post_with<F, Fut>(
        &self,
        pr_id: &str,
        read: F,
    ) -> Result<bool, String>
    where
        F: FnOnce(String) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<Value>, String>>,
    {
        let since = {
            let s = self.state.lock().await;
            s.posting
                .iter()
                .find(|p| p.pr_id == pr_id)
                .map(|p| p.mine.clone())
                .unwrap_or_default()
        };
        let posts = read(since).await?;
        let mut clear = true;
        self.update(|s| {
            let me = s.instance.clone();
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
            s.posting[i].read(&posts, &me);
            clear = !s.posting[i].stepped_back();
        })
        .await;
        Ok(clear)
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
