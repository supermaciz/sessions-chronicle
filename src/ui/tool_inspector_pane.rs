use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use adw::prelude::*;
use chrono::TimeZone;
use relm4::{Component, ComponentParts, ComponentSender, RelmWidgetExt, adw, gtk};

use crate::database::{load_reasoning_attachment, load_subagent, load_tool_call};
use crate::models::{ReasoningAttachment, Subagent, ToolCall, ToolCallStatus};
use crate::ui::format::format_duration_ms;
use crate::ui::markdown;
use crate::ui::tool_renderers::diff::DiffRenderer;
use crate::ui::tool_renderers::file::FileRenderer;
use crate::ui::tool_renderers::generic::GenericRenderer;
use crate::ui::tool_renderers::results::ResultsRenderer;
use crate::ui::tool_renderers::subagent::SubagentRenderer;
use crate::ui::tool_renderers::terminal::TerminalRenderer;
use crate::ui::tool_renderers::{RendererInit, RendererKind, resolve_renderer};

// ── Selection state ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
enum InspectorSelection {
    #[default]
    None,
    ToolCall {
        // Retained for potential future reload; currently only used at select time.
        #[allow(dead_code)]
        session_id: String,
        #[allow(dead_code)]
        tool_call_id: String,
    },
    Subagent {
        session_id: String,
        #[allow(dead_code)]
        subagent_id: String,
    },
    Reasoning {
        #[allow(dead_code)]
        session_id: String,
        #[allow(dead_code)]
        transcript_item_index: i64,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum LoadState {
    #[default]
    Idle,
    Loading,
    Ready,
    LoadError(String),
}

#[derive(Clone)]
struct RendererStackViews {
    stack: gtk::Stack,
    generic_container: gtk::Box,
    terminal_container: gtk::Box,
    diff_container: gtk::Box,
    file_container: gtk::Box,
    results_container: gtk::Box,
    subagent_container: gtk::Box,
}

#[derive(Clone)]
struct MarkdownSectionViews {
    section: gtk::Box,
    content: gtk::Box,
}

#[derive(Clone)]
struct TextSectionViews {
    section: gtk::Box,
    label: gtk::Label,
}

struct ToolDetailViews {
    name_label: gtk::Label,
    status_label: gtk::Label,
    metadata_label: gtk::Label,
    error_views: TextSectionViews,
    renderer_views: RendererStackViews,
}

struct SubagentDetailViews {
    title_label: gtk::Label,
    prompt_views: MarkdownSectionViews,
    result_views: MarkdownSectionViews,
    open_session_button: gtk::Button,
}

struct ReasoningDetailViews {
    title_label: gtk::Label,
    metadata_label: gtk::Label,
    visible_views: MarkdownSectionViews,
    summary_views: MarkdownSectionViews,
}

struct OverviewStackViews {
    content_stack: gtk::Stack,
    error_label: gtk::Label,
    tool_detail: ToolDetailViews,
    tool_scroll: gtk::ScrolledWindow,
    subagent_detail: SubagentDetailViews,
    subagent_scroll: gtk::ScrolledWindow,
    reasoning_detail: ReasoningDetailViews,
    reasoning_scroll: gtk::ScrolledWindow,
}

// ── Component ─────────────────────────────────────────────────────────────────

pub struct ToolInspectorPane {
    db_path: Arc<PathBuf>,
    selection: InspectorSelection,
    load_state: LoadState,
    active_request_id: u64,
    tool_call: Option<ToolCall>,
    subagent: Option<Subagent>,
    reasoning: Option<ReasoningAttachment>,

    // Overview content switcher: "empty" / "tool" / "subagent"
    content_stack: gtk::Stack,
    error_label: gtk::Label,

    // Tool-call detail widgets (inside "tool" stack page)
    tool_name_label: gtk::Label,
    tool_status_label: gtk::Label,
    tool_metadata_label: gtk::Label,
    tool_error_views: TextSectionViews,
    tool_renderer_views: RendererStackViews,
    tool_scroll: gtk::ScrolledWindow,

    // Subagent detail widgets (inside "subagent" stack page)
    subagent_title_label: gtk::Label,
    subagent_prompt_views: MarkdownSectionViews,
    subagent_result_views: MarkdownSectionViews,
    open_session_button: gtk::Button,
    subagent_scroll: gtk::ScrolledWindow,

    // Reasoning detail widgets (inside "reasoning" stack page)
    reasoning_title_label: gtk::Label,
    reasoning_metadata_label: gtk::Label,
    reasoning_visible_views: MarkdownSectionViews,
    reasoning_summary_views: MarkdownSectionViews,
    reasoning_scroll: gtk::ScrolledWindow,
}

#[derive(Debug)]
pub enum ToolInspectorPaneMsg {
    SelectToolCall {
        session_id: String,
        tool_call_id: String,
    },
    SelectSubagent {
        session_id: String,
        subagent_id: String,
    },
    SelectReasoning {
        session_id: String,
        transcript_item_index: i64,
    },
    Clear,
    /// Open the child session linked from the current subagent.
    OpenChildSession,
}

#[derive(Debug)]
pub enum ToolInspectorPaneOutput {
    OpenChildSession(String),
}

#[derive(Debug)]
pub enum ToolInspectorPaneCmd {
    ToolCall {
        request_id: u64,
        session_id: String,
        tool_call_id: String,
        load_duration_ms: u128,
        result: Result<Option<ToolCall>, String>,
    },
    Subagent {
        request_id: u64,
        session_id: String,
        subagent_id: String,
        load_duration_ms: u128,
        subagent_result: Result<Option<Subagent>, String>,
    },
    Reasoning {
        request_id: u64,
        load_duration_ms: u128,
        result: Result<Option<ReasoningAttachment>, String>,
    },
}

// ── Component impl ────────────────────────────────────────────────────────────

#[relm4::component(pub)]
impl Component for ToolInspectorPane {
    type Init = Arc<PathBuf>;
    type Input = ToolInspectorPaneMsg;
    type Output = ToolInspectorPaneOutput;
    type CommandOutput = ToolInspectorPaneCmd;
    type Widgets = ToolInspectorPaneWidgets;

    /// Minimal root widget — the real widget tree is built imperatively in init().
    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_vexpand: true,
            set_hexpand: true,
        }
    }

