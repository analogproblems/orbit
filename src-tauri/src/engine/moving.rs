//! Moving an Orbit into an always-on Orbit: packing this one once it has
//! stopped, and an always-on Orbit taking one in (see `crate::moving`).
use super::{
    ledger::{blocking, Reading},
    settings::validate_checkout,
    *,
};
use crate::moving::{self, Moved};

/// How long moving, and taking an Orbit in, wait for what's running in
/// the background to finish.
const QUIET: Duration = Duration::from_secs(60);
const BUSY: &str = "This Orbit is still busy with GitHub. Try again in a minute.";
/// How many moves into it an Orbit remembers the outcome of.
const MOVES: usize = 20;
/// How long after a move the moved Orbit's posts still count as the
/// always-on Orbit's own, as the two computers' clocks may differ. The
/// moved Orbit has stopped by then.
const CLOCKS: chrono::Duration = chrono::Duration::minutes(5);

/// Why an Orbit can't move now, if it can't: a scan can only finish where
/// it started.
fn cannot_move(s: &Snapshot) -> Option<String> {
    s.scans.iter().any(|x| scan_active(&x.status)).then(|| {
        "A security scan is running here. Wait for it to finish, or cancel it, then move.".into()
    })
}
/// Why an Orbit can't take another in, if it can't: it has its own
/// repositories, reviews or scans, which the move would mix up.
fn cannot_take_in(s: &Snapshot) -> Option<String> {
    (!s.settings.repositories.is_empty() || !s.reviews.is_empty() || !s.scans.is_empty()).then(|| {
        "This Orbit already has repositories or reviews of its own, so it can't take in another. Use it as it is instead.".into()
    })
}

