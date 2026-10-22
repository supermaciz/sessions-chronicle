//! Projection of native OpenCode V2 `session_message` rows into the shared
//! `ParsedSession` domain model.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::path::Path;

use crate::models::{
    AiAssistant, ContextEntry, ContextKind, Message, ReasoningAttachment, Role, Session,
    SessionEndingStatus, Subagent, ToolCall, ToolCallStatus, TranscriptItem, TranscriptItemKind,
};
use crate::parsers::{ParsedSession, PendingReasoning};

use super::{ParseError, SessionMetadata, extract_tokens, token_usage::TokenUsageAccumulator};

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
/// the result.
pub(crate) struct V2Projection {
    metadata: SessionMetadata,
    db_path: String,
    messages: Vec<Message>,
    tool_calls: Vec<ToolCall>,
    subagents: Vec<Subagent>,
    context_entries: Vec<ContextEntry>,
    transcript_items: Vec<TranscriptItem>,
    reasoning_attachments: Vec<ReasoningAttachment>,
    pending_reasoning: PendingReasoning,
    /// Task invocations emitted so far, in transcript order, for completion
    /// correlation. `subagent_index` points into `subagents`.
    task_invocations: Vec<TaskInvocation>,
    token_usage: TokenUsageAccumulator,
    has_user_message: bool,
    saw_corrupt_user: bool,
}

/// A projected `task` or `subagent` invocation, remembered so a later synthetic
/// completion can be matched to it. Matching only ever looks backwards, so the
/// completion is resolved as it streams through `push`.
struct TaskInvocation {
    child_id: String,
    seq: i64,
    subagent_index: usize,
    matched: bool,
}

