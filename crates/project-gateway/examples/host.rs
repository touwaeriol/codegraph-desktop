use project_gateway::{resolve_entry, Gateway, GatewayOptions};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let project_id = args[3].clone();
    let gateway = Gateway::start(GatewayOptions {
        project_id: project_id.clone(),
        root: args[2].clone().into(),
        entry: resolve_entry(std::path::Path::new(&args[1]))?,
        preferred_port: std::env::var("CODEGRAPH_TEST_HTTP_PORT")
            .ok()
            .map(|v| v.parse())
            .transpose()?,
        persistent_token: std::env::var("CODEGRAPH_TEST_HTTP_TOKEN").ok(),
    })
    .await?;
    let record = project_protocol::RuntimeRecord {
        project_id: project_id.clone(),
        generation: gateway.generation().into(),
        owner_pid: std::process::id(),
        endpoint: format!("http://127.0.0.1:{}/mcp", gateway.port()),
        token: gateway.token().into(),
        started_at: String::new(),
    };
    project_protocol::write_runtime(&record)?;
    println!(
        "{}",
        serde_json::json!({"port":gateway.port(),"pid":gateway.pid(),"generation":gateway.generation()})
    );
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    project_protocol::remove_runtime(&project_id)?;
    gateway.stop().await?;
    println!("stopped");
    Ok(())
}
