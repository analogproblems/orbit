pub mod actions_storage;
pub mod alerts;
pub mod antigravity;
pub mod api;
pub mod ask;
pub mod attention;
pub mod boxtool;
pub mod brief;
pub mod broke;
pub mod buddy;
pub mod ci;
pub mod claude;
pub mod cli;
pub mod client;
pub mod clock;
pub mod devcontainer;
pub mod engine;
pub mod extras;
pub mod github;
pub mod guide;
pub mod ledger;
pub mod logs;
pub mod models;
pub mod moving;
pub mod ollama;
pub mod ops;
pub mod orbits;
pub mod problems;
pub mod readiness;
pub mod remote;
pub mod reruns;
pub mod review_instructions;
pub mod review_loop;
pub mod rpc;
pub mod runners;
pub mod scans;
pub mod server;
pub mod service;
pub mod setup;
pub mod storage;
pub mod text;
pub mod types;
pub mod usage;
pub mod workflows;

#[cfg(feature = "desktop")]
mod desktop {
    use super::*;
    use serde_json::{json, Value};
    use std::{path::PathBuf, sync::Arc};
    use tauri::{AppHandle, Emitter, Manager, State};
    use tauri_plugin_dialog::DialogExt;
    use tauri_plugin_notification::NotificationExt;
    type AppState = Arc<engine::Engine>;
    /// The always-on Orbit this app shows instead of running its own
    /// (`crate::client`). `chosen` when you gave its link in Settings;
    /// otherwise it's Orbit without a window using this app's data folder,
    /// shown while it runs.
    struct AlwaysOn {
        client: Arc<client::Client>,
        chosen: bool,
    }
    /// What Settings shows about the always-on Orbit in use.
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct AlwaysOnStatus {
        host: String,
        chosen: bool,
        /// Shown while the app can't tell whether its Orbit moved there.
        pending: bool,
    }
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
        save_markdown(app, name, text, REPORT).await
    }
    /// How the save dialog asks for a report, and for the Decisions page.
    const REPORT: (&str, &str) = ("Save Markdown report (.md)", "Markdown report");
    const DECISIONS: (&str, &str) = ("Save decisions (.md)", "Markdown");
    /// Asks where to save `text`, suggesting `name`, and saves it there.
    /// Rust owns the dialog and the content; no general file-write API or
    /// frontend-supplied destination is exposed.
    async fn save_markdown(
        app: AppHandle,
        name: String,
        text: String,
        (title, filter): (&'static str, &'static str),
    ) -> Result<Option<String>, String> {
        tauri::async_runtime::spawn_blocking(move || {
            let path = storage::select_report_path(name.into(), |suggested| {
                let mut dialog = app
                    .dialog()
                    .file()
                    .set_title(title)
                    .set_file_name(suggested.file_name().unwrap_or_default().to_string_lossy())
                    .add_filter(filter, &["md"]);
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
        engine.watch_ci_soon();
        engine.update_clones_soon();
        Ok(())
    }
    #[tauri::command]
    async fn get_decisions(engine: State<'_, AppState>) -> Result<ledger::Decisions, String> {
        engine.decisions().await
    }
    #[tauri::command]
    async fn refresh_decisions(engine: State<'_, AppState>) -> Result<(), String> {
        engine.refresh_ledger_fully();
        Ok(())
    }
    /// Saves the Decisions page as Markdown where you choose.
    #[tauri::command]
    async fn export_decisions(
        app: tauri::AppHandle,
        engine: State<'_, AppState>,
        repo: Option<String>,
    ) -> Result<Option<String>, String> {
        let (name, text) = engine.export_decisions(repo).await?;
        save_markdown(app, name, text, DECISIONS).await
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
    async fn tell_agent(engine: State<'_, AppState>, key: String) -> Result<(), String> {
        engine.tell_agent(&key).await
    }
    #[tauri::command]
    async fn rerun_job(
        engine: State<'_, AppState>,
        key: String,
        number: u32,
    ) -> Result<(), String> {
        engine.rerun_job(&key, number).await
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
    async fn close_brief(engine: State<'_, AppState>, day: String) -> Result<(), String> {
        engine.close_brief(&day).await
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
    async fn set_api_key(
        engine: State<'_, AppState>,
        provider: String,
        key: Option<String>,
    ) -> Result<(), String> {
        engine.set_api_key(&provider, key).await
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
        if !guide::may_open(&url) {
            return Err("Only GitHub links and Bit's setup pages can be opened.".into());
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
    // The always-on Orbit is chosen from the desktop window only, so these
    // are left out of `remote::COMMANDS` too.
    #[tauri::command]
    async fn get_always_on(app: AppHandle) -> Result<Option<AlwaysOnStatus>, String> {
        let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
        Ok(app.try_state::<AlwaysOn>().map(|a| AlwaysOnStatus {
            host: a.client.link().host().into(),
            chosen: a.chosen,
            pending: client::pending_move(&root).is_some(),
        }))
    }
    /// Runs one of `remote::COMMANDS` on the always-on Orbit this app shows,
    /// as a browser there would. Reports and decisions come back as text,
    /// and are saved on this computer.
    #[tauri::command]
    async fn forward(app: AppHandle, command: String, args: Value) -> Result<Value, String> {
        let Some(always_on) = app.try_state::<AlwaysOn>().map(|a| a.client.clone()) else {
            return Err("This app isn't using an always-on Orbit.".into());
        };
        if !remote::COMMANDS.contains(&command.as_str()) {
            return Err("This action is not available here.".into());
        }
        let answer = always_on.call(&command, &args).await?;
        let ask = match command.as_str() {
            "export_report" => REPORT,
            "export_decisions" => DECISIONS,
            _ => return Ok(answer),
        };
        let text = answer["text"]
            .as_str()
            .ok_or("The always-on Orbit sent nothing to save.")?;
        // Only its name: a folder there means nothing here.
        let name = answer["fileName"]
            .as_str()
            .and_then(|n| std::path::Path::new(n).file_name())
            .map_or_else(|| "orbit.md".into(), |n| n.to_string_lossy().into_owned());
        Ok(json!(save_markdown(app, name, text.into(), ask).await?))
    }
    /// Uses the always-on Orbit at `link` from now on, once it answers: the
    /// app starts again showing it, with no Orbit of its own.
    #[tauri::command]
    async fn use_always_on(app: AppHandle, link: String) -> Result<(), String> {
        let link = client::Link::parse(&link)?;
        let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
        // While this app runs its own Orbit, the token in its folder is its own.
        if app.try_state::<AppState>().is_some()
            && remote::saved_token(&root).as_deref() == Some(link.token.as_str())
        {
            return Err(client::OWN_LINK.into());
        }
        client::Client::new(link.clone())?.check().await?;
        // A move the app can't tell the outcome of yet is asked about there.
        if let Some(mut moving) = client::pending_move(&root) {
            moving.link = link.clone();
            client::begin_move(&root, &moving)?;
        }
        client::save(&root, &link)?;
        app.request_restart();
        Ok(())
    }
    /// Moves this app's Orbit to the always-on Orbit at `link`, then uses
    /// it as `use_always_on` does: its repositories, settings, reviews,
    /// Bit's notes and memory, CI notes and history (see `crate::moving`).
    /// That Orbit must have nothing of its own yet and be signed in to
    /// GitHub as this one is. This app's Orbit stops first, and does
    /// nothing more once what it was doing is done, so nothing it does
    /// lands after it's packed. It keeps its state with autopilot off. If
    /// the move doesn't happen, a message says why and this app's Orbit
    /// starts again as it was; if the app can't tell, it runs no Orbit of
    /// its own until it can (`client::Moving`).
    #[tauri::command]
    async fn move_to_always_on(
        app: AppHandle,
        engine: State<'_, AppState>,
        link: String,
    ) -> Result<(), String> {
        let link = client::Link::parse(&link)?;
        let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
        if remote::saved_token(&root).as_deref() == Some(link.token.as_str()) {
            return Err(client::OWN_LINK.into());
        }
        let always_on = client::Client::new(link.clone())?;
        let before = always_on.snapshot().await?;
        let here = engine.snapshot().await;
        client::can_take_in(&before, here.github_user.as_deref())?;
        if let Some(why) = engine.cannot_move().await {
            return Err(why);
        }
        // From here on this app's Orbit has stopped, so the app starts
        // again whatever happens.
        engine.shutdown().await;
        let (title, message) =
            match client::move_to(&engine, &root, &always_on).await {
                // Still noted as moving until both are kept, so the app
                // finishes as it starts if it can't now.
                Ok(()) => match client::save(&root, &link).and_then(|()| client::end_move(&root)) {
                    Ok(()) => {
                        app.request_restart();
                        return Ok(());
                    }
                    Err(e) => (
                        "Orbit moved",
                        format!("Orbit moved, but couldn't note it yet: {e}. It tries again when it starts."),
                    ),
                },
                Err(client::Failed::Stayed(error)) => (
                    "Orbit didn't move",
                    format!("{error}\n\nThis app's own Orbit starts again as it was."),
                ),
                Err(client::Failed::Unsure(error)) => (
                    "Orbit can't tell yet whether it moved",
                    format!("{error}\n\n{}", client::UNSURE),
                ),
            };
        let restart = app.clone();
        app.dialog()
            .message(message.clone())
            .title(title)
            .kind(tauri_plugin_dialog::MessageDialogKind::Error)
            .show(move |_| restart.request_restart());
        Err(message)
    }
    /// Stops using the always-on Orbit: the app starts again with its own.
    #[tauri::command]
    async fn stop_always_on(app: AppHandle) -> Result<(), String> {
        let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
        // Also while a move is unsure: you say this app's Orbit runs.
        client::end_move(&root)?;
        client::forget(&root)?;
        app.request_restart();
        Ok(())
    }
    /// Shows a notification, and keeps its item to open when Orbit next
    /// comes to the front if it's in the background.
    fn notify(app: &AppHandle, title: &str, body: &str, item: &types::Attention) {
        let _ = app.notification().builder().title(title).body(body).show();
        let focused = app
            .get_webview_window("main")
            .and_then(|w| w.is_focused().ok())
            .unwrap_or(false);
        if !focused {
            *app.state::<Unseen>().0.lock().unwrap() = Some(item.clone());
        }
    }
    /// Shows the always-on Orbit at `link` in the window, with its
    /// notifications, instead of running an Orbit here.
    fn show_always_on(
        app: &AppHandle,
        root: PathBuf,
        link: client::Link,
        chosen: bool,
    ) -> Result<(), String> {
        let client = Arc::new(client::Client::new(link)?);
        app.manage(AlwaysOn {
            client: client.clone(),
            chosen,
        });
        let (shown, notified, problem) = (app.clone(), app.clone(), app.clone());
        tauri::async_runtime::spawn(async move {
            client
                .follow(
                    &root,
                    |snapshot| {
                        let _ = shown.emit("orbit-state", snapshot);
                    },
                    |n| notify(&notified, &n.title, &n.body, &n.item),
                    |p| {
                        let _ = problem.emit("orbit-connection", p);
                    },
                )
                .await;
        });
        Ok(())
    }
    /// Shows Orbit without a window using the data folder `root` on this
    /// computer, if it answers.
    fn show_running_here(app: &AppHandle, root: &std::path::Path) -> bool {
        let Some(link) = tauri::async_runtime::block_on(client::running_here(root)) else {
            return false;
        };
        show_always_on(app, root.into(), link, false).is_ok()
    }
    /// The commands the window may still run while this app's Orbit moves.
    const WHILE_MOVING: &[&str] = &["get_snapshot", "get_always_on"];
    /// `commands`, refusing new ones while this app's Orbit moves to an
    /// always-on Orbit (`Engine::fence`), as a browser's are refused.
    fn while_moving(
        commands: impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static,
    ) -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
        move |invoke| {
            let moving = invoke
                .message
                .webview()
                .try_state::<AppState>()
                .is_some_and(|engine| engine.moving());
            if moving && !WHILE_MOVING.contains(&invoke.message.command()) {
                invoke.resolver.reject(engine::MOVING);
                return true;
            }
            commands(invoke)
        }
    }
    pub fn run() {
        tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_notification::init())
            .setup(|app| {
                let handle = app.handle().clone();
                let root = app.path().app_data_dir()?;
                app.manage(Unseen::default());
                // A move the app couldn't tell the outcome of: asked again.
                let mut stayed = None;
                if let Some(moving) = client::pending_move(&root) {
                    match tauri::async_runtime::block_on(client::settle(&moving)) {
                        client::Outcome::Stayed => stayed = Some(moving.automatic),
                        // It moved, or the app still can't tell: either way
                        // it runs no Orbit of its own.
                        outcome => {
                            if outcome == client::Outcome::Moved
                                && client::save(&root, &moving.link).is_ok()
                            {
                                let _ = client::end_move(&root);
                            }
                            show_always_on(app.handle(), root, moving.link, true)?;
                            return Ok(());
                        }
                    }
                }
                if let Some(link) = client::load(&root) {
                    // It didn't move, and you chose an always-on Orbit
                    // since: this app's Orbit keeps autopilot off.
                    if stayed.is_some() {
                        let _ = client::end_move(&root);
                    }
                    show_always_on(app.handle(), root, link, true)?;
                    return Ok(());
                }
                let hub = remote::Hub::default();
                let browsers = hub.clone();
                let engine =
                    match engine::Engine::new(root.clone(), move |snapshot: &types::Snapshot| {
                        let _ = handle.emit("orbit-state", snapshot);
                        browsers.publish(snapshot);
                    }) {
                        Ok(engine) => engine,
                        // Orbit without a window uses this folder: show it.
                        Err(error)
                            if error == engine::ALREADY_RUNNING
                                && show_running_here(app.handle(), &root) =>
                        {
                            return Ok(());
                        }
                        Err(error) => {
                            // Returning the error would end Orbit with nothing
                            // on screen, so it's shown before Orbit closes.
                            for window in app.webview_windows().values() {
                                let _ = window.hide();
                            }
                            let exit = app.handle().clone();
                            app.dialog()
                                .message(error)
                                .title("Orbit can't start")
                                .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                                .show(move |_| exit.exit(1));
                            return Ok(());
                        }
                    };
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
                // It didn't move: autopilot as it was before the move.
                if let Some(automatic) = stayed {
                    if tauri::async_runtime::block_on(engine.set_autopilot(automatic)).is_ok() {
                        let _ = client::end_move(&root);
                    }
                }
                let access = remote::Access::new(engine.clone(), hub, root, assets);
                app.manage(access.clone());
                tauri::async_runtime::spawn(async move { access.start().await });
                let handle = app.handle().clone();
                engine.on_notice(move |n| notify(&handle, &n.title, &n.body, &n.item));
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
            .invoke_handler(while_moving(tauri::generate_handler![
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
                get_decisions,
                refresh_decisions,
                export_decisions,
                connect_services,
                start_review,
                cancel_review,
                stop_loop,
                continue_loop,
                use_fix,
                tell_agent,
                rerun_job,
                ask_buddy,
                ask_buddy_anything,
                use_proposal,
                forget_memory,
                close_brief,
                clear_memory,
                set_api_key,
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
                reset_remote_link,
                get_always_on,
                forward,
                use_always_on,
                move_to_always_on,
                stop_always_on
            ]))
            .build(tauri::generate_context!())
            .expect("Unable to start Orbit")
            .run(|app, event| {
                if let tauri::RunEvent::ExitRequested { .. } = event {
                    // There is no engine when Orbit couldn't start.
                    if let Some(engine) = app.try_state::<AppState>() {
                        tauri::async_runtime::block_on(engine.shutdown());
                    }
                }
            });
    }
}
#[cfg(feature = "desktop")]
pub use desktop::run;
