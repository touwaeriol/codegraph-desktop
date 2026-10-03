use project_gateway::relay::{Relay, Shared};
use rmcp::{transport::stdio, ServiceExt};
use std::sync::Arc;
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("连接失败：{e:#}。请在 CodeGraph Desktop 启动该项目。");
        std::process::exit(1)
    }
}
async fn run() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 3 && args[1] == "--project-id",
        "用法：cg-mcp-connector --project-id <uuid>"
    );
    let record = project_protocol::read_runtime(&args[2])?;
    let upstream = project_gateway::connect(&record).await?;
    let tools = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        upstream.list_all_tools(),
    )
    .await??;
    let shared = Arc::new(Shared {
        peer: upstream.peer().clone(),
        tools,
        root: None,
        gate: tokio::sync::Mutex::new(()),
        slots: tokio::sync::Semaphore::new(33),
        sessions: Default::default(),
        poisoned: Default::default(),
    });
    Relay::new(shared).serve(stdio()).await?.waiting().await?;
    upstream.cancel().await?;
    Ok(())
}
