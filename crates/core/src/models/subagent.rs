use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subagent {
    /// Session-scoped identifier
    pub id: String,
    /// Durable Claude agent identifier (for Task/Agent tool calls)
    pub agent_id: Option<String>,
    /// Teammate name for Claude Code v2.1.216+ subagent launches, taken from
    /// the `Agent`/`Task` tool call's `input.name`. The only value shared with
    /// the nested child transcript's filename.
    pub agent_name: Option<String>,
    pub session_id: String,
    pub title: String,
    pub prompt: Option<String>,
    pub result_summary: Option<String>,
    /// Links to a child session when the subagent spawned its own session
    pub child_session_id: Option<String>,
    pub parser_ref: Option<String>,
}
