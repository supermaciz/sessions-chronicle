use crate::models::ToolCallStatus;
use crate::ui::tool_renderers::RendererInit;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRenderedData {
    pub header: Option<String>,
    pub output_text: Option<String>,
    pub error_text: Option<String>,
    pub status: ToolCallStatus,
    pub duration_ms: Option<i64>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRenderer {
    init: RendererInit,
}

#[allow(dead_code)]
impl FileRenderer {
    pub fn new(init: RendererInit) -> Self {
        Self { init }
    }

    pub fn render_data(&self) -> FileRenderedData {
        let header = self
            .init
            .input_json
            .as_deref()
            .and_then(parse_file_header_from_input);

        FileRenderedData {
            header,
            output_text: self
                .init
                .output_text
                .clone()
                .filter(|text| !text.is_empty()),
            error_text: self.init.error_text.clone().filter(|text| !text.is_empty()),
            status: self.init.status,
            duration_ms: self.init.duration_ms,
        }
    }
}

#[allow(dead_code)]
pub fn format_file_header(path: &str, offset: Option<i64>, limit: Option<i64>) -> String {
    match (offset, limit) {
        (Some(start), Some(count)) if start >= 1 && count >= 1 => {
            let end = start.saturating_add(count.saturating_sub(1));
            format!("{path}:{start}-{end}")
        }
        (Some(start), _) if start >= 1 => format!("{path}:{start}"),
        _ => path.to_string(),
    }
}

fn parse_file_header_from_input(input_json: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(input_json).ok()?;
    let path = ["file_path", "filePath", "path"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|path| !path.is_empty())?;

    let offset = ["offset", "start"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(serde_json::Value::as_i64));
    let limit = ["limit", "count"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(serde_json::Value::as_i64));

    Some(format_file_header(path, offset, limit))
}

#[cfg(test)]
mod tests {
    use super::{FileRenderer, format_file_header};
    use crate::models::ToolCallStatus;
    use crate::ui::tool_renderers::RendererInit;

    #[test]
    fn file_renderer_formats_path_and_line_range_header() {
        let init = RendererInit {
            tool_name: "Read".to_string(),
            input_json: Some(r#"{"path":"src/main.rs","offset":42,"limit":40}"#.to_string()),
            output_text: Some("fn main() {}".to_string()),
            error_text: None,
            status: ToolCallStatus::Completed,
            duration_ms: Some(12),
        };

        let rendered = FileRenderer::new(init).render_data();
        assert_eq!(rendered.header.as_deref(), Some("src/main.rs:42-81"));
    }

    #[test]
    fn file_renderer_does_not_emit_invalid_ranges() {
        assert_eq!(
            format_file_header("src/main.rs", Some(0), Some(40)),
            "src/main.rs"
        );
        assert_eq!(
            format_file_header("src/main.rs", Some(42), Some(0)),
            "src/main.rs:42"
        );
        assert_eq!(
            format_file_header("src/main.rs", Some(-5), Some(2)),
            "src/main.rs"
        );
    }

    #[test]
    fn file_renderer_ignores_invalid_numeric_input_fields() {
        let init = RendererInit {
            tool_name: "Read".to_string(),
            input_json: Some(r#"{"path":"src/main.rs","offset":"oops","limit":-4}"#.to_string()),
            output_text: Some("fn main() {}".to_string()),
            error_text: None,
            status: ToolCallStatus::Completed,
            duration_ms: Some(12),
        };

        let rendered = FileRenderer::new(init).render_data();
        assert_eq!(rendered.header.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn file_renderer_preserves_output_and_error_channels() {
        let init = RendererInit {
            tool_name: "Read".to_string(),
            input_json: Some(r#"{"path":"src/main.rs"}"#.to_string()),
            output_text: Some("fn main() {}".to_string()),
            error_text: Some("permission denied".to_string()),
            status: ToolCallStatus::Error,
            duration_ms: Some(12),
        };

        let rendered = FileRenderer::new(init).render_data();
        assert_eq!(rendered.output_text.as_deref(), Some("fn main() {}"));
        assert_eq!(rendered.error_text.as_deref(), Some("permission denied"));
    }
}
