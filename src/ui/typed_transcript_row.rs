use gtk::prelude::*;
use relm4::{gtk, typed_view::list::RelmListItem};

use crate::ui::transcript_item_data::{TranscriptItemData, TranscriptItemKind};
use crate::ui::transcript_row::TranscriptRowBuildKind;

const MESSAGE_PAGE_NAME: &str = "message";
const TOOL_CALL_PAGE_NAME: &str = "tool-call";
const TOOL_BURST_PAGE_NAME: &str = "tool-burst";
const SUBAGENT_PAGE_NAME: &str = "subagent";

pub struct TranscriptRowWidgets {
    stack: gtk::Stack,
    message: MessagePageWidgets,
    tool_call: ToolCallPageWidgets,
    tool_burst: ToolBurstPageWidgets,
    subagent: SubagentPageWidgets,
}

pub struct MessagePageWidgets {
    root: gtk::Box,
}

pub struct ToolCallPageWidgets {
    root: gtk::Box,
}

pub struct ToolBurstPageWidgets {
    root: gtk::Box,
    children: gtk::Box,
}

pub struct SubagentPageWidgets {
    root: gtk::Box,
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

    fn bind(&mut self, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        let kind = TranscriptRowBuildKind::from(&self.kind);
        widgets.stack.set_visible_child_name(kind.page_name());

        match kind {
            TranscriptRowBuildKind::Message => self.bind_message_page(&mut widgets.message),
            TranscriptRowBuildKind::ToolCall => self.bind_tool_call_page(&mut widgets.tool_call),
            TranscriptRowBuildKind::ToolBurst => self.bind_tool_burst_page(&mut widgets.tool_burst),
            TranscriptRowBuildKind::Subagent => self.bind_subagent_page(&mut widgets.subagent),
        }
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
    pub(crate) fn bind_message_page(&self, _widgets: &mut MessagePageWidgets) {}

    pub(crate) fn bind_tool_call_page(&self, _widgets: &mut ToolCallPageWidgets) {}

    pub(crate) fn bind_tool_burst_page(&self, _widgets: &mut ToolBurstPageWidgets) {}

    pub(crate) fn bind_subagent_page(&self, _widgets: &mut SubagentPageWidgets) {}

    pub(crate) fn unbind_message_page(&self, _widgets: &mut MessagePageWidgets) {}

    pub(crate) fn unbind_tool_call_page(&self, _widgets: &mut ToolCallPageWidgets) {}

    pub(crate) fn unbind_tool_burst_page(&self, _widgets: &mut ToolBurstPageWidgets) {}

    pub(crate) fn unbind_subagent_page(&self, _widgets: &mut SubagentPageWidgets) {}
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
    root.set_visible(false);
    MessagePageWidgets { root }
}

fn build_tool_call_page() -> ToolCallPageWidgets {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_visible(false);
    ToolCallPageWidgets { root }
}

fn build_tool_burst_page() -> ToolBurstPageWidgets {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_visible(false);
    let children = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&children);
    ToolBurstPageWidgets { root, children }
}

fn build_subagent_page() -> SubagentPageWidgets {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_visible(false);
    SubagentPageWidgets { root }
}

fn cleanup_transcript_row_widgets(widgets: &mut TranscriptRowWidgets) {
    clear_box_children(&widgets.tool_burst.children);
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
    use relm4::gtk;
    use relm4::typed_view::list::RelmListItem;

    use crate::models::{MessagePreview, ReasoningPreview, Role, ToolCallStatus};
    use crate::ui::session_detail::SessionDetailMsg;
    use crate::ui::transcript_item_data::{TranscriptItemData, TranscriptItemKind};
    use crate::ui::transcript_row::{
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

    fn tool_call_init() -> ToolCallItemInit {
        ToolCallItemInit {
            item_index: 2,
            transcript_item_index: 12,
            session_id: "session-1".to_string(),
            tool_call_id: "call-1".to_string(),
            tool_name: "Read".to_string(),
            status: ToolCallStatus::Completed,
            preview: Some("src/ui/transcript_row.rs:1-20".to_string()),
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
}
