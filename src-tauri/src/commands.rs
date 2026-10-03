use crate::{configuration, models::*, persistence, AppState};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tauri::State;
#[tauri::command]
pub fn list_projects(state: State<AppState>) -> Result<Vec<Project>> {
    persistence::list(&state.db.lock().unwrap())
}
#[tauri::command]
pub fn add_project(
    state: State<AppState>,
    path: String,
    name: String,
    notes: Option<String>,
    auto_start: Option<bool>,
) -> Result<Project> {
    let canonical = persistence::canonical(&path)?;
    if let Some(existing) = persistence::list(&state.db.lock().unwrap())?
        .into_iter()
        .find(|p| persistence::canonical(&p.canonical_path).ok().as_ref() == Some(&canonical))
    {
        let mut error = AppError::new("PROJECT_DUPLICATE", "此目录已添加");
        error.existing_project_id = Some(existing.id);
        return Err(error);
    }
    let name = if name.trim().is_empty() {
        canonical
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into()
    } else {
        name.trim().into()
    };
    let p = Project {
        previous_roots: vec![],
        id: uuid::Uuid::new_v4().to_string(),
        name,
        root_path: persistence::display(&canonical),
        canonical_path: canonical.to_string_lossy().into(),
        notes: notes.unwrap_or_default(),
        auto_start: auto_start.unwrap_or(false),
        created_at: now(),
        updated_at: now(),
    };
    persistence::save(&state.db.lock().unwrap(), &p)?;
    Ok(p)
}
#[tauri::command]
pub fn update_project(
    state: State<AppState>,
    project_id: String,
    name: String,
    notes: String,
    auto_start: bool,
) -> Result<Project> {
    if name.trim().is_empty() {
        return Err(AppError::new("INVALID_NAME", "项目名称不能为空"));
    }
    let mut p = state.project(&project_id)?;
    p.name = name.trim().into();
    p.notes = notes;
    p.auto_start = auto_start;
    p.updated_at = now();
    persistence::save(&state.db.lock().unwrap(), &p)?;
    Ok(p)
}
#[tauri::command]
pub async fn relocate_project(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    selected_path: String,
) -> Result<Project> {
    let lock = state.operation_lock(&project_id);
    let _guard = lock.lock().await;
    let mut p = state.project(&project_id)?;
    let canonical = persistence::canonical(&selected_path)?;
    crate::runtime::stop(&app, &state, &project_id).await?;
    if p.canonical_path != canonical.to_string_lossy()
        && !p.previous_roots.contains(&p.canonical_path)
    {
        p.previous_roots.push(p.canonical_path.clone());
    }
    p.canonical_path = canonical.to_string_lossy().into();
    p.root_path = persistence::display(&canonical);
    p.updated_at = now();
    persistence::save(&state.db.lock().unwrap(), &p)?;
    state.publish(&app, RuntimeSnapshot::new(&project_id));
    Ok(p)
}
#[tauri::command]
pub async fn remove_project(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<()> {
    let lock = state.operation_lock(&project_id);
    let _guard = lock.lock().await;
    state.project(&project_id)?;
    crate::runtime::stop(&app, &state, &project_id).await?;
    state
        .db
        .lock()
        .unwrap()
        .execute("DELETE FROM projects WHERE id=?1", [&project_id])?;
    state.snapshots.lock().unwrap().remove(&project_id);
    Ok(())
}
#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<Settings> {
    settings(&state)
}
pub fn settings(state: &AppState) -> Result<Settings> {
    let db = state.db.lock().unwrap();
    Ok(Settings {
        codegraph_entry: persistence::setting(&db, "codegraphEntry")?,
        index_concurrency: persistence::setting(&db, "indexConcurrency")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(2),
        close_behavior: persistence::setting(&db, "closeBehavior")?
            .unwrap_or_else(|| "tray".into()),
        app_data_dir: state.data.to_string_lossy().into(),
    })
}
#[tauri::command]
pub fn save_settings(
    state: State<AppState>,
    index_concurrency: usize,
    close_behavior: String,
) -> Result<Settings> {
    if !(1..=4).contains(&index_concurrency) || !["tray", "exit"].contains(&close_behavior.as_str())
    {
        return Err(AppError::new("INVALID_SETTINGS", "设置值超出允许范围"));
    }
    {
        let db = state.db.lock().unwrap();
        persistence::set_setting(&db, "indexConcurrency", &index_concurrency.to_string())?;
        persistence::set_setting(&db, "closeBehavior", &close_behavior)?;
    }
    settings(&state)
}
pub fn entry(state: &AppState) -> Result<(PathBuf, project_gateway::CodeGraphEntry)> {
    let selected = settings(state)?
        .codegraph_entry
        .map(PathBuf::from)
        .or_else(|| which::which("codegraph").ok())
        .ok_or_else(|| AppError::new("CLI_NOT_FOUND", "未找到 CodeGraph，请选择安装入口"))?;
    let parsed =
        project_gateway::resolve_entry(&selected).map_err(|e| AppError::new("CLI_NOT_FOUND", e))?;
    Ok((selected, parsed))
}
async fn detect(state: &AppState) -> Environment {
    let (selected, entry) = match entry(state) {
        Ok(e) => e,
        Err(e) => {
            return Environment {
                available: false,
                entry: None,
                version: None,
                error: Some(e.message),
            }
        }
    };
    detect_candidate(selected, entry).await
}
async fn detect_candidate(
    selected: PathBuf,
    entry: project_gateway::CodeGraphEntry,
) -> Environment {
    let mut command = tokio::process::Command::new(&entry.program);
    command
        .args(&entry.prefix_args)
        .arg("--version")
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    match tokio::time::timeout(std::time::Duration::from_secs(15), command.output()).await {
        Ok(Ok(o)) if o.status.success() => {
            let version = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let mut help = tokio::process::Command::new(&entry.program);
            help.args(&entry.prefix_args)
                .args(["help", "serve"])
                .kill_on_drop(true);
            #[cfg(windows)]
            help.creation_flags(0x08000000);
            let capability =
                tokio::time::timeout(std::time::Duration::from_secs(15), help.output())
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .is_some_and(|out| {
                        let text = String::from_utf8_lossy(&out.stdout);
                        out.status.success() && text.contains("--mcp") && text.contains("--path")
                    });
            Environment {
                available: capability,
                entry: Some(selected.to_string_lossy().into()),
                version: Some(version.clone()),
                error: if !capability {
                    Some("入口不支持 CodeGraph serve --mcp --path".into())
                } else if version != "1.6.2" {
                    Some(format!(
                        "版本 {version} 尚未通过兼容性验证；当前基线为 1.6.2"
                    ))
                } else {
                    None
                },
            }
        }
        other => Environment {
            available: false,
            entry: Some(selected.to_string_lossy().into()),
            version: None,
            error: Some(format!("CodeGraph 版本检测失败：{other:?}")),
        },
    }
}
#[tauri::command]
pub async fn detect_codegraph(state: State<'_, AppState>) -> Result<Environment> {
    Ok(detect(&state).await)
}
#[tauri::command]
pub async fn set_codegraph_entry(
    state: State<'_, AppState>,
    selected_path: String,
) -> Result<Environment> {
    let parsed = project_gateway::resolve_entry(Path::new(&selected_path))
        .map_err(|e| AppError::new("CLI_NOT_FOUND", e))?;
    let environment = detect_candidate(PathBuf::from(&selected_path), parsed).await;
    if !environment.available {
        return Err(AppError::new(
            "CLI_INVALID",
            environment
                .error
                .unwrap_or_else(|| "CodeGraph 检测失败".into()),
        ));
    }
    persistence::set_setting(&state.db.lock().unwrap(), "codegraphEntry", &selected_path)?;
    Ok(environment)
}
#[tauri::command]
pub async fn get_project_snapshot(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<RuntimeSnapshot> {
    let p = state.project(&project_id)?;
    let mut s = state.snapshot(&project_id);
    if s.index_state == "unknown" {
        let lock = state.operation_lock(&project_id);
        if let Ok(_guard) = lock.try_lock() {
            match crate::runtime::status(&state, &p).await {
                Ok(index) => s.index_state = index,
                Err(e) => {
                    s.index_state = "error".into();
                    s.error = Some(e);
                }
            }
            s.index_stats = state.snapshot(&project_id).index_stats;
            state.publish(&app, s.clone());
            s = state.snapshot(&project_id);
        };
    }
    if let Some(g) = state.gateways.lock().await.get(&project_id) {
        s.sessions = g.session_count();
        if !g.is_alive() {
            s.state = "error".into();
            s.error = Some(AppError::new(
                "UPSTREAM_EXITED",
                "CodeGraph 会话已退出，请重新启动",
            ));
        }
    }
    Ok(s)
}
#[tauri::command]
pub fn start_project(app: tauri::AppHandle, project_id: String) -> Result<String> {
    crate::runtime::launch_operation(app, project_id, "start".into())
}
#[tauri::command]
pub fn stop_project(app: tauri::AppHandle, project_id: String) -> Result<String> {
    crate::runtime::launch_operation(app, project_id, "stop".into())
}
#[tauri::command]
pub fn restart_project(app: tauri::AppHandle, project_id: String) -> Result<String> {
    crate::runtime::launch_operation(app, project_id, "restart".into())
}
#[tauri::command]
pub fn run_index_task(app: tauri::AppHandle, project_id: String, kind: String) -> Result<String> {
    if !["init", "sync", "rebuild"].contains(&kind.as_str()) {
        return Err(AppError::new("INVALID_TASK", "不支持的索引操作"));
    }
    crate::runtime::launch_operation(app, project_id, kind)
}
#[tauri::command]
pub fn cancel_task(state: State<AppState>, operation_id: String) -> Result<()> {
    state.tasks.cancel(&operation_id)
}
#[tauri::command]
pub fn read_logs(
    state: State<AppState>,
    project_id: String,
    cursor: Option<u64>,
    limit: Option<usize>,
) -> Result<Vec<LogEntry>> {
    state.project(&project_id)?;
    Ok(state
        .logs
        .lock()
        .unwrap()
        .get(&project_id)
        .map(|q| {
            q.iter()
                .filter(|e| e.sequence > cursor.unwrap_or(0))
                .rev()
                .take(limit.unwrap_or(500).min(2000))
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        })
        .unwrap_or_default())
}
pub fn http_binding(state: &AppState, project_id: &str) -> Result<persistence::HttpBinding> {
    persistence::ensure_http_binding(&mut state.db.lock().unwrap(), project_id)
}
#[tauri::command]
pub fn preview_client_config(
    state: State<AppState>,
    project_id: String,
    clients: Vec<String>,
    action: String,
    overrides: Option<Vec<configuration::ConfigOverride>>,
) -> Result<configuration::ConfigPreview> {
    let p = state.project(&project_id)?;
    let managed = {
        let db = state.db.lock().unwrap();
        let mut stmt =
            db.prepare("SELECT client,fingerprint FROM managed_config WHERE project_id=?1")?;
        let rows = stmt
            .query_map([&project_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<HashMap<_, _>, _>>()?;
        rows
    };
    let preview = configuration::preview_with_overrides(
        &p,
        &clients,
        &action,
        &http_binding(&state, &project_id)?,
        &managed,
        overrides.as_deref().unwrap_or(&[]),
    )?;
    state
        .previews
        .lock()
        .unwrap()
        .insert(preview.preview_id.clone(), preview.clone());
    Ok(preview)
}
#[tauri::command]
pub async fn apply_client_config(
    state: State<'_, AppState>,
    preview_id: String,
) -> Result<configuration::ApplyResult> {
    let preview = state
        .previews
        .lock()
        .unwrap()
        .get(&preview_id)
        .cloned()
        .ok_or_else(|| AppError::new("PREVIEW_EXPIRED", "预览已失效，请重新预览"))?;
    let lock = state.operation_lock(&preview.project_id);
    let _guard = lock.lock().await;
    if !state.previews.lock().unwrap().contains_key(&preview_id) {
        return Err(AppError::new("PREVIEW_EXPIRED", "该预览已应用，请重新预览"));
    }
    let p = state.project(&preview.project_id)?;
    let root = preview
        .validated_root
        .as_deref()
        .unwrap_or(&p.canonical_path);
    if root != p.canonical_path && !p.previous_roots.iter().any(|r| r == root) {
        return Err(AppError::new(
            "PREVIEW_EXPIRED",
            "项目目录记录已变化，请重新预览",
        ));
    }
    let result = configuration::apply(&preview, Path::new(root), &state.data)?;
    let succeeded = result
        .files
        .iter()
        .all(|f| f.status == "success" || f.status == "unchanged");
    if preview.validated_root.is_none() && succeeded {
        let mut db = state.db.lock().unwrap();
        let tx = db.transaction()?;
        for f in &preview.files {
            if let Some(fp) = &f.fingerprint {
                tx.execute("INSERT INTO managed_config(project_id,client,fingerprint) VALUES(?1,?2,?3) ON CONFLICT(project_id,client) DO UPDATE SET fingerprint=excluded.fingerprint",rusqlite::params![p.id,f.client,fp])?;
            } else {
                tx.execute(
                    "DELETE FROM managed_config WHERE project_id=?1 AND client=?2",
                    rusqlite::params![p.id, f.client],
                )?;
            }
        }
        tx.commit()?;
    }
    if succeeded {
        state.previews.lock().unwrap().remove(&preview_id);
    }
    Ok(result)
}
#[tauri::command]
pub async fn get_client_config_status(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<configuration::ConfigStatus>> {
    let binding = http_binding(&state, &project_id)?;
    let mut statuses = configuration::status(&state.project(&project_id)?, &binding);
    if state
        .gateways
        .lock()
        .await
        .get(&project_id)
        .is_some_and(|g| g.port() != binding.port || g.token() != binding.token)
    {
        for status in &mut statuses {
            if status.state == "configured" {
                status.state = "repair".into();
                status.message = Some("请重启项目实例使固定 HTTP 配置生效".into());
            }
        }
    }
    Ok(statuses)
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    success: bool,
    tools: Vec<String>,
    message: String,
    checked_at: String,
}
#[tauri::command]
pub async fn test_project_mcp(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<ProbeResult> {
    state.project(&project_id)?;
    let record = project_protocol::read_runtime(&project_id)
        .map_err(|e| AppError::new("SERVICE_NOT_RUNNING", e))?;
    let tools = project_gateway::probe(&record)
        .await
        .map_err(|e| AppError::new("MCP_HANDSHAKE_FAILED", e))?;
    Ok(ProbeResult {
        success: true,
        tools,
        message: "网关 MCP 初始化及工具列表通过；真实客户端连接需在客户端确认。".into(),
        checked_at: now(),
    })
}
#[tauri::command]
pub fn get_project_tasks(state: State<AppState>, project_id: String) -> Result<Vec<TaskProgress>> {
    state.project(&project_id)?;
    Ok(state
        .task_history
        .lock()
        .unwrap()
        .values()
        .filter(|v| v.project_id == project_id)
        .cloned()
        .collect())
}
#[tauri::command]
pub fn list_config_backups(
    state: State<AppState>,
    project_id: String,
) -> Result<Vec<configuration::BackupSummary>> {
    state.project(&project_id)?;
    configuration::list_backups(&state.data, &project_id)
}
#[tauri::command]
pub fn preview_restore_backup(
    state: State<AppState>,
    operation_id: String,
) -> Result<configuration::ConfigPreview> {
    let projects = persistence::list(&state.db.lock().unwrap())?;
    for p in projects {
        if configuration::list_backups(&state.data, &p.id)?
            .iter()
            .any(|b| b.operation_id == operation_id)
        {
            let preview = configuration::preview_restore(
                &state.data,
                &operation_id,
                &p,
                &http_binding(&state, &p.id)?,
            )?;
            state
                .previews
                .lock()
                .unwrap()
                .insert(preview.preview_id.clone(), preview.clone());
            return Ok(preview);
        }
    }
    Err(AppError::new("BACKUP_NOT_FOUND", "找不到此备份所属项目"))
}
fn open_directory(path: &Path) -> Result<()> {
    if !path.is_dir() {
        return Err(AppError::new("PROJECT_MISSING", "目录不存在"));
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(path)
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}
#[tauri::command]
pub fn open_project_directory(state: State<AppState>, project_id: String) -> Result<()> {
    open_directory(Path::new(&state.project(&project_id)?.root_path))
}
#[tauri::command]
pub fn open_app_data_directory(state: State<AppState>) -> Result<()> {
    open_directory(&state.data)
}
#[tauri::command]
pub fn export_project_logs(state: State<AppState>, project_id: String) -> Result<String> {
    state.project(&project_id)?;
    let directory = state.data.join("exports");
    configuration::private_dir(&directory)?;
    let path = directory.join(format!("{}-{}.jsonl", project_id, uuid::Uuid::new_v4()));
    let logs = state.logs.lock().unwrap();
    let contents = logs
        .get(&project_id)
        .map(|q| {
            q.iter()
                .map(|v| serde_json::to_string(v).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    configuration::atomic_write(&path, contents.as_bytes())?;
    Ok(path.to_string_lossy().into())
}

#[tauri::command]
pub fn preview_previous_config(
    state: State<AppState>,
    project_id: String,
    previous_root: String,
) -> Result<configuration::ConfigPreview> {
    let mut p = state.project(&project_id)?;
    if !p.previous_roots.contains(&previous_root) || p.canonical_path == previous_root {
        return Err(AppError::new(
            "PROJECT_SCOPE_VIOLATION",
            "仅允许清理已登记的旧目录",
        ));
    }
    let managed = {
        let db = state.db.lock().unwrap();
        let mut stmt =
            db.prepare("SELECT client,fingerprint FROM managed_config WHERE project_id=?1")?;
        let rows = stmt
            .query_map([&project_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<HashMap<_, _>, _>>()?;
        rows
    };
    p.canonical_path = previous_root.clone();
    let mut preview = configuration::preview(
        &p,
        &["codex".into(), "claude".into()],
        "remove",
        &http_binding(&state, &project_id)?,
        &managed,
    )?;
    preview.validated_root = Some(previous_root);
    state
        .previews
        .lock()
        .unwrap()
        .insert(preview.preview_id.clone(), preview.clone());
    Ok(preview)
}
#[tauri::command]
pub async fn refresh_index_status(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<RuntimeSnapshot> {
    let lock = state.operation_lock(&project_id);
    let _guard = lock.lock().await;
    let p = state.project(&project_id)?;
    let mut s = state.snapshot(&project_id);
    match crate::runtime::status(&state, &p).await {
        Ok(index) => {
            s.index_state = index;
            s.error = None;
        }
        Err(e) => {
            s.index_state = "error".into();
            s.error = Some(e);
        }
    }
    s.index_stats = state.snapshot(&project_id).index_stats;
    state.publish(&app, s);
    Ok(state.snapshot(&project_id))
}
