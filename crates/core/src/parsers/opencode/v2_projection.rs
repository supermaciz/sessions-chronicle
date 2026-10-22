//! Projection of native OpenCode V2 `session_message` rows into the shared
//! `ParsedSession` domain model.

// `V2MessageRow` / `V2Projection` are consumed by Task 3 (tool-call/subagent
// projection) and the parser tests, but are not referenced by production code
// in this commit. The `seq` field is carried for ordering and Task 3.
#![allow(dead_code)]

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::path::Path;

use crate::models::{
    AiAssistant, ContextEntry, ContextKind, Message, ReasoningAttachment, Role, Session,
    SessionEndingStatus, TokenUsage, TranscriptItem, TranscriptItemKind,
};
use crate::parsers::{ParsedSession, PendingReasoning};

use super::{ParseError, SessionMetadata, extract_tokens};

/// One native `session_message` row, ordered by `seq, id` by the reader.
pub(crate) struct V2MessageRow {
    pub id: String,
    pub seq: i64,
    pub timestamp: DateTime<Utc>,
    /// The `type` column (`user`, `assistant`, `system`, `skill`, `compaction`, …).
    pub kind: String,
    /// The parsed `data` JSON payload. `Value::Null` marks a row whose payload
    /// the reader could not parse (only ever surfaced for corrupt user rows).
    pub data: Value,
}

/// Accumulates the shared `ParsedSession` while the reader streams rows through
/// [`V2Projection::push`], in `seq, id` order. `finish` assembles and validates
/// the result. Tool-call/subagent content is projected in Task 3; this
/// projection keeps the seam and leaves the `{new,push,finish}` signatures stable.
pub(crate) struct V2Projection {
    metadata: SessionMetadata,
    db_path: String,
    messages: Vec<Message>,
    context_entries: Vec<ContextEntry>,
    transcript_items: Vec<TranscriptItem>,
    reasoning_attachments: Vec<ReasoningAttachment>,
    pending_reasoning: PendingReasoning,
    token_input: i64,
    token_output: i64,
    token_reasoning: i64,
    token_cache_read: i64,
    token_cache_write: i64,
    saw_tokens: bool,
    has_user_message: bool,
    saw_corrupt_user: bool,
}

impl V2Projection {
    pub(crate) fn new(metadata: SessionMetadata, db_path: &Path) -> Self {
        Self {
            metadata,
            db_path: db_path.to_str().unwrap_or_default().to_string(),
            messages: Vec::new(),
            context_entries: Vec::new(),
            transcript_items: Vec::new(),
            reasoning_attachments: Vec::new(),
            pending_reasoning: PendingReasoning::default(),
            token_input: 0,
            token_output: 0,
            token_reasoning: 0,
            token_cache_read: 0,
            token_cache_write: 0,
            saw_tokens: false,
            has_user_message: false,
            saw_corrupt_user: false,
        }
    }

    pub(crate) fn push(&mut self, row: V2MessageRow) {
        match row.kind.as_str() {
            "user" => self.push_user(&row),
            "assistant" => self.push_assistant(&row),
            "system" => self.push_system(&row),
            "skill" => self.push_skill(&row),
            "compaction" => self.push_compaction(&row),
            other => tracing::debug!("skipping unknown V2 message type {other:?} ({})", row.id),
        }
    }

