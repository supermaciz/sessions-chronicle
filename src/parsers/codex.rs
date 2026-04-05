use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::models::{
    AiAssistant, Message, ReasoningAttachment, Role, Session, TokenUsage, ToolCall, ToolCallStatus,
};
use crate::models::{TranscriptItem, TranscriptItemKind};
use crate::parsers::ParsedSession;
use crate::parsers::model::normalize_model;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("First line must be session_meta")]
    MissingSessionMeta,
    #[error("Session contains no user messages")]
    NoUserMessages,
    #[error("Invalid session_meta JSON: {0}")]
    InvalidSessionMetaJson(String),
}

/// Mutable parsing state accumulator for Codex sessions.
struct ParseState {
    session_id: String,
    last_updated: DateTime<Utc>,
    current_turn_model: Option<String>,
    best_snapshot: Option<(i64, TokenUsage)>,
    has_user_message: bool,

    // Output collections
    messages: Vec<Message>,
    tool_calls: Vec<ToolCall>,
    transcript_items: Vec<TranscriptItem>,
    reasoning_attachments: Vec<ReasoningAttachment>,

    // Correlation: call_id -> index in tool_calls
    call_id_to_tc_idx: HashMap<String, usize>,

    // Counters
    msg_counter: i64,
    item_counter: i64,
    orphan_reasoning_index: i64,
    pending_reasoning: PendingReasoning,
}

#[derive(Debug, Clone, Default)]
struct PendingReasoning {
    visible_text: Option<String>,
    summary_text: Option<String>,
    encrypted_content: Option<String>,
    source_model: Option<String>,
    source_timestamp: Option<DateTime<Utc>>,
}

impl PendingReasoning {
    fn is_empty(&self) -> bool {
        self.visible_text.is_none()
            && self.summary_text.is_none()
            && self.encrypted_content.is_none()
    }

