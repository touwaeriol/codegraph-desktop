use crate::{configuration, models::*, persistence};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// Each file has its own durable intent, backup and compare-before-write guard.
/// A crash after writing but before updating ownership is recovered only by exact fingerprint.
pub fn migrate(db: &mut Connection, data: &Path) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    for project in persistence::list(db)? {
        let old: Option<u16> = db
            .query_row(
                "SELECT port FROM project_http WHERE project_id=?1",
                [&project.id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(old_port) = old else { continue };
        let binding = persistence::ensure_http_binding(db, &project.id)?;
        for client in ["codex", "claude"] {
            let owned: Option<String> = db
                .query_row(
                    "SELECT fingerprint FROM managed_config WHERE project_id=?1 AND client=?2",
                    params![project.id, client],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(owned) = owned else { continue };
            let pending: Option<(String,String)> = db.query_row("SELECT old_fingerprint,new_fingerprint FROM http_migration_pending WHERE project_id=?1 AND client=?2",params![project.id,client],|r| Ok((r.get(0)?,r.get(1)?))).optional()?;
            let path = Path::new(&project.canonical_path).join(if client == "codex" {
                ".codex/config.toml"
            } else {
                ".mcp.json"
            });
            if configuration::safe_path(Path::new(&project.canonical_path), &path).is_err() {
                notes.push(format!(
                    "{} / {client}: HTTP configuration path requires manual review",
                    project.name
                ));
                continue;
            }
            if let Some((old_fp, new_fp)) = pending {
                if owned == old_fp
                    && std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|s| {
                            configuration::entry_fingerprint(&s, client, "codegraph")
                                .ok()
                                .flatten()
                        })
                        .as_deref()
                        == Some(&new_fp)
                {
                    commit_ownership(db, &project.id, client, &new_fp)?;
                    continue;
                }
            }
            let preview = match configuration::shared_http_migration_preview(
                &project, client, &owned, old_port, &binding,
            ) {
                Ok(Some(p)) => p,
                Ok(None) => continue,
                Err(_) => {
                    notes.push(format!(
                        "{} / {client}: HTTP configuration requires manual review",
                        project.name
                    ));
                    continue;
                }
            };
            let new_fp = preview.files[0].fingerprint.as_ref().unwrap();
            db.execute("INSERT INTO http_migration_pending(project_id,client,old_fingerprint,new_fingerprint) VALUES(?1,?2,?3,?4) ON CONFLICT(project_id,client) DO UPDATE SET old_fingerprint=excluded.old_fingerprint,new_fingerprint=excluded.new_fingerprint",params![project.id,client,owned,new_fp])?;
            match configuration::apply(&preview, Path::new(&project.canonical_path), data) {
                Ok(result)
                    if result
                        .files
                        .iter()
                        .all(|f| f.status == "success" || f.status == "unchanged") =>
                {
                    commit_ownership(db, &project.id, client, new_fp)?;
                }
                _ => notes.push(format!(
                    "{} / {client}: HTTP configuration migration was not applied",
                    project.name
                )),
            }
        }
    }
    Ok(notes)
}
fn commit_ownership(db: &mut Connection, id: &str, client: &str, fingerprint: &str) -> Result<()> {
    let tx = db.transaction()?;
    tx.execute(
        "UPDATE managed_config SET fingerprint=?3 WHERE project_id=?1 AND client=?2",
        params![id, client, fingerprint],
    )?;
    tx.execute(
        "DELETE FROM http_migration_pending WHERE project_id=?1 AND client=?2",
        params![id, client],
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path) -> Project {
        Project {
            id: "migration-project".into(),
            name: "fixture".into(),
            root_path: root.to_string_lossy().into(),
            canonical_path: root.to_string_lossy().into(),
            notes: String::new(),
            auto_start: false,
            created_at: now(),
            updated_at: now(),
            previous_roots: vec![],
        }
    }
    #[test]
    fn migrates_owned_http_only_preserves_settings_and_can_restart() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let data = temp.path().join("data");
        let mut db = persistence::open(&data).unwrap();
        let p = fixture(&root);
        persistence::save(&db, &p).unwrap();
        let binding = persistence::ensure_http_binding(&mut db, &p.id).unwrap();
        let old_port: u16 = db
            .query_row(
                "SELECT port FROM project_http WHERE project_id=?1",
                [&p.id],
                |r| r.get(0),
            )
            .unwrap();
        let path = root.join(".mcp.json");
        let old=serde_json::json!({"unrelated":{"keep":true},"mcpServers":{"codegraph":{"type":"http","url":format!("http://127.0.0.1:{old_port}/mcp"),"headers":{"Authorization":binding.authorization()},"custom":"keep"},"other":{"command":"other"}}}).to_string();
        std::fs::write(&path, &old).unwrap();
        let old_fp = configuration::entry_fingerprint(&old, "claude", "codegraph")
            .unwrap()
            .unwrap();
        db.execute(
            "INSERT INTO managed_config VALUES(?1,'claude',?2)",
            params![p.id, old_fp],
        )
        .unwrap();
        assert!(migrate(&mut db, &data).unwrap().is_empty());
        let new = std::fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&new).unwrap();
        assert_eq!(value["mcpServers"]["codegraph"]["url"], binding.endpoint());
        assert_eq!(value["mcpServers"]["codegraph"]["custom"], "keep");
        assert_eq!(value["unrelated"]["keep"], true);
        assert_eq!(value["mcpServers"]["other"]["command"], "other");
        assert!(!root.join(".codex/config.toml").exists());
        assert!(migrate(&mut db, &data).unwrap().is_empty());
        assert_eq!(new, std::fs::read_to_string(&path).unwrap());
        // Simulate file written followed by process crash before ownership commit.
        let new_fp = configuration::entry_fingerprint(&new, "claude", "codegraph")
            .unwrap()
            .unwrap();
        db.execute("UPDATE managed_config SET fingerprint=?1", [&old_fp])
            .unwrap();
        db.execute(
            "INSERT INTO http_migration_pending VALUES(?1,'claude',?2,?3)",
            params![p.id, old_fp, new_fp],
        )
        .unwrap();
        migrate(&mut db, &data).unwrap();
        let restored: String = db
            .query_row("SELECT fingerprint FROM managed_config", [], |r| r.get(0))
            .unwrap();
        assert_eq!(restored, new_fp);
        // Even a valid old endpoint is not changed after manual edit.
        let edited = old.replace("\"keep\"", "\"manually changed\"");
        std::fs::write(&path, &edited).unwrap();
        db.execute("UPDATE managed_config SET fingerprint=?1", [&old_fp])
            .unwrap();
        migrate(&mut db, &data).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        // Fingerprint ownership alone is insufficient when the token is foreign.
        let foreign = old.replace(&binding.token, &"f".repeat(64));
        let foreign_fp = configuration::entry_fingerprint(&foreign, "claude", "codegraph")
            .unwrap()
            .unwrap();
        std::fs::write(&path, &foreign).unwrap();
        db.execute("UPDATE managed_config SET fingerprint=?1", [foreign_fp])
            .unwrap();
        migrate(&mut db, &data).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), foreign);
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    #[test]
    fn missing_unowned_and_failed_write_are_safe_and_retryable() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("project");
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        let mut db = persistence::open(&tmp.path().join("db")).unwrap();
        let p = Project {
            id: "toml-project".into(),
            name: "fixture".into(),
            root_path: root.to_string_lossy().into(),
            canonical_path: root.to_string_lossy().into(),
            notes: String::new(),
            auto_start: false,
            created_at: now(),
            updated_at: now(),
            previous_roots: vec![],
        };
        persistence::save(&db, &p).unwrap();
        let b = persistence::ensure_http_binding(&mut db, &p.id).unwrap();
        let old: u16 = db
            .query_row(
                "SELECT port FROM project_http WHERE project_id=?1",
                [&p.id],
                |r| r.get(0),
            )
            .unwrap();
        let content=format!("# keep comment\n[settings]\nx = 7\n[mcp_servers.codegraph]\nurl = \"http://127.0.0.1:{old}/mcp\"\ntool_timeout_sec = 987\ncustom = \"untouched\"\n[mcp_servers.codegraph.http_headers]\nAuthorization = \"{}\"\n",b.authorization());
        let path = root.join(".codex/config.toml");
        std::fs::write(&path, &content).unwrap();
        let backup = tmp.path().join("backup");
        // No ownership means no write.
        migrate(&mut db, &backup).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        let fp = configuration::entry_fingerprint(&content, "codex", "codegraph")
            .unwrap()
            .unwrap();
        db.execute(
            "INSERT INTO managed_config VALUES(?1,'codex',?2)",
            params![p.id, fp],
        )
        .unwrap();
        db.execute(
            "INSERT INTO managed_config VALUES(?1,'claude','missing')",
            [&p.id],
        )
        .unwrap();
        // A non-directory backup destination fails before changing the client file.
        std::fs::write(&backup, b"blocked").unwrap();
        assert_eq!(migrate(&mut db, &backup).unwrap().len(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        assert!(!root.join(".mcp.json").exists());
        std::fs::remove_file(&backup).unwrap();
        assert!(migrate(&mut db, &backup).unwrap().is_empty());
        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("# keep comment"));
        assert!(updated.contains("tool_timeout_sec = 987"));
        assert!(updated.contains("custom = \"untouched\""));
        assert!(updated.contains(&b.endpoint()));
        let pending: i64 = db
            .query_row("SELECT COUNT(*) FROM http_migration_pending", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(pending, 0);
    }
}