impl V2Projection {
    pub(crate) fn new(metadata: SessionMetadata, db_path: &Path) -> Self {
        Self {
            metadata,
            db_path: db_path.to_str().unwrap_or_default().to_string(),
            messages: Vec::new(),
            tool_calls: Vec::new(),
            subagents: Vec::new(),
            context_entries: Vec::new(),
            transcript_items: Vec::new(),
            reasoning_attachments: Vec::new(),
            pending_reasoning: PendingReasoning::default(),
            task_invocations: Vec::new(),
            token_usage: TokenUsageAccumulator::default(),
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
            "synthetic" => self.push_synthetic(&row),
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

        self.push_message(Role::User, text, None, row.timestamp);
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

        for (position, item) in content.iter().enumerate() {
            match item.get("type").and_then(|v| v.as_str()) {
                Some("text") => {
                    let Some(text) = item.get("text").and_then(|v| v.as_str()) else {
                        tracing::warn!("malformed assistant text in {}: missing text", row.id);
                        continue;
                    };
                    if text.trim().is_empty() {
                        continue;
                    }
                    self.push_message(Role::Assistant, text, model.as_deref(), row.timestamp);
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
                    let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
                        tracing::warn!("malformed assistant tool in {}: missing name", row.id);
                        continue;
                    };
                    if matches!(name, "task" | "subagent") {
                        let subagent = self.project_task_subagent(&row.id, item, position);
                        self.push_task_subagent(subagent, row.seq);
                    } else {
                        match project_tool(&self.metadata.id, item) {
                            Ok(tool_call) => self.push_tool_call(tool_call),
                            Err(err) => {
                                tracing::warn!("malformed assistant tool in {}: {err:#}", row.id);
                            }
                        }
                    }
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

    /// Projects a `task` or `subagent` item into a subagent row (never an
    /// ordinary tool call). Only `state.metadata.sessionId` / `sessionID` link a child;
    /// `input.task_id` alone establishes nothing.
    fn project_task_subagent(
        &self,
        message_id: &str,
        content: &Value,
        position: usize,
    ) -> Subagent {
        let id = content
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("{message_id}:task:{position}"));
        let state = content.get("state");
        let input = state.and_then(|s| s.get("input"));
        let title = input
            .and_then(|i| i.get("description"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let prompt = input
            .and_then(|i| i.get("prompt"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let child_session_id = state
            .and_then(|s| s.get("metadata"))
            .and_then(|m| {
                m.get("sessionId")
                    .and_then(|v| v.as_str())
                    .or_else(|| m.get("sessionID").and_then(|v| v.as_str()))
            })
            .map(str::to_string);
        let result_summary = state
            .and_then(|s| s.get("content"))
            .and_then(|v| v.as_array())
            .and_then(|content| result_text(content))
            .or_else(|| {
                state
                    .and_then(|s| s.get("error"))
                    .and_then(|e| e.get("message"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            });

        Subagent {
            id,
            agent_id: None,
            agent_name: input
                .and_then(|i| i.get("agent"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            session_id: self.metadata.id.clone(),
            title,
            prompt,
            result_summary,
            child_session_id,
            parser_ref: None,
        }
    }

    fn push_task_subagent(&mut self, subagent: Subagent, seq: i64) {
        let child_id = subagent.child_session_id.clone();
        let subagent_index = self.subagents.len();
        self.push_subagent(subagent);
        // Only a task with an authoritative child link can ever match a
        // completion; missing child metadata is not recorded and stays unmatched.
        if let Some(child_id) = child_id {
            self.task_invocations.push(TaskInvocation {
                child_id,
                seq,
                subagent_index,
                matched: false,
            });
        }
    }

    fn push_tool_call(&mut self, tool_call: ToolCall) {
        let id = tool_call.id.clone();
        let item_index = self.transcript_items.len() as i64;
        self.transcript_items.push(TranscriptItem {
            session_id: self.metadata.id.clone(),
            item_index,
            kind: TranscriptItemKind::ToolCall,
            message_index: None,
            tool_call_id: Some(id),
            subagent_id: None,
            context_id: None,
        });
        self.flush_reasoning(item_index);
        self.tool_calls.push(tool_call);
    }

    /// Projects a synthetic subagent-completion message. Matching only ever
    /// looks backwards, so the completion is resolved here, in stream order,
    /// before `finish` assembles the transcript.
    fn push_synthetic(&mut self, row: &V2MessageRow) {
        let metadata = row.data.get("metadata");
        if metadata
            .and_then(|m| m.get("source"))
            .and_then(|v| v.as_str())
            != Some("subagent")
        {
            tracing::debug!("skipping non-subagent synthetic message {}", row.id);
            return;
        }
        let Some(child_id) = metadata
            .and_then(|m| m.get("childID"))
            .and_then(|v| v.as_str())
        else {
            tracing::warn!("subagent completion {} missing metadata.childID", row.id);
            return;
        };
        let description = row
            .data
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let text = row
            .data
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // Candidate invocations: same child, strictly preceding, not yet matched.
        let candidates: Vec<usize> = self
            .task_invocations
            .iter()
            .enumerate()
            .filter(|(_, inv)| inv.child_id == child_id && inv.seq < row.seq && !inv.matched)
            .map(|(idx, _)| idx)
            .collect();

        if !candidates.is_empty() {
            let max_seq = candidates
                .iter()
                .map(|&idx| self.task_invocations[idx].seq)
                .max()
                .unwrap_or(row.seq);
            let mut top: Vec<usize> = candidates
                .into_iter()
                .filter(|&idx| self.task_invocations[idx].seq == max_seq)
                .collect();
            // Uniquely ordered match only: an equal-sequence tie stays unmatched.
            if top.len() == 1 {
                let invocation_idx = top.pop().expect("one candidate");
                let subagent_index = self.task_invocations[invocation_idx].subagent_index;
                self.enrich_subagent(subagent_index, &text);
                self.task_invocations[invocation_idx].matched = true;
                return;
            }
        }

        // No matching task (or an ambiguous tie): retain a stable standalone
        // subagent row keyed by the native message id.
        let subagent = Subagent {
            id: row.id.clone(),
            agent_id: None,
            agent_name: None,
            session_id: self.metadata.id.clone(),
            title: description.unwrap_or_else(|| "Subagent".to_string()),
            prompt: None,
            result_summary: (!text.is_empty()).then_some(text),
            child_session_id: Some(child_id.to_string()),
            parser_ref: None,
        };
        self.push_subagent(subagent);
    }

    fn push_subagent(&mut self, subagent: Subagent) {
        let id = subagent.id.clone();
        let item_index = self.transcript_items.len() as i64;
        self.transcript_items.push(TranscriptItem {
            session_id: self.metadata.id.clone(),
            item_index,
            kind: TranscriptItemKind::Subagent,
            message_index: None,
            tool_call_id: None,
            subagent_id: Some(id),
            context_id: None,
        });
        self.flush_reasoning(item_index);
        self.subagents.push(subagent);
    }

    /// Fills an absent result from a completion; never overwrites an explicit
    /// task output, and never duplicates the completion into a context row.
    fn enrich_subagent(&mut self, subagent_index: usize, text: &str) {
        let subagent = &mut self.subagents[subagent_index];
        if subagent.result_summary.is_none() && !text.is_empty() {
            subagent.result_summary = Some(text.to_string());
        }
    }

    fn push_message(
        &mut self,
        role: Role,
        text: &str,
        model: Option<&str>,
        timestamp: DateTime<Utc>,
    ) {
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
            role,
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
        // Do not flush pending reasoning here: context rows carry no reasoning
        // preview, so flushing would make an interrupted turn's reasoning
        // invisible. Carry it forward to the next message/tool/subagent item.
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

    /// Records a row the reader could not fully decode. When the row is (or may
    /// be) a user row, mark the transcript as corrupt so `finish` returns
    /// `IncompleteTranscript` rather than misclassifying the session as having
    /// no user messages. A `None` kind means the `type` column itself (or an
    /// earlier column) was unreadable, so the row could be a user row.
    pub(crate) fn note_corrupt_row(&mut self, kind: Option<&str>) {
        if kind.is_none() || kind == Some("user") {
            self.saw_corrupt_user = true;
        }
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
        self.token_usage.push(tokens);
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

        let token_usage = self.token_usage.finish();

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
            tool_calls: self.tool_calls,
            subagents: self.subagents,
            transcript_items: self.transcript_items,
            reasoning_attachments: self.reasoning_attachments,
            context_entries: self.context_entries,
            token_usage,
            agent_path: None,
        })
    }
}

/// Projects a non-`task` tool item into a `ToolCall`. Streaming raw input stays
/// a string; object input for other states is serialized compactly. Result text
/// joins supported text blocks in order, skipping file bodies. Execution
/// duration uses valid `ran`/`completed` content times; missing or reversed
/// ranges stay unknown.
fn project_tool(session_id: &str, content: &Value) -> Result<ToolCall> {
    let id = content
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("tool content missing id"))?
        .to_string();
    let tool_name = content
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("tool content missing name"))?
        .to_string();
    let state = content.get("state");

    let status = match state.and_then(|s| s.get("status")).and_then(|v| v.as_str()) {
        Some("streaming") => ToolCallStatus::Pending,
        Some("running") => ToolCallStatus::Running,
        Some("completed") => ToolCallStatus::Completed,
        Some("error") => ToolCallStatus::Error,
        _ => ToolCallStatus::Unknown,
    };

    let input_json = state.and_then(|s| s.get("input")).map(|v| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    });

    let output_text = state
        .and_then(|s| s.get("content"))
        .and_then(|v| v.as_array())
        .and_then(|content| result_text(content));

    let error_text = state
        .and_then(|s| s.get("error"))
        .and_then(|e| e.get("message"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let time = content.get("time");
    let started_at = time.and_then(|t| t.get("ran")).and_then(|v| v.as_i64());
    let ended_at = time
        .and_then(|t| t.get("completed"))
        .and_then(|v| v.as_i64());
    let duration_ms = match (started_at, ended_at) {
        (Some(start), Some(end)) if end >= start => Some(end - start),
        _ => None,
    };

    Ok(ToolCall {
        id,
        session_id: session_id.to_string(),
        subagent_id: None,
        tool_name,
        status,
        title: None,
        summary: None,
        input_json,
        output_text,
        error_text,
        started_at,
        ended_at,
        duration_ms,
        parser_call_id: None,
    })
}

/// Joins supported `text` blocks in `content` order with `\n`, skipping `file`
/// bodies (their URIs are never fetched). Returns `None` when there is no text.
fn result_text(content: &[Value]) -> Option<String> {
    let parts: Vec<&str> = content
        .iter()
        .filter(|item| item.get("type").and_then(|v| v.as_str()) == Some("text"))
        .filter_map(|item| item.get("text").and_then(|v| v.as_str()))
        .filter(|text| !text.is_empty())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}
