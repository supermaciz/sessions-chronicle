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
    pub const ALL: &'static [Tool] = &[Tool::ClaudeCode, Tool::OpenCode, Tool::Codex];

    #[allow(dead_code)]
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

    pub fn display_name(&self) -> &'static str {
        match self {
            Tool::ClaudeCode => "Claude Code",
            Tool::OpenCode => "OpenCode",
            Tool::Codex => "Codex",
        }
    }

    pub fn from_storage(value: &str) -> Option<Self> {
        match value {
            "claude_code" => Some(Tool::ClaudeCode),
            "opencode" => Some(Tool::OpenCode),
            "codex" => Some(Tool::Codex),
            _ => None,
        }
    }

    pub fn to_storage(self) -> String {
        match self {
            Tool::ClaudeCode => "claude_code".to_string(),
            Tool::OpenCode => "opencode".to_string(),
            Tool::Codex => "codex".to_string(),
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
