use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::models::{Message, Role, Session, Subagent, Tool, ToolCall, ToolCallStatus};
use crate::models::{TranscriptItem, TranscriptItemKind};
use crate::parsers::ParsedSession;
use crate::parsers::model::normalize_model;

pub struct ClaudeCodeParser;

impl ClaudeCodeParser {
    pub fn parse(&self, file_path: &Path) -> Result<ParsedSession> {
        let file = File::open(file_path).context("Failed to open session file")?;
        let reader = BufReader::new(file);

        let mut earliest_timestamp: Option<DateTime<Utc>> = None;
        let mut latest_timestamp: Option<DateTime<Utc>> = None;
        let mut project_path = None;

        let file_stem_id = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        let mut session_id_from_event = None;
        let mut has_user_message = false;
        let mut messages: Vec<Message> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut subagents: Vec<Subagent> = Vec::new();
        let mut transcript_items: Vec<TranscriptItem> = Vec::new();

        // Maps tool_use_id → index in tool_calls (for non-Task tools)
        let mut pending_calls: HashMap<String, usize> = HashMap::new();
        // Maps tool_use_id → index in subagents (for Task tools)
        let mut pending_subagents: HashMap<String, usize> = HashMap::new();

        let mut msg_counter: i64 = 0;
        let mut item_counter: i64 = 0;

        for line in reader.lines() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }

