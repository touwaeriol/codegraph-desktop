use crate::models::*;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

pub fn open(dir: &Path) -> Result<Connection> {
    std::fs::create_dir_all(dir)?;
    let mut db = Connection::open(dir.join("app.db"))?;
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
    let integrity: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Err(AppError::new(
            "DATABASE_CORRUPT",
            "应用数据库损坏，请保留文件并恢复备份",
        ));
    }
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > 3 {
        return Err(AppError::new(
            "DATABASE_VERSION",
            "数据库版本较新，请升级应用",
        ));
    }
    let tx = db.transaction()?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS projects(id TEXT PRIMARY KEY, canonical_key TEXT UNIQUE NOT NULL, data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS managed_config(project_id TEXT NOT NULL, client TEXT NOT NULL, fingerprint TEXT NOT NULL, PRIMARY KEY(project_id,client)); CREATE TABLE IF NOT EXISTS project_http(project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE, port INTEGER UNIQUE NOT NULL CHECK(port > 0 AND port <= 65535), token TEXT NOT NULL); PRAGMA user_version=2;")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS shared_http(id INTEGER PRIMARY KEY CHECK(id=1), port INTEGER NOT NULL CHECK(port > 0 AND port <= 65535)); CREATE TABLE IF NOT EXISTS http_migration_pending(project_id TEXT NOT NULL, client TEXT NOT NULL, old_fingerprint TEXT NOT NULL, new_fingerprint TEXT NOT NULL, PRIMARY KEY(project_id,client)); PRAGMA user_version=3;")?;
    tx.commit()?;
    Ok(db)
}
#[derive(Clone)]
pub struct HttpBinding {
    pub project_id: String,
    pub port: u16,
    pub token: String,
}
impl HttpBinding {
    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/mcp/{}", self.port, self.project_id)
    }
    pub fn authorization(&self) -> String {
        format!("Bearer {}", self.token)
    }
}
pub fn ensure_http_binding(db: &mut Connection, project_id: &str) -> Result<HttpBinding> {
    let mut binding = legacy_http_binding(db, project_id)?;
    binding.port = ensure_shared_http_port(db)?;
    Ok(binding)
}
pub fn ensure_shared_http_port(db: &mut Connection) -> Result<u16> {
    use rusqlite::OptionalExtension;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let existing: Option<u16> = tx
        .query_row("SELECT port FROM shared_http WHERE id=1", [], |r| r.get(0))
        .optional()?;
    let port = if let Some(port) = existing {
        port
    } else {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        tx.execute("INSERT INTO shared_http(id,port) VALUES(1,?1)", [port])?;
        port
    };
    tx.commit()?;
    Ok(port)
}
fn legacy_http_binding(db: &mut Connection, project_id: &str) -> Result<HttpBinding> {
    use rusqlite::OptionalExtension;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let existing: Option<(u16, String)> = tx
        .query_row(
            "SELECT port,token FROM project_http WHERE project_id=?1",
            [project_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((port, token)) = existing {
        if port == 0 || token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(AppError::new(
                "HTTP_BINDING_INVALID",
                "项目 HTTP 绑定记录损坏，请恢复应用数据库",
            ));
        }
        tx.commit()?;
        return Ok(HttpBinding {
            project_id: project_id.into(),
            port,
            token,
        });
    }
    let registered: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
        [project_id],
        |r| r.get(0),
    )?;
    if !registered {
        return Err(AppError::new("PROJECT_NOT_FOUND", "项目不存在"));
    }
    for _ in 0..64 {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
            .map_err(|_| AppError::new("PORT_BIND_FAILED", "无法分配本机 HTTP 端口"))?;
        let port = listener.local_addr()?.port();
        let reserved: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM project_http WHERE port=?1)",
            [port],
            |r| r.get(0),
        )?;
        if reserved {
            continue;
        }
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        tx.execute(
            "INSERT INTO project_http(project_id,port,token) VALUES(?1,?2,?3)",
            params![project_id, port, token],
        )?;
        tx.commit()?;
        drop(listener);
        return Ok(HttpBinding {
            project_id: project_id.into(),
            port,
            token,
        });
    }
    Err(AppError::new(
        "PORT_BIND_FAILED",
        "无法分配未被其他项目登记的 HTTP 端口",
    ))
}
pub fn list(db: &Connection) -> Result<Vec<Project>> {
    let mut stmt = db.prepare("SELECT data FROM projects ORDER BY rowid")?;
    let strings = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    strings
        .into_iter()
        .map(|s| serde_json::from_str(&s).map_err(|e| AppError::new("DATABASE_CORRUPT", e)))
        .collect()
}
pub fn get(db: &Connection, id: &str) -> Result<Project> {
    list(db)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::new("PROJECT_NOT_FOUND", "项目不存在"))
}
pub fn canonical(path: &str) -> Result<PathBuf> {
    let path = dunce::canonicalize(path).map_err(|e| AppError::new("PROJECT_MISSING", e))?;
    if !path.is_dir() {
        return Err(AppError::new("PROJECT_MISSING", "请选择一个存在的目录"));
    }
    Ok(path)
}
pub fn display(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        text.trim_start_matches(r"\\?\").to_string()
    }
}
fn key(path: &str) -> String {
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.into()
    }
}
pub fn save(db: &Connection, p: &Project) -> Result<()> {
    let data = serde_json::to_string(p).map_err(|e| AppError::new("SERIALIZATION", e))?;
    db.execute("INSERT INTO projects(id,canonical_key,data) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET canonical_key=excluded.canonical_key,data=excluded.data",params![p.id,key(&p.canonical_path),data]).map_err(|e|if matches!(&e,rusqlite::Error::SqliteFailure(code, _) if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE) {AppError::new("PROJECT_DUPLICATE","此目录已添加")} else {e.into()})?;
    Ok(())
}
pub fn setting(db: &Connection, key: &str) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(db
        .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}
