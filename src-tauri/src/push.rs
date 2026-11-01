//! Waking a phone or browser whose Orbit page is closed, so it can show
//! Orbit's notifications (Web Push). The wake-up is empty: the browser's
//! push service (Apple's, Google's, Mozilla's or Microsoft's) only learns
//! that something is waiting, never what. Woken, Orbit's service worker
//! (`public/sw.js`) reads what's new from this Orbit itself, over the same
//! link as the page, with a key that reads only that (`Engine::push_notices`).
//!
//! Each wake-up is signed with this Orbit's own key (VAPID, RFC 8292), made
//! the first time a device asks and kept in its data folder, so a push
//! service takes wake-ups for a device only from the Orbit it signed up
//! with.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use reqwest::Url;
use ring::{
    rand::SystemRandom,
    signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING},
};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

/// This Orbit's signing key, in its data folder.
const KEY_FILE: &str = "push-key";
/// Who sends the wake-ups, for a push service that needs to say something
/// about them.
const CONTACT: &str = "https://github.com/analogproblems/orbit";
/// How long a push service keeps a wake-up for a device that's off.
const KEEP: &str = "86400";
/// The longest a push service's address may be.
const MAX_ENDPOINT: usize = 2048;
/// How long a device's name may be, in characters.
pub const MAX_NAME: usize = 40;
/// How many devices can be woken. Adding one more forgets the oldest.
pub const MAX_DEVICES: usize = 10;
/// How long one wake-up may take.
const TIMEOUT: Duration = Duration::from_secs(15);
/// The push services browsers use, by the end of their addresses.
const SERVICES: &[&str] = &[
    // Chrome, Edge on Android, and other Chromium browsers.
    "fcm.googleapis.com",
    // Safari, on iPhone, iPad and Mac.
    "push.apple.com",
    // Firefox.
    "push.services.mozilla.com",
    // Edge on Windows.
    "notify.windows.com",
];
pub const NOT_A_PUSH_SERVICE: &str =
    "This browser's push service isn't one Orbit knows, so Orbit can't wake it.";

/// A browser that gets a wake-up when Orbit sends a notification, even
/// with its page closed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PushDevice {
    pub id: String,
    /// What it says it is, such as "iPhone".
    pub name: String,
    /// When it was added (RFC 3339).
    pub added: String,
    /// Where its push service takes wake-ups. Known only to Orbit: never
    /// shown (`attention::shown`).
    pub endpoint: String,
    /// The SHA-256 of the key it reads its notifications with.
    pub key_hash: String,
    /// The access link it was added with (`link`): a new link forgets it.
    pub link: String,
    /// The newest notification (`SentNotice::seq`) it read.
    pub read: u64,
    /// The newest one it was woken for.
    pub told: u64,
    /// Why the last wake-up failed, if it did.
    pub error: Option<String>,
}

/// What a device added gets back: its id, and the key its service worker
/// reads new notifications with. The key is shown this once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Added {
    pub id: String,
    pub key: String,
}

/// Where wake-ups go, and how. Tests take wake-ups on their own computer.
#[derive(Clone)]
pub struct Setup {
    pub client: reqwest::Client,
    /// Takes `http://127.0.0.1` addresses as push services, for tests.
    pub local: bool,
}
impl Default for Setup {
    fn default() -> Self {
        Self {
            // Only to the push service itself: never where it might send
            // Orbit on.
            client: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_default(),
            local: false,
        }
    }
}

