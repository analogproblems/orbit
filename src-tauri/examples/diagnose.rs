//! Read-only connection check. Never starts a review or publishes a comment.
use orbit_lib::engine::Engine;
#[tokio::main]
async fn main() {
    let root = std::env::temp_dir().join(format!("orbit-diagnose-{}", std::process::id()));
    let engine = Engine::new(root, |_| {}).expect("Initialize temporary state");
    let codex = engine.connect().await;
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
    println!(
        "GitHub: {}",
        github
            .as_ref()
            .map(|_| "connected".to_string())
            .unwrap_or_else(|e| e.clone())
    );
    println!("Open pull requests in scope: {}", snapshot.prs.len());
    engine.shutdown().await;
    if codex.is_err() || github.is_err() {
        std::process::exit(1);
    }
}
