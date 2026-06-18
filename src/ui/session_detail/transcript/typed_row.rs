use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use gtk::prelude::*;
use relm4::binding::Binding;
use relm4::{adw, gtk, typed_view::list::RelmListItem};

use crate::models::{Role, tool_name_icon};
use crate::ui::format::format_duration_ms;
use crate::ui::highlight;
use crate::ui::session_detail::SessionDetailMsg;
use crate::ui::session_detail::transcript::item_data::{TranscriptItemData, TranscriptItemKind};
use crate::ui::session_detail::transcript::item_init::{MessageItemInit, TranscriptRowBuildKind};
use crate::ui::session_detail::transcript::row_rendering::{
    format_reasoning_burst_label, format_tool_burst_accessible_label,
    format_tool_burst_match_badge_accessible_label, model_label_text, populate_tool_burst_children,
    render_content,
};
use crate::ui::session_detail::transcript::tool_call_row::{
    TOOL_ICONS, ToolCallRowHeaderInit, append_reasoning_pill, build_tool_call_row_header,
    encrypted_reasoning_pill, encrypted_reasoning_pill_with_label,
};

const TOOL_BURST_ARROW_COLLAPSED: &str = "pan-end-symbolic";
const TOOL_BURST_ARROW_EXPANDED: &str = "pan-down-symbolic";

const MESSAGE_PAGE_NAME: &str = "message";
const TOOL_CALL_PAGE_NAME: &str = "tool-call";
const TOOL_BURST_PAGE_NAME: &str = "tool-burst";
const SUBAGENT_PAGE_NAME: &str = "subagent";
pub(crate) const TRANSCRIPT_ROW_WIDGET_NAME_PREFIX: &str = "transcript-row-";

pub struct TranscriptRowWidgets {
    stack: gtk::Stack,
    message: MessagePageWidgets,
    tool_call: ToolCallPageWidgets,
    tool_burst: ToolBurstPageWidgets,
    subagent: SubagentPageWidgets,
}

pub struct MessagePageWidgets {
    root: gtk::Box,
    role_label: gtk::Label,
    model_sep: gtk::Label,
    model_label: gtk::Label,
    ts_sep: gtk::Label,
    ts_label: gtk::Label,
    reasoning_box: gtk::Box,
    content: gtk::Box,
    expand_button: gtk::Button,
    connected_handlers: Vec<(gtk::glib::Object, gtk::glib::SignalHandlerId)>,
}

pub struct ToolCallPageWidgets {
    root: gtk::Box,
    connected_handlers: Vec<(gtk::glib::Object, gtk::glib::SignalHandlerId)>,
}

pub struct ToolBurstPageWidgets {
    root: gtk::Box,
    header_button: gtk::Button,
    arrow_icon: gtk::Image,
    header_wrap: adw::WrapBox,
    revealer: gtk::Revealer,
    children: gtk::Box,
    reveal_binding: Option<gtk::glib::Binding>,
    children_built_for: Rc<Cell<Option<usize>>>,
    connected_handlers: Vec<(gtk::glib::Object, gtk::glib::SignalHandlerId)>,
}

pub struct SubagentPageWidgets {
    root: gtk::Box,
    connected_handlers: Vec<(gtk::glib::Object, gtk::glib::SignalHandlerId)>,
}

impl RelmListItem for TranscriptItemData {
    type Root = gtk::Box;
    type Widgets = TranscriptRowWidgets;

    fn setup(list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        list_item.set_activatable(false);
        list_item.set_selectable(false);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let stack = gtk::Stack::new();
        stack.set_hhomogeneous(false);
        stack.set_vhomogeneous(false);
        root.append(&stack);

        let message = build_message_page();
        stack.add_named(&message.root, Some(MESSAGE_PAGE_NAME));

        let tool_call = build_tool_call_page();
        stack.add_named(&tool_call.root, Some(TOOL_CALL_PAGE_NAME));

        let tool_burst = build_tool_burst_page();
        stack.add_named(&tool_burst.root, Some(TOOL_BURST_PAGE_NAME));

        let subagent = build_subagent_page();
        stack.add_named(&subagent.root, Some(SUBAGENT_PAGE_NAME));

        (
            root,
            TranscriptRowWidgets {
                stack,
                message,
                tool_call,
                tool_burst,
                subagent,
            },
        )
    }

    fn bind(&mut self, widgets: &mut Self::Widgets, root: &mut Self::Root) {
        let start = Instant::now();
        let kind = TranscriptRowBuildKind::from(&self.kind);
        root.set_widget_name(&format!(
            "{TRANSCRIPT_ROW_WIDGET_NAME_PREFIX}{}",
            self.item_index
        ));
        widgets.stack.set_visible_child_name(kind.page_name());

        match kind {
            TranscriptRowBuildKind::Message => self.bind_message_page(&mut widgets.message),
            TranscriptRowBuildKind::ToolCall => self.bind_tool_call_page(&mut widgets.tool_call),
            TranscriptRowBuildKind::ToolBurst => self.bind_tool_burst_page(&mut widgets.tool_burst),
            TranscriptRowBuildKind::Subagent => self.bind_subagent_page(&mut widgets.subagent),
        }

        self.sender
            .send(SessionDetailMsg::RowBuilt {
                item_index: self.item_index,
                kind,
                build_duration_ms: start.elapsed().as_millis(),
            })
            .ok();
    }

    fn unbind(&mut self, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        match TranscriptRowBuildKind::from(&self.kind) {
            TranscriptRowBuildKind::Message => self.unbind_message_page(&mut widgets.message),
            TranscriptRowBuildKind::ToolCall => self.unbind_tool_call_page(&mut widgets.tool_call),
            TranscriptRowBuildKind::ToolBurst => {
                self.unbind_tool_burst_page(&mut widgets.tool_burst)
            }
            TranscriptRowBuildKind::Subagent => self.unbind_subagent_page(&mut widgets.subagent),
        }

        cleanup_transcript_row_widgets(widgets);
    }
}

