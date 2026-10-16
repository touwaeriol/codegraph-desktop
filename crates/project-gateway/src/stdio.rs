use anyhow::Context;
use rmcp::ServiceExt;
use std::{
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

async fn drain_diagnostic(stderr: &mut tokio::process::ChildStderr, diagnostic: &mut Vec<u8>) {
    let mut buffer = [0; 4096];
    while let Ok(n) = stderr.read(&mut buffer).await {
        if n == 0 {
            break;
        }
        diagnostic.extend_from_slice(&buffer[..n]);
        if diagnostic.len() > 4096 {
            diagnostic.drain(..diagnostic.len() - 4096);
        }
    }
}

/// 仅进行握手与工具发现；成功、失败、超时或取消后均回收测试进程树。
pub async fn probe_stdio(
    command: &mut Command,
    timeout: Duration,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<Vec<String>> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (mut child, job) = crate::spawn_owned(command).context("启动 stdio MCP 失败")?;
    let input = child.stdin.take().context("MCP stdin 不可用")?;
    let output = child.stdout.take().context("MCP stdout 不可用")?;
    let mut stderr = child.stderr.take().context("MCP stderr 不可用")?;
    let mut diagnostic = Vec::new();
    // 持续排空 stderr，只保留末尾 4 KiB；不让安装日志阻塞握手或无限占用内存。
    let drain = async {
        drain_diagnostic(&mut stderr, &mut diagnostic).await;
        std::future::pending::<()>().await;
    };
    let probe = async {
        let client = ().serve((output, input)).await?;
        let result = client.list_all_tools().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), client.cancel()).await;
        let tools = result?;
        anyhow::ensure!(!tools.is_empty(), "MCP 工具列表为空");
        Ok(tools
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect())
    };
    let cancelled = async {
        while !cancel.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let result: anyhow::Result<Vec<String>> = tokio::select! {
        result = tokio::time::timeout(timeout, probe) => result.context("MCP 测试超时").and_then(|r| r),
        _ = drain => unreachable!(),
        _ = cancelled => Err(anyhow::anyhow!("MCP 测试已取消")),
    };
    let cleanup = job.terminate_and_wait().await;
    let reaped = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
    cleanup.context("MCP 测试进程清理失败")?;
    reaped.context("MCP 测试进程退出超时")??;
    let _ = tokio::time::timeout(
        Duration::from_secs(1),
        drain_diagnostic(&mut stderr, &mut diagnostic),
    )
    .await;
    result.map_err(|error| {
        if diagnostic.is_empty() {
            error
        } else {
            error.context(String::from_utf8_lossy(&diagnostic).trim().to_owned())
        }
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stdio_probe_discovers_tools_and_reaps_the_process() {
        let mut command = Command::new("sh");
        command.args(["-c", r#"
read -r request
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}}'
read -r notification
read -r request
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"inspect_fixture","inputSchema":{"type":"object"}}]}}'
sleep 60
"#]);
        let tools = probe_stdio(
            &mut command,
            Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
        assert_eq!(tools, ["inspect_fixture"]);
    }

    #[tokio::test]
    async fn stdio_probe_times_out_or_cancels_a_stalled_server() {
        for cancelled in [false, true] {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 60"]);
            let error = probe_stdio(
                &mut command,
                Duration::from_millis(100),
                Arc::new(AtomicBool::new(cancelled)),
            )
            .await
            .unwrap_err();
            assert!(error
                .to_string()
                .contains(if cancelled { "取消" } else { "超时" }));
        }
    }

    #[tokio::test]
    async fn stdio_probe_reports_startup_diagnostics() {
        let mut command = Command::new("sh");
        command.args(["-c", "echo unsupported-node-fixture >&2; sleep 0.1; exit 1"]);
        let error = probe_stdio(
            &mut command,
            Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap_err();
        assert!(format!("{error:#}").contains("unsupported-node-fixture"));
    }
}
