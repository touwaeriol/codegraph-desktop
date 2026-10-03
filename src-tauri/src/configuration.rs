use crate::models::*;
use crate::persistence::HttpBinding;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, io::Write, path::Path};
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConfigMode {
    Merge,
    Overwrite,
    Edit,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigOverride {
    pub client: String,
    pub mode: ConfigMode,
    pub content: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
    pub client: String,
    pub path: String,
    pub before: String,
    pub after: String,
    pub existed: bool,
    pub conflict: bool,
    #[serde(skip)]
    pub original: Option<Vec<u8>>,
    #[serde(skip)]
    pub fingerprint: Option<String>,
    #[serde(skip)]
    pub restore_bytes: Option<Option<Vec<u8>>>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPreview {
    #[serde(skip)]
    pub validated_root: Option<String>,
    pub preview_id: String,
    pub project_id: String,
    pub service_name: String,
    pub files: Vec<ConfigFile>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileResult {
    pub path: String,
    pub status: String,
    pub message: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub operation_id: String,
    pub backup_path: String,
    pub files: Vec<FileResult>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigStatus {
    pub client: String,
    pub path: String,
    pub state: String,
    pub message: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Backup {
    project_id: String,
    service_name: String,
    state: String,
    files: Vec<BackupFile>,
}
#[derive(Serialize, Deserialize)]
struct BackupFile {
    client: String,
    path: String,
    original: Option<Vec<u8>>,
    after_hash: String,
}
pub fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
pub fn service_name(_id: &str) -> String {
    "codegraph".into()
}
fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn text(bytes: &Option<Vec<u8>>) -> Result<String> {
    String::from_utf8(bytes.clone().unwrap_or_default())
        .map(|s| s.trim_start_matches('\u{feff}').to_string())
        .map_err(|_| {
            AppError::new(
                "CONFIG_PARSE_FAILED",
                "配置格式无效，请检查 TOML/JSON 语法和编码",
            )
        })
}
pub fn safe_path(root: &Path, path: &Path) -> Result<()> {
    let mut ancestor = path;
    while std::fs::symlink_metadata(ancestor)
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| AppError::new("CONFIG_PATH", "无法验证配置路径"))?;
    }
    let actual = ancestor.canonicalize()?;
    if !actual.starts_with(root.canonicalize()?) {
        return Err(AppError::new(
            "PROJECT_SCOPE_VIOLATION",
            "配置符号链接指向项目外部",
        ));
    }
    Ok(())
}
fn merge(
    before: &str,
    client: &str,
    name: &str,
    _id: &str,
    binding: &HttpBinding,
    remove: bool,
) -> Result<(String, bool, Option<String>)> {
    if client == "codex" {
        let mut doc = before.parse::<toml_edit::DocumentMut>().map_err(|_| {
            AppError::new(
                "CONFIG_PARSE_FAILED",
                "配置格式无效，请检查 TOML/JSON 语法和编码",
            )
        })?;
        if doc.get("mcp_servers").is_some_and(|v| !v.is_table()) {
            return Err(AppError::new(
                "CONFIG_PARSE_FAILED",
                "mcp_servers 必须是 TOML 表",
            ));
        }
        let existing = doc.get("mcp_servers").and_then(|v| v.get(name));
        let old_hash = existing.map(|v| hash(v.to_string().as_bytes()));
        let conflict = existing.is_some();
        if remove {
            if let Some(t) = doc.get_mut("mcp_servers").and_then(|v| v.as_table_mut()) {
                t.remove(name);
            }
        } else {
            if doc.get("mcp_servers").is_none() {
                doc["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let mut t = toml_edit::Table::new();
            t["url"] = toml_edit::value(binding.endpoint());
            let mut headers = toml_edit::Table::new();
            headers["Authorization"] = toml_edit::value(binding.authorization());
            t["http_headers"] = toml_edit::Item::Table(headers);
            t["startup_timeout_sec"] = toml_edit::value(30);
            t["tool_timeout_sec"] = toml_edit::value(120);
            doc["mcp_servers"][name] = toml_edit::Item::Table(t);
        }
        Ok((doc.to_string(), conflict, old_hash))
    } else if client == "claude" {
        let mut doc: serde_json::Value = if before.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(before).map_err(|_| {
                AppError::new(
                    "CONFIG_PARSE_FAILED",
                    "配置格式无效，请检查 TOML/JSON 语法和编码",
                )
            })?
        };
        let obj = doc
            .as_object_mut()
            .ok_or_else(|| AppError::new("CONFIG_PARSE_FAILED", "JSON 顶层必须为对象"))?;
        if !obj.contains_key("mcpServers") {
            obj.insert("mcpServers".into(), serde_json::json!({}));
        }
        let servers = obj
            .get_mut("mcpServers")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| AppError::new("CONFIG_PARSE_FAILED", "mcpServers 必须为对象"))?;
        let old = servers.get(name).map(|v| hash(v.to_string().as_bytes()));
        let conflict = old.is_some();
        if remove {
            servers.remove(name);
        } else {
            servers.insert(
                name.into(),
                serde_json::json!({"type":"http","url":binding.endpoint(),"headers":{"Authorization":binding.authorization()}}),
            );
        }
        Ok((
            format!("{}\n", serde_json::to_string_pretty(&doc).unwrap()),
            conflict,
            old,
        ))
    } else {
        Err(AppError::new("INVALID_CLIENT", "不支持的客户端"))
    }
}
pub fn entry_fingerprint(content: &str, client: &str, name: &str) -> Result<Option<String>> {
    let content = content.trim_start_matches('\u{feff}');
    match client {
        "codex" => {
            let doc = content.parse::<toml_edit::DocumentMut>().map_err(|_| {
                AppError::new(
                    "CONFIG_PARSE_FAILED",
                    "配置格式无效，请检查 TOML/JSON 语法和编码",
                )
            })?;
            let Some(servers) = doc.get("mcp_servers") else {
                return Ok(None);
            };
            let servers = servers.as_table_like().ok_or_else(|| {
                AppError::new("CONFIG_PARSE_FAILED", "mcp_servers 必须是 TOML 表")
            })?;
            Ok(servers
                .get(name)
                .map(|item| hash(item.to_string().as_bytes())))
        }
        "claude" => {
            let doc: serde_json::Value = if content.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(content).map_err(|_| {
                    AppError::new(
                        "CONFIG_PARSE_FAILED",
                        "配置格式无效，请检查 TOML/JSON 语法和编码",
                    )
                })?
            };
            let obj = doc
                .as_object()
                .ok_or_else(|| AppError::new("CONFIG_PARSE_FAILED", "JSON 顶层必须为对象"))?;
            let Some(servers) = obj.get("mcpServers") else {
                return Ok(None);
            };
            let servers = servers
                .as_object()
                .ok_or_else(|| AppError::new("CONFIG_PARSE_FAILED", "mcpServers 必须为对象"))?;
            Ok(servers
                .get(name)
                .map(|item| hash(item.to_string().as_bytes())))
        }
        _ => Err(AppError::new("INVALID_CLIENT", "不支持的客户端")),
    }
}
pub fn preview(
    p: &Project,
    clients: &[String],
    action: &str,
    binding: &HttpBinding,
    managed: &HashMap<String, String>,
) -> Result<ConfigPreview> {
    preview_with_overrides(p, clients, action, binding, managed, &[])
}
pub fn preview_with_overrides(
    p: &Project,
    clients: &[String],
    action: &str,
    binding: &HttpBinding,
    managed: &HashMap<String, String>,
    overrides: &[ConfigOverride],
) -> Result<ConfigPreview> {
    if action != "install" && action != "remove" {
        return Err(AppError::new("INVALID_ACTION", "无效配置操作"));
    }
    if action != "install" && !overrides.is_empty() {
        return Err(AppError::new("INVALID_ACTION", "移除配置不支持内容覆盖"));
    }
    let mut choices = HashMap::new();
    for change in overrides {
        if !clients.contains(&change.client)
            || !["codex", "claude"].contains(&change.client.as_str())
            || choices.insert(change.client.as_str(), change).is_some()
        {
            return Err(AppError::new(
                "INVALID_OVERRIDE",
                "覆盖设置必须对应所选客户端，且不能重复",
            ));
        }
        if change.mode != ConfigMode::Edit && change.content.is_some() {
            return Err(AppError::new(
                "INVALID_OVERRIDE",
                "仅手动编辑模式可以提供全文内容",
            ));
        }
    }
    let mut files = Vec::new();
    let root = Path::new(&p.canonical_path);
    let name = service_name(&p.id);
    for client in clients {
        if files.iter().any(|f: &ConfigFile| &f.client == client) {
            continue;
        }
        let path = root.join(if client == "codex" {
            ".codex/config.toml"
        } else if client == "claude" {
            ".mcp.json"
        } else {
            return Err(AppError::new("INVALID_CLIENT", client));
        });
        safe_path(root, &path)?;
        let original = read(&path)?;
        let before = text(&original)?;
        let choice = choices.get(client.as_str()).copied();
        let mode = choice.map(|v| v.mode).unwrap_or(ConfigMode::Merge);
        if mode != ConfigMode::Merge {
            let after = if mode == ConfigMode::Overwrite {
                merge("", client, &name, &p.id, binding, false)?.0
            } else {
                choice
                    .and_then(|v| v.content.as_ref())
                    .cloned()
                    .ok_or_else(|| {
                        AppError::new("INVALID_OVERRIDE", "手动编辑模式必须提供全文内容")
                    })?
            };
            validate_content(&after, client)?;
            let fingerprint = managed_fingerprint(&after, client, &name, &p.id, binding)?;
            files.push(ConfigFile {
                client: client.clone(),
                path: path.to_string_lossy().into(),
                before,
                after,
                existed: original.is_some(),
                conflict: original.is_some(),
                original,
                fingerprint,
                restore_bytes: None,
            });
            continue;
        }
        if client == "claude" && original.is_some() && before.trim().is_empty() {
            return Err(AppError::new(
                "CONFIG_PARSE_FAILED",
                "现有 Claude Code JSON 文件为空，不能作为空配置覆盖",
            ));
        }
        let legacy_name = format!("codegraph_project_{}", p.id.replace('-', ""));
        let legacy_hash = entry_fingerprint(&before, client, &legacy_name)?;
        let merge_input = if let Some(ref legacy_hash) = legacy_hash {
            if managed.get(client) != Some(legacy_hash) {
                return Err(AppError::new(
                    "CONFIG_CHANGED",
                    "旧 CodeGraph 配置已修改或归属无法确认，请检查后再迁移",
                ));
            }
            merge(&before, client, &legacy_name, &p.id, binding, true)?.0
        } else {
            before.clone()
        };
        let (after, conflict, old) = merge(
            &merge_input,
            client,
            &name,
            &p.id,
            binding,
            action == "remove",
        )?;
        if action == "remove" && old.is_some() && old.as_ref() != managed.get(client) {
            return Err(AppError::new(
                "CONFIG_CHANGED",
                "受管条目已修改或不属于本应用，无法自动移除",
            ));
        }
        let fingerprint = if action == "remove" {
            None
        } else {
            entry_fingerprint(&after, client, &name)?
        };
        let restore_bytes = if action == "remove" && old.is_none() && legacy_hash.is_none() {
            Some(original.clone())
        } else {
            None
        };
        files.push(ConfigFile {
            client: client.clone(),
            path: path.to_string_lossy().to_string(),
            before,
            after,
            existed: original.is_some(),
            conflict,
            original,
            fingerprint,
            restore_bytes,
        });
    }
    if files.is_empty() {
        return Err(AppError::new("INVALID_CLIENT", "至少选择一个客户端"));
    }
    Ok(ConfigPreview {
        validated_root: None,
        preview_id: uuid::Uuid::new_v4().to_string(),
        project_id: p.id.clone(),
        service_name: name,
        files,
    })
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new("CONFIG_PATH", "无父目录"))?;
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|e| AppError::new("CONFIG_WRITE_FAILED", e.error))?;
    Ok(())
}
fn restore(path: &Path, bytes: &Option<Vec<u8>>) -> Result<()> {
    if let Some(b) = bytes {
        atomic_write(path, b)
    } else if path.exists() {
        Ok(std::fs::remove_file(path)?)
    } else {
        Ok(())
    }
}
pub fn apply(preview: &ConfigPreview, root: &Path, data: &Path) -> Result<ApplyResult> {
    apply_with(preview, root, data, None)
}
fn apply_with(
    preview: &ConfigPreview,
    root: &Path,
    data: &Path,
    fail_at: Option<usize>,
) -> Result<ApplyResult> {
    for f in &preview.files {
        safe_path(root, Path::new(&f.path))?;
        if read(Path::new(&f.path))? != f.original {
            return Err(AppError::new("CONFIG_CHANGED", "文件已变化，请刷新预览"));
        }
    }
    let op = uuid::Uuid::new_v4().to_string();
    let backup = data.join("backups").join(&op);
    private_dir(&backup)?;
    let mut manifest = Backup {
        project_id: preview.project_id.clone(),
        service_name: preview.service_name.clone(),
        state: "pending".into(),
        files: preview
            .files
            .iter()
            .map(|f| BackupFile {
                client: f.client.clone(),
                path: f.path.clone(),
                original: f.original.clone(),
                after_hash: desired(f).map(hash).unwrap_or_default(),
            })
            .collect(),
    };
    atomic_write(
        &backup.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest).unwrap(),
    )?;
    let mut results = Vec::new();
    let mut written: Vec<&ConfigFile> = Vec::new();
    for (index, f) in preview.files.iter().enumerate() {
        if f.original.as_deref() == desired(f) {
            continue;
        }
        let result = safe_path(root, Path::new(&f.path)).and_then(|_| {
            if fail_at == Some(index) {
                return Err(AppError::new("CONFIG_WRITE_FAILED", "模拟第二文件写入失败"));
            }
            if read(Path::new(&f.path))? != f.original {
                return Err(AppError::new("CONFIG_CHANGED", "配置在写入时被修改"));
            }
            restore(Path::new(&f.path), &desired(f).map(|b| b.to_vec()))
        });
        if let Err(err) = result {
            let mut partial = false;
            for prev in written.iter().rev() {
                let status = if read(Path::new(&prev.path))
                    .is_ok_and(|b| b.as_deref() == desired(prev))
                    && restore(Path::new(&prev.path), &prev.original).is_ok()
                {
                    "rolledBack"
                } else {
                    partial = true;
                    "rollbackFailed"
                };
                results.push(FileResult {
                    path: prev.path.clone(),
                    status: status.into(),
                    message: "写入失败后恢复原文件".into(),
                });
            }
            results.push(FileResult {
                path: f.path.clone(),
                status: "failed".into(),
                message: err.message,
            });
            for pending in &preview.files {
                if !results.iter().any(|r| r.path == pending.path) {
                    results.push(FileResult {
                        path: pending.path.clone(),
                        status: if pending.original.as_deref() == desired(pending) {
                            "unchanged"
                        } else {
                            "skipped"
                        }
                        .into(),
                        message: "未修改".into(),
                    });
                }
            }
            manifest.state = if partial { "partial" } else { "rolledBack" }.into();
            atomic_write(
                &backup.join("manifest.json"),
                &serde_json::to_vec_pretty(&manifest).unwrap(),
            )?;
            return Ok(ApplyResult {
                operation_id: op,
                backup_path: backup.to_string_lossy().into(),
                files: results,
            });
        }
        written.push(f);
    }
    manifest.state = "completed".into();
    atomic_write(
        &backup.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest).unwrap(),
    )?;
    Ok(ApplyResult {
        operation_id: op,
        backup_path: backup.to_string_lossy().into(),
        files: preview
            .files
            .iter()
            .map(|f| FileResult {
                path: f.path.clone(),
                status: if f.original.as_deref() == desired(f) {
                    "unchanged"
                } else {
                    "success"
                }
                .into(),
                message: if f.original.as_deref() == desired(f) {
                    "内容未变化"
                } else {
                    "配置已写入"
                }
                .into(),
            })
            .collect(),
    })
}
pub fn status(p: &Project, binding: &HttpBinding) -> Vec<ConfigStatus> {
    ["codex", "claude"]
        .iter()
        .map(|client| {
            let path = Path::new(&p.canonical_path).join(if *client == "codex" {
                ".codex/config.toml"
            } else {
                ".mcp.json"
            });
            let mut item = ConfigStatus {
                client: client.to_string(),
                path: path.to_string_lossy().into(),
                state: "missing".into(),
                message: None,
            };
            let check = (|| -> Result<()> {
                let bytes = read(&path)?;
                if bytes.is_none() {
                    return Ok(());
                }
                let before = text(&bytes)?;
                if *client == "claude" && bytes.is_some() && before.trim().is_empty() {
                    return Err(AppError::new(
                        "CONFIG_PARSE_FAILED",
                        "现有 Claude Code JSON 文件为空",
                    ));
                }
                let (name, id) = (service_name(&p.id), &p.id);
                validate_content(&before, client)?;
                if entry_fingerprint(&before, client, &name)?.is_some() {
                    item.state =
                        if managed_fingerprint(&before, client, &name, id, binding)?.is_some() {
                            "configured"
                        } else {
                            "repair"
                        }
                        .into();
                } else if entry_fingerprint(
                    &before,
                    client,
                    &format!("codegraph_project_{}", p.id.replace('-', "")),
                )?
                .is_some()
                {
                    item.state = "repair".into();
                    item.message = Some("检测到旧连接器配置，请重新预览并应用 HTTP 配置".into());
                }
                Ok(())
            })();
            if let Err(e) = check {
                item.state = "parseError".into();
                item.message = Some(e.message);
            }
            item
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merge_preserves() {
        let (before, _, _) = merge(
            "# keep\nmodel = 'x'\n[mcp_servers.other]\ncommand='other'\n",
            "codex",
            "ours",
            "id",
            &test_binding(),
            false,
        )
        .unwrap();
        assert!(before.contains("# keep"));
        assert!(before.contains("model = 'x'"));
        assert!(before.contains("other"));
        let (json, _, _) = merge(
            r#"{"arbitrary":{"a":1},"mcpServers":{"old":{"command":"x"}}}"#,
            "claude",
            "ours",
            "id",
            &test_binding(),
            false,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["arbitrary"]["a"], 1);
        assert_eq!(v["mcpServers"]["old"]["command"], "x");
    }
    #[test]
    fn reject_external_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".mcp.json");
        std::fs::write(&path, "{}").unwrap();
        let p = ConfigPreview {
            validated_root: None,
            preview_id: "x".into(),
            project_id: "x".into(),
            service_name: "x".into(),
            files: vec![ConfigFile {
                client: "claude".into(),
                path: path.to_string_lossy().into(),
                before: "{}".into(),
                after: "{}".into(),
                existed: true,
                conflict: false,
                original: Some(b"{}".to_vec()),
                fingerprint: None,
                restore_bytes: None,
            }],
        };
        std::fs::write(&path, "{\"new\":1}").unwrap();
        assert_eq!(
            apply(&p, dir.path(), dir.path()).err().unwrap().code,
            "CONFIG_CHANGED"
        );
    }
}
fn desired(f: &ConfigFile) -> Option<&[u8]> {
    match &f.restore_bytes {
        Some(bytes) => bytes.as_deref(),
        None => Some(f.after.as_bytes()),
    }
}
pub fn private_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let user = format!(
            "{}\\{}",
            std::env::var("USERDOMAIN").map_err(|e| AppError::new("PRIVATE_DIRECTORY", e))?,
            std::env::var("USERNAME").map_err(|e| AppError::new("PRIVATE_DIRECTORY", e))?
        );
        let result = std::process::Command::new("icacls.exe")
            .arg(path)
            .args(["/inheritance:r", "/grant:r", &format!("{user}:(OI)(CI)F")])
            .creation_flags(0x08000000)
            .output()?;
        if !result.status.success() {
            return Err(AppError::new(
                "PRIVATE_DIRECTORY",
                "无法设置备份私有目录权限",
            ));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    pub operation_id: String,
    pub project_id: String,
    pub state: String,
    pub backup_path: String,
}
pub fn list_backups(data: &Path, project_id: &str) -> Result<Vec<BackupSummary>> {
    let dir = data.join("backups");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut result = Vec::new();
    for item in std::fs::read_dir(dir)? {
        let item = item?;
        let path = item.path().join("manifest.json");
        if !path.is_file() {
            continue;
        }
        let manifest: Backup = serde_json::from_slice(&std::fs::read(path)?)
            .map_err(|_| AppError::new("BACKUP_CORRUPT", "备份清单格式无效"))?;
        if manifest.project_id == project_id {
            result.push(BackupSummary {
                operation_id: item.file_name().to_string_lossy().into(),
                project_id: manifest.project_id,
                state: manifest.state,
                backup_path: item.path().to_string_lossy().into(),
            });
        }
    }
    Ok(result)
}
pub fn preview_restore(
    data: &Path,
    operation_id: &str,
    p: &Project,
    binding: &HttpBinding,
) -> Result<ConfigPreview> {
    uuid::Uuid::parse_str(operation_id).map_err(|e| AppError::new("INVALID_OPERATION", e))?;
    let path = data
        .join("backups")
        .join(operation_id)
        .join("manifest.json");
    let manifest: Backup = serde_json::from_slice(&std::fs::read(path)?)
        .map_err(|_| AppError::new("BACKUP_CORRUPT", "备份清单格式无效"))?;
    if manifest.project_id != p.id {
        return Err(AppError::new("PROJECT_SCOPE_VIOLATION", "备份不属于该项目"));
    }
    let root = std::iter::once(&p.canonical_path)
        .chain(p.previous_roots.iter())
        .find(|root| {
            manifest
                .files
                .iter()
                .all(|file| safe_path(Path::new(root), Path::new(&file.path)).is_ok())
        })
        .ok_or_else(|| {
            AppError::new(
                "PROJECT_SCOPE_VIOLATION",
                "备份路径不属于当前或已登记的旧项目目录",
            )
        })?;
    let mut files = Vec::new();
    for file in manifest.files {
        safe_path(Path::new(root), Path::new(&file.path))?;
        let current = read(Path::new(&file.path))?;
        if current == file.original {
            continue;
        }
        if current.as_deref().map(hash).unwrap_or_default() != file.after_hash {
            return Err(AppError::new(
                "CONFIG_CHANGED",
                "备份之后配置已被修改，不能覆盖后续编辑",
            ));
        }
        let before = text(&current)?;
        let after = text(&file.original)?;
        let fingerprint = if file.original.is_some() {
            managed_fingerprint(&after, &file.client, &manifest.service_name, &p.id, binding)
                .unwrap_or(None)
        } else {
            None
        };
        files.push(ConfigFile {
            client: file.client,
            path: file.path,
            before,
            after,
            existed: current.is_some(),
            conflict: true,
            original: current,
            fingerprint,
            restore_bytes: Some(file.original),
        });
    }
    if files.is_empty() {
        return Err(AppError::new("BACKUP_UNCHANGED", "文件已经恢复，无需修改"));
    }
    Ok(ConfigPreview {
        preview_id: uuid::Uuid::new_v4().to_string(),
        validated_root: if root != &p.canonical_path {
            Some(root.clone())
        } else {
            None
        },
        project_id: p.id.clone(),
        service_name: manifest.service_name,
        files,
    })
}
pub fn mark_interrupted(data: &Path) -> Result<()> {
    let dir = data.join("backups");
    if !dir.exists() {
        return Ok(());
    }
    for item in std::fs::read_dir(dir)? {
        let path = item?.path().join("manifest.json");
        if !path.is_file() {
            continue;
        }
        let mut manifest: Backup = serde_json::from_slice(&std::fs::read(&path)?)
            .map_err(|_| AppError::new("BACKUP_CORRUPT", "备份清单格式无效"))?;
        if manifest.state == "pending" {
            manifest.state = "interrupted".into();
            atomic_write(&path, &serde_json::to_vec_pretty(&manifest).unwrap())?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn fixed_name_migrates_owned_legacy_and_preserves_other_settings() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path());
        let binding = test_binding();
        for client in ["codex", "claude"] {
            let legacy = format!("codegraph_project_{}", p.id.replace('-', ""));
            let initial = if client == "codex" {
                "# keep comment\nmodel = 'existing-model'\n[mcp_servers.other]\ncommand = 'other-tool'\n"
            } else {
                r#"{"mcpServers":{"other":{"command":"other-tool"}},"custom":true}"#
            };
            let before = merge(initial, client, &legacy, &p.id, &binding, false)
                .unwrap()
                .0;
            let path = dir.path().join(if client == "codex" {
                ".codex/config.toml"
            } else {
                ".mcp.json"
            });
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &before).unwrap();
            let mut managed = HashMap::new();
            assert!(preview(&p, &[client.into()], "install", &binding, &managed).is_err());
            managed.insert(
                client.into(),
                entry_fingerprint(&before, client, &legacy)
                    .unwrap()
                    .unwrap(),
            );
            let change = preview(&p, &[client.into()], "install", &binding, &managed).unwrap();
            let after = &change.files[0].after;
            assert!(entry_fingerprint(after, client, &legacy).unwrap().is_none());
            assert!(entry_fingerprint(after, client, "codegraph")
                .unwrap()
                .is_some());
            assert_eq!(
                entry_fingerprint(initial, client, "other").unwrap(),
                entry_fingerprint(after, client, "other").unwrap()
            );
            if client == "codex" {
                assert!(after.contains("# keep comment"));
                assert!(after.contains("existing-model"));
            }
            let again = merge(after, client, "codegraph", &p.id, &binding, false).unwrap();
            assert_eq!(again.0, *after);
            assert!(again.1);
            let removal = preview(&p, &[client.into()], "remove", &binding, &managed).unwrap();
            assert!(entry_fingerprint(&removal.files[0].after, client, &legacy)
                .unwrap()
                .is_none());
            assert!(removal.files[0].restore_bytes.is_none());
        }
    }
    fn project(dir: &Path) -> Project {
        Project {
            previous_roots: vec![],
            id: uuid::Uuid::new_v4().to_string(),
            name: "测试".into(),
            root_path: dir.to_string_lossy().into(),
            canonical_path: dir.canonicalize().unwrap().to_string_lossy().into(),
            notes: String::new(),
            auto_start: false,
            created_at: now(),
            updated_at: now(),
        }
    }
    fn config_file(path: &Path, original: Option<Vec<u8>>, after: &str) -> ConfigFile {
        ConfigFile {
            client: "claude".into(),
            path: path.to_string_lossy().into(),
            before: text(&original).unwrap(),
            after: after.into(),
            existed: original.is_some(),
            conflict: false,
            original,
            fingerprint: None,
            restore_bytes: None,
        }
    }
    #[test]
    fn second_write_failure_restores_first_and_reports_every_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        std::fs::write(&a, b"{}").unwrap();
        let preview = ConfigPreview {
            validated_root: None,
            preview_id: "test".into(),
            project_id: "p".into(),
            service_name: "s".into(),
            files: vec![
                config_file(&a, Some(b"{}".to_vec()), "{\"x\":1}"),
                config_file(&b, None, "{}"),
                config_file(&dir.path().join("c.json"), None, "{}"),
            ],
        };
        let result = apply_with(&preview, dir.path(), dir.path(), Some(1)).unwrap();
        assert_eq!(std::fs::read(a).unwrap(), b"{}");
        assert!(!b.exists());
        assert_eq!(result.files.len(), 3);
        assert!(result.files.iter().any(|f| f.status == "rolledBack"));
        assert!(result.files.iter().any(|f| f.status == "skipped"));
    }
    #[test]
    fn restore_preserves_bom_and_removes_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path());
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        let bytes = b"\xef\xbb\xbf{}".to_vec();
        std::fs::write(&a, &bytes).unwrap();
        let preview = ConfigPreview {
            validated_root: None,
            preview_id: "test".into(),
            project_id: p.id.clone(),
            service_name: "ours".into(),
            files: vec![
                config_file(&a, Some(bytes.clone()), "{\"mcpServers\":{}}"),
                config_file(&b, None, "{\"mcpServers\":{}}"),
            ],
        };
        let written = apply(&preview, dir.path(), dir.path()).unwrap();
        let recovery =
            preview_restore(dir.path(), &written.operation_id, &p, &test_binding()).unwrap();
        let reverted = apply(&recovery, dir.path(), dir.path()).unwrap();
        assert!(reverted.files.iter().all(|f| f.status == "success"));
        assert_eq!(std::fs::read(a).unwrap(), bytes);
        assert!(!b.exists());
    }
    #[test]
    fn restore_refuses_later_edit() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path());
        let a = dir.path().join("a.json");
        let preview = ConfigPreview {
            validated_root: None,
            preview_id: "test".into(),
            project_id: p.id.clone(),
            service_name: "ours".into(),
            files: vec![config_file(&a, None, "{\"mcpServers\":{}}")],
        };
        let written = apply(&preview, dir.path(), dir.path()).unwrap();
        std::fs::write(a, b"{\"user\":true}").unwrap();
        assert_eq!(
            preview_restore(dir.path(), &written.operation_id, &p, &test_binding())
                .err()
                .unwrap()
                .code,
            "CONFIG_CHANGED"
        );
    }
    #[test]
    fn invalid_json_and_toml_never_overwrite() {
        assert!(merge("{broken", "claude", "ours", "id", &test_binding(), false).is_err());
        assert!(merge("[broken", "codex", "ours", "id", &test_binding(), false).is_err());
    }
}
fn invalid_structure(message: impl ToString) -> AppError {
    AppError::new("CONFIG_PARSE_FAILED", message)
}
fn validate_transport(command: Option<&str>, url: Option<&str>, kind: Option<&str>) -> Result<()> {
    if command.is_some_and(|v| v.trim().is_empty()) || url.is_some_and(|v| v.trim().is_empty()) {
        return Err(invalid_structure("MCP command/url 不能为空"));
    }
    if command.is_some() == url.is_some() {
        return Err(invalid_structure(
            "每个 MCP 服务必须指定 command 或 url，不能同时指定两者",
        ));
    }
    if let Some(kind) = kind {
        if !["stdio", "http", "sse", "streamable-http"].contains(&kind)
            || (kind == "stdio") != command.is_some()
        {
            return Err(invalid_structure("MCP type 与 command/url 不匹配"));
        }
    }
    Ok(())
}
fn validate_toml_server(server: &dyn toml_edit::TableLike) -> Result<()> {
    for key in ["command", "url", "type"] {
        if server.get(key).is_some_and(|v| v.as_str().is_none()) {
            return Err(invalid_structure(format!("MCP {key} 必须为字符串")));
        }
    }
    validate_transport(
        server.get("command").and_then(|v| v.as_str()),
        server.get("url").and_then(|v| v.as_str()),
        server.get("type").and_then(|v| v.as_str()),
    )?;
    if let Some(args) = server.get("args") {
        if !args
            .as_array()
            .is_some_and(|a| a.iter().all(|v| v.as_str().is_some()))
        {
            return Err(invalid_structure("MCP args 必须为字符串数组"));
        }
    }
    for key in ["env", "http_headers", "env_http_headers"] {
        if let Some(map) = server.get(key) {
            if !map
                .as_table_like()
                .is_some_and(|map| map.iter().all(|(_, v)| v.as_str().is_some()))
            {
                return Err(invalid_structure(format!("MCP {key} 必须为字符串键值表")));
            }
        }
    }
    for key in ["enabled", "required"] {
        if server.get(key).is_some_and(|v| v.as_bool().is_none()) {
            return Err(invalid_structure(format!("MCP {key} 必须为布尔值")));
        }
    }
    for key in ["startup_timeout_sec", "tool_timeout_sec"] {
        if server.get(key).is_some_and(|v| {
            !v.as_integer().is_some_and(|n| n >= 0)
                && !v.as_float().is_some_and(|n| n.is_finite() && n >= 0.0)
        }) {
            return Err(invalid_structure(format!("MCP {key} 必须为非负数")));
        }
    }
    Ok(())
}
fn validate_json_server(server: &serde_json::Map<String, serde_json::Value>) -> Result<()> {
    for key in ["command", "url", "type"] {
        if server.get(key).is_some_and(|v| !v.is_string()) {
            return Err(invalid_structure(format!("MCP {key} 必须为字符串")));
        }
    }
    validate_transport(
        server.get("command").and_then(|v| v.as_str()),
        server.get("url").and_then(|v| v.as_str()),
        server.get("type").and_then(|v| v.as_str()),
    )?;
    if let Some(args) = server.get("args") {
        if !args
            .as_array()
            .is_some_and(|a| a.iter().all(|v| v.is_string()))
        {
            return Err(invalid_structure("MCP args 必须为字符串数组"));
        }
    }
    for key in ["env", "headers"] {
        if let Some(map) = server.get(key) {
            if !map
                .as_object()
                .is_some_and(|map| map.values().all(|v| v.is_string()))
            {
                return Err(invalid_structure(format!("MCP {key} 必须为字符串键值对象")));
            }
        }
    }
    Ok(())
}
fn validate_content(content: &str, client: &str) -> Result<()> {
    let content = content.trim_start_matches('\u{feff}');
    match client {
        "codex" => {
            let doc = content
                .parse::<toml_edit::DocumentMut>()
                .map_err(|_| invalid_structure("配置格式无效，请检查 TOML/JSON 语法"))?;
            if let Some(servers) = doc.get("mcp_servers") {
                let servers = servers
                    .as_table_like()
                    .ok_or_else(|| invalid_structure("mcp_servers 必须为 TOML 表"))?;
                for (name, server) in servers.iter() {
                    let server = server.as_table_like().ok_or_else(|| {
                        invalid_structure(format!("MCP 服务 {name} 必须为 TOML 表"))
                    })?;
                    validate_toml_server(server)?;
                }
            }
            Ok(())
        }
        "claude" => {
            let doc: serde_json::Value = serde_json::from_str(content)
                .map_err(|_| invalid_structure("配置格式无效，请检查 TOML/JSON 语法"))?;
            let doc = doc
                .as_object()
                .ok_or_else(|| invalid_structure("JSON 顶层必须为对象"))?;
            if let Some(servers) = doc.get("mcpServers") {
                let servers = servers
                    .as_object()
                    .ok_or_else(|| invalid_structure("mcpServers 必须为对象"))?;
                for (name, server) in servers {
                    let server = server
                        .as_object()
                        .ok_or_else(|| invalid_structure(format!("MCP 服务 {name} 必须为对象")))?;
                    validate_json_server(server)?;
                }
            }
            Ok(())
        }
        _ => Err(AppError::new("INVALID_CLIENT", "不支持的客户端")),
    }
}
fn managed_fingerprint(
    content: &str,
    client: &str,
    name: &str,
    _id: &str,
    binding: &HttpBinding,
) -> Result<Option<String>> {
    let content = content.trim_start_matches('\u{feff}');
    let endpoint = binding.endpoint();
    let authorization = binding.authorization();
    let owned = match client {
        "codex" => {
            let doc = content
                .parse::<toml_edit::DocumentMut>()
                .map_err(|_| invalid_structure("配置格式无效，请检查 TOML/JSON 语法"))?;
            doc.get("mcp_servers")
                .and_then(|v| v.as_table_like())
                .and_then(|v| v.get(name))
                .and_then(|v| v.as_table_like())
                .is_some_and(|server| {
                    server.get("url").and_then(|v| v.as_str()) == Some(endpoint.as_str())
                        && server.get("command").is_none()
                        && server.get("args").is_none()
                        && server
                            .get("http_headers")
                            .and_then(|v| v.as_table_like())
                            .and_then(|v| v.get("Authorization"))
                            .and_then(|v| v.as_str())
                            == Some(authorization.as_str())
                })
        }
        "claude" => {
            let doc: serde_json::Value = serde_json::from_str(content)
                .map_err(|_| invalid_structure("配置格式无效，请检查 TOML/JSON 语法"))?;
            doc.get("mcpServers")
                .and_then(|v| v.get(name))
                .is_some_and(|server| {
                    server.get("type").and_then(|v| v.as_str()) == Some("http")
                        && server.get("url").and_then(|v| v.as_str()) == Some(endpoint.as_str())
                        && server.get("command").is_none()
                        && server.get("args").is_none()
                        && server
                            .get("headers")
                            .and_then(|v| v.get("Authorization"))
                            .and_then(|v| v.as_str())
                            == Some(authorization.as_str())
                })
        }
        _ => false,
    };
    if owned {
        entry_fingerprint(content, client, name)
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod override_tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Project, HttpBinding) {
        let dir = tempfile::tempdir().unwrap();
        let binding = test_binding();
        let p = Project {
            previous_roots: vec![],
            id: uuid::Uuid::new_v4().to_string(),
            name: "配置测试".into(),
            root_path: dir.path().to_string_lossy().into(),
            canonical_path: dunce::canonicalize(dir.path())
                .unwrap()
                .to_string_lossy()
                .into(),
            notes: String::new(),
            auto_start: false,
            created_at: now(),
            updated_at: now(),
        };
        (dir, p, binding)
    }
    fn file_path(root: &Path, client: &str) -> std::path::PathBuf {
        let path = root.join(if client == "codex" {
            ".codex/config.toml"
        } else {
            ".mcp.json"
        });
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        path
    }
    fn overridden(
        p: &Project,
        binding: &HttpBinding,
        client: &str,
        mode: ConfigMode,
        content: Option<String>,
    ) -> Result<ConfigPreview> {
        preview_with_overrides(
            p,
            &[client.into()],
            "install",
            binding,
            &HashMap::new(),
            &[ConfigOverride {
                client: client.into(),
                mode,
                content,
            }],
        )
    }
    #[test]
    fn overwrite_shows_full_replacement_and_restore_does_not_claim_foreign_service() {
        for client in ["codex", "claude"] {
            let (dir, p, binding) = fixture();
            let path = file_path(dir.path(), client);
            let original = if client == "codex" {
                "# user comment\nmodel='original'\n[mcp_servers.codegraph]\ncommand='third-party'\n"
            } else {
                r#"{"unrelated":{"keep":true},"mcpServers":{"codegraph":{"command":"third-party"}}}"#
            };
            std::fs::write(&path, original).unwrap();
            let change = overridden(&p, &binding, client, ConfigMode::Overwrite, None).unwrap();
            assert_eq!(change.files[0].before, original);
            assert!(!change.files[0].after.contains("third-party"));
            assert!(!change.files[0].after.contains("unrelated"));
            assert!(!change.files[0].after.contains("model"));
            assert!(change.files[0].fingerprint.is_some());
            let applied = apply(&change, dir.path(), dir.path()).unwrap();
            let recovery =
                preview_restore(dir.path(), &applied.operation_id, &p, &binding).unwrap();
            assert!(recovery.files[0].fingerprint.is_none());
            apply(&recovery, dir.path(), dir.path()).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
    }
    #[test]
    fn edit_preserves_exact_content_and_only_claims_matching_project_binding() {
        for client in ["codex", "claude"] {
            let (dir, p, binding) = fixture();
            let path = file_path(dir.path(), client);
            let mut content = merge("", client, "codegraph", &p.id, &binding, false)
                .unwrap()
                .0;
            if client == "codex" {
                content = format!("# edited by user\nmodel='user-model'\n{content}")
                    .replace("tool_timeout_sec = 120", "tool_timeout_sec = 240");
            } else {
                let mut json: serde_json::Value = serde_json::from_str(&content).unwrap();
                json["unrelated"] = serde_json::json!({"setting":42});
                content = format!("  {}\n\n", serde_json::to_string(&json).unwrap());
            }
            let change = overridden(
                &p,
                &binding,
                client,
                ConfigMode::Edit,
                Some(content.clone()),
            )
            .unwrap();
            assert_eq!(change.files[0].after, content);
            assert!(change.files[0].fingerprint.is_some());
            apply(&change, dir.path(), dir.path()).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
            assert_eq!(
                status(&p, &binding)
                    .into_iter()
                    .find(|s| s.client == client)
                    .unwrap()
                    .state,
                "configured"
            );
            let foreign = content.replace(&binding.token, &"b".repeat(64));
            let change = overridden(
                &p,
                &binding,
                client,
                ConfigMode::Edit,
                Some(foreign.clone()),
            )
            .unwrap();
            assert!(change.files[0].fingerprint.is_none());
            apply(&change, dir.path(), dir.path()).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), foreign);
            assert_eq!(
                status(&p, &binding)
                    .into_iter()
                    .find(|s| s.client == client)
                    .unwrap()
                    .state,
                "repair"
            );
            let without = if client == "codex" {
                "model='manual-only'\n"
            } else {
                r#"{"unrelated":42}"#
            };
            let change =
                overridden(&p, &binding, client, ConfigMode::Edit, Some(without.into())).unwrap();
            assert!(change.files[0].fingerprint.is_none());
            apply(&change, dir.path(), dir.path()).unwrap();
            assert_eq!(
                status(&p, &binding)
                    .into_iter()
                    .find(|s| s.client == client)
                    .unwrap()
                    .state,
                "missing"
            );
        }
    }
    #[test]
    fn edit_rejects_invalid_syntax_and_mcp_structure_without_writing() {
        let (dir, p, binding) = fixture();
        for (client, candidates) in [
            (
                "codex",
                vec![
                    "[broken",
                    "mcp_servers=1",
                    "[mcp_servers]\ncodegraph='bad'",
                    "[mcp_servers.codegraph]\ncommand='x'\nargs=[1]",
                    "[mcp_servers.codegraph]\ncommand=1",
                    "[mcp_servers.codegraph]\ncommand='x'\nenv={TOKEN=4}",
                ],
            ),
            (
                "claude",
                vec![
                    "{broken",
                    "[]",
                    r#"{"mcpServers":[]}"#,
                    r#"{"mcpServers":{"codegraph":"bad"}}"#,
                    r#"{"mcpServers":{"codegraph":{"command":"x","args":[1]}}}"#,
                    r#"{"mcpServers":{"codegraph":{"command":"x","url":"https://example.test"}}}"#,
                ],
            ),
        ] {
            let path = file_path(dir.path(), client);
            std::fs::write(&path, "unchanged original bytes").unwrap();
            for candidate in candidates {
                assert!(
                    overridden(
                        &p,
                        &binding,
                        client,
                        ConfigMode::Edit,
                        Some(candidate.into())
                    )
                    .is_err(),
                    "accepted {candidate}"
                );
                assert_eq!(
                    std::fs::read_to_string(&path).unwrap(),
                    "unchanged original bytes"
                );
            }
        }
    }
    #[test]
    fn stale_preview_rejects_external_edit_and_new_preview_reads_latest_disk() {
        let (dir, p, binding) = fixture();
        let path = file_path(dir.path(), "claude");
        std::fs::write(&path, r#"{"original":1}"#).unwrap();
        let stale = preview(&p, &["claude".into()], "install", &binding, &HashMap::new()).unwrap();
        let external = r#"{"original":1,"external":{"keep":true}}"#;
        std::fs::write(&path, external).unwrap();
        assert_eq!(
            apply(&stale, dir.path(), dir.path()).err().unwrap().code,
            "CONFIG_CHANGED"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), external);
        let fresh = preview(&p, &["claude".into()], "install", &binding, &HashMap::new()).unwrap();
        assert_ne!(fresh.preview_id, stale.preview_id);
        assert_eq!(fresh.files[0].before, external);
        assert!(fresh.files[0].after.contains("external"));
        apply(&fresh, dir.path(), dir.path()).unwrap();
        let actual: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(actual["external"]["keep"], true);
    }
    #[test]
    fn overwrite_repairs_invalid_original_and_backup_restores_exact_broken_bytes() {
        for (client, original) in [("codex", "[broken"), ("claude", "{broken")] {
            let (dir, p, binding) = fixture();
            let path = file_path(dir.path(), client);
            std::fs::write(&path, original).unwrap();
            assert!(preview(&p, &[client.into()], "install", &binding, &HashMap::new()).is_err());
            let change = overridden(&p, &binding, client, ConfigMode::Overwrite, None).unwrap();
            let applied = apply(&change, dir.path(), dir.path()).unwrap();
            let recovery =
                preview_restore(dir.path(), &applied.operation_id, &p, &binding).unwrap();
            assert!(recovery.files[0].fingerprint.is_none());
            apply(&recovery, dir.path(), dir.path()).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
    }
    #[test]
    fn overrides_are_scoped_and_only_allowed_for_install() {
        let (_dir, p, binding) = fixture();
        let change = ConfigOverride {
            client: "claude".into(),
            mode: ConfigMode::Overwrite,
            content: None,
        };
        assert!(preview_with_overrides(
            &p,
            &["codex".into()],
            "install",
            &binding,
            &HashMap::new(),
            std::slice::from_ref(&change)
        )
        .is_err());
        assert!(preview_with_overrides(
            &p,
            &["claude".into()],
            "remove",
            &binding,
            &HashMap::new(),
            std::slice::from_ref(&change)
        )
        .is_err());
        assert!(preview_with_overrides(
            &p,
            &["claude".into()],
            "install",
            &binding,
            &HashMap::new(),
            &[change.clone(), change]
        )
        .is_err());
        assert!(overridden(&p, &binding, "claude", ConfigMode::Edit, None).is_err());
    }
}

