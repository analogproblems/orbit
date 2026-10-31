//! Reading, before posting on a pull request, whether another Orbit posts
//! there (see `crate::orbits`).
use super::Engine;
use crate::{github, orbits::Posting, types::Snapshot};
use serde_json::Value;

/// Why Orbit couldn't say whether it may post on a pull request.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Unsure {
    /// What GitHub said.
    pub error: String,
    /// Whether what Orbit did read shows another Orbit posted after it.
    pub behind: bool,
}
impl From<Unsure> for String {
    fn from(unsure: Unsure) -> String {
        unsure.error
    }
}

impl Engine {
    /// Reads what was posted on pull request `number` of `repo` since
    /// Orbit's own last post there, comments and then reviews, and returns
    /// whether Orbit may post: not when another Orbit posted later. Both
    /// are read before anything is decided or kept, as a newer post in
    /// either can outweigh the other. When the reviews can't be read,
    /// what the comments show still says whether Orbit is `behind`, but
    /// isn't kept.
    pub(super) async fn clear_to_post(
        &self,
        repo: &str,
        number: u64,
        pr_id: &str,
    ) -> Result<bool, Unsure> {
        let since = {
            let s = self.state.lock().await;
            s.posting
                .iter()
                .find(|p| p.pr_id == pr_id)
                .map(|p| p.since().to_string())
                .unwrap_or_default()
        };
        let mut posts = github::comments_since(repo, number, &since)
            .await
            .map_err(|error| Unsure {
                error,
                behind: false,
            })?;
        match github::reviews(repo, number).await {
            Ok(reviews) => posts.extend(reviews),
            Err(error) => {
                let behind = !self.read_posts(pr_id, &posts, false).await;
                return Err(Unsure { error, behind });
            }
        }
        Ok(self.read_posts(pr_id, &posts, true).await)
    }
    /// Reads posts from a pull request into what Orbit knows there, those
    /// from accounts it trusts (`crate::orbits::trusted`), and returns
    /// whether Orbit may still post. Only a full read is kept.
    async fn read_posts(&self, pr_id: &str, posts: &[Value], keep: bool) -> bool {
        let read = |s: &Snapshot| {
            let login = s.github_user.clone().unwrap_or_default();
            let believed: Vec<Value> = posts
                .iter()
                .filter(|p| crate::orbits::trusted(p, &login, &s.settings.trusted_authors))
                .cloned()
                .collect();
            let mut posting = s
                .posting
                .iter()
                .find(|p| p.pr_id == pr_id)
                .cloned()
                .unwrap_or_else(|| Posting {
                    pr_id: pr_id.to_string(),
                    ..Posting::default()
                });
            posting.read(&believed, &s.instance);
            posting
        };
        if !keep {
            return !read(&*self.state.lock().await).stepped_back();
        }
        let mut clear = true;
        self.update(|s| {
            let posting = read(s);
            clear = !posting.stepped_back();
            match s.posting.iter_mut().find(|p| p.pr_id == pr_id) {
                Some(p) => *p = posting,
                None => s.posting.push(posting),
            }
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
