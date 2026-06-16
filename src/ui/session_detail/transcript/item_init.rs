//! Transcript item data prepared for typed transcript row rendering.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{TimeZone, Utc};

use crate::models::{MessagePreview, ReasoningPreview, Role, ToolCallStatus};

#[derive(Clone)]
pub struct MessageItemInit {
    pub item_index: usize,
    pub transcript_item_index: i64,
    pub preview: MessagePreview,
    pub highlight_query: Option<String>,
    pub db_path: Arc<PathBuf>,
}

#[derive(Debug, Clone)]
/// UI-facing data needed to render a single tool call transcript row.
///
/// `preview` is the preferred secondary line because it is derived from the
/// tool-specific input/output payload when possible. `summary` is a normalized
/// fallback string carried through the database layer; current parsers do not
/// populate it, but the row still supports it for future parser coverage and
/// historical data compatibility.
pub struct ToolCallItemInit {
    /// Stable transcript item position used for match/selection bookkeeping.
    pub item_index: usize,
    /// Stable transcript row index from database `transcript_items.item_index`.
    pub transcript_item_index: i64,
    /// Owning session id used for reasoning inspection routing.
    pub session_id: String,
    /// Session-scoped tool call identifier used by the inspector action.
    pub tool_call_id: String,
    /// Normalized tool call name shown in the primary monospace label.
    pub tool_name: String,
    /// Normalized execution status rendered as a badge.
    pub status: ToolCallStatus,
    /// Preferred short preview extracted from tool input/output content.
    pub preview: Option<String>,
    /// Optional one-line summary string used as a fallback preview/search text.
    pub summary: Option<String>,
    /// Optional execution duration displayed in the row suffix.
    pub duration_ms: Option<i64>,
    /// Active transcript search query used to compute per-row match counts.
    pub highlight_query: Option<String>,
    /// Presence flags for associated reasoning attachment.
    pub reasoning_preview: ReasoningPreview,
}

impl ToolCallItemInit {
    /// Returns the text actually shown as the secondary preview line:
    /// `preview` if present, otherwise `summary` as fallback.
    pub fn displayed_preview(&self) -> Option<&str> {
        self.preview.as_deref().or(self.summary.as_deref())
    }
}

#[derive(Debug, Clone)]
pub struct ToolBurstItemInit {
    pub item_index: usize,
    pub tool_calls: Vec<ToolCallItemInit>,
    pub category_counts: Vec<(String, usize)>,
    pub error_count: usize,
    pub total_duration_ms: Option<i64>,
    pub match_count: usize,
    pub child_match_counts: Vec<usize>,
    pub visible_reasoning_child_count: usize,
    pub encrypted_only_child_count: usize,
    pub default_expanded: bool,
}

#[derive(Clone)]
pub struct SubagentItemInit {
    pub item_index: usize,
    pub transcript_item_index: i64,
    pub session_id: String,
    pub subagent_id: String,
    pub title: String,
    pub reasoning_preview: ReasoningPreview,
}

#[derive(Clone)]
pub enum TranscriptItemInit {
    Message(MessageItemInit),
    ToolCall(ToolCallItemInit),
    ToolBurst(ToolBurstItemInit),
    Subagent(SubagentItemInit),
}

