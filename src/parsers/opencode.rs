use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{Message, Role, Session, Tool};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Skipping subagent session")]
    SubagentSession,
    #[error("Session contains no user messages")]
    NoUserMessages,
}

pub struct OpenCodeParser {
    storage_root: PathBuf,
}

struct SessionMetadata {
    id: String,
    directory: Option<String>,
    time_created: DateTime<Utc>,
    time_updated: DateTime<Utc>,
    parent_id: Option<String>,
}

struct MessageMetadata {
    id: String,
    role: Option<Role>,
    time_created: DateTime<Utc>,
}

struct PartData {
    id: String,
    kind: String,
    order: Option<i64>,
    /// Raw part JSON for extracting type-specific fields
    raw: Value,
}

impl OpenCodeParser {
    pub fn new(storage_root: &Path) -> Self {
        Self {
            storage_root: storage_root.to_path_buf(),
        }
    }

    pub fn parse(&self, session_path: &Path) -> Result<(Session, Vec<Message>)> {
        let metadata = self.parse_session_metadata(session_path)?;
        if metadata.parent_id.is_some() {
            return Err(ParseError::SubagentSession.into());
        }

        let mut messages = self.load_messages(&metadata.id)?;
        messages.sort_by(|a, b| {
            a.time_created
                .cmp(&b.time_created)
                .then_with(|| a.id.cmp(&b.id))
        });

        let mut flattened = Vec::new();
        let mut has_user_message = false;

        for message in messages {
            let mut parts = self.load_parts(&message.id)?;
            parts.sort_by(|a, b| match (a.order, b.order) {
                (Some(left), Some(right)) => left.cmp(&right).then_with(|| a.id.cmp(&b.id)),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => a.id.cmp(&b.id),
            });

            for part in parts {
                let messages_from_part = self.part_to_message(
                    &metadata.id,
                    message.role,
                    message.time_created,
                    &part,
                    &mut has_user_message,
                );
                flattened.extend(messages_from_part);
            }
        }

        if !has_user_message {
            return Err(ParseError::NoUserMessages.into());
        }

        for (index, message) in flattened.iter_mut().enumerate() {
            message.index = index;
        }

        let first_prompt = crate::parsers::extract_first_prompt(&flattened);

        let session = Session {
            id: metadata.id.clone(),
            tool: Tool::OpenCode,
            project_path: metadata.directory.clone(),
            start_time: metadata.time_created,
            message_count: flattened.len(),
            file_path: session_path.to_str().unwrap_or_default().to_string(),
            last_updated: metadata.time_updated,
            first_prompt,
        };

        Ok((session, flattened))
    }

    fn parse_session_metadata(&self, session_path: &Path) -> Result<SessionMetadata> {
        let value = Self::read_json(session_path).context("Failed to read session metadata")?;
        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| {
                session_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            })
            .context("Session id missing")?;

        let directory = value
            .get("directory")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let parent_id = value
            .get("parentID")
            .or_else(|| value.get("parentId"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let created_ms = value
            .get("time")
            .and_then(|v| v.get("created"))
            .and_then(|v| v.as_i64())
            .context("Session created time missing")?;

        let updated_ms = value
            .get("time")
            .and_then(|v| v.get("updated"))
            .and_then(|v| v.as_i64())
            .unwrap_or(created_ms);

        Ok(SessionMetadata {
            id,
            directory,
            time_created: Self::timestamp_from_millis(created_ms)?,
            time_updated: Self::timestamp_from_millis(updated_ms)?,
            parent_id,
        })
    }

    fn load_messages(&self, session_id: &str) -> Result<Vec<MessageMetadata>> {
        let messages_dir = self.storage_root.join("message").join(session_id);
        let entries = match fs::read_dir(&messages_dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err).context("Failed to read messages directory"),
        };

        let mut messages = Vec::new();
        for entry in entries {
            let entry = entry.context("Failed to read message entry")?;
            if !entry
                .file_type()
                .context("Failed to read message type")?
                .is_file()
            {
                continue;
            }

            let value = match Self::read_json(&entry.path()) {
                Ok(value) => value,
                Err(err) => {
                    tracing::warn!(
                        "Failed to parse message {}: {}",
                        entry.path().display(),
                        err
                    );
                    continue;
                }
            };

            let id = value
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .or_else(|| {
                    tracing::warn!("Message id missing in {}", entry.path().display());
                    None
                });

            let id = match id {
                Some(id) => id,
                None => continue,
            };

            let role = value.get("role").and_then(|v| v.as_str()).and_then(|role| {
                match role.to_lowercase().as_str() {
                    "user" => Some(Role::User),
                    "assistant" => Some(Role::Assistant),
                    _ => None,
                }
            });

            let created_ms = value
                .get("time")
                .and_then(|v| v.get("created"))
                .and_then(|v| v.as_i64());

            let created_ms = match created_ms {
                Some(created_ms) => created_ms,
                None => {
                    tracing::warn!("Message created time missing in {}", entry.path().display());
                    continue;
                }
            };

            let time_created = match Self::timestamp_from_millis(created_ms) {
                Ok(timestamp) => timestamp,
                Err(err) => {
                    tracing::warn!(
                        "Invalid message timestamp in {}: {}",
                        entry.path().display(),
                        err
                    );
                    continue;
                }
            };

            messages.push(MessageMetadata {
                id,
                role,
                time_created,
            });
        }

        Ok(messages)
    }

