use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub session_id: String,
    pub index: usize,
    pub role: Role,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    ToolCall,
    ToolResult,
}

impl Role {
    #[allow(dead_code)]
    pub fn color(&self) -> &'static str {
        match self {
            Role::User => "#3584e4",
            Role::Assistant => "#26a269",
            Role::ToolCall => "#e66100",
            Role::ToolResult => "#1c71d8",
        }
    }

    /// Parse a role from the database storage format.
    pub fn from_storage(s: &str) -> Option<Role> {
        match s.to_lowercase().as_str() {
            "user" => Some(Role::User),
            "assistant" => Some(Role::Assistant),
            "toolcall" | "tool_call" => Some(Role::ToolCall),
            "toolresult" | "tool_result" => Some(Role::ToolResult),
            _ => None,
        }
    }

    /// Return the storage format string for this role.
    #[allow(dead_code)]
    pub fn to_storage(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::ToolCall => "toolcall",
            Role::ToolResult => "toolresult",
        }
    }

    /// Return a display label for the role.
    pub fn label(&self) -> &'static str {
        match self {
            Role::User => "USER",
            Role::Assistant => "ASSISTANT",
            Role::ToolCall => "TOOL CALL",
            Role::ToolResult => "TOOL RESULT",
        }
    }

    /// Return the CSS class for styling this role.
    pub fn css_class(&self) -> &'static str {
        match self {
            Role::User => "role-user",
            Role::Assistant => "role-assistant",
            Role::ToolCall => "role-toolcall",
            Role::ToolResult => "role-toolresult",
        }
    }
}
