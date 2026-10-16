use crate::{
    configuration::{StdioTarget, Target},
    models::*,
    persistence, AppState,
};
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

const PACKAGE: &str = "rea-agents@6.5.0";

pub fn binding() -> Target {
    binding_for_platform(cfg!(windows))
}

fn binding_for_platform(windows: bool) -> Target {
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
            startup_timeout_sec: 120,
            tool_timeout_sec: 600,
        }),
    }
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

pub async fn probe(state: &AppState, project: Project) -> Result<Vec<String>> {
    let root = persistence::canonical(&project.canonical_path)?;
    let target = binding().stdio.unwrap();
    let (send, receive) = tokio::sync::oneshot::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    state.tasks.spawn(uuid::Uuid::new_v4().to_string(), cancel, async move {
        let mut command = tokio::process::Command::new(&target.command);
        command.args(&target.args).current_dir(root);
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
        let unix = binding_for_platform(false).stdio.unwrap();
        assert_eq!(unix.command, "npx");
        assert_eq!(unix.args, ["-y", "rea-agents@6.5.0", "mcp"]);
        let windows = binding_for_platform(true).stdio.unwrap();
        assert_eq!(windows.command, "cmd.exe");
        assert_eq!(
            windows.args,
            ["/d", "/c", "npx", "-y", "rea-agents@6.5.0", "mcp"]
        );
    }

    #[tokio::test]
    #[ignore = "需要受支持的 Node.js、npx 与 npm registry 网络访问"]
    async fn published_rea_stdio_handshake() {
        let home = tempfile::tempdir().unwrap();
        let target = binding().stdio.unwrap();
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
}