            let event: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(err) => {
                    tracing::warn!("Failed to parse JSON line: {}", err);
                    continue;
                }
            };

            if session_id_from_event.is_none() {
                session_id_from_event = event
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
            let is_message_like = matches!(event_type, Some("user") | Some("assistant"));

            if is_message_like
                && let Some(ts) = event
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Self::parse_timestamp(s).ok())
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

            match event_type {
                Some("user") => {
                    let content_val = event.get("message").and_then(|m| m.get("content"));
                    let Some(content_val) = content_val else {
                        continue;
                    };

                    // Check for tool_result blocks
                    if let Some(arr) = content_val.as_array() {
                        let has_tool_result = arr
                            .iter()
                            .any(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_result"));

                        if has_tool_result {
                            // Populate output_text on matching ToolCall/Subagent
                            for block in arr {
                                if block.get("type").and_then(|v| v.as_str()) != Some("tool_result")
                                {
                                    continue;
                                }
                                let tool_use_id = block
                                    .get("tool_use_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let result_text = Self::extract_tool_result_content(block);

                                if let Some(&tc_idx) = pending_calls.get(tool_use_id) {
                                    if let Some(tc) = tool_calls.get_mut(tc_idx) {
                                        tc.output_text = Some(result_text);
                                        tc.status = ToolCallStatus::Completed;
                                        if let Some(ts) = event
                                            .get("timestamp")
                                            .and_then(|v| v.as_str())
                                            .and_then(|s| Self::parse_timestamp(s).ok())
                                        {
                                            tc.ended_at = Some(ts.timestamp());
                                            if let Some(started) = tc.started_at {
                                                tc.duration_ms =
                                                    Some((ts.timestamp() - started) * 1000);
                                            }
                                        }
                                    }
                                } else if let Some(&sa_idx) = pending_subagents.get(tool_use_id)
                                    && let Some(sa) = subagents.get_mut(sa_idx)
                                {
                                    sa.result_summary = Some(result_text);
                                }
                            }

                            // Check if there's also text content in the same user event
                            let text_content = Self::extract_text_from_array(arr);
                            if let Some(text) = text_content
                                && !text.trim().is_empty()
                            {
                                has_user_message = true;
                                let ts = event
                                    .get("timestamp")
                                    .and_then(|v| v.as_str())
                                    .and_then(|s| Self::parse_timestamp(s).ok())
                                    .unwrap_or_else(Utc::now);
                                let evt_sid = event
                                    .get("sessionId")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                messages.push(Message {
                                    session_id: evt_sid,
                                    index: msg_counter as usize,
                                    role: Role::User,
                                    content: text,
                                    timestamp: ts,
                                    model: None,
                                });
                                transcript_items.push(TranscriptItem {
                                    session_id: String::new(),
                                    item_index: item_counter,
                                    kind: TranscriptItemKind::Message,
                                    message_index: Some(msg_counter),
                                    tool_call_id: None,
                                    subagent_id: None,
                                });
                                msg_counter += 1;
                                item_counter += 1;
                            }
                            continue;
                        }
                    }

                    // Regular user message (string or text-block array)
                    let text = match Self::extract_content(content_val) {
                        Some(t) if !t.trim().is_empty() => t,
                        _ => continue,
                    };
                    has_user_message = true;
                    let ts = event
                        .get("timestamp")
                        .and_then(|v| v.as_str())
                        .and_then(|s| Self::parse_timestamp(s).ok())
                        .unwrap_or_else(Utc::now);
                    let evt_sid = event
                        .get("sessionId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    messages.push(Message {
                        session_id: evt_sid,
                        index: msg_counter as usize,
                        role: Role::User,
                        content: text,
                        timestamp: ts,
                        model: None,
                    });
                    transcript_items.push(TranscriptItem {
                        session_id: String::new(),
                        item_index: item_counter,
                        kind: TranscriptItemKind::Message,
                        message_index: Some(msg_counter),
                        tool_call_id: None,
                        subagent_id: None,
                    });
                    msg_counter += 1;
                    item_counter += 1;
                }

                Some("assistant") => {
                    let content_val = event.get("message").and_then(|m| m.get("content"));
                    let Some(content_val) = content_val else {
                        continue;
                    };

                    let ts = event
                        .get("timestamp")
                        .and_then(|v| v.as_str())
                        .and_then(|s| Self::parse_timestamp(s).ok())
                        .unwrap_or_else(Utc::now);
                    let evt_sid = event
                        .get("sessionId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    // Extract model from the message object
                    let model_raw = event.get("message").and_then(|m| m.get("model"));
                    let model = normalize_model(model_raw);

                    // Extract text portion
                    let text = Self::extract_content(content_val).filter(|t| !t.trim().is_empty());
                    if let Some(text) = text {
                        messages.push(Message {
                            session_id: evt_sid.clone(),
                            index: msg_counter as usize,
                            role: Role::Assistant,
                            content: text,
                            timestamp: ts,
                            model,
                        });
                        transcript_items.push(TranscriptItem {
                            session_id: String::new(),
                            item_index: item_counter,
                            kind: TranscriptItemKind::Message,
                            message_index: Some(msg_counter),
                            tool_call_id: None,
                            subagent_id: None,
                        });
                        msg_counter += 1;
                        item_counter += 1;
                    }

                    // Extract tool_use blocks
                    if let Some(arr) = content_val.as_array() {
                        for block in arr {
                            if block.get("type").and_then(|v| v.as_str()) != Some("tool_use") {
                                continue;
                            }
                            let tool_use_id = match block.get("id").and_then(|v| v.as_str()) {
                                Some(id) => id.to_string(),
                                None => {
                                    tracing::warn!("tool_use block missing id, skipping");
                                    continue;
                                }
                            };
                            let tool_name = block
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let input_json = block.get("input").map(|v| v.to_string());

                            if tool_name == "Task" {
                                // Treat as subagent
                                let description = block
                                    .get("input")
                                    .and_then(|v| v.get("description"))
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string);
                                let prompt = block
                                    .get("input")
                                    .and_then(|v| v.get("prompt"))
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string);
                                let title = description.clone().unwrap_or_else(|| {
                                    prompt
                                        .as_deref()
                                        .unwrap_or("Subagent task")
                                        .chars()
                                        .take(80)
                                        .collect::<String>()
                                });
                                let sa_idx = subagents.len();
                                subagents.push(Subagent {
                                    id: tool_use_id.clone(),
                                    session_id: String::new(),
                                    title,
                                    prompt,
                                    result_summary: None,
                                    child_session_id: None,
                                    parser_ref: Some(tool_use_id.clone()),
                                });
                                pending_subagents.insert(tool_use_id.clone(), sa_idx);
                                transcript_items.push(TranscriptItem {
                                    session_id: String::new(),
                                    item_index: item_counter,
                                    kind: TranscriptItemKind::Subagent,
                                    message_index: None,
                                    tool_call_id: None,
                                    subagent_id: Some(tool_use_id),
                                });
                            } else {
                                // Regular tool call
                                let tc_idx = tool_calls.len();
                                tool_calls.push(ToolCall {
                                    id: tool_use_id.clone(),
                                    session_id: String::new(),
                                    subagent_id: None,
                                    tool_name: tool_name.clone(),
                                    status: ToolCallStatus::Pending,
                                    title: Some(tool_name),
                                    summary: None,
                                    input_json,
                                    output_text: None,
                                    error_text: None,
                                    started_at: Some(ts.timestamp()),
                                    ended_at: None,
                                    duration_ms: None,
                                    parser_call_id: None,
                                });
                                pending_calls.insert(tool_use_id.clone(), tc_idx);
                                transcript_items.push(TranscriptItem {
                                    session_id: String::new(),
                                    item_index: item_counter,
                                    kind: TranscriptItemKind::ToolCall,
                                    message_index: None,
                                    tool_call_id: Some(tool_use_id),
                                    subagent_id: None,
                                });
                            }
                            item_counter += 1;
                        }
                    }
                }

                _ => {}
            }
        }

        let Some(start_time) = earliest_timestamp else {
            anyhow::bail!("Session contains no messages");
        };

        if !has_user_message {
            anyhow::bail!("Session contains no user messages");
        }

        let final_session_id = session_id_from_event
            .or(file_stem_id)
            .unwrap_or_else(|| "unknown".to_string());

        let last_updated = latest_timestamp.unwrap_or(start_time);
        let first_prompt = crate::parsers::extract_first_prompt(&messages);

        let session = Session {
            id: final_session_id,
            tool: Tool::ClaudeCode,
            project_path,
            start_time,
            message_count: messages.len(),
            file_path: file_path.to_str().unwrap().to_string(),
            last_updated,
            first_prompt,
            parent_session_id: None,
            is_subagent: false,
            token_usage: None,
        };

        Ok(ParsedSession {
            session,
            messages,
            tool_calls,
            subagents,
            transcript_items,
            token_usage: None,
        })
    }

    fn parse_timestamp(s: &str) -> Result<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&Utc))
            .context("Failed to parse timestamp")
    }

    fn extract_tool_result_content(block: &Value) -> String {
        let content = block.get("content");
        match content {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|b| {
                    if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                        b.get("text").and_then(|v| v.as_str()).map(str::to_string)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Some(other) => other.to_string(),
            None => String::new(),
        }
    }

    fn extract_text_from_array(arr: &[Value]) -> Option<String> {
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
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    fn extract_content(value: &Value) -> Option<String> {
        if let Some(s) = value.as_str() {
            return Some(s.to_string());
        }

        if let Some(arr) = value.as_array() {
            return Self::extract_text_from_array(arr);
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
        let parsed = parser.parse(file.path()).unwrap();

        let expected_start = ClaudeCodeParser::parse_timestamp("2024-01-01T00:00:00Z").unwrap();
        let expected_end = ClaudeCodeParser::parse_timestamp("2024-01-01T00:00:01Z").unwrap();

        assert_eq!(parsed.session.id, "session-123");
        assert_eq!(parsed.session.project_path.as_deref(), Some("/tmp"));
        assert_eq!(parsed.session.start_time, expected_start);
        assert_eq!(parsed.session.last_updated, expected_end);
        assert_eq!(parsed.session.message_count, 2);
        assert_eq!(parsed.session.first_prompt.as_deref(), Some("Hello"));

        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.messages[0].session_id, "session-123");
        assert_eq!(parsed.messages[0].index, 0);
        assert_eq!(parsed.messages[0].role, Role::User);
        assert_eq!(parsed.messages[0].content, "Hello");
        assert_eq!(parsed.messages[1].session_id, "session-123");
        assert_eq!(parsed.messages[1].index, 1);
        assert_eq!(parsed.messages[1].role, Role::Assistant);
        assert_eq!(parsed.messages[1].content, "Hi!");

        assert_eq!(parsed.transcript_items.len(), 2);
    }

    #[test]
    fn parse_prefers_event_session_id_and_propagates_to_messages() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"event-123","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"event-123","message":{"content":"Hi!"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.session.id, "event-123");
        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.messages[0].session_id, "event-123");
        assert_eq!(parsed.messages[1].session_id, "event-123");
    }

    #[test]
    fn parse_message_count_matches_parsed_messages() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"session-123","message":{"content":"Hello"}}"#,
            r#"{"type":"system","timestamp":"2024-01-01T00:00:00Z","subtype":"session_start"}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"session-123","message":{"content":"Hi!"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.session.message_count, 2);
    }

    #[test]
    fn parse_extracts_tool_calls_from_assistant_content() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"s1","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"s1","message":{"content":[{"type":"text","text":"Let me read"},{"type":"tool_use","id":"toolu_001","name":"Read","input":{"file_path":"/tmp/test.txt"}}]}}"#,
            r#"{"type":"user","timestamp":"2024-01-01T00:00:02Z","sessionId":"s1","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_001","content":"file contents"}]}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:03Z","sessionId":"s1","message":{"content":"Done!"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "toolu_001");
        assert_eq!(parsed.tool_calls[0].tool_name, "Read");
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Completed);
        assert_eq!(
            parsed.tool_calls[0].output_text.as_deref(),
            Some("file contents")
        );

        // Transcript: user msg, assistant msg, tool call item, assistant msg
        assert_eq!(parsed.transcript_items.len(), 4);
        assert_eq!(
            parsed.transcript_items[2].kind,
            TranscriptItemKind::ToolCall
        );
    }

    #[test]
    fn parse_extracts_task_tool_as_subagent() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"s2","message":{"content":"Analyze this"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"s2","message":{"content":[{"type":"text","text":"Running task"},{"type":"tool_use","id":"toolu_task_001","name":"Task","input":{"description":"Analyze project","prompt":"List all files"}}]}}"#,
            r#"{"type":"user","timestamp":"2024-01-01T00:00:10Z","sessionId":"s2","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_task_001","content":"Found 5 files"}]}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:11Z","sessionId":"s2","message":{"content":"Done"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(parsed.subagents[0].id, "toolu_task_001");
        assert_eq!(parsed.subagents[0].title, "Analyze project");
        assert_eq!(
            parsed.subagents[0].result_summary.as_deref(),
            Some("Found 5 files")
        );
        assert_eq!(parsed.tool_calls.len(), 0);

        // Transcript: user, assistant text, subagent item, assistant
        assert_eq!(parsed.transcript_items.len(), 4);
        assert_eq!(
            parsed.transcript_items[2].kind,
            TranscriptItemKind::Subagent
        );
    }

    #[test]
    fn assistant_message_has_model() {
        let parsed = ClaudeCodeParser
            .parse(std::path::Path::new(
                "tests/fixtures/claude_sessions/sample-session.jsonl",
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
            Some("claude-sonnet-4-5-20250514")
        );
    }

    #[test]
    fn user_message_has_no_model() {
        let parsed = ClaudeCodeParser
            .parse(std::path::Path::new(
                "tests/fixtures/claude_sessions/sample-session.jsonl",
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
    fn parse_tool_result_only_events_produce_no_transcript_items() {
        let file = create_temp_session(&[
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","sessionId":"s3","message":{"content":"Hello"}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:01Z","sessionId":"s3","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#,
            r#"{"type":"user","timestamp":"2024-01-01T00:00:02Z","sessionId":"s3","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"file.txt"}]}}"#,
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:03Z","sessionId":"s3","message":{"content":"Here are the files"}}"#,
        ]);

        let parser = ClaudeCodeParser;
        let parsed = parser.parse(file.path()).unwrap();

        assert_eq!(parsed.messages.len(), 2); // user "Hello" + assistant "Here are the files"
        assert_eq!(parsed.session.message_count, 2);
        // Transcript: user msg, tool call, assistant msg (tool_result user event → no item)
        assert_eq!(parsed.transcript_items.len(), 3);
    }
}
