use crate::{commands, models::*, AppState};
use std::{
    path::Path,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, BufReader};

pub async fn status(state: &AppState, p: &Project) -> Result<String> {
    state
        .snapshots
        .lock()
        .unwrap()
        .entry(p.id.clone())
        .or_insert_with(|| RuntimeSnapshot::new(&p.id))
        .index_stats = None;
    if !Path::new(&p.canonical_path).is_dir() {
        return Err(AppError::new("PROJECT_MISSING", "项目目录不存在"));
    }
    if !Path::new(&p.canonical_path).join(".codegraph").exists() {
        return Ok("missing".into());
    }
    let (_, entry) = commands::entry(state)?;
    let mut cmd = tokio::process::Command::new(entry.program);
    cmd.args(entry.prefix_args)
        .args(["status", "--json"])
        .arg(&p.canonical_path)
        .env("CODEGRAPH_NO_DAEMON", "1")
        .current_dir(&p.canonical_path)
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let (child, job) = project_gateway::spawn_owned(&mut cmd)
        .map_err(|e| AppError::new("PROCESS_JOB_FAILED", e))?;
    let output = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output()).await;
    job.terminate_and_wait()
        .await
        .map_err(|e| AppError::new("PROCESS_STOP_FAILED", e))?;
    let out = output.map_err(|_| AppError::new("INDEX_STATUS_TIMEOUT", "读取索引超时"))??;
    if !out.status.success() {
        return Err(AppError::new(
            "INDEX_STATUS_FAILED",
            String::from_utf8_lossy(&out.stderr),
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| AppError::new("INDEX_STATUS_FAILED", format!("索引状态不是有效 JSON：{e}")))?;
    if value.get("initialized") == Some(&serde_json::Value::Bool(false)) {
        return Ok("missing".into());
    }
    let bound = value
        .get("projectPath")
        .and_then(|v| v.as_str())
        .and_then(|s| std::fs::canonicalize(s).ok());
    if value.get("initialized").and_then(|v| v.as_bool()) != Some(true)
        || value.pointer("/index/state").and_then(|v| v.as_str()) != Some("complete")
        || bound != std::fs::canonicalize(&p.canonical_path).ok()
    {
        return Err(AppError::new(
            "INDEX_NOT_READY",
            "索引未完成，或索引项目路径不匹配",
        ));
    }
    {
        let mut snapshots = state.snapshots.lock().unwrap();
        let s = snapshots
            .entry(p.id.clone())
            .or_insert_with(|| RuntimeSnapshot::new(&p.id));
        s.index_stats = Some(
            serde_json::json!({"fileCount":value.get("fileCount"),"nodeCount":value.get("nodeCount"),"edgeCount":value.get("edgeCount"),"checkedAt":now()}),
        );
    }
    Ok("ready".into())
}
pub async fn start(app: &tauri::AppHandle, state: &AppState, id: &str) -> Result<()> {
    let binding = commands::http_binding(state, id)?;
    if let Some(g) = state.gateways.lock().await.get(id) {
        if g.port() != binding.port || g.token() != binding.token {
            return Err(AppError::new(
                "HTTP_RESTART_REQUIRED",
                "HTTP 配置已更新，请重启项目实例使固定地址生效",
            ));
        }
        return if g.is_alive() {
            Ok(())
        } else {
            Err(AppError::new("UPSTREAM_EXITED", "项目会话已退出，请重启"))
        };
    }
    let p = state.project(id)?;
    let index = status(state, &p).await?;
    if index != "ready" {
        let mut s = state.snapshot(id);
        s.index_state = index;
        state.publish(app, s);
        return Err(AppError::new("INDEX_REQUIRED", "请先初始化项目索引"));
    }
    let (_, entry) = commands::entry(state)?;
    let mut s = state.snapshot(id);
    s.state = "starting".into();
    s.error = None;
    s.index_state = "ready".into();
    state.publish(app, s.clone());
    let gateway = project_gateway::Gateway::start(project_gateway::GatewayOptions {
        project_id: id.into(),
        root: p.canonical_path.into(),
        entry,
        preferred_port: Some(binding.port),
        persistent_token: Some(binding.token),
    })
    .await
    .map_err(|e| {
        let port_busy = e.to_string().starts_with("PORT_BIND_FAILED")
            || e.chain().any(|cause| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::AddrInUse)
            });
        AppError::new(
            if port_busy {
                "PORT_BIND_FAILED"
            } else {
                "MCP_HANDSHAKE_FAILED"
            },
            e,
        )
    })?;
    let record = project_protocol::RuntimeRecord {
        project_id: id.into(),
        generation: gateway.generation().into(),
        owner_pid: std::process::id(),
        endpoint: format!("http://127.0.0.1:{}/mcp", gateway.port()),
        token: gateway.token().into(),
        started_at: now(),
    };
    if let Err(e) = project_protocol::write_runtime(&record) {
        let _ = gateway.stop().await;
        return Err(AppError::new("RUNTIME_RECORD_FAILED", e));
    }
    s.state = "running".into();
    s.generation = gateway.generation().into();
    s.pid = Some(gateway.pid());
    s.port = Some(gateway.port());
    s.started_at = Some(record.started_at);
    s.sessions = gateway.session_count();
    state.gateways.lock().await.insert(id.into(), gateway);
    state.publish(app, s);
    state.log(
        app,
        id,
        "info",
        "runtime",
        "CodeGraph MCP 握手与工具列表验证通过",
    );
    Ok(())
}
pub async fn stop(app: &tauri::AppHandle, state: &AppState, id: &str) -> Result<()> {
    let gateway = state.gateways.lock().await.remove(id);
    if let Some(g) = gateway {
        let revoke = project_protocol::remove_runtime(id)
            .map_err(|e| AppError::new("RUNTIME_RECORD_FAILED", e));
        let mut s = state.snapshot(id);
        s.state = "stopping".into();
        state.publish(app, s.clone());
        g.stop()
            .await
            .map_err(|e| AppError::new("PROCESS_STOP_FAILED", e))?;
        revoke?;
        s.state = "stopped".into();
        s.pid = None;
        s.sessions = 0;
        s.started_at = None;
        s.error = None;
        state.publish(app, s);
        state.log(app, id, "info", "runtime", "项目网关及受管进程已停止");
    } else {
        project_protocol::remove_runtime(id)
            .map_err(|e| AppError::new("RUNTIME_RECORD_FAILED", e))?;
        let mut s = state.snapshot(id);
        s.state = "stopped".into();
        s.pid = None;
        s.sessions = 0;
        s.started_at = None;
        s.error = None;
        state.publish(app, s);
    }
    Ok(())
}
async fn index(
    app: &tauri::AppHandle,
    state: &AppState,
    id: &str,
    kind: &str,
    cancel: &AtomicBool,
) -> Result<()> {
    struct IndexPermit<'a>(&'a std::sync::atomic::AtomicUsize);
    impl Drop for IndexPermit<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(AppError::new("TASK_CANCELLED", "排队任务已取消"));
        }
        let limit = commands::settings(state)?.index_concurrency;
        let count = state.active_indexes.load(Ordering::SeqCst);
        if count < limit
            && state
                .active_indexes
                .compare_exchange(count, count + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _permit = IndexPermit(&state.active_indexes);
    let p = state.project(id)?;
    let was_running = state.gateways.lock().await.contains_key(id);
    stop(app, state, id).await?;
    let (_, entry) = commands::entry(state)?;
    let mut command = tokio::process::Command::new(entry.program);
    command.args(entry.prefix_args);
    match kind {
        "init" => {
            command.args(["init", "--yes"]);
        }
        "sync" => {
            command.arg("sync");
        }
        "rebuild" => {
            command.arg("index");
        }
        _ => return Err(AppError::new("INVALID_TASK", "未知任务")),
    };
    command
        .arg(&p.canonical_path)
        .current_dir(&p.canonical_path)
        .env("CODEGRAPH_NO_DAEMON", "1")
        .env("CODEGRAPH_HOST_PPID", std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let (mut child, job) = project_gateway::spawn_owned(&mut command)
        .map_err(|e| AppError::new("PROCESS_JOB_FAILED", e))?;
    let mut out = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut err = BufReader::new(child.stderr.take().unwrap()).lines();
    let mut out_open = true;
    let mut err_open = true;
    let mut s = state.snapshot(id);
    s.index_state = "indexing".into();
    s.error = None;
    state.publish(app, s.clone());
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    let result = loop {
        tokio::select! {
            line=out.next_line(),if out_open => {
                match line {Ok(Some(line))=>state.log(app,id,"info","index",&line),_=>out_open=false}
            },
            line=err.next_line(),if err_open => {
                match line {Ok(Some(line))=>state.log(app,id,"warn","index",&line),_=>err_open=false}
            },
            _=ticker.tick() => {
                if cancel.load(Ordering::SeqCst) {
                    break Err(AppError::new("TASK_CANCELLED","任务已取消，索引状态需重新检查"));
                }
                if let Some(exit)=child.try_wait()? {
                    break if exit.success(){Ok(())}else{Err(AppError::new("INDEX_FAILED",format!("索引命令退出：{exit}")))};
                }
            }
        }
    };
    job.terminate_and_wait()
        .await
        .map_err(|e| AppError::new("PROCESS_STOP_FAILED", e))?;
    drop(job);
    if child.try_wait()?.is_none() {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
    if state.quitting.load(Ordering::SeqCst) {
        s.index_state = "unknown".into();
        s.error = Some(AppError::new(
            "TASK_CANCELLED",
            "应用正在退出；索引状态将在下次启动时重新检查",
        ));
        state.publish(app, s);
        return Err(AppError::new("TASK_CANCELLED", "退出前已停止索引任务"));
    }
    let verification = status(state, &p).await;
    s.index_state = verification
        .as_ref()
        .cloned()
        .unwrap_or_else(|_| "error".into());
    let result = result.and_then(|_| match verification {
        Ok(v) if v == "ready" => Ok(()),
        Ok(_) => Err(AppError::new(
            "INDEX_NOT_READY",
            "CLI 已结束，但索引尚未就绪",
        )),
        Err(e) => Err(e),
    });
    s.index_stats = state.snapshot(id).index_stats;
    s.error = result.as_ref().err().cloned();
    state.publish(app, s);
    result?;
    if was_running && !cancel.load(Ordering::SeqCst) && !state.quitting.load(Ordering::SeqCst) {
        start(app, state, id).await?;
    }
    Ok(())
}
async fn cancellable_start(
    app: &tauri::AppHandle,
    state: &AppState,
    id: &str,
    cancel: &AtomicBool,
) -> Result<()> {
    tokio::select! {result=start(app,state,id)=>result,_=async {while !cancel.load(Ordering::SeqCst){tokio::time::sleep(Duration::from_millis(100)).await;}}=>Err(AppError::new("TASK_CANCELLED","启动已取消"))}
}
pub fn launch_operation(app: tauri::AppHandle, id: String, kind: String) -> Result<String> {
    let state = app.state::<AppState>();
    if state.quitting.load(Ordering::SeqCst) {
        return Err(AppError::new("APP_EXITING", "应用正在退出"));
    }
    state.project(&id)?;
    let op = uuid::Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let registry = state.tasks.clone();
    let operation = op.clone();
    registry.spawn(op.clone(), cancel.clone(), async move {
        let state = app.state::<AppState>();
        let lock = state.operation_lock(&id);
        let _guard = lock.lock().await;
        state.task_event(&app, &id, &operation, &kind, "running", None);
        let result = if cancel.load(Ordering::SeqCst) {
            Err(AppError::new("TASK_CANCELLED", "任务已取消"))
        } else {
            match kind.as_str() {
                "start" => cancellable_start(&app, &state, &id, &cancel).await,
                "stop" => stop(&app, &state, &id).await,
                "restart" => match stop(&app, &state, &id).await {
                    Ok(()) => cancellable_start(&app, &state, &id, &cancel).await,
                    Err(e) => Err(e),
                },
                _ => index(&app, &state, &id, &kind, &cancel).await,
            }
        };
        if let Err(ref error) = result {
            let mut s = state.snapshot(&id);
            if ["start", "stop", "restart"].contains(&kind.as_str()) {
                s.state = "error".into();
            }
            s.error = Some(error.clone());
            state.publish(&app, s);
            state.log(&app, &id, "error", &kind, &error.message);
        }
        let final_state = if result.is_ok() {
            "completed"
        } else if result
            .as_ref()
            .err()
            .is_some_and(|e| e.code == "TASK_CANCELLED")
        {
            "cancelled"
        } else {
            "failed"
        };
        state.task_event(&app, &id, &operation, &kind, final_state, result.err());
    })?;
    Ok(op)
}
