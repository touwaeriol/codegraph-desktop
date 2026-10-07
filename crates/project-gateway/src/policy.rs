use rmcp::model::{CallToolRequestParam, ErrorData, Tool};
use serde_json::{json, Map, Value};
use std::path::{Component, Path};

pub(crate) const UPSTREAM_TOOLS: &str = "explore,search,callers,impact";
const KINDS: &[&str] = &[
    "function",
    "method",
    "class",
    "interface",
    "type",
    "variable",
    "route",
    "component",
];

pub(crate) fn tools() -> Vec<Tool> {
    [
        (
            "codegraph_explore",
            "Locate relevant symbols in this project. By default returns names, signatures and file:line locations, NOT source bodies. Use callers/impact for relationships and LSP for precise source reads. Only includeSource=true explicitly requests source excerpts. maxChars bounds the entire response; truncated results are not complete file reads.",
            json!({
                "query": {"type":"string", "minLength":1, "maxLength":10000},
                "includeSource": {"type":"boolean", "default":false},
                "maxFiles": {"type":"integer", "minimum":1, "maximum":12, "default":3, "description":"Source mode only: maximum source files, not an output-size budget."}
            }),
            "query",
        ),
        (
            "codegraph_search",
            "Find symbols by name or code terms. Returns names, signatures and file:line locations, without source bodies. Narrow query/kind to reduce results; use LSP to read a selected symbol.",
            json!({
                "query": {"type":"string", "minLength":1, "maxLength":10000},
                "kind": {"type":"string", "enum":KINDS},
                "limit": {"type":"integer", "minimum":1, "maximum":100, "default":10}
            }),
            "query",
        ),
        (
            "codegraph_callers",
            "Find direct callers of a symbol, with locations and no source bodies. An optional existing project-relative file disambiguates the symbol; it is not a security boundary. Truncated results are not the complete caller set.",
            json!({
                "symbol": {"type":"string", "minLength":1, "maxLength":10000},
                "file": {"type":"string", "description":"Existing project-relative file; absolute paths and parent traversal are rejected."},
                "limit": {"type":"integer", "minimum":1, "maximum":100, "default":20}
            }),
            "symbol",
        ),
        (
            "codegraph_impact",
            "Find symbols potentially affected by a change, with locations and no source bodies. Graph relationships are not proof of exhaustive runtime impact. Narrow the symbol/file/depth when the response is truncated.",
            json!({
                "symbol": {"type":"string", "minLength":1, "maxLength":10000},
                "file": {"type":"string", "description":"Existing project-relative file; absolute paths and parent traversal are rejected."},
                "depth": {"type":"integer", "minimum":1, "maximum":5, "default":2}
            }),
            "symbol",
        ),
    ]
    .into_iter()
    .map(|(name, description, mut properties, required)| {
        properties["projectPath"] = json!({"type":"string", "description":"Optional identity check: must resolve to this gateway's bound project, never another project."});
        properties["maxChars"] = json!({"type":"integer", "minimum":512, "maximum":24000, "default":6000, "description":"Desktop response budget in Unicode characters, including serialized result metadata and truncation notices. Not an upstream CodeGraph option."});
        serde_json::from_value(json!({
            "name":name,
            "description":description,
            "inputSchema":{"type":"object", "properties":properties, "required":[required], "additionalProperties":false},
            "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }))
        .expect("static tool schema")
    })
    .collect()
}

fn fields(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "codegraph_explore" => Some(&["query", "maxFiles", "projectPath"]),
        "codegraph_search" => Some(&["query", "kind", "limit", "projectPath"]),
        "codegraph_callers" => Some(&["symbol", "file", "limit", "projectPath"]),
        "codegraph_impact" => Some(&["symbol", "file", "depth", "projectPath"]),
        _ => None,
    }
}

pub(crate) fn compatible(tool: &Tool) -> bool {
    let Some(allowed) = fields(&tool.name) else {
        return false;
    };
    let Some(properties) = tool
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
    else {
        return false;
    };
    let target = if tool.name == "codegraph_explore" || tool.name == "codegraph_search" {
        "query"
    } else {
        "symbol"
    };
    properties.keys().all(|key| allowed.contains(&key.as_str()))
        && [target, "projectPath"].iter().all(|key| {
            properties
                .get(*key)
                .and_then(|v| v.get("type"))
                .and_then(Value::as_str)
                == Some("string")
        })
        && tool
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .is_none_or(|required| {
                required
                    .iter()
                    .all(|key| key.as_str().is_some_and(|key| allowed.contains(&key)))
            })
}

pub(crate) fn check_index(root: &Path) -> Result<(), ErrorData> {
    let index = dunce::canonicalize(root.join(".codegraph"))
        .map_err(|_| invalid("INDEX_REQUIRED: the bound project's index is missing"))?;
    let database = dunce::canonicalize(index.join("codegraph.db"))
        .map_err(|_| invalid("INDEX_REQUIRED: the bound project's database is missing"))?;
    if !index.is_dir()
        || !index.starts_with(root)
        || !database.is_file()
        || !database.starts_with(root)
    {
        return Err(invalid(
            "PROJECT_SCOPE_VIOLATION: index is outside the project",
        ));
    }
    // Upstream walks to a parent index when this database has no nodes table.
    let db =
        rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| invalid("INDEX_REQUIRED: unable to open the bound project's database"))?;
    db.busy_timeout(std::time::Duration::from_millis(250))
        .map_err(|_| invalid("INDEX_REQUIRED: unable to validate the bound project's database"))?;
    let valid = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='nodes')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|_| invalid("INDEX_REQUIRED: unable to validate the bound project's database"))?;
    if !valid {
        return Err(invalid(
            "INDEX_REQUIRED: the bound project's index schema is missing",
        ));
    }
    Ok(())
}