    fn merge(&mut self, next: PendingReasoning) {
        if let Some(visible) = next.visible_text {
            match &mut self.visible_text {
                Some(current) => {
                    if !current.is_empty() {
                        current.push('\n');
                    }
                    current.push_str(&visible);
                }
                None => self.visible_text = Some(visible),
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

        if self.encrypted_content.is_none() {
            self.encrypted_content = next.encrypted_content;
        }
        if self.source_model.is_none() {
            self.source_model = next.source_model;
        }
        if self.source_timestamp.is_none() {
            self.source_timestamp = next.source_timestamp;
        }
    }

    fn into_attachment(self, session_id: &str, transcript_item_index: i64) -> ReasoningAttachment {
        ReasoningAttachment {
            session_id: session_id.to_string(),
            transcript_item_index,
            visible_text: self.visible_text,
            summary_text: self.summary_text,
            encrypted_content: self.encrypted_content,
            source_model: self.source_model,
            source_timestamp: self.source_timestamp,
        }
    }
}

impl ParseState {
    fn new(session_id: String, last_updated: DateTime<Utc>) -> Self {
        Self {
            session_id,
            last_updated,
            current_turn_model: None,
            best_snapshot: None,
            has_user_message: false,
            messages: Vec::new(),
            tool_calls: Vec::new(),
            transcript_items: Vec::new(),
            reasoning_attachments: Vec::new(),
            call_id_to_tc_idx: HashMap::new(),
            msg_counter: 0,
            item_counter: 0,
            orphan_reasoning_index: -1,
            pending_reasoning: PendingReasoning::default(),
        }
    }

    fn update_last_updated(&mut self, ts: DateTime<Utc>) {
        if ts > self.last_updated {
            self.last_updated = ts;
        }
    }

    fn push_message(
        &mut self,
        role: Role,
        content: String,
        timestamp: DateTime<Utc>,
        model: Option<String>,
    ) {
        if role == Role::User {
            self.has_user_message = true;
        }
        self.messages.push(Message {
            session_id: self.session_id.clone(),
            index: self.msg_counter as usize,
            role,
            content,
            timestamp,
            model,
        });
        self.transcript_items.push(TranscriptItem {
            session_id: self.session_id.clone(),
            item_index: self.item_counter,
            kind: TranscriptItemKind::Message,
            message_index: Some(self.msg_counter),
            tool_call_id: None,
            subagent_id: None,
        });
        self.flush_pending_reasoning_to_item(self.item_counter);
        self.msg_counter += 1;
        self.item_counter += 1;
    }

    fn push_tool_call(
        &mut self,
        call_id: String,
        tool_name: String,
        input_json: Option<String>,
        started_at: Option<i64>,
    ) {
        if self.call_id_to_tc_idx.contains_key(&call_id) {
            return;
        }
        let tc_idx = self.tool_calls.len();
        self.tool_calls.push(ToolCall {
            id: call_id.clone(),
            session_id: self.session_id.clone(),
            subagent_id: None,
            tool_name: tool_name.clone(),
            status: ToolCallStatus::Running,
            title: Some(tool_name),
            summary: None,
            input_json,
            output_text: None,
            error_text: None,
            started_at,
            ended_at: None,
            duration_ms: None,
            parser_call_id: Some(call_id.clone()),
        });
        self.call_id_to_tc_idx.insert(call_id.clone(), tc_idx);
        self.transcript_items.push(TranscriptItem {
            session_id: self.session_id.clone(),
            item_index: self.item_counter,
            kind: TranscriptItemKind::ToolCall,
            message_index: None,
            tool_call_id: Some(call_id),
            subagent_id: None,
        });
        self.flush_pending_reasoning_to_item(self.item_counter);
        self.item_counter += 1;
    }

    fn queue_reasoning(&mut self, reasoning: PendingReasoning) {
        self.pending_reasoning.merge(reasoning);
    }

    fn flush_pending_reasoning_to_item(&mut self, transcript_item_index: i64) {
        if self.pending_reasoning.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.pending_reasoning);
        self.reasoning_attachments
            .push(pending.into_attachment(&self.session_id, transcript_item_index));
    }

    fn flush_pending_reasoning_as_orphan(&mut self) {
        if self.pending_reasoning.is_empty() {
            return;
        }
        let orphan_index = self.orphan_reasoning_index;
        self.orphan_reasoning_index -= 1;
        let pending = std::mem::take(&mut self.pending_reasoning);
        self.reasoning_attachments
            .push(pending.into_attachment(&self.session_id, orphan_index));
    }

    fn complete_tool_call(
        &mut self,
        call_id: &str,
        output_text: Option<String>,
        error_text: Option<String>,
        ended_at: Option<i64>,
        duration_ms: Option<i64>,
        status: ToolCallStatus,
    ) {
        if let Some(&tc_idx) = self.call_id_to_tc_idx.get(call_id)
            && let Some(tc) = self.tool_calls.get_mut(tc_idx)
        {
            tc.output_text = output_text;
            tc.error_text = error_text;
            tc.ended_at = ended_at;
            tc.duration_ms = duration_ms;
            tc.status = status;
        }
    }

    fn handle_turn_context(&mut self, payload: &Value) {
        self.current_turn_model = normalize_model(payload.get("model"));
    }

    fn handle_response_item(&mut self, payload: &Value, event_ts: Option<DateTime<Utc>>) {
        let response_item = payload.get("response_item").unwrap_or(payload);
        match response_item.get("type").and_then(|v| v.as_str()) {
            Some("function_call") | Some("custom_tool_call") => {
                let call_id = match response_item.get("call_id").and_then(|v| v.as_str()) {
                    Some(id) if !id.is_empty() => id.to_string(),
                    _ => {
                        tracing::warn!("response_item call begin missing call_id, skipping");
                        return;
                    }
                };

                let tool_name = response_item
                    .get("name")
                    .or_else(|| response_item.get("tool_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let input_json = response_item
                    .get("arguments")
                    .or_else(|| response_item.get("input"))
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v.to_string())
                    });

                self.push_tool_call(
                    call_id,
                    tool_name,
                    input_json,
                    event_ts.map(|t| t.timestamp()),
                );
            }
            Some("function_call_output") | Some("custom_tool_call_output") => {
                let call_id = match response_item.get("call_id").and_then(|v| v.as_str()) {
                    Some(id) if !id.is_empty() => id,
                    _ => {
                        tracing::warn!("response_item call output missing call_id, skipping");
                        return;
                    }
                };

                let output_text = response_item.get("output").and_then(|v| {
                    if let Some(s) = v.as_str() {
                        Some(s.to_string())
                    } else if v.is_null() {
                        None
                    } else {
                        Some(v.to_string())
                    }
                });
                let error_text = response_item.get("error").and_then(|v| {
                    if let Some(s) = v.as_str() {
                        if s.is_empty() {
                            None
                        } else {
                            Some(s.to_string())
                        }
                    } else if v.is_null() {
                        None
                    } else {
                        Some(v.to_string())
                    }
                });

                let status_str = response_item
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_ascii_lowercase());
                let status = if matches!(status_str.as_deref(), Some("error") | Some("failed")) {
                    ToolCallStatus::Error
                } else if matches!(status_str.as_deref(), Some("completed")) {
                    ToolCallStatus::Completed
                } else if error_text.is_some() {
                    ToolCallStatus::Error
                } else {
                    ToolCallStatus::Completed
                };

                self.complete_tool_call(
                    call_id,
                    output_text,
                    error_text,
                    event_ts.map(|t| t.timestamp()),
                    response_item.get("duration_ms").and_then(|v| v.as_i64()),
                    status,
                );
            }
            Some("reasoning") => {
                let visible_text = response_item
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string);

                let summary_text = response_item
                    .get("summary")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|entry| {
                                entry
                                    .get("text")
                                    .and_then(|v| v.as_str())
                                    .map(str::trim)
                                    .filter(|text| !text.is_empty())
                                    .map(str::to_string)
                            })
                            .collect::<Vec<_>>()
                    })
                    .and_then(|parts| {
                        if parts.is_empty() {
                            None
                        } else {
                            Some(parts.join("\n"))
                        }
                    });

                let encrypted_content = response_item
                    .get("encrypted_content")
                    .or_else(|| response_item.get("encryptedContent"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);

                let reasoning = PendingReasoning {
                    visible_text,
                    summary_text,
                    encrypted_content,
                    source_model: self.current_turn_model.clone(),
                    source_timestamp: event_ts,
                };

                if !reasoning.is_empty() {
                    self.queue_reasoning(reasoning);
                }
            }
            _ => {}
        }
    }

    fn handle_event_msg(&mut self, payload: &Value, event_ts: Option<DateTime<Utc>>) {
        match payload.get("type").and_then(|v| v.as_str()) {
            Some("turn_context") => {
                self.handle_turn_context(payload);
            }

            Some("user_message") => {
                let content = match payload.get("message").and_then(|v| v.as_str()) {
                    Some(c) => c.to_string(),
                    None => return,
                };
                self.push_message(Role::User, content, event_ts.unwrap_or_else(Utc::now), None);
            }

            Some("agent_message") => {
                let content = match payload.get("message").and_then(|v| v.as_str()) {
                    Some(c) => c.to_string(),
                    None => return,
                };
                let model = self.current_turn_model.clone();
                self.push_message(
                    Role::Assistant,
                    content,
                    event_ts.unwrap_or_else(Utc::now),
                    model,
                );
            }

            Some(begin_type)
                if matches!(begin_type, "mcp_tool_call_begin" | "exec_command_begin") =>
            {
                let call_id = match payload.get("call_id").and_then(|v| v.as_str()) {
                    Some(id) => id.to_string(),
                    None => {
                        tracing::warn!("Tool call begin event missing call_id, skipping");
                        return;
                    }
                };

                let tool_name = if begin_type == "exec_command_begin" {
                    // For exec_command_begin, `payload["command"]` is the shell command
                    // text, not the tool name. Use the canonical name instead.
                    "exec_command".to_string()
                } else {
                    payload
                        .get("tool_name")
                        .or_else(|| payload.get("command"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(begin_type)
                        .to_string()
                };
                let input_json = if begin_type == "exec_command_begin" {
                    Some(
                        serde_json::json!({
                            "command": payload.get("command").and_then(|v| v.as_str()).unwrap_or(""),
                            "cwd": payload.get("cwd").and_then(|v| v.as_str()),
                        })
                        .to_string(),
                    )
                } else {
                    payload.get("input").map(|v| v.to_string())
                };

                self.push_tool_call(
                    call_id,
                    tool_name,
                    input_json,
                    event_ts.map(|t| t.timestamp()),
                );
            }

            Some("token_count") => {
                self.record_token_usage(payload);
            }

            Some("mcp_tool_call_end") | Some("exec_command_end") => {
                let call_id = match payload.get("call_id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => return,
                };
                let output = payload
                    .get("output")
                    .or_else(|| payload.get("stdout"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let error = payload
                    .get("error")
                    .or_else(|| payload.get("stderr"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                let exit_code = payload.get("exit_code").and_then(|v| v.as_i64());
                let status = match exit_code {
                    Some(0) | None => ToolCallStatus::Completed,
                    Some(_) => ToolCallStatus::Error,
                };

                self.complete_tool_call(
                    call_id,
                    output,
                    error,
                    event_ts.map(|t| t.timestamp()),
                    payload.get("duration_ms").and_then(|v| v.as_i64()),
                    status,
                );
            }

            _ => {}
        }
    }

    fn record_token_usage(&mut self, payload: &Value) {
        if let Some(info) = payload.get("info")
            && !info.is_null()
            && let Some(total_usage) = info.get("total_token_usage")
        {
            let input = total_usage
                .get("input_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let output = total_usage
                .get("output_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let reasoning = total_usage
                .get("reasoning_output_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let cached = total_usage
                .get("cached_input_tokens")
                .and_then(|v| v.as_i64());

            let global_total = input + output + reasoning;
            let replace = match &self.best_snapshot {
                Some((current_best, _)) => global_total > *current_best,
                None => true,
            };
            if replace {
                // Codex/OpenAI reports cached_input_tokens as the cached subset
                // of input_tokens, not as an extra bucket to add on top.
                self.best_snapshot = Some((
                    global_total,
                    TokenUsage {
                        input_tokens: input,
                        output_tokens: output,
                        cache_read_tokens: cached,
                        cache_write_tokens: None,
                        reasoning_tokens: if reasoning > 0 { Some(reasoning) } else { None },
                    },
                ));
            }
        }
    }
}

pub struct CodexParser;

impl CodexParser {
    pub fn parse(&self, file_path: &Path) -> Result<ParsedSession> {
        let file = File::open(file_path).context("Failed to open session file")?;
        let reader = BufReader::new(file);

        let mut lines = reader.lines();

        // First non-empty line must be session_meta
        let mut first_line = None;
        for line in lines.by_ref() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }
            first_line = Some(line);
            break;
        }

        let first_line = match first_line {
            Some(line) => line,
            None => return Err(ParseError::MissingSessionMeta.into()),
        };
        let first_event: Value = match serde_json::from_str(&first_line) {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!("Failed to parse first JSON line: {}", err);
                return Err(ParseError::InvalidSessionMetaJson(err.to_string()).into());
            }
        };

        if first_event.get("type").and_then(|v| v.as_str()) != Some("session_meta") {
            return Err(ParseError::MissingSessionMeta.into());
        }

        let payload = first_event
            .get("payload")
            .context("Session meta payload missing")?;

        let session_id = payload
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .context("Session id missing")?;

        let start_time = payload
            .get("timestamp")
            .and_then(|v| v.as_str())
            .context("Session timestamp missing")
            .and_then(Self::parse_timestamp)?;

        let project_path = payload
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let mut state = ParseState::new(session_id.clone(), start_time);

        for line in lines {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }

            let event: Value = match serde_json::from_str(&line) {
                Ok(event) => event,
                Err(err) => {
                    tracing::warn!("Failed to parse JSON line: {}", err);
                    continue;
                }
            };

            let event_type = event.get("type").and_then(|v| v.as_str());

            let event_ts = event
                .get("timestamp")
                .and_then(|v| v.as_str())
                .and_then(|s| match CodexParser::parse_timestamp(s) {
                    Ok(t) => Some(t),
                    Err(err) => {
                        tracing::warn!("Failed to parse event timestamp {}: {}", s, err);
                        None
                    }
                });

            if let Some(ts) = event_ts {
                state.update_last_updated(ts);
            }

            match event_type {
                Some("response_item") => {
                    if let Some(payload) = event.get("payload") {
                        state.handle_response_item(payload, event_ts);
                    }
                }
                Some("turn_context") => {
                    if let Some(payload) = event.get("payload") {
                        state.handle_turn_context(payload);
                    }
                }
                Some("event_msg") => {
                    if let Some(payload) = event.get("payload") {
                        state.handle_event_msg(payload, event_ts);
                    }
                }
                _ => {}
            }
        }

        if !state.has_user_message {
            return Err(ParseError::NoUserMessages.into());
        }

        state.flush_pending_reasoning_as_orphan();

        let first_prompt = crate::parsers::extract_first_prompt(&state.messages);
        let token_usage = state.best_snapshot.map(|(_, usage)| usage);

        Ok(ParsedSession {
            session: Session {
                id: session_id,
                tool: AiAssistant::Codex,
                project_path,
                project_id: None,
                start_time,
                message_count: state.messages.len(),
                file_path: file_path.to_str().unwrap_or_default().to_string(),
                last_updated: state.last_updated,
                pinned_at: None,
                first_prompt,
                parent_session_id: None,
                is_subagent: false,
                token_usage: None,
                edit_count: 0,
                read_count: 0,
                command_count: 0,
                ending_status: crate::models::SessionEndingStatus::Unknown,
            },
            messages: state.messages,
            tool_calls: state.tool_calls,
            subagents: Vec::new(),
            transcript_items: state.transcript_items,
            reasoning_attachments: state.reasoning_attachments,
            token_usage,
        })
    }

    fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(value)
            .map(|dt| dt.with_timezone(&Utc))
            .context("Failed to parse timestamp")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[test]
    fn parse_valid_session_extracts_messages() {
        let parser = CodexParser;
        let path = PathBuf::from(
            "tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl",
        );
        let parsed = parser.parse(&path).unwrap();
        assert_eq!(parsed.session.id, "019bce9f-0a40-79e2-8351-8818e8487fb6");
        assert_eq!(
            parsed.session.project_path.as_deref(),
            Some("/home/user/project")
        );
        assert_eq!(parsed.session.message_count, 2);
        assert_eq!(
            parsed.session.first_prompt.as_deref(),
            Some("Summarize the repo")
        );
        assert_eq!(parsed.messages[0].role, Role::User);
        assert_eq!(parsed.messages[0].content, "Summarize the repo");
        assert_eq!(parsed.messages[1].role, Role::Assistant);
    }

    #[test]
    fn agent_message_gets_model_from_turn_context() {
        let parsed = CodexParser
            .parse(std::path::Path::new(
                "tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl",
            ))
            .unwrap();
        let assistant_msgs: Vec<_> = parsed
            .messages
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .collect();
        assert_eq!(assistant_msgs[0].model.as_deref(), Some("o3-mini"));
    }

    #[test]
    fn user_message_has_no_model_codex() {
        let parsed = CodexParser
            .parse(std::path::Path::new(
                "tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl",
            ))
            .unwrap();
        let user_msgs: Vec<_> = parsed
            .messages
            .iter()
            .filter(|m| m.role == Role::User)
            .collect();
        assert!(user_msgs[0].model.is_none());
    }

    #[test]
    fn parse_empty_session_is_rejected() {
        let parser = CodexParser;
        let path = PathBuf::from(
            "tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-02-00-empty-session.jsonl",
        );
        let result = parser.parse(&path);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Session contains no user messages")
        );
    }

    #[test]
    fn parse_missing_session_meta_is_rejected() {
        let parser = CodexParser;
        let path = PathBuf::from(
            "tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-03-00-malformed.jsonl",
        );
        let result = parser.parse(&path);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("First line must be session_meta")
        );
    }

    #[test]
    fn parse_tool_session_extracts_tool_calls() {
        let parser = CodexParser;
        let path = PathBuf::from(
            "tests/fixtures/codex_sessions/2026/02/18/rollout-2026-02-18T10-00-00-codex-tools-session.jsonl",
        );
        let parsed = parser.parse(&path).unwrap();
        assert_eq!(parsed.session.id, "codex-tools-session");
        assert_eq!(parsed.messages.len(), 2); // user + agent
        assert_eq!(parsed.tool_calls.len(), 2); // grep + ls
        assert_eq!(parsed.tool_calls[0].id, "call-001");
        assert_eq!(parsed.tool_calls[0].tool_name, "grep");
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Completed);
        assert!(parsed.tool_calls[0].output_text.is_some());
        assert_eq!(parsed.tool_calls[1].id, "call-002");
        assert_eq!(parsed.tool_calls[1].status, ToolCallStatus::Completed);
        // Transcript: user msg, tool call 1, tool call 2, agent msg
        assert_eq!(parsed.transcript_items.len(), 4);
    }

    #[test]
    fn parse_response_item_function_call_flow_extracts_tool_call() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"codex-ri","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"run tests"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"function_call","name":"exec_command","call_id":"call_123","arguments":"{{\"cmd\":\"cargo test\"}}"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:03Z","payload":{{"type":"function_call_output","call_id":"call_123","output":"Process exited with code 0"}}}}"#).unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "call_123");
        assert_eq!(parsed.tool_calls[0].tool_name, "exec_command");
        assert_eq!(
            parsed.tool_calls[0].input_json.as_deref(),
            Some("{\"cmd\":\"cargo test\"}")
        );
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Completed);
        assert_eq!(
            parsed.tool_calls[0].output_text.as_deref(),
            Some("Process exited with code 0")
        );
        assert_eq!(parsed.tool_calls[0].error_text, None);
    }

    #[test]
    fn codex_reasoning_summary_attaches_to_following_tool_call() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"session_meta","payload":{{"id":"codex-r1","timestamp":"2026-04-05T10:00:00Z","cwd":"/tmp"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"event_msg","timestamp":"2026-04-05T10:00:01Z","payload":{{"type":"user_message","message":"Inspect the repo"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"response_item","timestamp":"2026-04-05T10:00:02Z","payload":{{"type":"reasoning","summary":[{{"type":"summary_text","text":"Need project structure first"}}],"encrypted_content":"cipher"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"response_item","timestamp":"2026-04-05T10:00:03Z","payload":{{"type":"function_call","call_id":"call-1","name":"shell","arguments":"{{\"command\":\"pwd\"}}"}}}}"#
        )
        .unwrap();

        let parser = CodexParser;
        let parsed = parser.parse(file.path()).unwrap();
        assert_eq!(parsed.reasoning_attachments.len(), 1);
        assert_eq!(
            parsed.reasoning_attachments[0].summary_text.as_deref(),
            Some("Need project structure first")
        );
        assert_eq!(
            parsed.reasoning_attachments[0].encrypted_content.as_deref(),
            Some("cipher")
        );
        assert_eq!(parsed.reasoning_attachments[0].transcript_item_index, 1);
    }

    #[test]
    fn parse_response_item_custom_tool_call_output_sets_error_status() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"codex-ri-custom","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"run custom"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"custom_tool_call","name":"my_tool","call_id":"call_456","input":"{{\"path\":\"src/main.rs\"}}"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:03Z","payload":{{"type":"custom_tool_call_output","call_id":"call_456","error":"Permission denied"}}}}"#).unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "call_456");
        assert_eq!(parsed.tool_calls[0].tool_name, "my_tool");
        assert_eq!(
            parsed.tool_calls[0].input_json.as_deref(),
            Some("{\"path\":\"src/main.rs\"}")
        );
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Error);
        assert_eq!(parsed.tool_calls[0].output_text, None);
        assert_eq!(
            parsed.tool_calls[0].error_text.as_deref(),
            Some("Permission denied")
        );
    }

    #[test]
    fn parse_mixed_stream_same_call_id_does_not_duplicate_tool_call() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"codex-mixed","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"run mixed"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"function_call","name":"exec_command","call_id":"call_shared","arguments":"{{\"cmd\":\"pwd\"}}"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:03Z","payload":{{"type":"exec_command_begin","call_id":"call_shared","command":"pwd","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:04Z","payload":{{"type":"function_call_output","call_id":"call_shared","status":"completed","output":"/tmp"}}}}"#).unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.transcript_items.len(), 2);
        assert_eq!(parsed.tool_calls[0].id, "call_shared");
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Completed);
    }

    #[test]
    fn parse_duplicate_begin_same_call_id_does_not_duplicate_tool_call() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"codex-dup-begin","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"run dup begin"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"exec_command_begin","call_id":"call_dup","command":"ls","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:03Z","payload":{{"type":"exec_command_begin","call_id":"call_dup","command":"ls","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:04Z","payload":{{"type":"exec_command_end","call_id":"call_dup","exit_code":0,"stdout":"ok"}}}}"#).unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.transcript_items.len(), 2);
        assert_eq!(parsed.tool_calls[0].id, "call_dup");
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Completed);
    }

    #[test]
    fn parse_orphan_response_item_output_without_begin_is_ignored() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"codex-orphan-output","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"run orphan output"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"function_call_output","call_id":"missing_begin","status":"completed","output":"ignored"}}}}"#).unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert!(parsed.tool_calls.is_empty());
        assert_eq!(parsed.transcript_items.len(), 1);
    }

    #[test]
    fn parse_response_item_output_status_failed_maps_to_error_without_error_text() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"codex-ri-status","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"run status"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"function_call","name":"exec_command","call_id":"call_status","arguments":"{{\"cmd\":\"false\"}}"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:03Z","payload":{{"type":"function_call_output","call_id":"call_status","status":"failed","output":"exit 1"}}}}"#).unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Error);
        assert_eq!(parsed.tool_calls[0].error_text, None);
    }

    #[test]
    fn parse_extracts_token_usage_from_highest_snapshot() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"tok-session","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"Hi"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":100,"output_tokens":50,"reasoning_output_tokens":30,"cached_input_tokens":80}}}}}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:03Z","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":500,"output_tokens":200,"reasoning_output_tokens":100,"cached_input_tokens":300}}}}}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:04Z","payload":{{"type":"agent_message","message":"Done"}}}}"#).unwrap();
        let parsed = CodexParser.parse(file.path()).unwrap();
        let usage = parsed.token_usage.expect("should have token_usage");
        assert_eq!(usage.input_tokens, 500);
        assert_eq!(usage.output_tokens, 200);
        assert_eq!(usage.reasoning_tokens, Some(100));
        assert_eq!(usage.cache_read_tokens, Some(300));
        assert_eq!(usage.cache_write_tokens, None);
    }

    #[test]
    fn parse_skips_null_info_token_count() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"tok-null","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"Hi"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"token_count","info":null}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:03Z","payload":{{"type":"agent_message","message":"Done"}}}}"#).unwrap();
        let parsed = CodexParser.parse(file.path()).unwrap();
        assert!(parsed.token_usage.is_none());
    }

    #[derive(Clone, Default)]
    struct BufferWriter {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl BufferWriter {
        fn contents(&self) -> String {
            let buffer = self.buffer.lock().unwrap();
            String::from_utf8_lossy(&buffer).to_string()
        }
    }

    struct BufferGuard {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl std::io::Write for BufferGuard {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut buffer = self.buffer.lock().unwrap();
            buffer.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufferWriter {
        type Writer = BufferGuard;

        fn make_writer(&'a self) -> Self::Writer {
            BufferGuard {
                buffer: Arc::clone(&self.buffer),
            }
        }
    }

    #[test]
    fn parse_invalid_event_timestamp_logs_warning() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"session_meta","payload":{{"id":"session-1","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"event_msg","timestamp":"not-a-ts","payload":{{"type":"user_message","message":"Hi"}}}}"#
        )
        .unwrap();

        let writer = BufferWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_writer(writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let parser = CodexParser;
        let result = parser.parse(file.path());
        assert!(result.is_ok());

        let logs = writer.contents();
        assert!(logs.contains("Failed to parse event timestamp not-a-ts"));
    }
}
