use super::*;
use axum::{extract::State, http::HeaderMap, http::StatusCode, routing::post, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use std::sync::{Arc, Mutex};

/// A wake-up a fake push service took: its headers and how long its body
/// was.
#[derive(Clone, Debug)]
pub(crate) struct Taken {
    pub authorization: String,
    pub ttl: String,
    pub urgency: String,
    pub topic: String,
    pub body: usize,
}
/// A push service on this computer that takes wake-ups at `/wake/<device>`
/// and answers with `status`.
#[derive(Clone)]
pub(crate) struct FakeService {
    pub taken: Arc<Mutex<Vec<Taken>>>,
    pub status: Arc<Mutex<StatusCode>>,
    pub url: String,
}
pub(crate) async fn fake_service() -> FakeService {
    async fn take(
        State(f): State<FakeService>,
        headers: HeaderMap,
        body: axum::body::Bytes,
    ) -> StatusCode {
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string()
        };
        f.taken.lock().unwrap().push(Taken {
            authorization: header("authorization"),
            ttl: header("ttl"),
            urgency: header("urgency"),
            topic: header("topic"),
            body: body.len(),
        });
        *f.status.lock().unwrap()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let service = FakeService {
        taken: Arc::default(),
        status: Arc::new(Mutex::new(StatusCode::CREATED)),
        url: format!("http://{}", listener.local_addr().unwrap()),
    };
    let app = Router::new()
        .route("/wake/{device}", post(take))
        .with_state(service.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    service
}
pub(crate) fn local() -> Setup {
    Setup {
        local: true,
        ..Default::default()
    }
}

#[test]
fn wakes_only_the_push_services_browsers_use() {
    for known in [
        "https://fcm.googleapis.com/fcm/send/abc:def",
        "https://web.push.apple.com/QGuQyavXutnMH",
        "https://updates.push.services.mozilla.com/wpush/v2/gAAAA",
        "https://wns2-par02p.notify.windows.com/w/?token=BQYAAA",
    ] {
        assert!(endpoint(known, false).is_ok(), "{known}");
    }
    for unknown in [
        // Not https, another port, or with a sign-in.
        "http://fcm.googleapis.com/fcm/send/abc",
        "https://fcm.googleapis.com:8443/fcm/send/abc",
        "https://user:pass@fcm.googleapis.com/fcm/send/abc",
        // Not theirs, however it's spelled.
        "https://fcm.googleapis.com.example.com/x",
        "https://evilfcm.googleapis.com.example/x",
        "https://notpush.apple.com/x",
        "https://example.com/wake",
        "https://127.0.0.1/wake",
        "file:///etc/passwd",
        "not a link",
    ] {
        assert_eq!(
            endpoint(unknown, false),
            Err(NOT_A_PUSH_SERVICE.into()),
            "{unknown}"
        );
    }
    let long = format!("https://fcm.googleapis.com/{}", "a".repeat(MAX_ENDPOINT));
    assert!(endpoint(&long, false).is_err());
    // Tests take wake-ups on their own computer, and only then.
    assert!(endpoint("http://127.0.0.1:4000/wake/1", true).is_ok());
    assert!(endpoint("http://127.0.0.1:4000/wake/1", false).is_err());
    assert!(endpoint("http://localhost:4000/wake/1", true).is_err());
}

#[test]
fn a_device_name_is_short_and_plain() {
    assert_eq!(name("  iPhone \n"), "iPhone");
    assert_eq!(name("Pixel\u{7}\u{1b}[31m"), "Pixel[31m");
    assert_eq!(name(""), "A browser");
    assert_eq!(name(&"x".repeat(100)).chars().count(), MAX_NAME);
}

#[test]
fn keeps_one_signing_key_only_orbit_can_read() {
    let d = tempfile::tempdir().unwrap();
    let made = Keys::load_or_make(d.path()).unwrap();
    let again = Keys::load_or_make(d.path()).unwrap();
    assert_eq!(made.public, again.public);
    // The uncompressed point browsers take as the server's key.
    let public = URL_SAFE_NO_PAD.decode(&made.public).unwrap();
    assert_eq!((public.len(), public[0]), (65, 4));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(d.path().join("push-key"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    std::fs::write(d.path().join("push-key"), b"not a key").unwrap();
    assert!(Keys::load_or_make(d.path())
        .err()
        .unwrap()
        .contains("Delete push-key"));
}

#[test]
fn signs_each_wake_up_for_its_push_service() {
    use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};
    let d = tempfile::tempdir().unwrap();
    let keys = Keys::load_or_make(d.path()).unwrap();
    let url = Url::parse("https://web.push.apple.com/QGuQyavXutnMH").unwrap();
    let header = keys.authorization(&url, 1_000_000).unwrap();
    let (token, key) = header
        .strip_prefix("vapid t=")
        .and_then(|rest| rest.split_once(", k="))
        .unwrap();
    assert_eq!(key, keys.public);
    let parts: Vec<&str> = token.split('.').collect();
    let decode = |part: &str| -> serde_json::Value {
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
    };
    assert_eq!(
        decode(parts[0]),
        serde_json::json!({"typ": "JWT", "alg": "ES256"})
    );
    assert_eq!(
        decode(parts[1]),
        serde_json::json!({
            "aud": "https://web.push.apple.com",
            "exp": 1_000_000 + 12 * 3600,
            "sub": "https://github.com/analogproblems/orbit",
        })
    );
    let signed = format!("{}.{}", parts[0], parts[1]);
    let public = URL_SAFE_NO_PAD.decode(key).unwrap();
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, public)
        .verify(
            signed.as_bytes(),
            &URL_SAFE_NO_PAD.decode(parts[2]).unwrap(),
        )
        .expect("signed with Orbit's key");
}

#[tokio::test]
async fn a_wake_up_is_empty_and_says_how_it_went() {
    let d = tempfile::tempdir().unwrap();
    let keys = Keys::load_or_make(d.path()).unwrap();
    let service = fake_service().await;
    let url = Url::parse(&format!("{}/wake/1", service.url)).unwrap();
    assert_eq!(wake(&local(), &keys, &url).await, Woken::Yes);
    let taken = service.taken.lock().unwrap()[0].clone();
    // Nothing about what's waiting: no body at all.
    assert_eq!(taken.body, 0);
    assert!(taken.authorization.starts_with("vapid t="));
    assert_eq!(
        (
            taken.ttl.as_str(),
            taken.urgency.as_str(),
            taken.topic.as_str()
        ),
        ("86400", "high", "orbit")
    );
    *service.status.lock().unwrap() = StatusCode::GONE;
    assert_eq!(wake(&local(), &keys, &url).await, Woken::Gone);
    *service.status.lock().unwrap() = StatusCode::NOT_FOUND;
    assert_eq!(wake(&local(), &keys, &url).await, Woken::Gone);
    *service.status.lock().unwrap() = StatusCode::TOO_MANY_REQUESTS;
    assert_eq!(
        wake(&local(), &keys, &url).await,
        Woken::Failed("Its push service answered 429.".into())
    );
    let nowhere = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}/wake/1", listener.local_addr().unwrap())
    };
    assert_eq!(
        wake(&local(), &keys, &Url::parse(&nowhere).unwrap()).await,
        Woken::Failed("Orbit couldn't reach its push service.".into())
    );
}
