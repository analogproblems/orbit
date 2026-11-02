//! Pushing a release tag, on your tap or Bit's (`crate::tagging`).
use super::{Engine, MOVING};
use crate::{logs, tagging, types::Snapshot};
use std::sync::atomic::Ordering;

impl Engine {
    /// Pushes the release tag `tag` that `repo` waits for, on your tap, or
    /// on Bit's while `still` says it is still Bit's to do. What waits is
    /// taken as it is now, commit and all (`tagging::offered`), and pushed
    /// only if that is still what waits once it is this push's turn. First
    /// Orbit looks on GitHub for any push it may or may not have made. Then
    /// it asks GitHub whether the version is still untagged, with or
    /// without its `v`, and the branch still at that commit, and only then
    /// records the push in the journal, saves that, and makes the tag. One
    /// tag at a time, and a tag is never made twice. None starts once this
    /// Orbit is moving, and a move waits for one on its way (`fence`).
    pub async fn push_tag(
        &self,
        repo: &str,
        tag: &str,
        still: &(dyn Fn(&Snapshot) -> Result<(), String> + Sync),
    ) -> Result<(), String> {
        let pinned = {
            let s = self.state.lock().await;
            still(&s)?;
            tagging::offered(&s, repo, tag)?
        };
        let _one = self.tagging.lock().await;
        if self.moving() {
            return Err(MOVING.into());
        }
        // None is being sent now, so one still marked as sending was cut
        // off by Orbit stopping, or GitHub's answer to it was unclear.
        self.look_for_cut_off_tags().await;
        let wanted = {
            let s = self.state.lock().await;
            still(&s)?;
            let now = tagging::may_push(&s, repo)?;
            if now != pinned {
                return Err(format!(
                    "What waits for a tag in {repo} changed while Orbit waited its turn, so it didn't push {tag}."
                ));
            }
            now
        };
        let sha = wanted.sha.clone();
        let mut checked = Ok(String::new());
        for name in tagging::spellings(&wanted) {
            match tagging::tag_sha(repo, &name).await {
                Ok(None) => {}
                Ok(Some(at)) => {
                    self.update(|s| {
                        if let Some(r) = s
                            .releases
                            .iter_mut()
                            .find(|r| crate::types::same_repo(&r.repo, repo))
                        {
                            tagging::seen(r, &name, &at);
                        }
                    })
                    .await;
                    let _ = self.flush().await;
                    return Err(format!(
                        "GitHub has {name} already, at {}, so Orbit didn't push {tag}.",
                        crate::ci::short(&at)
                    ));
                }
                Err(e) => {
                    checked = Err(e);
                    break;
                }
            }
        }
        let checked = match checked {
            Ok(_) => tagging::branch_sha(repo, &wanted.branch).await,
            Err(e) => Err(e),
        };
        let head = checked.map_err(|e| {
            format!(
                "Orbit couldn't check with GitHub first, so it didn't push {tag}: {}.",
                logs::short(&e)
            )
        })?;
        if head != sha {
            return Err(format!(
                "{} has moved on from {} since Orbit read it, so Orbit didn't push {tag}. It offers it again once CI passes at the new commit.",
                wanted.branch,
                crate::ci::short(&sha)
            ));
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
                    if tagging::may_push(s, repo).ok().as_ref() != Some(&wanted) {
                        return Err(format!("It changed while Orbit checked it with GitHub, so Orbit didn't push {tag}."));
                    }
                    Ok(tagging::record(&mut s.journal, &wanted, chrono::Utc::now()))
                },
                |e| format!("Orbit couldn't save its journal, so it didn't push {tag}: {e}"),
            );
            (self.emit)(&s);
            id?
        };
        let sent = tagging::push(repo, tag, &sha).await;
        self.update(|s| {
            tagging::settle(&mut s.journal, &id, sent.clone(), chrono::Utc::now());
            if sent.is_ok() {
                if let Some(r) = s
                    .releases
                    .iter_mut()
                    .find(|r| crate::types::same_repo(&r.repo, repo))
                {
                    tagging::seen(r, tag, &sha);
                }
            }
        })
        .await;
        let _ = self.flush().await;
        sent.map_err(|e| tagging::push_error(&e, tag))
    }
    /// Looks on GitHub for each push GitHub may or may not have taken
    /// (`tagging::unsettled`), and records whether it got there. None is
    /// sent again. Only while holding `tagging`, so none is being sent.
    pub(super) async fn look_for_cut_off_tags(&self) {
        let cut = {
            let s = self.state.lock().await;
            tagging::unsettled(&s.journal)
        };
        if cut.is_empty() {
            return;
        }
        for (id, repo, tag, _) in &cut {
            let found = tagging::tag_sha(repo, tag).await;
            self.update(|s| {
                tagging::found(
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
