//! Uses two existing indexed fixtures; does not write desktop runtime records.
use anyhow::{ensure, Result};
use project_gateway::{connect, resolve_entry, Gateway, GatewayOptions, SharedGateway};
use project_protocol::RuntimeRecord;
use rmcp::model::CallToolRequestParam;
async fn marker(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    own: &str,
    other: &str,
) -> Result<()> {
    let response = client
        .call_tool(CallToolRequestParam {
            name: "codegraph_explore".into(),
            arguments: serde_json::json!({"query":"uniqueMarker", "maxFiles":1})
                .as_object()
                .cloned(),
        })
        .await?;
    ensure!(response.is_error != Some(true), "marker query failed");
    let text = serde_json::to_string(&response)?;
    ensure!(
        text.contains(own) && !text.contains(other),
        "project marker isolation failed"
    );
    Ok(())
}
async fn reject_session(
    http: &reqwest::Client,
    record: &RuntimeRecord,
    session: &str,
) -> Result<()> {
    let response = http
        .post(&record.endpoint)
        .bearer_auth(&record.token)
        .header("Accept", "application/json, text/event-stream")
        .header("Mcp-Session-Id", session)
        .header("MCP-Protocol-Version", "2025-03-26")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":42,"method":"tools/list","params":{}}))
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::UNAUTHORIZED,
        "foreign or stale MCP session was not rejected: {}",
        response.status()
    );
    ensure!(
        response.text().await? == "Unauthorized: Session not found",
        "rejection was not a session isolation check"
    );
    Ok(())
}
fn record(g: &Gateway, id: &str) -> RuntimeRecord {
    RuntimeRecord {
        project_id: id.into(),
        generation: g.generation().into(),
        owner_pid: std::process::id(),
        endpoint: g.endpoint(),
        token: g.token().into(),
        started_at: String::new(),
    }
}
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        args.len() == 4,
        "usage: shared_http_smoke <entry> <project-a> <project-b>"
    );
    let entry = resolve_entry(std::path::Path::new(&args[1]))?;
    let host = SharedGateway::start(0).await?;
    let a_id = uuid::Uuid::new_v4().to_string();
    let b_id = uuid::Uuid::new_v4().to_string();
    let options = |id: &str, root: &str, token: char| GatewayOptions {
        project_id: id.into(),
        root: root.into(),
        entry: entry.clone(),
        preferred_port: Some(host.port()),
        persistent_token: Some(token.to_string().repeat(64)),
    };
    let a = Gateway::start_shared(options(&a_id, &args[2], 'a'), &host).await?;
    let b = Gateway::start_shared(options(&b_id, &args[3], 'b'), &host).await?;
    ensure!(a.port() == b.port(), "projects use different listeners");
    let a_record = record(&a, &a_id);
    let b_record = record(&b, &b_id);
    let ca = connect(&a_record).await?;
    let cb = connect(&b_record).await?;
    ensure!(!ca.list_all_tools().await?.is_empty(), "A tools missing");
    let http = reqwest::Client::builder().no_proxy().build()?;
    marker(&ca, "A_ONLY_739", "B_ONLY_739").await?;
    marker(&cb, "B_ONLY_739", "A_ONLY_739").await?;
    let initialize = http.post(&a_record.endpoint).bearer_auth(&a_record.token)
        .header("Accept", "application/json, text/event-stream")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-03-26", "capabilities":{}, "clientInfo":{"name":"shared-session-boundary-smoke", "version":"1"}
        }})).send().await?.error_for_status()?;
    let session = initialize
        .headers()
        .get("Mcp-Session-Id")
        .ok_or_else(|| anyhow::anyhow!("initialize returned no session ID"))?
        .to_str()?
        .to_owned();
    let _ = initialize.text().await?;
    reject_session(&http, &b_record, &session).await?;
    let identity = project_protocol::identity_endpoint(&b_record)?;
    ensure!(
        http.get(identity)
            .bearer_auth(a.token())
            .send()
            .await?
            .status()
            == 401,
        "cross-project token accepted"
    );
    let denied = ca
        .call_tool(CallToolRequestParam {
            name: "codegraph_explore".into(),
            arguments: serde_json::json!({"query":"fixture", "projectPath":args[3]})
                .as_object()
                .cloned(),
        })
        .await;
    ensure!(
        denied.is_err() || denied.as_ref().is_ok_and(|r| r.is_error == Some(true)),
        "cross-project path accepted"
    );
    a.stop().await?;
    ensure!(
        http.get(project_protocol::identity_endpoint(&a_record)?)
            .bearer_auth(&a_record.token)
            .send()
            .await?
            .status()
            == 404,
        "stopped A route survives"
    );
    ensure!(!cb.list_all_tools().await?.is_empty(), "stopping A broke B");
    marker(&cb, "B_ONLY_739", "A_ONLY_739").await?;
    let _ = ca.cancel().await;
    let _ = cb.cancel().await;
    let a2 = Gateway::start_shared(options(&a_id, &args[2], 'a'), &host).await?;
    ensure!(
        a2.endpoint() == a_record.endpoint,
        "restart changed project endpoint"
    );
    let restarted = connect(&record(&a2, &a_id)).await?;
    reject_session(&http, &record(&a2, &a_id), &session).await?;
    marker(&restarted, "A_ONLY_739", "B_ONLY_739").await?;
    ensure!(
        !restarted.list_all_tools().await?.is_empty(),
        "restarted route failed"
    );
    let _ = restarted.cancel().await;
    a2.stop().await?;
    host.stop().await?;
    ensure!(!b.is_alive(), "host shutdown did not cancel project");
    b.stop().await?;
    println!("PASS shared port, independent auth, unique source markers, project path isolation, foreign/stale session rejection, stop/restart isolation");
    Ok(())
}
