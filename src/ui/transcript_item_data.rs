use std::fmt;

use relm4::Sender;
use relm4::binding::{Binding, BoolBinding};

use crate::ui::session_detail::SessionDetailMsg;
use crate::ui::transcript_row::{
    MessageItemInit, SubagentItemInit, ToolBurstItemInit, ToolCallItemInit, TranscriptItemInit,
    count_tool_call_matches,
};

#[derive(Clone)]
pub struct TranscriptItemData {
    pub item_index: usize,
    pub transcript_item_index: Option<i64>,
    pub kind: TranscriptItemKind,
    pub expanded: BoolBinding,
    pub full_content: Option<String>,
    pub highlight_query: Option<String>,
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
            .field("has_full_content", &self.full_content.is_some())
            .field("highlight_query", &self.highlight_query)
            .finish_non_exhaustive()
    }
}

impl TranscriptItemData {
    pub fn from_init(init: TranscriptItemInit, sender: Sender<SessionDetailMsg>) -> Self {
        let item_index = init.item_index();
        let (kind, expanded, transcript_item_index, highlight_query) = match init {
            TranscriptItemInit::Message(message) => (
                TranscriptItemKind::Message(message.clone()),
                BoolBinding::new(false),
                Some(message.transcript_item_index),
                message.highlight_query,
            ),
            TranscriptItemInit::ToolCall(tool_call) => (
                TranscriptItemKind::ToolCall(tool_call.clone()),
                BoolBinding::new(false),
                Some(tool_call.transcript_item_index),
                tool_call.highlight_query,
            ),
            TranscriptItemInit::ToolBurst(tool_burst) => {
                let expanded = BoolBinding::new(tool_burst.default_expanded);
                let transcript_item_index = tool_burst
                    .tool_calls
                    .first()
                    .map(|tc| tc.transcript_item_index);
                let highlight_query = tool_burst
                    .tool_calls
                    .first()
                    .and_then(|tc| tc.highlight_query.clone());
                (
                    TranscriptItemKind::ToolBurst(tool_burst),
                    expanded,
                    transcript_item_index,
                    highlight_query,
                )
            }
            TranscriptItemInit::Subagent(subagent) => (
                TranscriptItemKind::Subagent(subagent.clone()),
                BoolBinding::new(false),
                Some(subagent.transcript_item_index),
                None,
            ),
        };

        Self {
            item_index,
            transcript_item_index,
            kind,
            expanded,
            full_content: None,
            highlight_query,
            sender,
        }
    }

    /// Apply a new transcript search query to this row.
    ///
    /// Tool bursts cache per-child and aggregate match counts that feed the
    /// collapsed group badge, so the query is propagated to each child and
    /// those counts are recomputed; otherwise the badge would keep showing
    /// stale counts from whatever query was active when the row was built.
    pub fn apply_highlight_query(&mut self, query: Option<String>) {
        self.highlight_query = query.clone();

        if let TranscriptItemKind::ToolBurst(burst) = &mut self.kind {
            for tool_call in &mut burst.tool_calls {
                tool_call.highlight_query = query.clone();
            }
            burst.child_match_counts = burst
                .tool_calls
                .iter()
                .map(count_tool_call_matches)
                .collect();
            burst.match_count = burst.child_match_counts.iter().sum();
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
        assert_eq!(data.transcript_item_index, Some(42));
        assert_eq!(data.highlight_query.as_deref(), Some("hello"));
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
    fn apply_highlight_query_recomputes_tool_burst_match_counts() {
        use crate::models::ToolCallStatus;
        use crate::ui::transcript_row::{ToolCallItemInit, build_tool_burst_init};

        let tool_call = ToolCallItemInit {
            item_index: 1,
            transcript_item_index: 10,
            session_id: "session-1".to_string(),
            tool_call_id: "call-1".to_string(),
            tool_name: "Read".to_string(),
            status: ToolCallStatus::Completed,
            preview: Some("src/needle.rs:1-20".to_string()),
            summary: None,
            duration_ms: None,
            highlight_query: None,
            reasoning_preview: ReasoningPreview::default(),
        };
        let burst = build_tool_burst_init(4, vec![tool_call], false);
        assert_eq!(
            burst.match_count, 0,
            "burst starts with no query, no matches"
        );

        let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
        let mut data = TranscriptItemData::from_init(TranscriptItemInit::ToolBurst(burst), sender);

        data.apply_highlight_query(Some("needle".to_string()));

        assert_eq!(data.highlight_query.as_deref(), Some("needle"));
        match &data.kind {
            TranscriptItemKind::ToolBurst(burst) => {
                assert_eq!(
                    burst.tool_calls[0].highlight_query.as_deref(),
                    Some("needle"),
                    "child tool calls must carry the new query"
                );
                assert_eq!(
                    burst.child_match_counts,
                    vec![1],
                    "child match counts must be recomputed against the new query"
                );
                assert_eq!(
                    burst.match_count, 1,
                    "burst aggregate match count must reflect the new query"
                );
            }
            other => panic!("expected tool burst, got {other:?}"),
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
