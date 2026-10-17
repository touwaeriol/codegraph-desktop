//! Per-engine tool filtering. Codex hides the tools an entry lists under
//! `disabled_tools`; Claude Code hides tools named by bare deny rules in the
//! project's `.claude/settings.json`. Both follow one disabled list per engine.
use crate::{commands, configuration, models::*, persistence, AppState};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use tauri::State;

const ENGINES: [&str; 4] = ["codegraph", "serena", "warpgrep", "rea"];
const DISABLED_SETTING: &str = "disabledTools";
const KNOWN_SETTING: &str = "engineTools";
/// Deny rules this app added to a project's Claude Code settings.
fn owned_setting(project_id: &str) -> String {
    format!("claudeDenyRules:{project_id}")
}

type Tools = BTreeMap<String, Vec<String>>;

fn load<T: serde::de::DeserializeOwned + Default>(state: &AppState, key: &str) -> Result<T> {
    Ok(persistence::setting(&state.db.lock().unwrap(), key)?
        .and_then(|saved| serde_json::from_str(&saved).ok())
        .unwrap_or_default())
}
fn store(state: &AppState, key: &str, value: &impl serde::Serialize) -> Result<()> {
    persistence::set_setting(
        &state.db.lock().unwrap(),
        key,
        &serde_json::to_string(value).unwrap(),
    )
}
fn engine(engine: &str) -> Result<()> {
    if ENGINES.contains(&engine) {
        Ok(())
    } else {
        Err(AppError::new(
            "INVALID_ENGINE",
            crate::i18n::tr("不支持的引擎", "Unknown engine"),
        ))
    }
}

