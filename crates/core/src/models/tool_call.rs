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
    Search,
    Agent,
    Web,
    Plan,
    Skill,
    UserInput,
    Other,
}

impl ToolCategory {
    /// Returns the symbolic icon name for this category.
    ///
    /// Callers must pass the appropriate `icon_names` constants from the
    /// binary crate.  Use [`tool_category_icon`] for the convenience wrapper.
    pub fn icon_name(self, icons: &ToolCategoryIcons) -> &'static str {
        match self {
            Self::Read => icons.read,
            Self::Edit => icons.edit,
            Self::Command => icons.command,
            Self::Search => icons.search,
            Self::Agent => icons.agent,
            Self::Web => icons.web,
            Self::Plan => icons.plan,
            Self::Skill => icons.skill,
            Self::UserInput => icons.user_input,
            Self::Other => icons.other,
        }
    }
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
        "Edit"
        | "Write"
        | "NotebookEdit"
        | "MultiEdit"
        | "edit"
        | "write"
        | "write_file"
        | "apply_patch"
        | "search_replace"
        | "file_system.write_file"
        | "file_system.search_replace" => ToolCategory::Edit,

        // Commands (shell execution)
        "Bash" | "bash" | "exec_command" | "shell" | "shell_command" | "write_stdin"
        | "file_system.bash" => ToolCategory::Command,

        // Reads (file content)
        "Read"
        | "read"
        | "read_file"
        | "list_directory"
        | "list_files"
        | "list"
        | "file_system.read_file" => ToolCategory::Read,

        // Search (codebase exploration)
        "Glob" | "Grep" | "grep" | "glob" | "codesearch" => ToolCategory::Search,

        // Agent (subagent spawning)
        "Agent"
        | "Task"
        | "TaskCreate"
        | "TaskUpdate"
        | "agents.spawn"
        | "agents.list"
        | "agents.list_configs"
        | "agents.send_message"
        | "agents.interrupt" => ToolCategory::Agent,

        // Web (network operations)
        "WebSearch"
        | "WebFetch"
        | "websearch"
        | "webfetch"
        | "web_search"
        | "web_fetch"
        | "connector_web_search.web_search"
        | "connector_web_search.open_url" => ToolCategory::Web,

        // Plan (task lists / planning)
        "TodoWrite" | "todowrite" | "todoread" | "todo" | "update_plan" | "vibe.todo" => {
            ToolCategory::Plan
        }

        // Skill (reusable capability invocation)
        "Skill" | "skill" | "skill.read" => ToolCategory::Skill,

        // User input (prompts directed at the user)
        "AskUserQuestion"
        | "ask_user_question"
        | "question"
        | "request_user_input"
        | "ui.ask_user_question" => ToolCategory::UserInput,

        // Everything else
        _ => ToolCategory::Other,
    }
}

/// Icon name constants for each tool category, bridging the binary
/// crate's generated `icon_names` module to the library crate.
pub struct ToolCategoryIcons {
    pub read: &'static str,
    pub edit: &'static str,
    pub command: &'static str,
    pub search: &'static str,
    pub agent: &'static str,
    pub web: &'static str,
    pub plan: &'static str,
    pub skill: &'static str,
    pub user_input: &'static str,
    pub other: &'static str,
}

