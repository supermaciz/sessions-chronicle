use crate::models::ToolCallStatus;
use crate::ui::tool_renderers::RendererInit;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultsEntry {
    pub path: String,
    pub line: Option<i64>,
    pub content: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultsRenderedData {
    pub entries: Vec<ResultsEntry>,
    pub output_text: Option<String>,
    pub error_text: Option<String>,
    pub status: ToolCallStatus,
    pub duration_ms: Option<i64>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultsRenderer {
    init: RendererInit,
}

#[allow(dead_code)]
impl ResultsRenderer {
    pub fn new(init: RendererInit) -> Self {
        Self { init }
    }

    pub fn render_data(&self) -> ResultsRenderedData {
        let output_text = self
            .init
            .output_text
            .clone()
            .filter(|text| !text.is_empty());
        let error_text = self.init.error_text.clone().filter(|text| !text.is_empty());
        let parse_mode = ResultsParseMode::for_tool_name(&self.init.tool_name);
        let entries = output_text
            .as_deref()
            .map(|output| parse_results_lines_with_mode(output, parse_mode))
            .unwrap_or_default();

        ResultsRenderedData {
            entries,
            output_text,
            error_text,
            status: self.init.status,
            duration_ms: self.init.duration_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultsParseMode {
    StrictPathLineContent,
    PathList,
}

impl ResultsParseMode {
    fn for_tool_name(tool_name: &str) -> Self {
        match tool_name.trim().to_ascii_lowercase().as_str() {
            "grep" | "search" => Self::StrictPathLineContent,
            _ => Self::PathList,
        }
    }
}

#[allow(dead_code)]
pub fn parse_results_lines(output: &str) -> Vec<ResultsEntry> {
    parse_results_lines_with_mode(output, ResultsParseMode::StrictPathLineContent)
}

fn parse_results_lines_with_mode(output: &str, mode: ResultsParseMode) -> Vec<ResultsEntry> {
    output
        .lines()
        .filter_map(|line| parse_results_line(line, mode))
        .collect::<Vec<_>>()
}

fn parse_results_line(line: &str, mode: ResultsParseMode) -> Option<ResultsEntry> {
    match mode {
        ResultsParseMode::StrictPathLineContent => parse_strict_path_line_content(line),
        ResultsParseMode::PathList => parse_path_list_entry(line),
    }
}

fn parse_path_list_entry(line: &str) -> Option<ResultsEntry> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(ResultsEntry {
        path: trimmed.to_string(),
        line: None,
        content: String::new(),
    })
}

fn parse_strict_path_line_content(line: &str) -> Option<ResultsEntry> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let colon_positions = trimmed
        .char_indices()
        .filter_map(|(index, ch)| (ch == ':').then_some(index))
        .collect::<Vec<_>>();

    if colon_positions.len() < 2 {
        return None;
    }

    for split_idx in 0..(colon_positions.len() - 1) {
        let left_colon = colon_positions[split_idx];
        let right_colon = colon_positions[split_idx + 1];

        let path = trimmed[..left_colon].trim();
        if path.is_empty() {
            continue;
        }

        let line_part = trimmed[left_colon + 1..right_colon].trim();
        let Ok(line_number) = line_part.parse::<i64>() else {
            continue;
        };
        if line_number <= 0 {
            continue;
        }

        let content = trimmed[right_colon + 1..].trim().to_string();
        return Some(ResultsEntry {
            path: path.to_string(),
            line: Some(line_number),
            content,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{ResultsEntry, ResultsRenderer, parse_results_lines};
    use crate::models::ToolCallStatus;
    use crate::ui::tool_renderers::RendererInit;

    #[test]
    fn results_renderer_parses_file_line_entries() {
        let output = "src/main.rs:10:fn main() {}\nsrc/lib.rs:7:pub fn run() {}";
        let entries = parse_results_lines(output);

        assert_eq!(
            entries,
            vec![
                ResultsEntry {
                    path: "src/main.rs".to_string(),
                    line: Some(10),
                    content: "fn main() {}".to_string(),
                },
                ResultsEntry {
                    path: "src/lib.rs".to_string(),
                    line: Some(7),
                    content: "pub fn run() {}".to_string(),
                },
            ]
        );
    }

    #[test]
    fn results_renderer_ignores_non_result_text_for_grep_like_tools() {
        let init = RendererInit {
            tool_name: "grep".to_string(),
            input_json: None,
            output_text: Some("No matches found. Try another query.".to_string()),
            error_text: None,
            status: ToolCallStatus::Completed,
            duration_ms: None,
        };

        let rendered = ResultsRenderer::new(init).render_data();
        assert!(rendered.entries.is_empty());
        assert_eq!(
            rendered.output_text.as_deref(),
            Some("No matches found. Try another query.")
        );
    }

    #[test]
    fn results_renderer_requires_positive_line_numbers() {
        let init = RendererInit {
            tool_name: "search".to_string(),
            input_json: None,
            output_text: Some(
                "src/main.rs:0:zero\nsrc/lib.rs:-7:negative\nsrc/bad.rs:not-a-number:oops\nsrc/app.rs:8:valid"
                    .to_string(),
            ),
            error_text: None,
            status: ToolCallStatus::Completed,
            duration_ms: None,
        };

        let rendered = ResultsRenderer::new(init).render_data();
        assert_eq!(
            rendered.entries,
            vec![ResultsEntry {
                path: "src/app.rs".to_string(),
                line: Some(8),
                content: "valid".to_string(),
            }]
        );
    }

    #[test]
    fn results_renderer_preserves_output_and_error_channels() {
        let init = RendererInit {
            tool_name: "search".to_string(),
            input_json: None,
            output_text: Some("src/app.rs:8:valid".to_string()),
            error_text: Some("warning: partial results".to_string()),
            status: ToolCallStatus::Error,
            duration_ms: None,
        };

        let rendered = ResultsRenderer::new(init).render_data();
        assert_eq!(
            rendered.entries,
            vec![ResultsEntry {
                path: "src/app.rs".to_string(),
                line: Some(8),
                content: "valid".to_string(),
            }]
        );
        assert_eq!(rendered.output_text.as_deref(), Some("src/app.rs:8:valid"));
        assert_eq!(
            rendered.error_text.as_deref(),
            Some("warning: partial results")
        );
    }
}
