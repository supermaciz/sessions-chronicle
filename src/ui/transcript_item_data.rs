use std::fmt;

use relm4::Sender;
use relm4::binding::{Binding, BoolBinding};

use crate::ui::session_detail::SessionDetailMsg;
use crate::ui::transcript_row::{
    MessageItemInit, SubagentItemInit, ToolBurstItemInit, ToolCallItemInit, TranscriptItemInit,
};

#[derive(Clone)]
pub struct TranscriptItemData {
    pub item_index: usize,
    pub kind: TranscriptItemKind,
    pub expanded: BoolBinding,
    pub sender: Sender<SessionDetailMsg>,
}

#[derive(Clone)]
pub enum TranscriptItemKind {
    Message(MessageItemInit),
    ToolCall(ToolCallItemInit),
    ToolBurst(ToolBurstItemInit),
    Subagent(SubagentItemInit),
}

impl fmt::Debug for TranscriptItemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(_) => f.write_str("Message"),
            Self::ToolCall(_) => f.write_str("ToolCall"),
            Self::ToolBurst(_) => f.write_str("ToolBurst"),
            Self::Subagent(_) => f.write_str("Subagent"),
        }
    }
}

impl fmt::Debug for TranscriptItemData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TranscriptItemData")
            .field("item_index", &self.item_index)
            .field("kind", &self.kind)
            .field("expanded", &self.expanded.get())
            .finish_non_exhaustive()
    }
}

impl TranscriptItemData {
    pub fn from_init(init: TranscriptItemInit, sender: Sender<SessionDetailMsg>) -> Self {
        let item_index = init.item_index();
        let (kind, expanded) = match init {
            TranscriptItemInit::Message(message) => (
                TranscriptItemKind::Message(message),
                BoolBinding::new(false),
            ),
            TranscriptItemInit::ToolCall(tool_call) => (
                TranscriptItemKind::ToolCall(tool_call),
                BoolBinding::new(false),
            ),
            TranscriptItemInit::ToolBurst(tool_burst) => {
                let expanded = BoolBinding::new(tool_burst.default_expanded);
                (TranscriptItemKind::ToolBurst(tool_burst), expanded)
            }
            TranscriptItemInit::Subagent(subagent) => (
                TranscriptItemKind::Subagent(subagent),
                BoolBinding::new(false),
            ),
        };

        Self {
            item_index,
            kind,
            expanded,
            sender,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use chrono::Utc;
    use relm4::binding::Binding;

    use super::{TranscriptItemData, TranscriptItemKind};
    use crate::models::{MessagePreview, ReasoningPreview, Role};
    use crate::ui::session_detail::SessionDetailMsg;
    use crate::ui::transcript_row::{MessageItemInit, TranscriptItemInit};

    #[test]
    fn from_init_preserves_message_identity_and_highlight() {
        let preview = MessagePreview {
            session_id: "session-1".to_string(),
            message_index: 3,
            role: Role::Assistant,
            content_preview: "hello".to_string(),
            content_len: 5,
            timestamp: Utc::now(),
            model: Some("gpt-5.4".to_string()),
            reasoning_preview: ReasoningPreview::default(),
        };
        let init = MessageItemInit {
            item_index: 7,
            transcript_item_index: 42,
            preview: preview.clone(),
            highlight_query: Some("hello".to_string()),
            db_path: Arc::new(PathBuf::from("/tmp/transcript-item-data.db")),
        };
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();

        let data = TranscriptItemData::from_init(TranscriptItemInit::Message(init), sender);

        assert_eq!(data.item_index, 7);
        match &data.kind {
            TranscriptItemKind::Message(message) => {
                assert_eq!(message.transcript_item_index, 42);
                assert_eq!(message.highlight_query.as_deref(), Some("hello"));
                assert_eq!(message.preview.session_id, preview.session_id);
                assert_eq!(message.preview.message_index, preview.message_index);
                assert_eq!(message.preview.content_preview, preview.content_preview);
                assert_eq!(message.preview.model, preview.model);
            }
            other => panic!("expected message item, got {other:?}"),
        }
    }

    #[test]
    fn cloned_data_shares_expansion_binding() {
        let init = MessageItemInit {
            item_index: 2,
            transcript_item_index: 8,
            preview: MessagePreview {
                session_id: "session-2".to_string(),
                message_index: 1,
                role: Role::User,
                content_preview: "question".to_string(),
                content_len: 8,
                timestamp: Utc::now(),
                model: None,
                reasoning_preview: ReasoningPreview::default(),
            },
            highlight_query: None,
            db_path: Arc::new(PathBuf::from("/tmp/transcript-item-data.db")),
        };
        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();

        let data = TranscriptItemData::from_init(TranscriptItemInit::Message(init), sender);
        let cloned = data.clone();

        assert!(!data.expanded.get());
        cloned.expanded.set(true);
        assert!(data.expanded.get());

        data.expanded.set(false);
        assert!(!cloned.expanded.get());
    }
}
