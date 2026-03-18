use crate::ui::tool_renderers::RendererInit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentRenderedData {
    pub input_text: Option<String>,
    pub result_text: Option<String>,
    pub error_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentRenderer {
    init: RendererInit,
}

impl SubagentRenderer {
    pub fn new(init: RendererInit) -> Self {
        Self { init }
    }

    pub fn render_data(&self) -> SubagentRenderedData {
        let result_text = self
            .init
            .output_text
            .as_deref()
            .filter(|text| !text.is_empty())
            .map(str::to_string);
        let error_text = self
            .init
            .error_text
            .as_deref()
            .filter(|text| !text.is_empty())
            .map(str::to_string);
        let input_text = self
            .init
            .input_json
            .as_deref()
            .filter(|text| !text.is_empty())
            .map(str::to_string);

        SubagentRenderedData {
            input_text,
            result_text,
            error_text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SubagentRenderer;
    use crate::models::ToolCallStatus;
    use crate::ui::tool_renderers::RendererInit;

    #[test]
    fn subagent_renderer_keeps_input_result_and_error_separately() {
        let init = RendererInit {
            tool_name: "Task".to_string(),
            input_json: Some("{\"prompt\":\"investigate\"}".to_string()),
            output_text: Some("subagent complete".to_string()),
            error_text: Some("partial failure".to_string()),
            status: ToolCallStatus::Error,
            duration_ms: None,
        };

        let rendered = SubagentRenderer::new(init).render_data();
        assert_eq!(
            rendered.input_text.as_deref(),
            Some("{\"prompt\":\"investigate\"}")
        );
        assert_eq!(rendered.result_text.as_deref(), Some("subagent complete"));
        assert_eq!(rendered.error_text.as_deref(), Some("partial failure"));
    }
}
