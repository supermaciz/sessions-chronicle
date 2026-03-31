use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDateTime, TimeZone, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::models::{
    AiAssistant, Message, Role, Session, TokenUsage, ToolCall, ToolCallStatus, TranscriptItem,
    TranscriptItemKind,
};
use crate::parsers::ParsedSession;
use crate::parsers::model::normalize_model;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Session contains no user messages")]
    NoUserMessages,
}

pub struct MistralVibeParser;

impl MistralVibeParser {
    pub fn parse(&self, session_dir: &Path) -> Result<ParsedSession> {
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

        let session_model =
            normalize_model(metadata.get("config").and_then(|c| c.get("active_model")));

        let token_usage = metadata.get("stats").and_then(|stats| {
            let prompt = stats.get("session_prompt_tokens")?.as_i64()?;
            let completion = stats.get("session_completion_tokens")?.as_i64()?;
            Some(TokenUsage {
                input_tokens: prompt,
                output_tokens: completion,
                cache_read_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
            })
        });

        let messages_path = session_dir.join("messages.jsonl");
        let file = File::open(&messages_path).context("Failed to open messages.jsonl")?;
        let reader = BufReader::new(file);

        let mut messages: Vec<Message> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut transcript_items: Vec<TranscriptItem> = Vec::new();
        let mut has_user_message = false;
        // Maps the raw tool_call id from the JSON → index in tool_calls vec for result correlation
        let mut pending_calls: HashMap<String, usize> = HashMap::new();

        for line in reader.lines() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }

            let event: Value = serde_json::from_str(&line).context("Failed to parse JSON")?;
            let role = event.get("role").and_then(|v| v.as_str());

