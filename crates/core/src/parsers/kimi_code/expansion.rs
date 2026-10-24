use std::collections::HashMap;

use crate::models::{TranscriptItem, TranscriptItemKind};
use crate::parsers::ParsedSession;

pub(super) struct Expansion {
    pub item_index: i64,
    pub subagent_ids: Vec<String>,
}

pub(super) fn expand_transcript(parsed: &mut ParsedSession, expansions: Vec<Expansion>) {
    let mut expansions_by_index = HashMap::new();
    for expansion in expansions {
        let Some(item) = parsed
            .transcript_items
            .iter()
            .find(|item| item.item_index == expansion.item_index)
        else {
            tracing::debug!(
                item_index = expansion.item_index,
                "ignoring Kimi transcript expansion for missing item"
            );
            continue;
        };
        if item.kind != TranscriptItemKind::ToolCall {
            tracing::debug!(
                item_index = expansion.item_index,
                "ignoring Kimi transcript expansion for non-tool-call item"
            );
            continue;
        }
        if !expansion.subagent_ids.is_empty() {
            expansions_by_index.insert(expansion.item_index, expansion.subagent_ids);
        }
    }

    if expansions_by_index.is_empty() {
        return;
    }

    let added_items = expansions_by_index.values().map(Vec::len).sum::<usize>();
    let mut expanded_items = Vec::with_capacity(parsed.transcript_items.len() + added_items);
    let mut old_to_new = HashMap::with_capacity(parsed.transcript_items.len());
    for item in parsed.transcript_items.drain(..) {
        let first_new_index = expanded_items.len() as i64;
        old_to_new.insert(item.item_index, first_new_index);
        if let Some(subagent_ids) = expansions_by_index.remove(&item.item_index) {
            for subagent_id in subagent_ids {
                expanded_items.push(TranscriptItem {
                    session_id: item.session_id.clone(),
                    item_index: expanded_items.len() as i64,
                    kind: TranscriptItemKind::Subagent,
                    message_index: None,
                    tool_call_id: None,
                    subagent_id: Some(subagent_id),
                    context_id: None,
                });
            }
        } else {
            let mut item = item;
            item.item_index = first_new_index;
            expanded_items.push(item);
        }
    }
    parsed.transcript_items = expanded_items;

    for attachment in &mut parsed.reasoning_attachments {
        if let Some(new_index) = old_to_new.get(&attachment.transcript_item_index) {
            attachment.transcript_item_index = *new_index;
        }
    }
    for entry in &mut parsed.context_entries {
        if let Some(new_index) = old_to_new.get(&entry.item_index) {
            entry.item_index = *new_index;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        ContextEntry, ContextKind, ReasoningAttachment, TranscriptItem, TranscriptItemKind,
    };
    use crate::parsers::ParsedSession;
    use chrono::{DateTime, Utc};

    fn parsed_session(transcript_items: Vec<TranscriptItem>) -> ParsedSession {
        ParsedSession {
            session: super::super::placeholder_session(
                "session",
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            ),
            messages: Vec::new(),
            tool_calls: Vec::new(),
            subagents: Vec::new(),
            transcript_items,
            reasoning_attachments: Vec::new(),
            context_entries: Vec::new(),
            token_usage: None,
            agent_path: None,
        }
    }

    fn message(item_index: i64, message_index: i64) -> TranscriptItem {
        TranscriptItem {
            session_id: "session".to_string(),
            item_index,
            kind: TranscriptItemKind::Message,
            message_index: Some(message_index),
            tool_call_id: None,
            subagent_id: None,
            context_id: None,
        }
    }

    fn tool_call(item_index: i64, tool_call_id: &str) -> TranscriptItem {
        TranscriptItem {
            session_id: "session".to_string(),
            item_index,
            kind: TranscriptItemKind::ToolCall,
            message_index: None,
            tool_call_id: Some(tool_call_id.to_string()),
            subagent_id: None,
            context_id: None,
        }
    }

    fn reasoning(transcript_item_index: i64) -> ReasoningAttachment {
        ReasoningAttachment {
            session_id: "session".to_string(),
            transcript_item_index,
            visible_text: Some(format!("reasoning at {transcript_item_index}")),
            summary_text: None,
            has_encrypted_content: false,
            source_model: None,
            source_timestamp: None,
        }
    }

    fn context(item_index: i64) -> ContextEntry {
        ContextEntry {
            id: format!("context-{item_index}"),
            session_id: "session".to_string(),
            kind: ContextKind::System,
            name: None,
            text: format!("context at {item_index}"),
            timestamp: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            item_index,
        }
    }

    #[test]
    fn expands_one_item_into_consecutive_subagents() {
        let mut parsed = parsed_session(vec![
            message(0, 0),
            tool_call(1, "swarm-call"),
            message(2, 1),
            tool_call(3, "other-call"),
        ]);
        parsed.reasoning_attachments = vec![reasoning(1), reasoning(3)];
        parsed.context_entries = vec![context(2)];

        expand_transcript(
            &mut parsed,
            vec![Expansion {
                item_index: 1,
                subagent_ids: vec!["s0".into(), "s1".into(), "s2".into()],
            }],
        );

        assert_eq!(
            parsed
                .transcript_items
                .iter()
                .map(|item| item.kind.clone())
                .collect::<Vec<_>>(),
            vec![
                TranscriptItemKind::Message,
                TranscriptItemKind::Subagent,
                TranscriptItemKind::Subagent,
                TranscriptItemKind::Subagent,
                TranscriptItemKind::Message,
                TranscriptItemKind::ToolCall,
            ]
        );
        assert_eq!(
            parsed
                .transcript_items
                .iter()
                .map(|item| item.item_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5]
        );
        let subagents = &parsed.transcript_items[1..4];
        assert_eq!(
            subagents
                .iter()
                .map(|item| item.subagent_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("s0"), Some("s1"), Some("s2")]
        );
        assert!(subagents.iter().all(|item| {
            item.session_id == "session"
                && item.message_index.is_none()
                && item.tool_call_id.is_none()
                && item.context_id.is_none()
        }));
        assert_eq!(
            parsed
                .reasoning_attachments
                .iter()
                .map(|attachment| attachment.transcript_item_index)
                .collect::<Vec<_>>(),
            vec![1, 5]
        );
        assert_eq!(parsed.context_entries[0].item_index, 4);
    }

    #[test]
    fn no_expansion_is_identity() {
        let mut parsed = parsed_session(vec![message(0, 0), tool_call(1, "call"), message(2, 1)]);
        parsed.reasoning_attachments = vec![reasoning(1)];
        parsed.context_entries = vec![context(2)];
        let before_items: Vec<_> = parsed
            .transcript_items
            .iter()
            .map(|item| {
                (
                    item.session_id.clone(),
                    item.item_index,
                    item.kind.clone(),
                    item.message_index,
                    item.tool_call_id.clone(),
                    item.subagent_id.clone(),
                    item.context_id.clone(),
                )
            })
            .collect();
        let before_reasoning = parsed.reasoning_attachments.clone();
        let before_context = parsed.context_entries.clone();

        expand_transcript(&mut parsed, Vec::new());

        let after_items: Vec<_> = parsed
            .transcript_items
            .iter()
            .map(|item| {
                (
                    item.session_id.clone(),
                    item.item_index,
                    item.kind.clone(),
                    item.message_index,
                    item.tool_call_id.clone(),
                    item.subagent_id.clone(),
                    item.context_id.clone(),
                )
            })
            .collect();
        assert_eq!(after_items, before_items);
        assert_eq!(parsed.reasoning_attachments, before_reasoning);
        assert_eq!(parsed.context_entries, before_context);
    }

    #[test]
    fn invariant_holds_after_two_expansions() {
        let mut parsed = parsed_session(vec![
            message(0, 0),
            tool_call(1, "swarm-one"),
            message(2, 1),
            tool_call(3, "swarm-two"),
            message(4, 2),
        ]);
        parsed.reasoning_attachments = vec![reasoning(0), reasoning(1), reasoning(3), reasoning(4)];
        parsed.context_entries = vec![context(1), context(2), context(3), context(4)];

        expand_transcript(
            &mut parsed,
            vec![
                Expansion {
                    item_index: 1,
                    subagent_ids: vec!["first-a".into(), "first-b".into()],
                },
                Expansion {
                    item_index: 3,
                    subagent_ids: vec!["second-a".into(), "second-b".into()],
                },
            ],
        );

        let indexes: Vec<_> = parsed
            .transcript_items
            .iter()
            .map(|item| item.item_index)
            .collect();
        assert_eq!(
            indexes,
            (0..parsed.transcript_items.len() as i64).collect::<Vec<_>>()
        );
        assert!(
            parsed
                .reasoning_attachments
                .iter()
                .all(|attachment| { indexes.contains(&attachment.transcript_item_index) })
        );
        assert!(
            parsed
                .context_entries
                .iter()
                .all(|entry| indexes.contains(&entry.item_index))
        );
    }

    #[test]
    fn ignores_expansions_for_missing_or_non_tool_call_items() {
        let mut parsed = parsed_session(vec![message(0, 0), tool_call(1, "call")]);
        let before_items = parsed.transcript_items.clone();

        expand_transcript(
            &mut parsed,
            vec![
                Expansion {
                    item_index: 0,
                    subagent_ids: vec!["not-a-tool".into()],
                },
                Expansion {
                    item_index: 9,
                    subagent_ids: vec!["missing".into()],
                },
            ],
        );

        assert_eq!(
            parsed
                .transcript_items
                .iter()
                .map(|item| (
                    item.item_index,
                    item.kind.clone(),
                    item.message_index,
                    item.tool_call_id.clone()
                ))
                .collect::<Vec<_>>(),
            before_items
                .iter()
                .map(|item| (
                    item.item_index,
                    item.kind.clone(),
                    item.message_index,
                    item.tool_call_id.clone()
                ))
                .collect::<Vec<_>>()
        );
    }
}
