use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDateTime, TimeZone, Utc};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::models::{Message, Role, Session, Tool};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Session contains no user messages")]
    NoUserMessages,
}

pub struct MistralVibeParser;

impl MistralVibeParser {
    pub fn parse(&self, session_dir: &Path) -> Result<(Session, Vec<Message>)> {
        let meta_path = session_dir.join("meta.json");
        let metadata = Self::read_json(&meta_path).context("Failed to read meta.json")?;

        let session_id = metadata
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .context("Session id missing")?;

        let start_time = metadata
            .get("start_time")
            .and_then(|v| v.as_str())
            .context("Session start time missing")
            .and_then(Self::parse_timestamp)?;

        let end_time = metadata
            .get("end_time")
            .and_then(|v| v.as_str())
            .and_then(|value| Self::parse_timestamp(value).ok())
            .unwrap_or(start_time);

        let project_path = metadata
            .get("environment")
            .and_then(|v| v.get("working_directory"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let messages_path = session_dir.join("messages.jsonl");
        let file = File::open(&messages_path).context("Failed to open messages.jsonl")?;
        let reader = BufReader::new(file);

        let mut messages = Vec::new();
        let mut has_user_message = false;

        for line in reader.lines() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }

            let event: Value = serde_json::from_str(&line).context("Failed to parse JSON")?;
            let role = event.get("role").and_then(|v| v.as_str());

            match role {
                Some("system") | Some("tool") => continue,
                Some("user") => {
                    if let Some(content) = Self::extract_content(&event) {
                        has_user_message = true;
                        Self::push_message(
                            &mut messages,
                            &session_id,
                            Role::User,
                            content,
                            start_time,
                        );
                    }
                }
                Some("assistant") => {
                    if let Some(content) = Self::extract_content(&event) {
                        Self::push_message(
                            &mut messages,
                            &session_id,
                            Role::Assistant,
                            content,
                            start_time,
                        );
                    }
                }
                _ => continue,
            }
        }

        if !has_user_message {
            return Err(ParseError::NoUserMessages.into());
        }

        Ok((
            Session {
                id: session_id.clone(),
                tool: Tool::MistralVibe,
                project_path,
                start_time,
                message_count: messages.len(),
                file_path: session_dir.to_str().unwrap_or_default().to_string(),
                last_updated: end_time,
            },
            messages,
        ))
    }

    fn extract_content(event: &Value) -> Option<String> {
        event
            .get("content")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|value| !value.trim().is_empty())
    }

    fn push_message(
        messages: &mut Vec<Message>,
        session_id: &str,
        role: Role,
        content: String,
        start_time: DateTime<Utc>,
    ) {
        let index = messages.len();
        let timestamp = start_time + Duration::seconds(index as i64);
        messages.push(Message {
            session_id: session_id.to_string(),
            index,
            role,
            content,
            timestamp,
        });
    }

    fn read_json(path: &Path) -> Result<Value> {
        let bytes =
            std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_slice(&bytes).context("Failed to parse JSON")
    }

    fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
        // 1) RFC3339 with timezone/offset
        if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
            return Ok(dt.with_timezone(&Utc));
        }

        // 2) Naive timestamps treated as UTC
        for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S"] {
            if let Ok(naive) = NaiveDateTime::parse_from_str(value, fmt) {
                return Ok(Utc.from_utc_datetime(&naive));
            }
        }

        anyhow::bail!("Failed to parse timestamp: {value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Role;
    use chrono::{NaiveDateTime, TimeZone};
    use serde_json::json;
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn write_meta(path: &Path) {
        let value = json!({
            "session_id": "temp-session",
            "start_time": "2026-02-03T19:14:51Z",
            "end_time": "2026-02-03T19:16:05Z",
            "environment": { "working_directory": "/tmp/project" }
        });
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }

    fn write_messages(path: &Path, lines: &[&str]) {
        let mut file = File::create(path).unwrap();
        for line in lines {
            writeln!(file, "{}", line).unwrap();
        }
    }

    fn create_temp_session_dir(lines: &[&str]) -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        write_meta(&root.join("meta.json"));
        write_messages(&root.join("messages.jsonl"), lines);
        temp_dir
    }

    #[test]
    fn parse_valid_session_extracts_messages_and_tool_calls() {
        let parser = MistralVibeParser;
        let path = PathBuf::from("tests/fixtures/vibe_sessions/session_20260203_191451_b9383361");
        let (session, messages) = parser.parse(&path).unwrap();

        assert_eq!(session.id, "session_20260203_191451_b9383361");
        assert_eq!(
            session.project_path.as_deref(),
            Some("/home/anon/projects/sessions-chronicle")
        );
        assert_eq!(session.message_count, 2);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[1].role, Role::Assistant);
    }

    #[test]
    fn parse_rejects_session_without_user_messages() {
        let temp_dir = create_temp_session_dir(&[
            r#"{"role":"system","content":"Boot"}"#,
            r#"{"role":"assistant","content":"No user"}"#,
        ]);
        let parser = MistralVibeParser;
        let result = parser.parse(temp_dir.path());

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no user messages"));
    }

    #[test]
    fn parse_accepts_timestamps_without_timezone() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let value = json!({
            "session_id": "temp-session",
            "start_time": "2026-02-04T11:38:48.695030",
            "end_time": "2026-02-04T11:43:02.173084",
            "environment": { "working_directory": "/tmp/project" }
        });
        fs::write(root.join("meta.json"), serde_json::to_vec(&value).unwrap()).unwrap();
        write_messages(
            &root.join("messages.jsonl"),
            &[
                r#"{"role":"user","content":"Hi"}"#,
                r#"{"role":"assistant","content":"Hello"}"#,
            ],
        );

        let parser = MistralVibeParser;
        let (session, _messages) = parser.parse(root).unwrap();
        let expected = Utc.from_utc_datetime(
            &NaiveDateTime::parse_from_str("2026-02-04T11:38:48.695030", "%Y-%m-%dT%H:%M:%S%.f")
                .unwrap(),
        );

        assert_eq!(session.start_time, expected);
    }

    #[test]
    fn parse_ignores_system_and_tool_roles() {
        let temp_dir = create_temp_session_dir(&[
            r#"{"role":"system","content":"Boot"}"#,
            r#"{"role":"tool","content":"Tool output"}"#,
            r#"{"role":"user","content":"Hi"}"#,
            r#"{"role":"assistant","content":"Hello"}"#,
        ]);
        let parser = MistralVibeParser;
        let (_session, messages) = parser.parse(temp_dir.path()).unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "Hi");
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].content, "Hello");
    }
}
