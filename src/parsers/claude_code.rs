use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::models::{Message, Role, Session, Tool};

pub struct ClaudeCodeParser;

impl ClaudeCodeParser {
    pub fn parse(&self, file_path: &Path) -> Result<(Session, Vec<Message>)> {
        let file = File::open(file_path).context("Failed to open session file")?;
        let reader = BufReader::new(file);

        let mut earliest_timestamp: Option<DateTime<Utc>> = None;
        let mut latest_timestamp: Option<DateTime<Utc>> = None;
        let mut project_path = None;

        let file_stem_id = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        let mut session_id = None;
        let mut has_user_message = false;
        let mut messages = Vec::new();

        for (index, line) in reader.lines().enumerate() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }

            let event: Value = serde_json::from_str(&line).context("Failed to parse JSON")?;

            if session_id.is_none() {
                session_id = event
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }

            if project_path.is_none() {
                project_path = event
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }

            let event_type = event.get("type").and_then(|v| v.as_str());
            let is_message_like = match event_type {
                Some("user") | Some("assistant") => true,
                _ => false,
            };

            if event_type == Some("user") {
                has_user_message = true;
            }

            if is_message_like
                && let Some(ts) = event.get("timestamp").and_then(|v| v.as_str())
                && let Ok(ts) = Self::parse_timestamp(ts)
            {
                earliest_timestamp = Some(match earliest_timestamp {
                    Some(existing) => existing.min(ts),
                    None => ts,
                });

                latest_timestamp = Some(match latest_timestamp {
                    Some(existing) => existing.max(ts),
                    None => ts,
                });
            }

            if let Some(msg) = Self::parse_event(&event, index) {
                messages.push(msg);
            }
        }

        let Some(start_time) = earliest_timestamp else {
            anyhow::bail!("Session contains no messages");
        };

        if !has_user_message {
            anyhow::bail!("Session contains no user messages");
        }

        let final_session_id = session_id
            .or(file_stem_id)
            .unwrap_or_else(|| "unknown".to_string());
        for message in &mut messages {
            message.session_id = final_session_id.clone();
        }

        let last_updated = latest_timestamp.unwrap_or(start_time);

        Ok((
            Session {
                id: final_session_id,
                tool: Tool::ClaudeCode,
                project_path,
                start_time,
                message_count: messages.len(),
                file_path: file_path.to_str().unwrap().to_string(),
                last_updated,
            },
            messages,
        ))
    }

    fn parse_event(event: &Value, index: usize) -> Option<Message> {
        let event_type = event.get("type")?.as_str()?;

        let (role, content) = match event_type {
            "user" => {
                let content = Self::extract_content(event.get("message")?.get("content")?)?;
                (Role::User, content)
            }
            "assistant" => {
                let content = Self::extract_content(event.get("message")?.get("content")?)?;
                (Role::Assistant, content)
            }
            _ => return None,
        };

        let timestamp = event
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|s| Self::parse_timestamp(s).ok())
            .unwrap_or_else(Utc::now);

        let session_id = event
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        Some(Message {
            session_id,
            index,
            role,
            content,
            timestamp,
        })
    }

    fn parse_timestamp(s: &str) -> Result<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&Utc))
            .context("Failed to parse timestamp")
    }

    fn extract_content(value: &Value) -> Option<String> {
        // Handle string content directly
        if let Some(s) = value.as_str() {
            return Some(s.to_string());
        }

        // Handle array of content blocks
        if let Some(arr) = value.as_array() {
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
                return None;
            }
            return Some(parts.join("\n"));
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
        assert!(result.unwrap_err().to_string().contains("no user messages"));
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
        // User message without timestamp should still count as having user input
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
        assert!(result.unwrap_err().to_string().contains("no messages"));
    }

    #[test]
    fn parse_returns_session_and_messages() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"session-123","cwd":"/tmp","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"session-123","cwd":"/tmp","message":{"content":"Hi!"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let (session, messages) = parser.parse(file.path()).unwrap();

        let expected_start = ClaudeCodeParser::parse_timestamp("2024-01-01T00:00:00Z").unwrap();
        let expected_end = ClaudeCodeParser::parse_timestamp("2024-01-01T00:00:01Z").unwrap();

        assert_eq!(session.id, "session-123");
        assert_eq!(session.project_path.as_deref(), Some("/tmp"));
        assert_eq!(session.start_time, expected_start);
        assert_eq!(session.last_updated, expected_end);
        assert_eq!(session.message_count, 2);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].session_id, "session-123");
        assert_eq!(messages[0].index, 0);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[1].session_id, "session-123");
        assert_eq!(messages[1].index, 1);
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].content, "Hi!");
    }

    #[test]
    fn parse_prefers_event_session_id_and_propagates_to_messages() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"event-123","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"event-123","message":{"content":"Hi!"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let (session, messages) = parser.parse(file.path()).unwrap();

        assert_eq!(session.id, "event-123");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].session_id, "event-123");
        assert_eq!(messages[1].session_id, "event-123");
    }

    #[test]
    fn parse_message_count_matches_parsed_messages() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"session-123","message":{"content":"Hello"}}"#,
            r#"{"type":"system","timestamp":"2024-01-01T00:00:00Z","subtype":"session_start"}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"session-123","message":{"content":"Hi!"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let (session, messages) = parser.parse(file.path()).unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(session.message_count, 2);
    }

    #[test]
    fn parse_ignores_tool_events() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"session-123","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"session-123","message":{"content":[{"type":"tool_use","name":"bash","input":{"command":"ls"}}]}}"#,
            r#"{"type":"system","timestamp":"2024-01-01T00:00:02Z","subtype":"local_command","command":["ls","-la"],"stdout":"file1.txt\nfile2.txt"}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:03Z","sessionId":"session-123","message":{"content":"Here are the files"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let (session, messages) = parser.parse(file.path()).unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(session.message_count, 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].content, "Here are the files");
    }
}
