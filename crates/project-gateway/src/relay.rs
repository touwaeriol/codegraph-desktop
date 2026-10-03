use rmcp::{
    model::*,
    service::{Peer, PeerRequestOptions, RequestContext},
    RoleClient, RoleServer, ServerHandler,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{Mutex, Semaphore};
pub struct Shared {
    pub peer: Peer<RoleClient>,
    pub tools: Vec<Tool>,
    pub root: Option<PathBuf>,
    pub gate: Mutex<()>,
    pub slots: Semaphore,
    pub sessions: AtomicUsize,
    pub poisoned: AtomicBool,
}
pub struct Relay {
    pub shared: Arc<Shared>,
    active: AtomicBool,
}
impl Relay {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            active: AtomicBool::new(false),
        }
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        if self.active.load(Ordering::SeqCst) {
            self.shared.sessions.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
fn error(message: impl Into<String>) -> ErrorData {
    ErrorData::internal_error(message.into(), None)
}
impl ServerHandler for Relay {
    async fn on_initialized(&self, _: rmcp::service::NotificationContext<RoleServer>) {
        if !self.active.swap(true, Ordering::SeqCst) {
            self.shared.sessions.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some("项目专属 CodeGraph 网关".into()),
            ..Default::default()
        }
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.shared.tools.clone(),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        mut request: CallToolRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        if self.shared.poisoned.load(Ordering::SeqCst) {
            return Err(error("上游执行结果不确定，请重启项目"));
        }
        if !self.shared.tools.iter().any(|t| t.name == request.name) {
            return Err(ErrorData::invalid_params("工具未通过项目隔离验证", None));
        }
        if let Some(root) = &self.shared.root {
            let args = request.arguments.get_or_insert_with(Default::default);
            if args
                .keys()
                .any(|k| !matches!(k.as_str(), "query" | "maxFiles" | "projectPath"))
            {
                return Err(ErrorData::invalid_params("未知参数", None));
            }
            if let Some(path) = args.get("projectPath") {
                let path = path
                    .as_str()
                    .ok_or_else(|| ErrorData::invalid_params("projectPath 必须是路径", None))?;
                if dunce::canonicalize(path).ok().as_ref() != Some(root) {
                    return Err(ErrorData::invalid_params("PROJECT_SCOPE_VIOLATION", None));
                }
            }
            args.insert("projectPath".into(), serde_json::json!(root));
        }
        let _slot = self
            .shared
            .slots
            .try_acquire()
            .map_err(|_| error("PROJECT_BUSY: 请求队列已满"))?;
        let _gate = tokio::select! { _=ctx.ct.cancelled()=>return Err(error("请求已取消")),gate=self.shared.gate.lock()=>gate};
        if self.shared.poisoned.load(Ordering::SeqCst) {
            return Err(error("上游执行结果不确定，请重启项目"));
        }
        let handle = self
            .shared
            .peer
            .send_cancellable_request(
                ClientRequest::CallToolRequest(CallToolRequest {
                    method: Default::default(),
                    params: request,
                    extensions: Default::default(),
                }),
                PeerRequestOptions::default(),
            )
            .await
            .map_err(|e| error(e.to_string()))?;
        // rmcp 0.8.5 locally resolves a request as Cancelled when a cancel
        // notification is sent; this does not acknowledge upstream completion.
        // Keep the serialization guard until a read-only query actually returns.
        // Downstream progress metadata is intentionally not forwarded.
        let id = handle.id.clone();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        let response = handle.await_response();
        tokio::pin!(response);
        let result = tokio::select! {
            result=&mut response=>Some(result),
            _=ctx.ct.cancelled()=>None,
            _=tokio::time::sleep_until(deadline)=>None,
        };
        let response = if let Some(result) = result {
            result.map_err(|e| error(e.to_string()))?
        } else {
            match tokio::time::timeout_at(deadline, &mut response).await {
                Ok(_) => return Err(error("请求已取消；上游只读查询已结束，结果已丢弃")),
                Err(_) => {
                    self.shared.poisoned.store(true, Ordering::SeqCst);
                    let _ = self
                        .shared
                        .peer
                        .notify_cancelled(CancelledNotificationParam {
                            request_id: id,
                            reason: Some("超时".into()),
                        })
                        .await;
                    return Err(error("上游执行超时且结束状态不确定，请重启项目"));
                }
            }
        };
        match response {
            ServerResult::CallToolResult(result) => Ok(result),
            _ => Err(error("上游响应类型错误")),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::ServiceExt;
    #[derive(Clone)]
    struct SlowTool {
        active: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
        started: Arc<tokio::sync::Notify>,
    }
    impl ServerHandler for SlowTool {
        fn get_info(&self) -> ServerInfo {
            ServerInfo {
                capabilities: ServerCapabilities::builder().enable_tools().build(),
                ..Default::default()
            }
        }
        async fn call_tool(
            &self,
            request: CallToolRequestParam,
            _: RequestContext<RoleServer>,
        ) -> Result<CallToolResult, ErrorData> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            self.started.notify_one();
            if request
                .arguments
                .as_ref()
                .and_then(|a| a.get("query"))
                .and_then(|v| v.as_str())
                == Some("slow")
            {
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(CallToolResult::success(vec![Content::text("completed")]))
        }
    }
    #[tokio::test]
    async fn cancelling_running_query_drains_before_next_client() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Notify::new());
        let fake = SlowTool {
            active: active.clone(),
            peak: peak.clone(),
            started: started.clone(),
        };
        let (server_io, client_io) = tokio::io::duplex(65536);
        let (server, upstream) = tokio::join!(fake.serve(server_io), ().serve(client_io));
        let server = server.unwrap();
        let upstream = upstream.unwrap();
        let tool:Tool=serde_json::from_value(serde_json::json!({"name":"codegraph_explore","inputSchema":{"type":"object","properties":{"query":{"type":"string"}}}})).unwrap();
        let shared = Arc::new(Shared {
            peer: upstream.peer().clone(),
            tools: vec![tool],
            root: None,
            gate: Mutex::new(()),
            slots: Semaphore::new(33),
            sessions: Default::default(),
            poisoned: Default::default(),
        });
        let (s1, c1) = tokio::io::duplex(65536);
        let (s2, c2) = tokio::io::duplex(65536);
        let relay1 = Relay::new(shared.clone());
        let relay2 = Relay::new(shared.clone());
        let (s1, c1, s2, c2) = tokio::join!(
            relay1.serve(s1),
            ().serve(c1),
            relay2.serve(s2),
            ().serve(c2)
        );
        let s1 = s1.unwrap();
        let c1 = c1.unwrap();
        let s2 = s2.unwrap();
        let c2 = c2.unwrap();
        let params = CallToolRequestParam {
            name: "codegraph_explore".into(),
            arguments: serde_json::json!({"query":"slow"}).as_object().cloned(),
        };
        let request = ClientRequest::CallToolRequest(CallToolRequest {
            method: Default::default(),
            params,
            extensions: Default::default(),
        });
        let handle = c1
            .send_cancellable_request(request, PeerRequestOptions::default())
            .await
            .unwrap();
        started.notified().await;
        handle
            .cancel(Some("test cancellation".into()))
            .await
            .unwrap();
        let result = c2
            .call_tool(CallToolRequestParam {
                name: "codegraph_explore".into(),
                arguments: serde_json::json!({"query":"fast"}).as_object().cloned(),
            })
            .await
            .unwrap();
        assert_ne!(result.is_error, Some(true));
        assert_eq!(peak.load(Ordering::SeqCst), 1);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert!(!shared.poisoned.load(Ordering::SeqCst));
        // Queue saturation rejects immediately and recovers after capacity frees.
        let full = shared.slots.acquire_many(33).await.unwrap();
        let rejected = c2
            .call_tool(CallToolRequestParam {
                name: "codegraph_explore".into(),
                arguments: serde_json::json!({"query":"fast"}).as_object().cloned(),
            })
            .await;
        assert!(rejected.unwrap_err().to_string().contains("PROJECT_BUSY"));
        drop(full);
        let _ = c1.cancel().await;
        let _ = c2.cancel().await;
        let _ = s1.cancel().await;
        let _ = s2.cancel().await;
        let _ = upstream.cancel().await;
        let _ = server.cancel().await;
    }
}