impl TranscriptItemData {
    pub(crate) fn bind_message_page(&self, widgets: &mut MessagePageWidgets) {
        let TranscriptItemKind::Message(message) = &self.kind else {
            return;
        };

        set_role_css_class(&widgets.root, message.preview.role);
        set_role_css_class(&widgets.role_label, message.preview.role);
        widgets
            .role_label
            .set_label(if message.preview.role == Role::Assistant {
                "Assistant"
            } else {
                "You"
            });

        if let Some(model) =
            model_label_text(message.preview.role, message.preview.model.as_deref())
        {
            widgets.model_sep.set_visible(true);
            widgets.model_label.set_visible(true);
            widgets.model_label.set_label(&model);
        } else {
            widgets.model_sep.set_visible(false);
            widgets.model_label.set_visible(false);
            widgets.model_label.set_label("");
        }

        widgets
            .ts_label
            .set_label(&message.preview.timestamp.format("%H:%M:%S").to_string());
        clear_box_children(&widgets.reasoning_box);
        if message.preview.reasoning_preview.has_visible_reasoning {
            let button = gtk::Button::with_label("Thinking");
            button.add_css_class("flat");
            button.add_css_class("pill");
            button.add_css_class("reasoning-pill");
            let sender = self.sender.clone();
            let transcript_item_index = message.transcript_item_index;
            let id = button.connect_clicked(move |_| {
                sender.emit(SessionDetailMsg::InspectReasoning(transcript_item_index));
            });
            widgets
                .connected_handlers
                .push((button.clone().upcast(), id));
            widgets.reasoning_box.append(&button);
        } else if message.preview.reasoning_preview.encrypted_only {
            widgets.reasoning_box.append(&encrypted_reasoning_pill());
        }

        render_message_body(
            &widgets.content,
            &widgets.expand_button,
            message,
            self.expanded.get(),
            &self.full_content.borrow(),
            self.highlight_query.as_deref(),
        );

        let can_expand = message.preview.is_truncated() && message.preview.role != Role::ToolResult;
        if can_expand {
            // Expand/collapse and lazy full-content loading mutate this row in
            // place rather than replacing the list item: replacing it makes
            // GtkListView reset the surrounding scroll back to the top (#170).
            // Re-render whenever `content_revision` is bumped (toggle, content
            // arrival, load-failure rollback).
            let revision_handler = {
                let content = widgets.content.clone();
                let expand_button = widgets.expand_button.clone();
                let message = message.clone();
                let expanded = self.expanded.clone();
                let full_content = self.full_content.clone();
                let highlight_query = self.highlight_query.clone();
                self.content_revision
                    .connect_notify_local(Some("value"), move |_, _| {
                        render_message_body(
                            &content,
                            &expand_button,
                            &message,
                            expanded.get(),
                            &full_content.borrow(),
                            highlight_query.as_deref(),
                        );
                    })
            };
            widgets
                .connected_handlers
                .push((self.content_revision.clone().upcast(), revision_handler));

            let sender = self.sender.clone();
            let item_index = self.item_index;
            let expanded = self.expanded.clone();
            let full_content = self.full_content.clone();
            let content_revision = self.content_revision.clone();
            let id = widgets.expand_button.connect_clicked(move |_| {
                let now_expanded = !expanded.get();
                expanded.set(now_expanded);
                if now_expanded && full_content.borrow().is_none() {
                    sender.emit(SessionDetailMsg::RequestMessageFullContent { item_index });
                }
                content_revision.set(!content_revision.get());
            });
            widgets
                .connected_handlers
                .push((widgets.expand_button.clone().upcast(), id));
        }
    }

    pub(crate) fn bind_tool_call_page(&self, widgets: &mut ToolCallPageWidgets) {
        let TranscriptItemKind::ToolCall(tool_call) = &self.kind else {
            return;
        };

        clear_box_children(&widgets.root);
        let refs = build_tool_call_page_content(tool_call, self.highlight_query.as_deref());

        if let Some(reasoning_button) = refs.reasoning_button {
            let sender = self.sender.clone();
            let transcript_item_index = tool_call.transcript_item_index;
            let id = reasoning_button.connect_clicked(move |_| {
                sender.emit(SessionDetailMsg::InspectReasoning(transcript_item_index));
            });
            widgets
                .connected_handlers
                .push((reasoning_button.upcast(), id));
        }

        let sender = self.sender.clone();
        let tool_call_id = tool_call.tool_call_id.clone();
        let id = refs.inspect_button.connect_clicked(move |_| {
            sender.emit(SessionDetailMsg::InspectToolCall(tool_call_id.clone()));
        });
        widgets
            .connected_handlers
            .push((refs.inspect_button.clone().upcast(), id));

        widgets.root.append(&refs.root);
    }

    pub(crate) fn bind_tool_burst_page(&self, widgets: &mut ToolBurstPageWidgets) {
        let TranscriptItemKind::ToolBurst(burst) = &self.kind else {
            return;
        };

        rebuild_tool_burst_header(&widgets.header_wrap, burst);
        widgets
            .header_button
            .update_property(&[gtk::accessible::Property::Label(
                &format_tool_burst_accessible_label(
                    &burst.category_counts,
                    burst.tool_calls.len(),
                    burst.error_count,
                ),
            )]);
        clear_box_children(&widgets.children);
        widgets.children_built_for.set(None);
        set_tool_burst_expanded_state(
            &widgets.header_button,
            &widgets.arrow_icon,
            self.expanded.get(),
        );

        let notify_id = {
            let children = widgets.children.clone();
            let revealer = widgets.revealer.clone();
            let children_built_for = widgets.children_built_for.clone();
            let sender = self.sender.clone();
            let burst = burst_with_highlight_query(burst, self.highlight_query.as_deref());
            let item_index = self.item_index;
            let header_button = widgets.header_button.clone();
            let arrow_icon = widgets.arrow_icon.clone();
            widgets.revealer.connect_reveal_child_notify(move |_| {
                let expanded = revealer.reveals_child();
                set_tool_burst_expanded_state(&header_button, &arrow_icon, expanded);
                if expanded {
                    build_tool_burst_children_if_needed(
                        &children,
                        &children_built_for,
                        &burst,
                        &sender,
                        item_index,
                    );
                }
            })
        };
        widgets
            .connected_handlers
            .push((widgets.revealer.clone().upcast(), notify_id));

        if self.expanded.get() {
            let burst = burst_with_highlight_query(burst, self.highlight_query.as_deref());
            build_tool_burst_children_if_needed(
                &widgets.children,
                &widgets.children_built_for,
                &burst,
                &self.sender,
                self.item_index,
            );
        }

        widgets.reveal_binding = Some(
            self.expanded
                .bind_property("value", &widgets.revealer, "reveal-child")
                .sync_create()
                .build(),
        );

        let expanded = self.expanded.clone();
        let id = widgets.header_button.connect_clicked(move |_| {
            expanded.set(!expanded.get());
        });
        widgets
            .connected_handlers
            .push((widgets.header_button.clone().upcast(), id));
    }

