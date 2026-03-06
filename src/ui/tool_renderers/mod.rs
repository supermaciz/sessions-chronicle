use crate::models::ToolCallStatus;

pub mod diff;
pub mod file;
pub mod generic;
pub mod results;
pub mod subagent;
pub mod terminal;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererKind {
    Terminal,
    Diff,
    File,
    Results,
    Generic,
    Subagent,
}

impl RendererKind {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Diff => "diff",
            Self::File => "file",
            Self::Results => "results",
            Self::Generic => "generic",
            Self::Subagent => "subagent",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererInit {
    pub tool_name: String,
    pub input_json: Option<String>,
    pub output_text: Option<String>,
    pub error_text: Option<String>,
    pub status: ToolCallStatus,
    pub duration_ms: Option<i64>,
}

#[allow(dead_code)]
pub fn resolve_renderer(tool_name: &str) -> RendererKind {
    let normalized = tool_name.trim().to_ascii_lowercase();
    let tokenized = normalized.replace('-', "_");

    let base_kind = match normalized.as_str() {
        "bash" | "shell" | "exec_command" => RendererKind::Terminal,
        "edit" | "apply_patch" => RendererKind::Diff,
        "read" | "write" => RendererKind::File,
        "grep" | "search" | "glob" => RendererKind::Results,
        _ => RendererKind::Generic,
    };

    if base_kind == RendererKind::Generic
        && (matches!(tokenized.as_str(), "agent" | "task")
            || tokenized.ends_with("_agent")
            || tokenized.ends_with("_task"))
    {
        RendererKind::Subagent
    } else {
        base_kind
    }
}

#[cfg(test)]
mod tests {
    use super::{RendererKind, resolve_renderer};

    #[test]
    fn resolve_renderer_maps_terminal_aliases_case_insensitive() {
        for tool_name in ["bash", "Bash", " shell ", "EXEC_COMMAND"] {
            assert_eq!(resolve_renderer(tool_name), RendererKind::Terminal);
        }
    }

    #[test]
    fn resolve_renderer_maps_diff_aliases_case_insensitive() {
        for tool_name in ["edit", "Edit", " apply_patch ", "APPLY_PATCH"] {
            assert_eq!(resolve_renderer(tool_name), RendererKind::Diff);
        }
    }

    #[test]
    fn resolve_renderer_maps_file_aliases_case_insensitive() {
        for tool_name in ["read", "Read", " write ", "WRITE"] {
            assert_eq!(resolve_renderer(tool_name), RendererKind::File);
        }
    }

    #[test]
    fn resolve_renderer_maps_results_aliases_case_insensitive() {
        for tool_name in ["grep", "Grep", " search ", "GLOB"] {
            assert_eq!(resolve_renderer(tool_name), RendererKind::Results);
        }
    }

    #[test]
    fn resolve_renderer_uses_subagent_view_for_task_and_agent_tools() {
        for tool_name in [
            "agent",
            "Agent",
            " task ",
            "TASK",
            "run_task",
            "delegate_agent",
        ] {
            assert_eq!(resolve_renderer(tool_name), RendererKind::Subagent);
        }
    }

    #[test]
    fn resolve_renderer_unknown_uses_generic_after_normalization() {
        assert_eq!(resolve_renderer("unknown_tool"), RendererKind::Generic);
        assert_eq!(resolve_renderer(" Unknown_Tool "), RendererKind::Generic);
    }
}
