use crate::models::*;
use crate::persistence::HttpBinding;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, io::Write, path::Path};
pub trait HttpTarget {
    fn name(&self) -> &str;
    fn endpoint(&self) -> String;
    fn auth(&self) -> Option<String>;
}
impl HttpTarget for HttpBinding {
    fn name(&self) -> &str {
        "codegraph"
    }
    fn endpoint(&self) -> String {
        self.endpoint()
    }
    fn auth(&self) -> Option<String> {
        Some(self.authorization())
    }
}
pub struct Target {
    pub name: String,
    pub endpoint: String,
    pub authorization: Option<String>,
}
impl HttpTarget for Target {
    fn name(&self) -> &str {
        &self.name
    }
    fn endpoint(&self) -> String {
        self.endpoint.clone()
    }
    fn auth(&self) -> Option<String> {
        self.authorization.clone()
    }
}
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
    pub ownership: HashMap<String, HashMap<String, Option<String>>>,
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
    #[serde(default)]
    service_names: Vec<String>,
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
    binding: &(impl HttpTarget + ?Sized),
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
            if let Some(auth) = binding.auth() {
                let mut headers = toml_edit::Table::new();
                headers["Authorization"] = toml_edit::value(auth);
                t["http_headers"] = toml_edit::Item::Table(headers);
            }
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
            servers.insert(name.into(), {
                let mut value = serde_json::json!({"type":"http","url":binding.endpoint()});
                if let Some(auth) = binding.auth() {
                    value["headers"] = serde_json::json!({"Authorization":auth});
                }
                value
            });
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
    binding: &impl HttpTarget,
    managed: &HashMap<String, String>,
) -> Result<ConfigPreview> {
    preview_with_overrides(p, clients, action, binding, managed, &[])
}
pub fn preview_with_overrides(
    p: &Project,
    clients: &[String],
    action: &str,
    binding: &impl HttpTarget,
    managed: &HashMap<String, String>,
    overrides: &[ConfigOverride],
) -> Result<ConfigPreview> {
    preview_from(p, clients, action, binding, managed, overrides, None)
}
fn preview_from(
    p: &Project,
    clients: &[String],
    action: &str,
    binding: &dyn HttpTarget,
    managed: &HashMap<String, String>,
    overrides: &[ConfigOverride],
    inputs: Option<&HashMap<String, Option<Vec<u8>>>>,
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
    let name = binding.name().to_string();
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
        let original = match inputs {
            Some(inputs) => inputs
                .get(client)
                .cloned()
                .ok_or_else(|| AppError::new("INVALID_CLIENT", "Missing preview input"))?,
            None => read(&path)?,
        };
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
        let legacy_hash = if binding.name() == "codegraph" {
            entry_fingerprint(&before, client, &legacy_name)?
        } else {
            None
        };
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
        ownership: HashMap::new(),
        validated_root: None,
        preview_id: uuid::Uuid::new_v4().to_string(),
        project_id: p.id.clone(),
        service_name: name,
        files,
    })
}
pub fn preview_many(
    p: &Project,
    clients: &[String],
    action: &str,
    bindings: &[Target],
    managed: &HashMap<String, HashMap<String, String>>,
    overrides: &[ConfigOverride],
) -> Result<ConfigPreview> {
    if bindings.is_empty() {
        return Err(AppError::new("INVALID_ENGINE", "Select at least one MCP"));
    }
    let mut combined: Option<ConfigPreview> = None;
    let empty = HashMap::new();
    for binding in bindings {
        let inputs = combined.as_ref().map(|preview| {
            preview
                .files
                .iter()
                .map(|f| {
                    (
                        f.client.clone(),
                        f.restore_bytes
                            .clone()
                            .unwrap_or_else(|| Some(f.after.as_bytes().to_vec())),
                    )
                })
                .collect()
        });
        // Overwrite clears the file once, then merges every selected engine into it.
        let choices = overrides
            .iter()
            .map(|change| ConfigOverride {
                mode: if combined.is_some() && change.mode == ConfigMode::Overwrite {
                    ConfigMode::Merge
                } else {
                    change.mode
                },
                ..change.clone()
            })
            .collect::<Vec<_>>();
        let mut next = preview_from(
            p,
            clients,
            action,
            binding,
            managed.get(&binding.name).unwrap_or(&empty),
            &choices,
            inputs.as_ref(),
        )?;
        let fingerprints = next
            .files
            .iter()
            .map(|f| (f.client.clone(), f.fingerprint.clone()))
            .collect();
        if let Some(preview) = &mut combined {
            for file in &mut preview.files {
                let next_file = next.files.iter().find(|f| f.client == file.client).unwrap();
                file.after.clone_from(&next_file.after);
                file.conflict |= next_file.conflict;
                file.restore_bytes = if action == "remove" && file.after == file.before {
                    Some(file.original.clone())
                } else {
                    None
                };
            }
            preview.ownership.insert(binding.name.clone(), fingerprints);
            preview.service_name.push_str(", ");
            preview.service_name.push_str(&binding.name);
        } else {
            next.ownership.insert(binding.name.clone(), fingerprints);
            combined = Some(next);
        }
    }
    Ok(combined.unwrap())
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
        service_names: preview.ownership.keys().cloned().collect(),
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
                    message: crate::i18n::message("写入失败后恢复原文件"),
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
                        message: crate::i18n::message("未修改"),
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
                message: crate::i18n::message(if f.original.as_deref() == desired(f) {
                    "内容未变化"
                } else {
                    "配置已写入"
                }),
            })
            .collect(),
    })
}
pub fn status(p: &Project, binding: &impl HttpTarget) -> Vec<ConfigStatus> {
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
                let (name, id) = (binding.name().to_string(), &p.id);
                validate_content(&before, client)?;
                if entry_fingerprint(&before, client, &name)?.is_some() {
                    item.state =
                        if managed_fingerprint(&before, client, &name, id, binding)?.is_some() {
                            "configured"
                        } else {
                            "repair"
                        }
                        .into();
                } else if binding.name() == "codegraph" && entry_fingerprint(
                    &before,
                    client,
                    &format!("codegraph_project_{}", p.id.replace('-', "")),
                )?
                .is_some()
                {
                    item.state = "repair".into();
                    item.message = Some(crate::i18n::message(
                        "检测到旧连接器配置，请重新预览并应用 HTTP 配置",
                    ));
                }
                Ok(())
            })();
            if let Err(e) = check {
                item.state = "parseError".into();
                item.message = Some(e.message);
            }
            if item.state == "repair" && item.message.is_none() {
                item.message = Some(crate::i18n::tr(
                    "连接地址或凭据与当前项目不一致，请预览并更新配置",
                    "Connection settings differ from this project. Preview and update the configuration",
                ).into());
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
            ownership: HashMap::new(),
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
pub fn backup_services(data: &Path, operation_id: &str) -> Result<Vec<String>> {
    uuid::Uuid::parse_str(operation_id).map_err(|e| AppError::new("INVALID_OPERATION", e))?;
    let manifest: Backup = serde_json::from_slice(&std::fs::read(
        data.join("backups")
            .join(operation_id)
            .join("manifest.json"),
    )?)
    .map_err(|_| AppError::new("BACKUP_CORRUPT", "备份清单格式无效"))?;
    Ok(if manifest.service_names.is_empty() {
        vec![manifest.service_name]
    } else {
        manifest.service_names
    })
}
#[cfg(test)]
pub fn preview_restore(
    data: &Path,
    operation_id: &str,
    p: &Project,
    binding: &impl HttpTarget,
) -> Result<ConfigPreview> {
    preview_restore_many(data, operation_id, p, &[binding])
}
pub fn preview_restore_many(
    data: &Path,
    operation_id: &str,
    p: &Project,
    bindings: &[&dyn HttpTarget],
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
    let mut ownership: HashMap<String, HashMap<String, Option<String>>> = HashMap::new();
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
        let mut fingerprint = None;
        for binding in bindings {
            let name = if manifest.service_names.is_empty() {
                &manifest.service_name
            } else {
                binding.name()
            };
            let restored = if file.original.is_some() {
                managed_fingerprint(&after, &file.client, name, &p.id, *binding).unwrap_or(None)
            } else {
                None
            };
            fingerprint.clone_from(&restored);
            ownership
                .entry(name.into())
                .or_default()
                .insert(file.client.clone(), restored);
        }
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
        ownership,
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
            ownership: HashMap::new(),
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
            ownership: HashMap::new(),
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
            ownership: HashMap::new(),
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
    binding: &(impl HttpTarget + ?Sized),
) -> Result<Option<String>> {
    let content = content.trim_start_matches('\u{feff}');
    let endpoint = binding.endpoint();
    let authorization = binding.auth();
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
                            == authorization.as_deref()
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
                            == authorization.as_deref()
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
    #[test]
    fn batch_engines_merge_overwrite_edit_and_restore_together() {
        for mode in [ConfigMode::Merge, ConfigMode::Overwrite, ConfigMode::Edit] {
            let (dir, p, cg) = fixture();
            let data = tempfile::tempdir().unwrap();
            let bindings = vec![
                Target {
                    name: "codegraph".into(),
                    endpoint: cg.endpoint(),
                    authorization: Some(cg.authorization()),
                },
                Target {
                    name: "serena".into(),
                    endpoint: "http://127.0.0.1:58310/mcp".into(),
                    authorization: None,
                },
            ];
            let clients: Vec<String> = vec!["codex".into(), "claude".into()];
            let originals = [
                "# preserved\nmodel = 'existing'\n[mcp_servers.other]\ncommand = 'other'\n",
                "{\"mcpServers\":{\"other\":{\"command\":\"other\"}}}",
            ];
            for (client, before) in clients.iter().zip(originals) {
                std::fs::write(file_path(dir.path(), client), before).unwrap();
            }
            let generated =
                preview_many(&p, &clients, "install", &bindings, &HashMap::new(), &[]).unwrap();
            let choices = generated
                .files
                .iter()
                .map(|f| ConfigOverride {
                    client: f.client.clone(),
                    mode,
                    content: (mode == ConfigMode::Edit).then(|| f.after.clone()),
                })
                .collect::<Vec<_>>();
            let preview = preview_many(
                &p,
                &clients,
                "install",
                &bindings,
                &HashMap::new(),
                &choices,
            )
            .unwrap();
            assert_eq!(preview.files.len(), 2);
            assert_eq!(preview.ownership.len(), 2);
            for (f, before) in preview.files.iter().zip(originals) {
                assert_eq!(f.before, before);
                for binding in &bindings {
                    assert!(managed_fingerprint(
                        &f.after,
                        &f.client,
                        &binding.name,
                        &p.id,
                        binding
                    )
                    .unwrap()
                    .is_some());
                    assert!(preview.ownership[&binding.name][&f.client].is_some());
                }
                assert_eq!(
                    entry_fingerprint(&f.after, &f.client, "other")
                        .unwrap()
                        .is_some(),
                    mode != ConfigMode::Overwrite
                );
                // Preview generation is read-only.
                assert_eq!(std::fs::read_to_string(&f.path).unwrap(), before);
            }
            let written = apply(&preview, dir.path(), data.path()).unwrap();
            assert!(written.files.iter().all(|f| f.status == "success"));
            let mut names = backup_services(data.path(), &written.operation_id).unwrap();
            names.sort();
            assert_eq!(names, ["codegraph", "serena"]);
            let refs = bindings
                .iter()
                .map(|b| b as &dyn HttpTarget)
                .collect::<Vec<_>>();
            let restore =
                preview_restore_many(data.path(), &written.operation_id, &p, &refs).unwrap();
            assert_eq!(restore.ownership.len(), 2);
            assert!(restore
                .ownership
                .values()
                .all(|files| files.values().all(Option::is_none)));
            apply(&restore, dir.path(), data.path()).unwrap();
            for (client, before) in clients.iter().zip(originals) {
                assert_eq!(
                    std::fs::read_to_string(file_path(dir.path(), client)).unwrap(),
                    before
                );
            }
        }
    }
    #[test]
    fn batch_engines_remove_conflicts_and_failure_do_not_partially_apply() {
        let (dir, p, cg) = fixture();
        let data = tempfile::tempdir().unwrap();
        let bindings = vec![
            Target {
                name: "codegraph".into(),
                endpoint: cg.endpoint(),
                authorization: Some(cg.authorization()),
            },
            Target {
                name: "serena".into(),
                endpoint: "http://127.0.0.1:58310/mcp".into(),
                authorization: None,
            },
        ];
        let clients: Vec<String> = vec!["codex".into(), "claude".into()];
        let preview =
            preview_many(&p, &clients, "install", &bindings, &HashMap::new(), &[]).unwrap();
        let failed = apply_with(&preview, dir.path(), data.path(), Some(1)).unwrap();
        assert!(failed.files.iter().any(|f| f.status == "rolledBack"));
        assert!(preview.files.iter().all(|f| !Path::new(&f.path).exists()));
        apply(&preview, dir.path(), data.path()).unwrap();
        let managed = preview
            .ownership
            .iter()
            .map(|(engine, files)| {
                (
                    engine.clone(),
                    files
                        .iter()
                        .filter_map(|(client, fp)| fp.clone().map(|fp| (client.clone(), fp)))
                        .collect(),
                )
            })
            .collect();
        let remove = preview_many(&p, &clients, "remove", &bindings, &managed, &[]).unwrap();
        for f in &remove.files {
            for name in ["codegraph", "serena"] {
                assert!(entry_fingerprint(&f.after, &f.client, name)
                    .unwrap()
                    .is_none());
            }
        }
        let changed = format!("{}\n# external", preview.files[0].after);
        let removed = apply(&remove, dir.path(), data.path()).unwrap();
        let refs = bindings
            .iter()
            .map(|b| b as &dyn HttpTarget)
            .collect::<Vec<_>>();
        let restore = preview_restore_many(data.path(), &removed.operation_id, &p, &refs).unwrap();
        assert!(restore
            .ownership
            .values()
            .all(|files| files.values().all(Option::is_some)));
        apply(&restore, dir.path(), data.path()).unwrap();
        std::fs::write(&preview.files[0].path, &changed).unwrap();
        assert_eq!(
            apply(&remove, dir.path(), data.path()).err().unwrap().code,
            "CONFIG_CHANGED"
        );
        assert_eq!(
            std::fs::read_to_string(&preview.files[1].path).unwrap(),
            preview.files[1].after
        );
        std::fs::write(
            &preview.files[0].path,
            preview.files[0].after.replace("58310", "58311"),
        )
        .unwrap();
        assert_eq!(
            preview_many(&p, &clients, "remove", &bindings, &managed, &[])
                .err()
                .unwrap()
                .code,
            "CONFIG_CHANGED"
        );
        // Manual edits only claim entries that still match each engine's binding.
        let edited = preview_many(
            &p,
            &clients,
            "install",
            &bindings,
            &HashMap::new(),
            &clients
                .iter()
                .map(|client| ConfigOverride {
                    client: client.clone(),
                    mode: ConfigMode::Edit,
                    content: Some(
                        if client == "codex" {
                            "model = 'manual'"
                        } else {
                            "{}"
                        }
                        .into(),
                    ),
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(edited
            .ownership
            .values()
            .all(|files| files.values().all(Option::is_none)));
    }
    #[test]
    fn serena_configuration_preserves_codegraph_and_tracks_separate_ownership() {
        let (_dir, p, codegraph) = fixture();
        let data = tempfile::tempdir().unwrap();
        let clients: Vec<String> = vec!["codex".into(), "claude".into()];
        let cg = preview(&p, &clients, "install", &codegraph, &HashMap::new()).unwrap();
        apply(&cg, Path::new(&p.canonical_path), data.path()).unwrap();
        let serena = Target {
            name: "serena".into(),
            endpoint: "http://127.0.0.1:58310/mcp".into(),
            authorization: None,
        };
        let change = preview(&p, &clients, "install", &serena, &HashMap::new()).unwrap();
        let owned: HashMap<_, _> = change
            .files
            .iter()
            .map(|f| (f.client.clone(), f.fingerprint.clone().unwrap()))
            .collect();
        for (before, after) in cg.files.iter().zip(&change.files) {
            assert_eq!(
                entry_fingerprint(&before.after, &before.client, "codegraph").unwrap(),
                entry_fingerprint(&after.after, &after.client, "codegraph").unwrap()
            );
            assert!(after.after.contains(&serena.endpoint));
        }
        let result = apply(&change, Path::new(&p.canonical_path), data.path()).unwrap();
        assert!(status(&p, &serena).iter().all(|s| s.state == "configured"));
        assert!(status(&p, &codegraph)
            .iter()
            .all(|s| s.state == "configured"));
        let restore = preview_restore(data.path(), &result.operation_id, &p, &serena).unwrap();
        assert_eq!(restore.service_name, "serena");
        assert!(restore.files.iter().all(|f| f.fingerprint.is_none()));
        let remove = preview(&p, &clients, "remove", &serena, &owned).unwrap();
        for f in &remove.files {
            assert!(entry_fingerprint(&f.after, &f.client, "serena")
                .unwrap()
                .is_none());
            assert!(entry_fingerprint(&f.after, &f.client, "codegraph")
                .unwrap()
                .is_some());
        }
        // A hand-edited service must not be removed using previous ownership.
        std::fs::write(
            &change.files[0].path,
            change.files[0].after.replace("58310", "58311"),
        )
        .unwrap();
        assert!(preview(&p, &clients, "remove", &serena, &owned).is_err());
        assert!(apply(&remove, Path::new(&p.canonical_path), data.path()).is_err());
    }
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
        binding: &impl HttpTarget,
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
        project_id: "test-project".into(),
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
            assert!(a.after.contains(&binding.auth().unwrap_or_default()));
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
                == Some(binding.auth().unwrap_or_default().as_str())
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
            binding.auth().unwrap_or_default()
        );
        let error = validate_content(&broken, "codex").err().unwrap();
        assert!(!error.message.contains(&binding.token));
        assert!(!error.message.contains("Bearer"));
    }
}

/// Prepare only an unchanged, previously owned HTTP entry. Never create a client file.
pub fn shared_http_migration_preview(
    p: &Project,
    client: &str,
    owned: &str,
    old_port: u16,
    binding: &HttpBinding,
) -> Result<Option<ConfigPreview>> {
    let relative = match client {
        "codex" => ".codex/config.toml",
        "claude" => ".mcp.json",
        _ => return Ok(None),
    };
    let path = Path::new(&p.canonical_path).join(relative);
    safe_path(Path::new(&p.canonical_path), &path)?;
    let Some(original) = read(&path)? else {
        return Ok(None);
    };
    let before = String::from_utf8(original.clone())
        .map_err(|_| AppError::new("CONFIG_PARSE_FAILED", "Invalid UTF-8 configuration"))?;
    if entry_fingerprint(&before, client, "codegraph")?.as_deref() != Some(owned) {
        return Ok(None);
    }
    let old_url = format!("http://127.0.0.1:{old_port}/mcp");
    let text = before.trim_start_matches('\u{feff}');
    let after = if client == "codex" {
        let mut doc = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| AppError::new("CONFIG_PARSE_FAILED", "Invalid TOML configuration"))?;
        let entry = &mut doc["mcp_servers"]["codegraph"];
        if entry.get("url").and_then(|v| v.as_str()) != Some(old_url.as_str())
            || entry
                .get("http_headers")
                .and_then(|v| v.get("Authorization"))
                .and_then(|v| v.as_str())
                != Some(binding.authorization().as_str())
            || entry.get("command").is_some()
            || entry.get("args").is_some()
        {
            return Ok(None);
        }
        entry["url"] = toml_edit::value(binding.endpoint());
        doc.to_string()
    } else {
        let mut doc: serde_json::Value = serde_json::from_str(text)
            .map_err(|_| AppError::new("CONFIG_PARSE_FAILED", "Invalid JSON configuration"))?;
        let entry = &mut doc["mcpServers"]["codegraph"];
        if entry.get("url").and_then(|v| v.as_str()) != Some(old_url.as_str())
            || entry.get("type").and_then(|v| v.as_str()) != Some("http")
            || entry
                .get("headers")
                .and_then(|v| v.get("Authorization"))
                .and_then(|v| v.as_str())
                != Some(binding.authorization().as_str())
            || entry.get("command").is_some()
            || entry.get("args").is_some()
        {
            return Ok(None);
        }
        entry["url"] = binding.endpoint().into();
        format!("{}\n", serde_json::to_string_pretty(&doc).unwrap())
    };
    let after = if before.starts_with('\u{feff}') {
        format!("\u{feff}{after}")
    } else {
        after
    };
    let fingerprint = entry_fingerprint(&after, client, "codegraph")?;
    Ok(Some(ConfigPreview {
        ownership: HashMap::new(),
        validated_root: None,
        preview_id: uuid::Uuid::new_v4().to_string(),
        project_id: p.id.clone(),
        service_name: "codegraph".into(),
        files: vec![ConfigFile {
            client: client.into(),
            path: path.to_string_lossy().into(),
            before,
            after,
            existed: true,
            conflict: false,
            original: Some(original),
            fingerprint,
            restore_bytes: None,
        }],
    }))
}