    pub(crate) fn bind_subagent_page(&self, widgets: &mut SubagentPageWidgets) {
        let TranscriptItemKind::Subagent(subagent) = &self.kind else {
            return;
        };

        clear_box_children(&widgets.root);
        let refs = build_subagent_page_content(subagent);

        if let Some(reasoning_button) = refs.reasoning_button {
            let sender = self.sender.clone();
            let transcript_item_index = subagent.transcript_item_index;
            let id = reasoning_button.connect_clicked(move |_| {
                sender.emit(SessionDetailMsg::InspectReasoning(transcript_item_index));
            });
            widgets
                .connected_handlers
                .push((reasoning_button.upcast(), id));
        }

        let sender = self.sender.clone();
        let subagent_id = subagent.subagent_id.clone();
        let id = refs.inspect_button.connect_clicked(move |_| {
            sender.emit(SessionDetailMsg::InspectSubagent(subagent_id.clone()));
        });
        widgets
            .connected_handlers
            .push((refs.inspect_button.clone().upcast(), id));

        widgets.root.append(&refs.root);
    }

    pub(crate) fn unbind_message_page(&self, widgets: &mut MessagePageWidgets) {
        disconnect_handlers(&mut widgets.connected_handlers);
        clear_box_children(&widgets.reasoning_box);
        clear_box_children(&widgets.content);
        widgets.expand_button.set_visible(false);
        widgets.expand_button.set_label("Show full message");
    }

    pub(crate) fn unbind_tool_call_page(&self, widgets: &mut ToolCallPageWidgets) {
        disconnect_handlers(&mut widgets.connected_handlers);
        clear_box_children(&widgets.root);
    }

    pub(crate) fn unbind_tool_burst_page(&self, widgets: &mut ToolBurstPageWidgets) {
        disconnect_handlers(&mut widgets.connected_handlers);
        if let Some(binding) = widgets.reveal_binding.take() {
            binding.unbind();
        }
        widgets.header_wrap.remove_all();
        clear_box_children(&widgets.children);
        widgets.children_built_for.set(None);
        set_tool_burst_expanded_state(
            &widgets.header_button,
            &widgets.arrow_icon,
            widgets.revealer.reveals_child(),
        );
    }

    pub(crate) fn unbind_subagent_page(&self, widgets: &mut SubagentPageWidgets) {
        disconnect_handlers(&mut widgets.connected_handlers);
        clear_box_children(&widgets.root);
    }
}

impl TranscriptRowBuildKind {
    fn page_name(self) -> &'static str {
        match self {
            Self::Message => MESSAGE_PAGE_NAME,
            Self::ToolCall => TOOL_CALL_PAGE_NAME,
            Self::ToolBurst => TOOL_BURST_PAGE_NAME,
            Self::Subagent => SUBAGENT_PAGE_NAME,
        }
    }
}

impl From<&TranscriptItemKind> for TranscriptRowBuildKind {
    fn from(kind: &TranscriptItemKind) -> Self {
        match kind {
            TranscriptItemKind::Message(_) => Self::Message,
            TranscriptItemKind::ToolCall(_) => Self::ToolCall,
            TranscriptItemKind::ToolBurst(_) => Self::ToolBurst,
            TranscriptItemKind::Subagent(_) => Self::Subagent,
        }
    }
}

fn build_message_page() -> MessagePageWidgets {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("message-row");
    root.set_spacing(4);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);

    let role_label = gtk::Label::new(None);
    role_label.add_css_class("caption");
    role_label.add_css_class("heading");
    role_label.set_halign(gtk::Align::Start);
    header.append(&role_label);

    let model_sep = gtk::Label::new(Some("·"));
    model_sep.add_css_class("caption");
    model_sep.add_css_class("dim-label");
    model_sep.set_visible(false);
    header.append(&model_sep);

    let model_label = gtk::Label::new(None);
    model_label.add_css_class("caption");
    model_label.add_css_class("dim-label");
    model_label.add_css_class("monospace");
    model_label.set_visible(false);
    header.append(&model_label);

    let ts_sep = gtk::Label::new(Some("·"));
    ts_sep.add_css_class("caption");
    ts_sep.add_css_class("dim-label");
    header.append(&ts_sep);

    let ts_label = gtk::Label::new(None);
    ts_label.add_css_class("caption");
    ts_label.add_css_class("dim-label");
    header.append(&ts_label);

    let reasoning_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    header.append(&reasoning_box);
    root.append(&header);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    root.append(&content);

    let expand_button = gtk::Button::new();
    expand_button.add_css_class("flat");
    expand_button.add_css_class("caption");
    expand_button.add_css_class("expand-toggle");
    expand_button.set_halign(gtk::Align::Start);
    expand_button.set_margin_top(4);
    expand_button.set_visible(false);
    // The button sits at the bottom of the message row, so it is often only
    // partially visible. Grabbing focus on click made GtkListView scroll the row
    // into view mid-press, moving the button out from under the pointer so the
    // release landed elsewhere and no `clicked` fired — the user had to click
    // twice. Keep keyboard focus (Tab) but do not grab it on click.
    expand_button.set_focus_on_click(false);
    root.append(&expand_button);

    MessagePageWidgets {
        root,
        role_label,
        model_sep,
        model_label,
        ts_sep,
        ts_label,
        reasoning_box,
        content,
        expand_button,
        connected_handlers: Vec::new(),
    }
}

/// Render the message body and expand-toggle label for the current expansion
/// state. Called both on initial bind and on every in-place re-render, so it must
/// be idempotent (`render_content` clears the container first).
fn render_message_body(
    content: &gtk::Box,
    expand_button: &gtk::Button,
    message: &MessageItemInit,
    expanded: bool,
    full_content: &Option<String>,
    highlight_query: Option<&str>,
) {
    let body = if expanded {
        full_content
            .as_deref()
            .unwrap_or(&message.preview.content_preview)
    } else {
        &message.preview.content_preview
    };
    render_content(content, body, message.preview.role, highlight_query);

    let can_expand = message.preview.is_truncated() && message.preview.role != Role::ToolResult;
    expand_button.set_visible(can_expand);
    expand_button.set_label(if expanded {
        "Collapse"
    } else {
        "Show full message"
    });
}

fn set_role_css_class(widget: &impl IsA<gtk::Widget>, role: Role) {
    let widget = widget.as_ref();
    for class in [
        Role::User.css_class(),
        Role::Assistant.css_class(),
        Role::ToolCall.css_class(),
        Role::ToolResult.css_class(),
    ] {
        widget.remove_css_class(class);
    }
    widget.add_css_class(role.css_class());
}

fn build_tool_call_page() -> ToolCallPageWidgets {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    ToolCallPageWidgets {
        root,
        connected_handlers: Vec::new(),
    }
}

