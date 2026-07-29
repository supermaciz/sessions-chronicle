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
    /// Moonshot AI's Kimi Code CLI assistant.
    KimiCode,
}

fn resolve_kimi_home(home: &str, configured: Option<std::ffi::OsString>) -> String {
    configured
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{home}/.kimi-code"))
}

impl AiAssistant {
    pub const ALL: &'static [AiAssistant] = &[
        AiAssistant::ClaudeCode,
        AiAssistant::OpenCode,
        AiAssistant::Codex,
        AiAssistant::MistralVibe,
        AiAssistant::KimiCode,
    ];

    #[allow(dead_code)]
    pub fn color(&self) -> &'static str {
        match self {
            AiAssistant::ClaudeCode => "#3584e4",
            AiAssistant::OpenCode => "#26a269",
            AiAssistant::Codex => "#e66100",
            AiAssistant::MistralVibe => "#1c71d8",
            AiAssistant::KimiCode => "#9141ac",
        }
    }

    pub fn icon_name(&self) -> &'static str {
        match self {
            AiAssistant::ClaudeCode => "claude-code-symbolic",
            AiAssistant::OpenCode => "opencode-symbolic",
            AiAssistant::Codex => "codex-symbolic",
            AiAssistant::MistralVibe => "mistral-vibe-symbolic",
            AiAssistant::KimiCode => "kimi-code-symbolic",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            AiAssistant::ClaudeCode => "Claude Code",
            AiAssistant::OpenCode => "OpenCode",
            AiAssistant::Codex => "Codex",
            AiAssistant::MistralVibe => "Mistral Vibe",
            AiAssistant::KimiCode => "Kimi Code",
        }
    }

    pub fn from_storage(value: &str) -> Option<Self> {
        match value {
            "claude_code" => Some(AiAssistant::ClaudeCode),
            "opencode" => Some(AiAssistant::OpenCode),
            "codex" => Some(AiAssistant::Codex),
            "mistral_vibe" => Some(AiAssistant::MistralVibe),
            "kimi_code" => Some(AiAssistant::KimiCode),
            _ => None,
        }
    }

    pub fn to_storage(self) -> String {
        match self {
            AiAssistant::ClaudeCode => "claude_code".to_string(),
            AiAssistant::OpenCode => "opencode".to_string(),
            AiAssistant::Codex => "codex".to_string(),
            AiAssistant::MistralVibe => "mistral_vibe".to_string(),
            AiAssistant::KimiCode => "kimi_code".to_string(),
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
            AiAssistant::KimiCode => {
                resolve_kimi_home(home.as_str(), std::env::var_os("KIMI_CODE_HOME"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AiAssistant;
    use std::ffi::OsString;

    #[test]
    fn kimi_code_identity_mappings_are_stable() {
        assert_eq!(AiAssistant::ALL.len(), 5);
        assert_eq!(AiAssistant::KimiCode.to_storage(), "kimi_code");
        assert_eq!(
            AiAssistant::from_storage("kimi_code"),
            Some(AiAssistant::KimiCode)
        );
        assert_eq!(AiAssistant::KimiCode.display_name(), "Kimi Code");
        assert_eq!(AiAssistant::KimiCode.icon_name(), "kimi-code-symbolic");
    }

    #[test]
    fn kimi_home_path_prefers_custom_root_and_never_appends_sessions() {
        assert_eq!(
            super::resolve_kimi_home("/home/tester", Some(OsString::from("/tmp/kimi"))),
            "/tmp/kimi"
        );
        assert_eq!(
            super::resolve_kimi_home("/home/tester", None),
            "/home/tester/.kimi-code"
        );
    }
}
