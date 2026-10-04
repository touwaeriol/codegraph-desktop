use crate::{configuration::Target, models::*, persistence, AppState};
use rusqlite::OptionalExtension;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{Manager, State};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub state: String,
    pub pid: Option<u32>,
    pub endpoint: Option<String>,
    pub started_at: Option<String>,
    pub tools: Vec<String>,
    pub error: Option<AppError>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            state: "stopped".into(),
            pid: None,
            endpoint: None,
            started_at: None,
            tools: vec![],
            error: None,
        }
    }
}
pub struct Instance {
    child: tokio::process::Child,
    job: project_gateway::ProcessJob,
}
impl Instance {
    pub async fn stop(&mut self) -> Result<()> {
        self.job
            .terminate_and_wait()
            .await
            .map_err(|e| AppError::new("SERENA_STOP_FAILED", e))?;
        tokio::time::timeout(Duration::from_secs(3), self.child.wait())
            .await
            .map_err(|e| AppError::new("SERENA_STOP_FAILED", e))??;
        Ok(())
    }
}
pub fn binding(state: &AppState, id: &str) -> Result<Target> {
    let mut db = state.db.lock().unwrap();
    binding_db(&mut db, id)
}
pub fn binding_db(db: &mut rusqlite::Connection, id: &str) -> Result<Target> {
    persistence::get(db, id)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let existing = tx
        .query_row(
            "SELECT port FROM serena_http WHERE project_id=?1",
            [id],
            |r| r.get::<_, u16>(0),
        )
        .optional()?;
    let port = if let Some(port) = existing {
        port
    } else {
        let mut selected = None;
        for _ in 0..32 {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
            let candidate = listener.local_addr()?.port();
            let used: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM serena_http WHERE port=?1 UNION ALL SELECT 1 FROM shared_http WHERE port=?1)", [candidate], |r| r.get(0))?;
            if !used {
                tx.execute(
                    "INSERT INTO serena_http(project_id,port) VALUES(?1,?2)",
                    rusqlite::params![id, candidate],
                )?;
                selected = Some(candidate);
                break;
            }
        }
        selected.ok_or_else(|| {
            AppError::new(
                "PORT_UNAVAILABLE",
                crate::i18n::tr("无法分配 Serena 本机端口", "No free Serena port available"),
            )
        })?
    };
    tx.commit()?;
    Ok(Target {
        name: "serena".into(),
        endpoint: format!("http://127.0.0.1:{port}/mcp"),
        authorization: None,
    })
}
fn candidates(saved: Option<String>) -> Vec<PathBuf> {
    if let Some(saved) = saved {
        return vec![PathBuf::from(saved)];
    }
    let mut paths = vec![];
    if let Ok(path) = which::which("serena") {
        paths.push(path);
    }
    for key in ["USERPROFILE", "HOME"] {
        if let Some(home) = std::env::var_os(key) {
            paths.push(
                PathBuf::from(home)
                    .join(".local/bin")
                    .join(if cfg!(windows) {
                        "serena.exe"
                    } else {
                        "serena"
                    }),
            );
        }
    }
    paths
}
fn validate_entry(path: &Path) -> Result<PathBuf> {
    let path = dunce::canonicalize(path)?;
    if !path.is_file()
        || path
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("cmd") || v.eq_ignore_ascii_case("bat"))
    {
        return Err(AppError::new(
            "SERENA_INVALID",
            crate::i18n::tr(
                "请选择 Serena 可执行文件，而不是 Shell 脚本",
                "Select the Serena executable, not a shell command",
            ),
        ));
    }
    Ok(path)
}
async fn output(path: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new(path);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (child, _job) = project_gateway::spawn_owned(&mut command)
        .map_err(|e| AppError::new("SERENA_INVALID", e))?;
    let out = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .map_err(|e| AppError::new("SERENA_TIMEOUT", e))??;
    if !out.status.success() {
        return Err(AppError::new(
            "SERENA_INVALID",
            String::from_utf8_lossy(&out.stderr),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().into())
}
#[tauri::command]
pub async fn detect_serena(
    state: State<'_, AppState>,
    selected_path: Option<String>,
) -> Result<Environment> {
    let saved = persistence::setting(&state.db.lock().unwrap(), "serenaEntry")?;
    let requested = selected_path.filter(|s| !s.trim().is_empty());
    let paths = candidates(requested.or(saved.clone()));
    let mut failure = crate::i18n::tr(
        "未找到 Serena，请安装后选择可执行入口",
        "Serena was not found. Install it, then select its executable",
    )
    .to_string();
    for path in paths {
        let result = async {
            let path = validate_entry(&path)?;
            let version = output(&path, &["--version"]).await?;
            let help = output(&path, &["start-mcp-server", "--help"]).await?;
            if !version.to_lowercase().contains("serena")
                || ![
                    "streamable-http",
                    "--project",
                    "--context",
                    "--enable-web-dashboard",
                    "--host",
                ]
                .iter()
                .all(|arg| help.contains(arg))
            {
                return Err(AppError::new(
                    "SERENA_INCOMPATIBLE",
                    crate::i18n::tr(
                        "Serena 入口必须支持原生 HTTP、项目和上下文参数",
                        "Serena requires native HTTP, project and context support",
                    ),
                ));
            }
            Ok((path, version))
        }
        .await;
        match result {
            Ok((path, version)) => {
                let db = state.db.lock().unwrap();
                if persistence::setting(&db, "serenaEntry")? != saved {
                    return Err(AppError::new("CONFIG_CHANGED", "入口已变化，请重新检测"));
                }
                persistence::set_setting(&db, "serenaEntry", &path.to_string_lossy())?;
                return Ok(Environment {
                    available: true,
                    entry: Some(path.to_string_lossy().into()),
                    version: Some(version),
                    error: None,
                });
            }
            Err(error) => failure = error.message,
        }
    }
    Ok(Environment {
        available: false,
        entry: saved,
        version: None,
        error: Some(failure),
    })
}
#[tauri::command]
pub async fn get_serena_snapshot(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Snapshot> {
    state.project(&project_id)?;
    let mut snapshot = state
        .serena_snapshots
        .lock()
        .unwrap()
        .get(&project_id)
        .cloned()
        .unwrap_or_default();
    if let Some(instance) = state.serena_instances.lock().await.get_mut(&project_id) {
        if instance.child.try_wait()?.is_some() {
            snapshot.state = "error".into();
            snapshot.pid = None;
            snapshot.error = Some(AppError::new(
                "SERENA_EXITED",
                crate::i18n::tr(
                    "Serena 已退出，请查看运行日志并重新启动",
                    "Serena exited. See project logs and restart it",
                ),
            ));
        }
    }
    Ok(snapshot)
}
#[tauri::command]
pub async fn list_serena_snapshots(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, Snapshot>> {
    let mut snapshots = state.serena_snapshots.lock().unwrap().clone();
    for (id, instance) in state.serena_instances.lock().await.iter_mut() {
        if instance.child.try_wait()?.is_some() {
            let snapshot = snapshots.entry(id.clone()).or_default();
            snapshot.state = "error".into();
            snapshot.pid = None;
            snapshot.error = Some(AppError::new(
                "SERENA_EXITED",
                crate::i18n::tr(
                    "Serena 已退出，请查看运行日志并重新启动",
                    "Serena exited. See project logs and restart it",
                ),
            ));
        }
    }
    Ok(snapshots)
}
fn publish(state: &AppState, id: &str, snapshot: Snapshot) {
    state
        .serena_snapshots
        .lock()
        .unwrap()
        .insert(id.into(), snapshot);
}
pub async fn stop(state: &AppState, id: &str) -> Result<()> {
    let mut instances = state.serena_instances.lock().await;
    if let Some(instance) = instances.get_mut(id) {
        instance.stop().await?;
    }
    instances.remove(id);
    publish(state, id, Snapshot::default());
    Ok(())
}
fn valid_tools(tools: &[String]) -> bool {
    tools.iter().any(|t| t == "find_symbol") && !tools.iter().any(|t| t == "activate_project")
}
fn server_command(entry: &Path, root: &Path, port: &str) -> Command {
    let mut command = Command::new(entry);
    // GUI launches may not have the user's uv tools directory on PATH.
    let mut paths = entry
        .parent()
        .into_iter()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    if let Ok(path) = std::env::join_paths(paths) {
        command.env("PATH", path);
    }
    command.env("PYTHONIOENCODING", "utf-8");
    command
        .args([
            "start-mcp-server",
            "--transport",
            "streamable-http",
            "--host",
            "127.0.0.1",
            "--port",
            port,
            "--context",
            "ide",
            "--project",
        ])
        .arg(root)
        .args([
            "--enable-web-dashboard",
            "false",
            "--open-web-dashboard",
            "false",
            "--enable-gui-log-window",
            "false",
        ])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}
async fn start(app: &tauri::AppHandle, state: &AppState, id: &str) -> Result<()> {
    let project = state.project(id)?;
    let root = persistence::canonical(&project.canonical_path)?;
    let entry =
        persistence::setting(&state.db.lock().unwrap(), "serenaEntry")?.ok_or_else(|| {
            AppError::new(
                "SERENA_MISSING",
                crate::i18n::tr("请先在设置中检测 Serena", "Detect Serena in Settings first"),
            )
        })?;
    let entry = validate_entry(Path::new(&entry))?;
    let target = binding(state, id)?;
    let address = target
        .endpoint
        .trim_start_matches("http://")
        .trim_end_matches("/mcp");
    // Refuse collisions instead of probing/adopting someone else's server.
    let reservation =
        std::net::TcpListener::bind(address).map_err(|e| AppError::new("SERENA_PORT_BUSY", e))?;
    let port = reservation.local_addr()?.port().to_string();
    let mut command = server_command(&entry, &root, &port);
    drop(reservation);
    let (mut child, job) = project_gateway::spawn_owned(&mut command)
        .map_err(|e| AppError::new("SERENA_START_FAILED", e))?;
    let pid = child.id();
    for stream in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
    ]
    .into_iter()
    .flatten()
    {
        let app = app.clone();
        let id = id.to_string();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                app.state::<AppState>()
                    .log(&app, &id, "info", "serena", &line);
            }
        });
    }
    let mut instance = Instance { child, job };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        if instance.child.try_wait()?.is_some() {
            return Err(AppError::new(
                "SERENA_EXITED",
                crate::i18n::tr(
                    "Serena 启动时退出，请查看运行日志",
                    "Serena exited during startup. See project logs",
                ),
            ));
        }
        if let Ok(tools) = project_gateway::probe_native_http(&target.endpoint).await {
            if !valid_tools(&tools) {
                return Err(AppError::new("SERENA_INCOMPATIBLE", crate::i18n::tr("Serena 必须在 IDE 模式提供 find_symbol 并禁用 activate_project，请升级 Serena","Serena must expose find_symbol and disable activate_project in IDE context. Update Serena")));
            }
            // A successful probe must never mask our process failing to bind.
            tokio::time::sleep(Duration::from_millis(200)).await;
            if instance.child.try_wait()?.is_some() {
                return Err(AppError::new(
                    "SERENA_EXITED",
                    crate::i18n::tr("Serena 在启动过程中退出", "Serena exited during startup"),
                ));
            }
            state
                .serena_instances
                .lock()
                .await
                .insert(id.into(), instance);
            publish(
                state,
                id,
                Snapshot {
                    state: "running".into(),
                    pid,
                    endpoint: Some(target.endpoint),
                    started_at: Some(now()),
                    tools,
                    error: None,
                },
            );
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::new(
                "SERENA_TIMEOUT",
                crate::i18n::tr(
                    "Serena 在 120 秒内未就绪，请查看运行日志",
                    "Serena did not become ready within 120 seconds. See project logs",
                ),
            ));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}
