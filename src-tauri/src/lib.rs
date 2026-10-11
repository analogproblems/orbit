pub mod antigravity;
pub mod ask;
pub mod attention;
pub mod buddy;
pub mod claude;
pub mod cli;
pub mod devcontainer;
pub mod engine;
pub mod extras;
pub mod github;
pub mod models;
pub mod remote;
pub mod review_instructions;
pub mod review_loop;
pub mod rpc;
pub mod scans;
pub mod storage;
pub mod types;
pub mod usage;

#[cfg(feature = "desktop")]
mod desktop {
    use super::*;
    use serde_json::Value;
    use std::sync::Arc;
    use tauri::{Emitter, Manager, State};
    use tauri_plugin_dialog::DialogExt;
    use tauri_plugin_notification::NotificationExt;
    type AppState = Arc<engine::Engine>;
    /// The item of the latest notification sent while Orbit was in the
    /// background. Desktop notifications report no clicks, so the item opens
    /// when Orbit next comes to the front, as clicking the notification does.
    #[derive(Default)]
    struct Unseen(std::sync::Mutex<Option<types::Attention>>);
    #[tauri::command]
    async fn get_snapshot(engine: State<'_, AppState>) -> Result<types::Snapshot, String> {
        Ok(engine.shown_snapshot().await)
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
    /// Opens a folder picker for a repository's checkout. The desktop app only:
    /// a browser connected over remote access cannot see the host's dialog.
    #[tauri::command]
    async fn choose_checkout(
        app: tauri::AppHandle,
        current: Option<String>,
    ) -> Result<Option<String>, String> {
        tauri::async_runtime::spawn_blocking(move || {
            let mut dialog = app
                .dialog()
                .file()
                .set_title("Choose the repository's local clone");
            if let Some(dir) = current
                .map(std::path::PathBuf::from)
                .filter(|p| p.is_absolute() && p.is_dir())
            {
                dialog = dialog.set_directory(dir);
            }
            dialog
                .blocking_pick_folder()
                .map(|p| {
                    p.into_path()
                        .map(|p| p.to_string_lossy().into_owned())
                        .map_err(|e| e.to_string())
                })
                .transpose()
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[tauri::command]
    async fn poll_now(engine: State<'_, AppState>) -> Result<(), String> {
        engine.poll().await?;
        engine.update_clones_soon();
        Ok(())
    }
    #[tauri::command]
    async fn connect_services(engine: State<'_, AppState>) -> Result<(), String> {
        engine.check_reviewers().await;
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
    async fn use_fix(engine: State<'_, AppState>, key: String) -> Result<(), String> {
        engine.fix(&key).await
    }
    #[tauri::command]
    async fn ask_buddy(
        engine: State<'_, AppState>,
        question: buddy::Question,
    ) -> Result<(), String> {
        engine.ask_buddy(question).await
    }
    #[tauri::command]
    async fn ask_buddy_anything(
        engine: State<'_, AppState>,
        question: String,
    ) -> Result<(), String> {
        engine.ask_buddy_anything(&question).await
    }
    #[tauri::command]
    async fn forget_memory(
        engine: State<'_, AppState>,
        repo: Option<String>,
        line: String,
    ) -> Result<(), String> {
        engine.forget_memory(repo.as_deref(), &line).await
    }
    #[tauri::command]
    async fn clear_memory(engine: State<'_, AppState>) -> Result<(), String> {
        engine.clear_memory().await
    }
    #[tauri::command]
    async fn use_proposal(
        engine: State<'_, AppState>,
        note_id: u64,
        proposal_id: u32,
    ) -> Result<(), String> {
        engine.use_proposal(note_id, proposal_id).await
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
    async fn get_scan_plugin(engine: State<'_, AppState>) -> Result<scans::PluginStatus, String> {
        engine.scan_plugin().await
    }
    #[tauri::command]
    async fn install_scan_plugin(
        engine: State<'_, AppState>,
    ) -> Result<scans::PluginStatus, String> {
        engine.install_scan_plugin().await
    }
    #[tauri::command]
    async fn start_scan(
        engine: State<'_, AppState>,
        repo: String,
        mode: String,
    ) -> Result<String, String> {
        engine.start_scan(&repo, &mode).await
    }
    #[tauri::command]
    async fn cancel_scan(engine: State<'_, AppState>, scan_id: String) -> Result<(), String> {
        engine.cancel_scan(&scan_id).await
    }
    #[tauri::command]
    async fn delete_scan(engine: State<'_, AppState>, scan_id: String) -> Result<(), String> {
        engine.delete_scan(&scan_id).await
    }
    #[tauri::command]
    async fn get_scan_report(
        engine: State<'_, AppState>,
        scan_id: String,
    ) -> Result<String, String> {
        engine.scan_report(&scan_id).await
    }
    #[tauri::command]
    async fn get_files(engine: State<'_, AppState>, pr_id: String) -> Result<Value, String> {
        engine.files(&pr_id).await
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
    // Browser access is controlled from the desktop window only; these three
    // are left out of `remote::COMMANDS`.
    #[tauri::command]
    async fn get_remote_access(
        access: State<'_, Arc<remote::Access>>,
    ) -> Result<remote::Status, String> {
        Ok(access.status().await)
    }
    #[tauri::command]
    async fn set_remote_access(
        access: State<'_, Arc<remote::Access>>,
        enabled: bool,
        port: u16,
    ) -> Result<remote::Status, String> {
        access.set(enabled, port).await
    }
    #[tauri::command]
    async fn reset_remote_link(
        access: State<'_, Arc<remote::Access>>,
    ) -> Result<remote::Status, String> {
        access.reset_link().await
    }
    pub fn run() {
        tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_notification::init())
            .setup(|app| {
                let handle = app.handle().clone();
                let root = app.path().app_data_dir()?;
                let hub = remote::Hub::default();
                let browsers = hub.clone();
                let engine =
                    engine::Engine::new(root.clone(), move |snapshot: &types::Snapshot| {
                        let _ = handle.emit("orbit-state", snapshot);
                        browsers.publish(snapshot);
                    })
                    .map_err(std::io::Error::other)?;
                let resolver = app.handle().clone();
                let assets: remote::Assets = Arc::new(move |path: &str| {
                    resolver
                        .asset_resolver()
                        .get(path.into())
                        .map(|a| remote::Asset {
                            bytes: a.bytes,
                            mime_type: a.mime_type,
                        })
                });
                let access = remote::Access::new(engine.clone(), hub, root, assets);
                app.manage(access.clone());
                tauri::async_runtime::spawn(async move { access.start().await });
                app.manage(Unseen::default());
                let handle = app.handle().clone();
                engine.on_notice(move |notice| {
                    let _ = handle
                        .notification()
                        .builder()
                        .title(&notice.title)
                        .body(&notice.body)
                        .show();
                    let focused = handle
                        .get_webview_window("main")
                        .and_then(|w| w.is_focused().ok())
                        .unwrap_or(false);
                    if !focused {
                        *handle.state::<Unseen>().0.lock().unwrap() = Some(notice.item.clone());
                    }
                });
                app.manage(engine.clone());
                tauri::async_runtime::spawn(async move {
                    engine.launch();
                });
                Ok(())
            })
            .on_window_event(|window, event| {
                if let tauri::WindowEvent::Focused(true) = event {
                    let unseen = window
                        .try_state::<Unseen>()
                        .and_then(|u| u.0.lock().unwrap().take());
                    if let Some(item) = unseen {
                        let _ = window.emit("orbit-open", item);
                    }
                }
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
                choose_checkout,
                poll_now,
                connect_services,
                start_review,
                cancel_review,
                stop_loop,
                continue_loop,
                use_fix,
                ask_buddy,
                ask_buddy_anything,
                use_proposal,
                forget_memory,
                clear_memory,
                publish_review,
                publish_plan,
                send_message,
                set_legacy_chat_model,
                answer_approval,
                get_files,
                get_scan_plugin,
                install_scan_plugin,
                start_scan,
                cancel_scan,
                delete_scan,
                get_scan_report,
                list_repositories,
                open_github,
                get_remote_access,
                set_remote_access,
                reset_remote_link
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
