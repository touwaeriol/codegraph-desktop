use crate::{
    configuration::{StdioTarget, Target},
    models::*,
    persistence, AppState,
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tauri::State;

const PACKAGE: &str = "rea-agents@6.5.0";
const ENVIRONMENT_SETTING: &str = "reaEnvironment";
/// Settings REA rejects at startup unless they hold an absolute path.
const ABSOLUTE_PATHS: [&str; 7] = [
    "GHIDRA_INSTALL_DIR",
    "JAVA_HOME",
    "REA_IDA_MCP_CONFIG",
    "REA_ILSPY_CMD_PATH",
    "REA_JADX_MCP_JAR",
    "REA_GHIDRA_NATIVEAOT_JAR",
    "REA_GHIDRA_SEED_FILE",
];

/// Variables clients pass to REA, which reads its provider and tool
/// configuration only from its environment.
pub type Environment = BTreeMap<String, String>;

pub fn binding(environment: Environment) -> Target {
    binding_for_platform(cfg!(windows), environment)
}
pub fn configured_binding(state: &AppState) -> Result<Target> {
    Ok(binding(environment(state)?))
}

fn binding_for_platform(windows: bool, env: Environment) -> Target {
    let mut args = Vec::new();
    if windows {
        args.extend(["/d", "/c", "npx"].map(str::to_owned));
    }
    args.extend(["-y", PACKAGE, "mcp"].map(str::to_owned));
    Target {
        name: "rea".into(),
        endpoint: String::new(),
        authorization: None,
        stdio: Some(StdioTarget {
            command: if windows { "cmd.exe" } else { "npx" }.into(),
            args,
            env,
            startup_timeout_sec: 120,
            tool_timeout_sec: 600,
        }),
        disabled_tools: vec![],
        known_tools: vec![],
    }
}

pub fn environment(state: &AppState) -> Result<Environment> {
    Ok(
        persistence::setting(&state.db.lock().unwrap(), ENVIRONMENT_SETTING)?
            .and_then(|saved| serde_json::from_str(&saved).ok())
            .unwrap_or_default(),
    )
}

fn invalid(message: String) -> AppError {
    AppError::new("INVALID_REA_ENVIRONMENT", message)
}
/// Trims entries, drops empty values and rejects what REA or the client
/// configuration cannot carry.
fn normalize(environment: Environment) -> Result<Environment> {
    let zh = crate::i18n::current() == "zh-CN";
    let mut normalized = Environment::new();
    for (key, value) in environment {
        let (key, value) = (key.trim().to_owned(), value.trim().to_owned());
        if value.is_empty() {
            continue;
        }
        let valid_key = key.len() <= 128
            && key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid_key {
            return Err(invalid(if zh {
                format!("环境变量名 {key:?} 无效，只能包含字母、数字和下划线，且不能以数字开头")
            } else {
                format!("Invalid variable name {key:?}. Use letters, digits and underscores, not starting with a digit")
            }));
        }
        if value.contains(['\n', '\r', '\0']) {
            return Err(invalid(if zh {
                format!("{key} 的值不能包含换行")
            } else {
                format!("The value of {key} cannot contain line breaks")
            }));
        }
        if ABSOLUTE_PATHS.contains(&key.as_str()) && !Path::new(&value).is_absolute() {
            return Err(invalid(if zh {
                format!("{key} 必须是绝对路径")
            } else {
                format!("{key} must be an absolute path")
            }));
        }
        if key == "REA_MCP_INPUT_SCHEMA_PROFILE" && !["full", "compact"].contains(&value.as_str()) {
            return Err(invalid(if zh {
                "REA_MCP_INPUT_SCHEMA_PROFILE 只能是 full 或 compact".to_owned()
            } else {
                "REA_MCP_INPUT_SCHEMA_PROFILE must be full or compact".to_owned()
            }));
        }
        normalized.insert(key, value);
    }
    Ok(normalized)
}

#[tauri::command]
pub fn get_rea_environment(state: State<AppState>) -> Result<Environment> {
    environment(&state)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSaved {
    environment: Environment,
    #[serde(flatten)]
    resynced: crate::commands::Resynced,
}

/// Saves the environment and rewrites every REA entry this app still
/// manages, so configured projects pick it up after a client reload.
#[tauri::command]
pub async fn save_rea_environment(
    state: State<'_, AppState>,
    environment: Environment,
) -> Result<EnvironmentSaved> {
    store_environment(&state, environment).await
}
pub(crate) async fn store_environment(
    state: &AppState,
    environment: Environment,
) -> Result<EnvironmentSaved> {
    let environment = normalize(environment)?;
    let previous = self::environment(state)?;
    persistence::set_setting(
        &state.db.lock().unwrap(),
        ENVIRONMENT_SETTING,
        &serde_json::to_string(&environment).unwrap(),
    )?;
    let resynced = if previous == environment {
        Default::default()
    } else {
        crate::commands::resync(state, "rea", |mut target| {
            if let Some(stdio) = &mut target.stdio {
                stdio.env.clone_from(&previous);
            }
            target
        })
        .await?
    };
    Ok(EnvironmentSaved {
        environment,
        resynced,
    })
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    name: String,
    ok: bool,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    remediation: Option<String>,
}

/// Runs `rea doctor` with the saved environment and keeps the runtime and
/// analysis-provider checks. Its global agent registration and skill checks
/// are skipped, since this app registers REA per project.
#[tauri::command]
pub async fn check_rea_environment(state: State<'_, AppState>) -> Result<Vec<Check>> {
    check_runtime().await?;
    doctor(configured_binding(&state)?.stdio.unwrap()).await
}
pub(crate) async fn doctor(target: StdioTarget) -> Result<Vec<Check>> {
    let mut args = target.args;
    args.pop();
    args.extend(
        [
            "doctor",
            "--format",
            "json",
            "--filter-output",
            "scope_checks",
        ]
        .map(str::to_owned),
    );
    let mut command = tokio::process::Command::new(&target.command);
    command
        .args(&args)
        .envs(&target.env)
        .current_dir(std::env::temp_dir())
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let failed = |detail: String| {
        AppError::new(
            "REA_DOCTOR_FAILED",
            format!(
                "{}: {detail}",
                crate::i18n::tr(
                    "REA 环境检查失败，请确认 npx 在 PATH 中且 npm registry 可访问",
                    "REA environment check failed. Check npx on PATH and npm registry access",
                )
            ),
        )
    };
    let output = tokio::time::timeout(
        Duration::from_secs(target.startup_timeout_sec as u64),
        command.output(),
    )
    .await
    .map_err(|_| failed("timeout".into()))?
    .map_err(|error| failed(error.to_string()))?;
    // doctor exits non-zero whenever a check fails, so only the JSON matters.
    #[derive(serde::Deserialize)]
    struct Report {
        scope_checks: Vec<Check>,
    }
    let report: Report = serde_json::from_slice(&output.stdout).map_err(|_| {
        failed(
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .last()
                .unwrap_or_default()
                .to_owned(),
        )
    })?;
    Ok(report
        .scope_checks
        .into_iter()
        .filter(|check| {
            !check.name.starts_with("registration:") && !check.name.starts_with("skill:")
        })
        .collect())
}

/// Checks the runtime clients use to launch REA. A missing npx blocks
/// activation; a Node.js version REA does not list only warns, since the
/// client may launch REA with a different Node.js.
pub async fn check_runtime() -> Result<Vec<String>> {
    let zh = crate::i18n::current() == "zh-CN";
    which::which("npx").map_err(|_| {
        AppError::new(
            crate::activation::ENVIRONMENT_REQUIRED,
            crate::i18n::tr(
                "REA 需要 npx，请先安装 Node.js（例如 brew install node）",
                "REA needs npx. Install Node.js first (for example, brew install node)",
            ),
        )
    })?;
    let mut command = tokio::process::Command::new("node");
    command
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let version = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .ok()
        .and_then(|output| output.ok())
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    if supported_node(&version) {
        return Ok(vec![]);
    }
    let version = if version.is_empty() { "?" } else { &version };
    Ok(vec![if zh {
        format!("当前 Node.js {version} 不在 REA 支持范围（22.19+、24.11+ 或 26+），如遇问题请升级")
    } else {
        format!("Node.js {version} is outside REA's supported range (22.19+, 24.11+ or 26+). Upgrade if REA fails")
    }])
}
fn supported_node(version: &str) -> bool {
    let version = version.trim().trim_start_matches('v');
    if version.contains('-') {
        return false;
    }
    let mut parts = version.split('.').map(|part| part.parse::<u32>().ok());
    let (Some(Some(major)), Some(Some(minor))) = (parts.next(), parts.next()) else {
        return false;
    };
    match major {
        22 => minor >= 19,
        24 => minor >= 11,
        major => major >= 26,
    }
}

/// Starts REA in `root` with the saved environment and lists its tools.
pub async fn probe(state: &AppState, root: std::path::PathBuf) -> Result<Vec<String>> {
    let target = configured_binding(state)?.stdio.unwrap();
    let (send, receive) = tokio::sync::oneshot::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    state.tasks.spawn(uuid::Uuid::new_v4().to_string(), cancel, async move {
        let mut command = tokio::process::Command::new(&target.command);
        command
            .args(&target.args)
            .envs(&target.env)
            .current_dir(root);
        let result = project_gateway::probe_stdio(
            &mut command,
            Duration::from_secs(target.startup_timeout_sec as u64),
            flag,
        )
        .await
        .map_err(|error| AppError::new(
            "REA_MCP_FAILED",
            format!("{}: {error:#}", crate::i18n::tr(
                "REA 测试失败，请确认 Node.js 版本符合要求、npx 在 PATH 中且 npm registry 可访问",
                "REA test failed. Check the required Node.js version, npx on PATH and npm registry access",
            )),
        ));
        let _ = send.send(result);
    })?;
    receive
        .await
        .map_err(|_| AppError::new("APP_EXITING", "应用正在退出"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_versions_follow_rea_support_policy() {
        for version in ["v22.19.0", "v24.11.1", "v26.12.0", "v27.0.0"] {
            assert!(supported_node(version), "{version}");
        }
        for version in [
            "v22.18.0",
            "v23.11.0",
            "v24.2.0",
            "v25.1.0",
            "v26.0.0-rc.1",
            "",
            "garbage",
        ] {
            assert!(!supported_node(version), "{version}");
        }
    }
    #[test]
    fn package_runner_is_pinned_and_wraps_windows_shims() {
        let unix = binding_for_platform(false, Environment::new())
            .stdio
            .unwrap();
        assert_eq!(unix.command, "npx");
        assert_eq!(unix.args, ["-y", "rea-agents@6.5.0", "mcp"]);
        assert!(unix.env.is_empty());
        let windows = binding_for_platform(true, Environment::new())
            .stdio
            .unwrap();
        assert_eq!(windows.command, "cmd.exe");
        assert_eq!(
            windows.args,
            ["/d", "/c", "npx", "-y", "rea-agents@6.5.0", "mcp"]
        );
    }
    #[test]
    fn environment_is_trimmed_and_validated() {
        let absolute = if cfg!(windows) {
            r"C:\Tools\ghidra"
        } else {
            "/opt/ghidra"
        };
        let normalized = normalize(Environment::from([
            (" GHIDRA_INSTALL_DIR ".into(), format!(" {absolute} ")),
            ("REA_ANALYSIS_PROVIDER".into(), "ghidra".into()),
            ("REA_IDA_MCP_CONFIG".into(), "   ".into()),
        ]))
        .unwrap();
        assert_eq!(
            normalized,
            Environment::from([
                ("GHIDRA_INSTALL_DIR".into(), absolute.into()),
                ("REA_ANALYSIS_PROVIDER".into(), "ghidra".into()),
            ])
        );
        for (key, value) in [
            ("1BAD", "x"),
            ("BAD-NAME", "x"),
            ("", "x"),
            ("GHIDRA_INSTALL_DIR", "relative/ghidra"),
            ("REA_MCP_INPUT_SCHEMA_PROFILE", "tiny"),
            ("REA_LOG_LEVEL", "info\nREA_TOKEN=x"),
        ] {
            let error = normalize(Environment::from([(key.into(), value.into())])).unwrap_err();
            assert_eq!(error.code, "INVALID_REA_ENVIRONMENT", "{key}={value}");
        }
    }

    #[tokio::test]
    #[ignore = "需要受支持的 Node.js、npx 与 npm registry 网络访问"]
    async fn published_rea_stdio_handshake() {
        let home = tempfile::tempdir().unwrap();
        let target = binding(Environment::new()).stdio.unwrap();
        let mut command = tokio::process::Command::new(&target.command);
        command
            .args(&target.args)
            .current_dir(home.path())
            .env("HOME", home.path())
            .env("USERPROFILE", home.path());
        let tools = project_gateway::probe_stdio(
            &mut command,
            Duration::from_secs(target.startup_timeout_sec as u64),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
        println!("REA MCP 握手成功，发现 {} 个工具", tools.len());
        assert!(!tools.is_empty());
    }

    #[tokio::test]
    #[ignore = "需要受支持的 Node.js、npx 与 npm registry 网络访问"]
    async fn published_rea_doctor_reports_configured_providers() {
        let missing = std::env::temp_dir().join("codegraph-desktop-missing-ghidra");
        let target = binding(Environment::from([(
            "GHIDRA_INSTALL_DIR".into(),
            missing.to_string_lossy().into_owned(),
        )]))
        .stdio
        .unwrap();
        let checks = doctor(target).await.unwrap();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!(checks)).unwrap()
        );
        assert!(checks.iter().any(|check| check.name == "node" && check.ok));
        let check = |name: &str| checks.iter().find(|check| check.name == name).unwrap();
        assert_eq!(
            check("ghidra").detail.as_deref(),
            Some(missing.to_string_lossy().as_ref())
        );
        assert!(!check("ghidra-installation").ok);
        assert!(checks
            .iter()
            .all(|check| !check.name.starts_with("registration:")));
    }
}
