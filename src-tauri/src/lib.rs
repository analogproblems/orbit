pub mod engine;
pub mod github;
pub mod rpc;
pub mod types;

#[cfg(feature = "desktop")]
mod desktop {
    use super::*;
    use serde_json::Value;
    use std::sync::Arc;
    use tauri::{Emitter, Manager, State};
    type AppState = Arc<engine::Engine>;
    #[tauri::command]
    async fn get_snapshot(engine: State<'_, AppState>) -> Result<types::Snapshot, String> {
        Ok(engine.snapshot().await)
    }
    #[tauri::command]
    async fn save_settings(
        engine: State<'_, AppState>,
        settings: types::Settings,
    ) -> Result<(), String> {
        engine.save_settings(settings).await
    }
    #[tauri::command]
    async fn poll_now(engine: State<'_, AppState>) -> Result<(), String> {
        engine.poll().await
    }
    #[tauri::command]
    async fn connect_services(engine: State<'_, AppState>) -> Result<(), String> {
        engine.connect().await?;
        engine.poll().await
    }
    #[tauri::command]
    async fn start_review(engine: State<'_, AppState>, pr_id: String) -> Result<String, String> {
        engine.enqueue(&pr_id, false).await
    }
    #[tauri::command]
    async fn cancel_review(engine: State<'_, AppState>, review_id: String) -> Result<(), String> {
        engine.cancel(&review_id).await
    }
    #[tauri::command]
    async fn publish_review(
        engine: State<'_, AppState>,
        review_id: String,
        allow_sensitive: bool,
    ) -> Result<(), String> {
        engine.publish(&review_id, false, allow_sensitive).await
    }
    #[tauri::command]
    async fn send_message(
        engine: State<'_, AppState>,
        review_id: String,
        text: String,
    ) -> Result<(), String> {
        engine.message(&review_id, &text).await
    }
    #[tauri::command]
    async fn answer_approval(
        engine: State<'_, AppState>,
        id: Value,
        accept: bool,
    ) -> Result<(), String> {
        engine.decide(id, accept).await
    }
    #[tauri::command]
    async fn get_files(engine: State<'_, AppState>, pr_id: String) -> Result<Value, String> {
        let s = engine.snapshot().await;
        let pr = s
            .prs
            .iter()
            .chain(s.history_prs.iter())
            .find(|p| p.id == pr_id)
            .ok_or("Unknown pull request")?;
        github::files(&pr.repo, pr.number).await
    }
    #[tauri::command]
    async fn list_repositories() -> Result<Vec<types::RepositorySummary>, String> {
        github::repositories().await
    }
    #[tauri::command]
    async fn open_github(url: String) -> Result<(), String> {
        if !url.starts_with("https://github.com/") || url.chars().any(|c| c.is_control()) {
            return Err("Only GitHub links can be opened.".into());
        }
        let (program, args) = if cfg!(target_os = "macos") {
            ("open", vec![url])
        } else if cfg!(windows) {
            ("rundll32", vec!["url.dll,FileProtocolHandler".into(), url])
        } else {
            ("xdg-open", vec![url])
        };
        github::run(program, &args, None, None).await.map(|_| ())
    }
    pub fn run() {
        tauri::Builder::default()
            .setup(|app| {
                let handle = app.handle().clone();
                let root = app.path().app_data_dir()?;
                let engine = engine::Engine::new(root, move |snapshot: &types::Snapshot| {
                    let _ = handle.emit("orbit-state", snapshot);
                })
                .map_err(std::io::Error::other)?;
                app.manage(engine.clone());
                tauri::async_runtime::spawn(async move {
                    engine.launch();
                });
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                get_snapshot,
                save_settings,
                poll_now,
                connect_services,
                start_review,
                cancel_review,
                publish_review,
                send_message,
                answer_approval,
                get_files,
                list_repositories,
                open_github
            ])
            .build(tauri::generate_context!())
            .expect("Unable to start Orbit")
            .run(|app, event| {
                if let tauri::RunEvent::ExitRequested { .. } = event {
                    let engine = app.state::<AppState>();
                    tauri::async_runtime::block_on(engine.shutdown());
                }
            });
    }
}
#[cfg(feature = "desktop")]
pub use desktop::run;
