//! Shared GTK renderers used by typed transcript rows.

use std::{rc::Rc, time::Duration, time::Instant};

use gtk::prelude::*;
use relm4::gtk;

use crate::models::Role;
use crate::ui::highlight;
use crate::ui::markdown;
use crate::ui::session_detail::SessionDetailMsg;
use crate::ui::session_detail::transcript::item_init::{ToolBurstItemInit, ToolCallItemInit};
use crate::ui::session_detail::transcript::tool_call_row::{
    ToolCallRowHeaderInit, build_tool_call_row_header,
};

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
mod tests {
    use super::*;
    use crate::models::{ReasoningPreview, ToolCallStatus};
    use crate::ui::session_detail::transcript::item_init::{
        ToolCallItemInit, build_tool_burst_init,
    };

    fn row_box_children(row: &gtk::Box) -> Vec<gtk::Widget> {
        let mut children = Vec::new();
        let mut child = row.first_child();

        while let Some(widget) = child {
            child = widget.next_sibling();
            children.push(widget);
        }

        children
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
                preview: Some("src/ui/transcript_row_rendering.rs:1-20".to_string()),
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
}
