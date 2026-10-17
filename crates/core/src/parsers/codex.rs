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

/// Row title used until a spawn names its subagent.
const GENERIC_SUBAGENT_TITLE: &str = "Codex subagent";

/// Which event family carries user and assistant message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryMode {
    Legacy,
    Paginated,
}

impl HistoryMode {
    /// Absent or any value other than `"paginated"` means legacy, matching
    /// Codex's deserialization default.
    fn from_metadata(payload: &Value) -> Self {
        match payload.get("history_mode").and_then(|v| v.as_str()) {
            Some("paginated") => Self::Paginated,
            _ => Self::Legacy,
        }
    }
}

/// Joins the `text` entries of a typed turn item's `content` array.
fn extract_item_text(item: &Value) -> Option<String> {
    let parts: Vec<&str> = item
        .get("content")?
        .as_array()?
        .iter()
        .filter(|entry| {
            entry
                .get("type")
                .and_then(|v| v.as_str())
                .is_some_and(|kind| kind.eq_ignore_ascii_case("text"))
        })
        .filter_map(|entry| entry.get("text").and_then(|v| v.as_str()))
        .collect();
    let text = parts.join("\n");
    if text.is_empty() { None } else { Some(text) }
}

/// Whether `event` is a transcript carrier inherited from a parent session.
///
/// Metadata and turn context stay visible so retained records can still use
/// them. Records without a valid ordinal are retained.
fn is_inherited_transcript_record(
    event: &Value,
    mode: HistoryMode,
    start_ordinal: Option<u64>,
) -> bool {
    let (HistoryMode::Paginated, Some(start)) = (mode, start_ordinal) else {
        return false;
    };
    let is_carrier = match event.get("type").and_then(|v| v.as_str()) {
        Some("response_item") => true,
        Some("event_msg") => {
            event
                .get("payload")
                .and_then(|p| p.get("type"))
                .and_then(|v| v.as_str())
                != Some("turn_context")
        }
        _ => false,
    };
    is_carrier
        && event
            .get("ordinal")
            .and_then(|v| v.as_u64())
            .is_some_and(|ordinal| ordinal < start)
}

