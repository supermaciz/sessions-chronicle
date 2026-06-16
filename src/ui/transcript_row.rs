//! Shared transcript row data and rendering helpers.
//!
//! The active `SessionDetail` transcript view is implemented by
//! `typed_transcript_row`; this module keeps the input structs, grouping
//! conversions, and helper renderers used by that typed ListView path.

use std::path::PathBuf;
use std::sync::Arc;
use std::{collections::BTreeMap, rc::Rc, time::Duration, time::Instant};

use chrono::{TimeZone, Utc};
use gtk::prelude::*;
use relm4::gtk;

use crate::models::{MessagePreview, ReasoningPreview, Role, ToolCallStatus};
use crate::ui::highlight;
use crate::ui::markdown;
use crate::ui::session_detail::SessionDetailMsg;
use crate::ui::tool_call_row::{ToolCallRowHeaderInit, build_tool_call_row_header};

/// Return the model display text for a transcript header.
/// Only assistant messages with a non-empty model value produce output.
pub(crate) fn model_label_text(role: Role, model: Option<&str>) -> Option<String> {
    if role != Role::Assistant {
        return None;
    }
    let text = model?.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

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

pub(crate) fn render_content(
    container: &gtk::Box,
    content: &str,
    role: Role,
    highlight_query: Option<&str>,
) -> usize {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let mut match_count = 0usize;

    if role == Role::Assistant {
        let (widget, count) = markdown::render_markdown_to_textview(content, highlight_query);
        match_count = count;
        container.append(&widget);
    } else if let Some(query) = highlight_query {
        let (markup, count) = highlight::highlight_text(content, query);
        match_count = count;
        let label = gtk::Label::new(None);
        label.set_markup(&markup);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_halign(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_selectable(true);
        container.append(&label);
    } else {
        let label = gtk::Label::new(Some(content));
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_halign(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_selectable(true);
        container.append(&label);
    }

    match_count
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

pub(crate) fn format_reasoning_burst_label(
    visible_reasoning_child_count: usize,
    encrypted_only_child_count: usize,
) -> Option<String> {
    if visible_reasoning_child_count > 0 {
        Some(format!("{} thinking", visible_reasoning_child_count))
    } else if encrypted_only_child_count > 0 {
        Some(format!("{} encrypted", encrypted_only_child_count))
    } else {
        None
    }
}

pub(crate) fn format_tool_burst_accessible_label(
    category_counts: &[(String, usize)],
    total_tool_calls: usize,
    error_count: usize,
) -> String {
    let mut details: Vec<String> = category_counts
        .iter()
        .map(|(name, count)| format!("{count} {name}"))
        .collect();
    if error_count > 0 {
        details.push(format!(
            "{error_count} {}",
            if error_count == 1 { "error" } else { "errors" }
        ));
    }

    let mut label = format!("{total_tool_calls} tool calls");
    if !details.is_empty() {
        label.push_str(": ");
        label.push_str(&details.join(", "));
    }
    label
}

pub(crate) fn format_tool_burst_match_badge_accessible_label(match_count: usize) -> String {
    format!("{match_count} search matches inside this group")
}

struct ToolCallWidgetRefs {
    root: gtk::Box,
}

fn build_tool_call_widget(
    init: &ToolCallItemInit,
    on_inspect: impl Fn(String) + 'static,
    on_inspect_reasoning: impl Fn(String, i64) + 'static,
) -> ToolCallWidgetRefs {
    let on_inspect = Rc::new(on_inspect);
    let on_inspect_reasoning = Rc::new(on_inspect_reasoning);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("tool-call-row");
    root.set_margin_top(2);
    root.set_margin_bottom(2);

    let header = build_tool_call_row_header(ToolCallRowHeaderInit {
        tool_name: &init.tool_name,
        status: init.status,
        duration_ms: init.duration_ms,
        highlight_query: init.highlight_query.as_deref(),
        reasoning_preview: init.reasoning_preview,
    });
    let row = header.row;

    if let Some(reasoning_btn) = header.reasoning_button {
        {
            let session_id = init.session_id.clone();
            let transcript_item_index = init.transcript_item_index;
            let on_inspect_reasoning = on_inspect_reasoning.clone();
            reasoning_btn.connect_clicked(move |_| {
                on_inspect_reasoning(session_id.clone(), transcript_item_index);
            });
        }
    }

    let inspect_btn = gtk::Button::new();
    inspect_btn.set_icon_name("view-reveal-symbolic");
    inspect_btn.set_tooltip_text(Some("Inspect tool call"));
    inspect_btn.add_css_class("flat");
    {
        let id = init.tool_call_id.clone();
        let on_inspect = on_inspect.clone();
        inspect_btn.connect_clicked(move |_| on_inspect(id.clone()));
    }

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    row.append(&spacer);

    row.append(&inspect_btn);
    root.append(&row);

    if let Some(preview) = init.displayed_preview() {
        let preview_label = gtk::Label::new(None);
        preview_label.add_css_class("caption");
        preview_label.add_css_class("dim-label");
        preview_label.add_css_class("preview-label");
        preview_label.set_halign(gtk::Align::Start);
        preview_label.set_margin_start(32);
        preview_label.set_margin_bottom(4);
        preview_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        if let Some(query) = init.highlight_query.as_deref() {
            let (markup, _) = highlight::highlight_text(preview, query);
            preview_label.set_markup(&markup);
        } else {
            preview_label.set_label(preview);
        }
        root.append(&preview_label);
    }

    ToolCallWidgetRefs { root }
}

pub(crate) fn populate_tool_burst_children(
    children: &gtk::Box,
    burst: &ToolBurstItemInit,
    sender: &relm4::Sender<SessionDetailMsg>,
    item_index: usize,
) {
    populate_tool_burst_children_impl(
        children,
        burst,
        Rc::new({
            let sender = sender.clone();
            move |id| {
                sender.emit(SessionDetailMsg::InspectToolCall(id));
            }
        }),
        Rc::new({
            let sender = sender.clone();
            move |_session_id, transcript_item_index| {
                sender.emit(SessionDetailMsg::InspectReasoning(transcript_item_index));
            }
        }),
        item_index,
    );
}

fn populate_tool_burst_children_impl(
    children: &gtk::Box,
    burst: &ToolBurstItemInit,
    on_inspect: Rc<dyn Fn(String)>,
    on_inspect_reasoning: Rc<dyn Fn(String, i64)>,
    item_index: usize,
) {
    let children_started_at = Instant::now();
    let mut max_child_build_duration = Duration::ZERO;
    for tool_call in &burst.tool_calls {
        let child_started_at = Instant::now();
        let child = build_tool_call_widget(
            tool_call,
            {
                let on_inspect = on_inspect.clone();
                move |id| {
                    on_inspect(id);
                }
            },
            {
                let on_inspect_reasoning = on_inspect_reasoning.clone();
                move |session_id, transcript_item_index| {
                    on_inspect_reasoning(session_id, transcript_item_index);
                }
            },
        );
        let child_duration = child_started_at.elapsed();
        max_child_build_duration = max_child_build_duration.max(child_duration);
        children.append(&child.root);
    }
    let children_duration = children_started_at.elapsed();
    tracing::debug!(
        item_index,
        tool_call_count = burst.tool_calls.len(),
        children_build_duration_ms = children_duration.as_millis(),
        max_child_build_duration_ms = max_child_build_duration.as_millis(),
        match_count = burst.match_count,
        "Built transcript tool burst children"
    );
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
            preview: crate::ui::tool_preview::extract_preview(
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
    item: &crate::ui::transcript_display::DisplayTranscriptItem,
    session_id: &str,
    highlight_query: Option<String>,
    db_path: Arc<PathBuf>,
) -> TranscriptItemInit {
    match item {
        crate::ui::transcript_display::DisplayTranscriptItem::Single(row) => {
            transcript_item_init_from_row_with_index(
                row,
                display_index,
                session_id,
                highlight_query,
                db_path,
            )
        }
        crate::ui::transcript_display::DisplayTranscriptItem::ToolBurst(burst) => {
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
    use crate::ui::session_detail::SessionDetailMsg;

    fn row_box_children(row: &gtk::Box) -> Vec<gtk::Widget> {
        let mut children = Vec::new();
        let mut child = row.first_child();

        while let Some(widget) = child {
            child = widget.next_sibling();
            children.push(widget);
        }

        children
    }

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
    fn model_label_text_assistant_with_model() {
        let result = model_label_text(Role::Assistant, Some("claude-sonnet-4-5-20250514"));
        assert_eq!(result.as_deref(), Some("claude-sonnet-4-5-20250514"));
    }

    #[test]
    fn model_label_text_assistant_empty_model() {
        assert_eq!(model_label_text(Role::Assistant, Some("")), None);
        assert_eq!(model_label_text(Role::Assistant, Some("  ")), None);
        assert_eq!(model_label_text(Role::Assistant, None), None);
    }

    #[test]
    fn model_label_text_non_assistant_with_model() {
        assert_eq!(model_label_text(Role::User, Some("o3-mini")), None);
        assert_eq!(model_label_text(Role::ToolResult, Some("o3-mini")), None);
        assert_eq!(model_label_text(Role::ToolCall, Some("o3-mini")), None);
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

    #[gtk::test]
    fn typed_tool_burst_population_emits_inspect_messages() {
        let burst = build_tool_burst_init(
            10,
            vec![ToolCallItemInit {
                item_index: 1,
                transcript_item_index: 41,
                session_id: "session-1".to_string(),
                tool_call_id: "call-1".to_string(),
                tool_name: "Read".to_string(),
                status: ToolCallStatus::Completed,
                preview: Some("src/ui/transcript_row.rs:1-20".to_string()),
                summary: None,
                duration_ms: Some(12),
                highlight_query: None,
                reasoning_preview: ReasoningPreview::default(),
            }],
            false,
        );
        let children = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let (sender, receiver) = relm4::channel::<SessionDetailMsg>();

        populate_tool_burst_children(&children, &burst, &sender, 10);

        let child = children
            .first_child()
            .and_then(|w| w.downcast::<gtk::Box>().ok())
            .expect("tool burst child root");
        let header = child
            .first_child()
            .and_then(|w| w.downcast::<gtk::Box>().ok())
            .expect("tool burst child header");
        let inspect = row_box_children(&header)
            .last()
            .cloned()
            .and_then(|w| w.downcast::<gtk::Button>().ok())
            .expect("inspect button");

        inspect.emit_clicked();

        assert!(matches!(
            gtk::glib::MainContext::default()
                .block_on(receiver.recv())
                .expect("inspect message"),
            SessionDetailMsg::InspectToolCall(id) if id == "call-1"
        ));
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
                preview: Some("read src/ui/transcript_row.rs".to_string()),
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
    fn tool_burst_accessible_label_summarizes_tools_and_errors() {
        let label = format_tool_burst_accessible_label(
            &[("Bash".to_string(), 1), ("Read".to_string(), 2)],
            3,
            1,
        );

        assert_eq!(label, "3 tool calls: 1 Bash, 2 Read, 1 error");
    }

    #[test]
    fn tool_burst_match_badge_accessible_label_is_descriptive() {
        assert_eq!(
            format_tool_burst_match_badge_accessible_label(2),
            "2 search matches inside this group"
        );
    }

    #[test]
    fn transcript_item_init_from_display_item_keeps_raw_child_transcript_indices() {
        let burst = crate::ui::transcript_display::DisplayTranscriptItem::ToolBurst(
            crate::ui::transcript_display::DisplayToolBurst {
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
    fn tool_burst_reasoning_header_prefers_visible_count() {
        assert_eq!(
            format_reasoning_burst_label(2, 1),
            Some("2 thinking".to_string())
        );
    }

    #[test]
    fn tool_burst_reasoning_header_falls_back_to_encrypted_count() {
        assert_eq!(
            format_reasoning_burst_label(0, 3),
            Some("3 encrypted".to_string())
        );
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
            r#"{"file_path":"src/ui/transcript_row.rs","offset":42,"limit":20}"#,
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
        assert!(read_preview.contains("transcript_row.rs:42-61"));

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