    fn init(
        db_path: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let overview_views = build_overview_stack(&sender);
        root.append(&overview_views.content_stack);
        let model = build_tool_inspector_model(db_path, overview_views);

        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            ToolInspectorPaneMsg::SelectToolCall {
                session_id,
                tool_call_id,
            } => self.select_tool_call(&sender, session_id, tool_call_id),

            ToolInspectorPaneMsg::SelectSubagent {
                session_id,
                subagent_id,
            } => self.select_subagent(&sender, session_id, subagent_id),

            ToolInspectorPaneMsg::SelectReasoning {
                session_id,
                transcript_item_index,
            } => self.select_reasoning(&sender, session_id, transcript_item_index),

            ToolInspectorPaneMsg::Clear => self.clear_selection(),

            ToolInspectorPaneMsg::OpenChildSession => self.emit_open_child_session(&sender),
        }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            ToolInspectorPaneCmd::ToolCall {
                request_id,
                session_id,
                tool_call_id,
                load_duration_ms,
                result,
            } => self.apply_tool_call_cmd(
                request_id,
                &session_id,
                &tool_call_id,
                load_duration_ms,
                result,
            ),
            ToolInspectorPaneCmd::Subagent {
                request_id,
                session_id,
                subagent_id,
                load_duration_ms,
                subagent_result,
            } => self.apply_subagent_cmd(
                request_id,
                &session_id,
                &subagent_id,
                load_duration_ms,
                subagent_result,
            ),
            ToolInspectorPaneCmd::Reasoning {
                request_id,
                load_duration_ms,
                result,
            } => self.apply_reasoning_cmd(request_id, load_duration_ms, result),
        }
    }

    fn post_view(&self, _widgets: &mut Self::Widgets) {
        let started_at = Instant::now();
        self.content_stack
            .set_visible_child_name(self.visible_page_name());
        self.render_tool_call_section();
        self.render_subagent_section();
        self.render_reasoning_section();
        tracing::debug!(
            duration_ms = started_at.elapsed().as_millis(),
            "Updated tool inspector pane view"
        );
    }
}

impl ToolInspectorPane {
    fn clear_loaded_content(&mut self) {
        self.tool_call = None;
        self.subagent = None;
        self.reasoning = None;
    }

    fn begin_selection_load(&mut self) -> u64 {
        self.clear_loaded_content();
        self.reset_overview_scroll_positions();
        begin_loading_request(&mut self.active_request_id, &mut self.load_state)
    }

    fn reset_overview_scroll_positions(&self) {
        reset_scroll_position(&self.tool_scroll);
        reset_scroll_position(&self.subagent_scroll);
        reset_scroll_position(&self.reasoning_scroll);
    }

    fn select_tool_call(
        &mut self,
        sender: &ComponentSender<Self>,
        session_id: String,
        tool_call_id: String,
    ) {
        self.selection = InspectorSelection::ToolCall {
            session_id: session_id.clone(),
            tool_call_id: tool_call_id.clone(),
        };
        let request_id = self.begin_selection_load();
        tracing::info!(
            request_id,
            session_id = session_id.as_str(),
            tool_call_id = tool_call_id.as_str(),
            "Inspector tool call selection started"
        );
        let db_path = self.db_path.clone();
        sender.spawn_oneshot_command(move || {
            let started_at = Instant::now();
            let result = load_tool_call(db_path.as_path(), &session_id, &tool_call_id)
                .map_err(|err| err.to_string());
            let load_duration_ms = started_at.elapsed().as_millis();
            ToolInspectorPaneCmd::ToolCall {
                request_id,
                session_id: session_id.clone(),
                tool_call_id: tool_call_id.clone(),
                load_duration_ms,
                result,
            }
        });
    }

    fn select_subagent(
        &mut self,
        sender: &ComponentSender<Self>,
        session_id: String,
        subagent_id: String,
    ) {
        self.selection = InspectorSelection::Subagent {
            session_id: session_id.clone(),
            subagent_id: subagent_id.clone(),
        };
        let request_id = self.begin_selection_load();
        tracing::info!(
            request_id,
            session_id = session_id.as_str(),
            subagent_id = subagent_id.as_str(),
            "Inspector subagent selection started"
        );
        let db_path = self.db_path.clone();
        sender.spawn_oneshot_command(move || {
            let started_at = Instant::now();
            let subagent_result = load_subagent(db_path.as_path(), &session_id, &subagent_id)
                .map_err(|err| err.to_string());
            let load_duration_ms = started_at.elapsed().as_millis();
            ToolInspectorPaneCmd::Subagent {
                request_id,
                session_id: session_id.clone(),
                subagent_id: subagent_id.clone(),
                load_duration_ms,
                subagent_result,
            }
        });
    }

    fn select_reasoning(
        &mut self,
        sender: &ComponentSender<Self>,
        session_id: String,
        transcript_item_index: i64,
    ) {
        self.selection = InspectorSelection::Reasoning {
            session_id: session_id.clone(),
            transcript_item_index,
        };
        let request_id = self.begin_selection_load();
        tracing::info!(
            request_id,
            session_id = session_id.as_str(),
            transcript_item_index,
            "Inspector reasoning selection started"
        );
        let db_path = self.db_path.clone();
        sender.spawn_oneshot_command(move || {
            let started_at = Instant::now();
            let result =
                load_reasoning_attachment(db_path.as_path(), &session_id, transcript_item_index)
                    .map_err(|err| err.to_string());
            let load_duration_ms = started_at.elapsed().as_millis();
            ToolInspectorPaneCmd::Reasoning {
                request_id,
                load_duration_ms,
                result,
            }
        });
    }

    fn clear_selection(&mut self) {
        self.selection = InspectorSelection::None;
        clear_active_request(&mut self.active_request_id, &mut self.load_state);
        self.clear_loaded_content();
    }

    fn emit_open_child_session(&self, sender: &ComponentSender<Self>) {
        if let Some(child_id) = self
            .subagent
            .as_ref()
            .and_then(|subagent| subagent.child_session_id.clone())
        {
            sender
                .output(ToolInspectorPaneOutput::OpenChildSession(child_id))
                .ok();
        }
    }

    fn accept_request_result<T>(&mut self, request_id: u64, result: &Result<T, String>) -> bool {
        apply_load_result(
            self.active_request_id,
            &mut self.load_state,
            request_id,
            result.as_ref().map(|_| ()).map_err(Clone::clone),
        )
        .is_some()
    }

    fn apply_tool_call_cmd(
        &mut self,
        request_id: u64,
        session_id: &str,
        tool_call_id: &str,
        load_duration_ms: u128,
        result: Result<Option<ToolCall>, String>,
    ) {
        if !self.accept_request_result(request_id, &result) {
            return;
        }

        let success = result.is_ok();
        let found = result.as_ref().map(|tool| tool.is_some()).unwrap_or(false);
        tracing::info!(
            request_id,
            session_id,
            tool_call_id,
            success,
            found,
            load_duration_ms,
            "Inspector tool call load completed"
        );

        match result {
            Ok(tool_call) => {
                if tool_call.is_none() {
                    tracing::warn!(
                        "Tool call not found: {} in session {}",
                        tool_call_id,
                        session_id
                    );
                }
                self.tool_call = tool_call;
            }
            Err(err) => {
                tracing::error!("Failed to load tool call {}: {}", tool_call_id, err);
                self.tool_call = None;
            }
        }
    }

