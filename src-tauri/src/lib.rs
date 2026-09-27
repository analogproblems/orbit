pub mod engine;
pub mod github;
pub mod models;
pub mod review_loop;
pub mod rpc;
pub mod storage;
pub mod types;

#[cfg(feature = "desktop")]
mod desktop {
    use super::*;
    use serde_json::Value;
    use std::sync::Arc;
    use tauri::{Emitter, Manager, State};
    use tauri_plugin_dialog::DialogExt;
    type AppState = Arc<engine::Engine>;
    #[tauri::command]
    async fn get_snapshot(engine: State<'_, AppState>) -> Result<types::Snapshot, String> {
        Ok(engine.snapshot().await)
    }
    #[tauri::command]
    async fn update_settings(
        engine: State<'_, AppState>,
        patch: types::SettingsPatch,
    ) -> Result<types::Settings, String> {
        engine.update_settings(patch).await
    }
    #[tauri::command]
    async fn save_repository(
        engine: State<'_, AppState>,
        repository: types::Repository,
        original: Option<String>,
    ) -> Result<types::Settings, String> {
        engine.save_repository(repository, original).await
    }
    #[tauri::command]
    async fn set_repository_automatic(
        engine: State<'_, AppState>,
        name: String,
        automatic: bool,
    ) -> Result<types::Settings, String> {
        engine.set_repository_automatic(&name, automatic).await
    }
    #[tauri::command]
    async fn disconnect_repository(
        engine: State<'_, AppState>,
        name: String,
    ) -> Result<types::Settings, String> {
        engine.disconnect_repository(&name).await
    }
    #[tauri::command]
    async fn get_storage_usage(
        engine: State<'_, AppState>,
    ) -> Result<storage::StorageUsage, String> {
        engine.storage_usage().await
    }
    #[tauri::command]
    async fn remove_worktrees(
        engine: State<'_, AppState>,
        review_ids: Vec<String>,
    ) -> Result<Vec<storage::CleanupResult>, String> {
        Ok(engine.remove_worktrees(review_ids).await)
    }
    #[tauri::command]
    async fn export_report(
        app: tauri::AppHandle,
        engine: State<'_, AppState>,
        review_id: String,
    ) -> Result<Option<String>, String> {
        let (name, text) = engine.export_report(&review_id).await?;
        // Rust owns the dialog and generated content; no general file-write API
        // or frontend-supplied destination is exposed.
        tauri::async_runtime::spawn_blocking(move || {
            let path = storage::select_report_path(name.into(), |suggested| {
                let mut dialog = app
                    .dialog()
                    .file()
                    .set_title("Save Markdown report (.md)")
                    .set_file_name(suggested.file_name().unwrap_or_default().to_string_lossy())
                    .add_filter("Markdown report", &["md"]);
                if let Some(parent) = suggested.parent().filter(|p| !p.as_os_str().is_empty()) {
                    dialog = dialog.set_directory(parent);
                }
                dialog
                    .blocking_save_file()
                    .map(|p| p.into_path().map_err(|e| e.to_string()))
                    .transpose()
            })?;
            storage::write_report(path.as_deref(), &text)
        })
        .await
        .map_err(|e| e.to_string())?
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
    async fn stop_loop(engine: State<'_, AppState>, pr_id: String) -> Result<(), String> {
        engine.stop_loop(&pr_id).await
    }
    #[tauri::command]
    async fn continue_loop(
        engine: State<'_, AppState>,
        pr_id: String,
        rounds: u32,
    ) -> Result<(), String> {
        engine.continue_loop(&pr_id, rounds).await
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
    async fn publish_plan(
        engine: State<'_, AppState>,
        review_id: String,
    ) -> Result<Option<types::PublishPlan>, String> {
        engine.publish_plan(&review_id).await
    }
    #[tauri::command]
    async fn set_legacy_chat_model(
        engine: State<'_, AppState>,
        review_id: String,
        model: String,
        effort: String,
    ) -> Result<(), String> {
        engine
            .set_legacy_chat_model(&review_id, model, effort)
            .await
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
            .plugin(tauri_plugin_dialog::init())
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
                update_settings,
                save_repository,
                set_repository_automatic,
                disconnect_repository,
                get_storage_usage,
                remove_worktrees,
                export_report,
                poll_now,
                connect_services,
                start_review,
                cancel_review,
                stop_loop,
                continue_loop,
                publish_review,
                publish_plan,
                send_message,
                set_legacy_chat_model,
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
