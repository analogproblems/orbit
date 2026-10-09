//! Read-only connection check. Never starts a review or publishes a comment.
use orbit_lib::engine::Engine;
#[tokio::main]
async fn main() {
    let root = std::env::temp_dir().join(format!("orbit-diagnose-{}", std::process::id()));
    let engine = Engine::new(root.clone(), |_| {}).expect("Initialize temporary state");
    let codex = engine.connect().await;
    let claude = engine.check_claude().await;
    let github = engine.poll().await;
    let snapshot = engine.snapshot().await;
    println!(
        "Codex: {}",
        codex
            .as_ref()
            .map(|_| "connected".to_string())
            .unwrap_or_else(|e| e.clone())
    );
    println!("Available models: {}", snapshot.models.len());
    // Claude reviews are optional, so a missing Claude is reported but not a failure.
    println!(
        "Claude: {}",
        match &claude {
            Ok(()) => format!(
                "signed in (version {})",
                snapshot.claude.version.as_deref().unwrap_or("unknown")
            ),
            Err(e) => e.clone(),
        }
    );
    println!(
        "GitHub: {}",
        github
            .as_ref()
            .map(|_| "connected".to_string())
            .unwrap_or_else(|e| e.clone())
    );
    match orbit_lib::github::repositories().await {
        Ok(repos) => println!("Accessible repositories: {}", repos.len()),
        Err(e) => println!("Accessible repositories: {e}"),
    }
    engine.shutdown().await;
    drop(engine);
    let _ = std::fs::remove_dir_all(&root);
    if codex.is_err() || github.is_err() {
        std::process::exit(1);
    }
}
