use super::*;
use crate::persistence;
use project_protocol::RuntimeRecord;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

// Tauri links its Common Controls v6 manifest into binaries, but not library tests.
#[cfg(windows)]
#[link(name = "resource", kind = "static")]
unsafe extern "C" {}

struct Fixture {
    app: tauri::App<MockRuntime>,
    state: AppState,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        std::fs::create_dir_all(bundle.join("bin")).unwrap();
        std::fs::create_dir_all(bundle.join("lib/dist/bin")).unwrap();
        let script = bundle.join("lib/dist/bin/codegraph.js");
        std::fs::write(&script, include_str!("fake-codegraph.cjs")).unwrap();
        let node = which::which("node").expect("runtime tests require Node.js on PATH");
        let entry;
        #[cfg(windows)]
        {
            let runtime = bundle.join("node.exe");
            if std::fs::hard_link(&node, &runtime).is_err() {
                std::fs::copy(&node, &runtime).unwrap();
            }
            entry = bundle.join("bin/codegraph.js");
            std::fs::write(&entry, "").unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            entry = bundle.join("bin/codegraph");
            let quote =
                |path: &Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
            std::fs::write(
                &entry,
                format!(
                    "#!/bin/sh\nexec {} {} \"$@\"\n",
                    quote(&node),
                    quote(&script)
                ),
            )
            .unwrap();
            std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let data = dir.path().join("data");
        let db = persistence::open(&data).unwrap();
        persistence::set_setting(&db, "codegraphEntry", &entry.to_string_lossy()).unwrap();
        Self {
            app: mock_builder().build(mock_context(noop_assets())).unwrap(),
            state: AppState {
                db: std::sync::Mutex::new(db),
                data,
                snapshots: Default::default(),
                gateways: Default::default(),
                http_host: Default::default(),
                serena_instances: Default::default(),
                serena_snapshots: Default::default(),
                locks: Default::default(),
                previews: Default::default(),
                tasks: Default::default(),
                task_history: Default::default(),
                logs: Default::default(),
                sequence: Default::default(),
                quitting: Default::default(),
                exit_ready: Default::default(),
                active_indexes: Default::default(),
            },
            dir,
        }
    }

    fn project(&self) -> Project {
        let id = uuid::Uuid::new_v4().to_string();
        let root = self.dir.path().join(&id);
        std::fs::create_dir_all(root.join(".codegraph")).unwrap();
        let root = persistence::canonical(&root.to_string_lossy()).unwrap();
        let project = Project {
            id,
            name: "runtime-test".into(),
            root_path: persistence::display(&root),
            canonical_path: root.to_string_lossy().into(),
            notes: String::new(),
            auto_start: false,
            created_at: now(),
            updated_at: now(),
            previous_roots: vec![],
        };
        persistence::save(&self.state.db.lock().unwrap(), &project).unwrap();
        project
    }

    async fn start(&self, id: &str) -> Result<()> {
        tokio::time::timeout(
            Duration::from_secs(20),
            start(self.app.handle(), &self.state, id),
        )
        .await
        .expect("start must not deadlock while cleaning an old gateway")
    }

    async fn stop(&self, id: &str) {
        stop(self.app.handle(), &self.state, id).await.unwrap();
    }

    async fn crash(&self, id: &str) {
        let record = project_protocol::read_runtime(id).unwrap();
        let client = project_gateway::connect(&record).await.unwrap();
        let request = serde_json::from_value(serde_json::json!({
            "name": "codegraph_explore", "arguments": {"query": "exit"}
        }))
        .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(5), client.call_tool(request))
                .await
                .unwrap()
                .is_err()
        );
        let _ = client.cancel().await;
        assert!(!self.state.gateways.lock().await.get(id).unwrap().is_alive());
        let mut snapshot = self.state.snapshot(id);
        snapshot.state = "error".into();
        snapshot.error = Some(AppError::new("UPSTREAM_EXITED", "项目会话已退出，请重启"));
        self.state.publish(self.app.handle(), snapshot);
        project_protocol::remove_runtime(id).unwrap();
    }

    async fn assert_running(&self, id: &str) -> RuntimeRecord {
        let snapshot = self.state.snapshot(id);
        assert_eq!(snapshot.state, "running");
        assert!(snapshot.error.is_none());
        assert!(snapshot.pid.is_some());
        assert!(self.state.gateways.lock().await.get(id).unwrap().is_alive());
        let record = project_protocol::read_runtime(id).unwrap();
        assert_eq!(snapshot.generation, record.generation);
        assert_eq!(
            project_gateway::probe(&record).await.unwrap(),
            ["codegraph_explore"]
        );
        record
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for project in persistence::list(&self.state.db.lock().unwrap()).unwrap() {
            let _ = project_protocol::remove_runtime(&project.id);
        }
    }
}