    fn load_parts(&self, message_id: &str) -> Result<Vec<PartData>> {
        let parts_dir = self.storage_root.join("part").join(message_id);
        let entries = match fs::read_dir(&parts_dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!("Missing parts for message {}", message_id);
                return Ok(Vec::new());
            }
            Err(err) => return Err(err).context("Failed to read parts directory"),
        };

        let mut parts = Vec::new();
        for entry in entries {
            let entry = entry.context("Failed to read part entry")?;
            if !entry
                .file_type()
                .context("Failed to read part type")?
                .is_file()
            {
                continue;
            }

            let value = match Self::read_json(&entry.path()) {
                Ok(value) => value,
                Err(err) => {
                    tracing::warn!("Failed to parse part {}: {}", entry.path().display(), err);
                    continue;
                }
            };

            let id = value.get("id").and_then(|v| v.as_str()).map(str::to_string);

            let id = match id {
                Some(id) => id,
                None => {
                    tracing::warn!("Part id missing in {}", entry.path().display());
                    continue;
                }
            };
            let kind = value
                .get("type")
                .and_then(|v| v.as_str())
                .map(str::to_string);

            let kind = match kind {
                Some(kind) => kind,
                None => {
                    tracing::warn!("Part type missing in {}", entry.path().display());
                    continue;
                }
            };
            let order = value.get("order").and_then(|v| v.as_i64());

            parts.push(PartData {
                id,
                kind,
                order,
                raw: value,
            });
        }

        Ok(parts)
    }

    fn part_to_message(
        &self,
        session_id: &str,
        message_role: Option<Role>,
        timestamp: DateTime<Utc>,
        part: &PartData,
        has_user_message: &mut bool,
    ) -> Vec<Message> {
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
                // OpenCode stores text directly on the part, not under content
                let text = part
                    .raw
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .filter(|value| !value.trim().is_empty());

                let text = match text {
                    Some(t) => t,
                    None => return Vec::new(),
                };

                if role == Role::User {
                    *has_user_message = true;
                }

                vec![Message {
                    session_id: session_id.to_string(),
                    index: 0,
                    role,
                    content: text,
                    timestamp,
                }]
            }
            // Skip metadata/control parts
            "tool" | "reasoning" | "step-start" | "step-finish" | "snapshot" | "compaction"
            | "subtask" => Vec::new(),
            other => {
                tracing::debug!("Unhandled part type: {}", other);
                Vec::new()
            }
        }
    }

    fn read_json(path: &Path) -> Result<Value> {
        let bytes = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_slice(&bytes).context("Failed to parse JSON")
    }

    fn timestamp_from_millis(value: i64) -> Result<DateTime<Utc>> {
        DateTime::<Utc>::from_timestamp_millis(value).context("Invalid timestamp")
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

        let parser = OpenCodeParser::new(root);
        let metadata = parser.parse_session_metadata(&session_path).unwrap();

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
    fn parse_skips_subagent_sessions() {
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
        let parser = OpenCodeParser::new(root);

        let parts = parser.load_parts("missing-msg").unwrap();
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
        let (session, messages) = parser.parse(&session_path).unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].index, 0);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "First");
        assert_eq!(messages[1].index, 1);
        assert_eq!(messages[1].role, Role::User);
        assert_eq!(messages[1].content, "Second");
        assert_eq!(session.first_prompt.as_deref(), Some("First"));
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
        let (_session, messages) = parser.parse(&session_path).unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "First message");
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].content, "Second message");
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
        let (_session, messages) = parser.parse(&session_path).unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "Hello");
    }

    #[test]
    fn tool_part_is_skipped() {
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
        let (_session, messages) = parser.parse(&session_path).unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "Run tool");
    }

    #[test]
    fn tool_part_with_output_is_skipped() {
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
        let (_session, messages) = parser.parse(&session_path).unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "Read file");
    }

    #[test]
    fn tool_part_with_error_is_skipped() {
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
        let (_session, messages) = parser.parse(&session_path).unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "Read file");
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
        let (_session, messages) = parser.parse(&session_path).unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].content, "I can help");
    }
}