impl Engine {
    /// Why this Orbit can't move now, if it can't.
    pub async fn cannot_move(&self) -> Option<String> {
        cannot_move(&*self.state.lock().await)
    }
    /// Whether this Orbit is moving, so it takes no new commands.
    pub fn moving(&self) -> bool {
        self.moving.load(Ordering::SeqCst)
    }
    /// Takes no new command from now on (`moving`), waits for what this
    /// Orbit is doing to finish, then holds off anything new until it
    /// starts again: syncing, publishing, a repository being connected,
    /// telling agents about CI, reading CI, the history and write-ups, and
    /// commit statuses on their way. A settings change after that is
    /// refused. For an Orbit that has shut down to move, so nothing it does
    /// lands after it's packed.
    pub async fn fence(&self) -> Result<(), String> {
        self.moving.store(true, Ordering::SeqCst);
        let until = tokio::time::Instant::now() + QUIET;
        let poll = tokio::time::timeout_at(until, self.poll_lock.lock())
            .await
            .map_err(|_| BUSY.to_string())?;
        let publish = tokio::time::timeout_at(until, self.publish_lock.lock())
            .await
            .map_err(|_| BUSY.to_string())?;
        let cloning = tokio::time::timeout_at(until, self.cloning.lock())
            .await
            .map_err(|_| BUSY.to_string())?;
        let quiet = self.quiet(until).await?;
        while !self.sending_statuses.lock().unwrap().is_empty() {
            if tokio::time::Instant::now() >= until {
                return Err(BUSY.into());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.fenced.store(true, Ordering::SeqCst);
        // Held for good: the app starts again once it has moved, or not.
        std::mem::forget((poll, publish, cloning, quiet));
        Ok(())
    }
    /// Turns autopilot on or off, as moving does, even once fenced.
    pub async fn set_autopilot(&self, on: bool) -> Result<(), String> {
        let mut s = self.state.lock().await;
        let mut settings = s.settings.clone();
        settings.automatic = on;
        self.apply_settings(&mut s, settings)
    }
    /// This Orbit, packed to move: its state as it saves it, and the files
    /// that go with it. Called once it has shut down and is fenced, so
    /// nothing changes meanwhile.
    pub async fn pack(&self) -> Result<Moved, String> {
        let mut s = self.snapshot().await;
        if let Some(why) = cannot_move(&s) {
            return Err(why);
        }
        // Devices are woken by the Orbit they signed up with, and where
        // they're woken never leaves it.
        s.push_devices.clear();
        let state = saved(&s)?;
        let scans: Vec<String> = s.scans.iter().map(|x| x.id.clone()).collect();
        let root = self.root.clone();
        let files = blocking(move || moving::collect(&root, &scans)).await?;
        Ok(Moved {
            id: uuid::Uuid::new_v4().simple().to_string(),
            state,
            files,
        })
    }
    /// Takes in an Orbit that moved here, if this one has nothing of its
    /// own yet. What was running there ends as it would if that Orbit
    /// restarted. Each repository uses its folder here when this computer
    /// has the same clone at the same place, or else Orbit's own copy,
    /// downloaded now and kept up to date. This Orbit keeps its name, and
    /// what the moved one posted until now counts as its own, so it picks up
    /// where that one left off. What belongs to this computer stays as it
    /// is: sign-ins, API keys, reviewers and access link.
    pub async fn take_in(self: &Arc<Self>, moved: Moved) -> Result<(), String> {
        let mut state: Snapshot = serde_json::from_value(moved.state)
            .map_err(|e| format!("Orbit can't read the moved Orbit's state: {e}"))?;
        // A folder on another kind of computer, such as `C:\…` here, isn't
        // even a path: Orbit's own copy goes there instead.
        for repo in &mut state.settings.repositories {
            if !std::path::Path::new(&repo.path).is_absolute() {
                if let Ok(folder) = crate::setup::clone_folder(&self.root, &repo.name) {
                    repo.path = folder.to_string_lossy().into_owned();
                }
            }
        }
        let recovered = recover(&mut state)?;
        if let Some(why) = cannot_move(&state) {
            return Err(why);
        }
        let id = moved.id;
        if !moving::valid_id(&id) {
            return Err(
                "The moving Orbit is too old to move here. Update it, then try again.".into(),
            );
        }
        {
            let s = self.state.lock().await;
            if let Some(m) = s.moves.iter().find(|m| m.id == id) {
                return Err(if m.moved { TAKEN_IN } else { CALLED_OFF }.into());
            }
            if let Some(why) = cannot_take_in(&s) {
                return Err(why);
            }
            // Under the state lock, as `move_status` reads it.
            if !self.taking_in.lock().unwrap().insert(id.clone()) {
                return Err("This move is already on its way in.".into());
            }
        }
        let _taking = Taking(&self.taking_in, &id);
        // Nothing that reads into the state runs meanwhile, so nothing
        // read before the move lands after it, and one move at a time.
        let poll = self.poll_lock.lock().await;
        let quiet = self.quiet(tokio::time::Instant::now() + QUIET).await?;
        let cloning = self.cloning.lock().await;
        // Again, as another move may have come in first, and nothing of
        // this one is written before.
        if let Some(why) = cannot_take_in(&*self.state.lock().await) {
            return Err(why);
        }
        for repo in &mut state.settings.repositories {
            if let Ok(path) = validate_checkout(&repo.name, &repo.path).await {
                repo.path = path;
                continue;
            }
            let folder = self.own_copy(&repo.name).await?;
            repo.path = validate_checkout(&repo.name, &folder.to_string_lossy())
                .await
                .map_err(|e| format!("Orbit's copy of {} can't be used: {e}", repo.name))?;
            repo.keep_updated = true;
        }
        let scans: Vec<String> = state.scans.iter().map(|x| x.id.clone()).collect();
        let (root, files) = (self.root.clone(), moved.files);
        blocking(move || moving::put(&root, &files, &scans)).await?;
        {
            let mut s = self.state.lock().await;
            if let Some(why) = cannot_take_in(&s) {
                return Err(why);
            }
            let until = chrono::Utc::now() + CLOCKS;
            self.save(
                &mut s,
                |next| {
                    keep_here(&mut state, next, until);
                    remember(&mut state.moves, &id, true);
                    *next = state;
                    Ok(())
                },
                |e| format!("Orbit couldn't save the moved Orbit: {e}"),
            )?;
            (self.emit)(&s);
        }
        drop((cloning, quiet, poll));
        self.recovered_publications
            .lock()
            .unwrap()
            .extend(recovered);
        self.publish_recovered();
        // The moved settings' model, among the models Codex has here.
        let models = {
            let s = self.state.lock().await;
            s.model_catalog_ready.then(|| s.models.clone())
        };
        if let Some(models) = models {
            let _ = self.apply_model_discovery(models, Ok(Value::Null)).await;
        }
        Ok(())
    }
    /// How the move `id` went, for certain. A move this Orbit isn't taking
    /// in and didn't take in is called off for good here, so if it's still
    /// on its way it's refused when it arrives.
    pub async fn move_status(&self, id: &str) -> Result<moving::Status, String> {
        let mut s = self.state.lock().await;
        if let Some(m) = s.moves.iter().find(|m| m.id == id) {
            return Ok(if m.moved {
                moving::Status::Moved
            } else {
                moving::Status::CalledOff
            });
        }
        if self.taking_in.lock().unwrap().contains(id) {
            return Ok(moving::Status::Running);
        }
        if !moving::valid_id(id) {
            return Err("That isn't a move's id.".into());
        }
        self.save(
            &mut s,
            |next| {
                remember(&mut next.moves, id, false);
                Ok(())
            },
            |e| format!("Orbit couldn't note how the move went: {e}"),
        )?;
        Ok(moving::Status::CalledOff)
    }
    /// Reads, in the background, what an Orbit just taken in needs from
    /// here: who answers Bit, then GitHub and CI.
    pub fn settle_in(self: &Arc<Self>) {
        let e = self.clone();
        tokio::spawn(async move {
            e.check_bit_answerer().await;
            e.pick_bit_answerer().await;
            let _ = e.poll().await;
            e.watch_ci().await;
            e.update_clones().await;
        });
    }
    /// Waits for the background reads and writes running to finish, until
    /// `until` at most, and holds off new ones until the guards drop.
    async fn quiet(&self, until: tokio::time::Instant) -> Result<Vec<Reading<'_>>, String> {
        let mut held = vec![];
        for flag in [
            &self.refreshing_ledger,
            &self.writing_up,
            &self.watching_ci,
            &self.telling,
            &self.reading_runners,
            &self.reading_storage,
            &self.reading_security,
            &self.updating_clones,
        ] {
            while flag.swap(true, Ordering::SeqCst) {
                if tokio::time::Instant::now() >= until {
                    return Err(BUSY.into());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            held.push(Reading(flag));
        }
        Ok(held)
    }
}

/// Why a move is refused: it was called off when its Orbit asked how it
/// went, as it hadn't arrived yet.
const CALLED_OFF: &str = "This move was called off. Try moving again.";
const TAKEN_IN: &str = "This Orbit already took this move in.";
/// A move being taken in, until the guard drops.
struct Taking<'a>(&'a std::sync::Mutex<HashSet<String>>, &'a str);
impl Drop for Taking<'_> {
    fn drop(&mut self) {
        self.0.lock().unwrap().remove(self.1);
    }
}
/// Notes how the move `id` went in `moves`, keeping the newest `MOVES`.
fn remember(moves: &mut Vec<MoveRecord>, id: &str, moved: bool) {
    moves.retain(|m| m.id != id);
    moves.push(MoveRecord {
        id: id.into(),
        moved,
    });
    let extra = moves.len().saturating_sub(MOVES);
    moves.drain(..extra);
}

/// Keeps what belongs to this computer in the moved state `moved`, from
/// this Orbit's own state `here`: who it is, with the moved Orbit's names
/// as its own until `until`, its sign-ins and reviewers as last checked,
/// and nothing of what the moved Orbit's computer held, such as worktrees.
pub(super) fn keep_here(
    moved: &mut Snapshot,
    here: &Snapshot,
    until: chrono::DateTime<chrono::Utc>,
) {
    let mut instance = here.instance.clone();
    instance.take_over(std::mem::take(&mut moved.instance), until);
    moved.instance = instance;
    moved.posting = here.posting.clone();
    moved.github_user = here.github_user.clone();
    moved.github_error = here.github_error.clone();
    moved.codex_error = here.codex_error.clone();
    moved.last_poll = here.last_poll.clone();
    moved.polling = here.polling;
    moved.poll_failures = here.poll_failures;
    moved.state_write_error = here.state_write_error.clone();
    moved.codex_ready = here.codex_ready;
    moved.models = here.models.clone();
    moved.model_catalog_ready = here.model_catalog_ready;
    moved.model_configuration_error = here.model_configuration_error.clone();
    moved.claude = here.claude.clone();
    moved.claude_models = here.claude_models.clone();
    moved.antigravity = here.antigravity.clone();
    moved.antigravity_models = here.antigravity_models.clone();
    moved.dev_containers = here.dev_containers.clone();
    moved.ollama = here.ollama.clone();
    moved.ollama_models = here.ollama_models.clone();
    moved.api_keys = here.api_keys.clone();
    moved.usage = here.usage.clone();
    moved.moves = here.moves.clone();
    // Devices are woken with this Orbit's own key, from where they were
    // added, and told of nothing the moved Orbit sent. The count of what's
    // sent only goes up, so a wake-up still on its way, counted before the
    // move, can never cover a notification sent after it.
    moved.notices_sent = moved.notices_sent.max(here.notices_sent);
    moved.push_devices = here.push_devices.clone();
    for d in &mut moved.push_devices {
        d.read = moved.notices_sent;
        d.told = moved.notices_sent;
    }
    // Read again here.
    moved.clone_updates.clear();
    moved.sync_failures.clear();
    moved.queue_failures.clear();
    moved.ledger.version = here.ledger.version.max(moved.ledger.version) + 1;
    for r in &mut moved.reviews {
        r.worktree = None;
    }
    for x in &mut moved.scans {
        x.worktree = None;
        x.scan_dir = None;
        x.disk_bytes = None;
    }
}
