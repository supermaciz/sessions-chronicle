pub mod json_backend;
pub mod sqlite_backend;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::models::{
    Message, Role, Session, Subagent, Tool, ToolCall, ToolCallStatus, TranscriptItem,
    TranscriptItemKind,
};
use crate::parsers::ParsedSession;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Session contains no user messages")]
    NoUserMessages,
}

pub(crate) struct SessionMetadata {
    pub id: String,
    pub directory: Option<String>,
    pub title: Option<String>,
    pub time_created: DateTime<Utc>,
    pub time_updated: DateTime<Utc>,
    pub parent_id: Option<String>,
}

pub(crate) struct MessageMetadata {
    pub id: String,
    pub role: Option<Role>,
    pub time_created: DateTime<Utc>,
}

pub(crate) struct PartData {
    pub id: String,
    pub kind: String,
    pub order: Option<i64>,
    pub raw: Value,
}

pub(crate) enum PartOutcome {
    Message(Message),
    ToolCall(ToolCall),
    Subagent(Subagent),
    Nothing,
}

pub(crate) struct SessionEntry {
    pub id: String,
    pub source: SessionSource,
}

pub(crate) enum SessionSource {
    JsonFile(PathBuf),
    SqliteRow { db_path: PathBuf },
}

pub(crate) trait OpenCodeBackend {
    fn list_sessions(&self) -> Result<Vec<SessionEntry>>;
    fn load_session_metadata(&self, entry: &SessionEntry) -> Result<SessionMetadata>;
    fn load_messages(&self, session_id: &str) -> Result<Vec<MessageMetadata>>;
    fn load_parts(&self, message_id: &str) -> Result<Vec<PartData>>;
}

pub(crate) fn read_json(path: &Path) -> Result<Value> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).context("Failed to parse JSON")
}

pub(crate) fn timestamp_from_millis(value: i64) -> Result<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp_millis(value).context("Invalid timestamp")
}

pub struct OpenCodeParser {
    storage_root: PathBuf,
}

impl OpenCodeParser {
    pub fn new(storage_root: &Path) -> Self {
        Self {
            storage_root: storage_root.to_path_buf(),
        }
    }

    pub fn parse_entry(
        &self,
        entry: &SessionEntry,
        backend: &dyn OpenCodeBackend,
    ) -> Result<ParsedSession> {
        let metadata = backend.load_session_metadata(entry)?;
        self.build_parsed_session(metadata, &entry.source, backend)
    }

    pub fn parse(&self, session_path: &Path) -> Result<ParsedSession> {
        let backend = json_backend::JsonBackend::new(&self.storage_root);
        let metadata = backend.parse_session_metadata_from_file(session_path)?;
        let source = SessionSource::JsonFile(session_path.to_path_buf());
        self.build_parsed_session_with_backend(metadata, &source, &backend)
    }

    fn build_parsed_session(
        &self,
        metadata: SessionMetadata,
        source: &SessionSource,
        backend: &dyn OpenCodeBackend,
    ) -> Result<ParsedSession> {
        self.build_parsed_session_with_backend(metadata, source, backend)
    }

