use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub existing_project_id: Option<String>,
}
impl AppError {
    pub fn new(code: &str, message: impl ToString) -> Self {
        Self {
            code: code.into(),
            message: message.to_string(),
            retryable: true,
            existing_project_id: None,
        }
    }
}
pub type Result<T> = std::result::Result<T, AppError>;
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::new("IO_ERROR", e)
    }
}
impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        Self::new("DATABASE_ERROR", e)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    #[serde(default)]
    pub previous_roots: Vec<String>,
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub canonical_path: String,
    pub notes: String,
    pub auto_start: bool,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub codegraph_entry: Option<String>,
    pub index_concurrency: usize,
    pub close_behavior: String,
    pub app_data_dir: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub available: bool,
    pub entry: Option<String>,
    pub version: Option<String>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub project_id: String,
    pub generation: String,
    pub sequence: u64,
    pub timestamp: String,
    pub state: String,
    pub pid: Option<u32>,
    pub port: Option<u16>,
    pub started_at: Option<String>,
    pub sessions: usize,
    pub index_state: String,
    pub error: Option<AppError>,
    pub index_stats: Option<serde_json::Value>,
}
impl RuntimeSnapshot {
    pub fn new(id: &str) -> Self {
        Self {
            project_id: id.into(),
            generation: uuid::Uuid::new_v4().to_string(),
            sequence: 0,
            timestamp: now(),
            state: "stopped".into(),
            pid: None,
            port: None,
            started_at: None,
            sessions: 0,
            index_state: "unknown".into(),
            error: None,
            index_stats: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub project_id: String,
    pub generation: String,
    pub sequence: u64,
    pub timestamp: String,
    pub level: String,
    pub stage: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgress {
    pub operation_id: String,
    pub project_id: String,
    pub generation: String,
    pub sequence: u64,
    pub timestamp: String,
    pub kind: String,
    pub state: String,
    pub message: String,
    pub error: Option<AppError>,
    pub started_at: String,
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
