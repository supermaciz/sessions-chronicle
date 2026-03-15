use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::models::{AiAssistant, Message, Role, Session, TokenUsage, ToolCall, ToolCallStatus};
use crate::models::{TranscriptItem, TranscriptItemKind};
use crate::parsers::ParsedSession;
use crate::parsers::model::normalize_model;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("First line must be session_meta")]
    MissingSessionMeta,
    #[error("Session contains no user messages")]
    NoUserMessages,
    #[error("Invalid session_meta JSON: {0}")]
    InvalidSessionMetaJson(String),
}

/// Intermediate state while correlating begin/end pairs.
struct PendingCall {
    tool_name: String,
    input_json: Option<String>,
    started_at: Option<i64>,
}

pub struct CodexParser;

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

        let mut last_updated = start_time;
        let mut messages: Vec<Message> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut transcript_items: Vec<TranscriptItem> = Vec::new();
        let mut has_user_message = false;

        // Maps call_id → PendingCall (for begin/end correlation)
        let mut pending_calls: HashMap<String, PendingCall> = HashMap::new();
        // Maps call_id → index in tool_calls (for end event to update)
        let mut call_id_to_tc_idx: HashMap<String, usize> = HashMap::new();

        let mut msg_counter: i64 = 0;
        let mut item_counter: i64 = 0;
        let mut current_turn_model: Option<String> = None;
        let mut best_snapshot: Option<(i64, TokenUsage)> = None;

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
                .and_then(|s| match Self::parse_timestamp(s) {
                    Ok(t) => Some(t),
                    Err(err) => {
                        tracing::warn!("Failed to parse event timestamp {}: {}", s, err);
                        None
                    }
                });

            if let Some(ts) = event_ts
                && ts > last_updated
            {
                last_updated = ts;
            }

            if event_type == Some("response_item") {
                let payload = match event.get("payload") {
                    Some(p) => p,
                    None => continue,
                };

                match payload.get("type").and_then(|v| v.as_str()) {
                    Some("function_call") | Some("custom_tool_call") => {
                        let call_id = match payload.get("call_id").and_then(|v| v.as_str()) {
                            Some(id) if !id.is_empty() => id.to_string(),
                            _ => {
                                tracing::warn!(
                                    "response_item call begin missing call_id, skipping"
                                );
                                continue;
                            }
                        };

                        if call_id_to_tc_idx.contains_key(&call_id) {
                            continue;
                        }

                        let tool_name = payload
                            .get("name")
                            .or_else(|| payload.get("tool_name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();

                        let input_json = payload
                            .get("arguments")
                            .or_else(|| payload.get("input"))
                            .map(|v| {
                                v.as_str()
                                    .map(str::to_string)
                                    .unwrap_or_else(|| v.to_string())
                            });

                        let tc_idx = tool_calls.len();
                        tool_calls.push(ToolCall {
                            id: call_id.clone(),
                            session_id: session_id.clone(),
                            subagent_id: None,
                            tool_name: tool_name.clone(),
                            status: ToolCallStatus::Running,
                            title: Some(tool_name),
                            summary: None,
                            input_json,
                            output_text: None,
                            error_text: None,
                            started_at: event_ts.map(|t| t.timestamp()),
                            ended_at: None,
                            duration_ms: None,
                            parser_call_id: Some(call_id.clone()),
                        });
                        call_id_to_tc_idx.insert(call_id.clone(), tc_idx);
                        transcript_items.push(TranscriptItem {
                            session_id: session_id.clone(),
                            item_index: item_counter,
                            kind: TranscriptItemKind::ToolCall,
                            message_index: None,
                            tool_call_id: Some(call_id.clone()),
                            subagent_id: None,
                        });
                        item_counter += 1;

                        pending_calls.insert(
                            call_id,
                            PendingCall {
                                tool_name: tool_calls[tc_idx].tool_name.clone(),
                                input_json: tool_calls[tc_idx].input_json.clone(),
                                started_at: tool_calls[tc_idx].started_at,
                            },
                        );
                    }
                    Some("function_call_output") | Some("custom_tool_call_output") => {
                        let call_id = match payload.get("call_id").and_then(|v| v.as_str()) {
                            Some(id) if !id.is_empty() => id,
                            _ => {
                                tracing::warn!(
                                    "response_item call output missing call_id, skipping"
                                );
                                continue;
                            }
                        };

                        if let Some(&tc_idx) = call_id_to_tc_idx.get(call_id)
                            && let Some(tc) = tool_calls.get_mut(tc_idx)
                        {
                            let output_text = payload.get("output").and_then(|v| {
                                if let Some(s) = v.as_str() {
                                    Some(s.to_string())
                                } else if v.is_null() {
                                    None
                                } else {
                                    Some(v.to_string())
                                }
                            });
                            let error_text = payload.get("error").and_then(|v| {
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

                            tc.output_text = output_text;
                            tc.error_text = error_text;
                            tc.ended_at = event_ts.map(|t| t.timestamp());
                            tc.duration_ms = payload.get("duration_ms").and_then(|v| v.as_i64());
                            let status = payload
                                .get("status")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_ascii_lowercase());
                            tc.status =
                                if matches!(status.as_deref(), Some("error") | Some("failed")) {
                                    ToolCallStatus::Error
                                } else if matches!(status.as_deref(), Some("completed")) {
                                    ToolCallStatus::Completed
                                } else if tc.error_text.is_some() {
                                    ToolCallStatus::Error
                                } else {
                                    ToolCallStatus::Completed
                                };
                        }
                        pending_calls.remove(call_id);
                    }
                    _ => {}
                }

                continue;
            }

            // Handle top-level turn_context (non-wrapped form)
            if event_type == Some("turn_context") {
                current_turn_model =
                    normalize_model(event.get("payload").and_then(|p| p.get("model")));
                continue;
            }

            if event_type != Some("event_msg") {
                continue;
            }

            let payload = match event.get("payload") {
                Some(p) => p,
                None => continue,
            };

            let message_type = payload.get("type").and_then(|v| v.as_str());

            match message_type {
                Some("turn_context") => {
                    current_turn_model = normalize_model(payload.get("model"));
                }

                Some("user_message") => {
                    let content = match payload.get("message").and_then(|v| v.as_str()) {
                        Some(c) => c.to_string(),
                        None => continue,
                    };
                    has_user_message = true;
                    messages.push(Message {
                        session_id: session_id.clone(),
                        index: msg_counter as usize,
                        role: Role::User,
                        content,
                        timestamp: event_ts.unwrap_or_else(Utc::now),
                        model: None,
                    });
                    transcript_items.push(TranscriptItem {
                        session_id: session_id.clone(),
                        item_index: item_counter,
                        kind: TranscriptItemKind::Message,
                        message_index: Some(msg_counter),
                        tool_call_id: None,
                        subagent_id: None,
                    });
                    msg_counter += 1;
                    item_counter += 1;
                }

                Some("agent_message") => {
                    let content = match payload.get("message").and_then(|v| v.as_str()) {
                        Some(c) => c.to_string(),
                        None => continue,
                    };
                    messages.push(Message {
                        session_id: session_id.clone(),
                        index: msg_counter as usize,
                        role: Role::Assistant,
                        content,
                        timestamp: event_ts.unwrap_or_else(Utc::now),
                        model: current_turn_model.clone(),
                    });
                    transcript_items.push(TranscriptItem {
                        session_id: session_id.clone(),
                        item_index: item_counter,
                        kind: TranscriptItemKind::Message,
                        message_index: Some(msg_counter),
                        tool_call_id: None,
                        subagent_id: None,
                    });
                    msg_counter += 1;
                    item_counter += 1;
                }

                Some(begin_type)
                    if matches!(begin_type, "mcp_tool_call_begin" | "exec_command_begin") =>
                {
                    let call_id = match payload.get("call_id").and_then(|v| v.as_str()) {
                        Some(id) => id.to_string(),
                        None => {
                            tracing::warn!("Tool call begin event missing call_id, skipping");
                            continue;
                        }
                    };

                    if call_id_to_tc_idx.contains_key(&call_id) {
                        continue;
                    }
                    let tool_name = payload
                        .get("tool_name")
                        .or_else(|| payload.get("command"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(begin_type)
                        .to_string();
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

                    pending_calls.insert(
                        call_id.clone(),
                        PendingCall {
                            tool_name,
                            input_json,
                            started_at: event_ts.map(|t| t.timestamp()),
                        },
                    );

                    // Allocate the ToolCall entry now with Pending status;
                    // the end event will fill in output/duration.
                    let tc_idx = tool_calls.len();
                    tool_calls.push(ToolCall {
                        id: call_id.clone(),
                        session_id: session_id.clone(),
                        subagent_id: None,
                        tool_name: pending_calls[&call_id].tool_name.clone(),
                        status: ToolCallStatus::Running,
                        title: Some(pending_calls[&call_id].tool_name.clone()),
                        summary: None,
                        input_json: pending_calls[&call_id].input_json.clone(),
                        output_text: None,
                        error_text: None,
                        started_at: pending_calls[&call_id].started_at,
                        ended_at: None,
                        duration_ms: None,
                        parser_call_id: Some(call_id.clone()),
                    });
                    call_id_to_tc_idx.insert(call_id.clone(), tc_idx);
                    transcript_items.push(TranscriptItem {
                        session_id: session_id.clone(),
                        item_index: item_counter,
                        kind: TranscriptItemKind::ToolCall,
                        message_index: None,
                        tool_call_id: Some(call_id),
                        subagent_id: None,
                    });
                    item_counter += 1;
                }

                Some("token_count") => {
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
                        let replace = match &best_snapshot {
                            Some((current_best, _)) => global_total > *current_best,
                            None => true,
                        };
                        if replace {
                            // Codex/OpenAI reports cached_input_tokens as the cached subset
                            // of input_tokens, not as an extra bucket to add on top.
                            best_snapshot = Some((
                                global_total,
                                TokenUsage {
                                    input_tokens: input,
                                    output_tokens: output,
                                    cache_read_tokens: cached,
                                    cache_write_tokens: None,
                                    reasoning_tokens: if reasoning > 0 {
                                        Some(reasoning)
                                    } else {
                                        None
                                    },
                                },
                            ));
                        }
                    }
                }

                Some("mcp_tool_call_end") | Some("exec_command_end") => {
                    let call_id = match payload.get("call_id").and_then(|v| v.as_str()) {
                        Some(id) => id,
                        None => continue,
                    };
                    if let Some(&tc_idx) = call_id_to_tc_idx.get(call_id)
                        && let Some(tc) = tool_calls.get_mut(tc_idx)
                    {
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
                        let duration_ms = payload.get("duration_ms").and_then(|v| v.as_i64());

                        tc.output_text = output;
                        tc.error_text = error;
                        tc.duration_ms = duration_ms;
                        tc.ended_at = event_ts.map(|t| t.timestamp());
                        tc.status = match exit_code {
                            Some(0) | None => ToolCallStatus::Completed,
                            Some(_) => ToolCallStatus::Error,
                        };
                    }
                    pending_calls.remove(call_id);
                }

                _ => {}
            }
        }

        if !has_user_message {
            return Err(ParseError::NoUserMessages.into());
        }

        let first_prompt = crate::parsers::extract_first_prompt(&messages);
        let token_usage = best_snapshot.map(|(_, usage)| usage);

        Ok(ParsedSession {
            session: Session {
                id: session_id,
                tool: AiAssistant::Codex,
                project_path,
                project_id: None,
                start_time,
                message_count: messages.len(),
                file_path: file_path.to_str().unwrap_or_default().to_string(),
                last_updated,
                first_prompt,
                parent_session_id: None,
                is_subagent: false,
                token_usage: None,
            },
            messages,
            tool_calls,
            subagents: Vec::new(),
            transcript_items,
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
