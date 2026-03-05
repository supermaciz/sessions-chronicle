use crate::ui::tool_renderers::RendererInit;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentRenderedData {
    pub summary_text: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentRenderer {
    init: RendererInit,
}

#[allow(dead_code)]
impl SubagentRenderer {
    pub fn new(init: RendererInit) -> Self {
        Self { init }
    }

    pub fn render_data(&self) -> SubagentRenderedData {
        let summary_text = self
            .init
            .output_text
            .as_deref()
            .filter(|text| !text.is_empty())
            .or(self
                .init
                .error_text
                .as_deref()
                .filter(|text| !text.is_empty()))
            .or(self
                .init
                .input_json
                .as_deref()
                .filter(|text| !text.is_empty()))
            .map(str::to_string)
            .unwrap_or_else(|| {
                "Subagent details are available in the dedicated subagent inspector view."
                    .to_string()
            });

        SubagentRenderedData { summary_text }
    }
}