/// Application-generated first prompt for a child session with no user message.
fn synthetic_child_prompt(payload: &Value) -> String {
    let from_path = payload
        .get("agent_path")
        .and_then(|v| v.as_str())
        .and_then(|path| path.rsplit('/').find(|segment| !segment.is_empty()));
    let from_nickname = || {
        payload
            .get("agent_nickname")
            .and_then(|v| v.as_str())
            .filter(|nickname| !nickname.trim().is_empty())
    };
    match from_path.or_else(from_nickname) {
        Some(name) => format!("Delegated task: {name}"),
        None => "Delegated task".to_string(),
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
    history_mode: HistoryMode,
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
            history_mode: HistoryMode::Legacy,
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

    /// Inserts an application-generated user message ahead of all entries and
    /// shifts every stored index that points at the entries behind it.
    fn prepend_synthetic_user(&mut self, content: String, timestamp: DateTime<Utc>) {
        // Reasoning still pending has no entry to attach to; it must not
        // attach to the generated label.
        self.drop_pending_reasoning_if_orphan();

        for message in &mut self.messages {
            message.index += 1;
        }
        for item in &mut self.transcript_items {
            item.item_index += 1;
            if let Some(message_index) = item.message_index.as_mut() {
                *message_index += 1;
            }
        }
        for attachment in &mut self.reasoning_attachments {
            attachment.transcript_item_index += 1;
        }

        self.messages.insert(
            0,
            Message {
                session_id: self.session_id.clone(),
                index: 0,
                role: Role::User,
                content,
                timestamp,
                model: None,
            },
        );
        self.transcript_items.insert(
            0,
            TranscriptItem {
                session_id: self.session_id.clone(),
                item_index: 0,
                kind: TranscriptItemKind::Message,
                message_index: Some(0),
                tool_call_id: None,
                subagent_id: None,
            },
        );
        self.msg_counter += 1;
        self.item_counter += 1;
        self.has_user_message = true;
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
            agent_name: None,
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
        let output =
            Self::parse_response_item_json_string(response_item, "output", call_id, "spawn_agent");

        let agent_id = output
            .as_ref()
            .and_then(|output| output.get("agent_id"))
            .and_then(|v| v.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if agent_id.is_none() {
            tracing::debug!(
                "response-item spawn_agent for call {call_id} has no agent_id; keeping unlinked subagent"
            );
        }

        let nickname = output
            .as_ref()
            .and_then(|output| output.get("nickname"))
            .and_then(|v| v.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        let task_name = output
            .as_ref()
            .and_then(|output| output.get("task_name"))
            .and_then(|v| v.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let task_label = task_name
            .as_deref()
            .and_then(|name| name.rsplit('/').find(|segment| !segment.is_empty()))
            .map(str::to_string);

        if let Some(&subagent_idx) = self.subagent_idx_by_call_id.get(call_id) {
            if let Some(agent_id) = agent_id {
                self.subagents[subagent_idx].agent_id = Some(agent_id.clone());
                self.subagent_indexes_by_agent_id
                    .entry(agent_id)
                    .or_default()
                    .push(subagent_idx);
            }
            if let Some(nickname) = nickname {
                self.subagents[subagent_idx].title = nickname;
            } else if let Some(label) = task_label
                && self.subagents[subagent_idx].title == GENERIC_SUBAGENT_TITLE
            {
                self.subagents[subagent_idx].title = label;
            }
            if task_name.is_some() {
                self.subagents[subagent_idx].agent_name = task_name;
            }
            return;
        }

        // Defensive fallback for malformed or future streams where the output
        // is observed without the begin-side row having been created.
        let title = nickname
            .or(pending.agent_type)
            .or(task_label)
            .unwrap_or_else(|| GENERIC_SUBAGENT_TITLE.to_string());
        self.push_subagent_row(call_id.to_string(), agent_id, title, pending.message);
        if let Some(last) = self.subagents.last_mut() {
            last.agent_name = task_name;
        }
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
                        let title = pending
                            .agent_type
                            .clone()
                            .unwrap_or_else(|| GENERIC_SUBAGENT_TITLE.to_string());
                        self.push_subagent_row(
                            call_id.clone(),
                            None,
                            title,
                            pending.message.clone(),
                        );
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

    fn handle_item_completed(&mut self, payload: &Value, event_ts: Option<DateTime<Utc>>) {
        let Some(item) = payload.get("item") else {
            return;
        };
        let (role, model) = match item.get("type").and_then(|v| v.as_str()) {
            Some("UserMessage") => (Role::User, None),
            Some("AgentMessage") => (Role::Assistant, self.current_turn_model.clone()),
            _ => return,
        };
        let Some(content) = extract_item_text(item) else {
            return;
        };
        self.push_message(role, content, event_ts.unwrap_or_else(Utc::now), model);
    }

    fn handle_event_msg(&mut self, payload: &Value, event_ts: Option<DateTime<Utc>>) {
        match payload.get("type").and_then(|v| v.as_str()) {
            Some("turn_context") => {
                self.handle_turn_context(payload);
            }

            Some("item_completed") if self.history_mode == HistoryMode::Paginated => {
                self.handle_item_completed(payload, event_ts);
            }

            Some("user_message") if self.history_mode == HistoryMode::Legacy => {
                let content = match payload.get("message").and_then(|v| v.as_str()) {
                    Some(c) => c.to_string(),
                    None => return,
                };
                self.push_message(Role::User, content, event_ts.unwrap_or_else(Utc::now), None);
            }

            Some("agent_message") if self.history_mode == HistoryMode::Legacy => {
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

fn open_rollout_reader(file_path: &Path) -> Result<Box<dyn BufRead>> {
    let file = File::open(file_path).context("Failed to open session file")?;
    if file_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".jsonl.zst"))
    {
        let decoder = zstd::stream::read::Decoder::new(file)
            .context("Failed to open compressed session file")?;
        return Ok(Box::new(BufReader::new(decoder)));
    }

    Ok(Box::new(BufReader::new(file)))
}

impl CodexParser {
    pub fn parse(&self, file_path: &Path) -> Result<ParsedSession> {
        let reader = open_rollout_reader(file_path)?;

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
        let parent_session_id = extract_parent_thread_id(payload.get("source")).or_else(|| {
            payload
                .get("parent_thread_id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        });
        let is_subagent = parent_session_id.is_some();
        let agent_path = payload
            .get("agent_path")
            .and_then(|v| v.as_str())
            .filter(|path| !path.is_empty())
            .map(str::to_string);

        let mut state = ParseState::new(session_id.clone(), start_time);
        state.history_mode = HistoryMode::from_metadata(payload);
        let start_ordinal = payload
            .get("subagent_history_start_ordinal")
            .and_then(|v| v.as_u64());

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

            if is_inherited_transcript_record(&event, state.history_mode, start_ordinal) {
                continue;
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
            if !is_subagent {
                return Err(ParseError::NoUserMessages.into());
            }
            state.prepend_synthetic_user(synthetic_child_prompt(payload), start_time);
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
            agent_path,
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

    #[test]
    fn parse_valid_session_extracts_messages() {
        let parser = CodexParser;
        let path = PathBuf::from(crate::fixture_path(
            "codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl",
        ));
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
                &crate::fixture_path("codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl"),
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
                &crate::fixture_path("codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl"),
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
        let path = PathBuf::from(crate::fixture_path(
            "codex_sessions/2026/01/18/rollout-2026-01-18T02-02-00-empty-session.jsonl",
        ));
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
        let path = PathBuf::from(crate::fixture_path(
            "codex_sessions/2026/01/18/rollout-2026-01-18T02-03-00-malformed.jsonl",
        ));
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
        let path = PathBuf::from(crate::fixture_path(
            "codex_sessions/2026/02/18/rollout-2026-02-18T10-00-00-codex-tools-session.jsonl",
        ));
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
    fn parse_response_item_spawn_without_agent_id_still_records_subagent() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"codex-spawn-noid","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"delegate"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"function_call","name":"spawn_agent","call_id":"call_spawn_noid","arguments":"{{\"agent_type\":\"product-manager\",\"message\":\"Advise the next milestone\"}}"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:03Z","payload":{{"type":"function_call_output","call_id":"call_spawn_noid","output":"{{\"error\":\"spawn rejected\"}}"}}}}"#).unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();

        // spawn_agent must not regress to a generic tool call.
        assert!(parsed.tool_calls.is_empty());

        // The spawn action still surfaces as a subagent row, just unlinked.
        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(parsed.subagents[0].id, "call_spawn_noid");
        assert_eq!(parsed.subagents[0].agent_id, None);
        assert_eq!(parsed.subagents[0].title, "product-manager");
        assert_eq!(
            parsed.subagents[0].prompt.as_deref(),
            Some("Advise the next milestone")
        );
        assert!(matches!(
            parsed.transcript_items.last().unwrap().kind,
            TranscriptItemKind::Subagent
        ));
    }

    #[test]
    fn parse_response_item_spawn_without_output_still_records_subagent() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"codex-spawn-truncated","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"delegate"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"function_call","name":"spawn_agent","call_id":"call_spawn_truncated","arguments":"{{\"agent_type\":\"product-manager\",\"message\":\"Advise the next milestone\"}}"}}}}"#).unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();

        assert!(parsed.tool_calls.is_empty());
        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(parsed.subagents[0].id, "call_spawn_truncated");
        assert_eq!(parsed.subagents[0].agent_id, None);
        assert_eq!(parsed.subagents[0].title, "product-manager");
        assert_eq!(
            parsed.subagents[0].prompt.as_deref(),
            Some("Advise the next milestone")
        );
        assert_eq!(
            parsed
                .transcript_items
                .iter()
                .filter(|item| item.kind == TranscriptItemKind::Subagent)
                .count(),
            1
        );
    }

    #[test]
    fn parse_response_item_spawn_with_unparseable_output_still_records_subagent() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"codex-spawn-badout","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"delegate"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"function_call","name":"spawn_agent","call_id":"call_spawn_badout","arguments":"{{\"agent_type\":\"product-manager\",\"message\":\"Advise the next milestone\"}}"}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:03Z","payload":{{"type":"function_call_output","call_id":"call_spawn_badout","output":"{{not json"}}}}"#).unwrap();

        let parsed = CodexParser.parse(file.path()).unwrap();

        // An unexpected output shape must not drop the spawn action entirely.
        assert!(parsed.tool_calls.is_empty());
        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(parsed.subagents[0].id, "call_spawn_badout");
        assert_eq!(parsed.subagents[0].agent_id, None);
        assert_eq!(parsed.subagents[0].title, "product-manager");
        assert_eq!(
            parsed.subagents[0].prompt.as_deref(),
            Some("Advise the next milestone")
        );
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
    fn parse_child_session_uses_direct_parent_thread_id_fallback() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"child-direct-parent","parent_thread_id":"019da0bb-541a-74e2-ae0a-6693c5e4fe04","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp","source":{{"cli":{{}}}}}}}}"#).unwrap();
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
    fn parse_reads_compressed_jsonl_zst_rollout() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir
            .path()
            .join("rollout-2026-01-01T00-00-00-compressed.jsonl.zst");
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = zstd::stream::write::Encoder::new(file, 0).unwrap();
        writeln!(encoder, r#"{{"type":"session_meta","payload":{{"id":"compressed-rollout","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#).unwrap();
        writeln!(encoder, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"Hi compressed"}}}}"#).unwrap();
        writeln!(encoder, r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"agent_message","message":"Done"}}}}"#).unwrap();
        encoder.finish().unwrap();

        let parsed = CodexParser.parse(&path).unwrap();

        assert_eq!(parsed.session.id, "compressed-rollout");
        assert_eq!(parsed.messages[0].content, "Hi compressed");
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

    #[test]
    fn parse_invalid_event_timestamp_continues() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"session_meta","payload":{{"id":"session-1","timestamp":"2026-01-01T00:00:00Z","cwd":"/tmp"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"event_msg","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"user_message","message":"Hi"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"turn_context","timestamp":"not-a-ts","payload":{{"model":"o3-mini"}}}}"#
        )
        .unwrap();

        let parser = CodexParser;
        let parsed = parser.parse(file.path()).unwrap();
        assert_eq!(
            parsed.session.last_updated,
            DateTime::parse_from_rfc3339("2026-01-01T00:00:01Z")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    fn parse_records(records: &[Value]) -> anyhow::Result<ParsedSession> {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for record in records {
            writeln!(file, "{record}").unwrap();
        }
        CodexParser.parse(file.path())
    }

    fn session_meta(overrides: Value) -> Value {
        let mut payload = serde_json::json!({
            "id": "00000000-0000-7000-8000-000000000999",
            "timestamp": "2026-10-15T10:00:00Z",
            "cwd": "/tmp/codex-pagination-fixture",
        });
        if let (Some(base), Some(extra)) = (payload.as_object_mut(), overrides.as_object()) {
            for (key, value) in extra {
                base.insert(key.clone(), value.clone());
            }
        }
        serde_json::json!({
            "timestamp": "2026-10-15T10:00:00Z",
            "type": "session_meta",
            "payload": payload,
        })
    }

    fn item_completed(ts: &str, item: Value) -> Value {
        serde_json::json!({
            "timestamp": ts,
            "type": "event_msg",
            "payload": {"type": "item_completed", "item": item},
        })
    }

    fn user_item(text: &str) -> Value {
        serde_json::json!({"type": "UserMessage", "content": [{"type": "text", "text": text}]})
    }

    fn legacy_user(ts: &str, text: &str) -> Value {
        serde_json::json!({
            "timestamp": ts,
            "type": "event_msg",
            "payload": {"type": "user_message", "message": text},
        })
    }

    fn paginated_root_path() -> PathBuf {
        crate::fixture_path(
            "codex_sessions/2026/10/15/rollout-2026-10-15T10-00-00-paginated-root.jsonl",
        )
    }

    fn ts(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn paginated_messages_use_item_completed() {
        let parsed = CodexParser.parse(&paginated_root_path()).unwrap();

        assert_eq!(
            parsed
                .messages
                .iter()
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>(),
            ["Explain\npagination", "Working", "Done", "Follow-up"]
        );
        assert_eq!(parsed.messages[0].role, Role::User);
        assert_eq!(parsed.messages[0].model, None);
        assert_eq!(parsed.messages[1].model.as_deref(), Some("gpt-5"));
        assert_eq!(
            parsed.session.first_prompt.as_deref(),
            Some("Explain pagination")
        );
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.reasoning_attachments.len(), 1);
        assert_eq!(parsed.reasoning_attachments[0].transcript_item_index, 2);
        assert_eq!(
            parsed
                .transcript_items
                .iter()
                .map(|item| item.kind.clone())
                .collect::<Vec<_>>(),
            [
                TranscriptItemKind::Message,
                TranscriptItemKind::Message,
                TranscriptItemKind::ToolCall,
                TranscriptItemKind::Message,
                TranscriptItemKind::Message,
            ]
        );
        assert_eq!(parsed.messages[0].timestamp, ts("2026-10-15T10:00:03Z"));
        assert_eq!(parsed.messages[1].timestamp, ts("2026-10-15T10:00:04Z"));
        assert_eq!(parsed.messages[2].timestamp, ts("2026-10-15T10:00:08Z"));
        assert_eq!(parsed.messages[3].timestamp, ts("2026-10-15T10:00:09Z"));
    }

    #[test]
    fn paginated_missing_timestamp_falls_back_to_now() {
        for timestamp in [None, Some("not-a-ts")] {
            let mut record = item_completed("x", user_item("Hello"));
            match timestamp {
                Some(value) => record["timestamp"] = Value::String(value.to_string()),
                None => {
                    record.as_object_mut().unwrap().remove("timestamp");
                }
            }
            let before = Utc::now();
            let parsed = parse_records(&[
                session_meta(serde_json::json!({"history_mode": "paginated"})),
                record,
            ])
            .unwrap();
            let after = Utc::now();
            let stamp = parsed.messages[0].timestamp;
            assert!(stamp >= before && stamp <= after);
        }
    }

    #[test]
    fn history_mode_selects_exactly_one_message_source() {
        let modes = [
            (None, false),
            (Some(serde_json::json!("legacy")), false),
            (Some(serde_json::json!("future")), false),
            (Some(serde_json::json!(null)), false),
            (Some(serde_json::json!(42)), false),
            (Some(serde_json::json!({})), false),
            (Some(serde_json::json!("paginated")), true),
        ];
        for (mode, paginated) in modes {
            let overrides = match &mode {
                Some(value) => serde_json::json!({"history_mode": value}),
                None => serde_json::json!({}),
            };
            let parsed = parse_records(&[
                session_meta(overrides),
                legacy_user("2026-10-15T10:00:01Z", "legacy text"),
                item_completed("2026-10-15T10:00:02Z", user_item("typed text")),
            ]);
            let expected = if paginated {
                "typed text"
            } else {
                "legacy text"
            };
            let parsed = parsed.unwrap_or_else(|err| panic!("mode {mode:?}: {err}"));
            assert_eq!(parsed.messages.len(), 1, "mode {mode:?}");
            assert_eq!(parsed.messages[0].content, expected, "mode {mode:?}");
        }
    }

    #[test]
    fn history_mode_is_selected_by_first_session_meta_only() {
        let parsed = parse_records(&[
            session_meta(serde_json::json!({})),
            session_meta(serde_json::json!({"history_mode": "paginated"})),
            legacy_user("2026-10-15T10:00:01Z", "legacy text"),
            item_completed("2026-10-15T10:00:02Z", user_item("typed text")),
        ])
        .unwrap();
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].content, "legacy text");
    }

    #[test]
    fn paginated_text_tolerates_malformed_items() {
        let malformed = [
            serde_json::json!({"type": "UserMessage"}),
            serde_json::json!({"type": "UserMessage", "content": null}),
            serde_json::json!({"type": "UserMessage", "content": "text"}),
            serde_json::json!({"type": "UserMessage", "content": {"text": "x"}}),
            serde_json::json!({"type": "UserMessage", "content": ["text", 3, null]}),
            serde_json::json!({"type": "UserMessage", "content": [{"type": "text"}]}),
            serde_json::json!({"type": "UserMessage", "content": [{"type": "text", "text": 5}]}),
            serde_json::json!({"type": "UserMessage", "content": [{"type": "text", "text": ""}]}),
            serde_json::json!({"type": "UserMessage", "content": [{"type": "skill", "text": "no"}]}),
            serde_json::json!({"type": "AgentMessage", "content": null}),
            serde_json::json!({"type": 7}),
            serde_json::json!("not an object"),
        ];
        let mut records = vec![session_meta(
            serde_json::json!({"history_mode": "paginated"}),
        )];
        for item in malformed {
            records.push(item_completed("2026-10-15T10:00:01Z", item));
        }
        records.push(serde_json::json!({
            "timestamp": "2026-10-15T10:00:02Z",
            "type": "event_msg",
            "payload": {"type": "item_completed"},
        }));
        records.push(item_completed("2026-10-15T10:00:03Z", user_item("kept")));
        let parsed = parse_records(&records).unwrap();
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].content, "kept");
    }

    #[test]
    fn paginated_ignores_non_message_items() {
        let mut records = vec![
            session_meta(serde_json::json!({"history_mode": "paginated"})),
            item_completed("2026-10-15T10:00:01Z", user_item("prompt")),
        ];
        for kind in [
            "CommandExecution",
            "FileChange",
            "McpToolCall",
            "WebSearch",
            "Extension",
            "Plan",
            "Reasoning",
        ] {
            records.push(item_completed(
                "2026-10-15T10:00:02Z",
                serde_json::json!({"type": kind, "content": [{"type": "text", "text": "x"}]}),
            ));
        }
        let parsed = parse_records(&records).unwrap();
        assert_eq!(parsed.messages.len(), 1);
        assert!(parsed.tool_calls.is_empty());
        assert_eq!(parsed.transcript_items.len(), 1);
    }

    #[test]
    fn paginated_empty_root_is_rejected() {
        let path = crate::fixture_path(
            "codex_sessions/2026/10/15/rollout-2026-10-15T10-00-00-paginated-empty.jsonl",
        );
        let err = CodexParser.parse(&path).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<ParseError>(),
            Some(ParseError::NoUserMessages)
        ));
    }

    fn fork_child_path() -> PathBuf {
        crate::fixture_path(
            "codex_subagent_linkage/2026/10/15/rollout-2026-10-15T10-00-00-fork-child.jsonl",
        )
    }

    fn at(ordinal: Value, mut record: Value) -> Value {
        record["ordinal"] = ordinal;
        record
    }

    fn paginated_meta(boundary: Value) -> Value {
        session_meta(serde_json::json!({
            "history_mode": "paginated",
            "subagent_history_start_ordinal": boundary,
        }))
    }

    fn call_record(ts: &str, name: &str, call_id: &str, arguments: &str) -> Value {
        serde_json::json!({
            "timestamp": ts,
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": name,
                "call_id": call_id,
                "arguments": arguments,
            },
        })
    }

    fn output_record(ts: &str, call_id: &str, output: &str) -> Value {
        serde_json::json!({
            "timestamp": ts,
            "type": "response_item",
            "payload": {"type": "function_call_output", "call_id": call_id, "output": output},
        })
    }

    fn reasoning_record(ts: &str, text: &str) -> Value {
        serde_json::json!({
            "timestamp": ts,
            "type": "response_item",
            "payload": {"type": "reasoning", "summary": [{"type": "summary_text", "text": text}]},
        })
    }

    #[test]
    fn paginated_boundary_filters_all_transcript_carriers() {
        let parsed = CodexParser.parse(&fork_child_path()).unwrap();

        assert_eq!(
            parsed
                .messages
                .iter()
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>(),
            ["Own prompt", "Own reply"]
        );
        assert_eq!(parsed.messages[0].model, None);
        assert_eq!(parsed.messages[1].model.as_deref(), Some("gpt-5"));
        assert_eq!(parsed.session.first_prompt.as_deref(), Some("Own prompt"));

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "own_call");
        assert_eq!(
            parsed.tool_calls[0].output_text.as_deref(),
            Some("own output")
        );

        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(parsed.subagents[0].id, "own_spawn");
        assert_eq!(
            parsed.subagents[0].agent_id.as_deref(),
            Some("00000000-0000-7000-8000-0000000002f1")
        );
        assert_eq!(parsed.subagents[0].prompt.as_deref(), Some("Own task"));
        assert_eq!(parsed.subagents[0].result_summary, None);

        assert_eq!(parsed.reasoning_attachments.len(), 1);
        assert_eq!(
            parsed.reasoning_attachments[0].summary_text.as_deref(),
            Some("Own reasoning")
        );
        assert_eq!(parsed.reasoning_attachments[0].transcript_item_index, 2);
        assert_eq!(
            parsed
                .transcript_items
                .iter()
                .map(|item| item.kind.clone())
                .collect::<Vec<_>>(),
            [
                TranscriptItemKind::Message,
                TranscriptItemKind::Message,
                TranscriptItemKind::ToolCall,
                TranscriptItemKind::Subagent,
            ]
        );
    }

    #[test]
    fn paginated_boundary_cannot_seed_pending_state() {
        let ts = "2026-10-15T10:00:01Z";
        let spawn_args = r#"{"message":"inherited task"}"#;

        // Call, spawn and wait begun below the boundary, completed above it.
        let parsed = parse_records(&[
            paginated_meta(serde_json::json!(10)),
            at(
                serde_json::json!(1),
                call_record(ts, "exec_command", "c1", "{}"),
            ),
            at(
                serde_json::json!(2),
                call_record(ts, "spawn_agent", "s1", spawn_args),
            ),
            at(
                serde_json::json!(3),
                call_record(ts, "wait_agent", "w1", "{}"),
            ),
            at(
                serde_json::json!(10),
                item_completed(ts, user_item("own prompt")),
            ),
            at(
                serde_json::json!(11),
                output_record(ts, "c1", "late output"),
            ),
            at(
                serde_json::json!(12),
                output_record(ts, "s1", r#"{"agent_id":"a1","nickname":"Late"}"#),
            ),
            at(
                serde_json::json!(13),
                output_record(ts, "w1", r#"{"status":{"a1":{"completed":"late"}}}"#),
            ),
        ])
        .unwrap();
        assert!(parsed.tool_calls.is_empty());
        assert!(parsed.subagents.is_empty());
        assert_eq!(parsed.transcript_items.len(), 1);

        // Pending reasoning below the boundary must not attach to the first
        // retained message.
        let parsed = parse_records(&[
            paginated_meta(serde_json::json!(10)),
            at(
                serde_json::json!(1),
                reasoning_record(ts, "inherited thought"),
            ),
            at(
                serde_json::json!(10),
                item_completed(ts, user_item("own prompt")),
            ),
        ])
        .unwrap();
        assert!(parsed.reasoning_attachments.is_empty());

        // A retained call reusing an inherited call ID keeps only its own data.
        let parsed = parse_records(&[
            paginated_meta(serde_json::json!(10)),
            at(
                serde_json::json!(1),
                call_record(ts, "exec_command", "dup", r#"{"cmd":"inherited"}"#),
            ),
            at(
                serde_json::json!(2),
                output_record(ts, "dup", "inherited output"),
            ),
            at(
                serde_json::json!(10),
                item_completed(ts, user_item("own prompt")),
            ),
            at(
                serde_json::json!(11),
                call_record(ts, "exec_command", "dup", r#"{"cmd":"own"}"#),
            ),
            at(
                serde_json::json!(12),
                output_record(ts, "dup", "own output"),
            ),
        ])
        .unwrap();
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(
            parsed.tool_calls[0].input_json.as_deref(),
            Some(r#"{"cmd":"own"}"#)
        );
        assert_eq!(
            parsed.tool_calls[0].output_text.as_deref(),
            Some("own output")
        );
    }

    #[test]
    fn paginated_boundary_is_inclusive_and_tolerant() {
        let below_10 = at(
            serde_json::json!(9),
            item_completed("2026-10-15T10:00:01Z", user_item("x")),
        );
        let at_10 = at(
            serde_json::json!(10),
            item_completed("2026-10-15T10:00:01Z", user_item("x")),
        );
        assert!(is_inherited_transcript_record(
            &below_10,
            HistoryMode::Paginated,
            Some(10)
        ));
        assert!(!is_inherited_transcript_record(
            &at_10,
            HistoryMode::Paginated,
            Some(10)
        ));
        assert!(!is_inherited_transcript_record(
            &below_10,
            HistoryMode::Legacy,
            Some(10)
        ));

        // Missing or invalid ordinals are retained.
        for ordinal in [
            None,
            Some(serde_json::json!(null)),
            Some(serde_json::json!(-1)),
            Some(serde_json::json!("9")),
            Some(serde_json::json!(9.5)),
            Some(serde_json::json!({})),
        ] {
            let mut record = item_completed("2026-10-15T10:00:01Z", user_item("kept"));
            if let Some(ordinal) = &ordinal {
                record["ordinal"] = ordinal.clone();
            }
            let parsed = parse_records(&[paginated_meta(serde_json::json!(10)), record]).unwrap();
            assert_eq!(parsed.messages.len(), 1, "ordinal {ordinal:?}");
        }

        // Invalid boundaries disable filtering.
        for boundary in [
            None,
            Some(serde_json::json!(null)),
            Some(serde_json::json!(-1)),
            Some(serde_json::json!("10")),
            Some(serde_json::json!(10.5)),
            Some(serde_json::json!({})),
        ] {
            let meta = match &boundary {
                Some(value) => paginated_meta(value.clone()),
                None => session_meta(serde_json::json!({"history_mode": "paginated"})),
            };
            let parsed = parse_records(&[meta, below_10.clone()]).unwrap();
            assert_eq!(parsed.messages.len(), 1, "boundary {boundary:?}");
        }

        // Boundary zero keeps ordinal zero; huge ordinals do not overflow.
        let parsed = parse_records(&[
            paginated_meta(serde_json::json!(0)),
            at(
                serde_json::json!(0),
                item_completed("2026-10-15T10:00:01Z", user_item("zero")),
            ),
        ])
        .unwrap();
        assert_eq!(parsed.messages.len(), 1);
        let huge = serde_json::json!(u64::MAX);
        let parsed = parse_records(&[
            paginated_meta(serde_json::json!(u64::MAX)),
            at(
                serde_json::json!(1),
                item_completed("2026-10-15T10:00:01Z", user_item("dropped")),
            ),
            at(
                huge,
                item_completed("2026-10-15T10:00:02Z", user_item("kept")),
            ),
        ])
        .unwrap();
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].content, "kept");

        // Both turn-context forms below the boundary still set the model.
        for context in [
            serde_json::json!({"type": "turn_context", "payload": {"model": "gpt-5"}}),
            serde_json::json!({
                "type": "event_msg",
                "payload": {"type": "turn_context", "model": "gpt-5"},
            }),
        ] {
            let parsed = parse_records(&[
                paginated_meta(serde_json::json!(10)),
                at(serde_json::json!(1), context),
                at(
                    serde_json::json!(10),
                    item_completed(
                        "2026-10-15T10:00:01Z",
                        serde_json::json!({
                            "type": "AgentMessage",
                            "content": [{"type": "text", "text": "reply"}],
                        }),
                    ),
                ),
                at(
                    serde_json::json!(11),
                    item_completed("2026-10-15T10:00:02Z", user_item("prompt")),
                ),
            ])
            .unwrap();
            assert_eq!(parsed.messages[0].model.as_deref(), Some("gpt-5"));
        }

        // Legacy sessions ignore the boundary field entirely.
        let parsed = parse_records(&[
            session_meta(serde_json::json!({"subagent_history_start_ordinal": 10})),
            at(
                serde_json::json!(1),
                legacy_user("2026-10-15T10:00:01Z", "legacy prompt"),
            ),
            at(
                serde_json::json!(2),
                call_record("2026-10-15T10:00:02Z", "exec_command", "c1", "{}"),
            ),
        ])
        .unwrap();
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.tool_calls.len(), 1);
    }

    fn linkage_fixture(name: &str) -> PathBuf {
        crate::fixture_path(&format!(
            "codex_subagent_linkage/2026/10/15/rollout-2026-10-15T10-00-00-{name}.jsonl"
        ))
    }

    fn child_overrides(mode: &str, extra: Value) -> Value {
        let mut overrides = serde_json::json!({
            "history_mode": mode,
            "source": {"subagent": {"thread_spawn": {
                "parent_thread_id": "00000000-0000-7000-8000-000000000201",
            }}},
        });
        for (key, value) in extra.as_object().unwrap() {
            overrides[key] = value.clone();
        }
        overrides
    }

    fn assert_no_user_messages(result: anyhow::Result<ParsedSession>) {
        let err = result.unwrap_err();
        assert!(matches!(
            err.downcast_ref::<ParseError>(),
            Some(ParseError::NoUserMessages)
        ));
    }

    fn assistant_item(text: &str) -> Value {
        serde_json::json!({"type": "AgentMessage", "content": [{"type": "text", "text": text}]})
    }

    #[test]
    fn child_without_user_gets_synthetic_prompt() {
        let parsed = CodexParser.parse(&linkage_fixture("task-child")).unwrap();

        assert_eq!(parsed.messages[0].role, Role::User);
        assert_eq!(parsed.messages[0].content, "Delegated task: analyze");
        assert_eq!(parsed.messages[0].index, 0);
        assert_eq!(parsed.messages[0].timestamp, parsed.session.start_time);
        assert_eq!(parsed.messages[0].model, None);
        assert_eq!(
            parsed.session.first_prompt.as_deref(),
            Some("Delegated task: analyze")
        );
        assert_eq!(parsed.session.message_count, parsed.messages.len());
        assert_eq!(parsed.messages[1].content, "Analysis complete");
        assert_eq!(parsed.subagents.len(), 1);

        // Metadata-only children and both history modes receive the prompt.
        for mode in ["legacy", "paginated"] {
            let parsed = parse_records(&[session_meta(child_overrides(
                mode,
                serde_json::json!({"agent_path": "/root/analyze"}),
            ))])
            .unwrap();
            assert_eq!(parsed.messages.len(), 1, "mode {mode}");
            assert_eq!(parsed.transcript_items.len(), 1, "mode {mode}");
            assert_eq!(parsed.messages[0].content, "Delegated task: analyze");

            let parsed = parse_records(&[
                session_meta(child_overrides(
                    mode,
                    serde_json::json!({"agent_path": "/root/analyze"}),
                )),
                item_completed("2026-10-15T10:00:01Z", assistant_item("Reply")),
                serde_json::json!({
                    "timestamp": "2026-10-15T10:00:02Z",
                    "type": "event_msg",
                    "payload": {"type": "agent_message", "message": "Legacy reply"},
                }),
            ])
            .unwrap();
            assert_eq!(parsed.messages[0].content, "Delegated task: analyze");
            assert_eq!(parsed.messages.len(), 2, "mode {mode}");
        }
    }

    #[test]
    fn synthetic_prompt_label_falls_back_through_path_and_nickname() {
        let cases = [
            (
                serde_json::json!({"agent_path": "/root/analyze/"}),
                "Delegated task: analyze",
            ),
            (
                serde_json::json!({"agent_path": "/root//analyze"}),
                "Delegated task: analyze",
            ),
            (
                serde_json::json!({"agent_path": "analyze"}),
                "Delegated task: analyze",
            ),
            (
                serde_json::json!({"agent_path": "/", "agent_nickname": "Ada"}),
                "Delegated task: Ada",
            ),
            (
                serde_json::json!({"agent_path": "", "agent_nickname": "Ada"}),
                "Delegated task: Ada",
            ),
            (
                serde_json::json!({"agent_path": 5, "agent_nickname": "Ada"}),
                "Delegated task: Ada",
            ),
            (
                serde_json::json!({"agent_nickname": "Ada"}),
                "Delegated task: Ada",
            ),
            (serde_json::json!({"agent_nickname": ""}), "Delegated task"),
            (
                serde_json::json!({"agent_nickname": null}),
                "Delegated task",
            ),
            (serde_json::json!({"agent_nickname": 7}), "Delegated task"),
            (serde_json::json!({"agent_path": "///"}), "Delegated task"),
            (serde_json::json!({}), "Delegated task"),
        ];
        for (extra, expected) in cases {
            let parsed =
                parse_records(&[session_meta(child_overrides("paginated", extra.clone()))])
                    .unwrap();
            assert_eq!(parsed.messages[0].content, expected, "metadata {extra}");
        }
    }

    #[test]
    fn synthetic_prompt_shifts_all_references() {
        let ts = "2026-10-15T10:00:01Z";
        let meta = session_meta(child_overrides(
            "paginated",
            serde_json::json!({"agent_path": "/root/analyze"}),
        ));
        let records = [
            meta,
            item_completed(ts, assistant_item("first reply")),
            reasoning_record(ts, "thinking"),
            call_record(ts, "exec_command", "call_a", r#"{"cmd":"ls"}"#),
            output_record(ts, "call_a", "ok"),
            call_record(ts, "spawn_agent", "spawn_a", r#"{"message":"go"}"#),
            item_completed(ts, assistant_item("last reply")),
            reasoning_record(ts, "orphan thinking"),
        ];
        let parsed = parse_records(&records).unwrap();

        // Same stream with a real prompt prepended gives the reference layout.
        let mut with_prompt = records.to_vec();
        with_prompt.insert(1, item_completed(ts, user_item("real prompt")));
        let baseline = parse_records(&with_prompt).unwrap();

        assert_eq!(parsed.messages.len(), baseline.messages.len());
        for (shifted, base) in parsed.messages.iter().zip(&baseline.messages).skip(1) {
            assert_eq!(shifted.index, base.index);
            assert_eq!(shifted.content, base.content);
        }
        assert_eq!(
            parsed.transcript_items.len(),
            baseline.transcript_items.len()
        );
        assert_eq!(parsed.transcript_items[0].kind, TranscriptItemKind::Message);
        assert_eq!(parsed.transcript_items[0].item_index, 0);
        assert_eq!(parsed.transcript_items[0].message_index, Some(0));
        for (shifted, base) in parsed
            .transcript_items
            .iter()
            .zip(&baseline.transcript_items)
            .skip(1)
        {
            assert_eq!(shifted.item_index, base.item_index);
            assert_eq!(shifted.message_index, base.message_index);
            assert_eq!(shifted.tool_call_id, base.tool_call_id);
            assert_eq!(shifted.subagent_id, base.subagent_id);
        }
        assert_eq!(parsed.reasoning_attachments.len(), 1);
        assert_eq!(baseline.reasoning_attachments.len(), 1);
        assert_eq!(
            parsed.reasoning_attachments[0].transcript_item_index,
            baseline.reasoning_attachments[0].transcript_item_index
        );
        assert_eq!(parsed.tool_calls[0].id, "call_a");
        assert_eq!(parsed.subagents[0].id, "spawn_a");
        assert_eq!(parsed.session.message_count, parsed.messages.len());
    }

    #[test]
    fn inherited_only_child_gets_prompt_but_review_is_rejected() {
        let parsed = CodexParser
            .parse(&linkage_fixture("inherited-only-child"))
            .unwrap();
        assert_eq!(parsed.messages[0].content, "Delegated task: inherited");
        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.messages[1].content, "Own reply");

        assert_no_user_messages(CodexParser.parse(&linkage_fixture("inherited-only-review")));
        assert_no_user_messages(parse_records(&[session_meta(serde_json::json!({
            "history_mode": "paginated",
            "agent_path": "/root/orphan",
        }))]));
        assert_no_user_messages(CodexParser.parse(&crate::fixture_path(
            "codex_sessions/2026/10/15/rollout-2026-10-15T10-00-00-paginated-empty.jsonl",
        )));
    }

    #[test]
    fn child_with_user_keeps_original_prompt() {
        for mode in ["legacy", "paginated"] {
            let user = match mode {
                "legacy" => legacy_user("2026-10-15T10:00:01Z", "Own prompt"),
                _ => item_completed("2026-10-15T10:00:01Z", user_item("Own prompt")),
            };
            let parsed = parse_records(&[
                session_meta(child_overrides(
                    mode,
                    serde_json::json!({"agent_path": "/root/analyze"}),
                )),
                user,
            ])
            .unwrap();
            assert_eq!(parsed.messages.len(), 1, "mode {mode}");
            assert_eq!(parsed.messages[0].content, "Own prompt");
            assert_eq!(parsed.session.first_prompt.as_deref(), Some("Own prompt"));
        }
    }

    fn spawn_records(arguments: &str, output: Option<&str>) -> Vec<Value> {
        let ts = "2026-10-15T10:00:02Z";
        let mut records = vec![
            session_meta(serde_json::json!({"history_mode": "paginated"})),
            item_completed("2026-10-15T10:00:01Z", user_item("delegate")),
            call_record(ts, "spawn_agent", "spawn_1", arguments),
        ];
        if let Some(output) = output {
            records.push(output_record(ts, "spawn_1", output));
        }
        records
    }

    #[test]
    fn spawn_output_task_name_sets_agent_name() {
        let parent = CodexParser.parse(&linkage_fixture("task-parent")).unwrap();
        assert_eq!(parent.subagents.len(), 1);
        assert_eq!(
            parent.subagents[0].agent_name.as_deref(),
            Some("/root/analyze")
        );
        assert_eq!(
            parent.subagents[0].prompt.as_deref(),
            Some("Analyze the parser")
        );
        assert_eq!(parent.subagents[0].agent_id, None);
        assert_eq!(parent.subagents[0].title, "analyze");
        assert!(parent.tool_calls.is_empty());

        // (arguments, output, expected agent_id, expected agent_name, expected title)
        let cases = [
            (
                "{}",
                Some(r#"{"task_name":"/root/analyze"}"#),
                None,
                Some("/root/analyze"),
                "analyze",
            ),
            (
                "{}",
                Some(r#"{"agent_id":"a1","task_name":"/root/analyze"}"#),
                Some("a1"),
                Some("/root/analyze"),
                "analyze",
            ),
            (
                "{}",
                Some(r#"{"nickname":"Ada","task_name":"/root/analyze"}"#),
                None,
                Some("/root/analyze"),
                "Ada",
            ),
            (
                r#"{"agent_type":"worker"}"#,
                Some(r#"{"task_name":"/root/analyze"}"#),
                None,
                Some("/root/analyze"),
                "worker",
            ),
            ("{}", Some(r#"{}"#), None, None, "Codex subagent"),
            (
                "{}",
                Some(r#"{"task_name":""}"#),
                None,
                None,
                "Codex subagent",
            ),
            (
                "{}",
                Some(r#"{"task_name":7}"#),
                None,
                None,
                "Codex subagent",
            ),
            (
                "{}",
                Some(r#"{"task_name":null}"#),
                None,
                None,
                "Codex subagent",
            ),
            ("{}", Some("not json"), None, None, "Codex subagent"),
            ("{}", None, None, None, "Codex subagent"),
            (
                "{}",
                Some(r#"{"task_name":"/"}"#),
                None,
                Some("/"),
                "Codex subagent",
            ),
            (
                "{}",
                Some(r#"{"task_name":"/root/analyze/"}"#),
                None,
                Some("/root/analyze/"),
                "analyze",
            ),
        ];
        for (arguments, output, agent_id, agent_name, title) in cases {
            let parsed = parse_records(&spawn_records(arguments, output)).unwrap();
            assert_eq!(parsed.subagents.len(), 1, "output {output:?}");
            let row = &parsed.subagents[0];
            assert_eq!(row.agent_id.as_deref(), agent_id, "output {output:?}");
            assert_eq!(row.agent_name.as_deref(), agent_name, "output {output:?}");
            assert_eq!(row.title, title, "output {output:?}");
            assert!(parsed.tool_calls.is_empty());
        }
    }

    #[test]
    fn codex_metadata_preserves_agent_path() {
        let child = CodexParser.parse(&linkage_fixture("task-child")).unwrap();
        assert_eq!(child.agent_path.as_deref(), Some("/root/analyze"));
        let parent = CodexParser.parse(&linkage_fixture("task-parent")).unwrap();
        assert_eq!(parent.agent_path, None);

        for (value, expected) in [
            (serde_json::json!(""), None),
            (serde_json::json!(5), None),
            (serde_json::json!(null), None),
            (serde_json::json!("/root/x"), Some("/root/x")),
        ] {
            let parsed = parse_records(&[
                session_meta(child_overrides(
                    "paginated",
                    serde_json::json!({"agent_path": value}),
                )),
                item_completed("2026-10-15T10:00:01Z", user_item("hi")),
            ])
            .unwrap();
            assert_eq!(parsed.agent_path.as_deref(), expected, "value {value}");
        }
    }
}