    fn apply_subagent_cmd(
        &mut self,
        request_id: u64,
        session_id: &str,
        subagent_id: &str,
        load_duration_ms: u128,
        subagent_result: Result<Option<Subagent>, String>,
    ) {
        if !self.accept_request_result(request_id, &subagent_result) {
            return;
        }

        let success = subagent_result.is_ok();
        let found = subagent_result
            .as_ref()
            .map(|subagent| subagent.is_some())
            .unwrap_or(false);
        tracing::info!(
            request_id,
            session_id,
            subagent_id,
            success,
            found,
            load_duration_ms,
            "Inspector subagent load completed"
        );

        match subagent_result {
            Ok(subagent) => {
                if subagent.is_none() {
                    tracing::warn!(
                        "Subagent not found: {} in session {}",
                        subagent_id,
                        session_id
                    );
                }
                self.subagent = subagent;
            }
            Err(err) => {
                tracing::error!("Failed to load subagent {}: {}", subagent_id, err);
                self.subagent = None;
            }
        }
    }

    fn apply_reasoning_cmd(
        &mut self,
        request_id: u64,
        load_duration_ms: u128,
        result: Result<Option<ReasoningAttachment>, String>,
    ) {
        if !self.accept_request_result(request_id, &result) {
            return;
        }

        let success = result.is_ok();
        let found = result
            .as_ref()
            .map(|reasoning| reasoning.is_some())
            .unwrap_or(false);
        tracing::info!(
            request_id,
            success,
            found,
            load_duration_ms,
            "Inspector reasoning load completed"
        );

        match result {
            Ok(attachment) => {
                self.reasoning = attachment;
            }
            Err(err) => {
                tracing::error!("Failed to load reasoning attachment: {}", err);
                self.reasoning = None;
            }
        }
    }

    fn visible_page_name(&self) -> &'static str {
        match &self.load_state {
            LoadState::Loading => "loading",
            LoadState::LoadError(message) => {
                self.error_label.set_label(message);
                "error"
            }
            LoadState::Idle | LoadState::Ready => match &self.selection {
                InspectorSelection::None => "empty",
                InspectorSelection::ToolCall { .. } if self.tool_call.is_some() => "tool",
                InspectorSelection::Subagent { .. } if self.subagent.is_some() => "subagent",
                InspectorSelection::Reasoning { .. } if self.reasoning.is_some() => "reasoning",
                _ => "empty",
            },
        }
    }

    fn render_tool_call_section(&self) {
        if let Some(tool_call) = self.tool_call.as_ref() {
            apply_tool_detail_views(
                &self.tool_name_label,
                &self.tool_status_label,
                &self.tool_metadata_label,
                &self.tool_error_views,
                &self.tool_renderer_views,
                tool_call,
            );
        }
    }

    fn render_subagent_section(&self) {
        if let Some(subagent) = self.subagent.as_ref() {
            self.subagent_title_label.set_label(&subagent.title);
            apply_optional_markdown_section(
                &self.subagent_prompt_views,
                subagent.prompt.as_deref(),
            );
            apply_optional_markdown_section(
                &self.subagent_result_views,
                subagent.result_summary.as_deref(),
            );
            self.open_session_button
                .set_visible(subagent.child_session_id.is_some());
        }
    }

    fn render_reasoning_section(&self) {
        if let Some(reasoning) = self.reasoning.as_ref() {
            self.reasoning_title_label.set_label(&format!(
                "Transcript item {}",
                reasoning.transcript_item_index
            ));

            let metadata_line = format_reasoning_metadata_line(reasoning);
            apply_optional_line(&self.reasoning_metadata_label, metadata_line.as_deref());
            apply_optional_markdown_section(
                &self.reasoning_visible_views,
                reasoning.visible_text.as_deref(),
            );
            apply_optional_markdown_section(
                &self.reasoning_summary_views,
                reasoning.summary_text.as_deref(),
            );
        }
    }
}

// ── Widget helpers ────────────────────────────────────────────────────────────

fn build_overview_stack(sender: &ComponentSender<ToolInspectorPane>) -> OverviewStackViews {
    let content_stack = gtk::Stack::new();
    content_stack.set_transition_type(gtk::StackTransitionType::None);
    content_stack.set_vexpand(true);

    content_stack.add_named(&build_empty_state_box(), Some("empty"));
    content_stack.add_named(&build_loading_state_box(), Some("loading"));

    let (error_box, error_label) = build_error_state_box();
    content_stack.add_named(&error_box, Some("error"));

    let tool_detail = build_tool_detail_views();
    let tool_scroll = build_tool_detail_page(&tool_detail);
    content_stack.add_named(&tool_scroll, Some("tool"));

    let subagent_detail = build_subagent_detail_views(sender);
    let subagent_scroll = build_subagent_detail_page(&subagent_detail);
    content_stack.add_named(&subagent_scroll, Some("subagent"));

    let reasoning_detail = build_reasoning_detail_views();
    let reasoning_scroll = build_reasoning_detail_page(&reasoning_detail);
    content_stack.add_named(&reasoning_scroll, Some("reasoning"));

    OverviewStackViews {
        content_stack,
        error_label,
        tool_detail,
        tool_scroll,
        subagent_detail,
        subagent_scroll,
        reasoning_detail,
        reasoning_scroll,
    }
}

fn build_empty_state_box() -> gtk::Box {
    let empty_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    empty_box.set_halign(gtk::Align::Center);
    empty_box.set_valign(gtk::Align::Center);
    empty_box.set_margin_all(24);

    let empty_icon = gtk::Image::from_icon_name("system-search-symbolic");
    empty_icon.set_pixel_size(48);
    empty_icon.add_css_class("dim-label");
    empty_box.append(&empty_icon);

    let empty_label = gtk::Label::new(Some("Select a tool call or subagent to inspect"));
    empty_label.add_css_class("dim-label");
    empty_label.set_wrap(true);
    empty_label.set_justify(gtk::Justification::Center);
    empty_box.append(&empty_label);

    empty_box
}

fn build_loading_state_box() -> gtk::Box {
    let loading_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    loading_box.set_halign(gtk::Align::Center);
    loading_box.set_valign(gtk::Align::Center);
    loading_box.set_margin_all(24);

    let spinner = gtk::Spinner::new();
    spinner.start();
    loading_box.append(&spinner);

    let loading_label = gtk::Label::new(Some("Loading inspector details..."));
    loading_label.add_css_class("dim-label");
    loading_box.append(&loading_label);

    loading_box
}

fn build_error_state_box() -> (gtk::Box, gtk::Label) {
    let error_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    error_box.set_halign(gtk::Align::Center);
    error_box.set_valign(gtk::Align::Center);
    error_box.set_margin_all(24);

    let error_title = gtk::Label::new(Some("Failed to load inspector details"));
    error_title.add_css_class("heading");
    error_box.append(&error_title);

    let error_label = gtk::Label::new(None);
    error_label.add_css_class("dim-label");
    error_label.set_wrap(true);
    error_label.set_justify(gtk::Justification::Center);
    error_box.append(&error_label);

    (error_box, error_label)
}

fn build_tool_detail_views() -> ToolDetailViews {
    let name_label = make_title_label();
    name_label.add_css_class("monospace");

    let status_label = make_caption_label();
    let metadata_label = make_metadata_label();
    let error_views = make_text_section("Error");
    error_views.section.add_css_class("inspector-error-section");
    error_views.label.add_css_class("inspector-error-text");

    ToolDetailViews {
        name_label,
        status_label,
        metadata_label,
        error_views,
        renderer_views: make_renderer_stack_views(),
    }
}

