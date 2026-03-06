use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;

use adw::prelude::*;
use chrono::TimeZone;
use relm4::{Component, ComponentParts, ComponentSender, RelmWidgetExt, adw, gtk};

use crate::database::{load_subagent, load_tool_call, load_tool_calls_for_subagent};
use crate::models::{Subagent, ToolCall, ToolCallStatus};
use crate::ui::format::{format_duration_ms, status_icon_name};
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
        // Retained for potential future reload.
        #[allow(dead_code)]
        subagent_id: String,
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

// ── Component ─────────────────────────────────────────────────────────────────

pub struct ToolInspectorPane {
    db_path: Arc<PathBuf>,
    selection: InspectorSelection,
    load_state: LoadState,
    active_request_id: u64,
    tool_call: Option<ToolCall>,
    subagent: Option<Subagent>,
    subagent_tools: Vec<ToolCall>,
    drilled_tool: Option<ToolCall>,
    pending_drill_tool_id: Option<String>,

    // Sender for attaching to dynamically-created inner-tool row callbacks.
    sender: ComponentSender<ToolInspectorPane>,

    // Navigation view (managed imperatively; stored for push/pop in post_view).
    nav_view: adw::NavigationView,

    // Overview content switcher: "empty" / "tool" / "subagent"
    content_stack: gtk::Stack,
    error_label: gtk::Label,

    // Tool-call detail widgets (inside "tool" stack page)
    tool_name_label: gtk::Label,
    tool_status_label: gtk::Label,
    tool_metadata_label: gtk::Label,
    tool_error_section: gtk::Box,
    tool_error_label: gtk::Label,
    tool_renderer_views: RendererStackViews,

    // Subagent detail widgets (inside "subagent" stack page)
    subagent_title_label: gtk::Label,
    subagent_prompt_section: gtk::Box,
    subagent_prompt_label: gtk::Label,
    subagent_result_section: gtk::Box,
    subagent_result_label: gtk::Label,
    subagent_tools_list: gtk::ListBox,
    open_session_button: gtk::Button,

    // Drill-down NavigationPage and its content widgets.
    // The page is pushed/popped based on drilled_tool state.
    drill_page: adw::NavigationPage,
    drill_name_label: gtk::Label,
    drill_status_label: gtk::Label,
    drill_metadata_label: gtk::Label,
    drill_error_section: gtk::Box,
    drill_error_label: gtk::Label,
    drill_renderer_views: RendererStackViews,

