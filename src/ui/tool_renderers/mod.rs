use crate::models::ToolCallStatus;

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
    match tool_name {
        "bash" | "shell" | "exec_command" => RendererKind::Terminal,
        "edit" | "apply_patch" => RendererKind::Diff,
        "read" | "write" => RendererKind::File,
        "grep" | "search" | "glob" => RendererKind::Results,
        "agent" | "task" => RendererKind::Subagent,
        _ => RendererKind::Generic,
    }
}

#[cfg(test)]
mod tests {
    use super::{RendererKind, resolve_renderer};

    #[test]
    fn resolve_renderer_maps_bash_to_terminal() {
        assert_eq!(resolve_renderer("bash"), RendererKind::Terminal);
    }

    #[test]
    fn resolve_renderer_maps_edit_to_diff() {
        assert_eq!(resolve_renderer("edit"), RendererKind::Diff);
    }

    #[test]
    fn resolve_renderer_unknown_uses_generic() {
        assert_eq!(resolve_renderer("unknown_tool"), RendererKind::Generic);
    }
}