fn build_tool_detail_page(views: &ToolDetailViews) -> gtk::ScrolledWindow {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
    outer.set_margin_all(16);
    outer.append(&views.name_label);
    outer.append(&views.status_label);
    outer.append(&views.metadata_label);
    outer.append(&views.error_views.section);
    outer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    outer.append(&views.renderer_views.stack);

    wrap_in_scrolled_window(&outer)
}

fn build_subagent_detail_views(sender: &ComponentSender<ToolInspectorPane>) -> SubagentDetailViews {
    let title_label = make_title_label();
    let prompt_views = make_markdown_section("Prompt");
    let result_views = make_markdown_section("Result");

    let open_session_button = gtk::Button::with_label("Open Full Session");
    open_session_button.add_css_class("suggested-action");
    {
        let sender = sender.clone();
        open_session_button
            .connect_clicked(move |_| sender.input(ToolInspectorPaneMsg::OpenChildSession));
    }

    SubagentDetailViews {
        title_label,
        prompt_views,
        result_views,
        open_session_button,
    }
}

fn build_subagent_detail_page(views: &SubagentDetailViews) -> gtk::ScrolledWindow {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
    outer.set_margin_all(16);
    outer.append(&views.title_label);
    outer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    outer.append(&views.prompt_views.section);
    outer.append(&views.result_views.section);
    outer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    outer.append(&views.open_session_button);

    wrap_in_scrolled_window(&outer)
}

fn build_reasoning_detail_views() -> ReasoningDetailViews {
    ReasoningDetailViews {
        title_label: make_title_label(),
        metadata_label: make_metadata_label(),
        visible_views: make_markdown_section("Thinking"),
        summary_views: make_markdown_section("Summary"),
    }
}

fn build_reasoning_detail_page(views: &ReasoningDetailViews) -> gtk::ScrolledWindow {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
    outer.set_margin_all(16);
    outer.append(&views.title_label);
    outer.append(&views.metadata_label);
    outer.append(&views.visible_views.section);
    outer.append(&views.summary_views.section);

    wrap_in_scrolled_window(&outer)
}

fn build_tool_inspector_model(
    db_path: Arc<PathBuf>,
    overview_views: OverviewStackViews,
) -> ToolInspectorPane {
    ToolInspectorPane {
        db_path,
        selection: InspectorSelection::None,
        load_state: LoadState::Idle,
        active_request_id: 0,
        tool_call: None,
        subagent: None,
        reasoning: None,
        content_stack: overview_views.content_stack,
        error_label: overview_views.error_label,
        tool_name_label: overview_views.tool_detail.name_label,
        tool_status_label: overview_views.tool_detail.status_label,
        tool_metadata_label: overview_views.tool_detail.metadata_label,
        tool_error_views: overview_views.tool_detail.error_views,
        tool_renderer_views: overview_views.tool_detail.renderer_views,
        tool_scroll: overview_views.tool_scroll,
        subagent_title_label: overview_views.subagent_detail.title_label,
        subagent_prompt_views: overview_views.subagent_detail.prompt_views,
        subagent_result_views: overview_views.subagent_detail.result_views,
        open_session_button: overview_views.subagent_detail.open_session_button,
        subagent_scroll: overview_views.subagent_scroll,
        reasoning_title_label: overview_views.reasoning_detail.title_label,
        reasoning_metadata_label: overview_views.reasoning_detail.metadata_label,
        reasoning_visible_views: overview_views.reasoning_detail.visible_views,
        reasoning_summary_views: overview_views.reasoning_detail.summary_views,
        reasoning_scroll: overview_views.reasoning_scroll,
    }
}

fn wrap_in_scrolled_window(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_hscrollbar_policy(gtk::PolicyType::Never);
    scrolled.set_child(Some(child));
    scrolled
}

fn reset_scroll_position(scrolled: &gtk::ScrolledWindow) {
    let adjustment = scrolled.vadjustment();
    adjustment.set_value(adjustment.lower());
}

fn make_title_label() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.add_css_class("title-3");
    label.set_halign(gtk::Align::Start);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
    label
}

fn make_caption_label() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.add_css_class("dim-label");
    label.add_css_class("caption");
    label.set_halign(gtk::Align::Start);
    label
}

fn make_metadata_label() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.add_css_class("dim-label");
    label.add_css_class("caption");
    label.add_css_class("inspector-metadata-line");
    label.set_halign(gtk::Align::Start);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_visible(false);
    label
}

fn make_renderer_stack_views() -> RendererStackViews {
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::None);

    let generic_container = make_renderer_container();
    stack.add_named(
        &make_renderer_page(&generic_container),
        Some(RendererKind::Generic.as_str()),
    );

    let terminal_container = make_renderer_container();
    stack.add_named(
        &make_renderer_page(&terminal_container),
        Some(RendererKind::Terminal.as_str()),
    );

    let diff_container = make_renderer_container();
    stack.add_named(
        &make_renderer_page(&diff_container),
        Some(RendererKind::Diff.as_str()),
    );

    let file_container = make_renderer_container();
    stack.add_named(
        &make_renderer_page(&file_container),
        Some(RendererKind::File.as_str()),
    );

    let results_container = make_renderer_container();
    stack.add_named(
        &make_renderer_page(&results_container),
        Some(RendererKind::Results.as_str()),
    );

    let subagent_container = make_renderer_container();
    stack.add_named(
        &make_renderer_page(&subagent_container),
        Some(RendererKind::Subagent.as_str()),
    );

    stack.set_visible_child_name(RendererKind::Generic.as_str());

    RendererStackViews {
        stack,
        generic_container,
        terminal_container,
        diff_container,
        file_container,
        results_container,
        subagent_container,
    }
}

fn make_renderer_page(content: &gtk::Box) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.append(content);
    page
}

fn make_renderer_container() -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    container.set_margin_all(12);
    container
}

fn make_mono_label() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.add_css_class("monospace");
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_halign(gtk::Align::Start);
    label.set_xalign(0.0);
    label.set_selectable(true);
    label
}

fn make_text_section(title: &str) -> TextSectionViews {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 4);
    section.set_valign(gtk::Align::Start);

    let header = gtk::Label::new(Some(title));
    header.add_css_class("inspector-section-heading");
    header.set_halign(gtk::Align::Start);

    let label = make_mono_label();
    label.add_css_class("inspector-code-block");

    section.append(&header);
    section.append(&label);
    section.set_visible(false);

    TextSectionViews { section, label }
}

fn make_markdown_section(title: &str) -> MarkdownSectionViews {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 4);
    section.set_valign(gtk::Align::Start);
    let header = gtk::Label::new(Some(title));
    header.add_css_class("inspector-section-heading");
    header.set_halign(gtk::Align::Start);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.set_hexpand(true);
    content.set_valign(gtk::Align::Start);

    section.append(&header);
    section.append(&content);
    section.set_visible(false);

    MarkdownSectionViews { section, content }
}

