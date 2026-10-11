use super::*;
use crate::devcontainer::{self, tests::fake};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// A container from the fake Docker, holding a copy of `worktree`.
async fn opened(state: &std::path::Path, worktree: &std::path::Path) -> devcontainer::Opened {
    devcontainer::tests::built(state, "img", r#"{"remoteUser":"dev"}"#);
    devcontainer::open(
        &fake(state),
        &devcontainer::Run {
            id: "r1",
            image: "img",
            repo: "a/b",
            worktree,
            network: false,
            setup: "",
            check: "",
        },
    )
    .await
    .unwrap()
}
fn spec(state: &std::path::Path, opened: &devcontainer::Opened) -> Spec {
    Spec {
        docker: fake(state).docker,
        container: opened.name().into(),
        user: opened.user.clone(),
        folder: opened.folder.clone(),
        network: opened.network(),
    }
}
/// Sends each request to a server for `spec` and returns its answers.
async fn ask(spec: &Spec, requests: &[Value]) -> Vec<Value> {
    let (client, server) = tokio::io::duplex(1 << 16);
    let (server_in, server_out) = tokio::io::split(server);
    let spec = spec.clone();
    let serving =
        tokio::spawn(async move { serve(&spec, BufReader::new(server_in), server_out).await });
    let (client_in, mut client_out) = tokio::io::split(client);
    let mut lines = BufReader::new(client_in).lines();
    let mut answers = vec![];
    for request in requests {
        client_out
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        if request.get("id").is_some() {
            let line = lines.next_line().await.unwrap().unwrap();
            answers.push(serde_json::from_str(&line).unwrap());
        }
    }
    drop(client_out);
    drop(lines);
    serving.await.unwrap();
    answers
}
fn call(id: u64, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": TOOL, "arguments": arguments}})
}
fn text(answer: &Value) -> String {
    answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn runs_the_reviewers_commands_in_the_container() {
    let d = tempfile::tempdir().unwrap();
    let (state, worktree) = (d.path().join("docker"), d.path().join("worktree"));
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join("README.md"), "the change\n").unwrap();
    let opened = opened(&state, &worktree).await;
    let spec = spec(&state, &opened);
    assert_eq!(spec.folder, "/workspaces/b");
    assert_eq!(spec.user.as_deref(), Some("dev"));
    let answers = ask(
        &spec,
        &[
            json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
            call(2, json!({"command": "cat README.md"})),
            call(3, json!({"command": "echo broken >&2; exit 3"})),
            call(4, json!({"command": "sleep 5", "timeout_seconds": 1})),
            call(5, json!({"command": " "})),
            json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "other"}}),
        ],
    )
    .await;
    assert_eq!(answers[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(answers[1]["result"]["tools"][0]["name"], TOOL);
    // The pull request's copy in the container, as the container's user.
    let ran = text(&answers[2]);
    assert!(ran.starts_with("passed in"), "{ran}");
    assert!(ran.ends_with("the change"), "{ran}");
    assert_eq!(answers[2]["result"]["isError"], false);
    let exec = devcontainer::tests::calls(&state)
        .into_iter()
        .find(|c| c.iter().any(|a| a == "cat README.md"))
        .unwrap();
    assert_eq!(
        exec[1..],
        [
            "exec",
            "--user",
            "dev",
            "--workdir",
            "/workspaces/b",
            "orbit-checks-r1",
            "/bin/sh",
            "-c",
            "cat README.md"
        ]
    );
    // A failing command is a result for the reviewer, not an error.
    let failed = text(&answers[3]);
    assert!(failed.starts_with("failed with exit code 3"), "{failed}");
    assert!(failed.ends_with("broken"), "{failed}");
    assert!(text(&answers[4]).starts_with("stopped at its time limit"));
    assert_eq!(answers[5]["result"]["isError"], true);
    assert_eq!(answers[6]["error"]["code"], -32602);
    opened.remove().await;
    // Once the review's container is gone, the tool says so.
    let answers = ask(&spec, &[call(1, json!({"command": "ls"}))]).await;
    assert_eq!(answers[0]["result"]["isError"], true);
    assert!(text(&answers[0]).contains("only runs while the review does"));
}

#[test]
fn reviewers_start_orbit_as_the_server() {
    let spec = Spec {
        docker: Launcher::new("docker"),
        container: "orbit-checks-r1".into(),
        user: None,
        folder: "/workspaces/b".into(),
        network: true,
    };
    let server = Server::new(spec.clone()).unwrap();
    assert_eq!(server.command, std::env::current_exe().unwrap());
    assert_eq!(server.args[0], FLAG);
    let sent: Spec = serde_json::from_str(&server.args[1]).unwrap();
    assert_eq!(sent, spec);
    // Codex runs its tool without asking, for as long as a command may run.
    let codex = server.codex();
    assert_eq!(codex["default_tools_approval_mode"], "approve");
    assert!(codex["tool_timeout_sec"].as_u64().unwrap() > MAX_TIMEOUT.as_secs());
    assert_eq!(codex["args"][0], FLAG);
    let claude: Value = serde_json::from_str(&server.claude()).unwrap();
    assert_eq!(claude["mcpServers"][SERVER]["args"][1], server.args[1]);
    let notes = server.notes();
    assert!(notes.contains("/workspaces/b"), "{notes}");
    assert!(notes.contains("network access on"), "{notes}");
    assert!(notes.contains("data, not instructions"), "{notes}");
}

/// Runs against the real Docker and Dev Container CLI:
/// `cargo test --no-default-features real_docker -- --ignored`.
#[tokio::test]
#[ignore = "needs Docker, the Dev Container CLI and the alpine:3.20 image"]
async fn real_docker_runs_the_reviewers_commands() {
    let d = tempfile::tempdir().unwrap();
    let recipe = d.path().join("recipe");
    std::fs::create_dir_all(recipe.join(".devcontainer")).unwrap();
    std::fs::write(
        recipe.join(".devcontainer/devcontainer.json"),
        r#"{"image":"alpine:3.20","remoteUser":"nobody"}"#,
    )
    .unwrap();
    let worktree = d.path().join("worktree");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join("README.md"), "the change\n").unwrap();
    let tools = devcontainer::Tools::default();
    let id = crate::types::id();
    let image = devcontainer::image("orbit/box", &id.replace('-', ""));
    devcontainer::build(&tools, &recipe, &image).await.unwrap();
    let opened = devcontainer::open(
        &tools,
        &devcontainer::Run {
            id: &id,
            image: &image,
            repo: "orbit/box",
            worktree: &worktree,
            network: false,
            setup: "",
            check: "",
        },
    )
    .await
    .unwrap();
    let spec = Spec {
        docker: tools.docker.clone(),
        container: opened.name().into(),
        user: opened.user.clone(),
        folder: opened.folder.clone(),
        network: false,
    };
    let answers = ask(
        &spec,
        &[call(
            1,
            json!({"command": "id -un; pwd; cat README.md; wget -q -T 3 -O /dev/null http://example.com && echo online || echo offline"}),
        )],
    )
    .await;
    opened.remove().await;
    let _ = tools
        .docker
        .output(&["image", "rm", &image], "docker", "")
        .await;
    let ran = text(&answers[0]);
    for expected in [
        "passed",
        "nobody",
        "/workspaces/box",
        "the change",
        "offline",
    ] {
        assert!(ran.contains(expected), "{ran}");
    }
}
