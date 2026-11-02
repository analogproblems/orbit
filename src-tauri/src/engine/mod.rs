use crate::{rpc::Rpc, storage, types::*};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Mutex;

mod ask;
mod chat;
mod checks;
pub(crate) mod claims;
mod cli_review;
mod clones;
mod connection;
mod events;
mod extras;
mod findings;
mod fixes;
mod inline;
mod ledger;
mod moving;
mod ops;
mod orbits;
mod prune;
mod publish;
mod push;
mod queue;
mod rounds;
mod run;
mod scans;
mod settings;
mod setup;
mod sign_in;
mod status;
mod watch;
mod worktrees;
mod writeup;

pub use push::NO_DEVICE;
pub use settings::validate_checkout;
pub(crate) use setup::carry_on;

/// How often the usage meter is read in full while Codex is connected.
const USAGE_REFRESH: Duration = Duration::from_secs(5 * 60);
/// How long to wait before trying again to save state that failed to save.
const SAVE_RETRY: Duration = Duration::from_secs(5);

#[cfg(test)]
mod antigravity_tests;
#[cfg(test)]
mod ask_tests;
#[cfg(test)]
mod chat_tests;
#[cfg(test)]
mod checks_tests;
#[cfg(test)]
mod claims_tests;
#[cfg(test)]
mod cli_review_tests;
#[cfg(test)]
mod clones_tests;
#[cfg(test)]
mod connection_tests;
#[cfg(test)]
mod findings_tests;
#[cfg(test)]
mod fixes_tests;
#[cfg(test)]
mod inline_tests;
#[cfg(test)]
mod ledger_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod moving_tests;
#[cfg(test)]
mod notice_tests;
#[cfg(test)]
mod ops_tests;
#[cfg(test)]
mod prune_tests;
#[cfg(test)]
mod publish_tests;
#[cfg(test)]
mod push_tests;
#[cfg(test)]
mod queue_tests;
#[cfg(test)]
mod rounds_tests;
#[cfg(test)]
mod run_tests;
#[cfg(test)]
mod scans_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod setup_tests;
#[cfg(test)]
mod sign_in_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod watch_tests;
#[cfg(test)]
mod worktrees_tests;
#[cfg(test)]
mod writeup_tests;

/// Why an Orbit can't start: another one uses its data folder.
pub const ALREADY_RUNNING: &str = "Orbit is already running for this workspace.";
/// Why an Orbit takes no command now: it's moving to an always-on Orbit.
pub const MOVING: &str =
    "Orbit is moving to the always-on Orbit, so it takes no changes here now. Try again once it has moved.";