pub fn set_setting(db: &Connection, key: &str, value: &str) -> Result<()> {
    db.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value])?;
    Ok(())
}
pub fn language(db: &Connection) -> Result<String> {
    let saved = setting(db, "language")?;
    Ok(crate::i18n::effective(saved.as_deref(), Some(crate::i18n::system_language())).into())
}
pub fn save_preferences(
    db: &mut Connection,
    index_concurrency: usize,
    close_behavior: &str,
    language: Option<&str>,
) -> Result<()> {
    if !(1..=4).contains(&index_concurrency) || !["tray", "exit"].contains(&close_behavior) {
        return Err(AppError::new("INVALID_SETTINGS", "设置值超出允许范围"));
    }
    if let Some(language) = language {
        crate::i18n::validate(language)?;
    }
    let tx = db.transaction()?;
    set_setting(&tx, "indexConcurrency", &index_concurrency.to_string())?;
    set_setting(&tx, "closeBehavior", close_behavior)?;
    if let Some(language) = language {
        set_setting(&tx, "language", language)?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistence_and_duplicate_identity() {
        let t = tempfile::tempdir().unwrap();
        let db = open(t.path()).unwrap();
        let p = Project {
            previous_roots: vec![],
            id: "one".into(),
            name: "测试".into(),
            root_path: display(t.path()),
            canonical_path: canonical(&t.path().to_string_lossy())
                .unwrap()
                .to_string_lossy()
                .into(),
            notes: "".into(),
            auto_start: false,
            created_at: now(),
            updated_at: now(),
        };
        save(&db, &p).unwrap();
        assert_eq!(get(&db, "one").unwrap().name, "测试");
        let mut duplicate = p.clone();
        duplicate.id = "two".into();
        assert!(save(&db, &duplicate).is_err());
    }
    #[test]
    fn canonical_directory_alias_and_distinct_worktrees() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("中文 & (项目)");
        let b = t.path().join("worktree-b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        assert_eq!(
            canonical(&a.to_string_lossy()).unwrap(),
            canonical(&a.join(".").to_string_lossy()).unwrap()
        );
        assert_ne!(
            canonical(&a.to_string_lossy()).unwrap(),
            canonical(&b.to_string_lossy()).unwrap()
        );
    }
    #[test]
    fn unc_display_retains_network_prefix() {
        assert_eq!(
            display(Path::new(r"\\?\UNC\server\share\project")),
            r"\\server\share\project"
        );
    }
}
#[cfg(test)]
mod http_binding_tests {
    use super::*;
    fn add(db: &Connection, id: &str) {
        save(
            db,
            &Project {
                previous_roots: vec![],
                id: id.into(),
                name: id.into(),
                root_path: id.into(),
                canonical_path: id.into(),
                notes: String::new(),
                auto_start: false,
                created_at: now(),
                updated_at: now(),
            },
        )
        .unwrap();
    }
    #[test]
    fn http_binding_is_stable_after_reopen_and_does_not_expose_token_in_projects() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(dir.path()).unwrap();
        add(&db, "a");
        add(&db, "b");
        let first = ensure_http_binding(&mut db, "a").unwrap();
        let second = ensure_http_binding(&mut db, "b").unwrap();
        assert_eq!(first.port, second.port);
        assert_ne!(first.endpoint(), second.endpoint());
        assert!(first.token != second.token);
        assert_eq!(first.token.len(), 64);
        assert!(!serde_json::to_string(&list(&db).unwrap())
            .unwrap()
            .contains(&first.token));
        drop(db);
        let mut db = open(dir.path()).unwrap();
        let restored = ensure_http_binding(&mut db, "a").unwrap();
        assert_eq!(restored.port, first.port);
        assert!(restored.token == first.token);
        let _occupied = std::net::TcpListener::bind(("127.0.0.1", first.port)).unwrap();
        let same = ensure_http_binding(&mut db, "a").unwrap();
        assert_eq!(same.port, first.port);
        assert!(same.token == first.token);
        db.execute("DELETE FROM projects WHERE id='a'", []).unwrap();
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM project_http WHERE project_id='a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
    #[test]
    fn concurrent_binding_creation_returns_one_persisted_identity() {
        let dir = tempfile::tempdir().unwrap();
        let first_db = open(dir.path()).unwrap();
        add(&first_db, "a");
        let second_db = open(dir.path()).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles = [first_db, second_db]
            .into_iter()
            .map(|mut db| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    ensure_http_binding(&mut db, "a").unwrap()
                })
            })
            .collect::<Vec<_>>();
        let values = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(values[0].port, values[1].port);
        assert!(values[0].token == values[1].token);
    }
    #[test]
    fn version_one_database_migration_preserves_projects_and_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let db = open(dir.path()).unwrap();
        add(&db, "old-project");
        set_setting(&db, "indexConcurrency", "3").unwrap();
        db.execute_batch("DROP TABLE project_http; PRAGMA user_version=1;")
            .unwrap();
        drop(db);
        let mut upgraded = open(dir.path()).unwrap();
        assert_eq!(get(&upgraded, "old-project").unwrap().name, "old-project");
        assert_eq!(
            setting(&upgraded, "indexConcurrency").unwrap().as_deref(),
            Some("3")
        );
        assert!(
            ensure_http_binding(&mut upgraded, "old-project")
                .unwrap()
                .port
                > 0
        );
        let version: i64 = upgraded
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 3);
    }
}

