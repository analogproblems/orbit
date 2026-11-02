//! Publishing a draft release, on your tap or Bit's (`crate::drafts`).
use super::{Engine, MOVING};
use crate::{drafts, logs, types::Snapshot};
use std::sync::atomic::Ordering;

impl Engine {
    /// Publishes the draft release for `tag` that `repo` is ready to
    /// publish, on your tap, or on Bit's while `still` says it is still
    /// Bit's to do. What is ready is taken as it is now, release, commit
    /// and all (`drafts::offered`), and published only if that is still
    /// what is ready once it is this publish's turn. First Orbit looks on
    /// GitHub for any tag or publish it may or may not have made. Then it
    /// asks GitHub whether the release is still a draft for that tag, and
    /// the tag still at the commit its build passed at, and only then
    /// records the publish in the journal, saves that, and publishes it.
    /// One release write at a time, sharing `tagging` with pushing a tag,
    /// and a release is never published twice. None starts once this Orbit
    /// is moving, and a move waits for one on its way (`fence`).
    pub async fn publish_release(
        &self,
        repo: &str,
        tag: &str,
        still: &(dyn Fn(&Snapshot) -> Result<(), String> + Sync),
    ) -> Result<(), String> {
        let pinned = {
            let s = self.state.lock().await;
            still(&s)?;
            drafts::offered(&s, repo, tag)?
        };
        let _one = self.tagging.lock().await;
        if self.moving() {
            return Err(MOVING.into());
        }
        // None is being sent now, so one still marked as sending was cut
        // off by Orbit stopping, or GitHub's answer to it was unclear.
        self.look_for_cut_off_tags().await;
        self.look_for_cut_off_publishes().await;
        let wanted = {
            let s = self.state.lock().await;
            still(&s)?;
            let now = drafts::may_publish(&s, repo)?;
            if now != pinned {
                return Err(format!(
                    "What is ready to publish in {repo} changed while Orbit waited its turn, so it didn't publish {tag}."
                ));
            }
            now
        };
        let first = |e: String| {
            format!(
                "Orbit couldn't check with GitHub first, so it didn't publish {tag}: {}.",
                logs::short(&e)
            )
        };
        let release = drafts::look_up(repo, wanted.id).await.map_err(first)?;
        let release = match release {
            None => {
                return Err(format!(
                    "The draft for {tag} is gone from GitHub, so Orbit didn't publish it."
                ))
            }
            Some(r) if !r.draft => {
                self.update(|s| {
                    if let Some(read) = s
                        .releases
                        .iter_mut()
                        .find(|x| crate::types::same_repo(&x.repo, repo))
                    {
                        drafts::published(read, r);
                    }
                })
                .await;
                let _ = self.flush().await;
                return Err(format!(
                    "GitHub says {tag} is published already, so Orbit didn't publish it again."
                ));
            }
            Some(r) => r,
        };
        if release.tag != tag {
            return Err(format!(
                "The draft is for {} now, not {tag}, so Orbit didn't publish it.",
                release.tag
            ));
        }
        let at = crate::tagging::tag_sha(repo, tag).await.map_err(first)?;
        match at {
            None => {
                return Err(format!(
                    "{tag} is gone from GitHub, and publishing the draft would make it again, so Orbit didn't."
                ))
            }
            Some(at) if at != wanted.sha => {
                return Err(format!(
                    "{tag} points at {} now, not {}, where its build passed, so Orbit didn't publish it.",
                    crate::ci::short(&at),
                    crate::ci::short(&wanted.sha)
                ))
            }
            Some(_) => {}
        }
        if self.moving() {
            return Err(MOVING.into());
        }
        let id = {
            let mut s = self.state.lock().await;
            // Saved before it is sent, so after a restart Orbit knows it
            // may have gone, and doesn't send it again. What Orbit read may
            // have changed while it asked GitHub.
            let id = self.save(
                &mut s,
                |s| {
                    still(s)?;
                    if drafts::may_publish(s, repo).ok().as_ref() != Some(&wanted) {
                        return Err(format!("It changed while Orbit checked it with GitHub, so Orbit didn't publish {tag}."));
                    }
                    Ok(drafts::record(&mut s.journal, &wanted, chrono::Utc::now()))
                },
                |e| format!("Orbit couldn't save its journal, so it didn't publish {tag}: {e}"),
            );
            (self.emit)(&s);
            id?
        };
        let sent = drafts::publish(repo, wanted.id).await;
        self.update(|s| {
            drafts::settle(&mut s.journal, &id, sent.clone(), chrono::Utc::now());
            if let Ok(release) = &sent {
                if let Some(read) = s
                    .releases
                    .iter_mut()
                    .find(|x| crate::types::same_repo(&x.repo, repo))
                {
                    drafts::published(read, release.clone());
                }
            }
        })
        .await;
        let _ = self.flush().await;
        sent.map(|_| ()).map_err(|e| drafts::publish_error(&e, tag))
    }
    /// Looks on GitHub at each draft whose publish GitHub may or may not
    /// have taken (`drafts::unsettled`), and records whether it was
    /// published. None is sent again. Only while holding `tagging`, so none
    /// is being sent.
    pub(super) async fn look_for_cut_off_publishes(&self) {
        let cut = {
            let s = self.state.lock().await;
            drafts::unsettled(&s.journal)
        };
        if cut.is_empty() {
            return;
        }
        for (id, repo, release) in &cut {
            let found = drafts::look_up(repo, *release).await;
            self.update(|s| {
                drafts::found(
                    &mut s.journal,
                    &mut s.releases,
                    id,
                    found,
                    chrono::Utc::now(),
                );
            })
            .await;
        }
        self.dirty.store(true, Ordering::SeqCst);
        let _ = self.flush().await;
    }
}
