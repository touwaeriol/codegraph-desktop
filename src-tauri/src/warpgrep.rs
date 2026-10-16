use crate::{commands, configuration::Target, models::*, persistence, AppState};
use project_gateway::warpgrep::{
    Morph, MorphError, MorphErrorKind, WarpGrepOptions, WarpGrepRoute,
};
use std::{collections::HashMap, sync::Arc};
use tauri::{Manager, State};

const KEY_SETTING: &str = "morphApiKey";
const VALIDATED_SETTING: &str = "morphKeyValidatedAt";

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
pub struct Instance {
    route: WarpGrepRoute,
    started_at: String,
    tools: Vec<String>,
}
impl Instance {
    fn snapshot(&self) -> Snapshot {
        let alive = self.route.is_alive();
        Snapshot {
            state: if alive { "running" } else { "error" }.into(),
            pid: None,
            endpoint: Some(self.route.endpoint().into()),
            started_at: Some(self.started_at.clone()),
            tools: self.tools.clone(),
            error: (!alive).then(|| {
                AppError::new(
                    "WARPGREP_STOPPED",
                    crate::i18n::tr(
                        "WarpGrep 路由已停止，请重新启动",
                        "The WarpGrep route stopped. Start it again",
                    ),
                )
            }),
        }
    }
}
fn stopped() -> Snapshot {
    Snapshot {
        state: "stopped".into(),
        pid: None,
        endpoint: None,
        started_at: None,
        tools: vec![],
        error: None,
    }
}

pub fn saved_key(db: &rusqlite::Connection) -> Result<Option<String>> {
    persistence::setting(db, KEY_SETTING)
}
/// Settings only ever expose a masked hint, never the key itself.
pub fn key_hint(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 10 {
        return "••••".into();
    }
    format!(
        "{}…{}",
        chars[..5].iter().collect::<String>(),
        chars[chars.len() - 4..].iter().collect::<String>()
    )
}
pub fn validated_at(db: &rusqlite::Connection) -> Result<Option<String>> {
    persistence::setting(db, VALIDATED_SETTING)
}
fn morph_error(error: MorphError) -> AppError {
    let summary = match error.kind {
        MorphErrorKind::InvalidKey => crate::i18n::tr(
            "API Key 格式无效，请粘贴完整的 Morph API Key",
            "The API key format is invalid. Paste the complete Morph API key",
        ),
        MorphErrorKind::Unauthorized => crate::i18n::tr(
            "Morph 拒绝了此 API Key，或该 Key 无权使用 WarpGrep",
            "Morph rejected this API key, or it cannot use WarpGrep",
        ),
        MorphErrorKind::PaymentRequired => crate::i18n::tr(
            "Morph 账户额度不足，请在 Morph 控制台充值后重试",
            "The Morph account has insufficient credit. Add credit in the Morph dashboard and retry",
        ),
        MorphErrorKind::RateLimited => crate::i18n::tr(
            "Morph API 请求过于频繁，请稍后重试",
            "Morph API rate limit reached. Retry later",
        ),
        MorphErrorKind::Unavailable => crate::i18n::tr(
            "Morph API 暂时不可用",
            "The Morph API is temporarily unavailable",
        ),
        MorphErrorKind::Network => crate::i18n::tr(
            "无法连接 Morph API，请检查网络或系统代理",
            "Unable to reach the Morph API. Check the network or system proxy",
        ),
        MorphErrorKind::Protocol => crate::i18n::tr(
            "Morph API 返回了无法识别的响应",
            "The Morph API returned an unexpected response",
        ),
    };
    AppError::new(error.code(), format!("{summary} ({})", error.detail))
}

#[tauri::command]
pub async fn save_morph_api_key(state: State<'_, AppState>, api_key: String) -> Result<Settings> {
    let key = api_key.trim().to_owned();
    // Persist only after a real WarpGrep completion succeeds with this key.
    Morph::new(&key)
        .map_err(morph_error)?
        .validate()
        .await
        .map_err(morph_error)?;
    {
        let mut db = state.db.lock().unwrap();
        let tx = db.transaction()?;
        persistence::set_setting(&tx, KEY_SETTING, &key)?;
        persistence::set_setting(&tx, VALIDATED_SETTING, &now())?;
        tx.commit()?;
    }
    *state.morph_key.write().unwrap() = Some(key);
    commands::settings(&state)
}
#[tauri::command]
pub async fn remove_morph_api_key(state: State<'_, AppState>) -> Result<Settings> {
    let routes = std::mem::take(&mut *state.warpgrep_instances.lock().await);
    for (_, instance) in routes {
        instance.route.stop().await;
    }
    {
        let mut db = state.db.lock().unwrap();
        let tx = db.transaction()?;
        persistence::delete_setting(&tx, KEY_SETTING)?;
        persistence::delete_setting(&tx, VALIDATED_SETTING)?;
        tx.commit()?;
    }
    if let Err(error) = crate::resume::clear_engine(&state.data, "warpgrep") {
        eprintln!("Cannot clear WarpGrep resume state: {}", error.message);
    }
    *state.morph_key.write().unwrap() = None;
    commands::settings(&state)
}

