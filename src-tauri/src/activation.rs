//! One-click activation: start an engine for a project and register it with
//! the active client, or stop it and remove that registration. REA has no
//! managed process, so only its registration changes.
use crate::{commands, i18n::tr, models::*, AppState};
use std::{sync::atomic::Ordering, time::Duration};
use tauri::Manager;

/// Error code for a missing prerequisite the user configures in Settings.
pub const ENVIRONMENT_REQUIRED: &str = "ENVIRONMENT_REQUIRED";

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activation {
    client: String,
    warnings: Vec<String>,
}

#[tauri::command]
pub async fn set_engine_enabled(
    app: tauri::AppHandle,
    project_id: String,
    engine: String,
    enabled: bool,
) -> Result<Activation> {
    let state = app.state::<AppState>();
    if !["codegraph", "serena", "warpgrep", "rea"].contains(&engine.as_str()) {
        return Err(AppError::new(
            "INVALID_ENGINE",
            tr("不支持的引擎", "Unknown engine"),
        ));
    }
    state.project(&project_id)?;
    let client = commands::settings(&state)?.active_client;
    let mut warnings = vec![];
    if enabled {
        warnings = prepare(&state, &engine).await?;
        operate(&app, &project_id, &engine, "start").await?;
        configure(&state, &project_id, &client, &engine, true).await?;
    } else {
        operate(&app, &project_id, &engine, "stop").await?;
        configure(&state, &project_id, &client, &engine, false).await?;
    }
    Ok(Activation { client, warnings })
}

fn required(summary: &str, detail: &str) -> AppError {
    AppError::new(
        ENVIRONMENT_REQUIRED,
        if detail.is_empty() {
            summary.to_owned()
        } else {
            format!("{summary} ({detail})")
        },
    )
}

/// Checks what the engine needs before anything starts, so a missing
/// prerequisite is reported as such instead of as a failed start.
async fn prepare(state: &AppState, engine: &str) -> Result<Vec<String>> {
    match engine {
        "codegraph" => commands::entry(state).map(|_| vec![]).map_err(|e| {
            required(
                tr(
                    "未找到可用的 CodeGraph，请在设置中配置入口",
                    "No usable CodeGraph found. Configure its entry in Settings",
                ),
                &e.message,
            )
        }),
        "serena" => crate::serena::ensure_entry(state)
            .await
            .map(|_| vec![])
            .map_err(|e| {
                required(
                    tr(
                        "未找到可用的 Serena，请在设置中检测入口",
                        "No usable Serena found. Detect its entry in Settings",
                    ),
                    &e.message,
                )
            }),
        "warpgrep" => crate::warpgrep::require_key(state)
            .map(|_| vec![])
            .map_err(|_| {
                required(
                    tr(
                        "WarpGrep 需要 Morph API Key，请在设置中配置并验证",
                        "WarpGrep needs a Morph API key. Configure and validate it in Settings",
                    ),
                    "",
                )
            }),
        _ => crate::rea::check_runtime().await,
    }
}

async fn operate(app: &tauri::AppHandle, id: &str, engine: &str, action: &str) -> Result<()> {
    let state = app.state::<AppState>();
    match engine {
        "codegraph" => {
            let operation =
                crate::runtime::launch_operation(app.clone(), id.into(), action.into())?;
            settle(&state, &operation).await
        }
        "serena" => {
            let operation = crate::serena::serena_operation(app.clone(), id.into(), action.into())?;
            settle(&state, &operation).await
        }
        "warpgrep" => {
            crate::warpgrep::warpgrep_operation(app.clone(), app.state(), id.into(), action.into())
                .await?;
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Waits for a task from the task registry to finish and returns its outcome.
async fn settle(state: &AppState, operation: &str) -> Result<()> {
    loop {
        if state.quitting.load(Ordering::SeqCst) {
            return Err(AppError::new("APP_EXITING", "应用正在退出"));
        }
        let task = state.task_history.lock().unwrap().get(operation).cloned();
        if let Some(result) = task.as_ref().and_then(outcome) {
            return result;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}
fn outcome(task: &TaskProgress) -> Option<Result<()>> {
    match task.state.as_str() {
        "running" => None,
        "completed" => Some(Ok(())),
        _ => Some(Err(task.error.clone().unwrap_or_else(|| {
            AppError::new(
                "TASK_FAILED",
                tr("操作未完成", "The operation did not complete"),
            )
        }))),
    }
}

async fn configure(
    state: &AppState,
    id: &str,
    client: &str,
    engine: &str,
    enabled: bool,
) -> Result<()> {
    let result =
        commands::set_mcp_configuration(state, id.into(), client.into(), engine.into(), enabled)
            .await?;
    if result
        .files
        .iter()
        .all(|file| file.status == "success" || file.status == "unchanged")
    {
        return Ok(());
    }
    Err(AppError::new(
        "CONFIG_INCOMPLETE",
        tr(
            "客户端配置未能完成，请在 MCP 配置中检查",
            "The client configuration did not complete. Check it under MCP configuration",
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task(state: &str, error: Option<AppError>) -> TaskProgress {
        TaskProgress {
            operation_id: "op".into(),
            project_id: "p".into(),
            generation: "g".into(),
            sequence: 1,
            timestamp: now(),
            kind: "start".into(),
            state: state.into(),
            message: state.into(),
            error,
            started_at: now(),
        }
    }
    #[test]
    fn settled_tasks_report_their_own_errors() {
        assert!(outcome(&task("running", None)).is_none());
        assert!(outcome(&task("completed", None)).unwrap().is_ok());
        let failed = outcome(&task(
            "failed",
            Some(AppError::new("INDEX_FAILED", "index failed")),
        ))
        .unwrap()
        .unwrap_err();
        assert_eq!(failed.code, "INDEX_FAILED");
        let cancelled = outcome(&task("cancelled", None)).unwrap().unwrap_err();
        assert_eq!(cancelled.code, "TASK_FAILED");
    }
    #[test]
    fn missing_prerequisites_carry_their_detail() {
        let error = required("Missing", "detail");
        assert_eq!(error.code, ENVIRONMENT_REQUIRED);
        assert!(error.message.contains("detail"));
        assert_eq!(required("Missing", "").message, "Missing");
    }
}
