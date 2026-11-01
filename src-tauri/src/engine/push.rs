//! Waking devices whose Orbit page is closed when Orbit sends a
//! notification, and what each reads once woken (see `crate::push`).
use super::*;
use crate::{
    attention::SentNotice,
    push::{self, Added, PushDevice, Woken},
};

/// How long Orbit waits to try a device again after its wake-up failed,
/// doubling each time, up to `WAKE_RETRY_MAX`.
const WAKE_RETRY: Duration = Duration::from_secs(30);
const WAKE_RETRY_MAX: Duration = Duration::from_secs(60 * 60);
/// How long a device woken but yet to read what's new waits before it's
/// woken again for something newer: the wake-up it has coming reads that
/// too, so a burst of notifications wakes it once.
const WAKE_AGAIN: Duration = Duration::from_secs(60);
pub const NO_DEVICE: &str =
    "This device no longer gets Orbit's alerts. Turn them on again in Settings, under Notifications.";

impl Engine {
    /// Takes wake-ups on this computer, as tests' push services do.
    #[cfg(test)]
    pub(crate) fn wake_locally(&self) {
        *self.push.lock().unwrap() = crate::push::tests::local();
    }
    /// This Orbit's key for signing wake-ups, made the first time.
    fn push_keys(&self) -> Result<Arc<push::Keys>, String> {
        let mut keys = self.push_keys.lock().unwrap();
        if let Some(keys) = &*keys {
            return Ok(keys.clone());
        }
        let made = Arc::new(push::Keys::load_or_make(&self.root)?);
        *keys = Some(made.clone());
        Ok(made)
    }
    /// The public key a browser signs up for wake-ups with.
    pub async fn push_key(&self) -> Result<String, String> {
        Ok(self.push_keys()?.public.clone())
    }
    /// Adds a browser to wake when Orbit sends a notification: where its
    /// push service takes wake-ups, and what it calls itself. One already
    /// woken there is replaced. It's told of nothing sent before.
    pub async fn add_push_device(&self, endpoint: &str, name: &str) -> Result<Added, String> {
        let local = self.push.lock().unwrap().local;
        let endpoint = push::endpoint(endpoint, local)?;
        self.push_keys()?;
        let added = Added {
            id: uuid::Uuid::new_v4().simple().to_string(),
            key: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
        };
        let device = |sent: u64| PushDevice {
            id: added.id.clone(),
            name: push::name(name),
            added: chrono::Utc::now().to_rfc3339(),
            endpoint: endpoint.to_string(),
            key_hash: push::hash(&added.key),
            link: push::link(&self.root),
            read: sent,
            told: sent,
            error: None,
        };
        let mut s = self.state.lock().await;
        self.save(
            &mut s,
            |s| {
                s.push_devices.retain(|d| d.endpoint != endpoint.as_str());
                s.push_devices.push(device(s.notices_sent));
                let extra = s.push_devices.len().saturating_sub(push::MAX_DEVICES);
                s.push_devices.drain(..extra);
                Ok(())
            },
            |e| format!("Orbit couldn't save this device: {e}"),
        )?;
        self.changed.store(true, Ordering::SeqCst);
        Ok(added)
    }
    /// Stops waking a device. One already gone is no error.
    pub async fn remove_push_device(&self, id: &str) -> Result<(), String> {
        let mut s = self.state.lock().await;
        if !s.push_devices.iter().any(|d| d.id == id) {
            return Ok(());
        }
        self.save(
            &mut s,
            |s| {
                s.push_devices.retain(|d| d.id != id);
                Ok(())
            },
            |e| format!("Orbit couldn't save that: {e}"),
        )?;
        self.changed.store(true, Ordering::SeqCst);
        self.wake_retry.lock().unwrap().remove(id);
        Ok(())
    }
    /// Forgets the devices added with an access link that no longer works,
    /// as after a new link is made.
    pub async fn forget_old_push_devices(&self) {
        let link = push::link(&self.root);
        let mut s = self.state.lock().await;
        if s.push_devices.iter().any(|d| d.link != link) {
            s.push_devices.retain(|d| d.link == link);
            self.dirty.store(true, Ordering::SeqCst);
        }
    }
    /// What's new for the device whose key is `key`, once woken: the
    /// notifications it hasn't read whose item still needs you, and none
    /// while notifications are off. None if no device has that key, or it
    /// was added with an old access link.
    pub async fn push_notices(&self, key: &str) -> Option<Vec<SentNotice>> {
        let hash = push::hash(key);
        let link = push::link(&self.root);
        let mut s = self.state.lock().await;
        let at = s
            .push_devices
            .iter()
            .position(|d| d.key_hash == hash && d.link == link)?;
        let read = s.push_devices[at].read;
        let open: HashSet<String> = crate::attention::items(&s)
            .into_iter()
            .map(|a| a.key)
            .collect();
        // Read all the same, so turning them on again shows nothing older.
        let on = s.settings.notify;
        let new = s
            .notices
            .iter()
            .filter(|n| on && n.seq > read && open.contains(&n.item.key))
            .cloned()
            .collect();
        let sent = s.notices_sent;
        let d = &mut s.push_devices[at];
        if d.read != sent {
            d.read = sent;
            d.told = d.told.max(sent);
            self.dirty.store(true, Ordering::SeqCst);
        }
        // Read, so the next notification wakes it straight away, unless its
        // wake-ups are failing.
        let id = d.id.clone();
        let mut retry = self.wake_retry.lock().unwrap();
        if retry.get(&id).is_some_and(|(_, wait)| wait.is_zero()) {
            retry.remove(&id);
        }
        Some(new)
    }
    /// Wakes each device that hasn't read a notification still open, in a
    /// task of its own, unless a pass is running. A device woken waits
    /// `WAKE_AGAIN` before it's woken again, unless it reads what's new
    /// first; one whose wake-up failed waits longer each time; one its push
    /// service no longer knows is forgotten. One with nothing still open to
    /// tell is caught up. While notifications are off, none is woken, and
    /// each is caught up, so turning them on again tells nothing older.
    pub(super) async fn wake_devices(self: &Arc<Self>) {
        if self.waking.load(Ordering::SeqCst) {
            return;
        }
        let mut s = self.state.lock().await;
        let sent = s.notices_sent;
        if !s.settings.notify {
            if s.push_devices
                .iter()
                .any(|d| d.told != sent || d.read != sent)
            {
                for d in &mut s.push_devices {
                    (d.told, d.read) = (sent, sent);
                }
                self.dirty.store(true, Ordering::SeqCst);
            }
            return;
        }
        let now = std::time::Instant::now();
        let mut retry = self.wake_retry.lock().unwrap();
        let resting: HashSet<String> = retry
            .iter()
            .filter(|(_, (at, _))| *at > now)
            .map(|(id, _)| id.clone())
            .collect();
        let waiting = |d: &PushDevice| resting.contains(&d.id);
        // Most ticks: everyone told, or waiting to be tried again.
        if s.push_devices.iter().all(|d| d.told >= sent || waiting(d)) {
            return;
        }
        // Added with a link that no longer works: forgotten, not woken.
        let link = push::link(&self.root);
        if s.push_devices.iter().any(|d| d.link != link) {
            s.push_devices.retain(|d| d.link == link);
            retry.retain(|id, _| s.push_devices.iter().any(|d| &d.id == id));
            self.dirty.store(true, Ordering::SeqCst);
        }
        let open: HashSet<String> = crate::attention::items(&s)
            .into_iter()
            .map(|a| a.key)
            .collect();
        let mut due = vec![];
        let mut caught_up = vec![];
        for d in s
            .push_devices
            .iter()
            .filter(|d| d.told < sent && !waiting(d))
        {
            let unread = s
                .notices
                .iter()
                .any(|n| n.seq > d.told.max(d.read) && open.contains(&n.item.key));
            if unread {
                due.push((d.id.clone(), d.endpoint.clone()));
            } else {
                caught_up.push(d.id.clone());
            }
        }
        for d in &mut s.push_devices {
            if caught_up.contains(&d.id) {
                d.told = sent;
                retry.remove(&d.id);
                self.dirty.store(true, Ordering::SeqCst);
            }
        }
        drop(retry);
        drop(s);
        if due.is_empty() || self.waking.swap(true, Ordering::SeqCst) {
            return;
        }
        let e = self.clone();
        tokio::spawn(async move {
            e.wake(due).await;
            e.waking.store(false, Ordering::SeqCst);
        });
    }
    async fn wake(&self, due: Vec<(String, String)>) {
        let link = push::link(&self.root);
        let keys = match self.push_keys() {
            Ok(keys) => keys,
            Err(error) => {
                for (id, _) in &due {
                    self.woken(id, &link, 0, Woken::Failed(error.clone())).await;
                }
                return;
            }
        };
        let setup = self.push.lock().unwrap().clone();
        for (id, endpoint) in due {
            let (sent, on) = {
                let s = self.state.lock().await;
                (s.notices_sent, s.settings.notify)
            };
            // Turned off meanwhile: the next pass catches them up.
            if !on {
                return;
            }
            let woken = match push::endpoint(&endpoint, setup.local) {
                Ok(url) => push::wake(&setup, &keys, &url).await,
                Err(error) => Woken::Failed(error),
            };
            self.woken(&id, &link, sent, woken).await;
        }
    }
    /// Notes how waking the device `id` for what was sent up to `sent` went.
    async fn woken(&self, id: &str, link: &str, sent: u64, woken: Woken) {
        let mut s = self.state.lock().await;
        let Some(at) = s.push_devices.iter().position(|d| d.id == id) else {
            return;
        };
        // Added with a link that no longer works: never woken again.
        if s.push_devices[at].link != link || woken == Woken::Gone {
            s.push_devices.remove(at);
            self.wake_retry.lock().unwrap().remove(id);
            self.dirty.store(true, Ordering::SeqCst);
            return;
        }
        let now_sent = s.notices_sent;
        let d = &mut s.push_devices[at];
        let mut retry = self.wake_retry.lock().unwrap();
        let now = std::time::Instant::now();
        if let Woken::Failed(error) = woken {
            d.error = Some(format!("Orbit couldn't wake it: {error}"));
            // Kept as how long it waited, which a wake-up that went keeps
            // at zero.
            let wait = retry.get(id).map_or(WAKE_RETRY, |(_, wait)| {
                (*wait * 2).clamp(WAKE_RETRY, WAKE_RETRY_MAX)
            });
            retry.insert(id.into(), (now + wait, wait));
        } else {
            // Never past what's sent now: a move in may have started the
            // count again lower while this wake-up was on its way.
            d.told = d.told.max(sent.min(now_sent));
            d.error = None;
            retry.insert(id.into(), (now + WAKE_AGAIN, Duration::ZERO));
        }
        self.dirty.store(true, Ordering::SeqCst);
    }
}
