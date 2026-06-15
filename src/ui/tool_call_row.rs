use gtk::prelude::*;
use relm4::gtk;

use crate::icon_names;
use crate::models::{ReasoningPreview, ToolCallStatus, ToolCategoryIcons, tool_name_icon};
use crate::ui::format::{format_duration_ms, tool_status_css_class, tool_status_label};
use crate::ui::highlight;

pub(crate) const TOOL_ICONS: ToolCategoryIcons = ToolCategoryIcons {
    read: icon_names::TEXT_SNIPPET,
    edit: icon_names::EDIT_DOCUMENT,
    command: icon_names::TERMINAL,
    search: icon_names::SEARCH,
    agent: icon_names::SMART_TOY,
    web: icon_names::EARTH,
    plan: icon_names::CLIPBOARD_TASK_LIST_REGULAR,
    skill: icon_names::DOCUMENT_ONE_PAGE_SPARKLE_REGULAR,
    user_input: icon_names::CHAT_BUBBLES_QUESTION_REGULAR,
    other: icon_names::BUILD,
};

pub(crate) struct ToolCallRowHeaderInit<'a> {
    pub tool_name: &'a str,
    pub status: ToolCallStatus,
    pub duration_ms: Option<i64>,
    pub highlight_query: Option<&'a str>,
    pub reasoning_preview: ReasoningPreview,
}

pub(crate) struct ToolCallRowHeaderWidgets {
    pub row: gtk::Box,
    pub reasoning_button: Option<gtk::Button>,
}

pub(crate) fn build_tool_call_row_header(
    init: ToolCallRowHeaderInit<'_>,
) -> ToolCallRowHeaderWidgets {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_margin_start(8);
    row.set_margin_end(4);
    row.set_margin_top(4);
    row.set_margin_bottom(4);

    let icon = gtk::Image::new();
    icon.set_icon_name(Some(tool_name_icon(init.tool_name, &TOOL_ICONS)));
    icon.set_pixel_size(16);
    row.append(&icon);

    let name_label = gtk::Label::new(None);
    name_label.add_css_class("monospace");
    name_label.set_halign(gtk::Align::Start);
    name_label.set_hexpand(false);
    name_label.set_xalign(0.0);
    name_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    if let Some(query) = init.highlight_query {
        let (markup, _) = highlight::highlight_text(init.tool_name, query);
        name_label.set_markup(&markup);
    } else {
        name_label.set_label(init.tool_name);
    }
    row.append(&name_label);

    let status_label = gtk::Label::new(Some(tool_status_label(init.status)));
    status_label.add_css_class("caption");
    status_label.add_css_class(tool_status_css_class(init.status));
    row.append(&status_label);

    if let Some(ms) = init.duration_ms {
        let duration = gtk::Label::new(Some(&format_duration_ms(ms)));
        duration.add_css_class("caption");
        duration.add_css_class("dim-label");
        row.append(&duration);
    }

    let reasoning_button = if init.reasoning_preview.has_visible_reasoning {
        let button = gtk::Button::with_label("Thinking");
        button.add_css_class("flat");
        button.add_css_class("pill");
        button.add_css_class("reasoning-pill");
        row.append(&button);
        Some(button)
    } else if init.reasoning_preview.encrypted_only {
        row.append(&encrypted_reasoning_pill());
        None
    } else {
        None
    };

    ToolCallRowHeaderWidgets {
        row,
        reasoning_button,
    }
}

pub(crate) fn encrypted_reasoning_pill() -> gtk::Box {
    encrypted_reasoning_pill_with_label("Thinking (encrypted)")
}

pub(crate) fn encrypted_reasoning_pill_with_label(text: &str) -> gtk::Box {
    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    pill.add_css_class("pill");
    pill.add_css_class("reasoning-pill-encrypted");

    let label = gtk::Label::new(Some(text));
    label.set_halign(gtk::Align::Center);
    pill.append(&label);

    pill
}
