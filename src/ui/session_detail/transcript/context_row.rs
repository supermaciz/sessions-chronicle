//! Compact, keyboard-operable transcript row for typed context entries
//! (system messages, injected skills, and compaction summaries).
//!
//! Context entries are neither user nor assistant messages: their full text is
//! stored outside the message tables, so it never reaches search or token
//! accounting. The row starts collapsed and loads its body lazily — only when
//! the user expands it — reusing the typed-row `expanded` / `full_content` /
//! `content_revision` lifecycle shared with message rows.

use gtk::prelude::*;
use relm4::binding::Binding;
use relm4::gtk;

use crate::models::{ContextKind, ContextPreview};
use crate::ui::markdown;
use crate::ui::session_detail::SessionDetailMsg;
use crate::ui::session_detail::transcript::item_data::{TranscriptItemData, TranscriptItemKind};

const CONTEXT_ARROW_COLLAPSED: &str = "pan-end-symbolic";
const CONTEXT_ARROW_EXPANDED: &str = "pan-down-symbolic";

pub(super) struct ContextPageWidgets {
    pub(super) root: gtk::Box,
    pub(super) header_button: gtk::Button,
    pub(super) arrow_icon: gtk::Image,
    pub(super) kind_label: gtk::Label,
    pub(super) revealer: gtk::Revealer,
    pub(super) body: gtk::Box,
    pub(super) reveal_binding: Option<gtk::glib::Binding>,
    pub(super) connected_handlers: Vec<(gtk::glib::Object, gtk::glib::SignalHandlerId)>,
}

/// Human-readable label distinguishing a context entry's kind.
///
/// Skill entries append their optional structured name when present, so a
/// "brainstorming" skill reads as "Skill · brainstorming" rather than a bare
/// "Skill".
pub(crate) fn context_kind_label(preview: &ContextPreview) -> String {
    match preview.kind {
        ContextKind::System => gettextrs::gettext("System"),
        ContextKind::Skill => {
            let skill = gettextrs::gettext("Skill");
            match preview.name.as_deref() {
                Some(name) if !name.is_empty() => format!("{skill} · {name}"),
                _ => skill,
            }
        }
        ContextKind::CompactionSummary => gettextrs::gettext("Compaction summary"),
    }
}

pub(super) fn build_context_page() -> ContextPageWidgets {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("context-row");

    let header_button = gtk::Button::new();
    header_button.add_css_class("flat");
    header_button.add_css_class("context-header-button");
    header_button.set_halign(gtk::Align::Fill);

    let header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header_row.set_margin_start(4);
    header_row.set_margin_end(4);

    let arrow_icon = gtk::Image::from_icon_name(CONTEXT_ARROW_COLLAPSED);
    arrow_icon.set_valign(gtk::Align::Start);
    arrow_icon.add_css_class("context-arrow");
    header_row.append(&arrow_icon);

    let kind_label = gtk::Label::new(None);
    kind_label.add_css_class("caption");
    kind_label.add_css_class("dim-label");
    kind_label.set_halign(gtk::Align::Start);
    kind_label.set_xalign(0.0);
    kind_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    header_row.append(&kind_label);

    header_button.set_child(Some(&header_row));
    root.append(&header_button);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
    body.set_margin_start(28);
    body.set_margin_end(8);
    body.set_margin_bottom(8);

    let revealer = gtk::Revealer::new();
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    revealer.set_child(Some(&body));
    root.append(&revealer);

    ContextPageWidgets {
        root,
        header_button,
        arrow_icon,
        kind_label,
        revealer,
        body,
        reveal_binding: None,
        connected_handlers: Vec::new(),
    }
}

