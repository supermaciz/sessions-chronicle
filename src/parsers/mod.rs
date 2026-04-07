pub mod claude_code;
pub mod codex;
pub mod mistral_vibe;
pub mod model;
pub mod opencode;

use chrono::{DateTime, Utc};

use crate::models::{
    Message, ReasoningAttachment, Role, Subagent, TokenUsage, ToolCall, TranscriptItem,
};

const FIRST_PROMPT_MAX_CHARS: usize = 200;

/// Accumulates reasoning parts (visible text, summary, encrypted flag) across
/// multiple content blocks until the next transcript item flushes them into a
/// single [`ReasoningAttachment`].  Shared by all four parsers.
#[derive(Debug, Clone, Default)]
pub(crate) struct PendingReasoning {
    pub visible_text: Option<String>,
    pub summary_text: Option<String>,
    pub has_encrypted_content: bool,
    pub source_model: Option<String>,
    pub source_timestamp: Option<DateTime<Utc>>,
}

impl PendingReasoning {
    pub fn is_empty(&self) -> bool {
        self.visible_text.is_none() && self.summary_text.is_none() && !self.has_encrypted_content
    }

    pub fn merge(&mut self, next: PendingReasoning) {
        if let Some(text) = next.visible_text {
            match &mut self.visible_text {
                Some(current) => {
                    if !current.is_empty() {
                        current.push('\n');
                    }
                    current.push_str(&text);
                }
                None => self.visible_text = Some(text),
            }
        }

        if let Some(summary) = next.summary_text {
            match &mut self.summary_text {
                Some(current) => {
                    if !current.is_empty() {
                        current.push('\n');
                    }
                    current.push_str(&summary);
                }
                None => self.summary_text = Some(summary),
            }
        }

        self.has_encrypted_content |= next.has_encrypted_content;
        if self.source_model.is_none() {
            self.source_model = next.source_model;
        }
        if self.source_timestamp.is_none() {
            self.source_timestamp = next.source_timestamp;
        }
    }

    pub fn into_attachment(
        self,
        session_id: &str,
        transcript_item_index: i64,
    ) -> ReasoningAttachment {
        ReasoningAttachment {
            session_id: session_id.to_string(),
            transcript_item_index,
            visible_text: self.visible_text,
            summary_text: self.summary_text,
            has_encrypted_content: self.has_encrypted_content,
            source_model: self.source_model,
            source_timestamp: self.source_timestamp,
        }
    }
}

/// All data produced by parsing a single session file.
#[derive(Debug)]
pub struct ParsedSession {
    pub session: crate::models::Session,
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCall>,
    pub subagents: Vec<Subagent>,
    pub transcript_items: Vec<TranscriptItem>,
    pub reasoning_attachments: Vec<ReasoningAttachment>,
    pub token_usage: Option<TokenUsage>,
}

pub(crate) fn extract_first_prompt(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .filter(|message| message.role == Role::User)
        .map(|message| normalize_prompt(&message.content))
        .find(|prompt| !prompt.is_empty())
}

pub(crate) fn normalize_prompt(content: &str) -> String {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&normalized, FIRST_PROMPT_MAX_CHARS)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut truncated: String = value.chars().take(max_chars).collect();
    truncated.push('\u{2026}');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn message(index: usize, role: Role, content: &str) -> Message {
        Message {
            session_id: "session-1".to_string(),
            index,
            role,
            content: content.to_string(),
            timestamp: Utc::now(),
            model: None,
        }
    }

    #[test]
    fn extract_first_prompt_skips_whitespace_only_user_message() {
        let messages = vec![
            message(0, Role::User, "   \n\t   "),
            message(1, Role::User, "  second   prompt  "),
        ];

        assert_eq!(
            extract_first_prompt(&messages),
            Some("second prompt".to_string())
        );
    }

    #[test]
    fn normalize_prompt_collapses_whitespace() {
        let messages = vec![message(
            0,
            Role::User,
            "  hello\n\n   world\tfrom   parser  ",
        )];

        assert_eq!(
            extract_first_prompt(&messages),
            Some("hello world from parser".to_string())
        );
    }

    #[test]
    fn normalize_prompt_truncates_at_200_and_201_char_boundaries() {
        let exactly_200 = "a".repeat(200);
        let exactly_201 = "b".repeat(201);

        assert_eq!(normalize_prompt(&exactly_200), exactly_200);

        let mut expected = "b".repeat(200);
        expected.push('\u{2026}');
        assert_eq!(normalize_prompt(&exactly_201), expected);
    }

    #[test]
    fn normalize_prompt_truncates_multibyte_chars_safely() {
        let multibyte = "é".repeat(201);

        let truncated = normalize_prompt(&multibyte);
        let mut expected = "é".repeat(200);
        expected.push('\u{2026}');
        assert_eq!(truncated, expected);
        assert_eq!(truncated.chars().count(), 201); // 200 chars + ellipsis
    }
}