fn apply_optional_markdown_section(views: &MarkdownSectionViews, text: Option<&str>) {
    clear_container(&views.content);
    match text {
        Some(value) if !value.is_empty() => {
            let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 0);
            wrapper.add_css_class("inspector-markdown-block");
            wrapper.set_valign(gtk::Align::Start);
            let (markdown_view, _) = markdown::render_markdown(value, None);
            markdown_view.set_valign(gtk::Align::Start);
            markdown_view.set_vexpand(false);
            wrapper.append(&markdown_view);
            views.content.append(&wrapper);
            views.section.set_visible(true);
        }
        _ => views.section.set_visible(false),
    }
}

fn apply_optional_text_section(views: &TextSectionViews, text: Option<&str>) {
    match text {
        Some(value) if !value.is_empty() => {
            views.label.set_label(value);
            views.section.set_visible(true);
        }
        _ => views.section.set_visible(false),
    }
}

fn apply_optional_line(label: &gtk::Label, text: Option<&str>) {
    match text {
        Some(value) if !value.is_empty() => {
            label.set_label(value);
            label.set_visible(true);
        }
        _ => label.set_visible(false),
    }
}

fn apply_tool_detail_views(
    name_label: &gtk::Label,
    status_label: &gtk::Label,
    metadata_label: &gtk::Label,
    error_views: &TextSectionViews,
    renderer_views: &RendererStackViews,
    tool_call: &ToolCall,
) {
    name_label.set_label(&tool_call.tool_name);
    status_label.set_label(&format_status_duration(
        tool_call.status,
        tool_call.duration_ms,
    ));
    let metadata_line = format_tool_metadata_line(tool_call);
    apply_optional_line(metadata_label, metadata_line.as_deref());
    let error_text = tool_error_message(tool_call);
    apply_optional_text_section(error_views, error_text);
    apply_renderer_stack(renderer_views, tool_call);
}

fn apply_renderer_stack(views: &RendererStackViews, tool_call: &ToolCall) {
    let started_at = Instant::now();
    let init = renderer_init_from_tool_call(tool_call);
    let renderer_kind = resolve_renderer(&init.tool_name);
    views.stack.set_visible_child_name(renderer_kind.as_str());

    match renderer_kind {
        RendererKind::Terminal => {
            let rendered = TerminalRenderer::new(init).render_data();
            let widget = build_terminal_widget(&rendered);
            clear_container(&views.terminal_container);
            views.terminal_container.append(&widget);
        }
        RendererKind::Diff => {
            let rendered = DiffRenderer::new(init).render_data();
            let widget = build_diff_widget(&rendered);
            clear_container(&views.diff_container);
            views.diff_container.append(&widget);
        }
        RendererKind::File => {
            let rendered = FileRenderer::new(init).render_data();
            let widget = build_file_widget(&rendered);
            clear_container(&views.file_container);
            views.file_container.append(&widget);
        }
        RendererKind::Results => {
            let rendered = ResultsRenderer::new(init).render_data();
            let widget = build_results_widget(&rendered);
            clear_container(&views.results_container);
            views.results_container.append(&widget);
        }
        RendererKind::Subagent => {
            let rendered = SubagentRenderer::new(init).render_data();
            let widget = build_subagent_widget(&rendered);
            clear_container(&views.subagent_container);
            views.subagent_container.append(&widget);
        }
        RendererKind::Generic => {
            let rendered = GenericRenderer::new(init).render_data();
            let widget = build_generic_widget(&rendered);
            clear_container(&views.generic_container);
            views.generic_container.append(&widget);
        }
    }

    tracing::debug!(
        renderer_kind = renderer_kind.as_str(),
        duration_ms = started_at.elapsed().as_millis(),
        "Rendered inspector tool renderer"
    );
}

fn clear_container(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn renderer_init_from_tool_call(tool_call: &ToolCall) -> RendererInit {
    RendererInit {
        tool_name: tool_call.tool_name.clone(),
        input_json: tool_call.input_json.clone(),
        output_text: tool_call.output_text.clone(),
        error_text: tool_call.error_text.clone(),
        status: tool_call.status,
        duration_ms: tool_call.duration_ms,
    }
}

// ── Widget builders ───────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum InspectorTextViewStyle {
    ExpandingCode,
    Code,
    ExpandingText,
    Text,
}

fn make_inspector_text_view(text: &str, style: InspectorTextViewStyle) -> gtk::TextView {
    let (monospace, vexpand) = match style {
        InspectorTextViewStyle::ExpandingCode => (true, true),
        InspectorTextViewStyle::Code => (true, false),
        InspectorTextViewStyle::ExpandingText => (false, true),
        InspectorTextViewStyle::Text => (false, false),
    };

    let view = gtk::TextView::new();
    view.buffer().set_text(text);
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.set_monospace(monospace);
    view.add_css_class("inspector-code-block");
    view.set_vexpand(vexpand);
    view
}

fn append_inspector_text_section(
    container: &gtk::Box,
    title: &str,
    text: &str,
    style: InspectorTextViewStyle,
) {
    let header = gtk::Label::new(Some(title));
    header.add_css_class("inspector-section-heading");
    header.set_halign(gtk::Align::Start);
    container.append(&header);
    container.append(&make_inspector_text_view(text, style));
}

fn build_generic_widget(
    rendered: &crate::ui::tool_renderers::generic::GenericRenderedData,
) -> gtk::Widget {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let has_input = rendered.input_text.as_deref().is_some();

    if let Some(input) = rendered.input_text.as_deref() {
        append_inspector_text_section(
            &container,
            "Input",
            input,
            InspectorTextViewStyle::ExpandingCode,
        );
    }

    if let Some(output) = rendered.output.as_ref() {
        let header = gtk::Label::new(Some("Output"));
        header.add_css_class("inspector-section-heading");
        header.set_halign(gtk::Align::Start);
        if has_input {
            header.set_margin_top(4);
        }
        container.append(&header);

        container.append(&build_output_render_plan_widget(output));
    }

    container.upcast()
}

fn build_output_render_plan_widget(
    output: &crate::ui::tool_renderers::generic::OutputRenderPlan,
) -> gtk::Widget {
    match output {
        crate::ui::tool_renderers::generic::OutputRenderPlan::PrettyJson(text) => {
            make_inspector_text_view(text, InspectorTextViewStyle::ExpandingCode).upcast()
        }
        crate::ui::tool_renderers::generic::OutputRenderPlan::Markdown(text) => {
            let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 0);
            wrapper.add_css_class("inspector-markdown-block");
            let (markdown_view, _) = markdown::render_markdown(text, None);
            wrapper.append(&markdown_view);
            wrapper.upcast()
        }
    }
}