/// Returns the icon name for a specific tool call name.
pub fn tool_name_icon(name: &str, icons: &ToolCategoryIcons) -> &'static str {
    classify_tool_name(name).icon_name(icons)
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
        // OpenCode / Codex / Mistral Vibe edit variants
        assert_eq!(classify_tool_name("edit"), ToolCategory::Edit);
        assert_eq!(classify_tool_name("write"), ToolCategory::Edit);
        assert_eq!(classify_tool_name("write_file"), ToolCategory::Edit);
        assert_eq!(classify_tool_name("apply_patch"), ToolCategory::Edit);
        assert_eq!(classify_tool_name("search_replace"), ToolCategory::Edit);
    }

    #[test]
    fn classify_command_tools() {
        assert_eq!(classify_tool_name("Bash"), ToolCategory::Command);
        assert_eq!(classify_tool_name("bash"), ToolCategory::Command);
        assert_eq!(classify_tool_name("exec_command"), ToolCategory::Command);
        // Codex shell variants
        assert_eq!(classify_tool_name("shell"), ToolCategory::Command);
        assert_eq!(classify_tool_name("shell_command"), ToolCategory::Command);
        assert_eq!(classify_tool_name("write_stdin"), ToolCategory::Command);
    }

    #[test]
    fn classify_read_tools() {
        assert_eq!(classify_tool_name("Read"), ToolCategory::Read);
        assert_eq!(classify_tool_name("read"), ToolCategory::Read);
        assert_eq!(classify_tool_name("read_file"), ToolCategory::Read);
        assert_eq!(classify_tool_name("list_directory"), ToolCategory::Read);
        assert_eq!(classify_tool_name("list"), ToolCategory::Read);
    }

    #[test]
    fn classify_search_tools() {
        assert_eq!(classify_tool_name("Glob"), ToolCategory::Search);
        assert_eq!(classify_tool_name("Grep"), ToolCategory::Search);
        assert_eq!(classify_tool_name("grep"), ToolCategory::Search);
        // OpenCode search variants
        assert_eq!(classify_tool_name("glob"), ToolCategory::Search);
        assert_eq!(classify_tool_name("codesearch"), ToolCategory::Search);
    }

    #[test]
    fn classify_agent_tools() {
        assert_eq!(classify_tool_name("Agent"), ToolCategory::Agent);
        assert_eq!(classify_tool_name("Task"), ToolCategory::Agent);
        assert_eq!(classify_tool_name("TaskCreate"), ToolCategory::Agent);
        assert_eq!(classify_tool_name("TaskUpdate"), ToolCategory::Agent);
    }

    #[test]
    fn classify_web_tools() {
        assert_eq!(classify_tool_name("WebSearch"), ToolCategory::Web);
        assert_eq!(classify_tool_name("WebFetch"), ToolCategory::Web);
        // Mistral Vibe snake_case variants
        assert_eq!(classify_tool_name("web_search"), ToolCategory::Web);
        assert_eq!(classify_tool_name("web_fetch"), ToolCategory::Web);
    }

    #[test]
    fn classify_plan_tools() {
        assert_eq!(classify_tool_name("TodoWrite"), ToolCategory::Plan);
        assert_eq!(classify_tool_name("todowrite"), ToolCategory::Plan);
        assert_eq!(classify_tool_name("todoread"), ToolCategory::Plan);
        assert_eq!(classify_tool_name("todo"), ToolCategory::Plan);
        assert_eq!(classify_tool_name("update_plan"), ToolCategory::Plan);
    }

    #[test]
    fn classify_skill_tools() {
        assert_eq!(classify_tool_name("Skill"), ToolCategory::Skill);
        assert_eq!(classify_tool_name("skill"), ToolCategory::Skill);
    }

    #[test]
    fn classify_user_input_tools() {
        assert_eq!(
            classify_tool_name("AskUserQuestion"),
            ToolCategory::UserInput
        );
        assert_eq!(
            classify_tool_name("ask_user_question"),
            ToolCategory::UserInput
        );
        assert_eq!(classify_tool_name("question"), ToolCategory::UserInput);
        assert_eq!(
            classify_tool_name("request_user_input"),
            ToolCategory::UserInput
        );
    }

    #[test]
    fn classify_unknown_tools_as_other() {
        assert_eq!(classify_tool_name("my_tool"), ToolCategory::Other);
        assert_eq!(classify_tool_name(""), ToolCategory::Other);
    }

    #[test]
    fn classify_vibe_unified_qualified_tools() {
        // Mistral Vibe Unified writes qualified tool names (fact H).
        assert_eq!(
            classify_tool_name("file_system.write_file"),
            ToolCategory::Edit
        );
        assert_eq!(
            classify_tool_name("file_system.search_replace"),
            ToolCategory::Edit
        );
        assert_eq!(
            classify_tool_name("file_system.bash"),
            ToolCategory::Command
        );
        assert_eq!(
            classify_tool_name("file_system.read_file"),
            ToolCategory::Read
        );
        assert_eq!(classify_tool_name("agents.spawn"), ToolCategory::Agent);
        assert_eq!(classify_tool_name("agents.list"), ToolCategory::Agent);
        assert_eq!(
            classify_tool_name("agents.list_configs"),
            ToolCategory::Agent
        );
        assert_eq!(
            classify_tool_name("agents.send_message"),
            ToolCategory::Agent
        );
        assert_eq!(classify_tool_name("agents.interrupt"), ToolCategory::Agent);
        assert_eq!(
            classify_tool_name("connector_web_search.web_search"),
            ToolCategory::Web
        );
        assert_eq!(
            classify_tool_name("connector_web_search.open_url"),
            ToolCategory::Web
        );
        assert_eq!(classify_tool_name("vibe.todo"), ToolCategory::Plan);
        assert_eq!(classify_tool_name("skill.read"), ToolCategory::Skill);
        assert_eq!(
            classify_tool_name("ui.ask_user_question"),
            ToolCategory::UserInput
        );
    }

    #[test]
    fn unknown_connector_suffix_is_not_stripped() {
        // Only exact qualified names classify: connectors outside the
        // pinned list stay Other, whatever suffix they carry.
        assert_eq!(
            classify_tool_name("connector_acme.bash"),
            ToolCategory::Other
        );
        assert_eq!(
            classify_tool_name("connector_github_app.search_code"),
            ToolCategory::Other
        );
    }
}