fn build_tool_burst_page() -> ToolBurstPageWidgets {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("tool-call-group");

    let header_button = gtk::Button::new();
    header_button.add_css_class("flat");
    header_button.add_css_class("tool-call-group-header-button");
    header_button.set_halign(gtk::Align::Fill);

    let header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);

    let arrow_icon = gtk::Image::from_icon_name(TOOL_BURST_ARROW_COLLAPSED);
    arrow_icon.set_valign(gtk::Align::Start);
    arrow_icon.add_css_class("tool-call-group-arrow");
    header_row.append(&arrow_icon);

    // AdwWrapBox (not GtkFlowBox): pills flow like words in a wrapping label,
    // so they reflow onto extra lines on narrow windows. GtkFlowBox here caused
    // a session-open freeze inside the recycled GtkListView rows.
    let header_wrap = adw::WrapBox::new();
    header_wrap.set_child_spacing(8);
    header_wrap.set_line_spacing(4);
    header_wrap.set_hexpand(true);
    header_wrap.add_css_class("tool-call-group-header");
    header_row.append(&header_wrap);
    header_button.set_child(Some(&header_row));
    root.append(&header_button);

    let children = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let revealer = gtk::Revealer::new();
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    revealer.set_child(Some(&children));
    root.append(&revealer);

    ToolBurstPageWidgets {
        root,
        header_button,
        arrow_icon,
        header_wrap,
        revealer,
        children,
        reveal_binding: None,
        children_built_for: Rc::new(Cell::new(None)),
        connected_handlers: Vec::new(),
    }
}

fn build_subagent_page() -> SubagentPageWidgets {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    SubagentPageWidgets {
        root,
        connected_handlers: Vec::new(),
    }
}

fn cleanup_transcript_row_widgets(widgets: &mut TranscriptRowWidgets) {
    clear_box_children(&widgets.tool_burst.children);
}

struct ToolCallPageContentRefs {
    root: gtk::Box,
    inspect_button: gtk::Button,
    reasoning_button: Option<gtk::Button>,
}

fn build_tool_call_page_content(
    init: &crate::ui::session_detail::transcript::item_init::ToolCallItemInit,
    highlight_query: Option<&str>,
) -> ToolCallPageContentRefs {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("tool-call-row");
    root.set_margin_top(2);
    root.set_margin_bottom(2);

    let header = build_tool_call_row_header(ToolCallRowHeaderInit {
        tool_name: &init.tool_name,
        status: init.status,
        duration_ms: init.duration_ms,
        highlight_query,
        reasoning_preview: init.reasoning_preview,
    });
    let row = header.row;
    let reasoning_button = header.reasoning_button;

    let inspect = gtk::Button::new();
    inspect.set_icon_name("view-reveal-symbolic");
    inspect.add_css_class("flat");
    inspect.set_tooltip_text(Some("Inspect tool call"));

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    row.append(&spacer);
    row.append(&inspect);
    root.append(&row);

    if let Some(preview) = init.displayed_preview() {
        let preview_label = gtk::Label::new(None);
        preview_label.add_css_class("caption");
        preview_label.add_css_class("dim-label");
        preview_label.add_css_class("preview-label");
        preview_label.set_halign(gtk::Align::Start);
        preview_label.set_xalign(0.0);
        preview_label.set_margin_start(32);
        preview_label.set_margin_bottom(4);
        preview_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        if let Some(query) = highlight_query {
            let (markup, _) = highlight::highlight_text(preview, query);
            preview_label.set_markup(&markup);
        } else {
            preview_label.set_label(preview);
        }
        root.append(&preview_label);
    }

    ToolCallPageContentRefs {
        root,
        inspect_button: inspect,
        reasoning_button,
    }
}

fn burst_with_highlight_query(
    burst: &crate::ui::session_detail::transcript::item_init::ToolBurstItemInit,
    highlight_query: Option<&str>,
) -> crate::ui::session_detail::transcript::item_init::ToolBurstItemInit {
    let mut burst = burst.clone();
    let highlight_query = highlight_query.map(str::to_string);
    for tool_call in &mut burst.tool_calls {
        tool_call.highlight_query = highlight_query.clone();
    }
    burst
}

struct SubagentPageContentRefs {
    root: gtk::Box,
    inspect_button: gtk::Button,
    reasoning_button: Option<gtk::Button>,
}

fn build_subagent_page_content(
    init: &crate::ui::session_detail::transcript::item_init::SubagentItemInit,
) -> SubagentPageContentRefs {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    root.add_css_class("subagent-row");
    root.set_margin_start(8);
    root.set_margin_end(4);
    root.set_margin_top(4);
    root.set_margin_bottom(4);

    let icon = gtk::Image::new();
    icon.set_icon_name(Some(TOOL_ICONS.agent));
    icon.set_pixel_size(16);
    root.append(&icon);

    let title = gtk::Label::new(Some(&init.title));
    title.set_halign(gtk::Align::Start);
    title.set_hexpand(false);
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    root.append(&title);

    let reasoning_button = append_reasoning_pill(&root, &init.reasoning_preview);

    let inspect = gtk::Button::new();
    inspect.set_icon_name("view-reveal-symbolic");
    inspect.add_css_class("flat");
    inspect.set_tooltip_text(Some("Inspect subagent"));

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    root.append(&spacer);
    root.append(&inspect);

    SubagentPageContentRefs {
        root,
        inspect_button: inspect,
        reasoning_button,
    }
}

fn rebuild_tool_burst_header(
    header: &adw::WrapBox,
    burst: &crate::ui::session_detail::transcript::item_init::ToolBurstItemInit,
) {
    header.remove_all();

    for (name, count) in &burst.category_counts {
        let pill_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        pill_box.add_css_class("pill");
        pill_box.add_css_class("tool-call-group-pill");

        let pill_icon = gtk::Image::new();
        pill_icon.set_icon_name(Some(tool_name_icon(name, &TOOL_ICONS)));
        pill_icon.set_pixel_size(12);
        pill_box.append(&pill_icon);

        let pill_label = gtk::Label::new(Some(&format!("{count} {name}")));
        pill_box.append(&pill_label);

        header.append(&pill_box);
    }

    if let Some(ms) = burst.total_duration_ms {
        let duration = gtk::Label::new(Some(&format_duration_ms(ms)));
        duration.add_css_class("caption");
        duration.add_css_class("dim-label");
        header.append(&duration);
    }

    if let Some(reasoning_label) = format_reasoning_burst_label(
        burst.visible_reasoning_child_count,
        burst.encrypted_only_child_count,
    ) {
        if burst.visible_reasoning_child_count > 0 {
            let badge = gtk::Label::new(Some(&reasoning_label));
            badge.add_css_class("pill");
            badge.add_css_class("tool-call-group-pill");
            badge.add_css_class("reasoning-pill");
            header.append(&badge);
        } else {
            let badge = encrypted_reasoning_pill_with_label(&reasoning_label);
            badge.add_css_class("tool-call-group-pill");
            header.append(&badge);
        }
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
        let badge = gtk::Label::new(Some(&format!("{burst_match_count} matches")));
        badge.add_css_class("pill");
        badge.add_css_class("accent");
        badge.add_css_class("tool-call-group-pill");
        badge.update_property(&[gtk::accessible::Property::Label(
            &format_tool_burst_match_badge_accessible_label(burst_match_count),
        )]);
        header.append(&badge);
    }
}