fn build_terminal_widget(
    rendered: &crate::ui::tool_renderers::terminal::TerminalRenderedData,
) -> gtk::Widget {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let has_command = rendered.command.as_deref().is_some();

    if let Some(command) = rendered.command.as_deref() {
        let header = gtk::Label::new(Some("Command"));
        header.add_css_class("inspector-section-heading");
        header.set_halign(gtk::Align::Start);
        container.append(&header);

        let command_label = gtk::Label::new(Some(&format!("$ {}", command)));
        command_label.add_css_class("terminal-command");
        command_label.add_css_class("monospace");
        command_label.set_halign(gtk::Align::Start);
        command_label.set_wrap(true);
        command_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        command_label.set_xalign(0.0);
        command_label.set_selectable(true);
        container.append(&command_label);
    }

    if let Some(output) = rendered.output_text.as_deref().filter(|t| !t.is_empty()) {
        let header = gtk::Label::new(Some("Output"));
        header.add_css_class("inspector-section-heading");
        header.set_halign(gtk::Align::Start);
        if has_command {
            header.set_margin_top(4);
        }
        container.append(&header);

        let output_view = make_inspector_text_view(output, InspectorTextViewStyle::ExpandingCode);
        output_view.add_css_class("terminal-output");
        container.append(&output_view);
    }

    if rendered.is_non_zero_exit
        && let Some(code) = rendered.exit_code
    {
        let exit_label = gtk::Label::new(Some(&format!("Exit code: {}", code)));
        exit_label.add_css_class("terminal-exit-nonzero");
        exit_label.set_halign(gtk::Align::Start);
        container.append(&exit_label);
    }

    container.upcast()
}

fn build_diff_widget(rendered: &crate::ui::tool_renderers::diff::DiffRenderedData) -> gtk::Widget {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 4);

    if rendered.hunks.is_empty() {
        let empty_label = gtk::Label::new(Some("No diff content available."));
        empty_label.add_css_class("dim-label");
        empty_label.set_halign(gtk::Align::Center);
        empty_label.set_margin_all(24);
        container.append(&empty_label);
        return container.upcast();
    }

    for hunk in &rendered.hunks {
        let hunk_box = gtk::Box::new(gtk::Orientation::Vertical, 0);

        let header = gtk::Label::new(Some(&hunk.header));
        header.add_css_class("diff-hunk-header");
        header.set_halign(gtk::Align::Start);
        header.set_xalign(0.0);
        header.set_wrap(true);
        header.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        header.set_selectable(true);
        hunk_box.append(&header);

        for line in &hunk.lines {
            let line_label = gtk::Label::new(Some(&line.text));
            line_label.set_halign(gtk::Align::Start);
            line_label.set_xalign(0.0);
            line_label.set_wrap(true);
            line_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            line_label.set_selectable(true);
            line_label.add_css_class("monospace");

            match line.kind {
                crate::ui::tool_renderers::diff::DiffLineKind::Add => {
                    line_label.add_css_class("diff-added");
                }
                crate::ui::tool_renderers::diff::DiffLineKind::Remove => {
                    line_label.add_css_class("diff-removed");
                }
                crate::ui::tool_renderers::diff::DiffLineKind::Context => {
                    line_label.add_css_class("diff-context");
                }
            }

            hunk_box.append(&line_label);
        }

        container.append(&hunk_box);
    }

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&container));
    scroll.set_vexpand(true);
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.upcast()
}

fn build_file_widget(rendered: &crate::ui::tool_renderers::file::FileRenderedData) -> gtk::Widget {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 8);

    if let Some(header) = rendered.header.as_deref() {
        let header_label = gtk::Label::new(Some(header));
        header_label.add_css_class("file-header");
        header_label.set_halign(gtk::Align::Start);
        header_label.set_xalign(0.0);
        header_label.set_wrap(true);
        header_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        header_label.set_selectable(true);
        container.append(&header_label);
    }

    if let Some(output) = rendered.output_text.as_deref() {
        let content = make_inspector_text_view(output, InspectorTextViewStyle::ExpandingCode);
        container.append(&content);
    }

    if rendered.output_text.is_none() && rendered.header.is_none() {
        let empty_label = gtk::Label::new(Some("No file content available."));
        empty_label.add_css_class("dim-label");
        empty_label.set_halign(gtk::Align::Center);
        empty_label.set_margin_all(24);
        container.append(&empty_label);
    }

    container.upcast()
}

fn build_results_widget(
    rendered: &crate::ui::tool_renderers::results::ResultsRenderedData,
) -> gtk::Widget {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 6);
    container.set_margin_top(8);
    container.set_margin_bottom(8);

    if !rendered.entries.is_empty() {
        for entry in &rendered.entries {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);

            let path_label = gtk::Label::new(Some(&entry.path));
            path_label.add_css_class("monospace");
            path_label.set_halign(gtk::Align::Start);
            path_label.set_xalign(0.0);
            path_label.set_wrap(true);
            path_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            row.append(&path_label);

            if let Some(line_num) = entry.line {
                let line_label = gtk::Label::new(Some(&format!(":{}", line_num)));
                line_label.add_css_class("monospace");
                line_label.add_css_class("dim-label");
                row.append(&line_label);
            }

            if !entry.content.is_empty() {
                let content_label = gtk::Label::new(Some(&format!("  {}", entry.content)));
                content_label.set_halign(gtk::Align::Start);
                content_label.set_xalign(0.0);
                content_label.set_hexpand(true);
                content_label.set_wrap(true);
                content_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                row.append(&content_label);
            }

            container.append(&row);
        }
    } else if let Some(output) = rendered.output_text.as_deref() {
        let output_view = make_inspector_text_view(output, InspectorTextViewStyle::ExpandingText);
        container.append(&output_view);
    } else {
        let empty_label = gtk::Label::new(Some("No results available."));
        empty_label.add_css_class("dim-label");
        empty_label.set_halign(gtk::Align::Center);
        empty_label.set_margin_all(24);
        container.append(&empty_label);
    }

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&container));
    scroll.set_vexpand(true);
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.upcast()
}

fn build_subagent_widget(
    rendered: &crate::ui::tool_renderers::subagent::SubagentRenderedData,
) -> gtk::Widget {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 8);

    if let Some(input) = rendered.input_text.as_deref() {
        append_inspector_text_section(&container, "Input", input, InspectorTextViewStyle::Code);
    }

    if let Some(result) = rendered.result_text.as_deref() {
        append_inspector_text_section(&container, "Result", result, InspectorTextViewStyle::Text);
    }

    if container.first_child().is_none() {
        let empty_label = gtk::Label::new(Some(
            "Subagent details are available in the dedicated subagent inspector view.",
        ));
        empty_label.add_css_class("dim-label");
        empty_label.set_halign(gtk::Align::Center);
        empty_label.set_margin_all(24);
        container.append(&empty_label);
    }

    container.upcast()
}

fn begin_loading_request(active_request_id: &mut u64, load_state: &mut LoadState) -> u64 {
    *active_request_id = active_request_id.saturating_add(1);
    *load_state = LoadState::Loading;
    *active_request_id
}

fn clear_active_request(active_request_id: &mut u64, load_state: &mut LoadState) {
    *active_request_id = active_request_id.saturating_add(1);
    *load_state = LoadState::Idle;
}