    fn build_parsed_session_with_backend(
        &self,
        metadata: SessionMetadata,
        source: &SessionSource,
        backend: &dyn OpenCodeBackend,
    ) -> Result<ParsedSession> {
        let is_subagent = metadata.parent_id.is_some();
        let parent_session_id = metadata.parent_id.clone();

        let mut messages = backend.load_messages(&metadata.id)?;
        messages.sort_by(|a, b| {
            a.time_created
                .cmp(&b.time_created)
                .then_with(|| a.id.cmp(&b.id))
        });

        let mut flattened: Vec<Message> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut subagents: Vec<Subagent> = Vec::new();
        let mut transcript_items: Vec<TranscriptItem> = Vec::new();
        let mut has_user_message = false;

        for message in messages {
            let mut parts = backend.load_parts(&message.id)?;
            parts.sort_by(|a, b| match (a.order, b.order) {
                (Some(left), Some(right)) => left.cmp(&right).then_with(|| a.id.cmp(&b.id)),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => a.id.cmp(&b.id),
            });

            for part in parts {
                let outcome = Self::process_part(
                    &metadata.id,
                    &message.id,
                    message.role,
                    message.time_created,
                    &part,
                    &mut has_user_message,
                );

                let item_idx = transcript_items.len() as i64;
                match outcome {
                    PartOutcome::Message(msg) => {
                        let msg_idx = flattened.len() as i64;
                        transcript_items.push(TranscriptItem {
                            session_id: metadata.id.clone(),
                            item_index: item_idx,
                            kind: TranscriptItemKind::Message,
                            message_index: Some(msg_idx),
                            tool_call_id: None,
                            subagent_id: None,
                        });
                        flattened.push(msg);
                    }
                    PartOutcome::ToolCall(tc) => {
                        let tc_id = tc.id.clone();
                        transcript_items.push(TranscriptItem {
                            session_id: metadata.id.clone(),
                            item_index: item_idx,
                            kind: TranscriptItemKind::ToolCall,
                            message_index: None,
                            tool_call_id: Some(tc_id),
                            subagent_id: None,
                        });
                        tool_calls.push(tc);
                    }
                    PartOutcome::Subagent(sa) => {
                        let sa_id = sa.id.clone();
                        transcript_items.push(TranscriptItem {
                            session_id: metadata.id.clone(),
                            item_index: item_idx,
                            kind: TranscriptItemKind::Subagent,
                            message_index: None,
                            tool_call_id: None,
                            subagent_id: Some(sa_id),
                        });
                        subagents.push(sa);
                    }
                    PartOutcome::Nothing => {}
                }
            }
        }

        if !has_user_message {
            return Err(ParseError::NoUserMessages.into());
        }

        for (index, message) in flattened.iter_mut().enumerate() {
            message.index = index;
        }

        let first_prompt = match &metadata.title {
            Some(title) if !title.trim().is_empty() => Some(title.clone()),
            _ => crate::parsers::extract_first_prompt(&flattened),
        };

        let file_path = match source {
            SessionSource::JsonFile(path) => path.to_str().unwrap_or_default().to_string(),
            SessionSource::SqliteRow { db_path } => {
                db_path.to_str().unwrap_or_default().to_string()
            }
        };

        let session = Session {
            id: metadata.id.clone(),
            tool: Tool::OpenCode,
            project_path: metadata.directory.clone(),
            start_time: metadata.time_created,
            message_count: flattened.len(),
            file_path,
            last_updated: metadata.time_updated,
            first_prompt,
            parent_session_id,
            is_subagent,
        };

        Ok(ParsedSession {
            session,
            messages: flattened,
            tool_calls,
            subagents,
            transcript_items,
        })
    }

    fn process_part(
        session_id: &str,
        message_id: &str,
        message_role: Option<Role>,
        timestamp: DateTime<Utc>,
        part: &PartData,
        has_user_message: &mut bool,
    ) -> PartOutcome {
        match part.kind.as_str() {
            "text" => {
                let role = match message_role {
                    Some(role) => role,
                    None => {
                        tracing::warn!(
                            "Missing message role for text part {} in session {}",
                            part.id,
                            session_id
                        );
                        Role::Assistant
                    }
                };

                let text = part
                    .raw
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .filter(|value| !value.trim().is_empty());

                let text = match text {
                    Some(t) => t,
                    None => return PartOutcome::Nothing,
                };

                if role == Role::User {
                    *has_user_message = true;
                }

                PartOutcome::Message(Message {
                    session_id: session_id.to_string(),
                    index: 0,
                    role,
                    content: text,
                    timestamp,
                })
            }
            "tool" => {
                let tool_name = part
                    .raw
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let state = part.raw.get("state");

                let status = match state.and_then(|s| s.get("status")).and_then(|v| v.as_str()) {
                    Some("completed") => ToolCallStatus::Completed,
                    Some("failed") | Some("error") => ToolCallStatus::Error,
                    Some("running") => ToolCallStatus::Running,
                    _ => ToolCallStatus::Unknown,
                };

                let input_json = state.and_then(|s| s.get("input")).map(|v| v.to_string());
                let output_text = state
                    .and_then(|s| s.get("output"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let error_text = state
                    .and_then(|s| s.get("error"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);

                PartOutcome::ToolCall(ToolCall {
                    id: format!("{}-{}-{}", session_id, message_id, part.id),
                    session_id: session_id.to_string(),
                    subagent_id: None,
                    tool_name,
                    status,
                    title: None,
                    summary: None,
                    input_json,
                    output_text,
                    error_text,
                    started_at: None,
                    ended_at: None,
                    duration_ms: None,
                    parser_call_id: None,
                })
            }
            "subtask" => {
                let title = part
                    .raw
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();

                let child_session_id = part
                    .raw
                    .get("childSessionID")
                    .or_else(|| part.raw.get("childSessionId"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);

                let result_summary = part
                    .raw
                    .get("state")
                    .and_then(|s| s.get("output"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);

                PartOutcome::Subagent(Subagent {
                    id: format!("{}-{}-{}", session_id, message_id, part.id),
                    session_id: session_id.to_string(),
                    title,
                    prompt: None,
                    result_summary,
                    child_session_id,
                    parser_ref: None,
                })
            }
            "reasoning" | "step-start" | "step-finish" | "snapshot" | "compaction" => {
                PartOutcome::Nothing
            }
            other => {
                tracing::debug!("Unhandled part type: {}", other);
                PartOutcome::Nothing
            }
        }
    }
}
