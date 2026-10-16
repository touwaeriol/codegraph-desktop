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