/// This Orbit's signing key.
pub struct Keys {
    pair: EcdsaKeyPair,
    /// Its public half as browsers take it: the uncompressed point,
    /// base64url without padding.
    pub public: String,
}
impl Keys {
    /// The key kept in `root`, made and kept there if there isn't one.
    pub fn load_or_make(root: &Path) -> Result<Self, String> {
        let path = root.join(KEY_FILE);
        let rng = SystemRandom::new();
        let pkcs8 = match std::fs::read(&path) {
            Ok(pkcs8) => pkcs8,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let made = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
                    .map_err(|_| "Orbit couldn't make a key to wake devices with.".to_string())?;
                crate::remote::write_private(&path, made.as_ref())?;
                made.as_ref().to_vec()
            }
            Err(e) => {
                return Err(format!(
                    "Orbit couldn't read its key for waking devices: {e}"
                ))
            }
        };
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &pkcs8, &rng)
            .map_err(|_| "Orbit's key for waking devices can't be read. Delete push-key in Orbit's data folder to make a new one, then turn alerts on again on each device.".to_string())?;
        let public = URL_SAFE_NO_PAD.encode(pair.public_key().as_ref());
        Ok(Self { pair, public })
    }
    /// The `Authorization` header for a wake-up to `endpoint` at `now`
    /// (seconds since 1970): a signed token for that push service, good for
    /// 12 hours, and the public key.
    pub fn authorization(&self, endpoint: &Url, now: i64) -> Result<String, String> {
        let claims = serde_json::json!({
            "aud": endpoint.origin().ascii_serialization(),
            "exp": now + 12 * 3600,
            "sub": CONTACT,
        });
        let unsigned = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256"}"#),
            URL_SAFE_NO_PAD.encode(claims.to_string()),
        );
        let signature = self
            .pair
            .sign(&SystemRandom::new(), unsigned.as_bytes())
            .map_err(|_| "Orbit couldn't sign a wake-up.".to_string())?;
        Ok(format!(
            "vapid t={unsigned}.{}, k={}",
            URL_SAFE_NO_PAD.encode(signature.as_ref()),
            self.public
        ))
    }
}

/// `text` as a push service's address, if it is one browsers use.
pub fn endpoint(text: &str, local: bool) -> Result<Url, String> {
    if text.len() > MAX_ENDPOINT {
        return Err(NOT_A_PUSH_SERVICE.into());
    }
    let url = Url::parse(text).map_err(|_| NOT_A_PUSH_SERVICE.to_string())?;
    let host = url.host_str().unwrap_or_default();
    let plain = url.username().is_empty() && url.password().is_none();
    let known = url.scheme() == "https"
        && url.port().is_none()
        && SERVICES
            .iter()
            .any(|s| host == *s || host.ends_with(&format!(".{s}")));
    let test = local && url.scheme() == "http" && host == "127.0.0.1";
    if plain && (known || test) {
        Ok(url)
    } else {
        Err(NOT_A_PUSH_SERVICE.into())
    }
}

/// What a device calls itself, made plain and short.
pub fn name(text: &str) -> String {
    let plain: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect();
    let plain = plain.trim();
    if plain.is_empty() {
        "A browser".into()
    } else {
        plain.into()
    }
}

/// The SHA-256 of `text`, in hex.
pub fn hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The access link a device is added with, as `PushDevice::link` keeps it:
/// part of the hash of the token in `root`, so a new link forgets it.
pub fn link(root: &Path) -> String {
    crate::remote::saved_token(root)
        .map(|token| hash(&token)[..16].to_string())
        .unwrap_or_default()
}

/// How a wake-up went.
#[derive(Debug, PartialEq)]
pub enum Woken {
    Yes,
    /// The push service no longer knows the device: it turned alerts off,
    /// or its browser dropped them.
    Gone,
    Failed(String),
}

/// Sends an empty wake-up to `endpoint`, signed with `keys`. Pending
/// wake-ups for the same device share a topic, so a device that's off gets
/// one when it's back.
pub async fn wake(setup: &Setup, keys: &Keys, endpoint: &Url) -> Woken {
    let authorization = match keys.authorization(endpoint, chrono::Utc::now().timestamp()) {
        Ok(a) => a,
        Err(e) => return Woken::Failed(e),
    };
    let sent = setup
        .client
        .post(endpoint.clone())
        .header("Authorization", authorization)
        .header("TTL", KEEP)
        .header("Urgency", "high")
        .header("Topic", "orbit")
        .body(Vec::new())
        .send()
        .await;
    match sent {
        Ok(r) if r.status().is_success() => Woken::Yes,
        Ok(r) if matches!(r.status().as_u16(), 404 | 410) => Woken::Gone,
        Ok(r) => Woken::Failed(format!(
            "Its push service answered {}.",
            r.status().as_u16()
        )),
        Err(e) if e.is_timeout() => Woken::Failed("Its push service didn't answer.".into()),
        Err(_) => Woken::Failed("Orbit couldn't reach its push service.".into()),
    }
}

#[cfg(test)]
#[path = "push_tests.rs"]
pub(crate) mod tests;