    // Interior-mutable flag: is drill_page currently pushed onto nav_view?
    drill_page_pushed: Cell<bool>,
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
    Clear,
    /// Drill into an inner tool call from the subagent overview.
    DrillDownTool(String),
    /// Sync model when the native NavigationView back button pops the drill page.
    PopDrillDown,
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
        result: Result<Option<ToolCall>, String>,
    },
    Subagent {
        request_id: u64,
        session_id: String,
        subagent_id: String,
        subagent_result: Result<Option<Subagent>, String>,
        tools_result: Result<Vec<ToolCall>, String>,
    },
    DrillTool {
        session_id: String,
        tool_call_id: String,
        result: Result<Option<ToolCall>, String>,
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
        // ── Navigation view ───────────────────────────────────────────────────
        let nav_view = adw::NavigationView::new();
        nav_view.set_vexpand(true);
        nav_view.set_hexpand(true);
        // `pop_on_escape` is left at its default (true) so that the drill-down
        // page can be dismissed natively via the Escape key.
        //
        // GTK4 event routing: the inner AdwNavigationView uses a widget-scoped
        // GtkShortcutController (GTK_SHORTCUT_SCOPE_MANAGED).  The app-level
        // `EscapeAction` accelerator is registered on the GtkApplication and
        // therefore also has global scope.  When both could fire on the same
        // Escape keypress, GTK resolves the conflict in favour of the more
        // specific, widget-level handler — the inner nav pops and the event is
        // consumed before the window action fires.
        //
        // The app-level Esc handler (`AppMsg::Escape`) therefore only runs when
        // the drill-down page is NOT currently pushed, giving the correct
        // priority chain: drill-down pop → close inspector pane → navigate back.

        // Sync drilled_tool state when the user uses the native back button.
        let popped_sender = sender.input_sender().clone();
        nav_view.connect_popped(move |_, page| {
            if page.tag().as_deref() == Some("drill-down") {
                popped_sender.send(ToolInspectorPaneMsg::PopDrillDown).ok();
            }
        });

        // ── Content stack (overview pages) ────────────────────────────────────
        let content_stack = gtk::Stack::new();
        content_stack.set_transition_type(gtk::StackTransitionType::None);
        content_stack.set_vexpand(true);

        // — Empty state ——————————————————————————————————————————————————————
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
        content_stack.add_named(&empty_box, Some("empty"));

        // — Loading state ————————————————————————————————————————————————————
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
        content_stack.add_named(&loading_box, Some("loading"));

        // — Load error state ——————————————————————————————————————————————————
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
        content_stack.add_named(&error_box, Some("error"));

        // — Tool-call detail ——————————————————————————————————————————————————
        let tool_name_label = make_title_label();
        tool_name_label.add_css_class("monospace");

        let tool_status_label = make_caption_label();
        let tool_metadata_label = make_metadata_label();
        let (tool_error_section, tool_error_label) = make_text_section("Error");
        tool_error_section.add_css_class("inspector-error-section");
        tool_error_label.add_css_class("inspector-error-text");
        let tool_renderer_views = make_renderer_stack_views();

        let tool_outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
        tool_outer.set_margin_all(16);
        tool_outer.append(&tool_name_label);
        tool_outer.append(&tool_status_label);
        tool_outer.append(&tool_metadata_label);
        tool_outer.append(&tool_error_section);
        tool_outer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        tool_outer.append(&tool_renderer_views.stack);

        let tool_scroll = gtk::ScrolledWindow::new();
        tool_scroll.set_vexpand(true);
        tool_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        tool_scroll.set_child(Some(&tool_outer));
        content_stack.add_named(&tool_scroll, Some("tool"));

        // — Subagent detail ———————————————————————————————————————————————————
        let subagent_title_label = make_title_label();

        let (subagent_prompt_section, subagent_prompt_label) = make_text_section("Prompt");
        let (subagent_result_section, subagent_result_label) = make_text_section("Result");

        let inner_tools_header = gtk::Label::new(Some("Inner Tools"));
        inner_tools_header.add_css_class("heading");
        inner_tools_header.set_halign(gtk::Align::Start);

        let subagent_tools_list = gtk::ListBox::new();
        subagent_tools_list.add_css_class("boxed-list");
        subagent_tools_list.set_selection_mode(gtk::SelectionMode::None);

        let open_session_button = gtk::Button::with_label("Open Full Session");
        open_session_button.add_css_class("suggested-action");
        {
            let s = sender.clone();
            open_session_button
                .connect_clicked(move |_| s.input(ToolInspectorPaneMsg::OpenChildSession));
        }

        let subagent_outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
        subagent_outer.set_margin_all(16);
        subagent_outer.append(&subagent_title_label);
        subagent_outer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        subagent_outer.append(&subagent_prompt_section);
        subagent_outer.append(&subagent_result_section);
        subagent_outer.append(&inner_tools_header);
        subagent_outer.append(&subagent_tools_list);
        subagent_outer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        subagent_outer.append(&open_session_button);

        let subagent_scroll = gtk::ScrolledWindow::new();
        subagent_scroll.set_vexpand(true);
        subagent_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        subagent_scroll.set_child(Some(&subagent_outer));
        content_stack.add_named(&subagent_scroll, Some("subagent"));

        // ── Overview NavigationPage ───────────────────────────────────────────
        let overview_page = adw::NavigationPage::builder()
            .title("Inspector")
            .tag("overview")
            .child(&content_stack)
            .build();
        nav_view.add(&overview_page);

        // ── Drill-down NavigationPage ─────────────────────────────────────────
        let drill_name_label = make_title_label();
        drill_name_label.add_css_class("monospace");
        let drill_status_label = make_caption_label();
        let drill_metadata_label = make_metadata_label();
        let (drill_error_section, drill_error_label) = make_text_section("Error");
        drill_error_section.add_css_class("inspector-error-section");
        drill_error_label.add_css_class("inspector-error-text");
        let drill_renderer_views = make_renderer_stack_views();

        let drill_outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
        drill_outer.set_margin_all(16);
        drill_outer.append(&drill_name_label);
        drill_outer.append(&drill_status_label);
        drill_outer.append(&drill_metadata_label);
        drill_outer.append(&drill_error_section);
        drill_outer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        drill_outer.append(&drill_renderer_views.stack);

        let drill_scroll = gtk::ScrolledWindow::new();
        drill_scroll.set_vexpand(true);
        drill_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        drill_scroll.set_child(Some(&drill_outer));

        // ToolbarView gives us a header bar with an automatic back button
        // when the page is nested inside an AdwNavigationView.
        let drill_header = adw::HeaderBar::new();
        let drill_toolbar = adw::ToolbarView::new();
        drill_toolbar.add_top_bar(&drill_header);
        drill_toolbar.set_content(Some(&drill_scroll));

        let drill_page = adw::NavigationPage::builder()
            .title("Tool Details")
            .tag("drill-down")
            .child(&drill_toolbar)
            .build();
        // Register the page so it always has a stable parent; push/pop manage
        // visibility.  Without add(), repeated push-after-pop cycles can hit a
        // GTK parentage assertion when the page is still being unparented by a
        // running pop animation.
        nav_view.add(&drill_page);

        // ── Attach nav_view to root ───────────────────────────────────────────
        root.append(&nav_view);

        let model = ToolInspectorPane {
            db_path,
            selection: InspectorSelection::None,
            load_state: LoadState::Idle,
            active_request_id: 0,
            tool_call: None,
            subagent: None,
            subagent_tools: Vec::new(),
            drilled_tool: None,
            pending_drill_tool_id: None,
            sender: sender.clone(),
            nav_view,
            content_stack,
            error_label,
            tool_name_label,
            tool_status_label,
            tool_metadata_label,
            tool_error_section,
            tool_error_label,
            tool_renderer_views,
            subagent_title_label,
            subagent_prompt_section,
            subagent_prompt_label,
            subagent_result_section,
            subagent_result_label,
            subagent_tools_list,
            open_session_button,
            drill_page,
            drill_name_label,
            drill_status_label,
            drill_metadata_label,
            drill_error_section,
            drill_error_label,
            drill_renderer_views,
            drill_page_pushed: Cell::new(false),
        };

        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            ToolInspectorPaneMsg::SelectToolCall {
                session_id,
                tool_call_id,
            } => {
                self.selection = InspectorSelection::ToolCall {
                    session_id: session_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                };
                let request_id =
                    begin_loading_request(&mut self.active_request_id, &mut self.load_state);
                self.drilled_tool = None;
                self.pending_drill_tool_id = None;
                self.tool_call = None;
                self.subagent = None;
                self.subagent_tools.clear();

                let db_path = self.db_path.clone();
                sender.spawn_oneshot_command(move || ToolInspectorPaneCmd::ToolCall {
                    request_id,
                    session_id: session_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    result: load_tool_call(db_path.as_path(), &session_id, &tool_call_id)
                        .map_err(|err| err.to_string()),
                });
            }

            ToolInspectorPaneMsg::SelectSubagent {
                session_id,
                subagent_id,
            } => {
                self.selection = InspectorSelection::Subagent {
                    session_id: session_id.clone(),
                    subagent_id: subagent_id.clone(),
                };
                let request_id =
                    begin_loading_request(&mut self.active_request_id, &mut self.load_state);
                self.drilled_tool = None;
                self.pending_drill_tool_id = None;
                self.tool_call = None;
                self.subagent = None;
                self.subagent_tools.clear();

                let db_path = self.db_path.clone();
                sender.spawn_oneshot_command(move || ToolInspectorPaneCmd::Subagent {
                    request_id,
                    session_id: session_id.clone(),
                    subagent_id: subagent_id.clone(),
                    subagent_result: load_subagent(db_path.as_path(), &session_id, &subagent_id)
                        .map_err(|err| err.to_string()),
                    tools_result: load_tool_calls_for_subagent(
                        db_path.as_path(),
                        &session_id,
                        &subagent_id,
                    )
                    .map_err(|err| err.to_string()),
                });
            }

            ToolInspectorPaneMsg::Clear => {
                self.selection = InspectorSelection::None;
                clear_active_request(&mut self.active_request_id, &mut self.load_state);
                self.tool_call = None;
                self.subagent = None;
                self.subagent_tools.clear();
                self.drilled_tool = None;
                self.pending_drill_tool_id = None;
            }

            ToolInspectorPaneMsg::DrillDownTool(tool_call_id) => {
                // Prefer already-loaded subagent_tools cache; fall back to DB.
                let cached = self
                    .subagent_tools
                    .iter()
                    .find(|t| t.id == tool_call_id)
                    .cloned();

                if let Some(tc) = cached {
                    self.drilled_tool = Some(tc);
                    self.pending_drill_tool_id = None;
                } else if let InspectorSelection::Subagent { ref session_id, .. } = self.selection {
                    self.pending_drill_tool_id = Some(tool_call_id.clone());
                    let selection_session_id = session_id.clone();
                    let db_path = self.db_path.clone();
                    sender.spawn_oneshot_command(move || ToolInspectorPaneCmd::DrillTool {
                        session_id: selection_session_id.clone(),
                        tool_call_id: tool_call_id.clone(),
                        result: load_tool_call(
                            db_path.as_path(),
                            &selection_session_id,
                            &tool_call_id,
                        )
                        .map_err(|err| err.to_string()),
                    });
                }
            }

            ToolInspectorPaneMsg::PopDrillDown => {
                // Native back button already popped the page; sync model state.
                self.drilled_tool = None;
                self.pending_drill_tool_id = None;
                self.drill_page_pushed.set(false);
            }

            ToolInspectorPaneMsg::OpenChildSession => {
                if let Some(ref sa) = self.subagent
                    && let Some(ref child_id) = sa.child_session_id
                {
                    sender
                        .output(ToolInspectorPaneOutput::OpenChildSession(child_id.clone()))
                        .ok();
                }
            }
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
                result,
            } => {
                let request_result = result.as_ref().map(|_| ()).map_err(Clone::clone);
                if apply_load_result(
                    self.active_request_id,
                    &mut self.load_state,
                    request_id,
                    request_result,
                )
                .is_none()
                {
                    return;
                }

                match result {
                    Ok(tc) => {
                        if tc.is_none() {
                            tracing::warn!(
                                "Tool call not found: {} in session {}",
                                tool_call_id,
                                session_id
                            );
                        }
                        self.tool_call = tc;
                    }
                    Err(err) => {
                        tracing::error!("Failed to load tool call {}: {}", tool_call_id, err);
                        self.tool_call = None;
                    }
                }
            }
            ToolInspectorPaneCmd::Subagent {
                request_id,
                session_id,
                subagent_id,
                subagent_result,
                tools_result,
            } => {
                let request_result = subagent_request_result(&subagent_result, &tools_result);
                if apply_load_result(
                    self.active_request_id,
                    &mut self.load_state,
                    request_id,
                    request_result,
                )
                .is_none()
                {
                    return;
                }

                match subagent_result {
                    Ok(sa) => {
                        if sa.is_none() {
                            tracing::warn!(
                                "Subagent not found: {} in session {}",
                                subagent_id,
                                session_id
                            );
                        }
                        self.subagent = sa;
                    }
                    Err(err) => {
                        tracing::error!("Failed to load subagent {}: {}", subagent_id, err);
                        self.subagent = None;
                    }
                }

                match tools_result {
                    Ok(tools) => self.subagent_tools = tools,
                    Err(err) => {
                        tracing::error!(
                            "Failed to load subagent tools for {}: {}",
                            subagent_id,
                            err
                        );
                        self.subagent_tools.clear();
                    }
                }
            }
            ToolInspectorPaneCmd::DrillTool {
                session_id,
                tool_call_id,
                result,
            } => {
                if !matches!(
                    self.selection,
                    InspectorSelection::Subagent {
                        session_id: ref active_session,
                        ..
                    } if active_session == &session_id
                ) {
                    return;
                }

                if self.pending_drill_tool_id.as_deref() != Some(tool_call_id.as_str()) {
                    return;
                }

                self.pending_drill_tool_id = None;
                match result {
                    Ok(tc) => self.drilled_tool = tc,
                    Err(err) => {
                        tracing::error!("Failed to load drill tool {}: {}", tool_call_id, err);
                    }
                }
            }
        }
    }

    fn post_view(&self, _widgets: &mut Self::Widgets) {
        // 1. Switch overview content stack to the appropriate page.
        let visible_page = match &self.load_state {
            LoadState::Loading => "loading",
            LoadState::LoadError(message) => {
                self.error_label.set_label(message);
                "error"
            }
            LoadState::Idle | LoadState::Ready => match &self.selection {
                InspectorSelection::None => "empty",
                InspectorSelection::ToolCall { .. } => {
                    if self.tool_call.is_some() {
                        "tool"
                    } else {
                        "empty"
                    }
                }
                InspectorSelection::Subagent { .. } => {
                    if self.subagent.is_some() {
                        "subagent"
                    } else {
                        "empty"
                    }
                }
            },
        };
        self.content_stack.set_visible_child_name(visible_page);

        // 2. Update tool-call content widgets.
        if let Some(ref tc) = self.tool_call {
            self.tool_name_label.set_label(&tc.tool_name);
            self.tool_status_label
                .set_label(&format_status_duration(tc.status, tc.duration_ms));
            let metadata_line = format_tool_metadata_line(tc);
            apply_optional_line(&self.tool_metadata_label, metadata_line.as_deref());
            let error_text = tool_error_message(tc);
            apply_optional_section(&self.tool_error_section, &self.tool_error_label, error_text);
            apply_renderer_stack(&self.tool_renderer_views, tc);
        }

        // 3. Update subagent content widgets.
        if let Some(ref sa) = self.subagent {
            self.subagent_title_label.set_label(&sa.title);
            apply_optional_section(
                &self.subagent_prompt_section,
                &self.subagent_prompt_label,
                sa.prompt.as_deref(),
            );
            apply_optional_section(
                &self.subagent_result_section,
                &self.subagent_result_label,
                sa.result_summary.as_deref(),
            );

            // Rebuild inner-tools list.
            while let Some(child) = self.subagent_tools_list.first_child() {
                self.subagent_tools_list.remove(&child);
            }
            for tool in &self.subagent_tools {
                let row = adw::ActionRow::builder()
                    .title(tool.title.as_deref().unwrap_or(&tool.tool_name))
                    .subtitle(&tool.tool_name)
                    .activatable(true)
                    .build();

                let status_icon = gtk::Image::from_icon_name(status_icon_name(tool.status));
                status_icon.set_pixel_size(16);
                row.add_prefix(&status_icon);

                if let Some(ms) = tool.duration_ms {
                    let dur_label = gtk::Label::new(Some(&format_duration_ms(ms)));
                    dur_label.add_css_class("dim-label");
                    dur_label.add_css_class("caption");
                    row.add_suffix(&dur_label);
                }

                let next_icon = gtk::Image::from_icon_name("go-next-symbolic");
                row.add_suffix(&next_icon);

                let s = self.sender.clone();
                let id = tool.id.clone();
                row.connect_activated(move |_| {
                    s.input(ToolInspectorPaneMsg::DrillDownTool(id.clone()));
                });

                self.subagent_tools_list.append(&row);
            }

            self.open_session_button
                .set_visible(sa.child_session_id.is_some());
        }

        // 4. Manage drill-down page push/pop.
        if let Some(ref tc) = self.drilled_tool {
            // Update drill-down content before (re-)showing the page.
            self.drill_page.set_title(&tc.tool_name);
            self.drill_name_label.set_label(&tc.tool_name);
            self.drill_status_label
                .set_label(&format_status_duration(tc.status, tc.duration_ms));
            let metadata_line = format_tool_metadata_line(tc);
            apply_optional_line(&self.drill_metadata_label, metadata_line.as_deref());
            let error_text = tool_error_message(tc);
            apply_optional_section(
                &self.drill_error_section,
                &self.drill_error_label,
                error_text,
            );
            apply_renderer_stack(&self.drill_renderer_views, tc);

            if !self.drill_page_pushed.get() {
                self.nav_view.push(&self.drill_page);
                self.drill_page_pushed.set(true);
            }
        } else if self.drill_page_pushed.get() {
            self.nav_view.pop();
            self.drill_page_pushed.set(false);
        }
    }
}

