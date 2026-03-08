pub mod json_backend;
pub mod sqlite_backend;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::models::{
    AiAssistant, Message, Role, Session, Subagent, TokenUsage, ToolCall, ToolCallStatus,
    TranscriptItem, TranscriptItemKind,
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
    pub model: Option<String>,
    pub tokens: Option<MessageTokens>,
}

pub(crate) struct MessageTokens {
    pub input: i64,
    pub output: i64,
    pub reasoning: Option<i64>,
    pub cache_read: Option<i64>,
    pub cache_write: Option<i64>,
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
    StepFinishTokens(MessageTokens),
    Nothing,
}

pub(crate) fn extract_tokens(tokens_val: &Value) -> Option<MessageTokens> {
    let input = tokens_val
        .get("input")
        .or_else(|| tokens_val.get("prompt"))
        .and_then(|v| v.as_i64())?;
    let output = tokens_val
        .get("output")
        .or_else(|| tokens_val.get("completion"))
        .and_then(|v| v.as_i64())?;
    let reasoning = tokens_val
        .get("reasoning")
        .and_then(|v| v.as_i64())
        .filter(|&v| v > 0);
    let cache_read = tokens_val
        .get("cache")
        .and_then(|c| c.get("read"))
        .and_then(|v| v.as_i64())
        .filter(|&v| v > 0);
    let cache_write = tokens_val
        .get("cache")
        .and_then(|c| c.get("write"))
        .and_then(|v| v.as_i64())
        .filter(|&v| v > 0);
    Some(MessageTokens {
        input,
        output,
        reasoning,
        cache_read,
        cache_write,
    })
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

    pub(crate) fn parse_entry(
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
        self.build_parsed_session(metadata, &source, &backend)
    }

    fn build_parsed_session(
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
        let mut step_finish_tokens: Vec<MessageTokens> = Vec::new();
        let mut has_message_level_tokens = false;
        let mut msg_level_input: i64 = 0;
        let mut msg_level_output: i64 = 0;
        let mut msg_level_reasoning: i64 = 0;
        let mut msg_level_cache_read: i64 = 0;
        let mut msg_level_cache_write: i64 = 0;

        for message in &messages {
            if let Some(ref tok) = message.tokens {
                has_message_level_tokens = true;
                msg_level_input += tok.input;
                msg_level_output += tok.output;
                msg_level_reasoning += tok.reasoning.unwrap_or(0);
                msg_level_cache_read += tok.cache_read.unwrap_or(0);
                msg_level_cache_write += tok.cache_write.unwrap_or(0);
            }
        }

        for message in &messages {
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
                    message.model.as_deref(),
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
                    PartOutcome::StepFinishTokens(tok) => {
                        step_finish_tokens.push(tok);
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

        // Aggregate token usage: prefer message-level tokens, fall back to step-finish
        let token_usage = if has_message_level_tokens {
            Some(TokenUsage {
                input_tokens: msg_level_input,
                output_tokens: msg_level_output,
                reasoning_tokens: if msg_level_reasoning > 0 {
                    Some(msg_level_reasoning)
                } else {
                    None
                },
                cache_read_tokens: if msg_level_cache_read > 0 {
                    Some(msg_level_cache_read)
                } else {
                    None
                },
                cache_write_tokens: if msg_level_cache_write > 0 {
                    Some(msg_level_cache_write)
                } else {
                    None
                },
            })
        } else if !step_finish_tokens.is_empty() {
            let mut sf_input: i64 = 0;
            let mut sf_output: i64 = 0;
            let mut sf_reasoning: i64 = 0;
            let mut sf_cache_read: i64 = 0;
            let mut sf_cache_write: i64 = 0;
            for tok in &step_finish_tokens {
                sf_input += tok.input;
                sf_output += tok.output;
                sf_reasoning += tok.reasoning.unwrap_or(0);
                sf_cache_read += tok.cache_read.unwrap_or(0);
                sf_cache_write += tok.cache_write.unwrap_or(0);
            }
            Some(TokenUsage {
                input_tokens: sf_input,
                output_tokens: sf_output,
                reasoning_tokens: if sf_reasoning > 0 {
                    Some(sf_reasoning)
                } else {
                    None
                },
                cache_read_tokens: if sf_cache_read > 0 {
                    Some(sf_cache_read)
                } else {
                    None
                },
                cache_write_tokens: if sf_cache_write > 0 {
                    Some(sf_cache_write)
                } else {
                    None
                },
            })
        } else {
            None
        };

        let file_path = match source {
            SessionSource::JsonFile(path) => path.to_str().unwrap_or_default().to_string(),
            SessionSource::SqliteRow { db_path } => {
                db_path.to_str().unwrap_or_default().to_string()
            }
        };

        let session = Session {
            id: metadata.id.clone(),
            tool: AiAssistant::OpenCode,
            project_path: metadata.directory.clone(),
            start_time: metadata.time_created,
            message_count: flattened.len(),
            file_path,
            last_updated: metadata.time_updated,
            first_prompt,
            parent_session_id,
            is_subagent,
            token_usage: None,
        };

        Ok(ParsedSession {
            session,
            messages: flattened,
            tool_calls,
            subagents,
            transcript_items,
            token_usage,
        })
    }

    fn process_part(
        session_id: &str,
        message_id: &str,
        message_role: Option<Role>,
        message_model: Option<&str>,
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

                let model = if role == Role::User {
                    None
                } else {
                    message_model.map(str::to_string)
                };

                PartOutcome::Message(Message {
                    session_id: session_id.to_string(),
                    index: 0,
                    role,
                    content: text,
                    timestamp,
                    model,
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
                    prompt: part
                        .raw
                        .get("prompt")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    result_summary,
                    child_session_id,
                    parser_ref: None,
                })
            }
            "step-finish" => {
                if let Some(tok) = part.raw.get("tokens").and_then(extract_tokens) {
                    PartOutcome::StepFinishTokens(tok)
                } else {
                    PartOutcome::Nothing
                }
            }
            "reasoning" | "step-start" | "snapshot" | "compaction" => PartOutcome::Nothing,
            other => {
                tracing::debug!("Unhandled part type: {}", other);
                PartOutcome::Nothing
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn write_json_file(path: &Path, value: &serde_json::Value) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    }

    fn write_session_file(
        root: &Path,
        project: &str,
        filename: &str,
        value: serde_json::Value,
    ) -> PathBuf {
        let path = root.join("session").join(project).join(filename);
        write_json_file(&path, &value);
        path
    }

    fn write_message_file(
        root: &Path,
        session_id: &str,
        filename: &str,
        value: serde_json::Value,
    ) -> PathBuf {
        let path = root.join("message").join(session_id).join(filename);
        write_json_file(&path, &value);
        path
    }

    fn write_part_file(root: &Path, message_id: &str, filename: &str, value: serde_json::Value) {
        let path = root.join("part").join(message_id).join(filename);
        write_json_file(&path, &value);
    }

    #[test]
    fn parse_session_metadata_extracts_fields() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let created = 1_704_067_200_000i64;
        let updated = 1_704_067_260_000i64;

        let session_path = write_session_file(
            root,
            "project-a",
            "session-001.json",
            json!({
                "id": "session-001",
                "directory": "/projects/alpha",
                "time": { "created": created, "updated": updated }
            }),
        );

        let backend = json_backend::JsonBackend::new(root);
        let metadata = backend
            .parse_session_metadata_from_file(&session_path)
            .unwrap();

        assert_eq!(metadata.id, "session-001");
        assert_eq!(metadata.directory.as_deref(), Some("/projects/alpha"));
        assert_eq!(
            metadata.time_created,
            DateTime::<Utc>::from_timestamp_millis(created).unwrap()
        );
        assert_eq!(
            metadata.time_updated,
            DateTime::<Utc>::from_timestamp_millis(updated).unwrap()
        );
    }

    #[test]
    fn parse_subagent_session_without_messages_fails() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "session-002.json",
            json!({
                "id": "session-002",
                "parentID": "session-001",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        let parser = OpenCodeParser::new(root);
        let result = parser.parse(&session_path);

        assert!(result.is_err());
    }

    #[test]
    fn parse_subagent_session_is_indexed_with_flag() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "child-session.json",
            json!({
                "id": "child-session",
                "parentID": "parent-session",
                "directory": "/projects/alpha",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "child-session",
            "msg-001.json",
            json!({
                "id": "msg-001",
                "sessionID": "child-session",
                "role": "user",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );

        write_part_file(
            root,
            "msg-001",
            "part-001.json",
            json!({
                "id": "part-001",
                "order": 1,
                "type": "text",
                "text": "Do the subtask"
            }),
        );

        let parser = OpenCodeParser::new(root);
        let parsed = parser.parse(&session_path).unwrap();

        assert!(parsed.session.is_subagent);
        assert_eq!(
            parsed.session.parent_session_id.as_deref(),
            Some("parent-session")
        );
        assert_eq!(parsed.messages.len(), 1);
    }

    #[test]
    fn parse_skips_sessions_without_user_messages() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "session-003.json",
            json!({
                "id": "session-003",
                "directory": "/projects/alpha",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-003",
            "msg-001.json",
            json!({
                "id": "msg-001",
                "sessionID": "session-003",
                "role": "assistant",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );

        write_part_file(
            root,
            "msg-001",
            "part-001.json",
            json!({
                "id": "part-001",
                "type": "text",
                "text": "Hello"
            }),
        );

        let parser = OpenCodeParser::new(root);
        let result = parser.parse(&session_path);

        assert!(result.is_err());
    }

    #[test]
    fn load_parts_handles_missing_files() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let backend = json_backend::JsonBackend::new(root);

        let parts = backend.load_parts("missing-msg").unwrap();
        assert!(parts.is_empty());
    }

    #[test]
    fn message_reconstruction_orders_correctly() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "session-004.json",
            json!({
                "id": "session-004",
                "directory": "/projects/alpha",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-004",
            "msg-001.json",
            json!({
                "id": "msg-001",
                "sessionID": "session-004",
                "role": "assistant",
                "time": { "created": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-004",
            "msg-002.json",
            json!({
                "id": "msg-002",
                "sessionID": "session-004",
                "role": "user",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );

        write_part_file(
            root,
            "msg-002",
            "part-002.json",
            json!({
                "id": "part-002",
                "order": 2,
                "type": "text",
                "text": "Second"
            }),
        );
        write_part_file(
            root,
            "msg-002",
            "part-001.json",
            json!({
                "id": "part-001",
                "order": 1,
                "type": "text",
                "text": "First"
            }),
        );

        write_part_file(
            root,
            "msg-001",
            "part-001.json",
            json!({
                "id": "part-001",
                "order": 1,
                "type": "tool",
                "tool": "grep",
                "state": { "input": { "pattern": "rust" } }
            }),
        );

        let parser = OpenCodeParser::new(root);
        let parsed = parser.parse(&session_path).unwrap();

        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.messages[0].index, 0);
        assert_eq!(parsed.messages[0].role, Role::User);
        assert_eq!(parsed.messages[0].content, "First");
        assert_eq!(parsed.messages[1].index, 1);
        assert_eq!(parsed.messages[1].role, Role::User);
        assert_eq!(parsed.messages[1].content, "Second");
        assert_eq!(parsed.session.first_prompt.as_deref(), Some("First"));

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].tool_name, "grep");
    }

    #[test]
    fn message_reconstruction_breaks_ties_by_id() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "session-005.json",
            json!({
                "id": "session-005",
                "directory": "/projects/alpha",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-005",
            "a.json",
            json!({
                "id": "msg-002",
                "sessionID": "session-005",
                "role": "assistant",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-005",
            "b.json",
            json!({
                "id": "msg-001",
                "sessionID": "session-005",
                "role": "user",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );

        write_part_file(
            root,
            "msg-001",
            "part-001.json",
            json!({
                "id": "part-001",
                "order": 1,
                "type": "text",
                "text": "First message"
            }),
        );

        write_part_file(
            root,
            "msg-002",
            "part-001.json",
            json!({
                "id": "part-001",
                "order": 1,
                "type": "text",
                "text": "Second message"
            }),
        );

        let parser = OpenCodeParser::new(root);
        let parsed = parser.parse(&session_path).unwrap();

        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.messages[0].role, Role::User);
        assert_eq!(parsed.messages[0].content, "First message");
        assert_eq!(parsed.messages[1].role, Role::Assistant);
        assert_eq!(parsed.messages[1].content, "Second message");
    }

    #[test]
    fn message_reconstruction_skips_invalid_entries() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "session-006.json",
            json!({
                "id": "session-006",
                "directory": "/projects/alpha",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-006",
            "msg-valid.json",
            json!({
                "id": "msg-valid",
                "sessionID": "session-006",
                "role": "user",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-006",
            "msg-invalid.json",
            json!({
                "id": "msg-invalid",
                "sessionID": "session-006",
                "role": "assistant"
            }),
        );

        write_part_file(
            root,
            "msg-valid",
            "part-valid.json",
            json!({
                "id": "part-valid",
                "order": 1,
                "type": "text",
                "text": "Hello"
            }),
        );

        write_part_file(
            root,
            "msg-valid",
            "part-invalid.json",
            json!({
                "id": "part-invalid",
                "text": "Ignore"
            }),
        );

        let parser = OpenCodeParser::new(root);
        let parsed = parser.parse(&session_path).unwrap();

        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].role, Role::User);
        assert_eq!(parsed.messages[0].content, "Hello");
    }

    #[test]
    fn tool_part_produces_tool_call_not_message() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "session-007.json",
            json!({
                "id": "session-007",
                "directory": "/projects/alpha",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-007",
            "msg-user.json",
            json!({
                "id": "msg-user",
                "sessionID": "session-007",
                "role": "user",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-007",
            "msg-tool.json",
            json!({
                "id": "msg-tool",
                "sessionID": "session-007",
                "role": "assistant",
                "time": { "created": 1_704_067_260_000i64 }
            }),
        );

        write_part_file(
            root,
            "msg-user",
            "part-user.json",
            json!({
                "id": "part-user",
                "order": 1,
                "type": "text",
                "text": "Run tool"
            }),
        );

        write_part_file(
            root,
            "msg-tool",
            "part-tool.json",
            json!({
                "id": "part-tool",
                "order": 1,
                "type": "tool",
                "tool": "read",
                "state": {
                    "status": "completed",
                    "input": { "path": "/tmp/test.txt" }
                }
            }),
        );

        let parser = OpenCodeParser::new(root);
        let parsed = parser.parse(&session_path).unwrap();

        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].role, Role::User);
        assert_eq!(parsed.messages[0].content, "Run tool");

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].tool_name, "read");
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Completed);
    }

    #[test]
    fn tool_part_with_output_is_extracted() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "session-009.json",
            json!({
                "id": "session-009",
                "directory": "/projects/alpha",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-009",
            "msg-user.json",
            json!({
                "id": "msg-user",
                "sessionID": "session-009",
                "role": "user",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-009",
            "msg-tool.json",
            json!({
                "id": "msg-tool",
                "sessionID": "session-009",
                "role": "assistant",
                "time": { "created": 1_704_067_260_000i64 }
            }),
        );

        write_part_file(
            root,
            "msg-user",
            "part-user.json",
            json!({
                "id": "part-user",
                "order": 1,
                "type": "text",
                "text": "Read file"
            }),
        );

        write_part_file(
            root,
            "msg-tool",
            "part-tool.json",
            json!({
                "id": "part-tool",
                "order": 1,
                "type": "tool",
                "tool": "read",
                "state": {
                    "status": "completed",
                    "input": { "path": "/tmp/test.txt" },
                    "output": "File contents here\nLine 2\nLine 3"
                }
            }),
        );

        let parser = OpenCodeParser::new(root);
        let parsed = parser.parse(&session_path).unwrap();

        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].content, "Read file");

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(
            parsed.tool_calls[0].output_text.as_deref(),
            Some("File contents here\nLine 2\nLine 3")
        );
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Completed);
    }

    #[test]
    fn tool_part_with_error_captures_error_text() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "session-010.json",
            json!({
                "id": "session-010",
                "directory": "/projects/alpha",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-010",
            "msg-user.json",
            json!({
                "id": "msg-user",
                "sessionID": "session-010",
                "role": "user",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-010",
            "msg-tool.json",
            json!({
                "id": "msg-tool",
                "sessionID": "session-010",
                "role": "assistant",
                "time": { "created": 1_704_067_260_000i64 }
            }),
        );

        write_part_file(
            root,
            "msg-user",
            "part-user.json",
            json!({
                "id": "part-user",
                "order": 1,
                "type": "text",
                "text": "Read file"
            }),
        );

        write_part_file(
            root,
            "msg-tool",
            "part-tool.json",
            json!({
                "id": "part-tool",
                "order": 1,
                "type": "tool",
                "tool": "read",
                "state": {
                    "status": "failed",
                    "input": { "path": "/tmp/missing.txt" },
                    "error": "File not found"
                }
            }),
        );

        let parser = OpenCodeParser::new(root);
        let parsed = parser.parse(&session_path).unwrap();

        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].content, "Read file");

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Error);
        assert_eq!(
            parsed.tool_calls[0].error_text.as_deref(),
            Some("File not found")
        );
    }

    #[test]
    fn subtask_part_produces_subagent_entry() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "session-sub.json",
            json!({
                "id": "session-sub",
                "directory": "/projects/alpha",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-sub",
            "msg-user.json",
            json!({
                "id": "msg-user",
                "sessionID": "session-sub",
                "role": "user",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-sub",
            "msg-asst.json",
            json!({
                "id": "msg-asst",
                "sessionID": "session-sub",
                "role": "assistant",
                "time": { "created": 1_704_067_260_000i64 }
            }),
        );

        write_part_file(
            root,
            "msg-user",
            "part-user.json",
            json!({
                "id": "part-user",
                "order": 1,
                "type": "text",
                "text": "Analyse docs"
            }),
        );

        write_part_file(
            root,
            "msg-asst",
            "part-subtask.json",
            json!({
                "id": "part-subtask",
                "order": 1,
                "type": "subtask",
                "description": "Summarise markdown files",
                "prompt": "## Review\n\n- inspect parser\n- report issues",
                "childSessionID": "session-child-123",
                "state": {
                    "status": "completed",
                    "output": "Found 3 files"
                }
            }),
        );

        let parser = OpenCodeParser::new(root);
        let parsed = parser.parse(&session_path).unwrap();

        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(parsed.subagents[0].title, "Summarise markdown files");
        assert_eq!(
            parsed.subagents[0].prompt.as_deref(),
            Some("## Review\n\n- inspect parser\n- report issues")
        );
        assert_eq!(
            parsed.subagents[0].child_session_id.as_deref(),
            Some("session-child-123")
        );
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("Found 3 files")
        );

        assert_eq!(parsed.transcript_items.len(), 2);
        assert_eq!(parsed.transcript_items[0].kind, TranscriptItemKind::Message);
        assert_eq!(
            parsed.transcript_items[1].kind,
            TranscriptItemKind::Subagent
        );
    }

    #[test]
    fn opencode_assistant_message_gets_model() {
        use crate::parsers::opencode::json_backend::JsonBackend;

        let storage_root = Path::new("tests/fixtures/opencode_storage");
        let parser = OpenCodeParser::new(storage_root);
        let json_backend = JsonBackend::new(storage_root);
        let entries = json_backend.list_sessions().unwrap();
        let entry = entries.iter().find(|e| e.id == "session-001").unwrap();
        let parsed = parser.parse_entry(entry, &json_backend).unwrap();

        let assistant_msgs: Vec<_> = parsed
            .messages
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .collect();
        assert!(!assistant_msgs.is_empty());
        assert_eq!(
            assistant_msgs[0].model.as_deref(),
            Some("anthropic/claude-sonnet-4-5")
        );
    }

    #[test]
    fn parse_extracts_token_usage_from_message_tokens() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "session-tok.json",
            json!({
                "id": "session-tok",
                "directory": "/projects/alpha",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-tok",
            "msg-user.json",
            json!({
                "id": "msg-user", "sessionID": "session-tok", "role": "user",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );
        write_message_file(
            root,
            "session-tok",
            "msg-asst.json",
            json!({
                "id": "msg-asst", "sessionID": "session-tok", "role": "assistant",
                "time": { "created": 1_704_067_260_000i64 },
                "data": { "tokens": { "input": 1000, "output": 500, "reasoning": 200, "cache": { "read": 300, "write": 50 } } }
            }),
        );

        write_part_file(
            root,
            "msg-user",
            "part-user.json",
            json!({
                "id": "part-user", "order": 1, "type": "text", "text": "Hello"
            }),
        );
        write_part_file(
            root,
            "msg-asst",
            "part-asst.json",
            json!({
                "id": "part-asst", "order": 1, "type": "text", "text": "Hi!"
            }),
        );

        let parser = OpenCodeParser::new(root);
        let parsed = parser.parse(&session_path).unwrap();
        let usage = parsed.token_usage.expect("should have token_usage");
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.reasoning_tokens, Some(200));
        assert_eq!(usage.cache_read_tokens, Some(300));
        assert_eq!(usage.cache_write_tokens, Some(50));
    }

    #[test]
    fn parse_step_finish_fallback_when_no_message_tokens() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "session-tok-sf.json",
            json!({
                "id": "session-tok-sf",
                "directory": "/projects/alpha",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-tok-sf",
            "msg-user.json",
            json!({
                "id": "msg-user", "sessionID": "session-tok-sf", "role": "user",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );
        write_message_file(
            root,
            "session-tok-sf",
            "msg-asst.json",
            json!({
                "id": "msg-asst", "sessionID": "session-tok-sf", "role": "assistant",
                "time": { "created": 1_704_067_260_000i64 }
            }),
        );

        write_part_file(
            root,
            "msg-user",
            "part-user.json",
            json!({
                "id": "part-user", "order": 1, "type": "text", "text": "Hello"
            }),
        );
        write_part_file(
            root,
            "msg-asst",
            "part-sf.json",
            json!({
                "id": "part-sf", "order": 1, "type": "step-finish",
                "tokens": { "input": 800, "output": 400 }
            }),
        );
        write_part_file(
            root,
            "msg-asst",
            "part-text.json",
            json!({
                "id": "part-text", "order": 2, "type": "text", "text": "Hi!"
            }),
        );

        let parser = OpenCodeParser::new(root);
        let parsed = parser.parse(&session_path).unwrap();
        let usage = parsed
            .token_usage
            .expect("should have token_usage from step-finish");
        assert_eq!(usage.input_tokens, 800);
        assert_eq!(usage.output_tokens, 400);
    }

    #[test]
    fn parse_no_tokens_anywhere_yields_none() {
        let storage_root = std::path::Path::new("tests/fixtures/opencode_storage");
        let parser = OpenCodeParser::new(storage_root);
        let json_backend = json_backend::JsonBackend::new(storage_root);
        let entries = json_backend.list_sessions().unwrap();
        let entry = entries.iter().find(|e| e.id == "session-001").unwrap();
        let parsed = parser.parse_entry(entry, &json_backend).unwrap();
        assert!(parsed.token_usage.is_none());
    }

    #[test]
    fn missing_role_defaults_to_assistant_for_text_parts() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let session_path = write_session_file(
            root,
            "project-a",
            "session-008.json",
            json!({
                "id": "session-008",
                "directory": "/projects/alpha",
                "time": { "created": 1_704_067_200_000i64, "updated": 1_704_067_260_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-008",
            "msg-user.json",
            json!({
                "id": "msg-user",
                "sessionID": "session-008",
                "role": "user",
                "time": { "created": 1_704_067_200_000i64 }
            }),
        );

        write_message_file(
            root,
            "session-008",
            "msg-missing-role.json",
            json!({
                "id": "msg-missing-role",
                "sessionID": "session-008",
                "time": { "created": 1_704_067_260_000i64 }
            }),
        );

        write_part_file(
            root,
            "msg-user",
            "part-user.json",
            json!({
                "id": "part-user",
                "order": 1,
                "type": "text",
                "text": "Hello"
            }),
        );

        write_part_file(
            root,
            "msg-missing-role",
            "part-assistant.json",
            json!({
                "id": "part-assistant",
                "order": 1,
                "type": "text",
                "text": "I can help"
            }),
        );

        let parser = OpenCodeParser::new(root);
        let parsed = parser.parse(&session_path).unwrap();

        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.messages[1].role, Role::Assistant);
        assert_eq!(parsed.messages[1].content, "I can help");
    }
}
