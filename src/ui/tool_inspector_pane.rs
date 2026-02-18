use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;

use adw::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, adw, gtk};

use crate::database::{load_subagent, load_tool_call, load_tool_calls_for_subagent};
use crate::models::{Subagent, ToolCall, ToolCallStatus};

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

// ── Component ─────────────────────────────────────────────────────────────────

pub struct ToolInspectorPane {
    db_path: Arc<PathBuf>,
    selection: InspectorSelection,
    tool_call: Option<ToolCall>,
    subagent: Option<Subagent>,
    subagent_tools: Vec<ToolCall>,
    drilled_tool: Option<ToolCall>,

    // Sender for attaching to dynamically-created inner-tool row callbacks.
    sender: ComponentSender<ToolInspectorPane>,

    // Navigation view (managed imperatively; stored for push/pop in post_view).
    nav_view: adw::NavigationView,

    // Overview content switcher: "empty" / "tool" / "subagent"
    content_stack: gtk::Stack,

    // Tool-call detail widgets (inside "tool" stack page)
    tool_name_label: gtk::Label,
    tool_status_label: gtk::Label,
    tool_input_section: gtk::Box,
    tool_input_label: gtk::Label,
    tool_output_section: gtk::Box,
    tool_output_label: gtk::Label,
    tool_error_section: gtk::Box,
    tool_error_label: gtk::Label,

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
    drill_input_section: gtk::Box,
    drill_input_label: gtk::Label,
    drill_output_section: gtk::Box,
    drill_output_label: gtk::Label,
    drill_error_section: gtk::Box,
    drill_error_label: gtk::Label,

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

// ── SimpleComponent impl ──────────────────────────────────────────────────────

#[relm4::component(pub)]
impl SimpleComponent for ToolInspectorPane {
    type Init = Arc<PathBuf>;
    type Input = ToolInspectorPaneMsg;
    type Output = ToolInspectorPaneOutput;
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
        // Inspector nav pop-on-escape is left at its default (true) so that the
        // drill-down page can be dismissed natively; the app-level Esc contract
        // handles the outer pane close / back-navigation priority (Phase 5).

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

        // — Tool-call detail ——————————————————————————————————————————————————
        let tool_name_label = make_title_label();
        tool_name_label.add_css_class("monospace");

        let tool_status_label = make_caption_label();

        let (tool_input_section, tool_input_label) = make_text_section("Input");
        let (tool_output_section, tool_output_label) = make_text_section("Output");
        let (tool_error_section, tool_error_label) = make_text_section("Error");

        let tool_outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
        tool_outer.set_margin_all(16);
        tool_outer.append(&tool_name_label);
        tool_outer.append(&tool_status_label);
        tool_outer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        tool_outer.append(&tool_input_section);
        tool_outer.append(&tool_output_section);
        tool_outer.append(&tool_error_section);

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

        let (drill_input_section, drill_input_label) = make_text_section("Input");
        let (drill_output_section, drill_output_label) = make_text_section("Output");
        let (drill_error_section, drill_error_label) = make_text_section("Error");

        let drill_outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
        drill_outer.set_margin_all(16);
        drill_outer.append(&drill_name_label);
        drill_outer.append(&drill_status_label);
        drill_outer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        drill_outer.append(&drill_input_section);
        drill_outer.append(&drill_output_section);
        drill_outer.append(&drill_error_section);

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
            tool_call: None,
            subagent: None,
            subagent_tools: Vec::new(),
            drilled_tool: None,
            sender: sender.clone(),
            nav_view,
            content_stack,
            tool_name_label,
            tool_status_label,
            tool_input_section,
            tool_input_label,
            tool_output_section,
            tool_output_label,
            tool_error_section,
            tool_error_label,
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
            drill_input_section,
            drill_input_label,
            drill_output_section,
            drill_output_label,
            drill_error_section,
            drill_error_label,
            drill_page_pushed: Cell::new(false),
        };

        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            ToolInspectorPaneMsg::SelectToolCall {
                session_id,
                tool_call_id,
            } => {
                self.selection = InspectorSelection::ToolCall {
                    session_id: session_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                };
                self.drilled_tool = None;
                self.subagent = None;
                self.subagent_tools.clear();

                match load_tool_call(&self.db_path, &session_id, &tool_call_id) {
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

            ToolInspectorPaneMsg::SelectSubagent {
                session_id,
                subagent_id,
            } => {
                self.selection = InspectorSelection::Subagent {
                    session_id: session_id.clone(),
                    subagent_id: subagent_id.clone(),
                };
                self.drilled_tool = None;
                self.tool_call = None;

                match load_subagent(&self.db_path, &session_id, &subagent_id) {
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

                match load_tool_calls_for_subagent(&self.db_path, &session_id, &subagent_id) {
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

            ToolInspectorPaneMsg::Clear => {
                self.selection = InspectorSelection::None;
                self.tool_call = None;
                self.subagent = None;
                self.subagent_tools.clear();
                self.drilled_tool = None;
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
                } else if let InspectorSelection::Subagent { ref session_id, .. } = self.selection {
                    match load_tool_call(&self.db_path, session_id, &tool_call_id) {
                        Ok(tc) => self.drilled_tool = tc,
                        Err(err) => {
                            tracing::error!("Failed to load drill tool {}: {}", tool_call_id, err);
                        }
                    }
                }
            }

            ToolInspectorPaneMsg::PopDrillDown => {
                // Native back button already popped the page; sync model state.
                self.drilled_tool = None;
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

    fn post_view(&self, _widgets: &mut Self::Widgets) {
        // 1. Switch overview content stack to the appropriate page.
        self.content_stack
            .set_visible_child_name(match &self.selection {
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
            });

        // 2. Update tool-call content widgets.
        if let Some(ref tc) = self.tool_call {
            self.tool_name_label.set_label(&tc.tool_name);
            self.tool_status_label
                .set_label(&format_status_duration(tc.status, tc.duration_ms));
            apply_optional_section(
                &self.tool_input_section,
                &self.tool_input_label,
                tc.input_json.as_deref(),
            );
            apply_optional_section(
                &self.tool_output_section,
                &self.tool_output_label,
                tc.output_text.as_deref(),
            );
            apply_optional_section(
                &self.tool_error_section,
                &self.tool_error_label,
                tc.error_text.as_deref(),
            );
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
            apply_optional_section(
                &self.drill_input_section,
                &self.drill_input_label,
                tc.input_json.as_deref(),
            );
            apply_optional_section(
                &self.drill_output_section,
                &self.drill_output_label,
                tc.output_text.as_deref(),
            );
            apply_optional_section(
                &self.drill_error_section,
                &self.drill_error_label,
                tc.error_text.as_deref(),
            );

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

fn format_duration_ms(ms: i64) -> String {
    if ms < 1_000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else {
        let secs = ms / 1_000;
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

fn status_icon_name(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Completed => "emblem-ok-symbolic",
        ToolCallStatus::Error => "dialog-error-symbolic",
        ToolCallStatus::Running => "emblem-synchronizing-symbolic",
        ToolCallStatus::Pending => "content-loading-symbolic",
        ToolCallStatus::Unknown => "dialog-question-symbolic",
    }
}
