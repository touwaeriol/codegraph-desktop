//! Real CodeGraph direct HTTP smoke: host <shim> <project-A> <project-B>.
use anyhow::{ensure, Result};
use project_gateway::{resolve_entry, CodeGraphEntry, Gateway, GatewayOptions};
use rmcp::{
    model::CallToolRequestParam,
    service::RunningService,
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    RoleClient, ServiceExt,
};
use std::path::PathBuf;
async fn client(port: u16, token: &str) -> Result<RunningService<RoleClient, ()>> {
    let http = reqwest::Client::builder().no_proxy().build()?;
    let transport = StreamableHttpClientTransport::with_client(
        http,
        StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{port}/mcp"))
            .auth_header(token.to_owned()),
    );
    Ok(tokio::time::timeout(std::time::Duration::from_secs(20), ().serve(transport)).await??)
}
async fn query(c: &RunningService<RoleClient, ()>, name: &str) -> Result<String> {
    let result = c
        .call_tool(CallToolRequestParam {
            name: "codegraph_explore".into(),
            arguments: serde_json::json!({"query":name,"maxFiles":1,"includeSource":true})
                .as_object()
                .cloned(),
        })
        .await?;
    ensure!(result.is_error != Some(true), "source query failed");
    Ok(serde_json::to_string(&result)?)
}
async fn lightweight_contract(c: &RunningService<RoleClient, ()>, root: &str) -> Result<()> {
    let mut names: Vec<_> = c
        .list_all_tools()
        .await?
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect();
    names.sort();
    ensure!(
        names
            == [
                "codegraph_callers",
                "codegraph_explore",
                "codegraph_impact",
                "codegraph_search"
            ],
        "unexpected Desktop tools: {names:?}"
    );
    let default = c
        .call_tool(CallToolRequestParam {
            name: "codegraph_explore".into(),
            arguments:
                serde_json::json!({"query":"uniqueMarker", "maxFiles":1, "projectPath":root})
                    .as_object()
                    .cloned(),
        })
        .await?;
    ensure!(default.is_error != Some(true), "default explore failed");
    let text = serde_json::to_string(&default)?;
    ensure!(
        text.contains("uniqueMarker") && text.contains("sample.ts"),
        "default explore lost symbol name/location"
    );
    ensure!(
        !text.contains("A_ONLY_739"),
        "default explore leaked function-body marker"
    );
    for (name, arguments) in [
        (
            "codegraph_explore",
            serde_json::json!({"query":"uniqueMarker", "maxFiles":1, "maxChars":512}),
        ),
        (
            "codegraph_search",
            serde_json::json!({"query":"uniqueMarker", "limit":1, "maxChars":512}),
        ),
        (
            "codegraph_callers",
            serde_json::json!({"symbol":"uniqueMarker", "maxChars":512}),
        ),
        (
            "codegraph_impact",
            serde_json::json!({"symbol":"uniqueMarker", "maxChars":512}),
        ),
    ] {
        let result = c
            .call_tool(CallToolRequestParam {
                name: name.into(),
                arguments: arguments.as_object().cloned(),
            })
            .await?;
        ensure!(
            result.is_error != Some(true),
            "{name} failed on small project"
        );
        ensure!(
            serde_json::to_string(&result)?.chars().count() <= 512,
            "{name} exceeded serialized maxChars"
        );
    }
    Ok(())
}
fn options(id: &str, root: &str, entry: &CodeGraphEntry, port: u16, token: &str) -> GatewayOptions {
    GatewayOptions {
        project_id: id.into(),
        root: PathBuf::from(root),
        entry: entry.clone(),
        preferred_port: Some(port),
        persistent_token: Some(token.into()),
    }
}
fn free_port() -> Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port())
}
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        args.len() == 4,
        "usage: http_direct_smoke <shim> <project-A> <project-B>"
    );
    let entry = resolve_entry(std::path::Path::new(&args[1]))?;
    let id = uuid::Uuid::new_v4().to_string();
    let other_id = uuid::Uuid::new_v4().to_string();
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let other_token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let port = free_port()?;
    let a = Gateway::start(options(&id, &args[2], &entry, port, &token)).await?;
    let first_generation = a.generation().to_owned();
    ensure!(
        a.port() == port && a.token() == token,
        "persistent identity changed"
    );
    let collision = Gateway::start(options(&id, &args[2], &entry, port, &token)).await;
    ensure!(
        collision.is_err(),
        "occupied persistent port silently moved"
    );
    ensure!(
        collision
            .err()
            .unwrap()
            .to_string()
            .contains("PORT_BIND_FAILED"),
        "wrong collision failure"
    );
    let other_port = free_port()?;
    let b = Gateway::start(options(
        &other_id,
        &args[3],
        &entry,
        other_port,
        &other_token,
    ))
    .await?;
    let (a1, a2, b1) = tokio::try_join!(
        client(port, &token),
        client(port, &token),
        client(other_port, &other_token)
    )?;
    lightweight_contract(&a1, &args[2]).await?;
    let (first, second, other) = tokio::try_join!(
        query(&a1, "uniqueMarker"),
        query(&a2, "secondaryMarker"),
        query(&b1, "uniqueMarker")
    )?;
    ensure!(
        first.contains("A_ONLY_739") && !first.contains("A_SECOND_842"),
        "A1 response mismatch"
    );
    ensure!(
        second.contains("A_SECOND_842") && !second.contains("A_ONLY_739"),
        "A2 response mismatch"
    );
    ensure!(other.contains("B_ONLY_739"), "B response mismatch");
    let outside = a1
        .call_tool(CallToolRequestParam {
            name: "codegraph_explore".into(),
            arguments: serde_json::json!({"query":"uniqueMarker","projectPath":args[3]})
                .as_object()
                .cloned(),
        })
        .await;
    ensure!(outside.is_err(), "cross-project request succeeded");
    let unauthorized = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(format!("http://127.0.0.1:{other_port}/identity"))
        .bearer_auth(&token)
        .send()
        .await?;
    ensure!(unauthorized.status() == 401, "cross-project token accepted");
    a1.cancel().await?;
    a2.cancel().await?;
    a.stop().await?;
    ensure!(
        query(&b1, "uniqueMarker").await?.contains("B_ONLY_739"),
        "stop A affected B"
    );
    let restarted = Gateway::start(options(&id, &args[2], &entry, port, &token)).await?;
    ensure!(
        restarted.port() == port
            && restarted.token() == token
            && restarted.generation() != first_generation,
        "restart identity mismatch"
    );
    let fresh = client(port, &token).await?;
    ensure!(
        query(&fresh, "uniqueMarker").await?.contains("A_ONLY_739"),
        "saved HTTP endpoint failed after restart"
    );
    fresh.cancel().await?;
    b1.cancel().await?;
    restarted.stop().await?;
    b.stop().await?;
    ensure!(
        std::net::TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "port not released"
    );
    println!("{{\"directHttpQueries\":true,\"defaultExploreSourceFree\":true,\"lightweightToolsCallable\":true,\"serializedMaxCharsRespected\":true,\"sameProjectMultiplexing\":true,\"crossProjectRejected\":true,\"fixedPortAndTokenRestart\":true,\"occupiedPortRejectedWithoutFallback\":true,\"portsReleased\":true}}");
    Ok(())
}