pub(super) fn bind_context_page(item: &TranscriptItemData, widgets: &mut ContextPageWidgets) {
    let TranscriptItemKind::Context(context) = &item.kind else {
        return;
    };

    widgets
        .kind_label
        .set_label(&context_kind_label(&context.preview));
    set_context_expanded_state(
        &widgets.header_button,
        &widgets.arrow_icon,
        item.expanded.get(),
    );
    render_context_body(
        &widgets.body,
        &item.full_content.borrow(),
        item.expanded.get(),
    );

    // Re-render the body in place whenever the content revision is bumped
    // (expand/collapse toggle, full-content arrival, load-failure rollback),
    // and keep the arrow icon plus the accessible expanded state in sync with
    // `expanded`.
    let body = widgets.body.clone();
    let expanded = item.expanded.clone();
    let full_content = item.full_content.clone();
    let header_button = widgets.header_button.clone();
    let arrow_icon = widgets.arrow_icon.clone();
    let revision_handler =
        item.content_revision
            .connect_notify_local(Some("value"), move |_, _| {
                render_context_body(&body, &full_content.borrow(), expanded.get());
                set_context_expanded_state(&header_button, &arrow_icon, expanded.get());
            });
    widgets
        .connected_handlers
        .push((item.content_revision.clone().upcast(), revision_handler));

    widgets.reveal_binding = Some(
        item.expanded
            .bind_property("value", &widgets.revealer, "reveal-child")
            .sync_create()
            .build(),
    );

    // Enter/Space activate the focused header button through `clicked`.
    let sender = item.sender.clone();
    let item_index = item.item_index;
    let expanded = item.expanded.clone();
    let full_content = item.full_content.clone();
    let content_revision = item.content_revision.clone();
    let header_id = widgets.header_button.connect_clicked(move |_| {
        let now_expanded = !expanded.get();
        expanded.set(now_expanded);
        if now_expanded && full_content.borrow().is_none() {
            sender.emit(SessionDetailMsg::RequestContextFullContent { item_index });
        }
        content_revision.set(!content_revision.get());
    });
    widgets
        .connected_handlers
        .push((widgets.header_button.clone().upcast(), header_id));
}

pub(super) fn unbind_context_page(widgets: &mut ContextPageWidgets) {
    disconnect_handlers(&mut widgets.connected_handlers);
    if let Some(binding) = widgets.reveal_binding.take() {
        binding.unbind();
    }
    widgets.kind_label.set_label("");
    clear_box_children(&widgets.body);
    set_context_expanded_state(&widgets.header_button, &widgets.arrow_icon, false);
}

fn render_context_body(body: &gtk::Box, full_content: &Option<String>, expanded: bool) {
    clear_box_children(body);
    if expanded && let Some(content) = full_content.as_deref() {
        let (widget, _) = markdown::render_markdown(content, None);
        body.append(&widget);
    }
}