fn set_tool_burst_expanded_state(button: &gtk::Button, arrow_icon: &gtk::Image, expanded: bool) {
    button.update_state(&[gtk::accessible::State::Expanded(Some(expanded))]);
    arrow_icon.set_icon_name(Some(if expanded {
        TOOL_BURST_ARROW_EXPANDED
    } else {
        TOOL_BURST_ARROW_COLLAPSED
    }));
}

fn build_tool_burst_children_if_needed(
    children: &gtk::Box,
    children_built_for: &Rc<Cell<Option<usize>>>,
    burst: &crate::ui::session_detail::transcript::item_init::ToolBurstItemInit,
    sender: &relm4::Sender<SessionDetailMsg>,
    item_index: usize,
) {
    if children_built_for.get() == Some(item_index) {
        return;
    }

    clear_box_children(children);
    populate_tool_burst_children(children, burst, sender, item_index);
    children_built_for.set(Some(item_index));
}

fn disconnect_handlers(handlers: &mut Vec<(gtk::glib::Object, gtk::glib::SignalHandlerId)>) {
    while let Some((object, id)) = handlers.pop() {
        object.disconnect(id);
    }
}

fn clear_box_children(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use chrono::Utc;
    use relm4::adw;
    use relm4::binding::Binding;
    use relm4::gtk;
    use relm4::gtk::prelude::*;
    use relm4::typed_view::list::RelmListItem;

    use crate::models::{MessagePreview, ReasoningPreview, Role, ToolCallStatus};
    use crate::ui::session_detail::SessionDetailMsg;
    use crate::ui::session_detail::transcript::item_data::{
        TranscriptItemData, TranscriptItemKind,
    };
    use crate::ui::session_detail::transcript::item_init::{
        MessageItemInit, SubagentItemInit, ToolBurstItemInit, ToolCallItemInit, TranscriptItemInit,
        TranscriptRowBuildKind,
    };

    fn message_init() -> MessageItemInit {
        MessageItemInit {
            item_index: 1,
            transcript_item_index: 11,
            preview: MessagePreview {
                session_id: "session-1".to_string(),
                message_index: 4,
                role: Role::Assistant,
                content_preview: "hello".to_string(),
                content_len: 5,
                timestamp: Utc::now(),
                model: Some("gpt-5.4".to_string()),
                reasoning_preview: ReasoningPreview::default(),
            },
            highlight_query: Some("hello".to_string()),
            db_path: Arc::new(PathBuf::from("/tmp/typed-transcript-row.db")),
        }
    }

    fn message_init_with_visible_reasoning() -> MessageItemInit {
        let mut init = message_init();
        init.preview.reasoning_preview = ReasoningPreview {
            has_reasoning: true,
            has_visible_reasoning: true,
            encrypted_only: false,
        };
        init
    }

    fn truncated_message_init() -> MessageItemInit {
        let mut init = message_init();
        init.preview.content_len = 64;
        init
    }

    fn tool_call_init() -> ToolCallItemInit {
        ToolCallItemInit {
            item_index: 2,
            transcript_item_index: 12,
            session_id: "session-1".to_string(),
            tool_call_id: "call-1".to_string(),
            tool_name: "Read".to_string(),
            status: ToolCallStatus::Completed,
            preview: Some("src/ui/transcript_item_init.rs:1-20".to_string()),
            summary: None,
            duration_ms: Some(15),
            highlight_query: Some("read".to_string()),
            reasoning_preview: ReasoningPreview::default(),
        }
    }

    fn subagent_init() -> SubagentItemInit {
        SubagentItemInit {
            item_index: 3,
            transcript_item_index: 13,
            session_id: "session-1".to_string(),
            subagent_id: "agent-1".to_string(),
            title: "Explore".to_string(),
            reasoning_preview: ReasoningPreview::default(),
        }
    }

    fn tool_burst_init(default_expanded: bool) -> ToolBurstItemInit {
        ToolBurstItemInit {
            item_index: 4,
            tool_calls: vec![tool_call_init()],
            category_counts: vec![("Read".to_string(), 1)],
            error_count: 0,
            total_duration_ms: Some(15),
            match_count: 2,
            child_match_counts: vec![2],
            visible_reasoning_child_count: 0,
            encrypted_only_child_count: 0,
            default_expanded,
        }
    }

    fn count_box_children(container: &gtk::Box) -> usize {
        let mut count = 0;
        let mut child = container.first_child();
        while let Some(current) = child {
            count += 1;
            child = current.next_sibling();
        }
        count
    }

    fn tool_burst_header_button(root: &gtk::Box) -> gtk::Button {
        root.first_child()
            .expect("tool burst header button")
            .downcast::<gtk::Button>()
            .expect("tool burst header button")
    }

    fn tool_burst_revealer(root: &gtk::Box) -> gtk::Revealer {
        root.last_child()
            .expect("tool burst revealer")
            .downcast::<gtk::Revealer>()
            .expect("tool burst revealer")
    }

    fn tool_burst_children_box(root: &gtk::Box) -> gtk::Box {
        tool_burst_revealer(root)
            .child()
            .expect("tool burst children")
            .downcast::<gtk::Box>()
            .expect("tool burst children box")
    }

    fn message_reasoning_button(root: &gtk::Box) -> gtk::Button {
        let header = root
            .first_child()
            .expect("message header")
            .downcast::<gtk::Box>()
            .expect("message header box");
        let reasoning_box = header
            .last_child()
            .expect("message reasoning box")
            .downcast::<gtk::Box>()
            .expect("message reasoning box");

        reasoning_box
            .first_child()
            .expect("message reasoning button")
            .downcast::<gtk::Button>()
            .expect("message reasoning button")
    }

    fn message_expand_button(root: &gtk::Box) -> gtk::Button {
        root.last_child()
            .expect("message expand button")
            .downcast::<gtk::Button>()
            .expect("message expand button")
    }

    fn message_content_text(widgets: &super::MessagePageWidgets) -> String {
        widgets
            .content
            .first_child()
            .expect("message content child")
            .downcast::<gtk::Label>()
            .expect("message content label")
            .label()
            .to_string()
    }

    #[test]
    fn transcript_item_kind_maps_to_build_kind() {
        let cases = [
            (
                TranscriptItemKind::Message(message_init()),
                TranscriptRowBuildKind::Message,
            ),
            (
                TranscriptItemKind::ToolCall(tool_call_init()),
                TranscriptRowBuildKind::ToolCall,
            ),
            (
                TranscriptItemKind::ToolBurst(ToolBurstItemInit {
                    item_index: 4,
                    tool_calls: vec![tool_call_init()],
                    category_counts: vec![("Read".to_string(), 1)],
                    error_count: 0,
                    total_duration_ms: Some(15),
                    match_count: 2,
                    child_match_counts: vec![2],
                    visible_reasoning_child_count: 0,
                    encrypted_only_child_count: 0,
                    default_expanded: false,
                }),
                TranscriptRowBuildKind::ToolBurst,
            ),
            (
                TranscriptItemKind::Subagent(subagent_init()),
                TranscriptRowBuildKind::Subagent,
            ),
        ];

        for (kind, expected) in cases {
            assert_eq!(TranscriptRowBuildKind::from(&kind), expected);
        }
    }

    #[gtk::test]
    fn transcript_item_data_noop_binders_accept_each_page_widget_type() {
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();

        let mut message = TranscriptItemData::from_init(
            TranscriptItemInit::Message(message_init()),
            sender.clone(),
        );
        let mut tool_call = TranscriptItemData::from_init(
            TranscriptItemInit::ToolCall(tool_call_init()),
            sender.clone(),
        );
        let mut tool_burst = TranscriptItemData::from_init(
            TranscriptItemInit::ToolBurst(ToolBurstItemInit {
                item_index: 4,
                tool_calls: vec![tool_call_init()],
                category_counts: vec![("Read".to_string(), 1)],
                error_count: 0,
                total_duration_ms: Some(15),
                match_count: 2,
                child_match_counts: vec![2],
                visible_reasoning_child_count: 0,
                encrypted_only_child_count: 0,
                default_expanded: false,
            }),
            sender.clone(),
        );
        let mut subagent =
            TranscriptItemData::from_init(TranscriptItemInit::Subagent(subagent_init()), sender);

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut message_widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        message.bind(&mut message_widgets, &mut root);
        message.unbind(&mut message_widgets, &mut root);

        let (_, mut tool_call_widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tool_call.bind(&mut tool_call_widgets, &mut root);
        tool_call.unbind(&mut tool_call_widgets, &mut root);

        let (_, mut tool_burst_widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tool_burst.bind(&mut tool_burst_widgets, &mut root);
        tool_burst.unbind(&mut tool_burst_widgets, &mut root);

        let (_, mut subagent_widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        subagent.bind(&mut subagent_widgets, &mut root);
        subagent.unbind(&mut subagent_widgets, &mut root);
    }

    #[gtk::test]
    fn bind_shows_selected_stack_page() {
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
        let mut message =
            TranscriptItemData::from_init(TranscriptItemInit::Message(message_init()), sender);

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        message.bind(&mut widgets, &mut root);

        assert_eq!(
            widgets.stack.visible_child_name().as_deref(),
            Some("message")
        );
        assert!(
            widgets
                .stack
                .visible_child()
                .expect("visible child")
                .is_visible()
        );
    }

    #[gtk::test]
    fn message_reasoning_button_emits_inspect_reasoning() {
        let (sender, receiver) = relm4::channel::<SessionDetailMsg>();
        let mut message = TranscriptItemData::from_init(
            TranscriptItemInit::Message(message_init_with_visible_reasoning()),
            sender,
        );

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        message.bind(&mut widgets, &mut root);

        assert!(matches!(
            gtk::glib::MainContext::default()
                .block_on(receiver.recv())
                .expect("row built"),
            SessionDetailMsg::RowBuilt { .. }
        ));

        message_reasoning_button(&widgets.message.root).emit_clicked();

        assert!(matches!(
            gtk::glib::MainContext::default()
                .block_on(receiver.recv())
                .expect("reasoning inspect message"),
            SessionDetailMsg::InspectReasoning(11)
        ));
    }

    #[gtk::test]
    fn message_expand_button_toggles_in_place_and_requests_full_content() {
        let (sender, receiver) = relm4::channel::<SessionDetailMsg>();
        let mut message = TranscriptItemData::from_init(
            TranscriptItemInit::Message(truncated_message_init()),
            sender,
        );

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        message.bind(&mut widgets, &mut root);

        assert!(matches!(
            gtk::glib::MainContext::default()
                .block_on(receiver.recv())
                .expect("row built"),
            SessionDetailMsg::RowBuilt { .. }
        ));

        let expand_button = message_expand_button(&widgets.message.root);

        assert!(expand_button.is_visible());
        assert_eq!(expand_button.label().as_deref(), Some("Show full message"));

        expand_button.emit_clicked();

        // The row toggles and re-renders in place (no list-model replacement),
        // so the expansion state and button label update synchronously...
        assert!(message.expanded.get());
        assert_eq!(expand_button.label().as_deref(), Some("Collapse"));

        // ...and the only message emitted is the off-thread full-content request,
        // because the body has not been loaded yet.
        assert!(matches!(
            gtk::glib::MainContext::default()
                .block_on(receiver.recv())
                .expect("full content request"),
            SessionDetailMsg::RequestMessageFullContent { item_index: 1 }
        ));

        // Collapsing again toggles back in place and emits nothing further.
        expand_button.emit_clicked();
        assert!(!message.expanded.get());
        assert_eq!(expand_button.label().as_deref(), Some("Show full message"));
    }

    #[gtk::test]
    fn expanded_message_renders_loaded_full_content() {
        let (sender, receiver) = relm4::channel::<SessionDetailMsg>();
        let mut init = truncated_message_init();
        init.preview.role = Role::User;
        init.preview.content_preview = "short preview".to_string();
        init.preview.content_len = 42;
        let mut message = TranscriptItemData::from_init(TranscriptItemInit::Message(init), sender);
        message.expanded.set(true);
        *message.full_content.borrow_mut() = Some("loaded full message body".to_string());

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        message.bind(&mut widgets, &mut root);
        let _ = gtk::glib::MainContext::default().block_on(receiver.recv());

        assert_eq!(
            message_content_text(&widgets.message),
            "loaded full message body"
        );
    }

    #[gtk::test]
    fn message_bind_applies_role_border_classes() {
        let (sender, receiver) = relm4::channel::<SessionDetailMsg>();
        let mut init = truncated_message_init();
        init.preview.role = Role::User;
        let mut message = TranscriptItemData::from_init(TranscriptItemInit::Message(init), sender);

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        message.bind(&mut widgets, &mut root);
        let _ = gtk::glib::MainContext::default().block_on(receiver.recv());

        assert!(widgets.message.root.has_css_class("message-row"));
        assert!(widgets.message.root.has_css_class("role-user"));
        assert!(widgets.message.role_label.has_css_class("heading"));
        assert!(widgets.message.role_label.has_css_class("role-user"));
    }

    #[gtk::test]
    fn tool_call_page_uses_data_level_highlight_query() {
        let (sender, receiver) = relm4::channel::<SessionDetailMsg>();
        let mut tool_call = TranscriptItemData::from_init(
            TranscriptItemInit::ToolCall(ToolCallItemInit {
                highlight_query: None,
                ..tool_call_init()
            }),
            sender,
        );
        tool_call.highlight_query = Some("Read".to_string());

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        tool_call.bind(&mut widgets, &mut root);
        let _ = gtk::glib::MainContext::default().block_on(receiver.recv());
        let row = widgets
            .tool_call
            .root
            .first_child()
            .expect("tool call content")
            .downcast::<gtk::Box>()
            .expect("tool call content box")
            .first_child()
            .expect("tool call row")
            .downcast::<gtk::Box>()
            .expect("tool call row box");
        let name_label = collect_box_children(&row)[1]
            .clone()
            .downcast::<gtk::Label>()
            .expect("tool name label");

        assert!(name_label.uses_markup());
    }

    #[gtk::test]
    fn tool_call_name_and_preview_truncate_overflow() {
        let (sender, receiver) = relm4::channel::<SessionDetailMsg>();
        let mut tool_call = TranscriptItemData::from_init(
            TranscriptItemInit::ToolCall(ToolCallItemInit {
                tool_name: "VeryLongToolNameThatShouldNotOverflowTheRow".to_string(),
                preview: Some("a very long preview that should stay on one line".to_string()),
                ..tool_call_init()
            }),
            sender,
        );

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        tool_call.bind(&mut widgets, &mut root);
        let _ = gtk::glib::MainContext::default().block_on(receiver.recv());

        let content = widgets
            .tool_call
            .root
            .first_child()
            .expect("tool call content")
            .downcast::<gtk::Box>()
            .expect("tool call content box");
        let row = content
            .first_child()
            .expect("tool call row")
            .downcast::<gtk::Box>()
            .expect("tool call row box");
        let name_label = collect_box_children(&row)[1]
            .clone()
            .downcast::<gtk::Label>()
            .expect("tool name label");
        let preview_label = collect_box_children(&content)[1]
            .clone()
            .downcast::<gtk::Label>()
            .expect("tool call preview label");

        assert_eq!(
            name_label.ellipsize(),
            gtk::pango::EllipsizeMode::End,
            "tool name must truncate instead of overflowing"
        );
        assert_eq!(
            preview_label.ellipsize(),
            gtk::pango::EllipsizeMode::End,
            "tool preview must truncate instead of wrapping"
        );
        assert!(
            !preview_label.wraps(),
            "tool preview must stay on a single line"
        );
    }

    fn collect_box_children(container: &gtk::Box) -> Vec<gtk::Widget> {
        let mut children = Vec::new();
        let mut child = container.first_child();
        while let Some(current) = child {
            child = current.next_sibling();
            children.push(current);
        }
        children
    }

    #[gtk::test]
    fn tool_call_page_keeps_metadata_left_of_inspect_button() {
        let (sender, receiver) = relm4::channel::<SessionDetailMsg>();
        let mut tool_call = TranscriptItemData::from_init(
            TranscriptItemInit::ToolCall(ToolCallItemInit {
                reasoning_preview: ReasoningPreview {
                    has_reasoning: true,
                    has_visible_reasoning: false,
                    encrypted_only: true,
                },
                ..tool_call_init()
            }),
            sender,
        );

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        tool_call.bind(&mut widgets, &mut root);
        let _ = gtk::glib::MainContext::default().block_on(receiver.recv());

        let row = widgets
            .tool_call
            .root
            .first_child()
            .expect("tool call content")
            .downcast::<gtk::Box>()
            .expect("tool call content box")
            .first_child()
            .expect("tool call row")
            .downcast::<gtk::Box>()
            .expect("tool call row box");
        let children = collect_box_children(&row);

        assert!(
            children[0].is::<gtk::Image>(),
            "tool call row must lead with a category icon"
        );

        let name_label = children[1]
            .clone()
            .downcast::<gtk::Label>()
            .expect("tool name label");
        assert!(
            !name_label.hexpands(),
            "tool name label must not absorb horizontal slack"
        );

        let spacer = &children[children.len() - 2];
        assert!(
            spacer.is::<gtk::Box>() && spacer.hexpands(),
            "a hexpanding spacer must sit just before the trailing inspect button"
        );

        let inspect = children
            .last()
            .expect("inspect button")
            .clone()
            .downcast::<gtk::Button>()
            .expect("inspect button");
        assert!(!inspect.hexpands());
    }

    #[gtk::test]
    fn subagent_page_keeps_metadata_left_of_inspect_button() {
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
        let mut subagent = TranscriptItemData::from_init(
            TranscriptItemInit::Subagent(SubagentItemInit {
                reasoning_preview: ReasoningPreview {
                    has_reasoning: true,
                    has_visible_reasoning: false,
                    encrypted_only: true,
                },
                ..subagent_init()
            }),
            sender,
        );

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        subagent.bind(&mut widgets, &mut root);

        let row = widgets
            .subagent
            .root
            .first_child()
            .expect("subagent content")
            .downcast::<gtk::Box>()
            .expect("subagent row box");
        let children = collect_box_children(&row);

        assert!(
            children[0].is::<gtk::Image>(),
            "subagent row must lead with the agent icon"
        );

        let title_label = children[1]
            .clone()
            .downcast::<gtk::Label>()
            .expect("subagent title label");
        assert!(
            !title_label.hexpands(),
            "subagent title label must not absorb horizontal slack"
        );

        let spacer = &children[children.len() - 2];
        assert!(
            spacer.is::<gtk::Box>() && spacer.hexpands(),
            "a hexpanding spacer must sit just before the trailing inspect button"
        );
    }

    #[gtk::test]
    fn subagent_title_truncates_overflow() {
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
        let mut subagent = TranscriptItemData::from_init(
            TranscriptItemInit::Subagent(SubagentItemInit {
                title: "Very long subagent title that should not overflow the row".to_string(),
                ..subagent_init()
            }),
            sender,
        );

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        subagent.bind(&mut widgets, &mut root);

        let row = widgets
            .subagent
            .root
            .first_child()
            .expect("subagent content")
            .downcast::<gtk::Box>()
            .expect("subagent row box");
        let title_label = collect_box_children(&row)[1]
            .clone()
            .downcast::<gtk::Label>()
            .expect("subagent title label");

        assert_eq!(
            title_label.ellipsize(),
            gtk::pango::EllipsizeMode::End,
            "subagent title must truncate instead of overflowing"
        );
    }

    #[gtk::test]
    fn tool_call_page_indents_preview_to_align_with_name() {
        let (sender, receiver) = relm4::channel::<SessionDetailMsg>();
        let mut tool_call =
            TranscriptItemData::from_init(TranscriptItemInit::ToolCall(tool_call_init()), sender);

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        tool_call.bind(&mut widgets, &mut root);
        let _ = gtk::glib::MainContext::default().block_on(receiver.recv());

        let content = widgets
            .tool_call
            .root
            .first_child()
            .expect("tool call content")
            .downcast::<gtk::Box>()
            .expect("tool call content box");
        let preview = collect_box_children(&content)[1]
            .clone()
            .downcast::<gtk::Label>()
            .expect("tool call preview label");

        assert!(
            preview.has_css_class("preview-label"),
            "preview must carry the preview-label style class"
        );
        assert_eq!(
            preview.margin_start(),
            32,
            "preview must be indented to align with the tool name, not the icon"
        );
    }

    #[gtk::test]
    fn tool_burst_header_shows_and_toggles_expand_chevron() {
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
        let mut burst = TranscriptItemData::from_init(
            TranscriptItemInit::ToolBurst(tool_burst_init(false)),
            sender,
        );

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        burst.bind(&mut widgets, &mut root);

        let arrow = &widgets.tool_burst.arrow_icon;
        assert!(arrow.has_css_class("tool-call-group-arrow"));
        assert_eq!(arrow.icon_name().as_deref(), Some("pan-end-symbolic"));

        tool_burst_header_button(&widgets.tool_burst.root).emit_clicked();
        assert_eq!(arrow.icon_name().as_deref(), Some("pan-down-symbolic"));
    }

    #[gtk::test]
    fn tool_burst_header_uses_wrap_box_so_pills_can_wrap() {
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
        let mut burst = TranscriptItemData::from_init(
            TranscriptItemInit::ToolBurst(tool_burst_init(false)),
            sender,
        );

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        burst.bind(&mut widgets, &mut root);

        let header_container = tool_burst_header_button(&widgets.tool_burst.root)
            .child()
            .expect("header row")
            .downcast::<gtk::Box>()
            .expect("header row box")
            .last_child()
            .expect("header pill container");
        assert!(
            header_container.is::<adw::WrapBox>(),
            "tool burst pills must live in an AdwWrapBox so they wrap onto \
             multiple lines instead of overflowing on narrow windows"
        );
    }

    #[gtk::test]
    fn tool_burst_header_renders_category_pill_with_icon() {
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
        let mut burst = TranscriptItemData::from_init(
            TranscriptItemInit::ToolBurst(tool_burst_init(false)),
            sender,
        );

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        burst.bind(&mut widgets, &mut root);

        let pill_box = widgets
            .tool_burst
            .header_wrap
            .first_child()
            .expect("category pill box")
            .downcast::<gtk::Box>()
            .expect("pill box");
        assert!(pill_box.has_css_class("tool-call-group-pill"));

        let pill_icon = pill_box
            .first_child()
            .expect("pill icon")
            .downcast::<gtk::Image>()
            .expect("pill icon");
        assert!(
            pill_icon.icon_name().is_some(),
            "category pill must carry a tool icon"
        );
    }

    #[gtk::test]
    fn burst_bind_builds_children_only_when_expanded() {
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();

        let mut collapsed = TranscriptItemData::from_init(
            TranscriptItemInit::ToolBurst(tool_burst_init(false)),
            sender.clone(),
        );
        let (_, mut collapsed_widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        collapsed.bind(&mut collapsed_widgets, &mut root);

        let collapsed_root = &collapsed_widgets.tool_burst.root;
        let collapsed_header = tool_burst_header_button(collapsed_root);
        let collapsed_revealer = tool_burst_revealer(collapsed_root);
        let collapsed_children = tool_burst_children_box(collapsed_root);

        assert!(!collapsed.expanded.get());
        assert!(!collapsed_revealer.reveals_child());
        assert_eq!(count_box_children(&collapsed_children), 0);

        collapsed_header.emit_clicked();

        assert!(collapsed.expanded.get());
        assert!(collapsed_revealer.reveals_child());
        assert_eq!(count_box_children(&collapsed_children), 1);

        let mut expanded = TranscriptItemData::from_init(
            TranscriptItemInit::ToolBurst(tool_burst_init(true)),
            sender,
        );
        let (_, mut expanded_widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        expanded.bind(&mut expanded_widgets, &mut root);

        let expanded_root = &expanded_widgets.tool_burst.root;
        let expanded_revealer = tool_burst_revealer(expanded_root);
        let expanded_children = tool_burst_children_box(expanded_root);

        assert!(expanded.expanded.get());
        assert!(expanded_revealer.reveals_child());
        assert_eq!(count_box_children(&expanded_children), 1);
    }

    #[gtk::test]
    fn tool_burst_bind_applies_group_border_class() {
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
        let mut burst = TranscriptItemData::from_init(
            TranscriptItemInit::ToolBurst(tool_burst_init(false)),
            sender,
        );
        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        burst.bind(&mut widgets, &mut root);

        assert!(widgets.tool_burst.root.has_css_class("tool-call-group"));
    }

    #[gtk::test]
    fn unbind_disconnects_handlers_and_reveal_binding() {
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
        let mut burst = TranscriptItemData::from_init(
            TranscriptItemInit::ToolBurst(tool_burst_init(false)),
            sender,
        );

        let list_item: gtk::ListItem = gtk::glib::Object::builder().build();
        let (_, mut widgets) = TranscriptItemData::setup(&list_item);
        let mut root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        burst.bind(&mut widgets, &mut root);

        let burst_root = &widgets.tool_burst.root;
        let header = tool_burst_header_button(burst_root);
        let revealer = tool_burst_revealer(burst_root);
        let children = tool_burst_children_box(burst_root);

        header.emit_clicked();
        assert!(burst.expanded.get());
        assert!(revealer.reveals_child());
        assert_eq!(count_box_children(&children), 1);

        burst.unbind(&mut widgets, &mut root);

        assert_eq!(count_box_children(&children), 0);

        let reveal_after_unbind = revealer.reveals_child();
        burst.expanded.set(false);
        assert_eq!(revealer.reveals_child(), reveal_after_unbind);

        let expanded_after_unbind = burst.expanded.get();
        header.emit_clicked();
        assert_eq!(burst.expanded.get(), expanded_after_unbind);
        assert_eq!(count_box_children(&children), 0);
    }
}
