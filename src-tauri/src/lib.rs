mod commands;
mod configuration;
mod discovery;
mod i18n;
mod lifecycle;
mod models;
mod persistence;
mod runtime;
use models::*;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Emitter, Manager};

pub struct AppState {
    db: Mutex<rusqlite::Connection>,
    data: PathBuf,
    snapshots: Mutex<HashMap<String, RuntimeSnapshot>>,
    gateways: tokio::sync::Mutex<HashMap<String, project_gateway::Gateway>>,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    previews: Mutex<HashMap<String, configuration::ConfigPreview>>,
    tasks: Arc<lifecycle::TaskRegistry>,
    task_history: Mutex<HashMap<String, TaskProgress>>,
    logs: Mutex<HashMap<String, VecDeque<LogEntry>>>,
    sequence: AtomicU64,
    quitting: AtomicBool,
    exit_ready: AtomicBool,
    active_indexes: AtomicUsize,
}
impl AppState {
    fn recover_cli_index_errors(&self, app: &tauri::AppHandle) {
        let changed = {
            let mut snapshots = self.snapshots.lock().unwrap();
            snapshots
                .values_mut()
                .filter_map(|snapshot| {
                    if discovery::clear_cli_index_error(snapshot) {
                        snapshot.sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
                        snapshot.timestamp = now();
                        Some(snapshot.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        for snapshot in changed {
            let _ = app.emit("project-state-changed", snapshot);
        }
    }
    fn project(&self, id: &str) -> Result<Project> {
        persistence::get(&self.db.lock().unwrap(), id)
    }
    fn operation_lock(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.locks
            .lock()
            .unwrap()
            .entry(id.into())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
    fn snapshot(&self, id: &str) -> RuntimeSnapshot {
        self.snapshots
            .lock()
            .unwrap()
            .entry(id.into())
            .or_insert_with(|| RuntimeSnapshot::new(id))
            .clone()
    }
    fn publish(&self, app: &tauri::AppHandle, mut s: RuntimeSnapshot) {
        s.sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        s.timestamp = now();
        self.snapshots
            .lock()
            .unwrap()
            .insert(s.project_id.clone(), s.clone());
        let _ = app.emit("project-state-changed", s);
    }
    fn log(&self, app: &tauri::AppHandle, id: &str, level: &str, stage: &str, message: &str) {
        let s = self.snapshot(id);
        let item = LogEntry {
            project_id: id.into(),
            generation: s.generation,
            sequence: self.sequence.fetch_add(1, Ordering::SeqCst) + 1,
            timestamp: now(),
            level: level.into(),
            stage: stage.into(),
            message: message.chars().take(4096).collect(),
        };
        let mut logs = self.logs.lock().unwrap();
        let queue = logs.entry(id.into()).or_default();
        queue.push_back(item.clone());
        while queue.len() > 2000 {
            queue.pop_front();
        }
        drop(logs);
        let dir = self.data.join("logs").join(id);
        if std::fs::create_dir_all(&dir).is_ok() {
            let path = dir.join("current.jsonl");
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > 10 * 1024 * 1024) {
                for i in (1..5).rev() {
                    let _ = std::fs::rename(
                        dir.join(format!("{i}.jsonl")),
                        dir.join(format!("{}.jsonl", i + 1)),
                    );
                }
                let _ = std::fs::rename(&path, dir.join("1.jsonl"));
            }
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                use std::io::Write;
                let _ = writeln!(f, "{}", serde_json::to_string(&item).unwrap());
            }
        }
        let _ = app.emit("project-log", item);
    }
    fn task_event(
        &self,
        app: &tauri::AppHandle,
        id: &str,
        op: &str,
        kind: &str,
        state: &str,
        error: Option<AppError>,
    ) {
        let s = self.snapshot(id);
        let event = TaskProgress {
            started_at: self
                .task_history
                .lock()
                .unwrap()
                .get(op)
                .map(|t| t.started_at.clone())
                .unwrap_or_else(now),
            operation_id: op.into(),
            project_id: id.into(),
            generation: s.generation,
            sequence: self.sequence.fetch_add(1, Ordering::SeqCst) + 1,
            timestamp: now(),
            kind: kind.into(),
            state: state.into(),
            message: error
                .as_ref()
                .map(|e| {
                    e.source_message
                        .clone()
                        .unwrap_or_else(|| e.message.clone())
                })
                .unwrap_or_else(|| state.into()),
            error,
        };
        self.task_history
            .lock()
            .unwrap()
            .insert(op.into(), event.clone());
        let _ = app.emit("task-progress", event);
    }
}
static UPDATE_SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
struct TrayLabels {
    show: tauri::menu::MenuItem<tauri::Wry>,
    stop: tauri::menu::MenuItem<tauri::Wry>,
    quit: tauri::menu::MenuItem<tauri::Wry>,
}
fn update_tray_language(app: &tauri::AppHandle) -> Result<()> {
    if let Some(labels) = app.try_state::<TrayLabels>() {
        for (item, zh, en) in [
            (&labels.show, "显示主窗口", "Show window"),
            (&labels.stop, "停止全部", "Stop all projects"),
            (&labels.quit, "退出", "Quit"),
        ] {
            item.set_text(i18n::tr(zh, en))
                .map_err(|e| AppError::new("TRAY_UPDATE_FAILED", e))?;
        }
    }
    Ok(())
}
fn shutdown(app: tauri::AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        UPDATE_SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
        return;
    };
    if state.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let report = state
            .tasks
            .close_and_drain(
                std::time::Duration::from_secs(15),
                std::time::Duration::from_secs(2),
            )
            .await;
        let gateways =
            match tokio::time::timeout(std::time::Duration::from_secs(2), state.gateways.lock())
                .await
            {
                Ok(mut gateways) => std::mem::take(&mut *gateways),
                Err(_) => {
                    state.exit_ready.store(true, Ordering::SeqCst);
                    app.exit(1);
                    return;
                }
            };
        let mut stops = tokio::task::JoinSet::new();
        for (id, gateway) in gateways {
            stops.spawn(async move {
                let revoked = project_protocol::remove_runtime(&id).is_ok();
                let stopped =
                    tokio::time::timeout(std::time::Duration::from_secs(10), gateway.stop())
                        .await
                        .is_ok_and(|r| r.is_ok());
                (id, revoked && stopped)
            });
        }
        let mut clean = report.remaining == 0;
        while let Some(result) = stops.join_next().await {
            match result {
                Ok((id, true)) => {
                    let mut snapshot = state.snapshot(&id);
                    snapshot.state = "stopped".into();
                    snapshot.pid = None;
                    snapshot.sessions = 0;
                    snapshot.started_at = None;
                    state.publish(&app, snapshot);
                    state.log(
                        &app,
                        &id,
                        "info",
                        "shutdown",
                        i18n::tr(
                            "受管 CodeGraph 已停止，桌面即将退出",
                            "Managed CodeGraph has stopped. The desktop application is exiting",
                        ),
                    );
                }
                Ok((id, false)) => {
                    clean = false;
                    state.log(
                        &app,
                        &id,
                        "error",
                        "shutdown",
                        i18n::tr("进程停止或运行记录清理失败，已回收本应用持有的进程资源","Process shutdown or runtime record cleanup failed; owned process resources have been released"),
                    );
                }
                Err(_) => clean = false,
            }
        }
        let _aborted_tasks = report.aborted;
        state.exit_ready.store(true, Ordering::SeqCst);
        app.exit(if clean { 0 } else { 1 });
    });
}
#[cfg(unix)]
fn install_shutdown_signals(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        let (Ok(mut term), Ok(mut interrupt)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) else {
            return;
        };
        tokio::select! {_=term.recv()=>{},_=interrupt.recv()=>{}}
        shutdown(app);
    });
}
pub fn run() {
    let update_only = lifecycle::requests_update(&std::env::args().collect::<Vec<_>>());
    let mut context = tauri::generate_context!();
    if update_only {
        for window in &mut context.config_mut().app.windows {
            window.visible = false;
        }
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if lifecycle::requests_update(&args) {
                if std::env::current_exe()
                    .is_ok_and(|exe| lifecycle::update_targets_current_executable(&args, &exe))
                {
                    shutdown(app.clone());
                }
                return;
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            if update_only || UPDATE_SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
                app.handle().exit(0);
                return Ok(());
            }
            let data = app.path().app_data_dir()?;
            configuration::private_dir(&data).map_err(|e| std::io::Error::other(e.message))?;
            configuration::mark_interrupted(&data).map_err(|e| std::io::Error::other(e.message))?;
            let db = persistence::open(&data).map_err(|e| std::io::Error::other(e.message))?;
            i18n::set(&persistence::language(&db).map_err(|e| std::io::Error::other(e.message))?);
            app.manage(AppState {
                db: Mutex::new(db),
                data,
                snapshots: Mutex::new(HashMap::new()),
                gateways: Default::default(),
                locks: Default::default(),
                previews: Default::default(),
                tasks: Default::default(),
                task_history: Default::default(),
                logs: Default::default(),
                sequence: AtomicU64::new(0),
                quitting: AtomicBool::new(false),
                exit_ready: AtomicBool::new(false),
                active_indexes: AtomicUsize::new(0),
            });
            #[cfg(unix)]
            install_shutdown_signals(app.handle().clone());
            if UPDATE_SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
                shutdown(app.handle().clone());
                return Ok(());
            }
            use tauri::{
                menu::{Menu, MenuItem},
                tray::TrayIconBuilder,
            };
            let show = MenuItem::with_id(
                app,
                "show",
                i18n::tr("显示主窗口", "Show window"),
                true,
                None::<&str>,
            )?;
            let stop = MenuItem::with_id(
                app,
                "stop",
                i18n::tr("停止全部", "Stop all projects"),
                true,
                None::<&str>,
            )?;
            let quit =
                MenuItem::with_id(app, "quit", i18n::tr("退出", "Quit"), true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &stop, &quit])?;
            app.manage(TrayLabels { show, stop, quit });
            let mut tray =
                TrayIconBuilder::new()
                    .menu(&menu)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "quit" => shutdown(app.clone()),
                        "stop" => {
                            let app = app.clone();
                            tauri::async_runtime::spawn(async move {
                                let state = app.state::<AppState>();
                                let ids: Vec<_> =
                                    state.gateways.lock().await.keys().cloned().collect();
                                for id in ids {
                                    let lock = state.operation_lock(&id);
                                    let _guard = lock.lock().await;
                                    let _ = runtime::stop(&app, &state, &id).await;
                                }
                            });
                        }
                        _ => (),
                    });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            let monitor = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    let state = monitor.state::<AppState>();
                    if state.quitting.load(Ordering::SeqCst) {
                        break;
                    }
                    let gateways = state.gateways.lock().await;
                    for (id, g) in gateways.iter() {
                        let mut s = state.snapshot(id);
                        let changed = s.sessions != g.session_count()
                            || (!g.is_alive() && s.state != "error");
                        s.sessions = g.session_count();
                        if !g.is_alive() {
                            s.state = "error".into();
                            s.error = Some(AppError::new(
                                "UPSTREAM_EXITED",
                                "CodeGraph 会话异常退出，请重启",
                            ));
                            let _ = project_protocol::remove_runtime(id);
                        }
                        if changed {
                            state.publish(&monitor, s);
                        }
                        for line in g.drain_logs() {
                            state.log(&monitor, id, "info", "codegraph", &line);
                        }
                    }
                }
            });
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let projects = commands::list_projects(handle.state()).unwrap_or_default();
                for p in projects {
                    if p.auto_start {
                        let _ = runtime::launch_operation(handle.clone(), p.id, "start".into());
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let Some(state) = window.try_state::<AppState>() else {
                    return;
                };
                api.prevent_close();
                let behavior = persistence::setting(&state.db.lock().unwrap(), "closeBehavior")
                    .ok()
                    .flatten();
                if behavior.as_deref() == Some("exit") {
                    shutdown(window.app_handle().clone());
                } else {
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::add_project,
            commands::update_project,
            commands::relocate_project,
            commands::remove_project,
            commands::get_settings,
            commands::save_settings,
            commands::detect_codegraph,
            commands::set_codegraph_entry,
            commands::get_project_snapshot,
            commands::refresh_index_status,
            commands::start_project,
            commands::stop_project,
            commands::restart_project,
            commands::run_index_task,
            commands::cancel_task,
            commands::read_logs,
            commands::preview_client_config,
            commands::apply_client_config,
            commands::get_client_config_status,
            commands::test_project_mcp,
            commands::get_project_tasks,
            commands::list_config_backups,
            commands::preview_restore_backup,
            commands::preview_previous_config,
            commands::open_project_directory,
            commands::open_app_data_directory,
            commands::export_project_logs
        ])
        .build(context);
    let app = match app {
        Ok(app) => app,
        Err(error) => {
            if update_only {
                return;
            }
            let data = project_protocol::runtime_dir()
                .ok()
                .and_then(|p| p.parent().map(|v| v.to_string_lossy().to_string()))
                .unwrap_or_else(|| {
                    i18n::tr(
                        "无法取得应用数据目录",
                        "Application data directory unavailable",
                    )
                    .into()
                });
            let description = if i18n::current() == "zh-CN" {
                format!("应用无法启动：{error}\n\n应用数据目录：{data}\n\n请保留 app.db 与 backups 目录以便恢复。应用没有重置数据库。")
            } else {
                format!("The application could not start: {error}\n\nApplication data directory: {data}\n\nPreserve app.db and the backups directory for recovery. The database has not been reset.")
            };
            rfd::MessageDialog::new()
                .set_title(i18n::tr(
                    "CodeGraph Desktop 启动失败",
                    "CodeGraph Desktop could not start",
                ))
                .set_level(rfd::MessageLevel::Error)
                .set_description(description)
                .show();
            return;
        }
    };
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if app
                .try_state::<AppState>()
                .is_some_and(|state| !state.exit_ready.load(Ordering::SeqCst))
            {
                api.prevent_exit();
                shutdown(app.clone());
            }
        }
    });
}
