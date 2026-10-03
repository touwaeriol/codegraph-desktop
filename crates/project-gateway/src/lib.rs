mod entry;
mod job;
pub mod relay;
use anyhow::Context;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    Router,
};
pub use entry::{resolve_entry, CodeGraphEntry};
pub use job::{spawn_owned, ProcessJob};
use rand::RngCore;
use rmcp::{
    service::RunningService,
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpService,
    },
    RoleClient, ServiceExt,
};
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    process::{Child, Command},
    sync::{Mutex, Semaphore},
};
use tokio_util::sync::CancellationToken;
pub struct GatewayOptions {
    pub project_id: String,
    pub root: PathBuf,
    pub entry: CodeGraphEntry,
    pub preferred_port: Option<u16>,
    pub persistent_token: Option<String>,
}
pub struct Gateway {
    logs: Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
    port: u16,
    pid: u32,
    generation: String,
    token: String,
    shared: Arc<relay::Shared>,
    child: Child,
    job: Option<job::ProcessJob>,
    upstream: Option<RunningService<RoleClient, ()>>,
    cancel: CancellationToken,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
}
#[derive(Clone)]
struct Auth {
    token: String,
    host: String,
}
async fn authorize(
    State(auth): State<Auth>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let h = request.headers();
    if h.contains_key("origin")
        || h.get("host").and_then(|v| v.to_str().ok()) != Some(auth.host.as_str())
        || h.get("authorization").and_then(|v| v.to_str().ok()) != Some(auth.token.as_str())
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}
impl Gateway {
    pub async fn start(options: GatewayOptions) -> anyhow::Result<Self> {
        uuid::Uuid::parse_str(&options.project_id)?;
        if let Some(token) = &options.persistent_token {
            anyhow::ensure!(
                token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
                "INVALID_HTTP_TOKEN: 持久令牌必须为 64 位十六进制字符串"
            );
            anyhow::ensure!(
                options.preferred_port.is_some_and(|port| port != 0),
                "INVALID_HTTP_PORT: 持久 HTTP 连接必须指定非零端口"
            );
        }
        let root = dunce::canonicalize(&options.root)?;
        anyhow::ensure!(root.join(".codegraph").is_dir(), "INDEX_REQUIRED");
        let listener = match TcpListener::bind(("127.0.0.1", options.preferred_port.unwrap_or(0)))
            .await
        {
            Ok(l) => l,
            Err(_) if options.persistent_token.is_none() && options.preferred_port.is_some() => {
                TcpListener::bind("127.0.0.1:0").await?
            }
            Err(e) => {
                return Err(anyhow::Error::new(e).context("PORT_BIND_FAILED: 无法绑定项目端口"))
            }
        };
        let port = listener.local_addr()?.port();
        let generation = uuid::Uuid::new_v4().to_string();
        let token = options.persistent_token.unwrap_or_else(|| {
            let mut bytes = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut bytes);
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        });
        let mut command = Command::new(&options.entry.program);
        command
            .args(&options.entry.prefix_args)
            .args(["serve", "--mcp", "--path"])
            .arg(&root)
            .current_dir(&root)
            .env("CODEGRAPH_NO_DAEMON", "1")
            .env("CODEGRAPH_HOST_PPID", std::process::id().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let (mut child, job) = spawn_owned(&mut command).context("启动 CodeGraph 失败")?;
        let pid = child.id().context("进程未启动")?;

        let logs = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        let log_copy = logs.clone();
        let stderr = child.stderr.take().unwrap();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut buffer = log_copy.lock().unwrap();
                if buffer.len() >= 2000 {
                    buffer.pop_front();
                }
                buffer.push_back(line);
            }
        });
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let upstream = tokio::time::timeout(Duration::from_secs(30), ().serve((output, input)))
            .await
            .context("MCP 握手超时")??;
        let tools =
            tokio::time::timeout(Duration::from_secs(30), upstream.list_all_tools()).await??;
        // 1.6.2 exposes explore; reject schema expansion until independently audited.
        let tools: Vec<_> = tools
            .into_iter()
            .filter(|t| {
                t.name == "codegraph_explore"
                    && t.input_schema
                        .get("properties")
                        .and_then(|v| v.as_object())
                        .is_some_and(|p| {
                            p.keys()
                                .all(|k| matches!(k.as_str(), "query" | "maxFiles" | "projectPath"))
                        })
            })
            .collect();
        anyhow::ensure!(!tools.is_empty(), "没有通过隔离验证的工具");
        let shared = Arc::new(relay::Shared {
            peer: upstream.peer().clone(),
            tools,
            root: Some(root),
            gate: Mutex::new(()),
            slots: Semaphore::new(33),
            sessions: Default::default(),
            poisoned: Default::default(),
        });
        let cancel = CancellationToken::new();
        let config = rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default();
        let copy = shared.clone();
        let service = StreamableHttpService::new(
            move || Ok(relay::Relay::new(copy.clone())),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let identity = serde_json::json!({"ownerPid":std::process::id(),"generation":generation,"projectId":options.project_id});
        let app = Router::new()
            .route(
                "/identity",
                axum::routing::get(move || {
                    let value = identity.clone();
                    async move { axum::Json(value) }
                }),
            )
            .nest_service("/mcp", service)
            .layer(middleware::from_fn_with_state(
                Auth {
                    token: format!("Bearer {token}"),
                    host: format!("127.0.0.1:{port}"),
                },
                authorize,
            ));
        let shutdown = cancel.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        Ok(Self {
            logs,
            port,
            pid,
            generation,
            token,
            shared,
            child,
            job: Some(job),
            upstream: Some(upstream),
            cancel,
            server,
        })
    }
    pub fn drain_logs(&self) -> Vec<String> {
        self.logs.lock().unwrap().drain(..).collect()
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn generation(&self) -> &str {
        &self.generation
    }
    pub fn token(&self) -> &str {
        &self.token
    }
    pub fn session_count(&self) -> usize {
        self.shared.sessions.load(Ordering::SeqCst)
    }
    pub fn tools(&self) -> Vec<String> {
        self.shared
            .tools
            .iter()
            .map(|t| t.name.to_string())
            .collect()
    }
    pub fn is_alive(&self) -> bool {
        !self.shared.peer.is_transport_closed() && !self.shared.poisoned.load(Ordering::SeqCst)
    }
    pub async fn stop(mut self) -> anyhow::Result<()> {
        self.cancel.cancel();
        if let Some(service) = self.upstream.take() {
            let _ = tokio::time::timeout(Duration::from_secs(3), service.cancel()).await;
        }
        match tokio::time::timeout(Duration::from_secs(3), self.child.wait()).await {
            Ok(status) => {
                status?;
            }
            Err(_) => {
                if let Some(job) = &self.job {
                    job.terminate_and_wait().await?;
                }
                self.child.kill().await?;
                self.child.wait().await?;
            }
        }
        if let Some(job) = &self.job {
            job.terminate_and_wait().await?;
        }
        self.job.take();
        self.server.abort();
        let _ = (&mut self.server).await;
        Ok(())
    }
}