#[cfg(test)]
mod shared_upgrade_tests {
    use super::*;
    #[test]
    fn v2_upgrade_preserves_legacy_identity_and_stable_shared_port() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(dir.path()).unwrap();
        let p = Project {
            id: "old".into(),
            name: "old".into(),
            root_path: "old".into(),
            canonical_path: "old".into(),
            notes: String::new(),
            auto_start: false,
            created_at: now(),
            updated_at: now(),
            previous_roots: vec![],
        };
        save(&db, &p).unwrap();
        let old = ensure_http_binding(&mut db, "old").unwrap();
        let legacy: u16 = db
            .query_row("SELECT port FROM project_http", [], |r| r.get(0))
            .unwrap();
        db.execute_batch(
            "DROP TABLE shared_http; DROP TABLE http_migration_pending; PRAGMA user_version=2;",
        )
        .unwrap();
        drop(db);
        let mut upgraded = open(dir.path()).unwrap();
        let shared = ensure_http_binding(&mut upgraded, "old").unwrap();
        assert_eq!(old.token, shared.token);
        assert_eq!(
            legacy,
            upgraded
                .query_row("SELECT port FROM project_http", [], |r| r.get::<_, u16>(0))
                .unwrap()
        );
        drop(upgraded);
        let mut reopened = open(dir.path()).unwrap();
        assert_eq!(shared.port, ensure_shared_http_port(&mut reopened).unwrap());
        assert_eq!(list(&reopened).unwrap().len(), 1);
    }
}
