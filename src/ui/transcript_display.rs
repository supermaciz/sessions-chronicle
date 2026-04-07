use crate::database::TranscriptItemRow;
use crate::models::TranscriptItemKind;

#[derive(Debug, Clone)]
pub enum DisplayTranscriptItem {
    Single(Box<TranscriptItemRow>),
    ToolBurst(DisplayToolBurst),
}

#[derive(Debug, Clone)]
pub struct DisplayToolBurst {
    pub rows: Vec<TranscriptItemRow>,
}

pub fn group_transcript_rows(rows: Vec<TranscriptItemRow>) -> Vec<DisplayTranscriptItem> {
    let mut grouped = Vec::new();
    let mut pending_tool_rows = Vec::new();

    for row in rows {
        if row.kind == TranscriptItemKind::ToolCall {
            pending_tool_rows.push(row);
            continue;
        }

        flush_pending_tool_rows(&mut grouped, &mut pending_tool_rows);
        grouped.push(DisplayTranscriptItem::Single(Box::new(row)));
    }

    flush_pending_tool_rows(&mut grouped, &mut pending_tool_rows);
    grouped
}

pub fn trailing_tool_call_rows(rows: &[TranscriptItemRow]) -> Vec<TranscriptItemRow> {
    let mut trailing = Vec::new();
    for row in rows.iter().rev() {
        if row.kind != TranscriptItemKind::ToolCall {
            break;
        }
        trailing.push(row.clone());
    }
    trailing.reverse();
    trailing
}

pub struct BoundaryRegroupResult {
    pub replacement_items: Vec<DisplayTranscriptItem>,
    pub remaining_rows: Vec<TranscriptItemRow>,
}

pub fn regroup_boundary(
    previous_trailing_tools: Vec<TranscriptItemRow>,
    mut next_page_rows: Vec<TranscriptItemRow>,
) -> BoundaryRegroupResult {
    let leading_tool_count = next_page_rows
        .iter()
        .take_while(|row| row.kind == TranscriptItemKind::ToolCall)
        .count();

    if leading_tool_count == 0 {
        return BoundaryRegroupResult {
            replacement_items: Vec::new(),
            remaining_rows: next_page_rows,
        };
    }

    let mut merged_rows = previous_trailing_tools;
    let leading_rows: Vec<_> = next_page_rows.drain(..leading_tool_count).collect();
    merged_rows.extend(leading_rows);

    let replacement_items = group_transcript_rows(merged_rows);
    BoundaryRegroupResult {
        replacement_items,
        remaining_rows: next_page_rows,
    }
}

/// Extract the raw tool call rows from the tail of a display item list.
/// Used to determine boundary candidates when `merged_rows` is no longer available.
pub fn trailing_tool_rows_from_display(items: &[DisplayTranscriptItem]) -> Vec<TranscriptItemRow> {
    match items.last() {
        Some(DisplayTranscriptItem::ToolBurst(burst)) => burst.rows.clone(),
        Some(DisplayTranscriptItem::Single(row)) if row.kind == TranscriptItemKind::ToolCall => {
            vec![row.as_ref().clone()]
        }
        _ => Vec::new(),
    }
}