#[tauri::command]
pub fn serena_operation(
    app: tauri::AppHandle,
    project_id: String,
    action: String,
) -> Result<String> {
    if !["start", "stop", "restart"].contains(&action.as_str()) {
        return Err(AppError::new("INVALID_ACTION", action));
    }
    let state = app.state::<AppState>();
    if state.quitting.load(Ordering::SeqCst) {
        return Err(AppError::new("APP_EXITING", "应用正在退出"));
    }
    state.project(&project_id)?;
    let op = uuid::Uuid::new_v4().to_string();
    let operation = op.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    state.tasks.clone().spawn(op.clone(),cancel.clone(),async move {
        let state=app.state::<AppState>(); let lock=state.operation_lock(&project_id); let _guard=lock.lock().await;
        let kind=format!("serena-{action}");
        state.task_event(&app,&project_id,&operation,&kind,"running",None);
        let work=async {
            state.project(&project_id)?;
            if action=="start" {
                if let Some(instance)=state.serena_instances.lock().await.get_mut(&project_id) { if instance.child.try_wait()?.is_none() { return Ok(()); } }
            }
            stop(&state,&project_id).await?;
            if action!="stop" {
                publish(&state,&project_id,Snapshot {state:"starting".into(),..Default::default()});
                start(&app,&state,&project_id).await?;
            }
            Ok(())
        };
        let result: Result<()> = tokio::select! {
            biased;
            _=async { while !cancel.load(Ordering::SeqCst) { tokio::time::sleep(Duration::from_millis(50)).await; } } => Err(AppError::new("TASK_CANCELLED","任务已取消")),
            result=work => result,
        };
        if let Err(error)=&result {
            let mut snapshot=state.serena_snapshots.lock().unwrap().get(&project_id).cloned().unwrap_or_default();
            snapshot.state="error".into(); snapshot.error=Some(error.clone()); publish(&state,&project_id,snapshot);
            state.log(&app,&project_id,"error","serena",&error.message);
        }
        state.task_event(&app,&project_id,&operation,&kind,if result.is_ok(){"completed"}else{"failed"},result.err());
    })?;
    Ok(op)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ports_survive_reopen_and_are_distinct_from_shared_gateway() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = persistence::open(dir.path()).unwrap();
        for id in ["a", "b"] {
            persistence::save(
                &db,
                &Project {
                    id: id.into(),
                    name: id.into(),
                    root_path: id.into(),
                    canonical_path: id.into(),
                    previous_roots: vec![],
                    notes: String::new(),
                    auto_start: false,
                    created_at: now(),
                    updated_at: now(),
                },
            )
            .unwrap();
        }
        let shared = persistence::ensure_shared_http_port(&mut db).unwrap();
        let a = binding_db(&mut db, "a").unwrap();
        let b = binding_db(&mut db, "b").unwrap();
        assert_ne!(a.endpoint, b.endpoint);
        assert_ne!(a.endpoint, format!("http://127.0.0.1:{shared}/mcp"));
        drop(db);
        let mut db = persistence::open(dir.path()).unwrap();
        assert_eq!(a.endpoint, binding_db(&mut db, "a").unwrap().endpoint);
        db.execute("DELETE FROM projects WHERE id='a'", []).unwrap();
        assert!(binding_db(&mut db, "a").is_err());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM serena_http", [], |r| r
                .get::<_, usize>(0))
                .unwrap(),
            1
        );
    }
    #[tokio::test]
    #[ignore = "requires SERENA_TEST_ENTRY pointing to an installed Serena executable"]
    async fn real_native_http_symbols_and_process_cleanup() {
        let entry =
            PathBuf::from(std::env::var_os("SERENA_TEST_ENTRY").expect("SERENA_TEST_ENTRY"));
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let mut instances = vec![];
        let mut endpoints = vec![];
        for marker in ["serena_fixture_alpha", "serena_fixture_beta"] {
            let project = root.join(marker);
            std::fs::create_dir_all(project.join(".serena")).unwrap();
            std::fs::write(
                project.join(".serena/project.yml"),
                format!("project_name: {marker}\nlanguages: [python]\nencoding: utf-8\n"),
            )
            .unwrap();
            std::fs::write(
                project.join("example.py"),
                format!("def {marker}():\n    return 42\n"),
            )
            .unwrap();
            let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = reserve.local_addr().unwrap().port().to_string();
            let endpoint = format!("http://127.0.0.1:{port}/mcp");
            let mut command = server_command(&entry, &project, &port);
            command
                .env("SERENA_HOME", root.join("serena-home"))
                .stdout(Stdio::null())
                .stderr(Stdio::from(
                    std::fs::File::create(root.join(format!("{marker}.log"))).unwrap(),
                ));
            drop(reserve);
            let (child, job) = project_gateway::spawn_owned(&mut command).unwrap();
            instances.push(Instance { child, job });
            let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
            let tools = loop {
                if let Ok(tools) = project_gateway::probe_native_http(&endpoint).await {
                    break tools;
                }
                assert!(
                    instances
                        .last_mut()
                        .unwrap()
                        .child
                        .try_wait()
                        .unwrap()
                        .is_none(),
                    "Serena exited: {}",
                    std::fs::read_to_string(root.join(format!("{marker}.log"))).unwrap()
                );
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "Serena startup timed out"
                );
                tokio::time::sleep(Duration::from_millis(500)).await;
            };
            assert!(valid_tools(&tools), "Unexpected tools: {tools:?}");
            let client = project_gateway::connect_native_http(&endpoint)
                .await
                .unwrap();
            let result=tokio::time::timeout(Duration::from_secs(90),client.call_tool(serde_json::from_value(serde_json::json!({"name":"find_symbol","arguments":{"name_path_pattern":marker,"relative_path":"example.py","include_body":true}})).unwrap())).await.unwrap().unwrap();
            let text = serde_json::to_string(&result).unwrap();
            assert!(
                result.is_error != Some(true),
                "Symbol lookup failed: {text}\n{}",
                String::from_utf8_lossy(
                    &std::fs::read(root.join(format!("{marker}.log"))).unwrap()
                )
            );
            assert!(text.contains(marker), "Missing expected symbol: {text}");
            let _ = client.cancel().await;
            endpoints.push(endpoint);
        }
        instances[0].stop().await.unwrap();
        assert!(project_gateway::probe_native_http(&endpoints[0])
            .await
            .is_err());
        assert!(valid_tools(
            &project_gateway::probe_native_http(&endpoints[1])
                .await
                .unwrap()
        ));
        instances[1].stop().await.unwrap();
        assert!(project_gateway::probe_native_http(&endpoints[1])
            .await
            .is_err());
    }
    #[test]
    fn rejects_project_switching_and_unrelated_servers() {
        assert!(valid_tools(&[
            "find_symbol".into(),
            "get_symbols_overview".into()
        ]));
        assert!(!valid_tools(&[
            "find_symbol".into(),
            "activate_project".into()
        ]));
        assert!(!valid_tools(&["unrelated".into()]));
    }
}
