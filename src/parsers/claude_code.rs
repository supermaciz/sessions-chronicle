use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

mod first_prompt;

use crate::models::{
    AiAssistant, Message, ReasoningAttachment, Role, Session, Subagent, TokenUsage, ToolCall,
    ToolCallStatus,
};
use crate::models::{TranscriptItem, TranscriptItemKind};
use crate::parsers::model::normalize_model;
use crate::parsers::{ParsedSession, PendingReasoning};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Session contains no messages")]
    NoMessages,
    #[error("Session contains no user messages")]
    NoUserMessages,
    #[error("Nested subagent file has malformed agent id")]
    MalformedNestedSubagentFile,
}

struct UsageEntry {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
}

pub struct ClaudeCodeParser;

fn claude_subagent_agent_id_from_path(file_path: &Path) -> Option<String> {
    file_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.strip_prefix("agent-"))
        .filter(|suffix| !suffix.is_empty())
        .map(str::to_string)
}

fn claude_subagent_parent_session_id_from_path(file_path: &Path) -> Option<String> {
    let subagents_dir = file_path.parent()?;
    if subagents_dir.file_name()?.to_str()? != "subagents" {
        return None;
    }

    subagents_dir
        .parent()?
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

fn claude_subagent_child_session_id(parent_session_id: &str, agent_id: &str) -> String {
    format!("claude-subagent::{parent_session_id}::{agent_id}")
}

fn extract_agent_id_from_result_text(result_text: &str) -> Option<String> {
    for line in result_text.lines().map(str::trim) {
        let Some(value) = line.strip_prefix("agentId:") else {
            continue;
        };
        let Some(token) = value.split_whitespace().next() else {
            continue;
        };

        let is_valid = token.len() >= 6
            && token.starts_with('a')
            && token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if is_valid {
            return Some(token.to_string());
        }
    }

    None
}

/// Private parsing state accumulator for Claude Code sessions.
/// Holds all mutable fields that were previously declared inside `parse()`.
struct ParseState {
    // Metadata accumulation
    earliest_timestamp: Option<DateTime<Utc>>,
    latest_timestamp: Option<DateTime<Utc>>,
    project_path: Option<String>,
    session_id_from_event: Option<String>,
    has_user_message: bool,

    // Output collections
    messages: Vec<Message>,
    tool_calls: Vec<ToolCall>,
    subagents: Vec<Subagent>,
    transcript_items: Vec<TranscriptItem>,
    reasoning_attachments: Vec<ReasoningAttachment>,

    // Pending correlation maps
    pending_calls: HashMap<String, usize>,
    pending_subagents: HashMap<String, usize>,

    // Counters
    msg_counter: i64,
    item_counter: i64,

    // Pending reasoning attached to the next transcript item.
    pending_reasoning_session_id: Option<String>,
    pending_reasoning: PendingReasoning,

    // Token usage dedupe state
    usage_map: HashMap<(String, String), UsageEntry>,
    anonymous_usage: Vec<UsageEntry>,
}

impl ParseState {
    fn new() -> Self {
        Self {
            earliest_timestamp: None,
            latest_timestamp: None,
            project_path: None,
            session_id_from_event: None,
            has_user_message: false,
            messages: Vec::new(),
            tool_calls: Vec::new(),
            subagents: Vec::new(),
            transcript_items: Vec::new(),
            reasoning_attachments: Vec::new(),
            pending_calls: HashMap::new(),
            pending_subagents: HashMap::new(),
            msg_counter: 0,
            item_counter: 0,
            pending_reasoning_session_id: None,
            pending_reasoning: PendingReasoning::default(),
            usage_map: HashMap::new(),
            anonymous_usage: Vec::new(),
        }
    }

    fn maybe_capture_session_id(&mut self, event: &Value) {
        if self.session_id_from_event.is_none() {
            self.session_id_from_event = event
                .get("sessionId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }

    fn maybe_capture_cwd(&mut self, event: &Value) {
        if self.project_path.is_none() {
            self.project_path = event
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }

    fn update_timestamps_for_message_event(&mut self, timestamp: Option<DateTime<Utc>>) {
        if let Some(ts) = timestamp {
            self.earliest_timestamp = Some(match self.earliest_timestamp {
                Some(existing) => existing.min(ts),
                None => ts,
            });
            self.latest_timestamp = Some(match self.latest_timestamp {
                Some(existing) => existing.max(ts),
                None => ts,
            });
        }
    }

    fn push_message(
        &mut self,
        session_id: String,
        role: Role,
        content: String,
        timestamp: DateTime<Utc>,
        model: Option<String>,
    ) {
        self.messages.push(Message {
            session_id,
            index: self.msg_counter as usize,
            role,
            content,
            timestamp,
            model,
        });
        self.msg_counter += 1;
    }

    fn push_message_transcript_item(&mut self) {
        let item_index = self.item_counter;
        self.transcript_items.push(TranscriptItem {
            session_id: String::new(),
            item_index,
            kind: TranscriptItemKind::Message,
            message_index: Some(self.msg_counter - 1),
            tool_call_id: None,
            subagent_id: None,
        });
        self.item_counter += 1;
        self.flush_pending_reasoning_to_item(item_index);
    }

    fn push_tool_call_transcript_item(&mut self, tool_call_id: String) {
        let item_index = self.item_counter;
        self.transcript_items.push(TranscriptItem {
            session_id: String::new(),
            item_index,
            kind: TranscriptItemKind::ToolCall,
            message_index: None,
            tool_call_id: Some(tool_call_id),
            subagent_id: None,
        });
        self.item_counter += 1;
        self.flush_pending_reasoning_to_item(item_index);
    }

    fn push_subagent_transcript_item(&mut self, subagent_id: String) {
        let item_index = self.item_counter;
        self.transcript_items.push(TranscriptItem {
            session_id: String::new(),
            item_index,
            kind: TranscriptItemKind::Subagent,
            message_index: None,
            tool_call_id: None,
            subagent_id: Some(subagent_id),
        });
        self.item_counter += 1;
        self.flush_pending_reasoning_to_item(item_index);
    }

    fn queue_reasoning(
        &mut self,
        session_id: &str,
        visible_text: Option<String>,
        has_encrypted_content: bool,
        source_model: Option<String>,
        source_timestamp: DateTime<Utc>,
    ) {
        let visible_text = visible_text
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        let next = PendingReasoning {
            visible_text,
            summary_text: None,
            has_encrypted_content,
            source_model,
            source_timestamp: Some(source_timestamp),
        };
        if next.is_empty() {
            return;
        }
        if self.pending_reasoning_session_id.is_none() {
            self.pending_reasoning_session_id = Some(session_id.to_string());
        }
        self.pending_reasoning.merge(next);
    }

    fn flush_pending_reasoning_to_item(&mut self, transcript_item_index: i64) {
        if self.pending_reasoning.is_empty() {
            return;
        }
        let session_id = self
            .pending_reasoning_session_id
            .take()
            .unwrap_or_else(|| "unknown".to_string());
        let pending = std::mem::take(&mut self.pending_reasoning);
        self.reasoning_attachments
            .push(pending.into_attachment(&session_id, transcript_item_index));
    }

    fn drop_pending_reasoning_if_orphan(&mut self) {
        if self.pending_reasoning.is_empty() {
            return;
        }
        tracing::debug!("dropping orphan reasoning with no visible transcript item");
        self.pending_reasoning_session_id = None;
        self.pending_reasoning = PendingReasoning::default();
    }

    /// Handles a user event: extracts content, processes tool results, and emits messages.
    fn handle_user_event(
        &mut self,
        event: &Value,
        extract_tool_result_content: impl Fn(&Value) -> String,
        extract_text_from_array: impl Fn(&[Value]) -> Option<String>,
        extract_content: impl Fn(&Value) -> Option<String>,
        parse_timestamp: impl Fn(&str) -> Result<DateTime<Utc>>,
    ) {
        let content_val = event.get("message").and_then(|m| m.get("content"));
        let Some(content_val) = content_val else {
            return;
        };

        // Check for tool_result blocks
        if let Some(arr) = content_val.as_array() {
            let has_tool_result = arr
                .iter()
                .any(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_result"));

            if has_tool_result {
                // Process tool results
                for block in arr {
                    self.record_tool_result(
                        block,
                        event,
                        &extract_tool_result_content,
                        &parse_timestamp,
                    );
                }

                // Check if there's also text content in the same user event
                let text_content = extract_text_from_array(arr);
                if let Some(text) = text_content
                    && !text.trim().is_empty()
                {
                    self.emit_user_message_from_event(event, text, &parse_timestamp);
                }
                return;
            }
        }

        // Regular user message (string or text-block array)
        let text = match extract_content(content_val) {
            Some(t) if !t.trim().is_empty() => t,
            _ => return,
        };
        self.emit_user_message_from_event(event, text, &parse_timestamp);
    }

    /// Records a tool result, correlating it with pending tool calls or subagents.
    fn record_tool_result(
        &mut self,
        block: &Value,
        event: &Value,
        extract_tool_result_content: impl Fn(&Value) -> String,
        parse_timestamp: impl Fn(&str) -> Result<DateTime<Utc>>,
    ) {
        if block.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
            return;
        }
        let tool_use_id = block
            .get("tool_use_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let result_text = extract_tool_result_content(block);
        let is_error = block
            .get("is_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if let Some(&tc_idx) = self.pending_calls.get(tool_use_id) {
            if let Some(tc) = self.tool_calls.get_mut(tc_idx) {
                if is_error {
                    tc.error_text = Some(result_text);
                    tc.status = ToolCallStatus::Error;
                } else {
                    tc.output_text = Some(result_text);
                    tc.status = ToolCallStatus::Completed;
                }
                if let Some(ts) = event
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .and_then(|s| parse_timestamp(s).ok())
                {
                    tc.ended_at = Some(ts.timestamp());
                    if let Some(started) = tc.started_at {
                        tc.duration_ms = Some((ts.timestamp() - started) * 1000);
                    }
                }
            }
        } else if let Some(&sa_idx) = self.pending_subagents.get(tool_use_id)
            && let Some(sa) = self.subagents.get_mut(sa_idx)
        {
            sa.result_summary = Some(result_text.clone());
            if sa.agent_id.is_none() {
                sa.agent_id = extract_agent_id_from_result_text(&result_text);
            }
        }
    }

    /// Emits a user message from an event with the given text content.
    fn emit_user_message_from_event(
        &mut self,
        event: &Value,
        text: String,
        parse_timestamp: impl Fn(&str) -> Result<DateTime<Utc>>,
    ) {
        self.has_user_message = true;
        let ts = event
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|s| parse_timestamp(s).ok())
            .unwrap_or_else(Utc::now);
        let evt_sid = event
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        self.push_message(evt_sid, Role::User, text, ts, None);
        self.push_message_transcript_item();
    }

    /// Handles an assistant event: extracts content, records usage, and emits messages/tool calls.
    fn handle_assistant_event(
        &mut self,
        event: &Value,
        parse_timestamp: impl Fn(&str) -> Result<DateTime<Utc>>,
    ) {
        let content_val = event.get("message").and_then(|m| m.get("content"));
        let Some(content_val) = content_val else {
            return;
        };

        // Extract assistant context (timestamp, session_id, model)
        let (timestamp, session_id, model) =
            self.extract_assistant_context(event, &parse_timestamp);

        // Record token usage from this event
        self.record_usage(event);

        if let Some(arr) = content_val.as_array() {
            let (reasoning_text, has_encrypted_signature) =
                ClaudeCodeParser::extract_reasoning_from_array(arr);
            self.queue_reasoning(
                &session_id,
                reasoning_text,
                has_encrypted_signature,
                model.clone(),
                timestamp,
            );

            if let Some(text) = ClaudeCodeParser::extract_assistant_text_from_array(arr)
                && !text.trim().is_empty()
            {
                self.push_message(
                    session_id.clone(),
                    Role::Assistant,
                    text,
                    timestamp,
                    model.clone(),
                );
                self.push_message_transcript_item();
            }

            // Extract and record tool_use blocks (regular tools and Task/Agent subagents)
            for block in arr {
                self.record_tool_use(block, &session_id, timestamp, &parse_timestamp);
            }
            return;
        }

        // Non-array assistant content (legacy string payloads).
        if let Some(text) = content_val
            .as_str()
            .map(str::to_string)
            .filter(|t| !t.trim().is_empty())
        {
            self.push_message(session_id, Role::Assistant, text, timestamp, model);
            self.push_message_transcript_item();
        }
    }

    /// Extracts context fields from an assistant event: (timestamp, session_id, model).
    fn extract_assistant_context(
        &self,
        event: &Value,
        parse_timestamp: impl Fn(&str) -> Result<DateTime<Utc>>,
    ) -> (DateTime<Utc>, String, Option<String>) {
        let timestamp = event
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|s| parse_timestamp(s).ok())
            .unwrap_or_else(Utc::now);
        let session_id = event
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let model_raw = event.get("message").and_then(|m| m.get("model"));
        let model = normalize_model(model_raw);
        (timestamp, session_id, model)
    }

    /// Records token usage from an assistant event message.usage field.
    fn record_usage(&mut self, event: &Value) {
        let Some(usage) = event.get("message").and_then(|m| m.get("usage")) else {
            return;
        };

        let input = usage
            .get("input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let cache_read = usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let cache_write = usage
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let entry = UsageEntry {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
        };

        let request_id = event
            .get("requestId")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let message_id = event
            .get("message")
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        if let (Some(req_id), Some(msg_id)) = (request_id, message_id) {
            let key = (req_id, msg_id);
            let entry_total = entry.input_tokens + entry.output_tokens;
            let replace = match self.usage_map.get(&key) {
                Some(existing) => entry_total > existing.input_tokens + existing.output_tokens,
                None => true,
            };
            if replace {
                self.usage_map.insert(key, entry);
            }
        } else {
            self.anonymous_usage.push(entry);
        }
    }

    /// Records a tool_use block: creates a ToolCall or Subagent, and emits a transcript item.
    fn record_tool_use(
        &mut self,
        block: &Value,
        _session_id: &str,
        timestamp: DateTime<Utc>,
        _parse_timestamp: impl Fn(&str) -> Result<DateTime<Utc>>,
    ) {
        if block.get("type").and_then(|v| v.as_str()) != Some("tool_use") {
            return;
        }

        let tool_use_id = match block.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => {
                tracing::warn!("tool_use block missing id, skipping");
                return;
            }
        };
        let tool_name = block
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let input_json = block.get("input").map(|v| v.to_string());

        if matches!(tool_name.as_str(), "Task" | "Agent") {
            // Treat as subagent
            let description = block
                .get("input")
                .and_then(|v| v.get("description"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let prompt = block
                .get("input")
                .and_then(|v| v.get("prompt"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let title = description.clone().unwrap_or_else(|| {
                prompt
                    .as_deref()
                    .unwrap_or("Subagent task")
                    .chars()
                    .take(80)
                    .collect::<String>()
            });
            self.subagents.push(Subagent {
                id: tool_use_id.clone(),
                agent_id: None,
                session_id: String::new(),
                title,
                prompt,
                result_summary: None,
                child_session_id: None,
                parser_ref: Some(tool_use_id.clone()),
            });
            self.pending_subagents
                .insert(tool_use_id.clone(), self.subagents.len() - 1);
            self.push_subagent_transcript_item(tool_use_id);
        } else {
            // Regular tool call
            self.tool_calls.push(ToolCall {
                id: tool_use_id.clone(),
                session_id: String::new(),
                subagent_id: None,
                tool_name: tool_name.clone(),
                status: ToolCallStatus::Pending,
                title: Some(tool_name),
                summary: None,
                input_json,
                output_text: None,
                error_text: None,
                started_at: Some(timestamp.timestamp()),
                ended_at: None,
                duration_ms: None,
                parser_call_id: None,
            });
            self.pending_calls
                .insert(tool_use_id.clone(), self.tool_calls.len() - 1);
            self.push_tool_call_transcript_item(tool_use_id);
        }
    }

    /// Finalizes the parsing state into a ParsedSession.
    /// Aggregates token usage, builds Session, computes first_prompt, and validates.
    fn finish(
        mut self,
        file_path: &Path,
        file_stem_id: Option<String>,
        had_parse_errors: bool,
    ) -> Result<ParsedSession> {
        let parent_session_id = claude_subagent_parent_session_id_from_path(file_path);
        let nested_agent_id = claude_subagent_agent_id_from_path(file_path);

        // A file under `<parent>/subagents/` whose name does not yield a valid
        // agent id would otherwise fall back to the sessionId embedded in its
        // events — which nested subagent transcripts copy from the parent.
        // Indexing it would overwrite the parent's transcript. Reject instead.
        if parent_session_id.is_some() && nested_agent_id.is_none() {
            return Err(ParseError::MalformedNestedSubagentFile.into());
        }

        let is_subagent = parent_session_id.is_some() && nested_agent_id.is_some();
        let parent_session_id = if is_subagent { parent_session_id } else { None };

        let final_session_id = match (&parent_session_id, &nested_agent_id) {
            (Some(parent_session_id), Some(agent_id)) => {
                claude_subagent_child_session_id(parent_session_id, agent_id)
            }
            _ => self
                .session_id_from_event
                .clone()
                .or(file_stem_id)
                .unwrap_or_else(|| "unknown".to_string()),
        };
        self.drop_pending_reasoning_if_orphan();

        // Aggregate token usage from all deduplicated entries
        let all_entries = self.usage_map.into_values().chain(self.anonymous_usage);
        let mut total_input: i64 = 0;
        let mut total_output: i64 = 0;
        let mut total_cache_read: i64 = 0;
        let mut total_cache_write: i64 = 0;
        let mut has_usage = false;
        for entry in all_entries {
            has_usage = true;
            total_input += entry.input_tokens;
            total_output += entry.output_tokens;
            total_cache_read += entry.cache_read_tokens;
            total_cache_write += entry.cache_write_tokens;
        }
        let token_usage = if has_usage {
            Some(TokenUsage {
                input_tokens: total_input,
                output_tokens: total_output,
                cache_read_tokens: if total_cache_read > 0 {
                    Some(total_cache_read)
                } else {
                    None
                },
                cache_write_tokens: if total_cache_write > 0 {
                    Some(total_cache_write)
                } else {
                    None
                },
                reasoning_tokens: None,
            })
        } else {
            None
        };

        let Some(start_time) = self.earliest_timestamp else {
            if had_parse_errors {
                anyhow::bail!("Session contained parse errors and no messages");
            }
            return Err(ParseError::NoMessages.into());
        };

        if !self.has_user_message {
            return Err(ParseError::NoUserMessages.into());
        }

        let last_updated = self.latest_timestamp.unwrap_or(start_time);
        let first_prompt = first_prompt::extract_first_prompt(&self.messages);

        let session = Session {
            id: final_session_id,
            tool: AiAssistant::ClaudeCode,
            project_path: self.project_path,
            project_id: None,
            start_time,
            message_count: self.messages.len(),
            file_path: file_path.to_str().unwrap().to_string(),
            last_updated,
            pinned_at: None,
            first_prompt,
            parent_session_id,
            is_subagent,
            token_usage: None,
            edit_count: 0,
            read_count: 0,
            command_count: 0,
            ending_status: crate::models::SessionEndingStatus::Unknown,
        };

        Ok(ParsedSession {
            session,
            messages: self.messages,
            tool_calls: self.tool_calls,
            subagents: self.subagents,
            transcript_items: self.transcript_items,
            reasoning_attachments: self.reasoning_attachments,
            token_usage,
        })
    }
}

impl ClaudeCodeParser {
    pub fn parse(&self, file_path: &Path) -> Result<ParsedSession> {
        let file = File::open(file_path).context("Failed to open session file")?;
        let reader = BufReader::new(file);

        let file_stem_id = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        let mut state = ParseState::new();
        let mut had_parse_errors = false;

        for line in reader.lines() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }

            let event: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(err) => {
                    tracing::warn!("Failed to parse JSON line: {}", err);
                    had_parse_errors = true;
                    continue;
                }
            };

            state.maybe_capture_session_id(&event);
            state.maybe_capture_cwd(&event);

            let event_type = event.get("type").and_then(|v| v.as_str());
            let is_message_like = matches!(event_type, Some("user") | Some("assistant"));

            if is_message_like {
                let ts = event
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Self::parse_timestamp(s).ok());
                state.update_timestamps_for_message_event(ts);
            }

            match event_type {
                Some("user") => {
                    state.handle_user_event(
                        &event,
                        Self::extract_tool_result_content,
                        Self::extract_text_from_array,
                        Self::extract_content,
                        Self::parse_timestamp,
                    );
                }

                Some("assistant") => {
                    state.handle_assistant_event(&event, Self::parse_timestamp);
                }

                _ => {}
            }
        }

        state.finish(file_path, file_stem_id, had_parse_errors)
    }

    fn parse_timestamp(s: &str) -> Result<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&Utc))
            .context("Failed to parse timestamp")
    }

    fn extract_tool_result_content(block: &Value) -> String {
        let content = block.get("content");
        match content {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|b| {
                    if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                        b.get("text").and_then(|v| v.as_str()).map(str::to_string)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Some(other) => other.to_string(),
            None => String::new(),
        }
    }

    fn extract_text_from_array(arr: &[Value]) -> Option<String> {
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|block| {
                let block_type = block.get("type")?.as_str()?;
                match block_type {
                    "text" => block.get("text")?.as_str().map(|s| s.to_string()),
                    "thinking" => block.get("thinking")?.as_str().map(|s| s.to_string()),
                    _ => None,
                }
            })
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    fn extract_assistant_text_from_array(arr: &[Value]) -> Option<String> {
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|block| {
                let block_type = block.get("type")?.as_str()?;
                if block_type == "text" {
                    block.get("text")?.as_str().map(str::to_string)
                } else {
                    None
                }
            })
            .collect();

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    fn extract_reasoning_from_array(arr: &[Value]) -> (Option<String>, bool) {
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|block| {
                let block_type = block.get("type")?.as_str()?;
                if block_type == "thinking" {
                    block
                        .get("thinking")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_string)
                } else {
                    None
                }
            })
            .collect();

        let has_encrypted_signature = arr.iter().any(|block| {
            block.get("type").and_then(|v| v.as_str()) == Some("thinking")
                && block
                    .get("signature")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty())
        });

        if parts.is_empty() {
            (None, has_encrypted_signature)
        } else {
            (Some(parts.join("\n")), has_encrypted_signature)
        }
    }

    fn extract_content(value: &Value) -> Option<String> {
        if let Some(s) = value.as_str() {
            return Some(s.to_string());
        }

        if let Some(arr) = value.as_array() {
            return Self::extract_text_from_array(arr);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_session(lines: &[&str]) -> NamedTempFile {
        let mut file = NamedTempFile::with_suffix(".jsonl").unwrap();
        for line in lines {
            writeln!(file, "{}", line).unwrap();
        }
        file.flush().unwrap();
        file
    }

    #[test]
    fn parse_metadata_rejects_no_user_messages() {
        let file = create_temp_session(&[
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:00Z","message":{"content":"Hello"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let result = parser.parse(file.path());

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err().downcast_ref::<ParseError>(),
            Some(ParseError::NoUserMessages)
        ));
    }

    #[test]
    fn parse_metadata_accepts_session_with_user_message() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"content":"Hi!"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let result = parser.parse(file.path());

        assert!(result.is_ok());
    }

    #[test]
    fn parse_metadata_detects_user_message_without_timestamp() {
        let file = create_temp_session(&[
            r#"{"type":"user","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"content":"Hi!"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let result = parser.parse(file.path());

        assert!(result.is_ok());
    }

    #[test]
    fn parse_metadata_rejects_empty_session() {
        let file = create_temp_session(&[]);

        let parser = ClaudeCodeParser;
        let result = parser.parse(file.path());

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err().downcast_ref::<ParseError>(),
            Some(ParseError::NoMessages)
        ));
    }

    #[test]
    fn parse_invalid_json_without_messages_remains_an_error() {
        let file = create_temp_session(&["not-json"]);

        let parser = ClaudeCodeParser;
        let result = parser.parse(file.path());

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("parse errors and no messages")
        );
    }

    #[test]
    fn parse_returns_session_and_messages() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"session-123","cwd":"/tmp","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"session-123","cwd":"/tmp","message":{"content":"Hi!"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        let expected_start = ClaudeCodeParser::parse_timestamp("2024-01-01T00:00:00Z").unwrap();
        let expected_end = ClaudeCodeParser::parse_timestamp("2024-01-01T00:00:01Z").unwrap();

        assert_eq!(parsed.session.id, "session-123");
        assert_eq!(parsed.session.project_path.as_deref(), Some("/tmp"));
        assert_eq!(parsed.session.start_time, expected_start);
        assert_eq!(parsed.session.last_updated, expected_end);
        assert_eq!(parsed.session.message_count, 2);
        assert_eq!(parsed.session.first_prompt.as_deref(), Some("Hello"));

        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.messages[0].session_id, "session-123");
        assert_eq!(parsed.messages[0].index, 0);
        assert_eq!(parsed.messages[0].role, Role::User);
        assert_eq!(parsed.messages[0].content, "Hello");
        assert_eq!(parsed.messages[1].session_id, "session-123");
        assert_eq!(parsed.messages[1].index, 1);
        assert_eq!(parsed.messages[1].role, Role::Assistant);
        assert_eq!(parsed.messages[1].content, "Hi!");

        assert_eq!(parsed.transcript_items.len(), 2);
    }

    #[test]
    fn parse_prefers_event_session_id_and_propagates_to_messages() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"event-123","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"event-123","message":{"content":"Hi!"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.session.id, "event-123");
        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.messages[0].session_id, "event-123");
        assert_eq!(parsed.messages[1].session_id, "event-123");
    }

    #[test]
    fn parse_message_count_matches_parsed_messages() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"session-123","message":{"content":"Hello"}}"#,
            r#"{"type":"system","timestamp":"2024-01-01T00:00:00Z","subtype":"session_start"}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"session-123","message":{"content":"Hi!"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.session.message_count, 2);
    }

    #[test]
    fn parse_extracts_tool_calls_from_assistant_content() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"s1","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"s1","message":{"content":[{"type":"text","text":"Let me read"},{"type":"tool_use","id":"toolu_001","name":"Read","input":{"file_path":"/tmp/test.txt"}}]}}"#,
            r#"{"type":"user","timestamp":"2024-01-01T00:00:02Z","sessionId":"s1","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_001","content":"file contents"}]}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:03Z","sessionId":"s1","message":{"content":"Done!"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "toolu_001");
        assert_eq!(parsed.tool_calls[0].tool_name, "Read");
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Completed);
        assert_eq!(
            parsed.tool_calls[0].output_text.as_deref(),
            Some("file contents")
        );

        // Transcript: user msg, assistant msg, tool call item, assistant msg
        assert_eq!(parsed.transcript_items.len(), 4);
        assert_eq!(
            parsed.transcript_items[2].kind,
            TranscriptItemKind::ToolCall
        );
    }

    #[test]
    fn assistant_thinking_stays_out_of_visible_message_content() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"content":"Help"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"model":"claude-sonnet-4-5","content":[{"type":"thinking","thinking":"private chain"},{"type":"text","text":"public answer"}]}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.messages[1].content, "public answer");
        assert_eq!(parsed.reasoning_attachments.len(), 1);
        assert_eq!(
            parsed.reasoning_attachments[0].visible_text.as_deref(),
            Some("private chain")
        );
        assert_eq!(parsed.reasoning_attachments[0].transcript_item_index, 1);
    }

    #[test]
    fn thinking_only_assistant_event_attaches_to_first_tool_call() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"content":"Run command"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"model":"claude-sonnet-4-5","content":[{"type":"thinking","thinking":"need to inspect repo"},{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"pwd"}}]}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.reasoning_attachments.len(), 1);
        assert_eq!(
            parsed.reasoning_attachments[0].visible_text.as_deref(),
            Some("need to inspect repo")
        );
        assert_eq!(parsed.reasoning_attachments[0].transcript_item_index, 1);
    }

    #[test]
    fn encrypted_signature_attaches_as_encrypted_only_reasoning() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"content":"Run command"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"content":[{"type":"thinking","thinking":"","signature":"sig"},{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"pwd"}}]}}"#,
        ]);

        let parsed = ClaudeCodeParser.parse(file.path()).unwrap();

        assert_eq!(parsed.reasoning_attachments.len(), 1);
        assert_eq!(parsed.reasoning_attachments[0].visible_text, None);
        assert!(parsed.reasoning_attachments[0].has_encrypted_content);
        assert_eq!(parsed.reasoning_attachments[0].transcript_item_index, 1);
    }

    #[test]
    fn parse_tool_result_is_error_sets_error_status() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"s_err","message":{"content":"Run the command"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"s_err","message":{"content":[{"type":"tool_use","id":"toolu_err_001","name":"Bash","input":{"command":"exit 1"}}]}}"#,
            r#"{"type":"user","timestamp":"2024-01-01T00:00:02Z","sessionId":"s_err","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_err_001","is_error":true,"content":"command failed"}]}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Error);
        assert_eq!(
            parsed.tool_calls[0].error_text.as_deref(),
            Some("command failed")
        );
    }

    #[test]
    fn parse_extracts_task_tool_as_subagent() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"s2","message":{"content":"Analyze this"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"s2","message":{"content":[{"type":"text","text":"Running task"},{"type":"tool_use","id":"toolu_task_001","name":"Task","input":{"description":"Analyze project","prompt":"List all files"}}]}}"#,
            r#"{"type":"user","timestamp":"2024-01-01T00:00:10Z","sessionId":"s2","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_task_001","content":"Found 5 files"}]}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:11Z","sessionId":"s2","message":{"content":"Done"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(parsed.subagents[0].id, "toolu_task_001");
        assert_eq!(parsed.subagents[0].title, "Analyze project");
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("Found 5 files")
        );
        assert_eq!(parsed.tool_calls.len(), 0);

        // Transcript: user, assistant text, subagent item, assistant
        assert_eq!(parsed.transcript_items.len(), 4);
        assert_eq!(
            parsed.transcript_items[2].kind,
            TranscriptItemKind::Subagent
        );
    }

    #[test]
    fn parse_extracts_agent_tool_as_subagent() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"s3","message":{"content":"Analyze this"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"s3","message":{"content":[{"type":"text","text":"Running task"},{"type":"tool_use","id":"toolu_agent_001","name":"Agent","input":{"description":"Analyze project","prompt":"List all files"}}]}}"#,
            r#"{"type":"user","timestamp":"2024-01-01T00:00:10Z","sessionId":"s3","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_agent_001","content":"Found 5 files"}]}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:11Z","sessionId":"s3","message":{"content":"Done"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(parsed.subagents[0].id, "toolu_agent_001");
        assert_eq!(parsed.subagents[0].title, "Analyze project");
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("Found 5 files")
        );
        assert_eq!(parsed.tool_calls.len(), 0);

        assert_eq!(parsed.transcript_items.len(), 4);
        assert_eq!(
            parsed.transcript_items[2].kind,
            TranscriptItemKind::Subagent
        );
    }

    #[test]
    fn parse_extracts_agent_id_from_async_subagent_result() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"parent-1","message":{"content":"Analyze this"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"parent-1","message":{"content":[{"type":"tool_use","id":"toolu_agent_001","name":"Agent","input":{"description":"Analyze project","prompt":"List all files"}}]}}"#,
            r#"{"type":"user","timestamp":"2024-01-01T00:00:02Z","sessionId":"parent-1","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_agent_001","content":[{"type":"text","text":"Async agent launched successfully.\nagentId: a41c0fb07beb52ed6"}]}]}}"#,
        ]);

        let parsed = ClaudeCodeParser.parse(file.path()).unwrap();

        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(
            parsed.subagents[0].agent_id.as_deref(),
            Some("a41c0fb07beb52ed6")
        );
    }

    #[test]
    fn parse_nested_subagent_file_marks_session_as_subagent() {
        let temp = tempfile::tempdir().unwrap();
        let parent_dir = temp.path().join("65ce34ec-2589-4f2a-aad3-f536cf8b2906");
        let subagents_dir = parent_dir.join("subagents");
        std::fs::create_dir_all(&subagents_dir).unwrap();

        let child_path = subagents_dir.join("agent-a41c0fb07beb52ed6.jsonl");
        std::fs::write(
            &child_path,
            concat!(
                r#"{"parentUuid":null,"isSidechain":true,"agentId":"a41c0fb07beb52ed6","type":"user","message":{"role":"user","content":"Analyze repo"},"timestamp":"2024-01-01T00:00:00Z","cwd":"/tmp/project","sessionId":"65ce34ec-2589-4f2a-aad3-f536cf8b2906"}"#,
                "\n",
                r#"{"parentUuid":"msg-1","isSidechain":true,"agentId":"a41c0fb07beb52ed6","type":"assistant","message":{"role":"assistant","content":"Done"},"timestamp":"2024-01-01T00:00:01Z","cwd":"/tmp/project","sessionId":"65ce34ec-2589-4f2a-aad3-f536cf8b2906"}"#,
                "\n",
            ),
        )
        .unwrap();

        let parsed = ClaudeCodeParser.parse(&child_path).unwrap();

        assert_eq!(
            parsed.session.id,
            "claude-subagent::65ce34ec-2589-4f2a-aad3-f536cf8b2906::a41c0fb07beb52ed6"
        );
        assert_eq!(
            parsed.session.parent_session_id.as_deref(),
            Some("65ce34ec-2589-4f2a-aad3-f536cf8b2906")
        );
        assert!(parsed.session.is_subagent);
    }

    #[test]
    fn parse_parent_transcript_is_not_marked_as_subagent() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"parent-plain-1","message":{"content":"Analyze repo"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"parent-plain-1","message":{"content":"Done"}}"#,
        ]);

        let parsed = ClaudeCodeParser.parse(file.path()).unwrap();

        assert_eq!(parsed.session.id, "parent-plain-1");
        assert_eq!(parsed.session.parent_session_id, None);
        assert!(!parsed.session.is_subagent);
    }

    #[test]
    fn parse_extracts_agent_id_for_supported_spacing_variants() {
        for result_text in [
            "Async agent launched successfully.\nagentId:a41c0fb07beb52ed6",
            "Async agent launched successfully.\nagentId: a41c0fb07beb52ed6",
            "Async agent launched successfully.\nagentId:\ta41c0fb07beb52ed6",
        ] {
            assert_eq!(
                extract_agent_id_from_result_text(result_text).as_deref(),
                Some("a41c0fb07beb52ed6")
            );
        }
    }

    #[test]
    fn parse_extracts_only_first_agent_id_token() {
        assert_eq!(
            extract_agent_id_from_result_text(
                "Async agent launched successfully.\nagentId: a41c0f extra"
            )
            .as_deref(),
            Some("a41c0f")
        );
    }

    #[test]
    fn parse_skips_malformed_agent_id_line_and_reads_next_valid_line() {
        assert_eq!(
            extract_agent_id_from_result_text(
                "Async agent launched successfully.\nagentId: abc$123\nagentId: a41c0fb07beb52ed6"
            )
            .as_deref(),
            Some("a41c0fb07beb52ed6")
        );
    }

    #[test]
    fn parse_rejects_agent_id_with_invalid_characters() {
        assert_eq!(
            extract_agent_id_from_result_text(
                "Async agent launched successfully.\nagentId: abc$123"
            ),
            None
        );
    }

    #[test]
    fn parse_rejects_empty_agent_id_in_async_subagent_result() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"parent-1","message":{"content":"Analyze this"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"parent-1","message":{"content":[{"type":"tool_use","id":"toolu_agent_001","name":"Agent","input":{"description":"Analyze project","prompt":"List all files"}}]}}"#,
            r#"{"type":"user","timestamp":"2024-01-01T00:00:02Z","sessionId":"parent-1","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_agent_001","content":[{"type":"text","text":"Async agent launched successfully.\nagentId:   "}]}]}}"#,
        ]);

        let parsed = ClaudeCodeParser.parse(file.path()).unwrap();

        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(parsed.subagents[0].agent_id, None);
    }

    #[test]
    fn parse_malformed_child_filename_rejects_file_instead_of_colliding_with_parent() {
        let temp = tempfile::tempdir().unwrap();
        let parent_dir = temp.path().join("65ce34ec-2589-4f2a-aad3-f536cf8b2906");
        let subagents_dir = parent_dir.join("subagents");
        std::fs::create_dir_all(&subagents_dir).unwrap();

        let malformed_child_path = subagents_dir.join("agent-.jsonl");
        std::fs::write(
            &malformed_child_path,
            concat!(
                r#"{"parentUuid":null,"isSidechain":true,"type":"user","message":{"role":"user","content":"Analyze repo"},"timestamp":"2024-01-01T00:00:00Z","cwd":"/tmp/project","sessionId":"65ce34ec-2589-4f2a-aad3-f536cf8b2906"}"#,
                "\n",
                r#"{"parentUuid":"msg-1","isSidechain":true,"type":"assistant","message":{"role":"assistant","content":"Done"},"timestamp":"2024-01-01T00:00:01Z","cwd":"/tmp/project","sessionId":"65ce34ec-2589-4f2a-aad3-f536cf8b2906"}"#,
                "\n",
            ),
        )
        .unwrap();

        let result = ClaudeCodeParser.parse(&malformed_child_path);

        assert!(matches!(
            result.unwrap_err().downcast_ref::<ParseError>(),
            Some(ParseError::MalformedNestedSubagentFile)
        ));
    }

    #[test]
    fn parse_nested_subagent_file_without_sidechain_evidence_is_still_subagent() {
        let temp = tempfile::tempdir().unwrap();
        let parent_dir = temp.path().join("65ce34ec-2589-4f2a-aad3-f536cf8b2906");
        let subagents_dir = parent_dir.join("subagents");
        std::fs::create_dir_all(&subagents_dir).unwrap();

        let child_path = subagents_dir.join("agent-a41c0fb07beb52ed6.jsonl");
        std::fs::write(
            &child_path,
            concat!(
                r#"{"parentUuid":null,"isSidechain":false,"agentId":"a41c0fb07beb52ed6","type":"user","message":{"role":"user","content":"Analyze repo"},"timestamp":"2024-01-01T00:00:00Z","cwd":"/tmp/project","sessionId":"65ce34ec-2589-4f2a-aad3-f536cf8b2906"}"#,
                "\n",
                r#"{"parentUuid":"msg-1","isSidechain":false,"agentId":"a41c0fb07beb52ed6","type":"assistant","message":{"role":"assistant","content":"Done"},"timestamp":"2024-01-01T00:00:01Z","cwd":"/tmp/project","sessionId":"65ce34ec-2589-4f2a-aad3-f536cf8b2906"}"#,
                "\n",
            ),
        )
        .unwrap();

        let parsed = ClaudeCodeParser.parse(&child_path).unwrap();

        assert_eq!(
            parsed.session.id,
            "claude-subagent::65ce34ec-2589-4f2a-aad3-f536cf8b2906::a41c0fb07beb52ed6"
        );
        assert_eq!(
            parsed.session.parent_session_id.as_deref(),
            Some("65ce34ec-2589-4f2a-aad3-f536cf8b2906")
        );
        assert!(parsed.session.is_subagent);
    }

    #[test]
    fn assistant_message_has_model() {
        let parsed = ClaudeCodeParser
            .parse(std::path::Path::new(
                "tests/fixtures/claude_sessions/sample-session.jsonl",
            ))
            .unwrap();
        let assistant_msgs: Vec<_> = parsed
            .messages
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .collect();
        assert!(!assistant_msgs.is_empty());
        assert_eq!(
            assistant_msgs[0].model.as_deref(),
            Some("claude-sonnet-4-5-20250514")
        );
    }

    #[test]
    fn user_message_has_no_model() {
        let parsed = ClaudeCodeParser
            .parse(std::path::Path::new(
                "tests/fixtures/claude_sessions/sample-session.jsonl",
            ))
            .unwrap();
        let user_msgs: Vec<_> = parsed
            .messages
            .iter()
            .filter(|m| m.role == Role::User)
            .collect();
        assert!(!user_msgs.is_empty());
        assert!(user_msgs[0].model.is_none());
    }

    #[test]
    fn parse_extracts_token_usage_from_assistant_events() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"s1","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"s1","requestId":"req1","message":{"id":"msg1","content":"Hi!","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":80,"cache_creation_input_tokens":20}}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:02Z","sessionId":"s1","requestId":"req2","message":{"id":"msg2","content":"More","usage":{"input_tokens":200,"output_tokens":75,"cache_read_input_tokens":150,"cache_creation_input_tokens":10}}}"#,
        ]);
        let parsed = ClaudeCodeParser.parse(file.path()).unwrap();
        let usage = parsed.token_usage.expect("should have token_usage");
        assert_eq!(usage.input_tokens, 300);
        assert_eq!(usage.output_tokens, 125);
        assert_eq!(usage.cache_read_tokens, Some(230));
        assert_eq!(usage.cache_write_tokens, Some(30));
        assert_eq!(usage.reasoning_tokens, None);
    }

    #[test]
    fn parse_deduplicates_token_usage_by_request_and_message_id() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"s1","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"s1","requestId":"req1","message":{"id":"msg1","content":"Hi!","usage":{"input_tokens":100,"output_tokens":50}}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:02Z","sessionId":"s1","requestId":"req1","message":{"id":"msg1","content":"Hi!","usage":{"input_tokens":120,"output_tokens":60}}}"#,
        ]);
        let parsed = ClaudeCodeParser.parse(file.path()).unwrap();
        let usage = parsed.token_usage.expect("should have token_usage");
        assert_eq!(usage.input_tokens, 120);
        assert_eq!(usage.output_tokens, 60);
    }

    #[test]
    fn parse_no_usage_blocks_yields_none_token_usage() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"s1","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"s1","message":{"content":"Hi!"}}"#,
        ]);
        let parsed = ClaudeCodeParser.parse(file.path()).unwrap();
        assert!(parsed.token_usage.is_none());
    }

    #[test]
    fn parse_user_tool_result_with_visible_text_preserves_both() {
        // Scenario: User event contains both tool_result (for pending tool) AND visible text
        // This tests that the parser correctly:
        // 1. Correlates the tool_result with the pending tool call
        // 2. Stores the result on the tool call and marks it Completed
        // 3. Extracts the visible text as a separate user message
        // 4. Maintains stable transcript ordering
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"mixed_test","message":{"content":"Initial user message"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"mixed_test","message":{"content":[{"type":"text","text":"I'll help you"},{"type":"tool_use","id":"toolu_mixed_001","name":"Read","input":{"file_path":"/tmp/test.txt"}}]}}"#,
            r#"{"type":"user","timestamp":"2024-01-01T00:00:02Z","sessionId":"mixed_test","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_mixed_001","content":"file contents here"},{"type":"text","text":"Please continue analyzing"}]}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:03Z","sessionId":"mixed_test","message":{"content":"Analysis complete"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        // Assert: Tool call should exist and be Completed
        assert_eq!(
            parsed.tool_calls.len(),
            1,
            "Should have exactly one tool call"
        );
        assert_eq!(parsed.tool_calls[0].id, "toolu_mixed_001");
        assert_eq!(parsed.tool_calls[0].tool_name, "Read");
        assert_eq!(
            parsed.tool_calls[0].status,
            ToolCallStatus::Completed,
            "Tool call should be Completed"
        );
        assert_eq!(
            parsed.tool_calls[0].output_text.as_deref(),
            Some("file contents here"),
            "Tool call should have the result text"
        );

        // Assert: Visible user text should be parsed as a separate user message
        let user_messages: Vec<_> = parsed
            .messages
            .iter()
            .filter(|m| m.role == Role::User)
            .collect();
        assert_eq!(user_messages.len(), 2, "Should have two user messages");
        assert_eq!(user_messages[0].content, "Initial user message");
        assert_eq!(
            user_messages[1].content, "Please continue analyzing",
            "Visible text from mixed event should be a user message"
        );

        // Assert: All messages present
        assert_eq!(
            parsed.messages.len(),
            4,
            "Should have 4 total messages: initial user, assistant text, mixed user, assistant final"
        );
        assert_eq!(parsed.messages[0].role, Role::User);
        assert_eq!(parsed.messages[0].content, "Initial user message");
        assert_eq!(parsed.messages[1].role, Role::Assistant);
        assert_eq!(parsed.messages[1].content, "I'll help you");
        assert_eq!(parsed.messages[2].role, Role::User);
        assert_eq!(parsed.messages[2].content, "Please continue analyzing");
        assert_eq!(parsed.messages[3].role, Role::Assistant);
        assert_eq!(parsed.messages[3].content, "Analysis complete");

        // Assert: Transcript ordering is stable
        // Expected order: user1, assistant_text, tool_call, user2 (from mixed), assistant2
        assert_eq!(
            parsed.transcript_items.len(),
            5,
            "Should have 5 transcript items"
        );
        assert_eq!(parsed.transcript_items[0].kind, TranscriptItemKind::Message);
        assert_eq!(parsed.transcript_items[0].message_index, Some(0));
        assert_eq!(parsed.transcript_items[1].kind, TranscriptItemKind::Message);
        assert_eq!(parsed.transcript_items[1].message_index, Some(1));
        assert_eq!(
            parsed.transcript_items[2].kind,
            TranscriptItemKind::ToolCall
        );
        assert_eq!(
            parsed.transcript_items[2].tool_call_id,
            Some("toolu_mixed_001".to_string())
        );
        assert_eq!(parsed.transcript_items[3].kind, TranscriptItemKind::Message);
        assert_eq!(parsed.transcript_items[3].message_index, Some(2));
        assert_eq!(parsed.transcript_items[4].kind, TranscriptItemKind::Message);
        assert_eq!(parsed.transcript_items[4].message_index, Some(3));
    }
}
