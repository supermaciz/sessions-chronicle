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

/// Broad category for activity counting in session rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    Edit,
    Command,
    Read,
    Other,
}

/// Classify a tool name into a broad activity category.
///
/// Tool names vary across AI assistants (Claude Code uses PascalCase,
/// OpenCode/Codex/Mistral use snake_case). This function matches known
/// names case-sensitively because each assistant is consistent within
/// itself.
pub fn classify_tool_name(name: &str) -> ToolCategory {
    match name {
        // Edits (file-modifying operations)
        "Edit" | "Write" | "NotebookEdit" | "MultiEdit" => ToolCategory::Edit,

        // Commands (shell execution)
        "Bash" | "bash" | "exec_command" => ToolCategory::Command,

        // Reads (file/codebase exploration)
        "Read" | "read" | "Glob" | "Grep" | "grep" | "read_file" | "list_directory"
        | "list_files" => ToolCategory::Read,

        // Everything else (Task, Agent, WebSearch, etc.)
        _ => ToolCategory::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_edit_tools() {
        assert_eq!(classify_tool_name("Edit"), ToolCategory::Edit);
        assert_eq!(classify_tool_name("Write"), ToolCategory::Edit);
        assert_eq!(classify_tool_name("NotebookEdit"), ToolCategory::Edit);
        assert_eq!(classify_tool_name("MultiEdit"), ToolCategory::Edit);
    }

    #[test]
    fn classify_command_tools() {
        assert_eq!(classify_tool_name("Bash"), ToolCategory::Command);
        assert_eq!(classify_tool_name("bash"), ToolCategory::Command);
        assert_eq!(classify_tool_name("exec_command"), ToolCategory::Command);
    }

    #[test]
    fn classify_read_tools() {
        assert_eq!(classify_tool_name("Read"), ToolCategory::Read);
        assert_eq!(classify_tool_name("Glob"), ToolCategory::Read);
        assert_eq!(classify_tool_name("Grep"), ToolCategory::Read);
        assert_eq!(classify_tool_name("read"), ToolCategory::Read);
        assert_eq!(classify_tool_name("grep"), ToolCategory::Read);
        assert_eq!(classify_tool_name("read_file"), ToolCategory::Read);
        assert_eq!(classify_tool_name("list_directory"), ToolCategory::Read);
    }

    #[test]
    fn classify_unknown_tools_as_other() {
        assert_eq!(classify_tool_name("Task"), ToolCategory::Other);
        assert_eq!(classify_tool_name("Agent"), ToolCategory::Other);
        assert_eq!(classify_tool_name("my_tool"), ToolCategory::Other);
        assert_eq!(classify_tool_name("WebSearch"), ToolCategory::Other);
        assert_eq!(classify_tool_name(""), ToolCategory::Other);
    }
}
