use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TranscriptItemKind {
    Message,
    ToolCall,
    Subagent,
}

impl TranscriptItemKind {
    pub fn from_storage(value: &str) -> Option<Self> {
        match value {
            "message" => Some(Self::Message),
            "tool_call" => Some(Self::ToolCall),
            "subagent" => Some(Self::Subagent),
            _ => None,
        }
    }

    pub fn to_storage(&self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::ToolCall => "tool_call",
            Self::Subagent => "subagent",
        }
    }
}

/// A single entry in the ordered transcript stream for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptItem {
    pub session_id: String,
    pub item_index: i64,
    pub kind: TranscriptItemKind,
    /// Set when kind == Message
    pub message_index: Option<i64>,
    /// Set when kind == ToolCall
    pub tool_call_id: Option<String>,
    /// Set when kind == Subagent
    pub subagent_id: Option<String>,
}