impl TranscriptItemInit {
    pub fn item_index(&self) -> usize {
        match self {
            Self::Message(init) => init.item_index,
            Self::ToolCall(init) => init.item_index,
            Self::ToolBurst(init) => init.item_index,
            Self::Subagent(init) => init.item_index,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TranscriptRowBuildKind {
    Message,
    ToolCall,
    ToolBurst,
    Subagent,
}

pub(crate) fn count_tool_call_matches(init: &ToolCallItemInit) -> usize {
    let Some(query) = init.highlight_query.as_deref() else {
        return 0;
    };

    let mut count =
        crate::utils::text_match::count_case_insensitive_matches(&init.tool_name, query);
    if let Some(text) = init.displayed_preview() {
        count += crate::utils::text_match::count_case_insensitive_matches(text, query);
    }
    count
}

pub fn build_tool_burst_init(
    item_index: usize,
    tool_calls: Vec<ToolCallItemInit>,
    default_expanded: bool,
) -> ToolBurstItemInit {
    let mut category_counts = BTreeMap::new();
    let mut error_count = 0usize;
    let mut total_duration_ms = 0i64;
    let mut saw_duration = false;
    let mut child_match_counts = Vec::new();
    let mut visible_reasoning_child_count = 0usize;
    let mut encrypted_only_child_count = 0usize;

    for tool_call in &tool_calls {
        *category_counts
            .entry(tool_call.tool_name.clone())
            .or_insert(0usize) += 1;
        if matches!(tool_call.status, ToolCallStatus::Error) {
            error_count += 1;
        }
        if let Some(ms) = tool_call.duration_ms {
            total_duration_ms += ms;
            saw_duration = true;
        }
        if tool_call.reasoning_preview.has_visible_reasoning {
            visible_reasoning_child_count += 1;
        } else if tool_call.reasoning_preview.encrypted_only {
            encrypted_only_child_count += 1;
        }
        child_match_counts.push(count_tool_call_matches(tool_call));
    }

    ToolBurstItemInit {
        item_index,
        tool_calls,
        category_counts: category_counts.into_iter().collect(),
        error_count,
        total_duration_ms: saw_duration.then_some(total_duration_ms),
        match_count: child_match_counts.iter().sum(),
        child_match_counts,
        visible_reasoning_child_count,
        encrypted_only_child_count,
        default_expanded,
    }
}

#[cfg(test)]
fn transcript_item_init_from_row(
    row: &crate::database::TranscriptItemRow,
    session_id: &str,
    highlight_query: Option<String>,
    db_path: Arc<PathBuf>,
) -> TranscriptItemInit {
    transcript_item_init_from_row_with_index(
        row,
        row.item_index as usize,
        session_id,
        highlight_query,
        db_path,
    )
}

fn transcript_item_init_from_row_with_index(
    row: &crate::database::TranscriptItemRow,
    item_index: usize,
    session_id: &str,
    highlight_query: Option<String>,
    db_path: Arc<PathBuf>,
) -> TranscriptItemInit {
    use crate::models::{ToolCallStatus, TranscriptItemKind};

    match row.kind {
        TranscriptItemKind::Message => {
            let role = row.role.unwrap_or(Role::User);
            let timestamp_unix = row.timestamp.unwrap_or(0);
            let timestamp = Utc
                .timestamp_opt(timestamp_unix, 0)
                .single()
                .unwrap_or_else(Utc::now);
            let message_index = row.message_index.unwrap_or(0) as usize;

            TranscriptItemInit::Message(MessageItemInit {
                item_index,
                transcript_item_index: row.item_index,
                preview: MessagePreview {
                    session_id: session_id.to_string(),
                    message_index,
                    role,
                    content_preview: row.content_preview.clone().unwrap_or_default(),
                    content_len: row.content_len.unwrap_or(0) as usize,
                    timestamp,
                    model: row.model.clone(),
                    reasoning_preview: row.reasoning_preview,
                },
                highlight_query,
                db_path,
            })
        }
        TranscriptItemKind::ToolCall => TranscriptItemInit::ToolCall(ToolCallItemInit {
            item_index,
            transcript_item_index: row.item_index,
            session_id: session_id.to_string(),
            tool_call_id: row.tool_call_id.clone().unwrap_or_default(),
            tool_name: row
                .tool_name
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            status: row.tool_status.unwrap_or(ToolCallStatus::Unknown),
            preview: crate::ui::session_detail::transcript::tool_preview::extract_preview(
                row.tool_name.as_deref().unwrap_or("unknown"),
                row.tool_input_json.as_deref().unwrap_or(""),
                row.tool_output_text.as_deref(),
            )
            .or_else(|| row.tool_summary.clone()),
            summary: row.tool_summary.clone(),
            duration_ms: row.duration_ms,
            highlight_query,
            reasoning_preview: row.reasoning_preview,
        }),
        TranscriptItemKind::Subagent => TranscriptItemInit::Subagent(SubagentItemInit {
            item_index,
            transcript_item_index: row.item_index,
            session_id: session_id.to_string(),
            subagent_id: row.subagent_id.clone().unwrap_or_default(),
            title: row
                .subagent_title
                .clone()
                .unwrap_or_else(|| "Subagent".to_string()),
            reasoning_preview: row.reasoning_preview,
        }),
        TranscriptItemKind::Unknown => {
            tracing::warn!(
                item_index = row.item_index,
                "transcript item with unknown kind; rendering as empty message"
            );
            TranscriptItemInit::Message(MessageItemInit {
                item_index,
                transcript_item_index: row.item_index,
                preview: MessagePreview {
                    session_id: session_id.to_string(),
                    message_index: 0,
                    role: Role::User,
                    content_preview: String::new(),
                    content_len: 0,
                    timestamp: Utc::now(),
                    model: None,
                    reasoning_preview: row.reasoning_preview,
                },
                highlight_query,
                db_path,
            })
        }
    }
}

pub fn transcript_item_init_from_display_item(
    display_index: usize,
    item: &crate::ui::session_detail::transcript::display::DisplayTranscriptItem,
    session_id: &str,
    highlight_query: Option<String>,
    db_path: Arc<PathBuf>,
) -> TranscriptItemInit {
    match item {
        crate::ui::session_detail::transcript::display::DisplayTranscriptItem::Single(row) => {
            transcript_item_init_from_row_with_index(
                row,
                display_index,
                session_id,
                highlight_query,
                db_path,
            )
        }
        crate::ui::session_detail::transcript::display::DisplayTranscriptItem::ToolBurst(burst) => {
            let tool_calls = burst
                .rows
                .iter()
                .filter_map(|row| {
                    match transcript_item_init_from_row_with_index(
                        row,
                        row.item_index as usize,
                        session_id,
                        highlight_query.clone(),
                        db_path.clone(),
                    ) {
                        TranscriptItemInit::ToolCall(tool_call) => Some(tool_call),
                        other => {
                            debug_assert!(
                                false,
                                "tool burst child must be a tool call, got {:?}",
                                std::mem::discriminant(&other)
                            );
                            None
                        }
                    }
                })
                .collect();
            TranscriptItemInit::ToolBurst(build_tool_burst_init(display_index, tool_calls, false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_row(
        item_index: i64,
        tool_name: &str,
        input_json: &str,
        output_text: Option<&str>,
        summary: Option<&str>,
    ) -> crate::database::TranscriptItemRow {
        crate::database::TranscriptItemRow {
            item_index,
            kind: crate::models::TranscriptItemKind::ToolCall,
            reasoning_preview: ReasoningPreview::default(),
            message_index: None,
            role: None,
            content_preview: None,
            content_len: None,
            timestamp: None,
            model: None,
            tool_call_id: Some(format!("call-{item_index}")),
            tool_name: Some(tool_name.to_string()),
            tool_status: Some(ToolCallStatus::Completed),
            tool_summary: summary.map(str::to_string),
            tool_input_json: Some(input_json.to_string()),
            tool_output_text: output_text.map(str::to_string),
            duration_ms: Some(25),
            subagent_id: None,
            subagent_title: None,
            subagent_prompt: None,
        }
    }

    fn preview_from_row(row: &crate::database::TranscriptItemRow) -> Option<String> {
        let init = transcript_item_init_from_row(
            row,
            "session-1",
            None,
            Arc::new(PathBuf::from("/tmp/sessions-chronicle-test.db")),
        );

        let TranscriptItemInit::ToolCall(tool_init) = init else {
            panic!("expected tool call init");
        };

        tool_init.preview
    }

    #[test]
    fn tool_call_match_count_uses_only_displayed_preview() {
        let with_preview = ToolCallItemInit {
            item_index: 7,
            transcript_item_index: 7,
            session_id: "session-1".to_string(),
            tool_call_id: "call-7".to_string(),
            tool_name: "Read".to_string(),
            status: ToolCallStatus::Completed,
            preview: Some("src/ui/session_detail.rs:1-20".to_string()),
            summary: Some("read the transcript loader".to_string()),
            duration_ms: Some(12),
            highlight_query: Some("read".to_string()),
            reasoning_preview: ReasoningPreview::default(),
        };
        assert_eq!(count_tool_call_matches(&with_preview), 1);

        let with_summary_fallback = ToolCallItemInit {
            item_index: 8,
            transcript_item_index: 8,
            session_id: "session-1".to_string(),
            tool_call_id: "call-8".to_string(),
            tool_name: "Read".to_string(),
            status: ToolCallStatus::Completed,
            preview: None,
            summary: Some("read the transcript loader".to_string()),
            duration_ms: Some(12),
            highlight_query: Some("read".to_string()),
            reasoning_preview: ReasoningPreview::default(),
        };
        assert_eq!(count_tool_call_matches(&with_summary_fallback), 2);
    }

    #[test]
    fn tool_burst_item_init_aggregates_categories_duration_errors_and_matches() {
        let tool_calls = vec![
            ToolCallItemInit {
                item_index: 1,
                transcript_item_index: 1,
                session_id: "session-1".to_string(),
                tool_call_id: "call-1".to_string(),
                tool_name: "Read".to_string(),
                status: ToolCallStatus::Completed,
                preview: Some("read src/ui/transcript_item_init.rs".to_string()),
                summary: None,
                duration_ms: Some(5),
                highlight_query: Some("read".to_string()),
                reasoning_preview: ReasoningPreview::default(),
            },
            ToolCallItemInit {
                item_index: 2,
                transcript_item_index: 2,
                session_id: "session-1".to_string(),
                tool_call_id: "call-2".to_string(),
                tool_name: "Edit".to_string(),
                status: ToolCallStatus::Error,
                preview: Some("edit src/ui/session_detail.rs".to_string()),
                summary: None,
                duration_ms: Some(8),
                highlight_query: Some("edit".to_string()),
                reasoning_preview: ReasoningPreview::default(),
            },
        ];

        let burst = build_tool_burst_init(10, tool_calls, false);
        assert_eq!(burst.error_count, 1);
        assert_eq!(burst.total_duration_ms, Some(13));
        assert_eq!(burst.match_count, 4);
        assert_eq!(burst.child_match_counts, vec![2, 2]);
        assert_eq!(
            burst.category_counts,
            vec![("Edit".to_string(), 1), ("Read".to_string(), 1)]
        );
    }

    #[test]
    fn transcript_item_init_from_display_item_keeps_raw_child_transcript_indices() {
        let burst =
            crate::ui::session_detail::transcript::display::DisplayTranscriptItem::ToolBurst(
                crate::ui::session_detail::transcript::display::DisplayToolBurst {
                    rows: vec![
                        tool_row(11, "Read", "{}", None, None),
                        tool_row(12, "Edit", "{}", None, None),
                    ],
                },
            );

        let init = transcript_item_init_from_display_item(
            5,
            &burst,
            "session-1",
            None,
            Arc::new(PathBuf::from("/tmp/test.db")),
        );

        let TranscriptItemInit::ToolBurst(burst_init) = init else {
            panic!("expected tool burst init");
        };

        assert_eq!(burst_init.tool_calls[0].transcript_item_index, 11);
        assert_eq!(burst_init.tool_calls[1].transcript_item_index, 12);
    }

    #[test]
    fn transcript_item_init_prefers_extracted_preview_over_summary() {
        let row = crate::database::TranscriptItemRow {
            item_index: 1,
            kind: crate::models::TranscriptItemKind::ToolCall,
            reasoning_preview: ReasoningPreview::default(),
            message_index: None,
            role: None,
            content_preview: None,
            content_len: None,
            timestamp: None,
            model: None,
            tool_call_id: Some("call-1".to_string()),
            tool_name: Some("bash".to_string()),
            tool_status: Some(ToolCallStatus::Completed),
            tool_summary: Some("summary fallback".to_string()),
            tool_input_json: Some(r#"{"command":"ls -la"}"#.to_string()),
            tool_output_text: None,
            duration_ms: Some(12),
            subagent_id: None,
            subagent_title: None,
            subagent_prompt: None,
        };

        let init = transcript_item_init_from_row(
            &row,
            "session-1",
            None,
            Arc::new(PathBuf::from("/tmp/test.db")),
        );

        let TranscriptItemInit::ToolCall(tool_init) = init else {
            panic!("expected tool call init");
        };

        assert_eq!(tool_init.preview.as_deref(), Some("$ ls -la"));
    }

    #[test]
    fn transcript_tool_items_emit_representative_preview_shapes() {
        let bash_row = tool_row(
            1,
            "bash",
            r#"{"command":"cargo test --all && cargo clippy --all"}"#,
            Some("Process exited with code 0"),
            Some("bash summary"),
        );
        let read_row = tool_row(
            2,
            "read",
            r#"{"file_path":"src/ui/transcript_item_init.rs","offset":42,"limit":20}"#,
            None,
            Some("read summary"),
        );
        let edit_row = tool_row(
            3,
            "edit",
            r#"{"file_path":"src/ui/tool_preview.rs","old_string":"a\nb","new_string":"a\nb\nc"}"#,
            None,
            Some("edit summary"),
        );
        let grep_row = tool_row(
            4,
            "grep",
            r#"{"pattern":"transcript_item_init_from_row"}"#,
            Some("Found 4 matches"),
            Some("grep summary"),
        );

        let bash_preview = preview_from_row(&bash_row).expect("bash preview should exist");
        assert!(bash_preview.starts_with("$ cargo test --all"));

        let read_preview = preview_from_row(&read_row).expect("read preview should exist");
        assert!(read_preview.contains("transcript_item_init.rs:42-61"));

        let edit_preview = preview_from_row(&edit_row).expect("edit preview should exist");
        assert!(edit_preview.contains("tool_preview.rs +1 -0"));

        let grep_preview = preview_from_row(&grep_row).expect("grep preview should exist");
        assert!(grep_preview.contains("pattern=\"transcript_item_init_from_row\""));
        assert!(grep_preview.contains("4 matches"));
    }

    #[test]
    fn transcript_tool_item_preview_falls_back_to_summary_when_extractor_returns_none() {
        let row = tool_row(
            5,
            "bash",
            "{not-json}",
            None,
            Some("fallback summary from db"),
        );

        assert_eq!(
            preview_from_row(&row).as_deref(),
            Some("fallback summary from db")
        );
    }
}
