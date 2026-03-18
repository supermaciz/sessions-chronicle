use crate::ui::tool_renderers::RendererInit;
use similar::{Algorithm, ChangeTag, DiffOp, TextDiff};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Add,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRenderedData {
    pub old_text: Option<String>,
    pub new_text: Option<String>,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRenderer {
    init: RendererInit,
}

impl DiffRenderer {
    pub fn new(init: RendererInit) -> Self {
        Self { init }
    }

    pub fn render_data(&self) -> DiffRenderedData {
        let (old_text, new_text) = parse_old_new(self.init.input_json.as_deref());
        let hunks = match (&old_text, &new_text) {
            (Some(old), Some(new)) => build_grouped_hunks(old, new),
            _ => Vec::new(),
        };

        DiffRenderedData {
            old_text,
            new_text,
            hunks,
        }
    }
}

fn build_grouped_hunks(old_text: &str, new_text: &str) -> Vec<DiffHunk> {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .timeout(Duration::from_millis(500))
        .diff_lines(old_text, new_text);

    diff.grouped_ops(3)
        .into_iter()
        .map(|ops| {
            let header = format_hunk_header(&ops);
            let mut lines = Vec::new();

            for op in ops {
                for change in diff.iter_changes(&op) {
                    lines.push(DiffLine {
                        kind: change_tag_to_kind(change.tag()),
                        old_index: change.old_index().map(|index| index + 1),
                        new_index: change.new_index().map(|index| index + 1),
                        text: change_text(change),
                    });
                }
            }

            DiffHunk { header, lines }
        })
        .collect()
}

fn change_tag_to_kind(tag: ChangeTag) -> DiffLineKind {
    match tag {
        ChangeTag::Equal => DiffLineKind::Context,
        ChangeTag::Delete => DiffLineKind::Remove,
        ChangeTag::Insert => DiffLineKind::Add,
    }
}

fn change_text(change: similar::Change<&str>) -> String {
    let mut text = change.to_string();
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }

    text
}

fn format_hunk_header(ops: &[DiffOp]) -> String {
    let old_start = ops.iter().map(|op| op.old_range().start).min().unwrap_or(0);
    let old_end = ops
        .iter()
        .map(|op| op.old_range().end)
        .max()
        .unwrap_or(old_start);
    let new_start = ops.iter().map(|op| op.new_range().start).min().unwrap_or(0);
    let new_end = ops
        .iter()
        .map(|op| op.new_range().end)
        .max()
        .unwrap_or(new_start);
    let old_count = old_end.saturating_sub(old_start);
    let new_count = new_end.saturating_sub(new_start);
    let old_header_start = if old_count == 0 {
        old_start
    } else {
        old_start + 1
    };
    let new_header_start = if new_count == 0 {
        new_start
    } else {
        new_start + 1
    };

    format!(
        "@@ -{},{} +{},{} @@",
        old_header_start, old_count, new_header_start, new_count
    )
}

fn parse_old_new(input_json: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(raw) = input_json else {
        return (None, None);
    };

    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return (None, None);
    };

    (
        extract_text(&value, OLD_KEYS),
        extract_text(&value, NEW_KEYS),
    )
}

const OLD_KEYS: &[&str] = &[
    "old_text",
    "old_string",
    "oldString",
    "old",
    "before",
    "original",
    "content",
];
const NEW_KEYS: &[&str] = &[
    "new_text",
    "new_string",
    "newString",
    "new",
    "after",
    "updated",
    "replacement",
];

fn extract_text(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(serde_json::Value::as_str))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::{DiffLineKind, DiffRenderer, build_grouped_hunks};
    use crate::models::ToolCallStatus;
    use crate::ui::tool_renderers::RendererInit;

    fn init_with_input(input_json: Option<&str>) -> RendererInit {
        RendererInit {
            tool_name: "Edit".to_string(),
            input_json: input_json.map(str::to_string),
            output_text: None,
            error_text: None,
            status: ToolCallStatus::Completed,
            duration_ms: None,
        }
    }

    #[test]
    fn diff_renderer_builds_hunks_with_context() {
        let input = r#"{
            "old_text": "alpha\nbravo\ncharlie\ndelta\n",
            "new_text": "alpha\nbravo\ncharlie-updated\ndelta\n"
        }"#;

        let rendered = DiffRenderer::new(init_with_input(Some(input))).render_data();

        assert!(!rendered.hunks.is_empty());
        let lines = &rendered.hunks[0].lines;
        assert!(lines.iter().any(|line| line.kind == DiffLineKind::Context));
        assert!(lines.iter().any(|line| line.kind == DiffLineKind::Remove));
        assert!(lines.iter().any(|line| line.kind == DiffLineKind::Add));
    }

    #[test]
    fn diff_renderer_handles_missing_old_or_new_text() {
        let old_only =
            DiffRenderer::new(init_with_input(Some(r#"{"old_text":"old only"}"#))).render_data();
        assert_eq!(old_only.old_text.as_deref(), Some("old only"));
        assert_eq!(old_only.new_text, None);
        assert!(old_only.hunks.is_empty());

        let malformed = DiffRenderer::new(init_with_input(Some("{not-json"))).render_data();
        assert_eq!(malformed.old_text, None);
        assert_eq!(malformed.new_text, None);
        assert!(malformed.hunks.is_empty());
    }

    #[test]
    fn diff_renderer_parses_old_string_new_string_aliases() {
        let rendered = DiffRenderer::new(init_with_input(Some(
            r#"{"old_string":"before","new_string":"after"}"#,
        )))
        .render_data();

        assert_eq!(rendered.old_text.as_deref(), Some("before"));
        assert_eq!(rendered.new_text.as_deref(), Some("after"));
    }

    #[test]
    fn diff_renderer_formats_zero_length_hunk_headers_for_insertions_and_deletions() {
        let insertion_hunks = build_grouped_hunks("", "added\n");
        assert_eq!(insertion_hunks[0].header, "@@ -0,0 +1,1 @@");

        let deletion_hunks = build_grouped_hunks("removed\n", "");
        assert_eq!(deletion_hunks[0].header, "@@ -1,1 +0,0 @@");
    }

    #[test]
    fn diff_renderer_mixed_hunk_line_numbers_match_expected_indices() {
        let hunks = build_grouped_hunks(
            "alpha\nbravo\ncharlie\n",
            "alpha\nbravo-2\ncharlie\ndelta\n",
        );
        let lines = &hunks[0].lines;

        assert_eq!(lines[0].kind, DiffLineKind::Context);
        assert_eq!(lines[0].old_index, Some(1));
        assert_eq!(lines[0].new_index, Some(1));

        assert_eq!(lines[1].kind, DiffLineKind::Remove);
        assert_eq!(lines[1].old_index, Some(2));
        assert_eq!(lines[1].new_index, None);

        assert_eq!(lines[2].kind, DiffLineKind::Add);
        assert_eq!(lines[2].old_index, None);
        assert_eq!(lines[2].new_index, Some(2));

        assert_eq!(lines[3].kind, DiffLineKind::Context);
        assert_eq!(lines[3].old_index, Some(3));
        assert_eq!(lines[3].new_index, Some(3));

        assert_eq!(lines[4].kind, DiffLineKind::Add);
        assert_eq!(lines[4].old_index, None);
        assert_eq!(lines[4].new_index, Some(4));
    }
}
