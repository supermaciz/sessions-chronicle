use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::models::{
    AiAssistant, Message, ReasoningAttachment, Role, Session, Subagent, TokenUsage, ToolCall,
    ToolCallStatus,
};
use crate::models::{TranscriptItem, TranscriptItemKind};
use crate::parsers::model::normalize_model;
use crate::parsers::{ParsedSession, PendingReasoning};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("First line must be session_meta")]
    MissingSessionMeta,
    #[error("Session contains no user messages")]
    NoUserMessages,
    #[error("Invalid session_meta JSON: {0}")]
    InvalidSessionMetaJson(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SubagentEventPriority {
    Interaction = 1,
    Waiting = 2,
    Close = 3,
    Resume = 4,
}

#[derive(Debug, Clone)]
struct StatusUpdate {
    summary: Option<String>,
    terminal: bool,
    // true for generic unit labels like "Shutdown"/"Not found" that should
    // never overwrite a detailed summary from `completed`/`errored`.
    coarse: bool,
}

fn parse_status_update(status: &Value) -> StatusUpdate {
    if let Some(unit) = status.as_str() {
        return match unit {
            "pending_init" | "running" | "interrupted" => StatusUpdate {
                summary: None,
                terminal: false,
                coarse: false,
            },
            "shutdown" => StatusUpdate {
                summary: Some("Shutdown".to_string()),
                terminal: true,
                coarse: true,
            },
            "not_found" => StatusUpdate {
                summary: Some("Not found".to_string()),
                terminal: true,
                coarse: true,
            },
            _ => StatusUpdate {
                summary: None,
                terminal: false,
                coarse: false,
            },
        };
    }

    if let Some(completed) = status.get("completed") {
        return StatusUpdate {
            summary: completed.as_str().map(str::to_string),
            terminal: true,
            coarse: false,
        };
    }

    if let Some(errored) = status.get("errored").and_then(|v| v.as_str()) {
        return StatusUpdate {
            summary: Some(format!("Error: {errored}")),
            terminal: true,
            coarse: false,
        };
    }

    StatusUpdate {
        summary: None,
        terminal: false,
        coarse: false,
    }
}

#[derive(Debug, Clone)]
struct PendingSpawn {
    agent_type: Option<String>,
    message: Option<String>,
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
    subagents: Vec<Subagent>,
    transcript_items: Vec<TranscriptItem>,
    reasoning_attachments: Vec<ReasoningAttachment>,

    // Correlation: call_id -> index in tool_calls
    call_id_to_tc_idx: HashMap<String, usize>,
    subagent_idx_by_call_id: HashMap<String, usize>,
    subagent_indexes_by_agent_id: HashMap<String, Vec<usize>>,
    subagent_priority_by_id: HashMap<String, SubagentEventPriority>,
    // Tracks whether the current `result_summary` came from a coarse unit
    // status label, so a later coarse terminal event cannot downgrade a
    // detailed `completed`/`errored` summary.
    subagent_summary_is_coarse: HashMap<String, bool>,
    pending_spawns: HashMap<String, PendingSpawn>,
    pending_waits: HashSet<String>,

    // Counters
    msg_counter: i64,
    item_counter: i64,
    pending_reasoning: PendingReasoning,
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
            subagents: Vec::new(),
            transcript_items: Vec::new(),
            reasoning_attachments: Vec::new(),
            call_id_to_tc_idx: HashMap::new(),
            subagent_idx_by_call_id: HashMap::new(),
            subagent_indexes_by_agent_id: HashMap::new(),
            subagent_priority_by_id: HashMap::new(),
            subagent_summary_is_coarse: HashMap::new(),
            pending_spawns: HashMap::new(),
            pending_waits: HashSet::new(),
            msg_counter: 0,
            item_counter: 0,
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

    fn push_subagent_row(
        &mut self,
        id: String,
        agent_id: Option<String>,
        title: String,
        prompt: Option<String>,
    ) {
        if self.subagent_idx_by_call_id.contains_key(&id) {
            return;
        }

        let subagent_idx = self.subagents.len();
        self.subagents.push(Subagent {
            id: id.clone(),
            agent_id: agent_id.clone(),
            session_id: self.session_id.clone(),
            title,
            prompt,
            result_summary: None,
            child_session_id: None,
            parser_ref: Some(id.clone()),
        });
        self.subagent_idx_by_call_id
            .insert(id.clone(), subagent_idx);
        self.subagent_priority_by_id
            .insert(id.clone(), SubagentEventPriority::Interaction);
        if let Some(agent_id) = agent_id {
            self.subagent_indexes_by_agent_id
                .entry(agent_id)
                .or_default()
                .push(subagent_idx);
        }

        self.transcript_items.push(TranscriptItem {
            session_id: self.session_id.clone(),
            item_index: self.item_counter,
            kind: TranscriptItemKind::Subagent,
            message_index: None,
            tool_call_id: None,
            subagent_id: Some(id),
        });
        self.flush_pending_reasoning_to_item(self.item_counter);
        self.item_counter += 1;
    }

    fn record_subagent_spawn(&mut self, payload: &Value) {
        let call_id = match payload.get("call_id").and_then(|v| v.as_str()) {
            Some(call_id) if !call_id.is_empty() => call_id,
            _ => {
                tracing::warn!("collab_agent_spawn_end missing call_id, skipping");
                return;
            }
        };

        let agent_id = payload
            .get("new_thread_id")
            .and_then(|v| v.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let title = payload
            .get("new_agent_nickname")
            .and_then(|v| v.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("Codex subagent")
            .to_string();
        let prompt = payload
            .get("prompt")
            .and_then(|v| v.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        self.push_subagent_row(call_id.to_string(), agent_id, title, prompt);
    }

    fn update_subagent_from_status(
        &mut self,
        call_id: Option<&str>,
        receiver_thread_id: Option<&str>,
        title: Option<&str>,
        prompt: Option<&str>,
        status: &Value,
        priority: SubagentEventPriority,
    ) {
        // Prefer per-thread resolution when the event carries an explicit
        // thread_id: waiting_end fan-out shares a single call_id across
        // multiple agent_statuses, so resolving by call_id first would
        // funnel every status update onto the same subagent row.
        let subagent_index = receiver_thread_id
            .filter(|id| !id.is_empty())
            .and_then(|thread_id| self.subagent_indexes_by_agent_id.get(thread_id))
            .and_then(|indexes| indexes.last().copied())
            .or_else(|| {
                call_id
                    .filter(|id| !id.is_empty())
                    .and_then(|id| self.subagent_idx_by_call_id.get(id).copied())
            });

        let Some(index) = subagent_index else {
            tracing::debug!("dropping orphan collab enrichment event with no matching spawn");
            return;
        };

        let update = parse_status_update(status);
        let subagent_id = self.subagents[index].id.clone();
        let current_priority = self
            .subagent_priority_by_id
            .get(&subagent_id)
            .copied()
            .unwrap_or(SubagentEventPriority::Interaction);

        {
            let subagent = &mut self.subagents[index];
            if let Some(title) = title.filter(|value| !value.is_empty()) {
                subagent.title = title.to_string();
            }
            if subagent.prompt.is_none() {
                subagent.prompt = prompt.map(str::to_string);
            }
        }

        if !update.terminal || priority < current_priority {
            return;
        }

        if let Some(summary) = update.summary.filter(|value| !value.is_empty()) {
            let current_is_coarse = self
                .subagent_summary_is_coarse
                .get(&subagent_id)
                .copied()
                .unwrap_or(true);
            let has_existing_summary = self.subagents[index]
                .result_summary
                .as_deref()
                .is_some_and(|s| !s.is_empty());

            // Never let a coarse unit label overwrite a detailed summary
            // from `completed`/`errored`, even if the new event has higher
            // priority (e.g. close_end:"shutdown" after waiting_end:completed).
            if update.coarse && has_existing_summary && !current_is_coarse {
                self.subagent_priority_by_id.insert(subagent_id, priority);
                return;
            }

            self.subagents[index].result_summary = Some(summary);
            self.subagent_summary_is_coarse
                .insert(subagent_id.clone(), update.coarse);
            self.subagent_priority_by_id.insert(subagent_id, priority);
            return;
        }

        if self.subagents[index].result_summary.is_none() {
            self.subagents[index].result_summary = Some(String::new());
            self.subagent_summary_is_coarse
                .insert(subagent_id.clone(), true);
        }
        self.subagent_priority_by_id.insert(subagent_id, priority);
    }

    fn flush_pending_reasoning_to_item(&mut self, transcript_item_index: i64) {
        if self.pending_reasoning.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.pending_reasoning);
        self.reasoning_attachments
            .push(pending.into_attachment(&self.session_id, transcript_item_index));
    }

    fn drop_pending_reasoning_if_orphan(&mut self) {
        if self.pending_reasoning.is_empty() {
            return;
        }
        tracing::debug!("dropping orphan reasoning with no visible transcript item");
        self.pending_reasoning = PendingReasoning::default();
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

    fn parse_response_item_json_string(
        response_item: &Value,
        field_name: &str,
        call_id: &str,
        tool_name: &str,
    ) -> Option<Value> {
        let Some(raw) = response_item.get(field_name).and_then(|v| v.as_str()) else {
            tracing::debug!(
                "response-item subagent {tool_name} call {call_id} field {field_name} is missing or is not a JSON string"
            );
            return None;
        };

        match serde_json::from_str(raw) {
            Ok(value) => Some(value),
            Err(err) => {
                tracing::debug!(
                    "failed to parse response-item subagent {tool_name} call {call_id} field {field_name}: {err}"
                );
                None
            }
        }
    }

    fn pending_spawn_from_response_item(response_item: &Value, call_id: &str) -> PendingSpawn {
        let Some(arguments) = Self::parse_response_item_json_string(
            response_item,
            "arguments",
            call_id,
            "spawn_agent",
        ) else {
            return PendingSpawn {
                agent_type: None,
                message: None,
            };
        };

        PendingSpawn {
            agent_type: arguments
                .get("agent_type")
                .and_then(|v| v.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            message: arguments
                .get("message")
                .and_then(|v| v.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        }
    }

    fn complete_pending_spawn(
        &mut self,
        call_id: &str,
        pending: PendingSpawn,
        response_item: &Value,
    ) {
        let Some(output) =
            Self::parse_response_item_json_string(response_item, "output", call_id, "spawn_agent")
        else {
            return;
        };
        let Some(agent_id) = output
            .get("agent_id")
            .and_then(|v| v.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
        else {
            tracing::debug!(
                "dropping response-item spawn_agent output without agent_id for call {call_id}"
            );
            return;
        };
        let title = output
            .get("nickname")
            .and_then(|v| v.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or(pending.agent_type)
            .unwrap_or_else(|| "Codex subagent".to_string());

        self.push_subagent_row(call_id.to_string(), Some(agent_id), title, pending.message);
    }

    fn complete_pending_wait(&mut self, call_id: &str, response_item: &Value) {
        let Some(output) =
            Self::parse_response_item_json_string(response_item, "output", call_id, "wait_agent")
        else {
            return;
        };
        let Some(statuses) = output.get("status").and_then(|v| v.as_object()) else {
            return;
        };

        for (agent_id, status) in statuses {
            self.update_subagent_from_status(
                None,
                Some(agent_id.as_str()),
                None,
                None,
                status,
                SubagentEventPriority::Waiting,
            );
        }
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

                match tool_name.as_str() {
                    "spawn_agent" => {
                        let pending =
                            Self::pending_spawn_from_response_item(response_item, &call_id);
                        self.pending_spawns.insert(call_id, pending);
                        return;
                    }
                    "wait_agent" => {
                        self.pending_waits.insert(call_id);
                        return;
                    }
                    _ => {}
                }

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

                if let Some(pending) = self.pending_spawns.remove(call_id) {
                    self.complete_pending_spawn(call_id, pending, response_item);
                    return;
                }

                if self.pending_waits.take(call_id).is_some() {
                    self.complete_pending_wait(call_id, response_item);
                    return;
                }

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

                let has_encrypted_content = response_item
                    .get("encrypted_content")
                    .or_else(|| response_item.get("encryptedContent"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty());

                let reasoning = PendingReasoning {
                    visible_text,
                    summary_text,
                    has_encrypted_content,
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

            Some("collab_agent_spawn_begin") => {}

            Some("collab_agent_spawn_end") => {
                self.record_subagent_spawn(payload);
            }

            Some("collab_waiting_end") => {
                let call_id = payload.get("call_id").and_then(|v| v.as_str());
                let mut thread_ids_with_agent_status = HashSet::new();

                if let Some(agent_statuses) =
                    payload.get("agent_statuses").and_then(|v| v.as_array())
                {
                    for agent_status in agent_statuses {
                        if let Some(thread_id) =
                            agent_status.get("thread_id").and_then(|v| v.as_str())
                        {
                            thread_ids_with_agent_status.insert(thread_id.to_string());
                        }

                        self.update_subagent_from_status(
                            call_id,
                            agent_status.get("thread_id").and_then(|v| v.as_str()),
                            agent_status.get("agent_nickname").and_then(|v| v.as_str()),
                            None,
                            agent_status.get("status").unwrap_or(&Value::Null),
                            SubagentEventPriority::Waiting,
                        );
                    }
                }

                if let Some(statuses) = payload.get("statuses").and_then(|v| v.as_object()) {
                    for (thread_id, status) in statuses {
                        if thread_ids_with_agent_status.contains(thread_id) {
                            continue;
                        }

                        self.update_subagent_from_status(
                            call_id,
                            Some(thread_id.as_str()),
                            None,
                            None,
                            status,
                            SubagentEventPriority::Waiting,
                        );
                    }
                }
            }

            Some("collab_close_end") => {
                self.update_subagent_from_status(
                    payload.get("call_id").and_then(|v| v.as_str()),
                    payload.get("receiver_thread_id").and_then(|v| v.as_str()),
                    payload
                        .get("receiver_agent_nickname")
                        .and_then(|v| v.as_str()),
                    None,
                    payload.get("status").unwrap_or(&Value::Null),
                    SubagentEventPriority::Close,
                );
            }

            Some("collab_resume_end") => {
                self.update_subagent_from_status(
                    payload.get("call_id").and_then(|v| v.as_str()),
                    payload.get("receiver_thread_id").and_then(|v| v.as_str()),
                    payload
                        .get("receiver_agent_nickname")
                        .and_then(|v| v.as_str()),
                    None,
                    payload.get("status").unwrap_or(&Value::Null),
                    SubagentEventPriority::Resume,
                );
            }

            Some("collab_agent_interaction_end") => {
                self.update_subagent_from_status(
                    payload.get("call_id").and_then(|v| v.as_str()),
                    payload.get("receiver_thread_id").and_then(|v| v.as_str()),
                    payload
                        .get("receiver_agent_nickname")
                        .and_then(|v| v.as_str()),
                    payload.get("prompt").and_then(|v| v.as_str()),
                    payload.get("status").unwrap_or(&Value::Null),
                    SubagentEventPriority::Interaction,
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

// Codex rollouts use two different keys for the subagent source variant:
// upstream `SessionSource` serializes as `sub_agent` (snake_case), but older
// rollouts emit `subagent` (no underscore). Both shapes appear in the wild,
// so accept either.
fn extract_parent_thread_id(source: Option<&Value>) -> Option<String> {
    let source = source?;

    source
        .get("subagent")
        .and_then(|subagent| subagent.get("thread_spawn"))
        .and_then(|spawn| spawn.get("parent_thread_id"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| {
            source
                .get("sub_agent")
                .and_then(|subagent| subagent.get("thread_spawn"))
                .and_then(|spawn| spawn.get("parent_thread_id"))
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
}

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
        let parent_session_id = extract_parent_thread_id(payload.get("source"));
        let is_subagent = parent_session_id.is_some();

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

        state.drop_pending_reasoning_if_orphan();

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
                parent_session_id,
                is_subagent,
                token_usage: None,
                edit_count: 0,
                read_count: 0,
                command_count: 0,
                ending_status: crate::models::SessionEndingStatus::Unknown,
            },
            messages: state.messages,
            tool_calls: state.tool_calls,
            subagents: state.subagents,
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
        assert!(parsed.reasoning_attachments[0].has_encrypted_content);
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

    #[test]
    fn parse_child_session_marks_session_as_subagent_for_sub_agent_source() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"child-sub-agent","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp","source":{{"sub_agent":{{"thread_spawn":{{"parent_thread_id":"019da0bb-541a-74e2-ae0a-6693c5e4fe04"}}}}}}}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"Hi"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"agent_message","message":"Done"}}}}"#).unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();

        assert!(parsed.session.is_subagent);
        assert_eq!(
            parsed.session.parent_session_id.as_deref(),
            Some("019da0bb-541a-74e2-ae0a-6693c5e4fe04")
        );
    }

    #[test]
    fn parse_child_session_marks_session_as_subagent_for_subagent_source() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"child-subagent","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp","source":{{"subagent":{{"thread_spawn":{{"parent_thread_id":"019da0bb-541a-74e2-ae0a-6693c5e4fe04"}}}}}}}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"Hi"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"agent_message","message":"Done"}}}}"#).unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();

        assert!(parsed.session.is_subagent);
        assert_eq!(
            parsed.session.parent_session_id.as_deref(),
            Some("019da0bb-541a-74e2-ae0a-6693c5e4fe04")
        );
    }

    #[test]
    fn parse_collab_agent_spawn_end_creates_subagent_and_transcript_item() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"session_meta","payload":{{"id":"codex-subagent","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"Inspect parser behavior"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"collab_agent_spawn_end","call_id":"call_spawn_1","new_thread_id":"019da0bd-3df2-7191-a1a8-e326b55fe052","new_agent_nickname":"Kierkegaard","prompt":"Inspect the failing parser tests"}}}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();

        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(parsed.subagents[0].id, "call_spawn_1");
        assert_eq!(
            parsed.subagents[0].agent_id.as_deref(),
            Some("019da0bd-3df2-7191-a1a8-e326b55fe052")
        );
        assert_eq!(parsed.subagents[0].title, "Kierkegaard");
        assert_eq!(
            parsed.subagents[0].prompt.as_deref(),
            Some("Inspect the failing parser tests")
        );

        assert_eq!(parsed.transcript_items.len(), 2);
        assert!(matches!(
            parsed.transcript_items[1].kind,
            TranscriptItemKind::Subagent
        ));
        assert_eq!(
            parsed.transcript_items[1].subagent_id.as_deref(),
            Some("call_spawn_1")
        );
    }

    #[test]
    fn parse_collab_agent_spawn_begin_without_end_does_not_create_subagent() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"session_meta","payload":{{"id":"codex-subagent-begin-only","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"Inspect parser behavior"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"collab_agent_spawn_begin","call_id":"call_spawn_1","new_thread_id":"019da0bd-3df2-7191-a1a8-e326b55fe052","new_agent_nickname":"Kierkegaard","prompt":"Inspect the failing parser tests"}}}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();

        assert!(parsed.subagents.is_empty());
        assert_eq!(parsed.transcript_items.len(), 1);
    }

    #[test]
    fn parse_close_end_overrides_waiting_end_summary_for_same_subagent() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"session_meta","payload":{"id":"019da0bb-541a-74e2-ae0a-6693c5e4fe04","timestamp":"2026-04-18T13:17:40Z","cwd":"/tmp/project"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:41Z","payload":{"type":"user_message","message":"Delegate this"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:42Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_1","sender_thread_id":"019da0bb-541a-74e2-ae0a-6693c5e4fe04","new_thread_id":"019da0bd-3df2-7191-a1a8-e326b55fe052","new_agent_nickname":"Kierkegaard","new_agent_role":"default","prompt":"Inspect the failing parser tests","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:43Z","payload":{"type":"collab_waiting_end","call_id":"call_spawn_1","sender_thread_id":"019da0bb-541a-74e2-ae0a-6693c5e4fe04","agent_statuses":[{"thread_id":"019da0bd-3df2-7191-a1a8-e326b55fe052","agent_nickname":"Kierkegaard","agent_role":"default","status":{"completed":"intermediate summary"}}],"statuses":{"019da0bd-3df2-7191-a1a8-e326b55fe052":{"completed":"intermediate summary"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:44Z","payload":{"type":"collab_close_end","call_id":"call_close_1","sender_thread_id":"019da0bb-541a-74e2-ae0a-6693c5e4fe04","receiver_thread_id":"019da0bd-3df2-7191-a1a8-e326b55fe052","receiver_agent_nickname":"Kierkegaard","receiver_agent_role":"default","status":{"completed":"final delegated answer"}}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("final delegated answer")
        );
    }

    #[test]
    fn parse_running_status_does_not_overwrite_completed_summary() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"session_meta","payload":{"id":"codex-non-terminal","timestamp":"2026-04-18T13:17:40Z","cwd":"/tmp/project"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:41Z","payload":{"type":"user_message","message":"Delegate this"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:42Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_1","sender_thread_id":"codex-non-terminal","new_thread_id":"child-1","new_agent_nickname":"Kierkegaard","new_agent_role":"default","prompt":"Inspect the failing parser tests","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:43Z","payload":{"type":"collab_waiting_end","call_id":"call_spawn_1","sender_thread_id":"codex-non-terminal","agent_statuses":[{"thread_id":"child-1","agent_nickname":"Kierkegaard","agent_role":"default","status":{"completed":"done already"}}],"statuses":{"child-1":{"completed":"done already"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:44Z","payload":{"type":"collab_waiting_end","call_id":"call_spawn_1","sender_thread_id":"codex-non-terminal","agent_statuses":[{"thread_id":"child-1","agent_nickname":"Kierkegaard","agent_role":"default","status":"running"}],"statuses":{"child-1":"running"}}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("done already")
        );
    }

    #[test]
    fn parse_unknown_agent_status_does_not_fail_session_parse() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"session_meta","payload":{"id":"codex-unknown-status","timestamp":"2026-04-18T13:17:40Z","cwd":"/tmp/project"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:41Z","payload":{"type":"user_message","message":"Delegate this"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:42Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_1","sender_thread_id":"codex-unknown-status","new_thread_id":"child-1","new_agent_nickname":"Kierkegaard","new_agent_role":"default","prompt":"Inspect the failing parser tests","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:43Z","payload":{"type":"collab_close_end","call_id":"call_close_1","sender_thread_id":"codex-unknown-status","receiver_thread_id":"child-1","receiver_agent_nickname":"Kierkegaard","receiver_agent_role":"default","status":{"paused_for_human":"needs approval"}}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(parsed.subagents.len(), 1);
    }

    #[test]
    fn parse_waiting_end_falls_back_to_statuses_map_when_agent_statuses_is_missing() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"session_meta","payload":{"id":"codex-status-map","timestamp":"2026-04-18T13:17:40Z","cwd":"/tmp/project"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:41Z","payload":{"type":"user_message","message":"Delegate this"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:42Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_1","sender_thread_id":"codex-status-map","new_thread_id":"child-1","new_agent_nickname":"Kierkegaard","new_agent_role":"default","prompt":"Inspect the failing parser tests","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:43Z","payload":{"type":"collab_waiting_end","call_id":"call_spawn_1","sender_thread_id":"codex-status-map","statuses":{"child-1":{"errored":"permission denied"}}}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("Error: permission denied")
        );
    }

    #[test]
    fn parse_waiting_end_falls_back_to_statuses_map_when_agent_statuses_is_partial() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"session_meta","payload":{"id":"codex-status-map-partial","timestamp":"2026-04-18T13:17:40Z","cwd":"/tmp/project"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:41Z","payload":{"type":"user_message","message":"Delegate this"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:42Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_1","sender_thread_id":"codex-status-map-partial","new_thread_id":"child-1","new_agent_nickname":"Kierkegaard","new_agent_role":"default","prompt":"Inspect parser behavior","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:43Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_2","sender_thread_id":"codex-status-map-partial","new_thread_id":"child-2","new_agent_nickname":"Camus","new_agent_role":"default","prompt":"Inspect parser behavior","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:44Z","payload":{"type":"collab_waiting_end","call_id":"call_wait_1","sender_thread_id":"codex-status-map-partial","agent_statuses":[{"thread_id":"child-1","agent_nickname":"Kierkegaard","agent_role":"default","status":{"completed":"first done"}}],"statuses":{"child-1":{"completed":"first done"},"child-2":{"errored":"second failed"}}}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(parsed.subagents.len(), 2);
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("first done")
        );
        assert_eq!(
            parsed.subagents[1].result_summary.as_deref(),
            Some("Error: second failed")
        );
    }

    #[test]
    fn parse_enrichment_without_call_id_updates_latest_matching_thread_row() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"session_meta","payload":{"id":"codex-retry","timestamp":"2026-04-18T13:17:40Z","cwd":"/tmp/project"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:41Z","payload":{"type":"user_message","message":"Delegate twice"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:42Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_1","sender_thread_id":"codex-retry","new_thread_id":"child-1","new_agent_nickname":"Kierkegaard","new_agent_role":"default","prompt":"first attempt","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:43Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_2","sender_thread_id":"codex-retry","new_thread_id":"child-1","new_agent_nickname":"Kierkegaard","new_agent_role":"default","prompt":"second attempt","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:44Z","payload":{"type":"collab_close_end","sender_thread_id":"codex-retry","receiver_thread_id":"child-1","receiver_agent_nickname":"Kierkegaard","receiver_agent_role":"default","status":{"completed":"second attempt finished"}}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(parsed.subagents.len(), 2);
        assert_eq!(parsed.subagents[0].result_summary, None);
        assert_eq!(
            parsed.subagents[1].result_summary.as_deref(),
            Some("second attempt finished")
        );
    }

    #[test]
    fn parse_waiting_end_fans_out_per_thread_when_call_id_matches_a_spawn() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"session_meta","payload":{"id":"codex-fanout","timestamp":"2026-04-18T13:17:40Z","cwd":"/tmp/project"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:41Z","payload":{"type":"user_message","message":"Delegate to two"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:42Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_1","sender_thread_id":"codex-fanout","new_thread_id":"child-1","new_agent_nickname":"Kierkegaard","new_agent_role":"default","prompt":"Inspect A","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:43Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_2","sender_thread_id":"codex-fanout","new_thread_id":"child-2","new_agent_nickname":"Camus","new_agent_role":"default","prompt":"Inspect B","status":"running"}}"#
        )
        .unwrap();
        // waiting_end reuses call_spawn_1 as its call_id, yet carries agent_statuses
        // for BOTH threads. Resolver must fan out per thread, not funnel both
        // updates onto call_spawn_1's row.
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:44Z","payload":{"type":"collab_waiting_end","call_id":"call_spawn_1","sender_thread_id":"codex-fanout","agent_statuses":[{"thread_id":"child-1","agent_nickname":"Kierkegaard","agent_role":"default","status":{"completed":"first done"}},{"thread_id":"child-2","agent_nickname":"Camus","agent_role":"default","status":{"completed":"second done"}}]}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(parsed.subagents.len(), 2);
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("first done")
        );
        assert_eq!(
            parsed.subagents[1].result_summary.as_deref(),
            Some("second done")
        );
    }

    #[test]
    fn parse_close_end_shutdown_does_not_overwrite_detailed_completed_summary() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"session_meta","payload":{"id":"codex-no-downgrade","timestamp":"2026-04-18T13:17:40Z","cwd":"/tmp/project"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:41Z","payload":{"type":"user_message","message":"Delegate this"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:42Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_1","sender_thread_id":"codex-no-downgrade","new_thread_id":"child-1","new_agent_nickname":"Kierkegaard","new_agent_role":"default","prompt":"Inspect","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:43Z","payload":{"type":"collab_waiting_end","call_id":"call_spawn_1","sender_thread_id":"codex-no-downgrade","agent_statuses":[{"thread_id":"child-1","agent_nickname":"Kierkegaard","agent_role":"default","status":{"completed":"detailed delegated answer"}}]}}"#
        )
        .unwrap();
        // close_end with coarse "shutdown" must not downgrade the detailed summary.
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:44Z","payload":{"type":"collab_close_end","call_id":"call_close_1","sender_thread_id":"codex-no-downgrade","receiver_thread_id":"child-1","receiver_agent_nickname":"Kierkegaard","receiver_agent_role":"default","status":"shutdown"}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("detailed delegated answer")
        );
    }

    #[test]
    fn parse_close_end_shutdown_fills_empty_summary() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"session_meta","payload":{"id":"codex-coarse-only","timestamp":"2026-04-18T13:17:40Z","cwd":"/tmp/project"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:41Z","payload":{"type":"user_message","message":"Delegate this"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:42Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_1","sender_thread_id":"codex-coarse-only","new_thread_id":"child-1","new_agent_nickname":"Kierkegaard","new_agent_role":"default","prompt":"Inspect","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:43Z","payload":{"type":"collab_close_end","call_id":"call_close_1","sender_thread_id":"codex-coarse-only","receiver_thread_id":"child-1","receiver_agent_nickname":"Kierkegaard","receiver_agent_role":"default","status":"shutdown"}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("Shutdown")
        );
    }

    #[test]
    fn parse_resume_end_replaces_earlier_coarse_close_summary() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"session_meta","payload":{"id":"codex-resume-over-close","timestamp":"2026-04-18T13:17:40Z","cwd":"/tmp/project"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:41Z","payload":{"type":"user_message","message":"Delegate this"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:42Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_1","new_thread_id":"child-1","new_agent_nickname":"Kierkegaard","prompt":"Inspect","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:43Z","payload":{"type":"collab_close_end","call_id":"call_close_1","receiver_thread_id":"child-1","receiver_agent_nickname":"Kierkegaard","status":"shutdown"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:44Z","payload":{"type":"collab_resume_end","call_id":"call_resume_1","receiver_thread_id":"child-1","receiver_agent_nickname":"Kierkegaard","status":{"completed":"resumed final answer"}}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("resumed final answer")
        );
    }

    #[test]
    fn parse_resume_end_shutdown_does_not_overwrite_detailed_waiting_summary() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"session_meta","payload":{"id":"codex-resume-no-downgrade","timestamp":"2026-04-18T13:17:40Z","cwd":"/tmp/project"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:41Z","payload":{"type":"user_message","message":"Delegate this"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:42Z","payload":{"type":"collab_agent_spawn_end","call_id":"call_spawn_1","new_thread_id":"child-1","new_agent_nickname":"Kierkegaard","prompt":"Inspect","status":"running"}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:43Z","payload":{"type":"collab_waiting_end","call_id":"call_wait_1","agent_statuses":[{"thread_id":"child-1","agent_nickname":"Kierkegaard","status":{"completed":"detailed waiting answer"}}]}}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"{"type":"event_msg","timestamp":"2026-04-18T13:17:44Z","payload":{"type":"collab_resume_end","call_id":"call_resume_1","receiver_thread_id":"child-1","receiver_agent_nickname":"Kierkegaard","status":"shutdown"}}"#
        )
        .unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("detailed waiting answer")
        );
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