    fn push_user(&mut self, row: &V2MessageRow) {
        let Some(text) = row.data.get("text").and_then(|v| v.as_str()) else {
            tracing::warn!("malformed user message {}: missing text", row.id);
            self.saw_corrupt_user = true;
            return;
        };
        if text.trim().is_empty() {
            // Readable but empty prompt: not a genuine user message, not corruption.
            return;
        }

        let session_id = self.metadata.id.clone();
        let message_index = self.messages.len() as i64;
        let item_index = self.transcript_items.len() as i64;
        self.transcript_items.push(TranscriptItem {
            session_id: session_id.clone(),
            item_index,
            kind: TranscriptItemKind::Message,
            message_index: Some(message_index),
            tool_call_id: None,
            subagent_id: None,
            context_id: None,
        });
        self.flush_reasoning(item_index);
        self.messages.push(Message {
            session_id,
            index: 0,
            role: Role::User,
            content: text.to_string(),
            timestamp: row.timestamp,
            model: None,
        });
        self.has_user_message = true;

        // User-attached skills become context entries immediately after the
        // genuine user message, in attachment order. Attachments without text
        // add no fabricated body.
        if let Some(skills) = row.data.get("skills").and_then(|v| v.as_array()) {
            for (position, skill) in skills.iter().enumerate() {
                let name = skill
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let skill_text = match skill.get("text").and_then(|v| v.as_str()) {
                    Some(t) if !t.trim().is_empty() => t.to_string(),
                    _ => continue,
                };
                let id = format!("{}:skill:{}", row.id, position);
                self.push_context(ContextKind::Skill, id, name, skill_text, row.timestamp);
            }
        }
    }

    fn push_assistant(&mut self, row: &V2MessageRow) {
        let model =
            crate::parsers::model::normalize_model(row.data.get("model").and_then(|m| m.get("id")));
        self.accumulate_tokens(&row.data);

        let Some(content) = row.data.get("content").and_then(|v| v.as_array()) else {
            tracing::warn!("malformed assistant message {}: missing content", row.id);
            return;
        };

        for item in content {
            match item.get("type").and_then(|v| v.as_str()) {
                Some("text") => {
                    let Some(text) = item.get("text").and_then(|v| v.as_str()) else {
                        tracing::warn!("malformed assistant text in {}: missing text", row.id);
                        continue;
                    };
                    if text.trim().is_empty() {
                        continue;
                    }
                    self.push_assistant_text(text, model.as_deref(), row.timestamp);
                }
                Some("reasoning") => {
                    let visible_text = item
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    self.pending_reasoning.merge(PendingReasoning {
                        visible_text,
                        summary_text: None,
                        has_encrypted_content: false,
                        source_model: model.clone(),
                        source_timestamp: Some(row.timestamp),
                    });
                }
                Some("tool") => {
                    // Task 3 projects tool-call/subagent content here; the seam
                    // keeps V2Projection::{new,push,finish} signatures unchanged.
                }
                other => {
                    tracing::debug!(
                        "skipping unknown assistant content type {other:?} in {}",
                        row.id
                    );
                }
            }
        }
    }

    fn push_system(&mut self, row: &V2MessageRow) {
        let Some(text) = row.data.get("text").and_then(|v| v.as_str()) else {
            tracing::warn!("malformed system message {}: missing text", row.id);
            return;
        };
        let name = row
            .data
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        self.push_context(
            ContextKind::System,
            row.id.clone(),
            name,
            text.to_string(),
            row.timestamp,
        );
    }

    fn push_skill(&mut self, row: &V2MessageRow) {
        let name = row
            .data
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let Some(text) = row.data.get("text").and_then(|v| v.as_str()) else {
            tracing::warn!("malformed skill message {}: missing text", row.id);
            return;
        };
        self.push_context(
            ContextKind::Skill,
            row.id.clone(),
            name,
            text.to_string(),
            row.timestamp,
        );
    }

    fn push_compaction(&mut self, row: &V2MessageRow) {
        // Only a completed compaction has a summary; running, failed, and
        // unknown forms carry no completed summary and create no context entry.
        if row.data.get("status").and_then(|v| v.as_str()) == Some("completed") {
            let Some(summary) = row.data.get("summary").and_then(|v| v.as_str()) else {
                tracing::warn!("malformed completed compaction {}: missing summary", row.id);
                return;
            };
            self.push_context(
                ContextKind::CompactionSummary,
                row.id.clone(),
                None,
                summary.to_string(),
                row.timestamp,
            );
        }
    }