#[cfg(test)]
fn test_binding() -> HttpBinding {
    HttpBinding {
        port: 41761,
        token: "a".repeat(64),
    }
}
#[cfg(test)]
mod http_config_tests {
    use super::*;
    fn project(root: &Path) -> Project {
        Project {
            previous_roots: vec![],
            id: uuid::Uuid::new_v4().to_string(),
            name: "HTTP".into(),
            root_path: root.to_string_lossy().into(),
            canonical_path: dunce::canonicalize(root).unwrap().to_string_lossy().into(),
            notes: String::new(),
            auto_start: false,
            created_at: now(),
            updated_at: now(),
        }
    }
    #[test]
    fn configuration_uses_direct_http_and_remains_identical_after_database_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path());
        let mut db = crate::persistence::open(dir.path()).unwrap();
        crate::persistence::save(&db, &p).unwrap();
        let binding = crate::persistence::ensure_http_binding(&mut db, &p.id).unwrap();
        let first = preview(
            &p,
            &["codex".into(), "claude".into()],
            "install",
            &binding,
            &HashMap::new(),
        )
        .unwrap();
        drop(db);
        let mut db = crate::persistence::open(dir.path()).unwrap();
        let restored = crate::persistence::ensure_http_binding(&mut db, &p.id).unwrap();
        let again = preview(
            &p,
            &["codex".into(), "claude".into()],
            "install",
            &restored,
            &HashMap::new(),
        )
        .unwrap();
        for (a, b) in first.files.iter().zip(&again.files) {
            assert!(a.after == b.after);
            assert!(!a.after.contains("command"));
            assert!(!a.after.contains("args"));
            assert!(a.after.contains(&binding.endpoint()));
            assert!(a.after.contains(&binding.authorization()));
            assert!(a.fingerprint.is_some());
        }
        let doc = first.files[0]
            .after
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(
            doc["mcp_servers"]["codegraph"]["url"].as_str(),
            Some(binding.endpoint().as_str())
        );
        assert!(
            doc["mcp_servers"]["codegraph"]["http_headers"]["Authorization"].as_str()
                == Some(binding.authorization().as_str())
        );
        let json: serde_json::Value = serde_json::from_str(&first.files[1].after).unwrap();
        assert_eq!(json["mcpServers"]["codegraph"]["type"], "http");
    }
    #[test]
    fn old_stdio_config_migrates_only_when_preview_is_applied() {
        for client in ["codex", "claude"] {
            for old_id_name in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let p = project(dir.path());
                let binding = test_binding();
                let name = if old_id_name {
                    format!("codegraph_project_{}", p.id.replace('-', ""))
                } else {
                    "codegraph".into()
                };
                let before = if client == "codex" {
                    format!("# retain\n[mcp_servers.{name}]\ncommand='cg-mcp-connector.exe'\nargs=['--project-id','{}']\n",p.id)
                } else {
                    serde_json::json!({"unrelated":true,"mcpServers":{name.clone():{"type":"stdio","command":"cg-mcp-connector.exe","args":["--project-id",p.id]}}}).to_string()
                };
                let path = dir.path().join(if client == "codex" {
                    ".codex/config.toml"
                } else {
                    ".mcp.json"
                });
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, &before).unwrap();
                assert_eq!(
                    status(&p, &binding)
                        .into_iter()
                        .find(|s| s.client == client)
                        .unwrap()
                        .state,
                    "repair"
                );
                let mut managed = HashMap::new();
                if old_id_name {
                    managed.insert(
                        client.into(),
                        entry_fingerprint(&before, client, &name).unwrap().unwrap(),
                    );
                }
                let change = preview(&p, &[client.into()], "install", &binding, &managed).unwrap();
                assert!(std::fs::read_to_string(&path).unwrap() == before);
                assert!(!change.files[0].after.contains("cg-mcp-connector"));
                assert!(change.files[0].after.contains(&binding.endpoint()));
                apply(&change, dir.path(), dir.path()).unwrap();
                assert_eq!(
                    status(&p, &binding)
                        .into_iter()
                        .find(|s| s.client == client)
                        .unwrap()
                        .state,
                    "configured"
                );
            }
        }
    }
    #[test]
    fn parse_errors_never_include_bearer_secret() {
        let binding = test_binding();
        let broken = format!(
            "[mcp_servers.codegraph]\nurl='{}'\nhttp_headers={{Authorization='{}'\n",
            binding.endpoint(),
            binding.authorization()
        );
        let error = validate_content(&broken, "codex").err().unwrap();
        assert!(!error.message.contains(&binding.token));
        assert!(!error.message.contains("Bearer"));
    }
}
