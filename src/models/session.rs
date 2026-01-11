use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub tool: Tool,
    pub project_path: Option<String>,
    pub start_time: DateTime<Utc>,
    pub message_count: usize,
    pub file_path: String,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Tool {
    ClaudeCode,
    OpenCode,
    Codex,
}

impl Tool {
    pub fn color(&self) -> &'static str {
        match self {
            Tool::ClaudeCode => "#3584e4",
            Tool::OpenCode => "#26a269",
            Tool::Codex => "#e66100",
        }
    }

    pub fn icon_name(&self) -> &'static str {
        match self {
            Tool::ClaudeCode => "claude-code-symbolic",
            Tool::OpenCode => "opencode-symbolic",
            Tool::Codex => "codex-symbolic",
        }
    }

    pub fn session_dir(&self) -> String {
        let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/home/user"));
        match self {
            Tool::ClaudeCode => format!("{}/.claude/projects", home),
            Tool::OpenCode => format!("{}/.local/share/opencode/storage/session", home),
            Tool::Codex => format!("{}/.codex/sessions", home),
        }
    }
}