fn apply_load_result(
    active_request_id: u64,
    load_state: &mut LoadState,
    request_id: u64,
    result: Result<(), String>,
) -> Option<()> {
    if request_id != active_request_id {
        return None;
    }

    *load_state = match result {
        Ok(()) => LoadState::Ready,
        Err(message) => LoadState::LoadError(message),
    };
    Some(())
}

// ── Formatting helpers ────────────────────────────────────────────────────────

fn format_status_duration(status: ToolCallStatus, duration_ms: Option<i64>) -> String {
    let status_str = match status {
        ToolCallStatus::Completed => "✓ Completed",
        ToolCallStatus::Error => "✗ Error",
        ToolCallStatus::Running => "⟳ Running",
        ToolCallStatus::Pending => "… Pending",
        ToolCallStatus::Unknown => "? Unknown",
    };
    match duration_ms {
        Some(ms) if ms > 0 => format!("{}  •  {}", status_str, format_duration_ms(ms)),
        _ => status_str.to_string(),
    }
}

fn format_tool_metadata_line(tool_call: &ToolCall) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(parser_call_id) = tool_call
        .parser_call_id
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        parts.push(format!("Call ID: {parser_call_id}"));
    }

    if let Some(started) = tool_call.started_at.and_then(format_unix_timestamp) {
        parts.push(format!("Start: {started}"));
    }

    if let Some(ended) = tool_call.ended_at.and_then(format_unix_timestamp) {
        parts.push(format!("End: {ended}"));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("  |  "))
    }
}

fn format_reasoning_metadata_line(reasoning: &ReasoningAttachment) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(model) = reasoning
        .source_model
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        parts.push(format!("Model: {model}"));
    }

    if let Some(ts) = reasoning
        .source_timestamp
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
    {
        parts.push(format!("Time: {ts}"));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("  |  "))
    }
}

fn tool_error_message(tool_call: &ToolCall) -> Option<&str> {
    tool_call
        .error_text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .or((tool_call.status == ToolCallStatus::Error).then_some("Tool reported an error."))
}