fn set_context_expanded_state(button: &gtk::Button, arrow_icon: &gtk::Image, expanded: bool) {
    button.update_state(&[gtk::accessible::State::Expanded(Some(expanded))]);
    arrow_icon.set_icon_name(Some(if expanded {
        CONTEXT_ARROW_EXPANDED
    } else {
        CONTEXT_ARROW_COLLAPSED
    }));
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
    use relm4::binding::Binding;
    use relm4::gtk;
    use relm4::gtk::prelude::*;

    use super::*;
    use crate::models::{ContextKind, ContextPreview};
    use crate::ui::session_detail::SessionDetailMsg;
    use crate::ui::session_detail::transcript::item_init::{ContextItemInit, TranscriptItemInit};

    fn context_item_init(
        item_index: usize,
        id: &str,
        kind: ContextKind,
        name: Option<&str>,
    ) -> ContextItemInit {
        ContextItemInit {
            item_index,
            transcript_item_index: 5,
            session_id: "session-1".to_string(),
            preview: ContextPreview {
                id: id.to_string(),
                kind,
                name: name.map(str::to_string),
                timestamp: Utc::now(),
            },
            db_path: Arc::new(PathBuf::from("/tmp/context-row-test.db")),
        }
    }

    fn context_item_data(
        sender: relm4::Sender<SessionDetailMsg>,
        init: ContextItemInit,
    ) -> TranscriptItemData {
        TranscriptItemData::from_init(TranscriptItemInit::Context(init), sender)
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

    #[test]
    fn context_kind_label_distinguishes_kinds() {
        let system = context_item_init(0, "ctx-1", ContextKind::System, None);
        assert_eq!(context_kind_label(&system.preview), "System");

        let skill_named = context_item_init(1, "ctx-2", ContextKind::Skill, Some("brainstorming"));
        assert_eq!(
            context_kind_label(&skill_named.preview),
            "Skill · brainstorming"
        );

        let skill_unnamed = context_item_init(2, "ctx-3", ContextKind::Skill, None);
        assert_eq!(context_kind_label(&skill_unnamed.preview), "Skill");

        let compaction = context_item_init(3, "ctx-4", ContextKind::CompactionSummary, None);
        assert_eq!(
            context_kind_label(&compaction.preview),
            "Compaction summary"
        );
    }

    #[gtk::test]
    fn context_starts_collapsed_and_loads_on_keyboard_expansion() {
        let (sender, receiver) = relm4::channel::<SessionDetailMsg>();
        let item = context_item_data(
            sender,
            context_item_init(0, "ctx-1", ContextKind::System, None),
        );

        assert!(!item.expanded.get());
        assert!(item.full_content.borrow().is_none());
        assert!(item.highlight_query.is_none());

        let mut widgets = build_context_page();
        bind_context_page(&item, &mut widgets);

        // No body content and no request before expansion.
        assert_eq!(count_box_children(&widgets.body), 0);
        assert_eq!(
            widgets.arrow_icon.icon_name().as_deref(),
            Some(CONTEXT_ARROW_COLLAPSED),
            "the arrow and its accessible Expanded state start collapsed"
        );

        // Enter/Space activate a focused button through the `clicked` signal.
        widgets.header_button.emit_clicked();

        assert!(item.expanded.get());
        assert_eq!(
            widgets.arrow_icon.icon_name().as_deref(),
            Some(CONTEXT_ARROW_EXPANDED),
            "the arrow (and its accessible Expanded state, updated in the same \
             call) must flip on keyboard activation"
        );
        // The only message emitted is the off-thread lazy body request.
        assert!(matches!(
            gtk::glib::MainContext::default()
                .block_on(receiver.recv())
                .expect("context body request after activation"),
            SessionDetailMsg::RequestContextFullContent { item_index: 0 }
        ));

        // Collapsing toggles back without requesting again.
        widgets.header_button.emit_clicked();
        assert!(!item.expanded.get());
        assert_eq!(
            widgets.arrow_icon.icon_name().as_deref(),
            Some(CONTEXT_ARROW_COLLAPSED),
            "the arrow and accessible state must collapse again"
        );
    }

    #[gtk::test]
    fn context_recycled_row_disconnects_handlers() {
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
        let item = context_item_data(
            sender,
            context_item_init(0, "ctx-old", ContextKind::System, None),
        );

        let mut widgets = build_context_page();
        bind_context_page(&item, &mut widgets);
        assert!(!widgets.connected_handlers.is_empty());

        let old_header = widgets.header_button.clone();

        unbind_context_page(&mut widgets);
        assert!(widgets.connected_handlers.is_empty());
        assert!(widgets.reveal_binding.is_none());
        assert_eq!(count_box_children(&widgets.body), 0);

        // The stale header no longer drives the previous item.
        old_header.emit_clicked();
        assert!(!item.expanded.get());

        // Rebind the recycled widgets to a different context: only the new item
        // responds and its request carries the new item index.
        let (sender2, receiver2) = relm4::channel::<SessionDetailMsg>();
        let item2 = context_item_data(
            sender2,
            context_item_init(1, "ctx-new", ContextKind::Skill, Some("brainstorming")),
        );
        bind_context_page(&item2, &mut widgets);

        widgets.header_button.emit_clicked();
        assert!(item2.expanded.get());
        assert!(matches!(
            gtk::glib::MainContext::default()
                .block_on(receiver2.recv())
                .expect("rebound context body request"),
            SessionDetailMsg::RequestContextFullContent { item_index: 1 }
        ));
    }

    #[gtk::test]
    fn context_expansion_renders_markdown_body() {
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
        let item = context_item_data(
            sender,
            context_item_init(0, "ctx-1", ContextKind::CompactionSummary, None),
        );
        item.expanded.set(true);
        *item.full_content.borrow_mut() = Some("**bold** and `code`".to_string());

        let mut widgets = build_context_page();
        bind_context_page(&item, &mut widgets);

        // Markdown renders as a widget tree (a container box), not a raw label,
        // proving the shared markdown helper was used rather than plain text.
        let container = widgets
            .body
            .first_child()
            .expect("markdown container")
            .downcast::<gtk::Box>()
            .expect("markdown container is a box");
        let prose_label = container
            .first_child()
            .expect("prose label")
            .downcast::<gtk::Label>()
            .expect("prose label");
        assert!(prose_label.is_selectable());
        assert!(prose_label.uses_markup());

        unbind_context_page(&mut widgets);
        assert_eq!(count_box_children(&widgets.body), 0);
    }
}
