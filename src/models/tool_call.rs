use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCallStatus {
    Pending,
    Running,
    Completed,
    Error,
    Unknown,
}

impl ToolCallStatus {
    #[allow(dead_code)]
    pub fn from_storage(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "error" => Self::Error,
            _ => Self::Unknown,
        }
    }

    pub fn to_storage(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Error => "error",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Session-scoped identifier
    pub id: String,
    pub session_id: String,
    /// NULL for top-level tool calls; set for tool calls owned by a subagent
    pub subagent_id: Option<String>,
    pub tool_name: String,
    pub status: ToolCallStatus,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub input_json: Option<String>,
    pub output_text: Option<String>,
    pub error_text: Option<String>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub duration_ms: Option<i64>,
    /// Tool-specific correlation id (Codex call_id, Mistral tool_calls[].id; NULL for Claude/OpenCode)
    pub parser_call_id: Option<String>,
}