pub fn binding(state: &AppState, id: &str) -> Result<Target> {
    let b = commands::http_binding(state, id)?;
    Ok(Target {
        name: "warpgrep".into(),
        endpoint: format!("http://127.0.0.1:{}/warpgrep/{id}", b.port),
        authorization: Some(b.authorization()),
        stdio: None,
    })
}
pub fn require_key(state: &AppState) -> Result<()> {
    if state.morph_key.read().unwrap().is_none() {
        return Err(AppError::new(
            "MORPH_KEY_REQUIRED",
            crate::i18n::tr(
                "请先在设置中配置并验证 Morph API Key",
                "Configure and validate a Morph API key in Settings first",
            ),
        ));
    }
    Ok(())
}
pub async fn is_running(state: &AppState, id: &str) -> bool {
    state
        .warpgrep_instances
        .lock()
        .await
        .get(id)
        .is_some_and(|i| i.route.is_alive())
}
pub async fn stop(state: &AppState, id: &str) {
    let instance = state.warpgrep_instances.lock().await.remove(id);
    if let Some(instance) = instance {
        instance.route.stop().await;
    }
}
async fn start(app: &tauri::AppHandle, state: &AppState, id: &str) -> Result<()> {
    require_key(state)?;
    let project = state.project(id)?;
    let root = persistence::canonical(&project.canonical_path)?;
    let binding = commands::http_binding(state, id)?;
    let route = {
        let mut host = state.http_host.lock().await;
        if host.is_none() {
            *host = Some(
                project_gateway::SharedGateway::start(binding.port)
                    .await
                    .map_err(|e| AppError::new("PORT_BIND_FAILED", e))?,
            );
        }
        let log = {
            let app = app.clone();
            let id = id.to_owned();
            Arc::new(move |line: String| {
                app.state::<AppState>()
                    .log(&app, &id, "info", "warpgrep", &line)
            })
        };
        host.as_ref()
            .unwrap()
            .register_warpgrep(WarpGrepOptions {
                project_id: id.into(),
                root,
                token: binding.token.clone(),
                key: state.morph_key.clone(),
                log: Some(log),
            })
            .map_err(|e| AppError::new("WARPGREP_START_FAILED", e))?
    };
    // Verify the route end to end with the same credentials clients receive.
    let tools = project_gateway::probe_http(route.endpoint(), &binding.token)
        .await
        .map_err(|e| AppError::new("MCP_HANDSHAKE_FAILED", e))?;
    state.warpgrep_instances.lock().await.insert(
        id.into(),
        Instance {
            route,
            started_at: now(),
            tools,
        },
    );
    state.log(
        app,
        id,
        "info",
        "warpgrep",
        crate::i18n::tr(
            "WarpGrep 路由已启动，MCP 工具列表验证通过",
            "WarpGrep route started and its MCP tool list was verified",
        ),
    );
    Ok(())
}
#[tauri::command]
pub async fn warpgrep_operation(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    action: String,
) -> Result<Snapshot> {
    if !["start", "stop", "restart"].contains(&action.as_str()) {
        return Err(AppError::new("INVALID_ACTION", action));
    }
    if state.quitting.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(AppError::new("APP_EXITING", "应用正在退出"));
    }
    state.project(&project_id)?;
    if action == "start" && is_running(&state, &project_id).await {
        state.remember_engine(&project_id, "warpgrep", true);
        return get_warpgrep_snapshot(state, project_id).await;
    }
    stop(&state, &project_id).await;
    if action != "stop" {
        if let Err(error) = start(&app, &state, &project_id).await {
            state.remember_engine(&project_id, "warpgrep", false);
            state.log(&app, &project_id, "error", "warpgrep", &error.message);
            return Err(error);
        }
    }
    state.remember_engine(&project_id, "warpgrep", action != "stop");
    get_warpgrep_snapshot(state, project_id).await
}
#[tauri::command]
pub async fn get_warpgrep_snapshot(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Snapshot> {
    state.project(&project_id)?;
    Ok(state
        .warpgrep_instances
        .lock()
        .await
        .get(&project_id)
        .map(Instance::snapshot)
        .unwrap_or_else(stopped))
}
#[tauri::command]
pub async fn list_warpgrep_snapshots(
    state: State<'_, AppState>,
) -> Result<HashMap<String, Snapshot>> {
    Ok(state
        .warpgrep_instances
        .lock()
        .await
        .iter()
        .map(|(id, instance)| (id.clone(), instance.snapshot()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_hint_never_reveals_the_secret() {
        let key = "sk-abcdefghijklmnopqrstuvwxyz0123";
        let hint = key_hint(key);
        assert_eq!(hint, "sk-ab…0123");
        assert!(!hint.contains("ghijklmnop"));
        assert_eq!(key_hint("short-key"), "••••");
    }
}