fn invalid(message: &str) -> ErrorData {
    ErrorData::invalid_params(message.to_owned(), None)
}

fn number(
    args: &mut Map<String, Value>,
    key: &str,
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64, ErrorData> {
    let value = match args.get(key) {
        None => default,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| invalid(&format!("{key} must be an integer")))?,
    };
    if !(min..=max).contains(&value) {
        return Err(invalid(&format!("{key} must be between {min} and {max}")));
    }
    args.insert(key.into(), json!(value));
    Ok(value)
}

pub(crate) fn prepare(
    mut request: CallToolRequestParam,
    root: Option<&Path>,
) -> Result<(CallToolRequestParam, usize), ErrorData> {
    let allowed = fields(&request.name).ok_or_else(|| invalid("Tool is not approved"))?;
    let args = request.arguments.get_or_insert_with(Default::default);
    if args.keys().any(|key| {
        !allowed.contains(&key.as_str())
            && key != "maxChars"
            && !(request.name == "codegraph_explore" && key == "includeSource")
    }) {
        return Err(invalid("Unknown argument"));
    }
    let key = if request.name == "codegraph_explore" || request.name == "codegraph_search" {
        "query"
    } else {
        "symbol"
    };
    let text = args
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(&format!("{key} must be a string")))?;
    if text.trim().is_empty() || text.chars().count() > 10000 {
        return Err(invalid(
            "Query or symbol must contain 1 to 10000 characters",
        ));
    }
    let budget = number(args, "maxChars", 6000, 512, 24000)? as usize;
    args.remove("maxChars");
    if let Some(root) = root {
        if let Some(path) = args.get("projectPath") {
            let path = path
                .as_str()
                .ok_or_else(|| invalid("projectPath must be a path"))?;
            if dunce::canonicalize(path).ok().as_deref() != Some(root) {
                return Err(invalid("PROJECT_SCOPE_VIOLATION"));
            }
        }
        check_index(root)?;
        args.insert("projectPath".into(), json!(root));
    }
    if let Some(file) = args.get("file") {
        let file = file
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| invalid("file must be a project-relative path"))?
            .replace('\\', "/");
        let path = Path::new(&file);
        if path.is_absolute()
            || file.contains(':')
            || path.components().any(|part| {
                matches!(
                    part,
                    Component::ParentDir | Component::Prefix(_) | Component::RootDir
                )
            })
        {
            return Err(invalid("PROJECT_SCOPE_VIOLATION"));
        }
        if let Some(root) = root {
            let canonical = dunce::canonicalize(root.join(path))
                .map_err(|_| invalid("file does not exist in this project"))?;
            if !canonical.is_file() || !canonical.starts_with(root) {
                return Err(invalid("PROJECT_SCOPE_VIOLATION"));
            }
        }
        args.insert("file".into(), json!(file));
    }
    match request.name.as_ref() {
        "codegraph_explore" => {
            let source = match args.remove("includeSource") {
                None => false,
                Some(Value::Bool(value)) => value,
                _ => return Err(invalid("includeSource must be a boolean")),
            };
            number(args, "maxFiles", 3, 1, 12)?;
            if !source {
                request.name = "codegraph_search".into();
                args.remove("maxFiles");
                args.insert("limit".into(), json!(10));
            }
        }
        "codegraph_search" => {
            if let Some(kind) = args.get("kind") {
                if !kind.as_str().is_some_and(|kind| KINDS.contains(&kind)) {
                    return Err(invalid("Unsupported symbol kind"));
                }
            }
            number(args, "limit", 10, 1, 100)?;
        }
        "codegraph_callers" => {
            number(args, "limit", 20, 1, 100)?;
        }
        "codegraph_impact" => {
            number(args, "depth", 2, 1, 5)?;
        }
        _ => unreachable!(),
    }
    Ok((request, budget))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(name: &str, args: Value) -> CallToolRequestParam {
        serde_json::from_value(json!({"name":name,"arguments":args})).unwrap()
    }
    #[test]
    fn exploration_is_source_free_unless_explicitly_enabled() {
        for extra in [
            json!({}),
            json!({"includeSource":false}),
            json!({"maxFiles":1}),
        ] {
            let mut args = json!({"query":"start", "maxChars":512});
            args.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let (call, budget) = prepare(request("codegraph_explore", args), None).unwrap();
            assert_eq!(call.name, "codegraph_search");
            assert_eq!(
                call.arguments.unwrap(),
                json!({"query":"start","limit":10})
                    .as_object()
                    .unwrap()
                    .clone()
            );
            assert_eq!(budget, 512);
        }
        let (call, budget) = prepare(
            request(
                "codegraph_explore",
                json!({"query":"start","includeSource":true}),
            ),
            None,
        )
        .unwrap();
        assert_eq!(call.name, "codegraph_explore");
        assert_eq!(call.arguments.unwrap()["maxFiles"], 3);
        assert_eq!(budget, 6000);
    }
    #[test]
    fn rejects_unknown_arguments_and_invalid_limits() {
        for args in [
            json!({"query":"start","includeSource":"true"}),
            json!({"query":"start","maxChars":511}),
            json!({"query":"start","maxChars":24001}),
            json!({"query":"start","maxFiles":0}),
            json!({"query":"start","maxChars":1000.5}),
            json!({"query":"start","includeCode":true}),
            json!({"query":""}),
        ] {
            assert!(prepare(request("codegraph_explore", args), None).is_err());
        }
        assert!(prepare(
            request(
                "codegraph_search",
                json!({"query":"start","includeSource":true})
            ),
            None
        )
        .is_err());
        assert!(prepare(
            request(
                "codegraph_search",
                json!({"query":"start","kind":"unknown"})
            ),
            None
        )
        .is_err());
        assert!(prepare(
            request("codegraph_impact", json!({"symbol":"start","depth":6})),
            None
        )
        .is_err());
        assert!(prepare(
            request("codegraph_callers", json!({"symbol":"start","limit":-1})),
            None
        )
        .is_err());
        assert!(prepare(request("arbitrary_tool", json!({"query":"start"})), None).is_err());
    }
    #[test]
    fn file_filters_cannot_traverse_or_be_absolute() {
        for file in [
            "../outside.rs",
            "..\\outside.rs",
            "/outside.rs",
            "C:\\outside.rs",
            "\\\\server\\share\\file.rs",
        ] {
            assert!(prepare(
                request("codegraph_callers", json!({"symbol":"start","file":file})),
                None
            )
            .is_err());
        }
    }
    #[test]
    fn bound_root_blocks_overrides_missing_indexes_and_external_links() {
        let temp = tempfile::tempdir().unwrap();
        let parent = dunce::canonicalize(temp.path()).unwrap();
        std::fs::create_dir(parent.join(".codegraph")).unwrap();
        rusqlite::Connection::open(parent.join(".codegraph/codegraph.db"))
            .unwrap()
            .execute("CREATE TABLE nodes(id TEXT)", [])
            .unwrap();
        let root = parent.join("project");
        std::fs::create_dir_all(root.join(".codegraph")).unwrap();
        let database = root.join(".codegraph/codegraph.db");
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute("CREATE TABLE nodes(id TEXT)", [])
            .unwrap();
        std::fs::write(root.join("sample.rs"), "fn target() {}\n").unwrap();
        let foreign = tempfile::tempdir().unwrap();
        std::fs::write(foreign.path().join("outside.rs"), "fn secret() {}\n").unwrap();
        let (call, _) = prepare(
            request(
                "codegraph_callers",
                json!({"symbol":"target","file":"sample.rs"}),
            ),
            Some(&root),
        )
        .unwrap();
        assert_eq!(call.arguments.unwrap()["projectPath"], json!(root));
        assert!(prepare(
            request(
                "codegraph_search",
                json!({"query":"target","projectPath":foreign.path()})
            ),
            Some(&root)
        )
        .is_err());
        assert!(prepare(
            request(
                "codegraph_callers",
                json!({"symbol":"target","file":"missing.rs"})
            ),
            Some(&root)
        )
        .is_err());
        link_dir(foreign.path(), &root.join("linked"));
        assert!(prepare(
            request(
                "codegraph_callers",
                json!({"symbol":"target","file":"linked/outside.rs"})
            ),
            Some(&root)
        )
        .is_err());
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute("DROP TABLE nodes", [])
            .unwrap();
        assert!(prepare(
            request("codegraph_search", json!({"query":"target"})),
            Some(&root)
        )
        .is_err());
        std::fs::remove_file(&database).unwrap();
        std::fs::remove_dir(root.join(".codegraph")).unwrap();
        assert!(prepare(
            request("codegraph_search", json!({"query":"target"})),
            Some(&root)
        )
        .is_err());
        link_dir(foreign.path(), &root.join(".codegraph"));
        assert!(prepare(
            request("codegraph_search", json!({"query":"target"})),
            Some(&root)
        )
        .is_err());
    }
    fn link_dir(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let result = std::process::Command::new("cmd.exe")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .creation_flags(0x08000000)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    #[test]
    fn advertised_tools_use_desktop_policy_without_upstream_prompts() {
        for tool in tools() {
            let serialized = serde_json::to_string(&tool).unwrap();
            assert!(!serialized.contains("alwaysLoad"));
            assert!(!serialized.contains("PRIMARY TOOL"));
            assert_eq!(tool.input_schema["additionalProperties"], false);
            assert_eq!(tool.input_schema["properties"]["maxChars"]["default"], 6000);
        }
    }
}