fn format_unix_timestamp(timestamp: i64) -> Option<String> {
    chrono::Utc
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ToolCall;

    #[test]
    fn stale_request_results_are_ignored() {
        let mut request_id = 0;
        let mut state = LoadState::Idle;

        let first = begin_loading_request(&mut request_id, &mut state);
        let second = begin_loading_request(&mut request_id, &mut state);

        assert!(apply_load_result(request_id, &mut state, first, Ok(())).is_none());
        assert_eq!(state, LoadState::Loading);

        assert!(apply_load_result(request_id, &mut state, second, Ok(())).is_some());
        assert_eq!(state, LoadState::Ready);
    }

    #[test]
    fn load_state_transitions_idle_loading_ready() {
        let mut request_id = 0;
        let mut state = LoadState::Idle;

        let current = begin_loading_request(&mut request_id, &mut state);
        assert_eq!(state, LoadState::Loading);

        let transition = apply_load_result(request_id, &mut state, current, Ok(()));
        assert!(transition.is_some());
        assert_eq!(state, LoadState::Ready);
    }

    #[test]
    fn clear_invalidates_in_flight_request_results() {
        let mut request_id = 0;
        let mut state = LoadState::Idle;

        let in_flight = begin_loading_request(&mut request_id, &mut state);
        clear_active_request(&mut request_id, &mut state);

        assert!(apply_load_result(request_id, &mut state, in_flight, Ok(())).is_none());
        assert_eq!(state, LoadState::Idle);
    }

    #[test]
    fn inspector_tool_call_command_debug_includes_load_duration() {
        let cmd = ToolInspectorPaneCmd::ToolCall {
            request_id: 1,
            session_id: "session-1".to_string(),
            tool_call_id: "call-1".to_string(),
            load_duration_ms: 9,
            result: Ok(None),
        };

        let debug = format!("{cmd:?}");
        assert!(debug.contains("load_duration_ms"));
        assert!(debug.contains("9"));
    }

    #[test]
    fn format_status_duration_keeps_error_label_with_duration() {
        let text = format_status_duration(ToolCallStatus::Error, Some(1200));
        assert!(text.contains("Error"));
        assert!(text.contains("1.2s"));
    }

    #[test]
    fn metadata_line_includes_call_id_only() {
        let mut tool_call = sample_tool_call(ToolCallStatus::Completed);
        tool_call.parser_call_id = Some("call-123".to_string());

        let line = format_tool_metadata_line(&tool_call);
        assert_eq!(line.as_deref(), Some("Call ID: call-123"));
    }

    #[test]
    fn metadata_line_includes_timestamps_only() {
        let mut tool_call = sample_tool_call(ToolCallStatus::Completed);
        tool_call.started_at = Some(0);
        tool_call.ended_at = Some(1);

        let line = format_tool_metadata_line(&tool_call);
        assert_eq!(
            line.as_deref(),
            Some("Start: 1970-01-01 00:00:00 UTC  |  End: 1970-01-01 00:00:01 UTC")
        );
    }

    #[test]
    fn metadata_line_includes_call_id_and_timestamps() {
        let mut tool_call = sample_tool_call(ToolCallStatus::Completed);
        tool_call.parser_call_id = Some("call-xyz".to_string());
        tool_call.started_at = Some(0);
        tool_call.ended_at = Some(1);

        let line = format_tool_metadata_line(&tool_call);
        assert_eq!(
            line.as_deref(),
            Some(
                "Call ID: call-xyz  |  Start: 1970-01-01 00:00:00 UTC  |  End: 1970-01-01 00:00:01 UTC"
            )
        );
    }

    #[test]
    fn metadata_line_omits_empty_values() {
        let tool_call = sample_tool_call(ToolCallStatus::Completed);
        assert_eq!(format_tool_metadata_line(&tool_call), None);
    }

    #[test]
    fn error_message_falls_back_for_error_status_without_text() {
        let mut missing = sample_tool_call(ToolCallStatus::Error);
        missing.error_text = None;
        assert_eq!(
            tool_error_message(&missing),
            Some("Tool reported an error.")
        );

        let mut blank = sample_tool_call(ToolCallStatus::Error);
        blank.error_text = Some("   ".to_string());
        assert_eq!(tool_error_message(&blank), Some("Tool reported an error."));
    }

    #[gtk::test]
    fn markdown_sections_pin_content_to_top() {
        let views = make_markdown_section("Error");

        assert_eq!(views.section.valign(), gtk::Align::Start);
        assert_eq!(views.content.valign(), gtk::Align::Start);
    }

    #[gtk::test]
    fn markdown_sections_wrap_rendered_content_in_styled_child() {
        let views = make_markdown_section("Error");
        apply_optional_markdown_section(&views, Some("permission denied while opening the file"));

        assert!(!views.content.has_css_class("inspector-markdown-block"));

        let wrapper = views
            .content
            .first_child()
            .and_then(|child| child.downcast::<gtk::Box>().ok())
            .expect("markdown section should wrap rendered content in a box");
        assert!(wrapper.has_css_class("inspector-markdown-block"));
    }

    #[gtk::test]
    fn text_sections_use_selectable_labels() {
        let views = make_text_section("Error");
        apply_optional_text_section(&views, Some("permission denied while opening the file"));

        assert!(views.section.is_visible());
        assert_eq!(
            views.label.label(),
            "permission denied while opening the file"
        );
        assert!(views.label.is_selectable());
    }

    #[gtk::test]
    fn inspector_text_view_preserves_shared_properties_and_styles() {
        let cases = [
            (InspectorTextViewStyle::ExpandingCode, true, true),
            (InspectorTextViewStyle::Code, true, false),
            (InspectorTextViewStyle::ExpandingText, false, true),
            (InspectorTextViewStyle::Text, false, false),
        ];

        for (style, monospace, vexpand) in cases {
            let view = make_inspector_text_view("sample output", style);
            let buffer = view.buffer();

            assert_eq!(
                buffer.text(&buffer.start_iter(), &buffer.end_iter(), false),
                "sample output"
            );
            assert!(!view.is_editable());
            assert!(!view.is_cursor_visible());
            assert_eq!(view.wrap_mode(), gtk::WrapMode::WordChar);
            assert_eq!(view.is_monospace(), monospace);
            assert_eq!(view.vexpands(), vexpand);
            assert!(view.has_css_class("inspector-code-block"));
        }
    }

    #[gtk::test]
    fn inspector_text_section_appends_heading_and_configured_view() {
        let container = gtk::Box::new(gtk::Orientation::Vertical, 8);
        append_inspector_text_section(
            &container,
            "Input",
            "{\"path\":\"README.md\"}",
            InspectorTextViewStyle::ExpandingCode,
        );

        let header = container
            .first_child()
            .and_then(|child| child.downcast::<gtk::Label>().ok())
            .expect("section heading");
        assert_eq!(header.label(), "Input");
        assert_eq!(header.halign(), gtk::Align::Start);
        assert!(header.has_css_class("inspector-section-heading"));

        let input = header
            .next_sibling()
            .and_then(|child| child.downcast::<gtk::TextView>().ok())
            .expect("input text view");
        assert!(input.is_monospace());
        assert!(input.vexpands());
    }

    #[gtk::test]
    fn terminal_output_keeps_its_specific_css_class() {
        use crate::models::ToolCallStatus;
        use crate::ui::tool_renderers::terminal::TerminalRenderedData;

        let rendered = TerminalRenderedData {
            command: None,
            output_text: Some("done".to_string()),
            error_text: None,
            display_text: Some("done".to_string()),
            exit_code: None,
            is_non_zero_exit: false,
            status: ToolCallStatus::Completed,
            duration_ms: None,
        };

        let output = build_terminal_widget(&rendered)
            .downcast::<gtk::Box>()
            .expect("terminal container")
            .last_child()
            .and_then(|child| child.downcast::<gtk::TextView>().ok())
            .expect("terminal output text view");
        assert!(output.has_css_class("terminal-output"));
        assert!(output.has_css_class("inspector-code-block"));
    }

    #[gtk::test]
    fn reset_scroll_position_returns_to_adjustment_start() {
        let adjustment = gtk::Adjustment::new(50.0, 10.0, 100.0, 1.0, 10.0, 10.0);
        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_vadjustment(Some(&adjustment));

        reset_scroll_position(&scrolled);

        assert_eq!(adjustment.value(), adjustment.lower());
    }

    #[gtk::test]
    fn diff_hunk_header_wraps_so_it_does_not_lock_a_min_width() {
        use crate::ui::tool_renderers::diff::{DiffHunk, DiffRenderedData};

        let rendered = DiffRenderedData {
            old_text: None,
            new_text: None,
            hunks: vec![DiffHunk {
                header: "@@ -1,4 +1,4 @@ fn an_extremely_long_function_signature_here()"
                    .to_string(),
                lines: Vec::new(),
            }],
        };

        let widget = build_diff_widget(&rendered);
        let container = widget
            .downcast::<gtk::ScrolledWindow>()
            .expect("diff scroll")
            .child()
            .expect("diff viewport")
            .downcast::<gtk::Viewport>()
            .expect("diff viewport")
            .child()
            .expect("diff container")
            .downcast::<gtk::Box>()
            .expect("diff container box");
        let hunk_box = container
            .first_child()
            .expect("hunk box")
            .downcast::<gtk::Box>()
            .expect("hunk box");
        let header = hunk_box
            .first_child()
            .expect("hunk header")
            .downcast::<gtk::Label>()
            .expect("hunk header label");

        assert!(header.wraps(), "diff hunk header must wrap");
    }

    #[gtk::test]
    fn file_header_wraps_so_it_does_not_lock_a_min_width() {
        use crate::models::ToolCallStatus;
        use crate::ui::tool_renderers::file::FileRenderedData;

        let rendered = FileRenderedData {
            header: Some("/an/extremely/long/absolute/path/to/some/source/file.rs".to_string()),
            output_text: None,
            error_text: None,
            status: ToolCallStatus::Completed,
            duration_ms: None,
        };

        let widget = build_file_widget(&rendered);
        let header = widget
            .downcast::<gtk::Box>()
            .expect("file container box")
            .first_child()
            .expect("file header")
            .downcast::<gtk::Label>()
            .expect("file header label");

        assert!(header.wraps(), "file header must wrap");
    }

    #[gtk::test]
    fn results_entry_path_wraps_so_it_does_not_lock_a_min_width() {
        use crate::models::ToolCallStatus;
        use crate::ui::tool_renderers::results::{ResultsEntry, ResultsRenderedData};

        let rendered = ResultsRenderedData {
            entries: vec![ResultsEntry {
                path: "/an/extremely/long/absolute/path/to/some/matched/file.rs".to_string(),
                line: Some(42),
                content: "let x = 1;".to_string(),
            }],
            output_text: None,
            error_text: None,
            status: ToolCallStatus::Completed,
            duration_ms: None,
        };

        let widget = build_results_widget(&rendered);
        let row = widget
            .downcast::<gtk::ScrolledWindow>()
            .expect("results scroll")
            .child()
            .expect("results viewport")
            .downcast::<gtk::Viewport>()
            .expect("results viewport")
            .child()
            .expect("results container")
            .downcast::<gtk::Box>()
            .expect("results container box")
            .first_child()
            .expect("results row")
            .downcast::<gtk::Box>()
            .expect("results row box");
        let path = row
            .first_child()
            .expect("results path")
            .downcast::<gtk::Label>()
            .expect("results path label");

        assert!(path.wraps(), "results entry path must wrap");
    }

    fn sample_tool_call(status: ToolCallStatus) -> ToolCall {
        ToolCall {
            id: "tool-1".to_string(),
            session_id: "session-1".to_string(),
            subagent_id: None,
            tool_name: "terminal".to_string(),
            status,
            title: None,
            summary: None,
            input_json: None,
            output_text: None,
            error_text: None,
            started_at: None,
            ended_at: None,
            duration_ms: None,
            parser_call_id: None,
        }
    }
}
