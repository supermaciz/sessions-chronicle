use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::token_usage::TokenUsage;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndingStatus {
    /// No tool calls present or status is truly unknown.
    #[default]
    Unknown,
    /// Last tool call completed successfully.
    Clean,
    /// Last tool call is still pending or running.
    Abrupt,
    /// Last tool call ended with an error.
    Error,
}

impl SessionEndingStatus {
    pub fn from_storage(value: &str) -> Self {
        match value {
            "clean" => SessionEndingStatus::Clean,
            "abrupt" => SessionEndingStatus::Abrupt,
            "error" => SessionEndingStatus::Error,
            _ => SessionEndingStatus::Unknown,
        }
    }

    pub fn to_storage(&self) -> &'static str {
        match self {
            SessionEndingStatus::Clean => "clean",
            SessionEndingStatus::Abrupt => "abrupt",
            SessionEndingStatus::Error => "error",
            SessionEndingStatus::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub tool: AiAssistant,
    pub project_path: Option<String>,
    #[serde(default)]
    pub project_id: Option<i64>,
    pub start_time: DateTime<Utc>,
    pub message_count: usize,
    pub file_path: String,
    pub last_updated: DateTime<Utc>,
    #[serde(default)]
    pub pinned_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub first_prompt: Option<String>,
    #[serde(default)]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub is_subagent: bool,
    #[serde(default)]
    pub token_usage: Option<TokenUsage>,
    #[serde(default)]
    pub edit_count: usize,
    #[serde(default)]
    pub read_count: usize,
    #[serde(default)]
    pub command_count: usize,
    #[serde(default)]
    pub ending_status: SessionEndingStatus,
}

/// AI coding assistant whose sessions are tracked by this application.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AiAssistant {
    /// Anthropic's Claude Code CLI agent.
    ClaudeCode,
    /// OpenCode terminal-based AI coding agent.
    OpenCode,
    /// OpenAI Codex CLI agent.
    Codex,
    /// Mistral Vibe coding assistant.
    MistralVibe,
}

impl AiAssistant {
    pub const ALL: &'static [AiAssistant] = &[
        AiAssistant::ClaudeCode,
        AiAssistant::OpenCode,
        AiAssistant::Codex,
        AiAssistant::MistralVibe,
    ];

    #[allow(dead_code)]
    pub fn color(&self) -> &'static str {
        match self {
            AiAssistant::ClaudeCode => "#3584e4",
            AiAssistant::OpenCode => "#26a269",
            AiAssistant::Codex => "#e66100",
            AiAssistant::MistralVibe => "#1c71d8",
        }
    }

    pub fn icon_name(&self) -> &'static str {
        match self {
            AiAssistant::ClaudeCode => "claude-code-symbolic",
            AiAssistant::OpenCode => "opencode-symbolic",
            AiAssistant::Codex => "codex-symbolic",
            AiAssistant::MistralVibe => "mistral-vibe-symbolic",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            AiAssistant::ClaudeCode => "Claude Code",
            AiAssistant::OpenCode => "OpenCode",
            AiAssistant::Codex => "Codex",
            AiAssistant::MistralVibe => "Mistral Vibe",
        }
    }

    pub fn from_storage(value: &str) -> Option<Self> {
        match value {
            "claude_code" => Some(AiAssistant::ClaudeCode),
            "opencode" => Some(AiAssistant::OpenCode),
            "codex" => Some(AiAssistant::Codex),
            "mistral_vibe" => Some(AiAssistant::MistralVibe),
            _ => None,
        }
    }

    pub fn to_storage(self) -> String {
        match self {
            AiAssistant::ClaudeCode => "claude_code".to_string(),
            AiAssistant::OpenCode => "opencode".to_string(),
            AiAssistant::Codex => "codex".to_string(),
            AiAssistant::MistralVibe => "mistral_vibe".to_string(),
        }
    }

    pub fn session_dir(&self) -> String {
        let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/home/user"));
        match self {
            AiAssistant::ClaudeCode => format!("{}/.claude/projects", home),
            AiAssistant::OpenCode => format!("{}/.local/share/opencode/storage/session", home),
            AiAssistant::Codex => format!("{}/.codex/sessions", home),
            AiAssistant::MistralVibe => std::env::var("VIBE_HOME")
                .map(|vibe_home| format!("{}/logs/session", vibe_home))
                .unwrap_or_else(|_| format!("{}/.vibe/logs/session", home)),
        }
    }
}
