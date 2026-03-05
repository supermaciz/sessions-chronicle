use crate::models::ToolCallStatus;
use crate::ui::tool_renderers::RendererInit;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRenderedData {
    pub command: Option<String>,
    pub output_text: Option<String>,
    pub error_text: Option<String>,
    pub display_text: Option<String>,
    pub exit_code: Option<i64>,
    pub is_non_zero_exit: bool,
    pub status: ToolCallStatus,
    pub duration_ms: Option<i64>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRenderer {
    init: RendererInit,
}

#[allow(dead_code)]
impl TerminalRenderer {
    pub fn new(init: RendererInit) -> Self {
        Self { init }
    }

    pub fn render_data(&self) -> TerminalRenderedData {
        let command = extract_command(self.init.input_json.as_deref());
        let output_text = self.init.output_text.clone();
        let error_text = self.init.error_text.clone();
        let display_text = output_text.clone().or_else(|| error_text.clone());
        let exit_code = infer_exit_code(output_text.as_deref(), error_text.as_deref());

        TerminalRenderedData {
            command,
            output_text,
            error_text,
            display_text,
            exit_code,
            is_non_zero_exit: exit_code.is_some_and(|code| code != 0),
            status: self.init.status,
            duration_ms: self.init.duration_ms,
        }
    }
}

#[allow(dead_code)]
pub fn extract_command(input_json: Option<&str>) -> Option<String> {
    let raw = input_json?;
    let value = serde_json::from_str::<serde_json::Value>(raw).ok()?;

    for key in ["command", "cmd"] {
        if let Some(command) = value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return Some(command.to_string());
        }
    }

    None
}

#[allow(dead_code)]
pub fn infer_exit_code(output_text: Option<&str>, error_text: Option<&str>) -> Option<i64> {
    [output_text, error_text]
        .into_iter()
        .flatten()
        .find_map(exit_code_from_text)
}

fn exit_code_from_text(text: &str) -> Option<i64> {
    let lower = text.to_ascii_lowercase();

    for pattern in ["exit code", "exited with code"] {
        if let Some(index) = lower.find(pattern) {
            let suffix = &text[index + pattern.len()..];
            if let Some(code) = first_integer(suffix) {
                return Some(code);
            }
        }
    }

    None
}

fn first_integer(text: &str) -> Option<i64> {
    let start = text
        .char_indices()
        .find(|(_, ch)| ch.is_ascii_digit() || *ch == '-')
        .map(|(index, _)| index)?;
    let candidate = &text[start..];
    let end = candidate
        .char_indices()
        .find(|(index, ch)| *index > 0 && !ch.is_ascii_digit())
        .map(|(index, _)| index)
        .unwrap_or(candidate.len());

    candidate[..end].parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::TerminalRenderer;
    use crate::models::ToolCallStatus;
    use crate::ui::tool_renderers::RendererInit;

    #[test]
    fn terminal_renderer_extracts_command_from_input_json() {
        let init = RendererInit {
            tool_name: "Bash".to_string(),
            input_json: Some("{\"command\":\"cargo test\"}".to_string()),
            output_text: Some("ok".to_string()),
            error_text: None,
            status: ToolCallStatus::Completed,
            duration_ms: Some(42),
        };

        let renderer = TerminalRenderer::new(init);
        let rendered = renderer.render_data();

        assert_eq!(rendered.command.as_deref(), Some("cargo test"));
    }

    #[test]
    fn terminal_renderer_detects_non_zero_exit_code() {
        let init = RendererInit {
            tool_name: "shell".to_string(),
            input_json: Some("{\"cmd\":\"false\"}".to_string()),
            output_text: Some("Process exited with code 17".to_string()),
            error_text: None,
            status: ToolCallStatus::Error,
            duration_ms: None,
        };

        let renderer = TerminalRenderer::new(init);
        let rendered = renderer.render_data();

        assert_eq!(rendered.exit_code, Some(17));
        assert!(rendered.is_non_zero_exit);
    }
}
