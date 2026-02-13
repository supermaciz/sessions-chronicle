use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::models::{Message, Role, Session, Tool};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("First line must be session_meta")]
    MissingSessionMeta,
    #[error("Session contains no user messages")]
    NoUserMessages,
    #[error("Invalid session_meta JSON: {0}")]
    InvalidSessionMetaJson(String),
}

pub struct CodexParser;

impl CodexParser {
    pub fn parse(&self, file_path: &Path) -> Result<(Session, Vec<Message>)> {
        let file = File::open(file_path).context("Failed to open session file")?;
        let reader = BufReader::new(file);

        let mut lines = reader.lines();
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

        let mut last_updated = start_time;
        let mut messages = Vec::new();
        let mut has_user_message = false;

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
            if event_type != Some("event_msg") {
                continue;
            }

            let payload = match event.get("payload") {
                Some(payload) => payload,
                None => continue,
            };

            let message_type = payload.get("type").and_then(|v| v.as_str());
            let (role, content) = match message_type {
                Some("user_message") => {
                    let content = match payload.get("message").and_then(|v| v.as_str()) {
                        Some(content) => content.to_string(),
                        None => continue,
                    };
                    (Role::User, content)
                }
                Some("agent_message") => {
                    let content = match payload.get("message").and_then(|v| v.as_str()) {
                        Some(content) => content.to_string(),
                        None => continue,
                    };
                    (Role::Assistant, content)
                }
                _ => continue,
            };

            if role == Role::User {
                has_user_message = true;
            }

            let parsed_timestamp = match event.get("timestamp").and_then(|v| v.as_str()) {
                Some(raw) => match Self::parse_timestamp(raw) {
                    Ok(parsed) => Some(parsed),
                    Err(err) => {
                        tracing::warn!("Failed to parse event timestamp {}: {}", raw, err);
                        None
                    }
                },
                None => None,
            };

            if let Some(parsed) = parsed_timestamp
                && parsed > last_updated
            {
                last_updated = parsed;
            }

            let timestamp = parsed_timestamp.unwrap_or_else(Utc::now);
            let index = messages.len();

            messages.push(Message {
                session_id: session_id.clone(),
                index,
                role,
                content,
                timestamp,
            });
        }

        if !has_user_message {
            return Err(ParseError::NoUserMessages.into());
        }

        // first_prompt is populated via extract_first_prompt() in parsers/mod.rs.
        let first_prompt = crate::parsers::extract_first_prompt(&messages);

        Ok((
            Session {
                id: session_id,
                tool: Tool::Codex,
                project_path,
                start_time,
                message_count: messages.len(),
                file_path: file_path.to_str().unwrap_or_default().to_string(),
                last_updated,
                first_prompt,
            },
            messages,
        ))
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
        let (session, messages) = parser.parse(&path).unwrap();
        assert_eq!(session.id, "019bce9f-0a40-79e2-8351-8818e8487fb6");
        assert_eq!(session.project_path.as_deref(), Some("/home/user/project"));
        assert_eq!(session.message_count, 2);
        assert_eq!(session.first_prompt.as_deref(), Some("Summarize the repo"));
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "Summarize the repo");
        assert_eq!(messages[1].role, Role::Assistant);
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
