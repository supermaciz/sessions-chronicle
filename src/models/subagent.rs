use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subagent {
    /// Session-scoped identifier
    pub id: String,
    /// Durable Claude agent identifier (for Task/Agent tool calls)
    pub agent_id: Option<String>,
    pub session_id: String,
    pub title: String,
    pub prompt: Option<String>,
    pub result_summary: Option<String>,
    /// Links to a child session when the subagent spawned its own session
    pub child_session_id: Option<String>,
    pub parser_ref: Option<String>,
}
