//! Morph WarpGrep served as a project-scoped, read-only MCP route.
//!
//! The model plans grep/read/list/glob calls; this module executes them inside
//! the bound project root only. Tool semantics and output formats follow
//! `@morphllm/morphsdk` 0.2.196 (`tools/warp_grep`), which the model expects.
use crate::{authorize, Auth, Routes, SharedGateway};
use axum::{middleware, Router};
use rmcp::{
    model::*,
    service::RequestContext,
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
    RoleServer, ServerHandler,
};
use serde_json::{json, Value};
use std::{
    path::{Component, Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, Instant, SystemTime},
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_API_BASE: &str = "https://api.morphllm.com/v1";
pub const MODEL: &str = "morph-warp-grep-v2.1";
pub const TOOL_NAME: &str = "codebase_search";
const MAX_TURNS: usize = 6;
const MAX_CONTEXT_CHARS: usize = 321_600;
const MAX_OUTPUT_LINES: usize = 200;
const MAX_LIST_RESULTS: usize = 500;
const MAX_LIST_DEPTH: usize = 3;
const MAX_READ_LINES: usize = 800;
const MAX_FILE_BYTES: u64 = 2_000_000;
const MAX_GLOB_RESULTS: usize = 100;
const MAX_GREP_SCAN_LINES: usize = 5_000;
const LIST_TIMEOUT: Duration = Duration::from_secs(2);
// A normal WarpGrep turn takes about 2 seconds; a stalled request is retried
// once, and the whole search stays below Codex's default 120 s tool timeout.
const MODEL_TIMEOUT: Duration = Duration::from_secs(20);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(100);
const TRUNCATED_MARKER: &str = "[truncated for context limit]";
const EXCLUDES: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".bzr",
    "node_modules",
    "bower_components",
    ".pnpm",
    ".yarn",
    "vendor",
    "packages",
    "Pods",
    ".bundle",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".venv",
    "venv",
    ".tox",
    ".nox",
    ".eggs",
    "*.egg-info",
    "dist",
    "build",
    "out",
    "output",
    "target",
    "_build",
    ".next",
    ".nuxt",
    ".output",
    ".vercel",
    ".netlify",
    ".cache",
    ".parcel-cache",
    ".turbo",
    ".nx",
    ".gradle",
    ".idea",
    ".vscode",
    ".vs",
    "coverage",
    ".coverage",
    "htmlcov",
    ".nyc_output",
    "tmp",
    "temp",
    ".tmp",
    ".temp",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
    "Cargo.lock",
    "Gemfile.lock",
    "poetry.lock",
    "*.min.js",
    "*.min.css",
    "*.bundle.js",
    "*.wasm",
    "*.so",
    "*.dll",
    "*.pyc",
    "*.map",
    ".*",
];
const SKIP_NAMES: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".bzr",
    "node_modules",
    "bower_components",
    ".pnpm",
    ".yarn",
    "vendor",
    "Pods",
    ".bundle",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".venv",
    "venv",
    ".tox",
    ".nox",
    ".eggs",
    "dist",
    "build",
    "out",
    "output",
    "target",
    "_build",
    ".next",
    ".nuxt",
    ".output",
    ".vercel",
    ".netlify",
    ".cache",
    ".parcel-cache",
    ".turbo",
    ".nx",
    ".gradle",
    ".idea",
    ".vscode",
    ".vs",
    "coverage",
    ".coverage",
    "htmlcov",
    ".nyc_output",
    "tmp",
    "temp",
    ".tmp",
    ".temp",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
    "Cargo.lock",
    "Gemfile.lock",
    "poetry.lock",
];
const SKIP_SUFFIXES: &[&str] = &[
    ".min.js",
    ".min.css",
    ".bundle.js",
    ".wasm",
    ".so",
    ".dll",
    ".pyc",
    ".map",
];

/// Shared, replaceable API key. The desktop updates it when Settings change.
pub type ApiKey = Arc<RwLock<Option<String>>>;
pub type LogSink = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MorphErrorKind {
    InvalidKey,
    Unauthorized,
    PaymentRequired,
    RateLimited,
    Unavailable,
    Network,
    Protocol,
}
#[derive(Debug, Clone)]
pub struct MorphError {
    pub kind: MorphErrorKind,
    pub detail: String,
}
impl MorphError {
    fn new(kind: MorphErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
    pub fn code(&self) -> &'static str {
        match self.kind {
            MorphErrorKind::InvalidKey | MorphErrorKind::Unauthorized => "MORPH_UNAUTHORIZED",
            MorphErrorKind::PaymentRequired => "MORPH_PAYMENT_REQUIRED",
            MorphErrorKind::RateLimited => "MORPH_RATE_LIMITED",
            MorphErrorKind::Unavailable => "MORPH_UNAVAILABLE",
            MorphErrorKind::Network => "MORPH_NETWORK",
            MorphErrorKind::Protocol => "MORPH_PROTOCOL",
        }
    }
}
impl std::fmt::Display for MorphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.detail)
    }
}
impl std::error::Error for MorphError {}

fn tool_specs() -> Value {
    json!([
        {"type":"function","function":{"name":"list_directory","description":"Execute ls or find commands to explore directory structure. Max 500 results. Common junk directories are excluded automatically.","parameters":{"type":"object","properties":{"command":{"type":"string","description":"Full ls or find command (e.g. ls -la src/, find . -maxdepth 2 -type f -name '*.py', find . -type d, ls -d */)."}},"required":["command"]}}},
        {"type":"function","function":{"name":"grep_search","description":"Search for a regex pattern in file contents. Returns matching lines with file paths and line numbers. Case-insensitive. Respects .gitignore.","parameters":{"type":"object","properties":{"pattern":{"type":"string","description":"Regex pattern to search for in file contents (e.g. 'class\\s+\\w+Error', 'import|require|from', 'def (get|set|update)_user')."},"path":{"type":"string","description":"File or directory to search in. Defaults to current working directory."},"glob":{"type":"string","description":"Glob pattern to filter files (e.g. '*.py', '*.{ts,tsx,js,jsx,py,go}', 'src/**/*.go', '!*.test.*')."},"limit":{"type":"integer","description":"Limit output to first N matching lines. Shows all matches if not specified."}},"required":["pattern"]}}},
        {"type":"function","function":{"name":"glob","description":"Find files by name/extension using glob patterns. Returns absolute paths sorted by modification time (newest first). Respects .gitignore. Max 100 results.","parameters":{"type":"object","properties":{"pattern":{"type":"string","description":"Glob pattern to match files (e.g. '*.py', 'src/**/*.js', '*.{ts,tsx}', 'test_*.py')."},"path":{"type":"string","description":"Directory to search in. Defaults to repository root."}},"required":["pattern"]}}},
        {"type":"function","function":{"name":"read","description":"Read entire files or specific line ranges using absolute paths.","parameters":{"type":"object","properties":{"path":{"type":"string","description":"File path to read, using absolute path (e.g. '/home/ubuntu/repo/src/main.py' or windows path)."},"lines":{"type":"string","description":"Optional line range (e.g. '1-50' or '1-20,45-80'). Omit to read entire file."}},"required":["path"]}}},
        {"type":"function","function":{"name":"finish","description":"Submit final answer with all relevant code locations. Include imports and over-include rather than miss context.","parameters":{"type":"object","properties":{"files":{"type":"string","description":"One file per line as path:lines (e.g. 'src/auth.py:1-15,25-50\\nsrc/user.py'). Omit line range to include entire file."}},"required":["files"]}}}
    ])
}