            match role {
                Some("system") => continue,
                Some("tool") => {
                    // Correlate tool result with the pending ToolCall by tool_call_id
                    if let Some(raw_id) = event.get("tool_call_id").and_then(|v| v.as_str())
                        && let Some(&tc_idx) = pending_calls.get(raw_id)
                    {
                        let output = event
                            .get("content")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        tool_calls[tc_idx].output_text = output;
                        tool_calls[tc_idx].status = ToolCallStatus::Completed;
                    }
                }
                Some("user") => {
                    if let Some(content) = Self::extract_content(&event) {
                        has_user_message = true;
                        let msg_idx = messages.len() as i64;
                        let item_idx = transcript_items.len() as i64;
                        Self::push_message(
                            &mut messages,
                            &session_id,
                            Role::User,
                            content,
                            start_time,
                            None,
                        );
                        transcript_items.push(TranscriptItem {
                            session_id: session_id.clone(),
                            item_index: item_idx,
                            kind: TranscriptItemKind::Message,
                            message_index: Some(msg_idx),
                            tool_call_id: None,
                            subagent_id: None,
                        });
                    }
                }
                Some("assistant") => {
                    // Text content produces a Message transcript item
                    if let Some(content) = Self::extract_content(&event) {
                        let msg_idx = messages.len() as i64;
                        let item_idx = transcript_items.len() as i64;
                        Self::push_message(
                            &mut messages,
                            &session_id,
                            Role::Assistant,
                            content,
                            start_time,
                            session_model.clone(),
                        );
                        transcript_items.push(TranscriptItem {
                            session_id: session_id.clone(),
                            item_index: item_idx,
                            kind: TranscriptItemKind::Message,
                            message_index: Some(msg_idx),
                            tool_call_id: None,
                            subagent_id: None,
                        });
                    }
                    // tool_calls array produces ToolCall transcript items
                    if let Some(tc_arr) = event.get("tool_calls").and_then(|v| v.as_array()) {
                        for tc_val in tc_arr {
                            let raw_id = match tc_val.get("id").and_then(|v| v.as_str()) {
                                Some(id) => id.to_string(),
                                None => continue,
                            };
                            let tool_name = tc_val
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let input_json = tc_val
                                .get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|v| v.as_str())
                                .map(str::to_string);

                            let tc_idx = tool_calls.len();
                            let item_idx = transcript_items.len() as i64;
                            let tc_id = format!("{}-{}", session_id, raw_id);

                            pending_calls.insert(raw_id, tc_idx);
                            tool_calls.push(ToolCall {
                                id: tc_id.clone(),
                                session_id: session_id.clone(),
                                subagent_id: None,
                                tool_name,
                                status: ToolCallStatus::Pending,
                                title: None,
                                summary: None,
                                input_json,
                                output_text: None,
                                error_text: None,
                                started_at: None,
                                ended_at: None,
                                duration_ms: None,
                                parser_call_id: None,
                            });
                            transcript_items.push(TranscriptItem {
                                session_id: session_id.clone(),
                                item_index: item_idx,
                                kind: TranscriptItemKind::ToolCall,
                                message_index: None,
                                tool_call_id: Some(tc_id),
                                subagent_id: None,
                            });
                        }
                    }
                }
                _ => continue,
            }
        }

        if !has_user_message {
            return Err(ParseError::NoUserMessages.into());
        }

        let first_prompt = crate::parsers::extract_first_prompt(&messages);

        Ok(ParsedSession {
            session: Session {
                id: session_id.clone(),
                tool: AiAssistant::MistralVibe,
                project_path,
                project_id: None,
                start_time,
                message_count: messages.len(),
                file_path: session_dir.to_str().unwrap_or_default().to_string(),
                last_updated: end_time,
                first_prompt,
                parent_session_id: None,
                is_subagent: false,
                token_usage: None,
                edit_count: 0,
                read_count: 0,
                command_count: 0,
                ending_status: crate::models::SessionEndingStatus::Unknown,
            },
            messages,
            tool_calls,
            subagents: Vec::new(),
            transcript_items,
            token_usage,
        })
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
        model: Option<String>,
    ) {
        let index = messages.len();
        let timestamp = start_time + Duration::seconds(index as i64);
        messages.push(Message {
            session_id: session_id.to_string(),
            index,
            role,
            content,
            timestamp,
            model,
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
        let parsed = parser.parse(&path).unwrap();
        let session = parsed.session;
        let messages = parsed.messages;

        assert_eq!(session.id, "session_20260203_191451_b9383361");
        assert_eq!(
            session.project_path.as_deref(),
            Some("/home/anon/projects/sessions-chronicle")
        );
        assert_eq!(session.message_count, 2);
        assert_eq!(
            session.first_prompt.as_deref(),
            Some("List the files in the project root.")
        );
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
        let parsed = parser.parse(root).unwrap();
        let session = parsed.session;
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
            r#"{"role":"tool","tool_call_id":"x","content":"Tool output"}"#,
            r#"{"role":"user","content":"Hi"}"#,
            r#"{"role":"assistant","content":"Hello"}"#,
        ]);
        let parser = MistralVibeParser;
        let parsed = parser.parse(temp_dir.path()).unwrap();
        let messages = parsed.messages;

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "Hi");
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].content, "Hello");
    }

    #[test]
    fn parse_v27_session_does_not_depend_on_system_message_position() {
        let parser = MistralVibeParser;

        let with_system = TempDir::new().unwrap();
        let with_system_root = with_system.path();
        fs::write(
            with_system_root.join("meta.json"),
            serde_json::to_vec(&json!({
                "session_id": "v27-with-system",
                "start_time": "2026-02-03T19:14:51Z",
                "end_time": "2026-02-03T19:16:05Z",
                "system_prompt": "You are Mistral Vibe system prompt",
                "environment": { "working_directory": "/tmp/project" }
            }))
            .unwrap(),
        )
        .unwrap();
        write_messages(
            &with_system_root.join("messages.jsonl"),
            &[
                r#"{"role":"user","content":"Hi"}"#,
                r#"{"role":"system","content":"Injected system row"}"#,
                r#"{"role":"assistant","content":"Hello"}"#,
            ],
        );

        let without_system = TempDir::new().unwrap();
        let without_system_root = without_system.path();
        fs::write(
            without_system_root.join("meta.json"),
            serde_json::to_vec(&json!({
                "session_id": "v27-without-system",
                "start_time": "2026-02-03T19:14:51Z",
                "end_time": "2026-02-03T19:16:05Z",
                "system_prompt": "You are Mistral Vibe system prompt",
                "environment": { "working_directory": "/tmp/project" }
            }))
            .unwrap(),
        )
        .unwrap();
        write_messages(
            &without_system_root.join("messages.jsonl"),
            &[
                r#"{"role":"user","content":"Hi"}"#,
                r#"{"role":"assistant","content":"Hello"}"#,
            ],
        );

        let with_system_parsed = parser.parse(with_system_root).unwrap();
        let without_system_parsed = parser.parse(without_system_root).unwrap();

        assert_eq!(with_system_parsed.messages.len(), 2);
        assert_eq!(with_system_parsed.messages[0].role, Role::User);
        assert_eq!(with_system_parsed.messages[0].content, "Hi");
        assert_eq!(with_system_parsed.messages[1].role, Role::Assistant);
        assert_eq!(with_system_parsed.messages[1].content, "Hello");
        assert_eq!(
            with_system_parsed.session.first_prompt.as_deref(),
            Some("Hi")
        );

        assert_eq!(without_system_parsed.messages.len(), 2);
        assert_eq!(without_system_parsed.messages[0].content, "Hi");
        assert_eq!(without_system_parsed.messages[1].content, "Hello");

        let with_contents: Vec<String> = with_system_parsed
            .messages
            .iter()
            .map(|m| m.content.clone())
            .collect();
        let without_contents: Vec<String> = without_system_parsed
            .messages
            .iter()
            .map(|m| m.content.clone())
            .collect();
        assert_eq!(with_contents, without_contents);
    }

    #[test]
    fn parse_extracts_tool_calls_from_assistant_messages() {
        let temp_dir = create_temp_session_dir(&[
            r#"{"role":"user","content":"Run bash"}"#,
            r#"{"role":"assistant","content":null,"tool_calls":[{"id":"call-1","type":"function","function":{"name":"bash","arguments":"{\"command\":\"ls\"}"}}]}"#,
            r#"{"role":"tool","tool_call_id":"call-1","content":"file1.txt\nfile2.rs"}"#,
        ]);
        let parser = MistralVibeParser;
        let parsed = parser.parse(temp_dir.path()).unwrap();

        // Only the user message in messages list (assistant had no text content)
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].role, Role::User);

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].tool_name, "bash");
        assert_eq!(
            parsed.tool_calls[0].input_json.as_deref(),
            Some("{\"command\":\"ls\"}")
        );
        assert_eq!(
            parsed.tool_calls[0].output_text.as_deref(),
            Some("file1.txt\nfile2.rs")
        );
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Completed);

        // Transcript: user message + tool call
        assert_eq!(parsed.transcript_items.len(), 2);
        assert_eq!(parsed.transcript_items[0].kind, TranscriptItemKind::Message);
        assert_eq!(
            parsed.transcript_items[1].kind,
            TranscriptItemKind::ToolCall
        );
    }

    #[test]
    fn mistral_vibe_assistant_gets_session_model() {
        let parsed = MistralVibeParser
            .parse(Path::new(
                "tests/fixtures/vibe_sessions/session_20260203_191451_b9383361",
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
            Some("mistral-large-latest")
        );
    }

    #[test]
    fn mistral_vibe_user_has_no_model() {
        let parsed = MistralVibeParser
            .parse(Path::new(
                "tests/fixtures/vibe_sessions/session_20260203_191451_b9383361",
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
    fn parse_extracts_token_usage_from_stats() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let meta = json!({
            "session_id": "tok-session",
            "start_time": "2026-02-03T19:14:51Z",
            "end_time": "2026-02-03T19:16:05Z",
            "environment": { "working_directory": "/tmp/project" },
            "stats": { "session_prompt_tokens": 5000, "session_completion_tokens": 2000 }
        });
        fs::write(root.join("meta.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
        write_messages(
            &root.join("messages.jsonl"),
            &[
                r#"{"role":"user","content":"Hi"}"#,
                r#"{"role":"assistant","content":"Hello"}"#,
            ],
        );
        let parsed = MistralVibeParser.parse(root).unwrap();
        let usage = parsed.token_usage.expect("should have token_usage");
        assert_eq!(usage.input_tokens, 5000);
        assert_eq!(usage.output_tokens, 2000);
        assert_eq!(usage.cache_read_tokens, None);
        assert_eq!(usage.reasoning_tokens, None);
    }

    #[test]
    fn parse_no_stats_yields_none_token_usage() {
        let temp_dir = create_temp_session_dir(&[
            r#"{"role":"user","content":"Hi"}"#,
            r#"{"role":"assistant","content":"Hello"}"#,
        ]);
        let parsed = MistralVibeParser.parse(temp_dir.path()).unwrap();
        assert!(parsed.token_usage.is_none());
    }

    #[test]
    fn parse_tool_call_without_result_stays_pending() {
        let temp_dir = create_temp_session_dir(&[
            r#"{"role":"user","content":"Run something"}"#,
            r#"{"role":"assistant","content":null,"tool_calls":[{"id":"call-x","type":"function","function":{"name":"search","arguments":"{}"}}]}"#,
        ]);
        let parser = MistralVibeParser;
        let parsed = parser.parse(temp_dir.path()).unwrap();

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Pending);
        assert!(parsed.tool_calls[0].output_text.is_none());
    }
}