fn flush_pending_tool_rows(
    grouped: &mut Vec<DisplayTranscriptItem>,
    pending_tool_rows: &mut Vec<TranscriptItemRow>,
) {
    match pending_tool_rows.len() {
        0 => {}
        1 => grouped.push(DisplayTranscriptItem::Single(Box::new(
            pending_tool_rows.remove(0),
        ))),
        _ => grouped.push(DisplayTranscriptItem::ToolBurst(DisplayToolBurst {
            rows: std::mem::take(pending_tool_rows),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Role, ToolCallStatus};

    fn message_row(item_index: i64) -> TranscriptItemRow {
        TranscriptItemRow {
            item_index,
            kind: TranscriptItemKind::Message,
            reasoning_preview: crate::models::ReasoningPreview::default(),
            message_index: Some(item_index),
            role: Some(Role::Assistant),
            content_preview: Some(format!("message-{item_index}")),
            content_len: Some(10),
            timestamp: Some(0),
            model: None,
            tool_call_id: None,
            tool_name: None,
            tool_status: None,
            tool_summary: None,
            tool_input_json: None,
            tool_output_text: None,
            duration_ms: None,
            subagent_id: None,
            subagent_title: None,
            subagent_prompt: None,
        }
    }

    fn tool_row(item_index: i64, tool_name: &str) -> TranscriptItemRow {
        TranscriptItemRow {
            item_index,
            kind: TranscriptItemKind::ToolCall,
            reasoning_preview: crate::models::ReasoningPreview::default(),
            message_index: None,
            role: None,
            content_preview: None,
            content_len: None,
            timestamp: None,
            model: None,
            tool_call_id: Some(format!("call-{item_index}")),
            tool_name: Some(tool_name.to_string()),
            tool_status: Some(ToolCallStatus::Completed),
            tool_summary: Some(format!("summary-{item_index}")),
            tool_input_json: Some("{}".to_string()),
            tool_output_text: None,
            duration_ms: Some(25),
            subagent_id: None,
            subagent_title: None,
            subagent_prompt: None,
        }
    }

    fn subagent_row(item_index: i64) -> TranscriptItemRow {
        TranscriptItemRow {
            item_index,
            kind: TranscriptItemKind::Subagent,
            reasoning_preview: crate::models::ReasoningPreview::default(),
            message_index: None,
            role: None,
            content_preview: None,
            content_len: None,
            timestamp: None,
            model: None,
            tool_call_id: None,
            tool_name: None,
            tool_status: None,
            tool_summary: None,
            tool_input_json: None,
            tool_output_text: None,
            duration_ms: None,
            subagent_id: Some(format!("sub-{item_index}")),
            subagent_title: Some("Subagent".to_string()),
            subagent_prompt: None,
        }
    }

    #[test]
    fn tool_burst_preserves_child_reasoning_flags() {
        let rows = vec![
            TranscriptItemRow {
                item_index: 0,
                kind: TranscriptItemKind::ToolCall,
                reasoning_preview: crate::models::ReasoningPreview {
                    has_reasoning: true,
                    has_visible_reasoning: true,
                    encrypted_only: false,
                },
                message_index: None,
                role: None,
                content_preview: None,
                content_len: None,
                timestamp: None,
                model: None,
                tool_call_id: Some("call-0".to_string()),
                tool_name: Some("Read".to_string()),
                tool_status: Some(ToolCallStatus::Completed),
                tool_summary: None,
                tool_input_json: Some("{}".to_string()),
                tool_output_text: None,
                duration_ms: Some(10),
                subagent_id: None,
                subagent_title: None,
                subagent_prompt: None,
            },
            TranscriptItemRow {
                item_index: 1,
                kind: TranscriptItemKind::ToolCall,
                reasoning_preview: crate::models::ReasoningPreview {
                    has_reasoning: true,
                    has_visible_reasoning: false,
                    encrypted_only: true,
                },
                message_index: None,
                role: None,
                content_preview: None,
                content_len: None,
                timestamp: None,
                model: None,
                tool_call_id: Some("call-1".to_string()),
                tool_name: Some("Edit".to_string()),
                tool_status: Some(ToolCallStatus::Completed),
                tool_summary: None,
                tool_input_json: Some("{}".to_string()),
                tool_output_text: None,
                duration_ms: Some(20),
                subagent_id: None,
                subagent_title: None,
                subagent_prompt: None,
            },
        ];

        let grouped = group_transcript_rows(rows);
        let DisplayTranscriptItem::ToolBurst(burst) = &grouped[0] else {
            panic!("expected tool burst");
        };

        assert_eq!(burst.rows.len(), 2);
        assert!(burst.rows[0].reasoning_preview.has_visible_reasoning);
        assert!(burst.rows[1].reasoning_preview.encrypted_only);
    }

    #[test]
    fn isolated_tool_call_stays_single() {
        let rows = vec![message_row(0), tool_row(1, "Read"), message_row(2)];
        let grouped = group_transcript_rows(rows);
        assert_eq!(grouped.len(), 3);
        assert!(matches!(grouped[1], DisplayTranscriptItem::Single(_)));
    }

    #[test]
    fn two_consecutive_tool_calls_become_burst() {
        let rows = vec![message_row(0), tool_row(1, "Read"), tool_row(2, "Edit")];
        let grouped = group_transcript_rows(rows);
        assert_eq!(grouped.len(), 2);
        assert!(matches!(grouped[1], DisplayTranscriptItem::ToolBurst(_)));
    }

    #[test]
    fn subagent_breaks_tool_burst() {
        let rows = vec![tool_row(0, "Read"), subagent_row(1), tool_row(2, "Edit")];
        let grouped = group_transcript_rows(rows);
        assert_eq!(grouped.len(), 3);
        assert!(matches!(grouped[0], DisplayTranscriptItem::Single(_)));
        assert!(matches!(grouped[1], DisplayTranscriptItem::Single(_)));
        assert!(matches!(grouped[2], DisplayTranscriptItem::Single(_)));
    }

    #[test]
    fn mixed_tool_run_preserves_order_inside_burst() {
        let rows = vec![
            tool_row(0, "Read"),
            tool_row(1, "Read"),
            tool_row(2, "Grep"),
            tool_row(3, "Edit"),
            tool_row(4, "Bash"),
        ];
        let grouped = group_transcript_rows(rows);
        assert_eq!(grouped.len(), 1);

        let DisplayTranscriptItem::ToolBurst(burst) = &grouped[0] else {
            panic!("expected tool burst");
        };

        let names: Vec<&str> = burst
            .rows
            .iter()
            .map(|row| row.tool_name.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(names, vec!["Read", "Read", "Grep", "Edit", "Bash"]);
    }

    #[test]
    fn trailing_tool_calls_are_reported_as_boundary_candidates() {
        let rows = vec![message_row(0), tool_row(1, "Read"), tool_row(2, "Edit")];
        let trailing = trailing_tool_call_rows(&rows);
        assert_eq!(trailing.len(), 2);
    }

    #[test]
    fn regroup_boundary_merges_previous_trailing_tools_with_next_page_prefix() {
        let previous = vec![tool_row(1, "Read"), tool_row(2, "Edit")];
        let next_page = vec![tool_row(3, "Bash"), message_row(4)];

        let result = regroup_boundary(previous, next_page);
        assert_eq!(result.replacement_items.len(), 1);
        assert!(matches!(
            result.replacement_items[0],
            DisplayTranscriptItem::ToolBurst(_)
        ));
        assert_eq!(result.remaining_rows.len(), 1);
    }

    #[test]
    fn regroup_boundary_does_nothing_when_next_page_starts_with_message() {
        let previous = vec![tool_row(1, "Read"), tool_row(2, "Edit")];
        let next_page = vec![message_row(3)];

        let result = regroup_boundary(previous, next_page);
        assert!(result.replacement_items.is_empty());
        assert_eq!(result.remaining_rows.len(), 1);
    }
}