#[derive(Clone)]
pub struct Morph {
    http: reqwest::Client,
    base: String,
    key: String,
}
struct ToolCall {
    id: String,
    name: String,
    arguments: Value,
}
pub struct SearchOutcome {
    pub text: String,
    pub turns: usize,
    pub files: usize,
}
impl Morph {
    pub fn new(key: &str) -> Result<Self, MorphError> {
        Self::with_base(key, DEFAULT_API_BASE)
    }
    pub fn with_base(key: &str, base: &str) -> Result<Self, MorphError> {
        let key = key.trim();
        // Keys are sent as an HTTP header; reject anything that is not a plain token.
        if key.len() < 8 || key.len() > 512 || !key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(MorphError::new(
                MorphErrorKind::InvalidKey,
                "API key must be 8-512 visible ASCII characters",
            ));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(MODEL_TIMEOUT)
            .user_agent(concat!("codegraph-desktop/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| MorphError::new(MorphErrorKind::Network, e.to_string()))?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_owned(),
            key: key.to_owned(),
        })
    }
    async fn complete(&self, messages: &[Value], max_tokens: u32) -> Result<Value, MorphError> {
        let body = json!({
            "model": MODEL,
            "temperature": 0.0,
            "max_tokens": max_tokens,
            "messages": messages,
            "tools": tool_specs(),
        });
        let response = self
            .http
            .post(format!("{}/chat/completions", self.base))
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                MorphError::new(
                    MorphErrorKind::Network,
                    // reqwest errors carry the URL but never request headers.
                    e.without_url().to_string(),
                )
            })?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| MorphError::new(MorphErrorKind::Network, e.without_url().to_string()))?;
        if !status.is_success() {
            let detail = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| {
                    v.pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| text.chars().take(300).collect());
            let detail = format!(
                "HTTP {}: {}",
                status.as_u16(),
                detail.chars().take(300).collect::<String>()
            );
            let kind = match status.as_u16() {
                401 | 403 => MorphErrorKind::Unauthorized,
                402 => MorphErrorKind::PaymentRequired,
                429 => MorphErrorKind::RateLimited,
                _ => MorphErrorKind::Unavailable,
            };
            return Err(MorphError::new(kind, detail));
        }
        let data: Value = serde_json::from_str(&text)
            .map_err(|_| MorphError::new(MorphErrorKind::Protocol, "response is not JSON"))?;
        data.pointer("/choices/0/message")
            .filter(|m| m.is_object())
            .cloned()
            .ok_or_else(|| MorphError::new(MorphErrorKind::Protocol, "response has no message"))
    }
    /// Retry once for transient failures: timeouts, network errors, 429 and 5xx.
    async fn complete_retrying(
        &self,
        messages: &[Value],
        max_tokens: u32,
        log: &(dyn Fn(String) + Send + Sync),
    ) -> Result<Value, MorphError> {
        match self.complete(messages, max_tokens).await {
            Err(e)
                if matches!(
                    e.kind,
                    MorphErrorKind::Network
                        | MorphErrorKind::Unavailable
                        | MorphErrorKind::RateLimited
                ) =>
            {
                log(format!("WarpGrep model call failed, retrying once: {e}"));
                tokio::time::sleep(Duration::from_millis(500)).await;
                self.complete(messages, max_tokens).await
            }
            result => result,
        }
    }
    /// Spend one minimal WarpGrep turn to prove the key, model access and credit.
    pub async fn validate(&self) -> Result<(), MorphError> {
        let probe = "<repo_structure>\n/probe\n/probe/main.rs\n</repo_structure>\n\n<search_string>\nFind the program entry point\n</search_string>\n<context_budget>0% (0K/321K chars)</context_budget>\nYou have used 0 turns and have 6 remaining";
        let message = self
            .complete_retrying(&[json!({"role":"user","content":probe})], 128, &|_| ())
            .await?;
        if parse_tool_calls(&message).is_empty()
            && message
                .get("content")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        {
            return Err(MorphError::new(
                MorphErrorKind::Protocol,
                "WarpGrep returned neither tool calls nor content",
            ));
        }
        Ok(())
    }
    /// `include_source` adds the code of every finished range; otherwise the
    /// outcome lists only files and line ranges for the client to read itself.
    pub async fn search(
        &self,
        repo: &Repo,
        query: &str,
        include_source: bool,
        log: &(dyn Fn(String) + Send + Sync),
    ) -> Result<SearchOutcome, MorphError> {
        let initial = {
            let repo = repo.clone();
            let query = query.to_owned();
            tokio::task::spawn_blocking(move || repo.initial_state(&query))
                .await
                .map_err(|e| MorphError::new(MorphErrorKind::Protocol, e.to_string()))?
        };
        let mut messages = vec![json!({"role":"user","content":initial})];
        for turn in 1..=MAX_TURNS {
            enforce_context_limit(&mut messages);
            let started = Instant::now();
            let mut message = self.complete_retrying(&messages, 2048, log).await;
            if matches!(&message, Ok(m) if parse_tool_calls(m).is_empty() && m.get("content").and_then(Value::as_str).is_none_or(str::is_empty))
            {
                // Mirrors the SDK: one retry for an empty completion.
                tokio::time::sleep(Duration::from_millis(200)).await;
                message = self.complete_retrying(&messages, 2048, log).await;
            }
            let model = started.elapsed().as_secs_f32();
            let message = message.inspect_err(|e| {
                log(format!(
                    "WarpGrep turn {turn}: model failed after {model:.1}s: {e}"
                ))
            })?;
            let calls = parse_tool_calls(&message);
            log(format!(
                "WarpGrep turn {turn}: model {model:.1}s, {}",
                summarize_calls(&calls)
            ));
            if calls.is_empty() {
                return Err(MorphError::new(
                    MorphErrorKind::Protocol,
                    format!("WarpGrep produced no tool calls on turn {turn}"),
                ));
            }
            messages.push(json!({
                "role": "assistant",
                "content": message.get("content").cloned().unwrap_or(Value::Null),
                "tool_calls": message.get("tool_calls").cloned().unwrap_or(json!([])),
            }));
            if let Some(finish) = calls.iter().find(|c| c.name == "finish") {
                let files = finish
                    .arguments
                    .get("files")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let repo = repo.clone();
                let specs = parse_finish_files(&files);
                let count = specs.len();
                let text =
                    tokio::task::spawn_blocking(move || repo.finish(&files, specs, include_source))
                        .await
                        .map_err(|e| MorphError::new(MorphErrorKind::Protocol, e.to_string()))?;
                return Ok(SearchOutcome {
                    text,
                    turns: turn,
                    files: count,
                });
            }
            let started = Instant::now();
            let mut tasks = tokio::task::JoinSet::new();
            for (index, call) in calls.into_iter().enumerate() {
                let repo = repo.clone();
                tasks.spawn_blocking(move || {
                    (index, call.id, repo.execute(&call.name, &call.arguments))
                });
            }
            let mut results = Vec::new();
            while let Some(result) = tasks.join_next().await {
                results.push(
                    result.map_err(|e| MorphError::new(MorphErrorKind::Protocol, e.to_string()))?,
                );
            }
            log(format!(
                "WarpGrep turn {turn}: local tools {:.1}s",
                started.elapsed().as_secs_f32()
            ));
            results.sort_by_key(|(index, _, _)| *index);
            for (_, id, content) in results {
                messages.push(json!({"role":"tool","tool_call_id":id,"content":content}));
            }
            let remaining = MAX_TURNS - turn;
            let hint = if remaining == 1 {
                format!("\nYou have used {turn} turns, you only have 1 turn remaining. You have run out of turns to explore the code base and MUST call the finish tool now")
            } else {
                format!(
                    "\nYou have used {turn} turn{} and have {remaining} remaining",
                    if turn == 1 { "" } else { "s" }
                )
            };
            messages.push(
                json!({"role":"user","content":format!("{hint}\n{}", context_budget(&messages))}),
            );
        }
        Err(MorphError::new(
            MorphErrorKind::Protocol,
            format!("WarpGrep did not finish within {MAX_TURNS} turns"),
        ))
    }
}