/// Tools the user disabled for an engine, sorted.
pub fn disabled(state: &AppState, engine: &str) -> Result<Vec<String>> {
    Ok(load::<Tools>(state, DISABLED_SETTING)?
        .remove(engine)
        .unwrap_or_default())
}
/// Records the tools an engine offered, so settings can list them later.
pub fn remember(state: &AppState, engine: &str, tools: &[String]) -> Result<()> {
    self::engine(engine)?;
    let mut known: Tools = load(state, KNOWN_SETTING)?;
    let mut tools = tools.to_vec();
    tools.sort();
    tools.dedup();
    if known.get(engine) != Some(&tools) {
        known.insert(engine.into(), tools);
        store(state, KNOWN_SETTING, &known)?;
    }
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineTools {
    known: Tools,
    disabled: Tools,
}
#[tauri::command]
pub fn get_engine_tools(state: State<AppState>) -> Result<EngineTools> {
    Ok(EngineTools {
        known: load(&state, KNOWN_SETTING)?,
        disabled: load(&state, DISABLED_SETTING)?,
    })
}

/// Lists an engine's tools: REA from a temporary process, the other engines
/// from the first project whose instance is running.
#[tauri::command]
pub async fn refresh_engine_tools(
    state: State<'_, AppState>,
    engine: String,
) -> Result<Vec<String>> {
    self::engine(&engine)?;
    let tools = if engine == "rea" {
        crate::rea::probe(&state, std::env::temp_dir()).await?
    } else {
        let projects = persistence::list(&state.db.lock().unwrap())?;
        let mut found = None;
        for project in projects {
            if let Ok(tools) = commands::probe_tools(&state, &project.id, &engine).await {
                found = Some(tools);
                break;
            }
        }
        found.ok_or_else(|| {
            AppError::new(
                "SERVICE_NOT_RUNNING",
                crate::i18n::tr(
                    "没有运行中的实例，请先启动任一项目的该引擎",
                    "No instance is running. Start this engine for any project first",
                ),
            )
        })?
    };
    remember(&state, &engine, &tools)?;
    Ok(tools)
}

/// MCP tool names: 1–128 characters from letters, digits, `_`, `-` and `.`.
fn valid_tool(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisabledSaved {
    disabled: Vec<String>,
    #[serde(flatten)]
    resynced: commands::Resynced,
}
/// Saves an engine's disabled tools, rewrites the Codex entries this app
/// manages and updates Claude Code deny rules where the engine is configured.
#[tauri::command]
pub async fn save_disabled_tools(
    state: State<'_, AppState>,
    engine: String,
    tools: Vec<String>,
) -> Result<DisabledSaved> {
    save(&state, engine, tools).await
}
pub(crate) async fn save(
    state: &AppState,
    engine: String,
    tools: Vec<String>,
) -> Result<DisabledSaved> {
    self::engine(&engine)?;
    let tools = tools
        .iter()
        .map(|tool| tool.trim().to_owned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(tool) = tools.iter().find(|tool| !valid_tool(tool)) {
        return Err(AppError::new(
            "INVALID_TOOL",
            format!(
                "{}: {tool:?}",
                crate::i18n::tr("工具名称无效", "Invalid tool name")
            ),
        ));
    }
    let mut all: Tools = load(state, DISABLED_SETTING)?;
    let previous = all.remove(&engine).unwrap_or_default();
    if !tools.is_empty() {
        all.insert(engine.clone(), tools.clone());
    }
    store(state, DISABLED_SETTING, &all)?;
    if previous == tools {
        return Ok(DisabledSaved {
            disabled: tools,
            resynced: Default::default(),
        });
    }
    let mut resynced = commands::resync(state, &engine, |mut target| {
        target.disabled_tools.clone_from(&previous);
        target
    })
    .await?;
    let projects = persistence::list(&state.db.lock().unwrap())?;
    for project in projects {
        let lock = state.operation_lock(&project.id);
        let _guard = lock.lock().await;
        let label = commands::Resynced::label(&project, "claude");
        match sync_claude(state, &project) {
            Ok(true) => resynced.updated.push(label),
            Ok(false) => {}
            Err(error) => resynced.failed.push(format!("{label}: {}", error.message)),
        }
    }
    Ok(DisabledSaved {
        disabled: tools,
        resynced,
    })
}

pub fn claude_settings(project: &Project) -> PathBuf {
    Path::new(&project.canonical_path).join(".claude/settings.json")
}

/// Brings this app's deny rules in the project's Claude Code settings in line
/// with the disabled tools of every engine configured there. Rules the user
/// wrote are never removed. Returns whether the file changed.
pub fn sync_claude(state: &AppState, project: &Project) -> Result<bool> {
    let mut desired = BTreeSet::new();
    for engine in ENGINES {
        let Ok(binding) = commands::binding_for(state, &project.id, Some(engine)) else {
            continue;
        };
        if binding.disabled_tools.is_empty()
            || !configuration::status(project, &binding)
                .iter()
                .any(|status| status.client == "claude" && status.state == "configured")
        {
            continue;
        }
        for tool in &binding.disabled_tools {
            desired.insert(format!("mcp__{}__{tool}", binding.name));
        }
    }
    let key = owned_setting(&project.id);
    let owned: BTreeSet<String> = load(state, &key)?;
    let root = Path::new(&project.canonical_path);
    let path = claude_settings(project);
    configuration::safe_path(root, &path)?;
    let original = match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let (after, owned_next) = reconcile(original.as_deref(), &desired, &owned)?;
    if let Some(after) = &after {
        configuration::atomic_write(&path, after.as_bytes())?;
    }
    if owned_next != owned {
        store(state, &key, &owned_next)?;
    }
    Ok(after.is_some())
}

fn invalid_settings(message: &str) -> AppError {
    AppError::new(
        "CONFIG_PARSE_FAILED",
        format!(".claude/settings.json: {message}"),
    )
}
/// Returns the new file content, or `None` when nothing changes, and the
/// rules this app owns afterwards.
fn reconcile(
    original: Option<&[u8]>,
    desired: &BTreeSet<String>,
    owned: &BTreeSet<String>,
) -> Result<(Option<String>, BTreeSet<String>)> {
    let text = original
        .map(|bytes| {
            String::from_utf8(bytes.to_vec())
                .map_err(|_| invalid_settings("UTF-8"))
                .map(|text| text.trim_start_matches('\u{feff}').to_owned())
        })
        .transpose()?
        .unwrap_or_default();
    let mut doc: serde_json::Value = if text.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&text).map_err(|e| invalid_settings(&e.to_string()))?
    };
    let root = doc
        .as_object_mut()
        .ok_or_else(|| invalid_settings("top level must be an object"))?;
    let existing: BTreeSet<String> = match root.get("permissions").and_then(|p| p.get("deny")) {
        None => BTreeSet::new(),
        Some(deny) => deny
            .as_array()
            .ok_or_else(|| invalid_settings("permissions.deny must be an array"))?
            .iter()
            .filter_map(|rule| rule.as_str().map(str::to_owned))
            .collect(),
    };
    let stale: BTreeSet<String> = owned
        .iter()
        .filter(|rule| !desired.contains(*rule) && existing.contains(*rule))
        .cloned()
        .collect();
    let missing: Vec<&String> = desired
        .iter()
        .filter(|rule| !existing.contains(*rule))
        .collect();
    let owned_next = desired
        .iter()
        .filter(|rule| owned.contains(*rule) || !existing.contains(*rule))
        .cloned()
        .collect();
    if stale.is_empty() && missing.is_empty() {
        return Ok((None, owned_next));
    }
    let permissions = root
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| invalid_settings("permissions must be an object"))?;
    let deny = permissions
        .entry("deny")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .unwrap();
    deny.retain(|rule| !rule.as_str().is_some_and(|rule| stale.contains(rule)));
    deny.extend(missing.iter().map(|rule| serde_json::json!(rule)));
    if deny.is_empty() {
        permissions.remove("deny");
    }
    if permissions.is_empty() {
        root.remove("permissions");
    }
    Ok((
        Some(format!("{}\n", serde_json::to_string_pretty(&doc).unwrap())),
        owned_next,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(rules: &[&str]) -> BTreeSet<String> {
        rules.iter().map(|rule| rule.to_string()).collect()
    }
    fn deny(content: &str) -> Vec<String> {
        let doc: serde_json::Value = serde_json::from_str(content).unwrap();
        doc["permissions"]["deny"]
            .as_array()
            .map(|rules| {
                rules
                    .iter()
                    .map(|rule| rule.as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn deny_rules_are_added_and_removed_without_touching_user_rules() {
        let original = br#"{"model":"opus","permissions":{"allow":["Read"],"deny":["Bash(rm *)","mcp__rea__shared"]}}"#;
        let desired = set(&["mcp__rea__shared", "mcp__rea__hidden"]);
        let (after, owned) = reconcile(Some(original), &desired, &BTreeSet::new()).unwrap();
        let after = after.unwrap();
        assert_eq!(
            deny(&after),
            ["Bash(rm *)", "mcp__rea__shared", "mcp__rea__hidden"]
        );
        assert_eq!(owned, set(&["mcp__rea__hidden"]));
        let doc: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(doc["model"], "opus");
        assert_eq!(doc["permissions"]["allow"], serde_json::json!(["Read"]));
        assert!(after.starts_with("{\n  \"model\""));

        let (unchanged, kept) = reconcile(Some(after.as_bytes()), &desired, &owned).unwrap();
        assert!(unchanged.is_none());
        assert_eq!(kept, owned);

        let (cleared, owned) = reconcile(Some(after.as_bytes()), &BTreeSet::new(), &owned).unwrap();
        assert_eq!(deny(&cleared.unwrap()), ["Bash(rm *)", "mcp__rea__shared"]);
        assert!(owned.is_empty());
    }

    #[test]
    fn rules_the_user_removed_are_no_longer_owned() {
        let owned = set(&["mcp__rea__gone"]);
        let (after, owned) = reconcile(Some(b"{}"), &BTreeSet::new(), &owned).unwrap();
        assert!(after.is_none());
        assert!(owned.is_empty());
    }

    #[test]
    fn files_are_created_only_when_needed_and_emptied_sections_are_dropped() {
        let (nothing, _) = reconcile(None, &BTreeSet::new(), &BTreeSet::new()).unwrap();
        assert!(nothing.is_none());
        let desired = set(&["mcp__serena__rename_symbol"]);
        let (created, owned) = reconcile(None, &desired, &BTreeSet::new()).unwrap();
        let created = created.unwrap();
        assert_eq!(deny(&created), ["mcp__serena__rename_symbol"]);
        let (removed, _) = reconcile(Some(created.as_bytes()), &BTreeSet::new(), &owned).unwrap();
        assert_eq!(removed.unwrap(), "{}\n");
    }

    #[test]
    fn malformed_settings_are_reported_instead_of_overwritten() {
        let desired = set(&["mcp__rea__x"]);
        for content in [
            "{",
            "[]",
            r#"{"permissions":[]}"#,
            r#"{"permissions":{"deny":"x"}}"#,
        ] {
            let error =
                reconcile(Some(content.as_bytes()), &desired, &BTreeSet::new()).unwrap_err();
            assert_eq!(error.code, "CONFIG_PARSE_FAILED", "{content}");
        }
    }

    #[test]
    fn tool_names_follow_the_mcp_character_set() {
        for name in ["find_symbol", "binary.session", "get-file", "A1"] {
            assert!(valid_tool(name), "{name}");
        }
        for name in ["", "with space", "mcp(x)", "a*", &"x".repeat(129)] {
            assert!(!valid_tool(name), "{name}");
        }
    }
}
