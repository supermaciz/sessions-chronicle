use crate::ui::markdown;
use crate::ui::tool_renderers::RendererInit;
use relm4::gtk;
use relm4::gtk::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputRenderPlan {
    PrettyJson(String),
    Markdown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericRenderedData {
    pub input_text: Option<String>,
    pub output: Option<OutputRenderPlan>,
    pub error: Option<OutputRenderPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericRenderer {
    init: RendererInit,
}

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
            .filter(|text| !text.is_empty())
            .map(output_render_plan_from_text);
        let error = self
            .init
            .error_text
            .as_deref()
            .filter(|text| !text.is_empty())
            .map(output_render_plan_from_text);

        GenericRenderedData {
            input_text,
            output,
            error,
        }
    }

    #[allow(dead_code)]
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

pub fn pretty_or_raw_json(text: &str) -> String {
    try_pretty_json(text).unwrap_or_else(|| text.to_string())
}

fn output_render_plan_from_text(text: &str) -> OutputRenderPlan {
    match try_pretty_json(text) {
        Some(pretty) => OutputRenderPlan::PrettyJson(pretty),
        None => OutputRenderPlan::Markdown(text.to_string()),
    }
}

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

    fn init_with_data(
        input_json: Option<&str>,
        output_text: Option<&str>,
        error_text: Option<&str>,
    ) -> RendererInit {
        RendererInit {
            tool_name: "unknown".to_string(),
            input_json: input_json.map(str::to_string),
            output_text: output_text.map(str::to_string),
            error_text: error_text.map(str::to_string),
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
        let renderer = GenericRenderer::new(init_with_data(None, Some("**done**"), None));
        let rendered = renderer.render_data();

        assert_eq!(
            rendered.output,
            Some(OutputRenderPlan::Markdown("**done**".to_string()))
        );
    }

    #[test]
    fn generic_input_invalid_json_passthrough_unchanged() {
        let raw = "{not valid json";
        let renderer = GenericRenderer::new(init_with_data(Some(raw), None, None));
        let rendered = renderer.render_data();

        assert_eq!(rendered.input_text, Some(raw.to_string()));
    }

    #[test]
    fn generic_output_json_uses_pretty_print_path() {
        let renderer = GenericRenderer::new(init_with_data(None, Some("{\"ok\":true}"), None));
        let rendered = renderer.render_data();

        assert_eq!(
            rendered.output,
            Some(OutputRenderPlan::PrettyJson(
                "{\n  \"ok\": true\n}".to_string()
            ))
        );
    }

    #[test]
    fn generic_output_none_when_output_and_error_missing() {
        let renderer = GenericRenderer::new(init_with_data(None, None, None));

        assert_eq!(renderer.render_data().output, None);
    }

    #[test]
    fn generic_output_falls_back_to_error_text_when_output_missing() {
        let renderer = GenericRenderer::new(init_with_data(None, None, Some("tool failed")));

        assert_eq!(renderer.render_data().output, None);
        assert_eq!(
            renderer.render_data().error,
            Some(OutputRenderPlan::Markdown("tool failed".to_string()))
        );
    }

    #[test]
    fn generic_output_preserves_output_and_error_channels() {
        let renderer = GenericRenderer::new(init_with_data(
            None,
            Some("{\"ok\":true}"),
            Some("tool failed"),
        ));

        let rendered = renderer.render_data();
        assert_eq!(
            rendered.output,
            Some(OutputRenderPlan::PrettyJson(
                "{\n  \"ok\": true\n}".to_string()
            ))
        );
        assert_eq!(
            rendered.error,
            Some(OutputRenderPlan::Markdown("tool failed".to_string()))
        );
    }
}