#[tokio::test]
async fn start_recovers_exited_gateway_without_disturbing_other_projects() {
    let fixture = Fixture::new();
    let a = fixture.project();
    let b = fixture.project();
    fixture.start(&a.id).await.unwrap();
    fixture.start(&b.id).await.unwrap();
    let mut previous = fixture.assert_running(&a.id).await;
    let other = fixture.assert_running(&b.id).await;
    fixture.start(&a.id).await.unwrap();
    assert_eq!(
        fixture.assert_running(&a.id).await.generation,
        previous.generation
    );

    for _ in 0..2 {
        fixture.crash(&a.id).await;
        fixture.start(&a.id).await.unwrap();
        let recovered = fixture.assert_running(&a.id).await;
        assert_ne!(recovered.generation, previous.generation);
        assert_eq!(recovered.endpoint, previous.endpoint);
        assert_eq!(recovered.token, previous.token);
        assert!(project_gateway::connect(&previous).await.is_err());
        assert_eq!(
            fixture.assert_running(&b.id).await.generation,
            other.generation
        );
        previous = recovered;
    }
    fixture.stop(&a.id).await;
    fixture.stop(&b.id).await;
}

#[tokio::test]
async fn failed_recovery_can_be_retried_without_stale_registration() {
    let fixture = Fixture::new();
    let project = fixture.project();
    fixture.start(&project.id).await.unwrap();
    fixture.crash(&project.id).await;
    let marker = Path::new(&project.canonical_path).join("fail-start");
    std::fs::write(&marker, "").unwrap();
    assert_eq!(
        fixture.start(&project.id).await.unwrap_err().code,
        "MCP_HANDSHAKE_FAILED"
    );
    assert!(!fixture
        .state
        .gateways
        .lock()
        .await
        .contains_key(&project.id));
    assert!(project_protocol::read_runtime(&project.id).is_err());
    let snapshot = fixture.state.snapshot(&project.id);
    assert!(snapshot.pid.is_none());
    assert!(snapshot.started_at.is_none());
    assert_eq!(snapshot.sessions, 0);
    std::fs::remove_file(marker).unwrap();
    fixture.start(&project.id).await.unwrap();
    fixture.assert_running(&project.id).await;
    fixture.stop(&project.id).await;
}

#[tokio::test]
async fn dead_gateway_uses_current_binding_but_live_gateway_requires_explicit_restart() {
    let fixture = Fixture::new();
    let project = fixture.project();
    fixture.start(&project.id).await.unwrap();
    let previous = fixture.assert_running(&project.id).await;
    let token = "b".repeat(64);
    fixture
        .state
        .db
        .lock()
        .unwrap()
        .execute(
            "UPDATE project_http SET token=?1 WHERE project_id=?2",
            rusqlite::params![token, project.id],
        )
        .unwrap();
    assert_eq!(
        fixture.start(&project.id).await.unwrap_err().code,
        "HTTP_RESTART_REQUIRED"
    );
    assert_eq!(
        fixture.assert_running(&project.id).await.generation,
        previous.generation
    );
    fixture.crash(&project.id).await;
    fixture.start(&project.id).await.unwrap();
    assert_eq!(fixture.assert_running(&project.id).await.token, token);
    fixture.stop(&project.id).await;
}