// ── Widget helpers ────────────────────────────────────────────────────────────

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

/// Build a section box (heading label + mono content label), hidden by default.
fn make_text_section(title: &str) -> (gtk::Box, gtk::Label) {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let header = gtk::Label::new(Some(title));
    header.add_css_class("inspector-section-heading");
    header.set_halign(gtk::Align::Start);
    let content = make_mono_label();
    content.add_css_class("inspector-code-block");
    section.append(&header);
    section.append(&content);
    section.set_visible(false);
    (section, content)
}

/// Show or hide a section based on whether `text` is non-empty.
fn apply_optional_section(section: &gtk::Box, label: &gtk::Label, text: Option<&str>) {
    match text {
        Some(t) if !t.is_empty() => {
            label.set_label(t);
            section.set_visible(true);
        }
        _ => section.set_visible(false),
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

fn apply_renderer_stack(views: &RendererStackViews, tool_call: &ToolCall) {
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

fn build_generic_widget(
    rendered: &crate::ui::tool_renderers::generic::GenericRenderedData,
) -> gtk::Widget {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 8);

    if let Some(input) = rendered.input_text.as_deref() {
        let header = gtk::Label::new(Some("Input"));
        header.add_css_class("inspector-section-heading");
        header.set_halign(gtk::Align::Start);
        container.append(&header);

        let content = gtk::TextView::new();
        content.buffer().set_text(input);
        content.set_editable(false);
        content.set_cursor_visible(false);
        content.set_wrap_mode(gtk::WrapMode::WordChar);
        content.set_monospace(true);
        content.add_css_class("inspector-code-block");
        content.set_vexpand(true);
        container.append(&content);
    }

    if let Some(output) = rendered.output.as_ref() {
        let header = gtk::Label::new(Some("Output"));
        header.add_css_class("inspector-section-heading");
        header.set_halign(gtk::Align::Start);
        container.append(&header);

        let text = match output {
            crate::ui::tool_renderers::generic::OutputRenderPlan::PrettyJson(t) => t.as_str(),
            crate::ui::tool_renderers::generic::OutputRenderPlan::Markdown(t) => t.as_str(),
        };

        let content = gtk::TextView::new();
        content.buffer().set_text(text);
        content.set_editable(false);
        content.set_cursor_visible(false);
        content.set_wrap_mode(gtk::WrapMode::WordChar);
        content.set_monospace(true);
        content.add_css_class("inspector-code-block");
        content.set_vexpand(true);
        container.append(&content);
    }

    container.upcast()
}

fn build_terminal_widget(
    rendered: &crate::ui::tool_renderers::terminal::TerminalRenderedData,
) -> gtk::Widget {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 8);

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
        container.append(&header);

        let output_view = gtk::TextView::new();
        output_view.buffer().set_text(output);
        output_view.set_editable(false);
        output_view.set_cursor_visible(false);
        output_view.set_wrap_mode(gtk::WrapMode::WordChar);
        output_view.set_monospace(true);
        output_view.add_css_class("terminal-output");
        output_view.add_css_class("inspector-code-block");
        output_view.set_vexpand(true);
        container.append(&output_view);
    }

    if rendered.is_non_zero_exit {
        if let Some(code) = rendered.exit_code {
            let exit_label = gtk::Label::new(Some(&format!("Exit code: {}", code)));
            exit_label.add_css_class("terminal-exit-nonzero");
            exit_label.set_halign(gtk::Align::Start);
            container.append(&exit_label);
        }
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
        header_label.set_selectable(true);
        container.append(&header_label);
    }

    if let Some(output) = rendered.output_text.as_deref() {
        let content = gtk::TextView::new();
        content.buffer().set_text(output);
        content.set_editable(false);
        content.set_cursor_visible(false);
        content.set_wrap_mode(gtk::WrapMode::WordChar);
        content.set_monospace(true);
        content.add_css_class("inspector-code-block");
        content.set_vexpand(true);
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
        let output_view = gtk::TextView::new();
        output_view.buffer().set_text(output);
        output_view.set_editable(false);
        output_view.set_cursor_visible(false);
        output_view.set_wrap_mode(gtk::WrapMode::WordChar);
        output_view.add_css_class("inspector-code-block");
        output_view.set_vexpand(true);
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
        let header = gtk::Label::new(Some("Input"));
        header.add_css_class("inspector-section-heading");
        header.set_halign(gtk::Align::Start);
        container.append(&header);

        let content = gtk::TextView::new();
        content.buffer().set_text(input);
        content.set_editable(false);
        content.set_cursor_visible(false);
        content.set_wrap_mode(gtk::WrapMode::WordChar);
        content.set_monospace(true);
        content.add_css_class("inspector-code-block");
        container.append(&content);
    }

    if let Some(result) = rendered.result_text.as_deref() {
        let header = gtk::Label::new(Some("Result"));
        header.add_css_class("inspector-section-heading");
        header.set_halign(gtk::Align::Start);
        container.append(&header);

        let content = gtk::TextView::new();
        content.buffer().set_text(result);
        content.set_editable(false);
        content.set_cursor_visible(false);
        content.set_wrap_mode(gtk::WrapMode::WordChar);
        content.add_css_class("inspector-code-block");
        container.append(&content);
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

fn subagent_request_result(
    subagent_result: &Result<Option<Subagent>, String>,
    tools_result: &Result<Vec<ToolCall>, String>,
) -> Result<(), String> {
    if let Err(err) = subagent_result {
        return Err(err.clone());
    }

    if let Err(err) = tools_result {
        tracing::warn!(
            "Subagent loaded but tools list failed; continuing with empty tools: {}",
            err
        );
    }

    Ok(())
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
    use crate::ui::tool_renderers::file::FileRenderedData;
    use crate::ui::tool_renderers::generic::{GenericRenderedData, OutputRenderPlan};
    use crate::ui::tool_renderers::results::ResultsRenderedData;
    use crate::ui::tool_renderers::subagent::SubagentRenderedData;
    use crate::ui::tool_renderers::terminal::TerminalRenderedData;

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
    fn subagent_tools_failure_does_not_force_global_load_error() {
        let state_result =
            subagent_request_result(&Ok(None), &Err("tools fetch failed".to_string()));

        assert_eq!(state_result, Ok(()));
    }

    #[test]
    fn formatter_renders_terminal_content_without_error_section_duplication() {
        let rendered = TerminalRenderedData {
            command: Some("cargo test".to_string()),
            output_text: Some("done".to_string()),
            error_text: Some("warn".to_string()),
            display_text: Some("done".to_string()),
            exit_code: Some(1),
            is_non_zero_exit: true,
            status: ToolCallStatus::Error,
            duration_ms: None,
        };

        let text = format_terminal_renderer_text(&rendered);
        assert!(text.contains("$ cargo test"));
        assert!(text.contains("Output\ndone"));
        assert!(!text.contains("Error\nwarn"));
    }

    #[test]
    fn formatter_renders_generic_file_and_results_content_without_error_duplication() {
        let generic_text = format_generic_renderer_text(&GenericRenderedData {
            input_text: None,
            output: Some(OutputRenderPlan::Markdown("ok".to_string())),
            error: Some(OutputRenderPlan::Markdown("failed".to_string())),
        });
        assert!(generic_text.contains("Output\nok"));
        assert!(!generic_text.contains("Error\nfailed"));

        let file_text = format_file_renderer_text(&FileRenderedData {
            header: Some("src/main.rs".to_string()),
            output_text: Some("fn main() {}".to_string()),
            error_text: Some("permission denied".to_string()),
            status: ToolCallStatus::Error,
            duration_ms: None,
        });
        assert!(file_text.contains("Output\nfn main() {}"));
        assert!(!file_text.contains("Error\npermission denied"));

        let results_text = format_results_renderer_text(&ResultsRenderedData {
            entries: vec![],
            output_text: Some("raw output".to_string()),
            error_text: Some("raw error".to_string()),
            status: ToolCallStatus::Error,
            duration_ms: None,
        });
        assert!(results_text.contains("Output\nraw output"));
        assert!(!results_text.contains("Error\nraw error"));
    }

    #[test]
    fn formatter_renders_subagent_input_and_result_without_error_duplication() {
        let text = format_subagent_renderer_text(&SubagentRenderedData {
            input_text: Some("{\"prompt\":\"investigate\"}".to_string()),
            result_text: Some("completed".to_string()),
            error_text: Some("partial failure".to_string()),
        });

        assert!(text.contains("Input\n{\"prompt\":\"investigate\"}"));
        assert!(text.contains("Result\ncompleted"));
        assert!(!text.contains("Error\npartial failure"));
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