pub async fn connect(
    record: &project_protocol::RuntimeRecord,
) -> anyhow::Result<RunningService<RoleClient, ()>> {
    project_protocol::validate_runtime(record)?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .build()?;
    let identity: serde_json::Value = http
        .get(record.endpoint.replace("/mcp", "/identity"))
        .bearer_auth(&record.token)
        .timeout(Duration::from_secs(20))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    anyhow::ensure!(
        identity["projectId"].as_str() == Some(record.project_id.as_str())
            && identity["ownerPid"].as_u64() == Some(record.owner_pid as u64)
            && identity["generation"].as_str() == Some(record.generation.as_str()),
        "运行记录已过期"
    );
    let transport = rmcp::transport::StreamableHttpClientTransport::with_client(
        http,
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
            record.endpoint.clone(),
        )
        .auth_header(record.token.clone()),
    );
    Ok(tokio::time::timeout(Duration::from_secs(20), ().serve(transport)).await??)
}
pub async fn probe(record: &project_protocol::RuntimeRecord) -> anyhow::Result<Vec<String>> {
    let client = connect(record).await?;
    let result = tokio::time::timeout(Duration::from_secs(20), client.list_all_tools()).await;
    let _ = client.cancel().await;
    Ok(result??.into_iter().map(|t| t.name.to_string()).collect())
}
impl Drop for Gateway {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.server.abort();
        // Closing Job Object terminates every owned descendant on Windows.
        self.job.take();
    }
}

#[cfg(test)]
mod persistent_http_tests {
    use super::*;
    fn options(token: &str, port: Option<u16>) -> GatewayOptions {
        GatewayOptions {
            project_id: uuid::Uuid::new_v4().to_string(),
            root: PathBuf::from("missing-validation-root"),
            entry: CodeGraphEntry {
                program: PathBuf::from("never-start"),
                prefix_args: vec![],
            },
            preferred_port: port,
            persistent_token: Some(token.into()),
        }
    }
    #[tokio::test]
    async fn invalid_persistent_credentials_fail_before_starting_upstream() {
        for invalid in [
            "".to_owned(),
            "a".repeat(63),
            "g".repeat(64),
            "a".repeat(65),
        ] {
            let error = Gateway::start(options(&invalid, Some(12345)))
                .await
                .err()
                .expect("invalid token accepted");
            assert!(error.to_string().contains("INVALID_HTTP_TOKEN"));
        }
        for port in [None, Some(0)] {
            let error = Gateway::start(options(&"a".repeat(64), port))
                .await
                .err()
                .expect("missing fixed port accepted");
            assert!(error.to_string().contains("INVALID_HTTP_PORT"));
        }
    }
    #[tokio::test]
    async fn persistent_port_collision_does_not_start_or_fallback() {
        let root =
            std::env::temp_dir().join(format!("codegraph-fixed-port-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".codegraph")).unwrap();
        let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut settings = options(&"a".repeat(64), Some(occupied.local_addr().unwrap().port()));
        settings.root = root.clone();
        let error = Gateway::start(settings)
            .await
            .err()
            .expect("occupied fixed port accepted");
        assert!(error.to_string().contains("PORT_BIND_FAILED"));
        assert!(error.chain().any(|cause| cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::AddrInUse)));
        std::fs::remove_dir(root.join(".codegraph")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