type Notifier = Box<dyn Fn(&crate::attention::Notice) + Send + Sync>;
/// Orbit's review engine. Its methods are split by area: `connection` (Codex),
/// `settings` (workspace settings and checkouts), `queue` (sync and scheduling),
/// `run` (preparing and running reviews, Codex events), `events` (what a
/// reviewer reports, in Orbit's own terms), `chat` (follow-ups),
/// `publish` (GitHub comments), `worktrees` (cleanup), `clones` (keeping
/// users' clones up to date), `scans` (repository security scans),
/// `fixes` (Bit's fixes and answers), `ask` (questions you type to Bit),
/// `setup` (repositories Bit sets up), `ledger` (each repository's
/// pull-request history, for the Decisions page), `writeup` (its
/// plain-language write-ups), `watch` (reading CI), `ops` (Orbit's
/// comments and issues about failing CI) and `moving` (moving into an
/// always-on Orbit).
///
/// # Locks
///
/// | Lock           | Protects                                                          |
/// | -------------- | ----------------------------------------------------------------- |
/// | `connect_lock` | One `connect` at a time.                                          |
/// | `poll_lock`    | One sync at a time; `poll` skips a round rather than wait.        |
/// | `publish_lock` | One publication at a time.                                        |
/// | `containers`   | Read while a review sets up its dev container; written while     |
/// |                | leftovers from an earlier run are removed at launch.              |
/// | `cleanup_lock` | One worktree removal at a time.                                   |
/// | `cloning`      | One download of a repository for Bit at a time.                   |
/// | `git_lock`     | Git commands against users' clones (fetch, worktree add/remove,   |
/// |                | fast-forward).                                                    |
/// | `rpc`          | The current Codex connection.                                     |
/// | `state`        | The snapshot shown in the UI and saved to disk.                   |
/// | `synced_repos` | Repositories that synced in the latest poll.                      |
/// | `items`        | In-progress Codex items, keyed by `thread:item`.                  |
/// | `preparing`    | Abort handles of running preparations. A `std` mutex: never held  |
/// |                | across an `.await`.                                               |
///
/// When holding more than one, take them in the order of the table. Current
/// nesting: `connect_lock` → `rpc`, `items` and `state` (one at a time;
/// `items` via `retire_dead_connection` → `disconnected`); `poll_lock` →
/// `state` and `synced_repos` (one at a time); `publish_lock` → `state`;
/// `containers` → `git_lock`; `cleanup_lock` → `git_lock` → `state`; `cloning` → `state`
/// (`set_up_repository`); `poll_lock` → `cloning` → `state` (`take_in`); `poll_lock` →
/// `publish_lock` → `state`, held for good once the Orbit moves (`fence`); `state` → `synced_repos`
/// (`schedule`); `state` → `taking_in` (`take_in`, `move_status`); `state` → `fixing` (`flush`); `state` → `wake_retry` (`wake_devices`, `woken`, `push_notices`, `remove_push_device`); `state` → `asking`
/// (`ask_buddy_anything`); `state` → `using` (`use_proposal`). `preparing`,
/// `recovered_publications`, `status_sender`, `sending_statuses`,
/// `boxes` and `stopping_scans` (`std` mutexes, like `preparing` and `fixing`) are
/// otherwise only taken on their own.
///
/// Known exception: `clean_worktrees_with` briefly holds a `git_lock` try-lock
/// guard while it try-locks `cleanup_lock`, the reverse of `cleanup_lock` →
/// `git_lock`. Both are non-blocking `try_lock` calls, so it cannot deadlock.
pub struct Engine {
    pub state: Mutex<Snapshot>,
    pub rpc: Mutex<Option<Arc<Rpc>>>,
    root: PathBuf,
    emit: Box<dyn Fn(&Snapshot) + Send + Sync>,
    /// Shows a desktop notification; set by the app, absent in tests.
    notifier: std::sync::OnceLock<Notifier>,
    connect_lock: Mutex<()>,
    poll_lock: Mutex<()>,
    publish_lock: Mutex<()>,
    git_lock: Mutex<()>,
    containers: tokio::sync::RwLock<()>,
    items: Mutex<HashMap<String, Value>>,
    /// State must be written to disk (and shown) on the next flush.
    dirty: AtomicBool,
    /// State only needs to be shown, e.g. streamed text not yet worth persisting.
    changed: AtomicBool,
    stopping: AtomicBool,
    /// This Orbit is moving to an always-on Orbit (`fence`): it takes no
    /// new commands. Then `fenced` once what was running has finished: it
    /// takes no settings change either.
    moving: AtomicBool,
    fenced: AtomicBool,
    /// The moves into this Orbit it's taking in now, by id.
    taking_in: std::sync::Mutex<HashSet<String>>,
    /// An `update_clones` pass is running; another one skips rather than waits.
    updating_clones: AtomicBool,
    /// CI is being read; another read skips rather than waits.
    watching_ci: AtomicBool,
    /// Orbit's comments and issues about failing CI are being brought up to
    /// date.
    telling: AtomicBool,
    /// Self-hosted runners are being read; another read skips rather than
    /// waits.
    reading_runners: AtomicBool,
    /// Actions storage is being read; another read skips rather than waits.
    reading_storage: AtomicBool,
    /// Security alerts are being read; another read skips rather than
    /// waits.
    reading_security: AtomicBool,
    /// The pull-request history is being read; another read skips rather
    /// than waits.
    refreshing_ledger: AtomicBool,
    /// When Orbit last failed to start each pull request's automatic
    /// review, so it tries again each `queue::QUEUE_RETRY` rather than on
    /// every tick.
    queue_retry: std::sync::Mutex<HashMap<String, std::time::Instant>>,
    /// When each repository's history last read every open pull request,
    /// changed or not, without failing.
    ledger_full_at: std::sync::Mutex<HashMap<String, std::time::Instant>>,
    /// Write-ups are being made; another run skips rather than waits.
    writing_up: AtomicBool,
    /// The Codex thread writing a pull request up.
    writing_thread: std::sync::Mutex<Option<writeup::Writing>>,
    /// Write-ups that failed, by repository and pull request: what they
    /// were made from and how many times.
    write_failed: std::sync::Mutex<HashMap<(String, u64), (String, u32)>>,
    /// Repositories whose open PRs synced in the latest poll; autopilot only acts on these.
    synced_repos: Mutex<HashSet<String>>,
    /// Running preparations, so Stop can end a long fetch or checkout immediately.
    preparing: std::sync::Mutex<HashMap<String, tokio::task::AbortHandle>>,
    cleanup_lock: Mutex<()>,
    /// Automatic reviews whose extraction a restart cut short; `launch`
    /// publishes their reports.
    recovered_publications: std::sync::Mutex<Vec<String>>,
    /// Sends `orbit/review` commit statuses; tests replace it.
    status_sender: std::sync::Mutex<status::StatusSender>,
    /// Rounds whose commit status is on its way to GitHub.
    sending_statuses: std::sync::Mutex<HashSet<String>>,
    /// Approvals being answered, so the developer and the time limit
    /// cannot both answer one.
    answering: std::sync::Mutex<HashSet<String>>,
    /// Scans whose stop is being confirmed with the plugin, so a second
    /// Cancel waits for the first.
    stopping_scans: std::sync::Mutex<HashSet<String>>,
    /// The ChatGPT account the usage meter belongs to, so a reconnect as
    /// another account, or with an API key, clears the old numbers.
    usage_account: std::sync::Mutex<Option<String>>,
    /// How Claude is started; tests start a fake.
    claude_launcher: std::sync::Mutex<crate::cli::Launcher>,
    /// How Antigravity is started and where its settings are; tests use a
    /// fake.
    antigravity: std::sync::Mutex<crate::antigravity::Setup>,
    /// Where API keys are kept and where their providers answer; tests use
    /// a fake store and fake providers.
    apis: std::sync::Mutex<crate::api::Setup>,
    /// Held while an API key is saved or checked.
    api_lock: Mutex<()>,
    /// Where wake-ups for devices go (`crate::push`); tests take them on
    /// their own computer.
    push: std::sync::Mutex<crate::push::Setup>,
    /// This Orbit's key for signing wake-ups, once a device needs it.
    push_keys: std::sync::Mutex<Option<Arc<crate::push::Keys>>>,
    /// Devices are being woken; another pass skips rather than waits.
    waking: AtomicBool,
    /// When each device may be woken again, and how long it waited: after a
    /// wake-up failed, longer each time; after one went, `WAKE_AGAIN`
    /// (kept as zero), unless it reads what's new first.
    wake_retry: std::sync::Mutex<HashMap<String, (std::time::Instant, Duration)>>,
    /// How Docker and the Dev Container CLI are started; tests use fakes.
    dev_tools: std::sync::Mutex<crate::devcontainer::Tools>,
    /// How actionlint and zizmor are started; tests use fakes.
    lint_tools: std::sync::Mutex<crate::lint::Tools>,
    /// Dev containers kept for reviewers' own commands, by review; removed
    /// when the review ends.
    boxes: std::sync::Mutex<HashMap<String, checks::ReviewBox>>,
    /// One dev container image is built at a time, so two reviews of a
    /// repository don't build the same image.
    building: Mutex<()>,
    /// Each review's running Claude or Antigravity turn, so Stop can end it.
    cli_turns: std::sync::Mutex<HashMap<String, Arc<crate::cli::Running>>>,
    /// Items that just opened whose fix Bit does on its own; the next tick
    /// does them.
    fixing: std::sync::Mutex<Vec<(Attention, crate::buddy::Fix)>>,
    /// The question you typed that Codex is answering for Bit.
    asking: std::sync::Mutex<Option<ask::Asking>>,
    /// Answer buttons being used, by note and button, so each runs once.
    using: std::sync::Mutex<HashSet<(u64, u32)>>,
    /// Counts the times you forgot something from Bit's memory, so an
    /// answer to a question asked before can't bring it back.
    memory_generation: std::sync::atomic::AtomicU64,
    /// Sign-ins waiting for you to enter their code, by `github` or
    /// `codex`.
    signing_in: std::sync::Mutex<HashSet<&'static str>>,
    /// The Codex sign-in Bit started, until Codex says it finished.
    codex_login: std::sync::Mutex<Option<sign_in::Waiting>>,
    cloning: Mutex<()>,
    _instance_lock: std::fs::File,
}
impl Engine {
    pub fn new(
        root: PathBuf,
        emit: impl Fn(&Snapshot) + Send + Sync + 'static,
    ) -> Result<Arc<Self>, String> {
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let instance_lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join("orbit.lock"))
            .map_err(|e| e.to_string())?;
        instance_lock
            .try_lock()
            .map_err(|_| ALREADY_RUNNING.to_string())?;
        let path = root.join("state.json");
        let mut state: Snapshot = if path.exists() {
            serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?).map_err(
                |e| format!("Cannot read saved Orbit state; it has been left untouched: {e}"),
            )?
        } else {
            Snapshot::default()
        };
        let recovered = recover(&mut state)?;
        // Saved at the first flush, before anything is signed with it.
        state
            .instance
            .settle(&crate::orbits::home(&root), chrono::Utc::now());
        Ok(Arc::new(Self {
            state: Mutex::new(state),
            rpc: Mutex::new(None),
            root,
            // Loop states and attention items are derived for display, never saved.
            emit: Box::new(move |s: &Snapshot| emit(&crate::attention::shown(s))),
            notifier: std::sync::OnceLock::new(),
            connect_lock: Mutex::new(()),
            poll_lock: Mutex::new(()),
            publish_lock: Mutex::new(()),
            git_lock: Mutex::new(()),
            containers: tokio::sync::RwLock::new(()),
            items: Mutex::new(HashMap::new()),
            dirty: AtomicBool::new(true),
            changed: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            moving: AtomicBool::new(false),
            fenced: AtomicBool::new(false),
            taking_in: std::sync::Mutex::new(HashSet::new()),
            updating_clones: AtomicBool::new(false),
            watching_ci: AtomicBool::new(false),
            telling: AtomicBool::new(false),
            reading_runners: AtomicBool::new(false),
            reading_storage: AtomicBool::new(false),
            reading_security: AtomicBool::new(false),
            refreshing_ledger: AtomicBool::new(false),
            ledger_full_at: std::sync::Mutex::new(HashMap::new()),
            queue_retry: std::sync::Mutex::new(HashMap::new()),
            writing_up: AtomicBool::new(false),
            writing_thread: std::sync::Mutex::new(None),
            write_failed: std::sync::Mutex::new(HashMap::new()),
            synced_repos: Mutex::new(HashSet::new()),
            preparing: std::sync::Mutex::new(HashMap::new()),
            cleanup_lock: Mutex::new(()),
            recovered_publications: std::sync::Mutex::new(recovered),
            status_sender: std::sync::Mutex::new(status::github_sender()),
            sending_statuses: std::sync::Mutex::new(HashSet::new()),
            answering: std::sync::Mutex::new(HashSet::new()),
            stopping_scans: std::sync::Mutex::new(HashSet::new()),
            usage_account: std::sync::Mutex::new(None),
            claude_launcher: std::sync::Mutex::new(crate::claude::launcher()),
            apis: std::sync::Mutex::new(crate::api::Setup::default()),
            api_lock: Mutex::new(()),
            push: std::sync::Mutex::new(Default::default()),
            push_keys: std::sync::Mutex::new(None),
            waking: AtomicBool::new(false),
            wake_retry: std::sync::Mutex::new(HashMap::new()),
            antigravity: std::sync::Mutex::new(crate::antigravity::Setup::default()),
            dev_tools: std::sync::Mutex::new(Default::default()),
            lint_tools: std::sync::Mutex::new(Default::default()),
            boxes: Default::default(),
            building: Mutex::new(()),
            cli_turns: std::sync::Mutex::new(HashMap::new()),
            fixing: std::sync::Mutex::new(vec![]),
            asking: std::sync::Mutex::new(None),
            using: std::sync::Mutex::new(HashSet::new()),
            memory_generation: std::sync::atomic::AtomicU64::new(0),
            signing_in: std::sync::Mutex::new(HashSet::new()),
            codex_login: std::sync::Mutex::new(None),
            cloning: Mutex::new(()),
            _instance_lock: instance_lock,
        }))
    }
    pub async fn snapshot(&self) -> Snapshot {
        self.state.lock().await.clone()
    }
    /// The snapshot as emitted to the app.
    pub async fn shown_snapshot(&self) -> Snapshot {
        crate::attention::shown(&*self.state.lock().await)
    }
    pub async fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        let mut s = self.state.lock().await;
        f(&mut s);
        self.dirty.store(true, Ordering::SeqCst);
    }
    /// Like `update`, for changes the UI should show but that need not survive
    /// a restart on their own, such as streamed text before its item completes.
    async fn update_live(&self, f: impl FnOnce(&mut Snapshot)) {
        let mut s = self.state.lock().await;
        f(&mut s);
        self.changed.store(true, Ordering::SeqCst);
    }
    /// Sets how desktop notifications are shown. Only the first call counts.
    pub fn on_notice(&self, notify: impl Fn(&crate::attention::Notice) + Send + Sync + 'static) {
        let _ = self.notifier.set(Box::new(notify));
    }
    pub async fn flush(&self) -> Result<(), String> {
        let mut s = self.state.lock().await;
        // The record of announced items is saved with the state, so an item
        // still open after a restart is not announced again.
        let (mut notices, recorded) = crate::attention::announce(&mut s);
        // Bit's feed is saved the same way, so a note is written once.
        let fresh: Vec<Attention> = notices.iter().map(|n| n.item.clone()).collect();
        let noted = crate::buddy::observe(&mut s, &fresh);
        // An item Bit fixes on its own is noted instead of notified; if the
        // fix fails, it is notified then.
        let mut fixing = vec![];
        notices.retain(|n| match crate::buddy::will_fix(&s, &n.item) {
            Some(fix) => {
                fixing.push((n.item.clone(), fix));
                false
            }
            None => true,
        });
        self.fixing.lock().unwrap().extend(fixing);
        let enabled = s.settings.notify;
        // Kept with the state, so an app showing this Orbit from another
        // computer shows them too.
        if enabled {
            notices
                .iter()
                .for_each(|n| crate::attention::record(&mut s, n));
        }
        if recorded || noted || (enabled && !notices.is_empty()) {
            self.dirty.store(true, Ordering::SeqCst);
        }
        let mut result = Ok(());
        if self.dirty.swap(false, Ordering::SeqCst) {
            self.changed.store(false, Ordering::SeqCst);
            if let Err(e) = self.persist(&s) {
                self.dirty.store(true, Ordering::SeqCst);
                s.settings.automatic = false;
                s.state_write_error = Some(e.clone());
                s.codex_error = Some(format!(
                    "Could not save local state; automation paused: {e}"
                ));
                result = Err(e);
            }
            (self.emit)(&s);
        } else if self.changed.swap(false, Ordering::SeqCst) {
            (self.emit)(&s);
        }
        drop(s);
        if enabled {
            if let Some(notify) = self.notifier.get() {
                notices.iter().for_each(notify);
            }
        }
        result
    }
    /// Makes `edit` to a copy of the state and saves the copy before it
    /// becomes the state, so a change that can't be saved never happens.
    /// `edit` may refuse with its own error; a failed save is told by
    /// `failed`. Showing the change is left to the caller.
    fn save<T>(
        &self,
        s: &mut Snapshot,
        edit: impl FnOnce(&mut Snapshot) -> Result<T, String>,
        failed: impl FnOnce(String) -> String,
    ) -> Result<T, String> {
        let mut next = s.clone();
        let out = edit(&mut next)?;
        self.persist(&next).map_err(failed)?;
        *s = next;
        Ok(out)
    }
    fn persist(&self, s: &Snapshot) -> Result<(), String> {
        let data = serde_json::to_vec(&saved(s)?).map_err(|e| e.to_string())?;
        let temp = self.root.join("state.json.tmp");
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(|e| e.to_string())?;
        file.write_all(&data)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        std::fs::rename(temp, self.root.join("state.json")).map_err(|e| e.to_string())?;
        // Make the rename itself durable.
        #[cfg(unix)]
        std::fs::File::open(&self.root)
            .and_then(|dir| dir.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Publishes automatic reviews whose report survived a restart during
    /// extraction, as a finished extraction would have.
    fn publish_recovered(self: &Arc<Self>) {
        let recovered = std::mem::take(&mut *self.recovered_publications.lock().unwrap());
        for id in recovered {
            self.autopost(&id);
        }
    }
    pub fn launch(self: &Arc<Self>) {
        self.publish_recovered();
        let e = self.clone();
        tokio::spawn(async move {
            let _ = e.connect().await;
        });
        let e = self.clone();
        tokio::spawn(async move {
            e.check_reviewers().await;
            e.remove_leftover_containers().await;
        });
        let e = self.clone();
        tokio::spawn(async move {
            let _ = e.poll().await;
            e.watch_ci().await;
            e.update_clones().await;
            e.clean_worktrees().await;
        });
        let e = self.clone();
        tokio::spawn(async move {
            e.resume_cleanup().await;
        });
        let e = self.clone();
        tokio::spawn(async move {
            // A new access link made while Orbit was off forgets the
            // devices added with the old one.
            e.forget_old_push_devices().await;
        });
        let e = self.clone();
        tokio::spawn(async move {
            e.resume_scans().await;
        });
        let e = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(200));
            let mut last_poll = std::time::Instant::now();
            let mut last_usage = std::time::Instant::now();
            let mut last_ledger: Option<std::time::Instant> = None;
            let mut write_ups = WriteUps::default();
            while !e.stopping.load(Ordering::SeqCst) {
                tick.tick().await;
                if e.flush().await.is_err() {
                    // Automation is paused until state saves again, but
                    // approvals still expire, kept containers still go and
                    // GitHub checks still close. Saving is retried less often.
                    e.housekeeping().await;
                    tokio::time::sleep(SAVE_RETRY).await;
                    continue;
                }
                let (poll_seconds, codex_ready, cli_ready, unread, now_write_ups) = {
                    let s = e.state.lock().await;
                    (
                        s.settings.poll_seconds,
                        s.codex_ready,
                        s.claude.ready || s.antigravity.ready,
                        ledger::unread(&s),
                        s.settings.write_ups,
                    )
                };
                // Write-ups start once turned on or given another writer, and
                // otherwise follow each read of the history.
                if now_write_ups != write_ups {
                    write_ups = now_write_ups;
                    if write_ups.enabled {
                        e.write_up_soon();
                    }
                }
                // The history is read on its own schedule, and as soon as a
                // repository is connected.
                if (unread || last_ledger.is_none_or(|t| t.elapsed() >= ledger::REFRESH))
                    && e.refresh_ledger_soon()
                {
                    last_ledger = Some(std::time::Instant::now());
                }
                if last_poll.elapsed().as_secs() >= poll_seconds {
                    last_poll = std::time::Instant::now();
                    let p = e.clone();
                    tokio::spawn(async move {
                        let _ = p.poll().await;
                        p.watch_ci().await;
                        p.update_clones().await;
                        p.clean_worktrees().await;
                    });
                }
                // Updates arrive as Codex works; a gentle full read catches
                // windows that reset while Orbit sat idle.
                if codex_ready && last_usage.elapsed() >= USAGE_REFRESH {
                    last_usage = std::time::Instant::now();
                    tokio::spawn(e.clone().read_usage());
                }
                e.tick(codex_ready || cli_ready).await;
            }
        });
    }
    /// Work done on every tick: Bit's fixes, scheduling, which needs a
    /// reviewer, and commit statuses, which only need GitHub, so a
    /// disconnect still closes the checks it failed.
    async fn tick(self: &Arc<Self>, reviewer_ready: bool) {
        self.do_fixes();
        self.wake_devices().await;
        if reviewer_ready {
            self.schedule().await;
        }
        self.housekeeping().await;
    }
    /// Work that starts nothing new, so it runs even while state can't be saved.
    async fn housekeeping(self: &Arc<Self>) {
        self.expire_approvals().await;
        self.close_boxes().await;
        self.sync_commit_statuses().await;
        self.prune_state().await;
    }
    pub async fn storage_usage(&self) -> Result<storage::StorageUsage, String> {
        let snapshot = self.snapshot().await;
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || storage::inspect(&root, &snapshot))
            .await
            .map_err(|e| e.to_string())
    }
    /// The changed files of a pull request Orbit knows about.
    pub async fn files(&self, pr_id: &str) -> Result<Value, String> {
        let (repo, number) = {
            let s = self.state.lock().await;
            let pr = s
                .prs
                .iter()
                .chain(s.history_prs.iter())
                .find(|p| p.id == pr_id)
                .ok_or("Unknown pull request")?;
            (pr.repo.clone(), pr.number)
        };
        crate::github::files(&repo, number).await
    }
    pub async fn export_report(&self, id: &str) -> Result<(String, String), String> {
        storage::report(&self.snapshot().await, id)
    }
    pub async fn shutdown(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(rpc) = self.rpc.lock().await.as_ref() {
            rpc.shutdown().await;
        }
        self.stop_all_cli_turns().await;
        let ids: Vec<String> = self.boxes.lock().unwrap().keys().cloned().collect();
        for id in ids {
            self.close_box(&id).await;
        }
        let _ = self.flush().await;
    }
}
/// The state as `state.json` holds it.
pub(crate) fn saved(s: &Snapshot) -> Result<Value, String> {
    let mut saved = serde_json::to_value(s).map_err(|e| e.to_string())?;
    // Usage describes the signed-in account right now, and the password
    // store this computer has right now, so they are shown but never saved.
    if let Some(saved) = saved.as_object_mut() {
        saved.remove("usage");
        saved.remove("passwordStore");
    }
    Ok(saved)
}
/// Brings saved state up to date and ends what a stop cut short, as when
/// Orbit starts with it: reviews and scans that were running, approvals and
/// what was last seen of the reviewers. Returns the automatic reviews whose
/// report survived extraction, to publish.
pub(crate) fn recover(state: &mut Snapshot) -> Result<Vec<String>, String> {
    // Repository names and PR ids became case-insensitive. Older versions could
    // save `a/b` and `A/B` as separate repositories, so merge those before
    // validating: keep the first entry, automatic if either was.
    let mut repositories: Vec<Repository> = vec![];
    for repo in std::mem::take(&mut state.settings.repositories) {
        match repositories
            .iter_mut()
            .find(|r| same_repo(&r.name, &repo.name))
        {
            Some(kept) => kept.automatic |= repo.automatic,
            None => repositories.push(repo),
        }
    }
    state.settings.repositories = repositories;
    state.settings.buddy.normalize();
    crate::buddy::recover(&mut state.buddy_feed);
    for p in state.prs.iter_mut().chain(state.history_prs.iter_mut()) {
        p.id = p.id.to_ascii_lowercase();
    }
    let mut seen = HashSet::new();
    state.prs.retain(|p| seen.insert(p.id.clone()));
    state.history_prs.retain(|p| seen.insert(p.id.clone()));
    state.settings.validate()?;
    let mut recovered = vec![];
    for r in &mut state.reviews {
        r.pr_id = r.pr_id.to_ascii_lowercase();
        r.sensitive = r.credential_scan();
        if active(&r.status)
            && !r.end_follow_up(Some(
                "Orbit restarted during the follow-up, so its answer may be incomplete.".into(),
            ))
        {
            if r.end_extraction(Some("Orbit restarted during extraction.".into())) {
                if r.automatic {
                    recovered.push(r.id.clone());
                }
            } else {
                r.status = "cancelled".into();
                r.error = Some(
                    "Orbit restarted while this review was active. Start a new review to retry."
                        .into(),
                );
            }
        } else if r.status == "publishing" {
            r.status = "ready".into();
            r.error = Some(
                "Publication was interrupted. Retry publishing to reconcile the existing GitHub comment."
                    .into(),
            );
        }
    }
    for scan in &mut state.scans {
        const CLOSED: &str =
            "Orbit closed while this scan was running. Run a new scan to start over.";
        match scan.status.as_str() {
            // Nothing was started in Codex yet.
            "preparing" => {
                scan.status = "interrupted".into();
                scan.finished_at = Some(now());
                scan.error = Some(CLOSED.into());
            }
            // The plugin's work may outlive Orbit, so it is stopped once
            // Codex connects before the scan ends.
            "running" | "cancelling" => {
                scans::begin_stop(scan, Some("interrupted"), Some(CLOSED.into()))
            }
            _ => {}
        }
    }
    state.approvals.clear();
    state.polling = false;
    state.codex_ready = false;
    state.model_catalog_ready = false;
    state.model_configuration_error = None;
    state.models.clear();
    state.claude = CliStatus::default();
    state.claude_models = crate::claude::catalog();
    state.antigravity = CliStatus::default();
    state.ollama = CliStatus::default();
    for api in crate::api::Api::ALL {
        let key = state.api_keys.get_mut(api);
        key.status = CliStatus::default();
        key.models.clear();
    }
    state.dev_containers = CliStatus::default();
    state.ledger.refreshing = false;
    state.ledger.progress = None;
    state.ledger.writing = None;
    Ok(recovered)
}
impl Drop for Engine {
    fn drop(&mut self) {
        // Explicitly release the lock: a concurrently spawned child may briefly
        // inherit the descriptor between fork and exec, even with CLOEXEC.
        let _ = self._instance_lock.unlock();
    }
}
