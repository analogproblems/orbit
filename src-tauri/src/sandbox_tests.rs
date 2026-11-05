use super::*;
use serde_json::Value;

/// A fake `srt` keeping its state in `state` (see `sandbox_fixture.cjs`).
pub(crate) fn fake(state: &Path) -> Launcher {
    Launcher {
        program: "node".into(),
        prefix: vec![
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/sandbox_fixture.cjs").into(),
            state.to_string_lossy().into_owned(),
        ],
    }
}
/// Every call the fake received.
pub(crate) fn calls(state: &Path) -> Vec<Value> {
    std::fs::read_to_string(state.join("log"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn writes_only_to_the_copy_and_reads_no_sign_ins() {
    let s = settings(
        &[
            Path::new("/data/sandbox/r1/copy"),
            Path::new("/data/sandbox/r1/tmp"),
        ],
        Path::new("/data"),
        false,
    );
    assert_eq!(
        s["filesystem"]["allowWrite"],
        json!(["/data/sandbox/r1/copy", "/data/sandbox/r1/tmp"])
    );
    // Orbit's own data is hidden, apart from the copy the commands run in.
    let deny = s["filesystem"]["denyRead"].as_array().unwrap();
    assert!(deny.contains(&json!("/data")));
    for private in [
        "~/.ssh",
        "~/.config/gh",
        "~/.codex",
        "~/.claude",
        "~/.gemini",
    ] {
        assert!(deny.contains(&json!(private)), "{private}");
    }
    assert_eq!(s["filesystem"]["allowRead"], s["filesystem"]["allowWrite"]);
    // The runtime refuses a settings file without every list.
    assert_eq!(s["filesystem"]["denyWrite"], json!([]));
    assert_eq!(s["network"]["deniedDomains"], json!([]));
    assert_eq!(s["network"]["allowedDomains"], json!([]));
    let online = settings(&[Path::new("/c")], Path::new("/data"), true);
    let hosts = online["network"]["allowedDomains"].as_array().unwrap();
    assert!(hosts.contains(&json!("registry.npmjs.org")));
    assert!(!hosts.iter().any(|h| h.as_str().unwrap().contains('*')));
}

#[cfg(unix)]
#[tokio::test]
async fn runs_setup_then_checks_in_the_copy() {
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let folder = d.path().join("sandbox/r1");
    let copy = folder.join("copy");
    std::fs::create_dir_all(&copy).unwrap();
    let checks = run_checks(
        &fake(&state),
        Plan {
            folder: &folder,
            copy: &copy,
            data: d.path(),
            network: true,
            setup: "echo installed > deps && echo set up",
            check: "test -f deps && echo 3 passed",
        },
    )
    .await
    .unwrap();
    assert!(checks.sandbox && checks.network);
    assert!(checks.setup.as_ref().unwrap().passed());
    let check = checks.check.unwrap();
    assert!(
        check.passed() && check.output.contains("3 passed"),
        "{check:?}"
    );
    let calls = calls(&state);
    assert_eq!(calls.len(), 2);
    // Setup reaches the package registries; the checks reach nothing.
    assert!(!calls[0]["settings"]["network"]["allowedDomains"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(calls[1]["settings"]["network"]["allowedDomains"], json!([]));
    for call in &calls {
        assert_eq!(call["cwd"], json!(copy.canonicalize().unwrap()));
        assert_eq!(call["tmpdir"], json!(folder.join("tmp")));
        assert_eq!(call["args"][2], json!("--"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn skips_the_checks_after_a_failed_setup() {
    let d = tempfile::tempdir().unwrap();
    let folder = d.path().join("sandbox/r1");
    let copy = folder.join("copy");
    std::fs::create_dir_all(&copy).unwrap();
    let checks = run_checks(
        &fake(&d.path().join("state")),
        Plan {
            folder: &folder,
            copy: &copy,
            data: d.path(),
            network: false,
            setup: "echo no registry >&2; exit 3",
            check: "echo never",
        },
    )
    .await
    .unwrap();
    let setup = checks.setup.unwrap();
    assert_eq!(setup.exit_code, Some(3));
    assert!(setup.output.contains("no registry"));
    assert!(checks.check.is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn says_when_the_sandbox_cannot_start() {
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let folder = d.path().join("trial");
    assert_eq!(
        check(&fake(&state), &folder).await.unwrap(),
        "Sandbox runtime 0.0.81"
    );
    assert!(!folder.exists());
    std::fs::write(state.join("no-sandbox"), "").unwrap();
    let error = check(&fake(&state), &folder).await.unwrap_err();
    assert!(error.starts_with("The sandbox can't start on this computer: Error: Sandbox dependencies are not available"), "{error}");
    let error = check(&Launcher::new("/nonexistent/srt"), &folder)
        .await
        .unwrap_err();
    assert!(error.contains(INSTALL), "{error}");
}

/// Runs against the real runtime: `npm install -g
/// @anthropic-ai/sandbox-runtime`, then `npm run test:rust --
/// the_real_sandbox -- --ignored`.
#[cfg(unix)]
#[tokio::test]
#[ignore]
async fn the_real_sandbox_keeps_commands_in_their_copy() {
    let d = tempfile::tempdir().unwrap();
    let data = d.path().join("data");
    let folder = data.join("sandbox/r1");
    let copy = folder.join("copy");
    std::fs::create_dir_all(&copy).unwrap();
    std::fs::write(data.join("state.json"), "private").unwrap();
    let outside = d.path().join("outside.txt");
    let checks = run_checks(
        &launcher(),
        Plan {
            folder: &folder,
            copy: &copy,
            data: &data,
            network: false,
            setup: "",
            check: &format!(
                "echo ok > made.txt && echo tmp > \"$TMPDIR/t\" && echo wrote; \
                 echo no > {} || echo outside-refused; \
                 cat {} || echo data-refused; \
                 curl -sS -m 5 https://example.com >/dev/null 2>&1 || echo network-refused",
                outside.display(),
                data.join("state.json").display()
            ),
        },
    )
    .await
    .unwrap();
    let out = checks.check.unwrap().output;
    for said in [
        "wrote",
        "outside-refused",
        "data-refused",
        "network-refused",
    ] {
        assert!(out.contains(said), "{said}: {out}");
    }
    assert!(!out.contains("private"), "{out}");
    assert!(copy.join("made.txt").exists());
    assert!(!outside.exists());
}
