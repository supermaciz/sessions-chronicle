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
    pub fn color(&self) -> &'static str {
        match self {
            Role::User => "#3584e4",
            Role::Assistant => "#26a269",
            Role::ToolCall => "#e66100",
            Role::ToolResult => "#1c71d8",
        }
    }
}
