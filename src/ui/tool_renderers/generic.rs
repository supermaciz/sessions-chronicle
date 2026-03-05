use crate::ui::markdown;
use crate::ui::tool_renderers::RendererInit;
use relm4::gtk;
use relm4::gtk::prelude::*;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputRenderPlan {
    PrettyJson(String),
    Markdown(String),
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericRenderedData {
    pub input_text: Option<String>,
    pub output: Option<OutputRenderPlan>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericRenderer {
    init: RendererInit,
}

#[allow(dead_code)]
impl GenericRenderer {
    pub fn new(init: RendererInit) -> Self {
        Self { init }
    }

    pub fn render_data(&self) -> GenericRenderedData {
        let input_text = self.init.input_json.as_deref().map(pretty_or_raw_json);
        let output = self
            .init
            .output_text
            .as_deref()
            .map(output_render_plan_from_text);

        GenericRenderedData { input_text, output }
    }

    pub fn render_output_textview(&self) -> Option<gtk::TextView> {
        match self.render_data().output {
            Some(OutputRenderPlan::PrettyJson(text)) => Some(plain_text_to_textview(&text)),
            Some(OutputRenderPlan::Markdown(text)) => {
                let (view, _) = markdown::render_markdown_to_textview(&text, None);
                Some(view)
            }
            None => None,
        }
    }
}

#[allow(dead_code)]
pub fn pretty_or_raw_json(text: &str) -> String {
    try_pretty_json(text).unwrap_or_else(|| text.to_string())
}

#[allow(dead_code)]
fn output_render_plan_from_text(text: &str) -> OutputRenderPlan {
    match try_pretty_json(text) {
        Some(pretty) => OutputRenderPlan::PrettyJson(pretty),
        None => OutputRenderPlan::Markdown(text.to_string()),
    }
}

#[allow(dead_code)]
fn try_pretty_json(text: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
}

#[allow(dead_code)]
fn plain_text_to_textview(text: &str) -> gtk::TextView {
    let buffer = gtk::TextBuffer::new(None);
    buffer.set_text(text);

    let view = gtk::TextView::with_buffer(&buffer);
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.set_monospace(true);
    view.set_hexpand(true);
    view
}

#[cfg(test)]
mod tests {
    use super::{GenericRenderer, OutputRenderPlan, pretty_or_raw_json};
    use crate::models::ToolCallStatus;
    use crate::ui::tool_renderers::RendererInit;

    fn init_with_output(output_text: Option<&str>) -> RendererInit {
        RendererInit {
            tool_name: "unknown".to_string(),
            input_json: None,
            output_text: output_text.map(str::to_string),
            error_text: None,
            status: ToolCallStatus::Completed,
            duration_ms: None,
        }
    }

    #[test]
    fn pretty_json_input_formats_with_two_space_indent() {
        let formatted = pretty_or_raw_json("{\"name\":\"Bash\",\"ok\":true}");
        let expected = "{\n  \"name\": \"Bash\",\n  \"ok\": true\n}";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn generic_output_uses_markdown_for_non_json_text() {
        let renderer = GenericRenderer::new(init_with_output(Some("**done**")));
        let rendered = renderer.render_data();

        assert_eq!(
            rendered.output,
            Some(OutputRenderPlan::Markdown("**done**".to_string()))
        );
    }
}
