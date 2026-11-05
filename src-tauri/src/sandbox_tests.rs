use super::*;
use serde_json::Value;
use std::ffi::OsString;

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
        &[Path::new("/data/clones/a/b/.git")],
        Path::new("/data"),
        false,
    );
    assert_eq!(
        s["filesystem"]["allowWrite"],
        json!(["/data/sandbox/r1/copy", "/data/sandbox/r1/tmp"])
    );
    // What it writes it reads, and the clone's Git metadata too.
    assert_eq!(
        s["filesystem"]["allowRead"],
        json!([
            "/data/sandbox/r1/copy",
            "/data/sandbox/r1/tmp",
            "/data/clones/a/b/.git"
        ])
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
    // The runtime refuses a settings file without every list.
    assert_eq!(s["filesystem"]["denyWrite"], json!([]));
    assert_eq!(s["network"]["deniedDomains"], json!([]));
    assert_eq!(s["network"]["allowedDomains"], json!([]));
    let online = settings(&[Path::new("/c")], &[], Path::new("/data"), true);
    let hosts = online["network"]["allowedDomains"].as_array().unwrap();
    assert!(hosts.contains(&json!("registry.npmjs.org")));
    assert!(!hosts.iter().any(|h| h.as_str().unwrap().contains('*')));
}

#[test]
fn passes_on_no_tokens_and_keeps_caches_in_the_temporary_folder() {
    let ours = [
        ("PATH", "/usr/bin"),
        ("HOME", "/home/me"),
        ("LANG", "en_GB.UTF-8"),
        ("LC_ALL", "en_GB.UTF-8"),
        ("GH_TOKEN", "sentinel-token"),
        ("GITHUB_TOKEN", "sentinel-token"),
        ("OPENAI_API_KEY", "sentinel-token"),
        ("ANTHROPIC_API_KEY", "sentinel-token"),
        ("AWS_SECRET_ACCESS_KEY", "sentinel-token"),
        ("SSH_AUTH_SOCK", "/run/agent"),
        ("npm_config_cache", "/home/me/.npm"),
        ("TMPDIR", "/tmp"),
    ]
    .map(|(k, v)| (OsString::from(k), OsString::from(v)));
    let temp = Path::new("/data/sandbox/r1/tmp");
    let env = environment(ours, temp);
    let get = |name: &str| {
        let all: Vec<_> = env.iter().filter(|(k, _)| k == name).collect();
        assert!(all.len() <= 1, "{name} twice");
        all.first().map(|(_, v)| v.to_string_lossy().into_owned())
    };
    assert_eq!(get("PATH").as_deref(), Some("/usr/bin"));
    assert_eq!(get("HOME").as_deref(), Some("/home/me"));
    assert_eq!(get("LC_ALL").as_deref(), Some("en_GB.UTF-8"));
    for (_, value) in &env {
        assert_ne!(value, "sentinel-token");
    }
    assert_eq!(get("SSH_AUTH_SOCK"), None);
    for name in ["TMPDIR", "TMP", "TEMP"] {
        assert_eq!(get(name).as_deref(), Some("/data/sandbox/r1/tmp"));
    }
    // Caches go in the temporary folder, wherever Orbit's own are.
    assert_eq!(
        get("npm_config_cache").as_deref(),
        Some("/data/sandbox/r1/tmp/cache/npm")
    );
    for (name, _) in CACHES {
        assert!(
            get(name).unwrap().starts_with("/data/sandbox/r1/tmp/"),
            "{name}"
        );
        assert!(!KEEP.contains(&name), "{name}");
    }
}

/// Whether the commands may get `name` from Orbit's environment.
fn allowed(name: &str) -> bool {
    KEEP.contains(&name)
        || name.starts_with("LC_")
        || ["TMPDIR", "TMP", "TEMP"].contains(&name)
        || CACHES.iter().any(|(n, _)| *n == name)
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
            git: None,
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
    // Orbit's environment has more than the commands may get, such as
    // what Cargo sets for its tests, and none of it reaches them.
    assert!(std::env::vars().any(|(name, _)| !allowed(&name)));
    for call in &calls {
        assert_eq!(call["cwd"], json!(copy.canonicalize().unwrap()));
        assert_eq!(call["tmpdir"], json!(folder.join("tmp")));
        assert_eq!(call["args"][2], json!("--"));
        let env = call["env"].as_object().unwrap();
        for name in env.keys() {
            assert!(allowed(name), "{name}");
        }
        assert_eq!(env["npm_config_cache"], json!(folder.join("tmp/cache/npm")));
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
            git: None,
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

#[cfg(unix)]
#[tokio::test]
async fn checks_at_the_same_time_keep_to_their_own_folders() {
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let srt = fake(&state);
    let (one, two) = (d.path().join("one"), d.path().join("two"));
    let (a, b) = tokio::join!(check(&srt, &one), check(&srt, &two));
    assert!(a.is_ok() && b.is_ok(), "{a:?} {b:?}");
    assert!(!one.exists() && !two.exists());
    let trials: Vec<String> = calls(&state)
        .iter()
        .filter(|c| c["args"][0] == json!("--settings"))
        .map(|c| c["cwd"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(trials.len(), 2);
    assert_ne!(trials[0], trials[1]);
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
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(data.join("state.json"), "private").unwrap();
    // A clone in Orbit's data, as Orbit's own are, with the copy a worktree
    // of it.
    let clone = data.join("clones/acme/widgets");
    std::fs::create_dir_all(&clone).unwrap();
    let git = |dir: &Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Orbit",
                "-c",
                "user.email=orbit@example.com",
            ])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout).unwrap()
    };
    git(&clone, &["init", "-q"]);
    std::fs::write(clone.join("file.txt"), "hello").unwrap();
    git(&clone, &["add", "."]);
    git(&clone, &["commit", "-q", "-m", "init"]);
    let head = git(&clone, &["rev-parse", "HEAD"]);
    git(
        &clone,
        &[
            "worktree",
            "add",
            "--detach",
            copy.to_str().unwrap(),
            head.trim(),
        ],
    );
    let dir = copy.join(git(&copy, &["rev-parse", "--git-dir"]).trim());
    let common = clone.join(".git").canonicalize().unwrap();
    let dir = dir.canonicalize().unwrap();
    let outside = d.path().join("outside.txt");
    let checks = run_checks(
        &launcher(),
        Plan {
            folder: &folder,
            copy: &copy,
            data: &data,
            git: Some(Git {
                dir: &dir,
                common: &common,
            }),
            network: false,
            setup: "",
            check: &format!(
                "echo ok > made.txt && echo tmp > \"$TMPDIR/t\" && echo wrote; \
                 echo no > {} || echo outside-refused; \
                 cat {} || echo data-refused; \
                 curl -sS -m 5 https://example.com >/dev/null 2>&1 || echo network-refused; \
                 echo \"head $(git rev-parse HEAD)\"; \
                 git status --short; \
                 touch {}/planted || echo git-refused; \
                 mkdir -p \"$npm_config_cache\" && echo cache-made; \
                 echo \"token ${{GH_TOKEN:-none}}\"",
                outside.display(),
                data.join("state.json").display(),
                common.display()
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
        &format!("head {}", head.trim()),
        "?? made.txt",
        "git-refused",
        "cache-made",
        "token none",
    ] {
        assert!(out.contains(said), "{said}: {out}");
    }
    assert!(!out.contains("private"), "{out}");
    assert!(copy.join("made.txt").exists());
    assert!(!outside.exists());
    assert!(!common.join("planted").exists());
}