fn summarize_calls(calls: &[ToolCall]) -> String {
    if calls.is_empty() {
        return "no tool calls".into();
    }
    calls
        .iter()
        .map(|call| {
            let args = ["pattern", "path", "glob", "command", "lines", "files"]
                .iter()
                .filter_map(|key| call.arguments.get(*key).and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            let args: String = args.chars().take(160).collect();
            format!("{}({args})", call.name)
        })
        .collect::<Vec<_>>()
        .join(", ")
}
fn parse_tool_calls(message: &Value) -> Vec<ToolCall> {
    message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|call| {
            Some(ToolCall {
                id: call.get("id")?.as_str()?.to_owned(),
                name: call.pointer("/function/name")?.as_str()?.to_owned(),
                arguments: call
                    .pointer("/function/arguments")
                    .and_then(Value::as_str)
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| json!({})),
            })
        })
        .collect()
}
fn message_size(message: &Value) -> usize {
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .map_or(0, |s| s.chars().count());
    let calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|c| {
            ["/function/name", "/function/arguments"]
                .iter()
                .filter_map(|p| c.pointer(p).and_then(Value::as_str))
                .map(|s| s.chars().count())
                .sum::<usize>()
        })
        .sum::<usize>();
    content + calls
}
fn context_budget(messages: &[Value]) -> String {
    let total: usize = messages.iter().map(message_size).sum();
    format!(
        "<context_budget>{}% ({}K/{}K chars)</context_budget>",
        total * 100 / MAX_CONTEXT_CHARS,
        total / 1000,
        MAX_CONTEXT_CHARS / 1000
    )
}
/// Replace the oldest tool results and turn hints until the history fits.
fn enforce_context_limit(messages: &mut [Value]) {
    let mut total: usize = messages.iter().map(message_size).sum();
    let mut first_user = true;
    for message in messages.iter_mut() {
        if total <= MAX_CONTEXT_CHARS {
            return;
        }
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if role == "user" && std::mem::take(&mut first_user) {
            continue;
        }
        if (role == "tool" || role == "user")
            && message.get("content").and_then(Value::as_str) != Some(TRUNCATED_MARKER)
        {
            let before = message_size(message);
            message["content"] = json!(TRUNCATED_MARKER);
            total = total - before + TRUNCATED_MARKER.len();
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Lines {
    All,
    Ranges(Vec<(usize, usize)>),
}
fn parse_ranges(text: &str) -> Vec<(usize, usize)> {
    text.split(',')
        .filter_map(|part| {
            let mut bounds = part
                .trim()
                .splitn(2, '-')
                .map(|v| v.trim().parse::<usize>());
            let start = bounds.next()?.ok()?;
            let end = match bounds.next() {
                Some(end) => end.ok()?,
                None => start,
            };
            (start > 0 && end >= start).then_some((start, end))
        })
        .collect()
}
fn merge_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if start <= last.1 + 1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}
fn parse_finish_files(text: &str) -> Vec<(String, Lines)> {
    text.split_whitespace()
        .map(|item| {
            // Skip a Windows drive letter before looking for the range separator.
            let skip = if item.len() > 1
                && item.as_bytes()[1] == b':'
                && item.as_bytes()[0].is_ascii_alphabetic()
            {
                2
            } else {
                0
            };
            match item[skip..].find(':').map(|i| i + skip) {
                None => (item.to_owned(), Lines::All),
                Some(colon) => {
                    let ranges = parse_ranges(&item[colon + 1..]);
                    let lines = if ranges.is_empty() {
                        Lines::All
                    } else {
                        Lines::Ranges(merge_ranges(ranges))
                    };
                    (item[..colon].to_owned(), lines)
                }
            }
        })
        .collect()
}

/// A canonical project root. Every path the model supplies resolves inside it.
#[derive(Clone)]
pub struct Repo {
    root: PathBuf,
}
impl Repo {
    pub fn new(root: &Path) -> std::io::Result<Self> {
        let root = dunce::canonicalize(root)?;
        if !root.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "project root is not a directory",
            ));
        }
        Ok(Self { root })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    fn resolve(&self, input: &str) -> Result<PathBuf, String> {
        let input = input.trim();
        if input.is_empty() || input == "." || input == "./" {
            return Ok(self.root.clone());
        }
        let raw = Path::new(input);
        let mut candidates = vec![if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            self.root.join(raw)
        }];
        // Models sometimes drop the root ("/src/a.rs") or repeat its name ("repo/src/a.rs").
        let stripped = input.trim_start_matches(['/', '\\']);
        if stripped != input {
            candidates.push(self.root.join(stripped));
        }
        if let (Some(Component::Normal(first)), Some(name)) = (
            Path::new(stripped).components().next(),
            self.root.file_name(),
        ) {
            if first == name {
                candidates.push(
                    self.root.join(
                        Path::new(stripped)
                            .strip_prefix(name)
                            .unwrap_or(Path::new("")),
                    ),
                );
            }
        }
        let mut outside = false;
        for candidate in candidates {
            if let Ok(path) = dunce::canonicalize(&candidate) {
                if path.starts_with(&self.root) {
                    return Ok(path);
                }
                outside = true;
            }
        }
        Err(if outside {
            format!("[PATH ERROR] Path outside repository root: {input}")
        } else {
            format!("[FILE NOT FOUND] There is no file or directory at \"{input}\". Double-check the path exists and is spelled correctly.")
        })
    }
    fn relative(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(rel) if rel.as_os_str().is_empty() => ".".into(),
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => path.to_string_lossy().into_owned(),
        }
    }
    fn overrides(&self, include: Option<&str>) -> Result<ignore::overrides::Override, String> {
        let mut builder = ignore::overrides::OverrideBuilder::new(&self.root);
        if let Some(glob) = include.filter(|g| !g.trim().is_empty()) {
            builder
                .add(glob.trim())
                .map_err(|e| format!("[GLOB ERROR] {e}"))?;
        }
        for exclude in EXCLUDES {
            builder
                .add(&format!("!{exclude}"))
                .map_err(|e| format!("[GLOB ERROR] {e}"))?;
        }
        builder.build().map_err(|e| format!("[GLOB ERROR] {e}"))
    }
    fn walk(&self, target: &Path, include: Option<&str>) -> Result<ignore::Walk, String> {
        Ok(ignore::WalkBuilder::new(target)
            .standard_filters(true)
            .require_git(false)
            .follow_links(false)
            .overrides(self.overrides(include)?)
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !entry.file_type().is_some_and(|t| t.is_dir())
                    || !nested_repository(entry.path())
            })
            .sort_by_file_name(|a, b| a.cmp(b))
            .build())
    }
    fn initial_state(&self, query: &str) -> String {
        let mut lines = vec![self.root.to_string_lossy().into_owned()];
        lines.extend(self.list(&self.root, MAX_OUTPUT_LINES, 2));
        format!(
            "<repo_structure>\n{}\n</repo_structure>\n\n<search_string>\n{query}\n</search_string>\n{}\nYou have used 0 turns and have {MAX_TURNS} remaining",
            lines.join("\n"),
            context_budget(&[])
        )
    }
    fn execute(&self, name: &str, args: &Value) -> String {
        let text = |key: &str| args.get(key).and_then(Value::as_str);
        match name {
            "grep_search" => {
                let output = self.grep(
                    text("pattern").unwrap_or_default(),
                    text("path").unwrap_or("."),
                    text("glob"),
                );
                match args
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize)
                {
                    Some(limit) if limit > 0 && output.lines().count() > limit => format!(
                        "{}\n... (truncated at {limit} lines)",
                        output.lines().take(limit).collect::<Vec<_>>().join("\n")
                    ),
                    _ => output,
                }
            }
            "glob" => self.glob(text("pattern").unwrap_or_default(), text("path")),
            "list_directory" => {
                // Only the path operand of an ls/find command is used; nothing is executed.
                let path = text("command")
                    .unwrap_or(".")
                    .split_whitespace()
                    .skip(1)
                    .find(|t| !t.starts_with('-') && !t.starts_with('|') && !t.starts_with("\\("))
                    .unwrap_or(".")
                    .to_owned();
                match self.resolve(&path) {
                    Ok(dir) if dir.is_dir() => {
                        let entries = self.list(&dir, MAX_LIST_RESULTS, MAX_LIST_DEPTH);
                        if entries.is_empty() {
                            "empty".into()
                        } else {
                            entries.join("\n")
                        }
                    }
                    _ => "empty".into(),
                }
            }
            "read" => {
                let ranges = text("lines").map(parse_ranges).unwrap_or_default();
                self.read(text("path").unwrap_or_default(), &ranges)
            }
            _ => format!("Unknown tool: {name}"),
        }
    }
    fn list(&self, dir: &Path, max_results: usize, max_depth: usize) -> Vec<String> {
        fn skip(name: &str) -> bool {
            SKIP_NAMES.contains(&name)
                || name.starts_with('.')
                || SKIP_SUFFIXES.iter().any(|s| name.ends_with(s))
        }
        fn walk(
            dir: &Path,
            depth: usize,
            max_depth: usize,
            max: usize,
            started: Instant,
            out: &mut Vec<PathBuf>,
        ) {
            if depth > max_depth || out.len() >= max || started.elapsed() > LIST_TIMEOUT {
                return;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                if out.len() >= max || started.elapsed() > LIST_TIMEOUT {
                    return;
                }
                let name = entry.file_name();
                if skip(&name.to_string_lossy()) {
                    continue;
                }
                // file_type() does not follow symlinks, so linked directories stay unexpanded.
                let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
                if is_dir && nested_repository(&entry.path()) {
                    continue;
                }
                out.push(entry.path());
                if is_dir {
                    walk(&entry.path(), depth + 1, max_depth, max, started, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, 0, max_depth, max_results, Instant::now(), &mut out);
        out.into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    }
    fn grep(&self, pattern: &str, path: &str, glob: Option<&str>) -> String {
        if pattern.is_empty() {
            return "[GREP ERROR] pattern is required".into();
        }
        let regex = match regex::RegexBuilder::new(pattern)
            .size_limit(1 << 22)
            .build()
        {
            Ok(regex) => regex,
            Err(e) => return format!("[GREP ERROR] invalid regex: {e}"),
        };
        let target = match self.resolve(path) {
            Ok(target) => target,
            Err(_) => return "no matches".into(),
        };
        let files: Vec<PathBuf> = if target.is_file() {
            vec![target]
        } else {
            match self.walk(&target, glob) {
                Ok(walk) => walk
                    .flatten()
                    .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
                    .map(|e| e.into_path())
                    .collect(),
                Err(e) => return e,
            }
        };
        let mut out: Vec<String> = Vec::new();
        let mut capped = false;
        'files: for file in files {
            let Some(content) = read_text(&file) else {
                continue;
            };
            let lines: Vec<&str> = content.lines().collect();
            let hits: Vec<usize> = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| regex.is_match(line))
                .map(|(i, _)| i)
                .collect();
            if hits.is_empty() {
                continue;
            }
            let rel = self.relative(&file);
            let mut last: Option<usize> = None;
            for &hit in &hits {
                let start = hit.saturating_sub(1);
                let end = (hit + 1).min(lines.len() - 1);
                for (index, text) in lines.iter().enumerate().take(end + 1).skip(start) {
                    if last.is_some_and(|l| index <= l) {
                        continue;
                    }
                    if last.is_none_or(|l| index > l + 1) && !out.is_empty() {
                        out.push("--".into());
                    }
                    let separator = if hits.binary_search(&index).is_ok() {
                        ':'
                    } else {
                        '-'
                    };
                    let line = text.trim_start();
                    let line = if line.len() > 400 {
                        "[Omitted long line]"
                    } else {
                        line
                    };
                    out.push(format!("{rel}{separator}{}{separator}{line}", index + 1));
                    last = Some(index);
                    if out.len() >= MAX_GREP_SCAN_LINES {
                        capped = true;
                        break 'files;
                    }
                }
            }
        }
        if out.is_empty() {
            return "no matches".into();
        }
        if out.len() > MAX_OUTPUT_LINES {
            let total = out.len();
            out.truncate(MAX_OUTPUT_LINES);
            out.push(format!(
                "... (output truncated at {MAX_OUTPUT_LINES} of {total}{} lines)",
                if capped { "+" } else { "" }
            ));
        }
        out.join("\n")
    }
    fn glob(&self, pattern: &str, path: Option<&str>) -> String {
        if pattern.trim().is_empty() {
            return "[GLOB ERROR] pattern is required".into();
        }
        let dir = match self.resolve(path.unwrap_or(".")) {
            Ok(dir) if dir.is_dir() => dir,
            _ => return format!("[PATH ERROR] Directory not found: {}", path.unwrap_or(".")),
        };
        let walk = match self.walk(&dir, Some(pattern)) {
            Ok(walk) => walk,
            Err(e) => return e,
        };
        let mut files: Vec<(SystemTime, PathBuf)> = walk
            .flatten()
            .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
            .map(|e| {
                let modified = e
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                (modified, e.into_path())
            })
            .collect();
        if files.is_empty() {
            return "no matches".into();
        }
        files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let total = files.len();
        let body = files
            .iter()
            .take(MAX_GLOB_RESULTS)
            .map(|(_, p)| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let truncated = if total > MAX_GLOB_RESULTS {
            format!("\n[{} files truncated]", total - MAX_GLOB_RESULTS)
        } else {
            String::new()
        };
        format!(
            "Found {total} file(s) matching \"{pattern}\" within {}, sorted by modification time (newest first):\n---\n{body}\n---{truncated}",
            dir.to_string_lossy()
        )
    }
    /// Numbered `N|text` lines, capped like the SDK provider.
    fn numbered(&self, path: &str, range: Option<(usize, usize)>) -> Result<Vec<String>, String> {
        let file = self.resolve(path)?;
        if !file.is_file() {
            return Err(format!("[FILE NOT FOUND] You tried to read \"{path}\" but there is no file at this path. Double-check the path exists and is spelled correctly."));
        }
        let content = read_text(&file).ok_or_else(|| {
            format!("[UNREADABLE FILE] You tried to read \"{path}\" but this file is either too large or not a text file, so it cannot be read. Try a different file.")
        })?;
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        let (mut start, mut end) = range.unwrap_or((1, total));
        end = end.min(total);
        if (start > total && total > 0) || start > end {
            (start, end) = (1, total);
        }
        let mut out: Vec<String> = (start..=end)
            .filter(|&i| i >= 1)
            .map(|i| format!("{i}|{}", lines.get(i - 1).copied().unwrap_or_default()))
            .collect();
        if out.len() > MAX_READ_LINES {
            let count = out.len();
            out.truncate(MAX_READ_LINES);
            out.push(format!(
                "... (output truncated at {MAX_READ_LINES} of {count} lines)"
            ));
        }
        Ok(out)
    }
    fn read(&self, path: &str, ranges: &[(usize, usize)]) -> String {
        let chunks: Result<Vec<String>, String> = if ranges.is_empty() {
            self.numbered(path, None).map(|l| vec![l.join("\n")])
        } else {
            ranges
                .iter()
                .map(|&r| self.numbered(path, Some(r)).map(|l| l.join("\n")))
                .collect()
        };
        match chunks {
            Err(e) => e,
            Ok(chunks) if chunks.iter().all(String::is_empty) => "(empty file)".into(),
            Ok(chunks) => chunks.join("\n...\n"),
        }
    }
    fn finish(&self, raw: &str, files: Vec<(String, Lines)>, include_source: bool) -> String {
        if files.is_empty() {
            return if raw.trim().is_empty() {
                "No relevant code found. Try rephrasing your search term.".into()
            } else {
                raw.trim().to_owned()
            };
        }
        let strip = |lines: Vec<String>| {
            lines
                .into_iter()
                .map(|l| match l.split_once('|') {
                    Some((n, text)) if n.bytes().all(|b| b.is_ascii_digit()) => text.to_owned(),
                    _ => l,
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut index = vec!["Relevant context found:".to_owned()];
        let mut bodies = Vec::new();
        for (path, lines) in &files {
            let display = self
                .resolve(path)
                .map(|p| self.relative(&p))
                .unwrap_or_else(|_| path.clone());
            let label = match lines {
                Lines::All => "*".to_owned(),
                Lines::Ranges(r) => r
                    .iter()
                    .map(|(s, e)| format!("{s}-{e}"))
                    .collect::<Vec<_>>()
                    .join(","),
            };
            index.push(format!("- {display}:{label}"));
            if !include_source {
                continue;
            }
            let content = match lines {
                Lines::All => self
                    .numbered(path, None)
                    .map(strip)
                    .unwrap_or_else(|_| format!("[couldn't find: {path}]")),
                Lines::Ranges(ranges) => ranges
                    .iter()
                    .enumerate()
                    .map(|(i, &(s, e))| {
                        let body = self
                            .numbered(path, Some((s, e)))
                            .map(strip)
                            .unwrap_or_else(|_| format!("[couldn't find: {path}]"));
                        if i > 0 || s > 1 {
                            format!("// ... existing code, block starting at line {s} ...\n{body}")
                        } else {
                            body
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            let attr = match lines {
                Lines::All => String::new(),
                Lines::Ranges(_) => format!(" lines=\"{label}\""),
            };
            bodies.push(format!(
                "<file path=\"{display}\"{attr}>\n{content}\n</file>\n"
            ));
        }
        if !include_source {
            return format!(
                "WarpGrep searched this project.\n\n{}\n\nRead the listed files and line ranges for their code, or pass includeSource=true to include them here.",
                index.join("\n")
            );
        }
        format!(
            "WarpGrep searched this project.\n\n{}\n\nHere is the content of files:\n\n{}",
            index.join("\n"),
            bodies.join("\n")
        )
    }
}
/// Nested repositories and git worktrees (a `.git` file or directory) are not
/// part of this project, matching how git itself treats them.
fn nested_repository(dir: &Path) -> bool {
    dir.join(".git").exists()
}
/// UTF-8 (lossy) text of a regular file, or None for large or binary files.
fn read_text(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn tool() -> Tool {
    serde_json::from_value(json!({
        "name": TOOL_NAME,
        "description": "Natural-language code search subagent (Morph WarpGrep) for this project. It runs several rounds of grep, file listing and reads inside the bound project, then returns the relevant files and line ranges; set includeSource=true to also get their code. Use a targeted question such as \"Where are HTTP routes authenticated?\", not bare keywords or a symbol name. Results may be partial. File excerpts read during the search are sent to the Morph API.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "search_string": {"type":"string", "minLength":1, "maxLength":2000, "description":"What to find or understand, as a specific natural-language question."},
                "includeSource": {"type":"boolean", "default":false, "description":"Also return the code of each listed range in <file> blocks. By default only file paths and line ranges are returned; read them with your file tools."},
                "maxChars": {"type":"integer", "minimum":1000, "maximum":48000, "default":12000, "description":"Response budget in Unicode characters, including serialized result metadata and truncation notices."}
            },
            "required": ["search_string"],
            "additionalProperties": false
        },
        "annotations": {"readOnlyHint":true, "destructiveHint":false, "idempotentHint":true, "openWorldHint":true}
    }))
    .expect("static tool schema")
}

#[derive(Clone)]
struct Server {
    repo: Repo,
    key: ApiKey,
    base: String,
    slots: Arc<Semaphore>,
    log: Option<LogSink>,
}
fn invalid(message: &str) -> ErrorData {
    ErrorData::invalid_params(message.to_owned(), None)
}
impl ServerHandler for Server {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some("项目专属只读 WarpGrep 网关。codebase_search 以自然语言描述检索目标，由 Morph WarpGrep 在当前项目内多轮 grep/读取后返回相关文件与行段，includeSource=true 时附带对应代码；检索中读取的代码片段会发送至 Morph API。精确符号查找请使用 CodeGraph 或 LSP。".into()),
            ..Default::default()
        }
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: vec![tool()],
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        if request.name != TOOL_NAME {
            return Err(invalid("Tool is not approved"));
        }
        let args = request.arguments.unwrap_or_default();
        if args
            .keys()
            .any(|k| !["search_string", "includeSource", "maxChars"].contains(&k.as_str()))
        {
            return Err(invalid("Unknown argument"));
        }
        let query = args
            .get("search_string")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|q| !q.is_empty() && q.chars().count() <= 2000)
            .ok_or_else(|| invalid("search_string must contain 1 to 2000 characters"))?
            .to_owned();
        let budget = match args.get("maxChars") {
            None => 12000,
            Some(v) => v
                .as_u64()
                .filter(|v| (1000..=48000).contains(v))
                .ok_or_else(|| invalid("maxChars must be an integer between 1000 and 48000"))?
                as usize,
        };
        let include_source = match args.get("includeSource") {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| invalid("includeSource must be a boolean"))?,
        };
        let Some(key) = self.key.read().unwrap().clone() else {
            return Ok(CallToolResult::error(vec![Content::text(
                "MORPH_KEY_REQUIRED: configure and validate a Morph API key in CodeGraph Desktop settings",
            )]));
        };
        let _slot = self.slots.clone().try_acquire_owned().map_err(|_| {
            ErrorData::internal_error("PROJECT_BUSY: too many concurrent WarpGrep searches", None)
        })?;
        let morph = Morph::with_base(&key, &self.base)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let log = |line: String| {
            if let Some(log) = &self.log {
                log(line)
            }
        };
        log(format!(
            "WarpGrep search: {}",
            query.chars().take(200).collect::<String>()
        ));
        let started = Instant::now();
        let outcome = tokio::select! {
            _ = ctx.ct.cancelled() => {
                log(format!("WarpGrep cancelled by the client after {:.1}s", started.elapsed().as_secs_f32()));
                return Err(ErrorData::internal_error("请求已取消", None));
            }
            outcome = tokio::time::timeout(SEARCH_TIMEOUT, morph.search(&self.repo, &query, include_source, &log)) => outcome,
        };
        let elapsed = started.elapsed().as_secs_f32();
        let result = match outcome {
            Ok(Ok(outcome)) => {
                log(format!(
                    "WarpGrep: {} turn(s), {} file(s), {elapsed:.1}s",
                    outcome.turns, outcome.files
                ));
                CallToolResult::success(vec![Content::text(outcome.text)])
            }
            Ok(Err(e)) => {
                log(format!("WarpGrep failed after {elapsed:.1}s: {e}"));
                CallToolResult::error(vec![Content::text(format!("Search failed: {e}"))])
            }
            Err(_) => {
                log(format!("WarpGrep timed out after {elapsed:.1}s"));
                CallToolResult::error(vec![Content::text(format!(
                    "Search failed: timed out after {}s",
                    SEARCH_TIMEOUT.as_secs()
                ))])
            }
        };
        Ok(crate::budget::bound(result, budget))
    }
}

pub struct WarpGrepOptions {
    pub project_id: String,
    pub root: PathBuf,
    /// The project's 64-hex HTTP token, shared with its CodeGraph route.
    pub token: String,
    pub key: ApiKey,
    pub log: Option<LogSink>,
}
/// A registered `/warpgrep/<project>` route; dropping it unregisters the route.
pub struct WarpGrepRoute {
    routes: Routes,
    route_key: String,
    cancel: CancellationToken,
    sessions: Arc<LocalSessionManager>,
    endpoint: String,
}
pub(crate) fn route_key(project_id: &str) -> String {
    format!("warpgrep/{project_id}")
}
impl SharedGateway {
    pub fn register_warpgrep(&self, options: WarpGrepOptions) -> anyhow::Result<WarpGrepRoute> {
        self.register_warpgrep_with_base(options, DEFAULT_API_BASE)
    }
    pub fn register_warpgrep_with_base(
        &self,
        options: WarpGrepOptions,
        base: &str,
    ) -> anyhow::Result<WarpGrepRoute> {
        uuid::Uuid::parse_str(&options.project_id)?;
        anyhow::ensure!(
            options.token.len() == 64 && options.token.bytes().all(|b| b.is_ascii_hexdigit()),
            "INVALID_HTTP_TOKEN"
        );
        anyhow::ensure!(!self.cancel.is_cancelled(), "SHARED_GATEWAY_STOPPED");
        let repo = Repo::new(&options.root).map_err(|e| anyhow::anyhow!("PROJECT_MISSING: {e}"))?;
        let server = Server {
            repo,
            key: options.key,
            base: base.to_owned(),
            slots: Arc::new(Semaphore::new(4)),
            log: options.log,
        };
        let cancel = self.cancel.child_token();
        let factory_cancel = cancel.clone();
        let sessions = Arc::new(LocalSessionManager::default());
        let service = StreamableHttpService::new(
            move || {
                if factory_cancel.is_cancelled() {
                    return Err(std::io::Error::other("PROJECT_STOPPED"));
                }
                Ok(server.clone())
            },
            sessions.clone(),
            StreamableHttpServerConfig::default(),
        );
        let path = format!("/warpgrep/{}", options.project_id);
        let app = Router::new()
            .nest_service(&path, service)
            .layer(middleware::from_fn_with_state(
                Auth {
                    token: format!("Bearer {}", options.token),
                    host: format!("127.0.0.1:{}", self.port),
                },
                authorize,
            ));
        let key = route_key(&options.project_id);
        {
            let mut routes = self.routes.write().unwrap();
            anyhow::ensure!(!self.cancel.is_cancelled(), "SHARED_GATEWAY_STOPPED");
            anyhow::ensure!(!routes.contains_key(&key), "PROJECT_ALREADY_REGISTERED");
            routes.insert(key.clone(), (app, cancel.clone()));
        }
        Ok(WarpGrepRoute {
            routes: self.routes.clone(),
            route_key: key,
            cancel,
            sessions,
            endpoint: format!("http://127.0.0.1:{}{path}", self.port),
        })
    }
}
impl WarpGrepRoute {
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub fn is_alive(&self) -> bool {
        !self.cancel.is_cancelled()
    }
    pub async fn stop(mut self) {
        self.unregister();
        let handles: Vec<_> = self
            .sessions
            .sessions
            .write()
            .await
            .drain()
            .map(|(_, h)| h)
            .collect();
        for handle in handles {
            let _ = tokio::time::timeout(Duration::from_secs(1), handle.close()).await;
        }
    }
    fn unregister(&mut self) {
        self.cancel.cancel();
        self.routes.write().unwrap().remove(&self.route_key);
    }
}
impl Drop for WarpGrepRoute {
    fn drop(&mut self) {
        self.unregister();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Repo) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/auth")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/lib")).unwrap();
        std::fs::write(
            root.join("src/auth/login.rs"),
            "use crate::db;\n\npub fn login(user: &str) -> bool {\n    verify_token(user)\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "fn main() {\n    login(\"a\");\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("node_modules/lib/index.js"),
            "function login() {}\n",
        )
        .unwrap();
        std::fs::write(root.join("blob.bin"), b"login\0\0\0").unwrap();
        let repo = Repo::new(root).unwrap();
        (dir, repo)
    }

    #[test]
    fn grep_uses_ripgrep_format_and_skips_excluded_and_binary_files() {
        let (_dir, repo) = fixture();
        let out = repo.grep("login", ".", None);
        assert!(
            out.contains("src/auth/login.rs:3:pub fn login(user: &str) -> bool {"),
            "{out}"
        );
        assert!(
            out.contains("src/auth/login.rs-4-verify_token(user)"),
            "{out}"
        );
        assert!(out.contains("src/main.rs:2:login(\"a\");"), "{out}");
        assert!(out.contains("\n--\n"), "{out}");
        assert!(
            !out.contains("node_modules") && !out.contains("blob.bin"),
            "{out}"
        );
        let only_main = repo.grep("login", ".", Some("main.rs"));
        assert!(
            only_main.contains("src/main.rs") && !only_main.contains("auth"),
            "{only_main}"
        );
        assert_eq!(repo.grep("absent_symbol", "src", None), "no matches");
        assert!(repo.grep("(", ".", None).starts_with("[GREP ERROR]"));
    }

    #[test]
    fn paths_are_confined_to_the_project_root() {
        let (dir, repo) = fixture();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "TOP SECRET").unwrap();
        let secret = outside.path().join("secret.txt");
        for path in [
            secret.to_string_lossy().into_owned(),
            "../secret.txt".into(),
            format!(
                "../{}/secret.txt",
                outside.path().file_name().unwrap().to_string_lossy()
            ),
        ] {
            let out = repo.read(&path, &[]);
            assert!(!out.contains("TOP SECRET"), "{path}: {out}");
        }
        assert!(!repo
            .grep("SECRET", &outside.path().to_string_lossy(), None)
            .contains("TOP SECRET"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&secret, dir.path().join("link.txt")).unwrap();
            assert!(!repo.read("link.txt", &[]).contains("TOP SECRET"));
            assert!(!repo.grep("SECRET", ".", None).contains("TOP SECRET"));
        }
        let _ = dir;
    }

    #[test]
    fn read_supports_absolute_relative_and_dropped_root_paths() {
        let (_dir, repo) = fixture();
        let absolute = repo.root().join("src/auth/login.rs");
        let expected = "3|pub fn login(user: &str) -> bool {";
        assert!(repo
            .read(&absolute.to_string_lossy(), &[(3, 3)])
            .contains(expected));
        assert!(repo.read("src/auth/login.rs", &[(3, 3)]).contains(expected));
        assert!(repo
            .read("/src/auth/login.rs", &[(3, 3)])
            .contains(expected));
        let name = repo
            .root()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(repo
            .read(&format!("{name}/src/auth/login.rs"), &[(3, 3)])
            .contains(expected));
        assert_eq!(
            repo.read("src/auth/login.rs", &[(1, 1), (4, 4)]),
            "1|use crate::db;\n...\n4|    verify_token(user)"
        );
        assert!(repo.read("missing.rs", &[]).starts_with("[FILE NOT FOUND]"));
        assert!(repo.read("blob.bin", &[]).starts_with("[UNREADABLE FILE]"));
    }

    #[test]
    fn listing_and_glob_follow_sdk_formats() {
        let (_dir, repo) = fixture();
        let listing = repo.execute("list_directory", &json!({"command":"ls -la src/"}));
        assert!(listing.contains(
            &repo
                .root()
                .join("src")
                .join("main.rs")
                .to_string_lossy()
                .into_owned()
        ));
        assert!(!listing.contains("node_modules"));
        let glob = repo.glob("*.rs", None);
        assert!(
            glob.starts_with("Found 2 file(s) matching \"*.rs\""),
            "{glob}"
        );
        assert!(glob.contains(
            &repo
                .root()
                .join("src")
                .join("main.rs")
                .to_string_lossy()
                .into_owned()
        ));
        assert_eq!(repo.glob("*.py", None), "no matches");
        let state = repo.initial_state("Find login");
        assert!(state.starts_with(&format!(
            "<repo_structure>\n{}\n",
            repo.root().to_string_lossy()
        )));
        assert!(state.contains("<search_string>\nFind login\n</search_string>"));
        assert!(!state.contains("node_modules"));
    }

    #[test]
    fn finish_specs_parse_ranges_and_windows_drives() {
        assert_eq!(
            parse_finish_files("src/a.rs:1-5,3-9,20 C:\\repo\\b.rs:2-3 c.rs"),
            vec![
                ("src/a.rs".into(), Lines::Ranges(vec![(1, 9), (20, 20)])),
                ("C:\\repo\\b.rs".into(), Lines::Ranges(vec![(2, 3)])),
                ("c.rs".into(), Lines::All),
            ]
        );
        let (_dir, repo) = fixture();
        let specs = "src/auth/login.rs:3-4 src/main.rs";
        let text = repo.finish("", parse_finish_files(specs), true);
        assert!(text.contains("- src/auth/login.rs:3-4"), "{text}");
        assert!(text.contains("<file path=\"src/auth/login.rs\" lines=\"3-4\">\n// ... existing code, block starting at line 3 ...\npub fn login"), "{text}");
        assert!(
            text.contains("<file path=\"src/main.rs\">\nfn main() {"),
            "{text}"
        );
        // Without source, only the index is returned, so no file body leaks in.
        let index = repo.finish("", parse_finish_files(specs), false);
        assert!(
            index.contains("- src/auth/login.rs:3-4\n- src/main.rs:*"),
            "{index}"
        );
        assert!(
            !index.contains("<file")
                && !index.contains("pub fn login")
                && !index.contains("fn main"),
            "{index}"
        );
        assert!(index.contains("includeSource=true"), "{index}");
        for include_source in [false, true] {
            assert!(repo
                .finish("nothing relevant", vec![], include_source)
                .contains("nothing relevant"));
        }
    }

    #[test]
    fn context_limit_truncates_oldest_results_but_keeps_query() {
        let mut messages = vec![
            json!({"role":"user","content":"query"}),
            json!({"role":"tool","tool_call_id":"a","content":"x".repeat(MAX_CONTEXT_CHARS)}),
            json!({"role":"tool","tool_call_id":"b","content":"recent"}),
        ];
        enforce_context_limit(&mut messages);
        assert_eq!(messages[0]["content"], "query");
        assert_eq!(messages[1]["content"], TRUNCATED_MARKER);
        assert_eq!(messages[2]["content"], "recent");
    }

    #[test]
    fn keys_must_be_header_safe() {
        for key in [
            "",
            "short",
            "has space key",
            "bad\nnewline-key",
            "密钥密钥密钥密钥",
        ] {
            assert!(matches!(Morph::new(key), Err(e) if e.kind == MorphErrorKind::InvalidKey));
        }
        assert!(Morph::new("sk-test-1234567890").is_ok());
    }

    /// Fake Morph endpoint: grep first, then finish with the hit.
    async fn fake_morph(status: u16) -> (String, Arc<std::sync::Mutex<Vec<Value>>>) {
        fake_morph_flaky(status, 0).await
    }
    /// `transient` leading requests fail with 503 before normal behaviour.
    async fn fake_morph_flaky(
        status: u16,
        transient: usize,
    ) -> (String, Arc<std::sync::Mutex<Vec<Value>>>) {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let record = seen.clone();
        let failures = Arc::new(std::sync::Mutex::new(transient));
        let app = Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move |headers: axum::http::HeaderMap, axum::Json(body): axum::Json<Value>| {
                let record = record.clone();
                let failures = failures.clone();
                async move {
                    use axum::response::IntoResponse;
                    let transient = {
                        let mut left = failures.lock().unwrap();
                        let fail = *left > 0;
                        *left = left.saturating_sub(1);
                        fail
                    };
                    if transient {
                        return (axum::http::StatusCode::SERVICE_UNAVAILABLE, "busy").into_response();
                    }
                    if status != 200 {
                        return (
                            axum::http::StatusCode::from_u16(status).unwrap(),
                            axum::Json(json!({"error":{"message":"denied"}})),
                        )
                            .into_response();
                    }
                    assert_eq!(headers["authorization"], "Bearer sk-test-1234567890");
                    let turn = body["messages"].as_array().unwrap().iter().filter(|m| m["role"] == "assistant").count();
                    record.lock().unwrap().push(body);
                    let call = if turn == 0 {
                        json!({"id":"c1","type":"function","function":{"name":"grep_search","arguments":"{\"pattern\":\"fn login\"}"}})
                    } else {
                        json!({"id":"c2","type":"function","function":{"name":"finish","arguments":"{\"files\":\"src/auth/login.rs:3-5\"}"}})
                    };
                    axum::Json(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[call]}}]})).into_response()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}/v1"), seen)
    }

    #[tokio::test]
    async fn search_loop_executes_tools_locally_and_resolves_finish() {
        let (_dir, repo) = fixture();
        let (base, seen) = fake_morph(200).await;
        let morph = Morph::with_base("sk-test-1234567890", &base).unwrap();
        morph.validate().await.unwrap();
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        let outcome = morph
            .search(&repo, "Where is login implemented?", true, &|line| {
                lines.lock().unwrap().push(line)
            })
            .await
            .unwrap();
        let lines = lines.into_inner().unwrap();
        assert!(
            lines[0].starts_with("WarpGrep turn 1: model ")
                && lines[0].ends_with("grep_search(fn login)"),
            "{lines:?}"
        );
        assert!(lines
            .iter()
            .any(|l| l.starts_with("WarpGrep turn 1: local tools ")));
        assert!(lines
            .iter()
            .any(|l| l.contains("turn 2") && l.contains("finish(src/auth/login.rs:3-5)")));
        assert_eq!((outcome.turns, outcome.files), (2, 1));
        assert!(
            outcome.text.contains("pub fn login(user: &str) -> bool {"),
            "{}",
            outcome.text
        );
        let seen = seen.lock().unwrap();
        let second = &seen.last().unwrap()["messages"];
        let tool = second
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "tool")
            .unwrap();
        assert_eq!(tool["tool_call_id"], "c1");
        assert!(tool["content"]
            .as_str()
            .unwrap()
            .contains("src/auth/login.rs:3:pub fn login"));
        assert!(second.as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("You have used 1 turn and have 5 remaining"));
    }

    #[tokio::test]
    async fn transient_model_failures_are_retried_once() {
        let (_dir, repo) = fixture();
        let (base, _) = fake_morph_flaky(200, 1).await;
        let morph = Morph::with_base("sk-test-1234567890", &base).unwrap();
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        let outcome = morph
            .search(&repo, "Where is login implemented?", false, &|line| {
                lines.lock().unwrap().push(line)
            })
            .await
            .unwrap();
        assert_eq!(outcome.files, 1);
        assert!(
            outcome.text.contains("- src/auth/login.rs:3-5") && !outcome.text.contains("<file"),
            "{}",
            outcome.text
        );
        assert!(lines.lock().unwrap()[0]
            .starts_with("WarpGrep model call failed, retrying once: MORPH_UNAVAILABLE: HTTP 503"));
        // Two consecutive failures exhaust the single retry.
        let (base, _) = fake_morph_flaky(200, 2).await;
        let error = Morph::with_base("sk-test-1234567890", &base)
            .unwrap()
            .search(&repo, "Where is login implemented?", false, &|_| ())
            .await
            .err()
            .unwrap();
        assert_eq!(error.kind, MorphErrorKind::Unavailable);
    }

    #[test]
    fn nested_repositories_and_worktrees_are_not_searched() {
        let (dir, repo) = fixture();
        let snapshot = dir.path().join("artifacts/old-snapshot/src");
        std::fs::create_dir_all(&snapshot).unwrap();
        std::fs::write(
            dir.path().join("artifacts/old-snapshot/.git"),
            "gitdir: elsewhere
",
        )
        .unwrap();
        std::fs::write(
            snapshot.join("copy.rs"),
            "pub fn login() {}
",
        )
        .unwrap();
        let nested = dir.path().join("vendored-repo");
        std::fs::create_dir_all(nested.join(".git")).unwrap();
        std::fs::write(
            nested.join("lib.rs"),
            "fn login() {}
",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("artifacts/notes.rs"),
            "// login notes
",
        )
        .unwrap();
        let grep = repo.grep("login", ".", None);
        assert!(
            !grep.contains("old-snapshot") && !grep.contains("vendored-repo"),
            "{grep}"
        );
        assert!(grep.contains("artifacts/notes.rs"), "{grep}");
        let glob = repo.glob("*.rs", None);
        assert!(glob.starts_with("Found 3 file(s)"), "{glob}");
        let state = repo.initial_state("login");
        assert!(
            !state.contains("old-snapshot") && !state.contains("vendored-repo"),
            "{state}"
        );
        assert!(state.contains("notes.rs"), "{state}");
    }

    #[tokio::test]
    async fn validation_classifies_rejections() {
        for (status, kind) in [
            (401, MorphErrorKind::Unauthorized),
            (402, MorphErrorKind::PaymentRequired),
            (429, MorphErrorKind::RateLimited),
            (500, MorphErrorKind::Unavailable),
        ] {
            let (base, _) = fake_morph(status).await;
            let error = Morph::with_base("sk-test-1234567890", &base)
                .unwrap()
                .validate()
                .await
                .unwrap_err();
            assert_eq!(error.kind, kind);
            assert!(error.detail.contains("denied"));
        }
    }

    #[tokio::test]
    async fn shared_route_requires_project_token_and_exposes_one_read_only_tool() {
        let (dir, _) = fixture();
        let (base, _) = fake_morph(200).await;
        let host = SharedGateway::start(0).await.unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let token = "ab".repeat(32);
        let key: ApiKey = Arc::new(RwLock::new(Some("sk-test-1234567890".into())));
        let route = host
            .register_warpgrep_with_base(
                WarpGrepOptions {
                    project_id: id.clone(),
                    root: dir.path().into(),
                    token: token.clone(),
                    key: key.clone(),
                    log: None,
                },
                &base,
            )
            .unwrap();
        assert!(
            crate::probe_http(route.endpoint(), "cd".repeat(32).as_str())
                .await
                .is_err()
        );
        assert_eq!(
            crate::probe_http(route.endpoint(), &token).await.unwrap(),
            [TOOL_NAME]
        );
        let client = crate::connect_http(route.endpoint(), &token).await.unwrap();
        let result = client
            .call_tool(serde_json::from_value(json!({"name":TOOL_NAME,"arguments":{"search_string":"Where is login implemented?"}})).unwrap())
            .await
            .unwrap();
        assert_ne!(result.is_error, Some(true));
        // Locations only by default; the code comes back only when asked for.
        let text = serde_json::to_string(&result).unwrap();
        assert!(
            text.contains("src/auth/login.rs:3-5") && !text.contains("verify_token"),
            "{text}"
        );
        let result = client
            .call_tool(serde_json::from_value(json!({"name":TOOL_NAME,"arguments":{"search_string":"Where is login implemented?","includeSource":true}})).unwrap())
            .await
            .unwrap();
        assert_ne!(result.is_error, Some(true));
        assert!(serde_json::to_string(&result)
            .unwrap()
            .contains("verify_token"));
        assert!(client
            .call_tool(
                serde_json::from_value(
                    json!({"name":TOOL_NAME,"arguments":{"search_string":"x","includeSource":"yes"}})
                )
                .unwrap()
            )
            .await
            .is_err());
        *key.write().unwrap() = None;
        let result = client
            .call_tool(
                serde_json::from_value(
                    json!({"name":TOOL_NAME,"arguments":{"search_string":"again"}}),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(client
            .call_tool(
                serde_json::from_value(
                    json!({"name":TOOL_NAME,"arguments":{"search_string":"x","path":"/"}})
                )
                .unwrap()
            )
            .await
            .is_err());
        let _ = client.cancel().await;
        let endpoint = route.endpoint().to_owned();
        route.stop().await;
        assert!(crate::probe_http(&endpoint, &token).await.is_err());
        host.stop().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires MORPH_API_KEY and network access; spends a few WarpGrep turns"]
    async fn real_morph_search_over_this_crate() {
        let key = std::env::var("MORPH_API_KEY").expect("MORPH_API_KEY");
        Morph::new(&key).unwrap().validate().await.unwrap();
        // Same path as a real client: shared gateway route, project token, MCP tool call.
        let host = SharedGateway::start(0).await.unwrap();
        let token = "ef".repeat(32);
        let route = host
            .register_warpgrep(WarpGrepOptions {
                project_id: uuid::Uuid::new_v4().to_string(),
                root: env!("CARGO_MANIFEST_DIR").into(),
                token: token.clone(),
                key: Arc::new(RwLock::new(Some(key))),
                log: Some(Arc::new(|line| println!("[log] {line}"))),
            })
            .unwrap();
        let client = crate::connect_http(route.endpoint(), &token).await.unwrap();
        let result = client
            .call_tool(serde_json::from_value(json!({"name":TOOL_NAME,"arguments":{
                "search_string":"Where does the gateway check the bearer token and Host header of incoming HTTP requests?",
                "includeSource":true
            }})).unwrap())
            .await
            .unwrap();
        let text = &result.content[0].as_text().unwrap().text;
        println!("{text}");
        assert_ne!(result.is_error, Some(true), "{text}");
        assert!(
            text.contains("lib.rs") && text.contains("fn authorize"),
            "{text}"
        );
        let _ = client.cancel().await;
        route.stop().await;
        host.stop().await.unwrap();
    }
}
