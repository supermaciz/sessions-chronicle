use std::path::PathBuf;
use std::sync::Arc;
use std::{collections::BTreeMap, rc::Rc};

use anyhow::Result;
use chrono::{TimeZone, Utc};
use gtk::prelude::*;
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};
use relm4::gtk;

use crate::database::load_message_full_content;
use crate::models::{MessagePreview, Role, ToolCallStatus};
use crate::ui::format::{format_duration_ms, tool_status_css_class, tool_status_label};
use crate::ui::highlight;
use crate::ui::markdown;

/// Return the model display text for a transcript header.
/// Only assistant messages with a non-empty model value produce output.
fn model_label_text(role: Role, model: Option<&str>) -> Option<String> {
    if role != Role::Assistant {
        return None;
    }
    let text = model?.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

// ---------------------------------------------------------------------------
// Init types
// ---------------------------------------------------------------------------

pub struct MessageItemInit {
    pub item_index: usize,
    pub preview: MessagePreview,
    pub highlight_query: Option<String>,
    pub db_path: Arc<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ToolCallItemInit {
    pub item_index: usize,
    pub tool_call_id: String,
    pub tool_name: String,
    pub status: ToolCallStatus,
    pub preview: Option<String>,
    pub summary: Option<String>,
    pub duration_ms: Option<i64>,
    pub highlight_query: Option<String>,
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
    pub default_expanded: bool,
}

pub struct SubagentItemInit {
    pub item_index: usize,
    pub subagent_id: String,
    pub title: String,
}

pub enum TranscriptItemInit {
    Message(MessageItemInit),
    ToolCall(ToolCallItemInit),
    ToolBurst(ToolBurstItemInit),
    Subagent(SubagentItemInit),
}

// ---------------------------------------------------------------------------
// Messages and outputs
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum TranscriptRowMsg {
    ToggleExpand,
    InspectClicked,
}

#[derive(Debug)]
pub enum TranscriptRowCmd {
    FullContentLoaded(Result<String>),
}

#[derive(Debug)]
pub enum TranscriptRowOutput {
    MatchSegmentsChanged {
        item_index: usize,
        segments: Vec<usize>,
    },
    ExpandLoadFailed {
        #[allow(dead_code)]
        item_index: usize,
    },
    InspectToolCall(String),
    InspectSubagent(String),
}

// ---------------------------------------------------------------------------
// Kind enum
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum TranscriptRowKind {
    Message,
    ToolCall,
    ToolBurst,
    Subagent,
}

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct TranscriptRow {
    item_index: usize,
    kind: TranscriptRowKind,

    // --- Message state ---
    preview: Option<MessagePreview>,
    highlight_query: Option<String>,
    db_path: Option<Arc<PathBuf>>,
    expanded: bool,
    full_content: Option<String>,
    loading_full_content: bool,
    rendered_match_count: usize,

    // --- ToolCall state ---
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    tool_status: Option<ToolCallStatus>,
    tool_preview: Option<String>,
    tool_summary: Option<String>,
    tool_duration_ms: Option<i64>,
    tool_highlight_query: Option<String>,

    // --- ToolBurst state ---
    tool_burst: Option<ToolBurstItemInit>,

    // --- Subagent state ---
    subagent_id: Option<String>,
    subagent_title: Option<String>,
}

// ---------------------------------------------------------------------------
// Widgets
// ---------------------------------------------------------------------------

pub struct TranscriptRowWidgets {
    // Message widgets
    content_container: gtk::Box,
    expand_button: gtk::Button,
    // ToolCall widgets (needed for inspect button sensitivity)
    // Subagent widgets
    // (Nothing dynamic for tool/subagent in Phase 3)
}

// ---------------------------------------------------------------------------
// FactoryComponent implementation
// ---------------------------------------------------------------------------

fn render_content(
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

fn count_tool_call_matches(init: &ToolCallItemInit) -> usize {
    let Some(query) = init.highlight_query.as_deref() else {
        return 0;
    };

    let mut count = highlight::find_case_insensitive_matches_in_text(&init.tool_name, query).len();
    if let Some(preview) = init.preview.as_deref() {
        count += highlight::find_case_insensitive_matches_in_text(preview, query).len();
    }
    if let Some(summary) = init.summary.as_deref() {
        count += highlight::find_case_insensitive_matches_in_text(summary, query).len();
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
        default_expanded,
    }
}

fn format_tool_burst_accessible_label(
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

pub fn format_tool_burst_match_badge_accessible_label(match_count: usize) -> String {
    format!("{match_count} search matches inside this group")
}

struct ToolCallWidgetRefs {
    root: gtk::Box,
    match_count: usize,
}

fn build_tool_call_widget(
    init: &ToolCallItemInit,
    on_inspect: impl Fn(String) + 'static,
) -> ToolCallWidgetRefs {
    let on_inspect = Rc::new(on_inspect);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("tool-call-row");
    root.set_margin_top(2);
    root.set_margin_bottom(2);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_margin_start(8);
    row.set_margin_end(4);
    row.set_margin_top(4);
    row.set_margin_bottom(4);

    let icon = gtk::Image::new();
    icon.set_icon_name(Some("utilities-terminal-symbolic"));
    icon.set_pixel_size(16);
    row.append(&icon);

    let name_label = gtk::Label::new(None);
    name_label.add_css_class("monospace");
    name_label.set_halign(gtk::Align::Start);
    name_label.set_hexpand(true);
    name_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    if let Some(query) = init.highlight_query.as_deref() {
        let (markup, _) = highlight::highlight_text(&init.tool_name, query);
        name_label.set_markup(&markup);
    } else {
        name_label.set_label(&init.tool_name);
    }
    row.append(&name_label);

    let status_label = gtk::Label::new(Some(tool_status_label(init.status)));
    status_label.add_css_class("caption");
    status_label.add_css_class(tool_status_css_class(init.status));
    row.append(&status_label);

    if let Some(ms) = init.duration_ms {
        let dur_label = gtk::Label::new(Some(&format_duration_ms(ms)));
        dur_label.add_css_class("caption");
        dur_label.add_css_class("dim-label");
        row.append(&dur_label);
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
    row.append(&inspect_btn);
    root.append(&row);

    if let Some(preview) = init.preview.as_deref().or(init.summary.as_deref()) {
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

    ToolCallWidgetRefs {
        root,
        match_count: count_tool_call_matches(init),
    }
}

impl FactoryComponent for TranscriptRow {
    type Init = TranscriptItemInit;
    type Input = TranscriptRowMsg;
    type Output = TranscriptRowOutput;
    type CommandOutput = TranscriptRowCmd;
    type Root = gtk::Box;
    type Widgets = TranscriptRowWidgets;
    type ParentWidget = gtk::Box;
    type Index = DynamicIndex;

    fn init_root(&self) -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init_model(init: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        match init {
            TranscriptItemInit::Message(m) => Self {
                item_index: m.item_index,
                kind: TranscriptRowKind::Message,
                preview: Some(m.preview),
                highlight_query: m.highlight_query,
                db_path: Some(m.db_path),
                expanded: false,
                full_content: None,
                loading_full_content: false,
                rendered_match_count: 0,
                tool_call_id: None,
                tool_name: None,
                tool_status: None,
                tool_preview: None,
                tool_summary: None,
                tool_duration_ms: None,
                tool_highlight_query: None,
                tool_burst: None,
                subagent_id: None,
                subagent_title: None,
            },
            TranscriptItemInit::ToolCall(tc) => Self {
                item_index: tc.item_index,
                kind: TranscriptRowKind::ToolCall,
                preview: None,
                highlight_query: None,
                db_path: None,
                expanded: false,
                full_content: None,
                loading_full_content: false,
                rendered_match_count: 0,
                tool_call_id: Some(tc.tool_call_id),
                tool_name: Some(tc.tool_name),
                tool_status: Some(tc.status),
                tool_preview: tc.preview,
                tool_summary: tc.summary,
                tool_duration_ms: tc.duration_ms,
                tool_highlight_query: tc.highlight_query,
                tool_burst: None,
                subagent_id: None,
                subagent_title: None,
            },
            TranscriptItemInit::ToolBurst(tb) => Self {
                item_index: tb.item_index,
                kind: TranscriptRowKind::ToolBurst,
                preview: None,
                highlight_query: None,
                db_path: None,
                expanded: false,
                full_content: None,
                loading_full_content: false,
                rendered_match_count: tb.match_count,
                tool_call_id: None,
                tool_name: None,
                tool_status: None,
                tool_preview: None,
                tool_summary: None,
                tool_duration_ms: None,
                tool_highlight_query: None,
                tool_burst: Some(tb),
                subagent_id: None,
                subagent_title: None,
            },
            TranscriptItemInit::Subagent(sa) => Self {
                item_index: sa.item_index,
                kind: TranscriptRowKind::Subagent,
                preview: None,
                highlight_query: None,
                db_path: None,
                expanded: false,
                full_content: None,
                loading_full_content: false,
                rendered_match_count: 0,
                tool_call_id: None,
                tool_name: None,
                tool_status: None,
                tool_preview: None,
                tool_summary: None,
                tool_duration_ms: None,
                tool_highlight_query: None,
                tool_burst: None,
                subagent_id: Some(sa.subagent_id),
                subagent_title: Some(sa.title),
            },
        }
    }

    fn init_widgets(
        &mut self,
        _index: &DynamicIndex,
        root: Self::Root,
        _returned_widget: &gtk::Widget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        match self.kind {
            TranscriptRowKind::Message => self.build_message_widgets(&root, sender),
            TranscriptRowKind::ToolCall => self.build_tool_call_widgets(&root, sender),
            TranscriptRowKind::ToolBurst => self.build_tool_burst_widgets(&root, sender),
            TranscriptRowKind::Subagent => self.build_subagent_widgets(&root, sender),
        }
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::Input,
        sender: FactorySender<Self>,
    ) {
        match message {
            TranscriptRowMsg::ToggleExpand => {
                if self.kind != TranscriptRowKind::Message {
                    return;
                }
                let Some(ref preview) = self.preview else {
                    return;
                };

                if self.expanded {
                    // Collapse: show preview
                    self.expanded = false;
                    let count = render_content(
                        &widgets.content_container,
                        &preview.content_preview,
                        preview.role,
                        self.highlight_query.as_deref(),
                    );
                    self.update_expand_button(widgets, preview);
                    if count != self.rendered_match_count {
                        self.rendered_match_count = count;
                        sender
                            .output(TranscriptRowOutput::MatchSegmentsChanged {
                                item_index: self.item_index,
                                segments: vec![count],
                            })
                            .ok();
                    }
                } else if let Some(ref full) = self.full_content {
                    // Expand with cached content
                    self.expanded = true;
                    let count = render_content(
                        &widgets.content_container,
                        full,
                        preview.role,
                        self.highlight_query.as_deref(),
                    );
                    self.update_expand_button(widgets, preview);
                    if count != self.rendered_match_count {
                        self.rendered_match_count = count;
                        sender
                            .output(TranscriptRowOutput::MatchSegmentsChanged {
                                item_index: self.item_index,
                                segments: vec![count],
                            })
                            .ok();
                    }
                } else {
                    // Fetch full content from DB
                    self.loading_full_content = true;
                    self.update_expand_button(widgets, preview);
                    let db_path = self.db_path.clone().expect("message row has db_path");
                    let session_id = preview.session_id.clone();
                    let message_index = preview.message_index;
                    sender.spawn_oneshot_command(move || {
                        TranscriptRowCmd::FullContentLoaded(load_message_full_content(
                            &db_path,
                            &session_id,
                            message_index,
                        ))
                    });
                }
            }
            TranscriptRowMsg::InspectClicked => match self.kind {
                TranscriptRowKind::ToolCall => {
                    if let Some(ref id) = self.tool_call_id {
                        sender
                            .output(TranscriptRowOutput::InspectToolCall(id.clone()))
                            .ok();
                    }
                }
                TranscriptRowKind::ToolBurst => {}
                TranscriptRowKind::Subagent => {
                    if let Some(ref id) = self.subagent_id {
                        sender
                            .output(TranscriptRowOutput::InspectSubagent(id.clone()))
                            .ok();
                    }
                }
                TranscriptRowKind::Message => {}
            },
        }
    }

    fn update_cmd_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::CommandOutput,
        sender: FactorySender<Self>,
    ) {
        match message {
            TranscriptRowCmd::FullContentLoaded(Ok(content)) => {
                let Some(ref preview) = self.preview else {
                    return;
                };
                self.full_content = Some(content.clone());
                self.expanded = true;
                self.loading_full_content = false;
                let count = render_content(
                    &widgets.content_container,
                    &content,
                    preview.role,
                    self.highlight_query.as_deref(),
                );
                self.update_expand_button(widgets, preview);
                if count != self.rendered_match_count {
                    self.rendered_match_count = count;
                    sender
                        .output(TranscriptRowOutput::MatchSegmentsChanged {
                            item_index: self.item_index,
                            segments: vec![count],
                        })
                        .ok();
                }
            }
            TranscriptRowCmd::FullContentLoaded(Err(err)) => {
                let Some(ref preview) = self.preview else {
                    return;
                };
                tracing::error!(
                    "Failed to load full content for item {}: {}",
                    self.item_index,
                    err
                );
                self.expanded = false;
                self.loading_full_content = false;
                self.update_expand_button(widgets, preview);
                sender
                    .output(TranscriptRowOutput::ExpandLoadFailed {
                        item_index: self.item_index,
                    })
                    .ok();
            }
        }
    }
}

impl TranscriptRow {
    /// Build the widget tree for a message transcript item.
    fn build_message_widgets(
        &mut self,
        root: &gtk::Box,
        sender: FactorySender<Self>,
    ) -> TranscriptRowWidgets {
        let preview = self.preview.as_ref().expect("message kind has preview");

        root.add_css_class("message-row");
        root.add_css_class(preview.role.css_class());
        root.set_spacing(4);

        // Header: role label [· model] · timestamp
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let role_label = gtk::Label::new(Some(preview.role.label()));
        role_label.add_css_class("caption");
        role_label.add_css_class("heading");
        role_label.add_css_class(preview.role.css_class());
        role_label.set_halign(gtk::Align::Start);
        header.append(&role_label);

        if let Some(model_text) = model_label_text(preview.role, preview.model.as_deref()) {
            let sep1 = gtk::Label::new(Some("·"));
            sep1.add_css_class("caption");
            sep1.add_css_class("dim-label");
            header.append(&sep1);

            let model_label = gtk::Label::new(Some(&model_text));
            model_label.add_css_class("caption");
            model_label.add_css_class("dim-label");
            model_label.add_css_class("monospace");
            model_label.set_halign(gtk::Align::Start);
            header.append(&model_label);
        }

        let sep_ts = gtk::Label::new(Some("·"));
        sep_ts.add_css_class("caption");
        sep_ts.add_css_class("dim-label");
        header.append(&sep_ts);

        let ts_label = gtk::Label::new(Some(&preview.timestamp.format("%H:%M:%S").to_string()));
        ts_label.add_css_class("caption");
        ts_label.add_css_class("dim-label");
        ts_label.set_halign(gtk::Align::Start);
        header.append(&ts_label);

        root.append(&header);

        // Content container
        let content_container = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.append(&content_container);

        // Expand toggle button
        let expand_button = gtk::Button::new();
        expand_button.add_css_class("flat");
        expand_button.add_css_class("caption");
        expand_button.add_css_class("expand-toggle");
        expand_button.set_halign(gtk::Align::Start);
        expand_button.set_margin_top(4);
        expand_button.set_label("Show full message");
        expand_button.set_visible(preview.is_truncated() && preview.role != Role::ToolResult);
        {
            let s = sender.clone();
            expand_button.connect_clicked(move |_| {
                s.input(TranscriptRowMsg::ToggleExpand);
            });
        }
        root.append(&expand_button);

        // Render initial content
        let match_count = render_content(
            &content_container,
            &preview.content_preview,
            preview.role,
            self.highlight_query.as_deref(),
        );
        self.rendered_match_count = match_count;
        sender
            .output(TranscriptRowOutput::MatchSegmentsChanged {
                item_index: self.item_index,
                segments: vec![match_count],
            })
            .ok();

        TranscriptRowWidgets {
            content_container,
            expand_button,
        }
    }

    /// Build the widget tree for a tool call transcript item.
    fn build_tool_call_widgets(
        &mut self,
        root: &gtk::Box,
        sender: FactorySender<Self>,
    ) -> TranscriptRowWidgets {
        let init = ToolCallItemInit {
            item_index: self.item_index,
            tool_call_id: self.tool_call_id.clone().unwrap_or_default(),
            tool_name: self
                .tool_name
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            status: self.tool_status.unwrap_or(ToolCallStatus::Unknown),
            preview: self.tool_preview.clone(),
            summary: self.tool_summary.clone(),
            duration_ms: self.tool_duration_ms,
            highlight_query: self.tool_highlight_query.clone(),
        };

        let refs = build_tool_call_widget(&init, {
            let sender = sender.clone();
            move |id| {
                sender.output(TranscriptRowOutput::InspectToolCall(id)).ok();
            }
        });
        root.append(&refs.root);
        self.rendered_match_count = refs.match_count;
        sender
            .output(TranscriptRowOutput::MatchSegmentsChanged {
                item_index: self.item_index,
                segments: vec![refs.match_count],
            })
            .ok();

        TranscriptRowWidgets {
            content_container: gtk::Box::new(gtk::Orientation::Vertical, 0),
            expand_button: gtk::Button::new(),
        }
    }

    fn build_tool_burst_widgets(
        &mut self,
        root: &gtk::Box,
        sender: FactorySender<Self>,
    ) -> TranscriptRowWidgets {
        let Some(burst) = self.tool_burst.as_ref() else {
            return TranscriptRowWidgets {
                content_container: gtk::Box::new(gtk::Orientation::Vertical, 0),
                expand_button: gtk::Button::new(),
            };
        };

        root.add_css_class("tool-call-group");

        let expander = gtk::Expander::new(None);
        expander.set_expanded(burst.default_expanded);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.add_css_class("tool-call-group-header");

        for (name, count) in &burst.category_counts {
            let pill = gtk::Label::new(Some(&format!("{count} {name}")));
            pill.add_css_class("pill");
            pill.add_css_class("tool-call-group-pill");
            header.append(&pill);
        }

        if let Some(total_ms) = burst.total_duration_ms {
            let duration = gtk::Label::new(Some(&format_duration_ms(total_ms)));
            duration.add_css_class("caption");
            duration.add_css_class("dim-label");
            header.append(&duration);
        }

        let total = gtk::Label::new(Some(&format!("{} tool calls", burst.tool_calls.len())));
        total.add_css_class("caption");
        total.add_css_class("dim-label");
        header.append(&total);

        if burst.error_count > 0 {
            let error_label = gtk::Label::new(Some(&format!(
                "{} {}",
                burst.error_count,
                if burst.error_count == 1 {
                    "error"
                } else {
                    "errors"
                }
            )));
            error_label.add_css_class("caption");
            error_label.add_css_class("status-error");
            header.append(&error_label);
        }

        let burst_match_count: usize = burst.child_match_counts.iter().sum();
        if burst_match_count > 0 {
            let badge = gtk::Label::new(Some(&format!("{} matches", burst_match_count)));
            badge.add_css_class("pill");
            badge.add_css_class("accent");
            badge.add_css_class("tool-call-group-pill");
            let badge_a11y = format_tool_burst_match_badge_accessible_label(burst_match_count);
            badge.update_property(&[gtk::accessible::Property::Label(&badge_a11y)]);
            header.append(&badge);
        }

        expander.set_label_widget(Some(&header));
        let expander_a11y = format_tool_burst_accessible_label(
            &burst.category_counts,
            burst.tool_calls.len(),
            burst.error_count,
        );
        expander.update_property(&[gtk::accessible::Property::Label(&expander_a11y)]);

        let children = gtk::Box::new(gtk::Orientation::Vertical, 0);
        for tool_call in &burst.tool_calls {
            let child = build_tool_call_widget(tool_call, {
                let sender = sender.clone();
                move |id| {
                    sender.output(TranscriptRowOutput::InspectToolCall(id)).ok();
                }
            });
            children.append(&child.root);
        }

        expander.set_child(Some(&children));
        root.append(&expander);

        self.rendered_match_count = burst_match_count;
        sender
            .output(TranscriptRowOutput::MatchSegmentsChanged {
                item_index: self.item_index,
                segments: burst.child_match_counts.clone(),
            })
            .ok();

        TranscriptRowWidgets {
            content_container: gtk::Box::new(gtk::Orientation::Vertical, 0),
            expand_button: gtk::Button::new(),
        }
    }

    /// Build the widget tree for a subagent transcript item.
    fn build_subagent_widgets(
        &mut self,
        root: &gtk::Box,
        sender: FactorySender<Self>,
    ) -> TranscriptRowWidgets {
        root.add_css_class("subagent-row");
        root.set_margin_top(2);
        root.set_margin_bottom(2);

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_margin_start(8);
        row.set_margin_end(4);
        row.set_margin_top(4);
        row.set_margin_bottom(4);

        // Subagent icon
        let icon = gtk::Image::new();
        icon.set_icon_name(Some("system-run-symbolic"));
        icon.set_pixel_size(16);
        row.append(&icon);

        // Title
        let title_label = gtk::Label::new(self.subagent_title.as_deref());
        title_label.set_halign(gtk::Align::Start);
        title_label.set_hexpand(true);
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&title_label);

        // Inspect button
        let inspect_btn = gtk::Button::new();
        inspect_btn.set_icon_name("view-reveal-symbolic");
        inspect_btn.set_tooltip_text(Some("Inspect subagent"));
        inspect_btn.add_css_class("flat");
        {
            let s = sender.clone();
            inspect_btn.connect_clicked(move |_| {
                s.input(TranscriptRowMsg::InspectClicked);
            });
        }
        row.append(&inspect_btn);

        root.append(&row);

        // Return dummy widgets struct
        TranscriptRowWidgets {
            content_container: gtk::Box::new(gtk::Orientation::Vertical, 0),
            expand_button: gtk::Button::new(),
        }
    }

    /// Update expand button label/sensitivity after state changes.
    fn update_expand_button(&self, widgets: &mut TranscriptRowWidgets, preview: &MessagePreview) {
        let label = if self.loading_full_content {
            "Loading..."
        } else if self.expanded {
            "Collapse"
        } else {
            "Show full message"
        };
        widgets.expand_button.set_label(label);
        widgets
            .expand_button
            .set_sensitive(!self.loading_full_content);
        widgets
            .expand_button
            .set_visible(preview.is_truncated() && preview.role != Role::ToolResult);
    }
}

/// Build a `TranscriptItemInit` from a `TranscriptItemRow` returned by the DB query.
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
                preview: MessagePreview {
                    session_id: session_id.to_string(),
                    message_index,
                    role,
                    content_preview: row.content_preview.clone().unwrap_or_default(),
                    content_len: row.content_len.unwrap_or(0) as usize,
                    timestamp,
                    model: row.model.clone(),
                },
                highlight_query,
                db_path,
            })
        }
        TranscriptItemKind::ToolCall => TranscriptItemInit::ToolCall(ToolCallItemInit {
            item_index,
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
        }),
        TranscriptItemKind::Subagent => TranscriptItemInit::Subagent(SubagentItemInit {
            item_index,
            subagent_id: row.subagent_id.clone().unwrap_or_default(),
            title: row
                .subagent_title
                .clone()
                .unwrap_or_else(|| "Subagent".to_string()),
        }),
        TranscriptItemKind::Unknown => {
            tracing::warn!(
                item_index = row.item_index,
                "transcript item with unknown kind; rendering as empty message"
            );
            TranscriptItemInit::Message(MessageItemInit {
                item_index,
                preview: MessagePreview {
                    session_id: session_id.to_string(),
                    message_index: 0,
                    role: Role::User,
                    content_preview: String::new(),
                    content_len: 0,
                    timestamp: Utc::now(),
                    model: None,
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
                .map(|row| {
                    match transcript_item_init_from_row_with_index(
                        row,
                        display_index,
                        session_id,
                        highlight_query.clone(),
                        db_path.clone(),
                    ) {
                        TranscriptItemInit::ToolCall(tool_call) => tool_call,
                        _ => panic!("tool burst child must be a tool call"),
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
    fn tool_call_match_count_includes_name_preview_and_summary() {
        let init = ToolCallItemInit {
            item_index: 7,
            tool_call_id: "call-7".to_string(),
            tool_name: "Read".to_string(),
            status: ToolCallStatus::Completed,
            preview: Some("src/ui/session_detail.rs:1-20".to_string()),
            summary: Some("read the transcript loader".to_string()),
            duration_ms: Some(12),
            highlight_query: Some("read".to_string()),
        };

        assert_eq!(count_tool_call_matches(&init), 2);
    }

    #[test]
    fn tool_burst_item_init_aggregates_categories_duration_errors_and_matches() {
        let tool_calls = vec![
            ToolCallItemInit {
                item_index: 1,
                tool_call_id: "call-1".to_string(),
                tool_name: "Read".to_string(),
                status: ToolCallStatus::Completed,
                preview: Some("read src/ui/transcript_row.rs".to_string()),
                summary: None,
                duration_ms: Some(5),
                highlight_query: Some("read".to_string()),
            },
            ToolCallItemInit {
                item_index: 2,
                tool_call_id: "call-2".to_string(),
                tool_name: "Edit".to_string(),
                status: ToolCallStatus::Error,
                preview: Some("edit src/ui/session_detail.rs".to_string()),
                summary: None,
                duration_ms: Some(8),
                highlight_query: Some("edit".to_string()),
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
    fn transcript_item_init_prefers_extracted_preview_over_summary() {
        let row = crate::database::TranscriptItemRow {
            item_index: 1,
            kind: crate::models::TranscriptItemKind::ToolCall,
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
