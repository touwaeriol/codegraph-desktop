use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    #[serde(skip)]
    pub source_message: Option<String>,
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub existing_project_id: Option<String>,
}
impl AppError {
    pub fn new(code: &str, message: impl ToString) -> Self {
        let source_message = message.to_string();
        Self {
            code: code.into(),
            message: crate::i18n::message(&source_message),
            source_message: Some(source_message),
            retryable: true,
            existing_project_id: None,
        }
    }
}
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut out = serializer.serialize_struct("AppError", 4)?;
        out.serialize_field("code", &self.code)?;
        out.serialize_field(
            "message",
            &crate::i18n::message(self.source_message.as_deref().unwrap_or(&self.message)),
        )?;
        out.serialize_field("retryable", &self.retryable)?;
        out.serialize_field("existingProjectId", &self.existing_project_id)?;
        out.end()
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
    pub language: String,
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
    #[serde(serialize_with = "crate::i18n::serialize_message")]
    pub message: String,
    pub error: Option<AppError>,
    pub started_at: String,
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
