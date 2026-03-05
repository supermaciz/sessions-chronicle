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
    pub raw_text: Option<String>,
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
        let raw_text = self
            .init
            .output_text
            .clone()
            .or_else(|| self.init.error_text.clone());
        let entries = raw_text
            .as_deref()
            .map(parse_results_lines)
            .unwrap_or_default();

        ResultsRenderedData {
            entries,
            raw_text,
            status: self.init.status,
            duration_ms: self.init.duration_ms,
        }
    }
}

#[allow(dead_code)]
pub fn parse_results_lines(output: &str) -> Vec<ResultsEntry> {
    output
        .lines()
        .filter_map(parse_results_line)
        .collect::<Vec<_>>()
}

fn parse_results_line(line: &str) -> Option<ResultsEntry> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let colon_positions = trimmed
        .char_indices()
        .filter_map(|(index, ch)| (ch == ':').then_some(index))
        .collect::<Vec<_>>();

    if colon_positions.len() < 2 {
        return Some(ResultsEntry {
            path: trimmed.to_string(),
            line: None,
            content: String::new(),
        });
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

        let content = trimmed[right_colon + 1..].trim().to_string();
        return Some(ResultsEntry {
            path: path.to_string(),
            line: Some(line_number),
            content,
        });
    }

    // Fallback: keep the line, but preserve that no line number was parsed.
    Some(ResultsEntry {
        path: trimmed.to_string(),
        line: None,
        content: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::{ResultsEntry, parse_results_lines};

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
}