    fn push_assistant_text(&mut self, text: &str, model: Option<&str>, timestamp: DateTime<Utc>) {
        let session_id = self.metadata.id.clone();
        let message_index = self.messages.len() as i64;
        let item_index = self.transcript_items.len() as i64;
        self.transcript_items.push(TranscriptItem {
            session_id: session_id.clone(),
            item_index,
            kind: TranscriptItemKind::Message,
            message_index: Some(message_index),
            tool_call_id: None,
            subagent_id: None,
            context_id: None,
        });
        self.flush_reasoning(item_index);
        self.messages.push(Message {
            session_id,
            index: 0,
            role: Role::Assistant,
            content: text.to_string(),
            timestamp,
            model: model.map(str::to_string),
        });
    }

    fn push_context(
        &mut self,
        kind: ContextKind,
        id: String,
        name: Option<String>,
        text: String,
        timestamp: DateTime<Utc>,
    ) {
        let session_id = self.metadata.id.clone();
        let item_index = self.transcript_items.len() as i64;
        self.transcript_items.push(TranscriptItem {
            session_id: session_id.clone(),
            item_index,
            kind: TranscriptItemKind::Context,
            message_index: None,
            tool_call_id: None,
            subagent_id: None,
            context_id: Some(id.clone()),
        });
        self.flush_reasoning(item_index);
        self.context_entries.push(ContextEntry {
            id,
            session_id,
            kind,
            name,
            text,
            timestamp,
            item_index,
        });
    }

    fn flush_reasoning(&mut self, item_index: i64) {
        let pending = std::mem::take(&mut self.pending_reasoning);
        if !pending.is_empty() {
            let session_id = self.metadata.id.clone();
            self.reasoning_attachments
                .push(pending.into_attachment(&session_id, item_index));
        }
    }

    fn accumulate_tokens(&mut self, data: &Value) {
        let Some(tokens) = data.get("tokens") else {
            return;
        };
        let Some(tokens) = extract_tokens(tokens) else {
            return;
        };
        self.saw_tokens = true;
        self.token_input += tokens.input;
        self.token_output += tokens.output;
        self.token_reasoning += tokens.reasoning.unwrap_or(0);
        self.token_cache_read += tokens.cache_read.unwrap_or(0);
        self.token_cache_write += tokens.cache_write.unwrap_or(0);
    }

    pub(crate) fn finish(self) -> Result<ParsedSession> {
        if !self.has_user_message {
            if self.saw_corrupt_user {
                return Err(ParseError::IncompleteTranscript.into());
            }
            return Err(ParseError::NoUserMessages.into());
        }

        let mut messages = self.messages;
        for (index, message) in messages.iter_mut().enumerate() {
            message.index = index;
        }

        let first_prompt = match &self.metadata.title {
            Some(title) if !title.trim().is_empty() => Some(title.clone()),
            _ => crate::parsers::extract_first_prompt(&messages),
        };

        let token_usage = self.saw_tokens.then(|| TokenUsage {
            input_tokens: self.token_input,
            output_tokens: self.token_output,
            reasoning_tokens: (self.token_reasoning > 0).then_some(self.token_reasoning),
            cache_read_tokens: (self.token_cache_read > 0).then_some(self.token_cache_read),
            cache_write_tokens: (self.token_cache_write > 0).then_some(self.token_cache_write),
        });

        let session = Session {
            id: self.metadata.id.clone(),
            tool: AiAssistant::OpenCode,
            project_path: self.metadata.directory.clone(),
            project_id: None,
            start_time: self.metadata.time_created,
            message_count: messages.len(),
            file_path: self.db_path,
            last_updated: self.metadata.time_updated,
            pinned_at: None,
            first_prompt,
            parent_session_id: self.metadata.parent_id.clone(),
            is_subagent: self.metadata.parent_id.is_some(),
            token_usage: None,
            edit_count: 0,
            read_count: 0,
            command_count: 0,
            ending_status: SessionEndingStatus::Unknown,
        };

        Ok(ParsedSession {
            session,
            messages,
            tool_calls: Vec::new(),
            subagents: Vec::new(),
            transcript_items: self.transcript_items,
            reasoning_attachments: self.reasoning_attachments,
            context_entries: self.context_entries,
            token_usage,
            agent_path: None,
        })
    }
}
