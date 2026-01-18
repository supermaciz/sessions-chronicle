use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::models::{Message, Role, Session, Tool};

pub struct ClaudeCodeParser;

impl ClaudeCodeParser {
    pub fn parse_metadata(&self, file_path: &Path) -> Result<Session> {
        let file = File::open(file_path).context("Failed to open session file")?;

        let reader = BufReader::new(file);
        let mut first_timestamp = None;
        let mut project_path = None;
        let file_stem_id = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        let mut session_id = file_stem_id;
        let mut message_count = 0;

        for line in reader.lines() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }

            let event: Value = serde_json::from_str(&line).context("Failed to parse JSON")?;

            // Extract session ID from first event
            if session_id.is_none() {
                session_id = event
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }

            // Extract project path from cwd
            if project_path.is_none() {
                project_path = event
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }

            // Extract first timestamp
            if first_timestamp.is_none()
                && let Some(ts) = event.get("timestamp").and_then(|v| v.as_str())
            {
                first_timestamp = Self::parse_timestamp(ts).ok();
            }

            message_count += 1;

            // Only process first few events for metadata
            if message_count >= 10 {
                break;
            }
        }

        // Count total messages by reading entire file
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let total_count = reader
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.trim().is_empty())
            .count();

        Ok(Session {
            id: session_id.unwrap_or_else(|| {
                file_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            }),
            tool: Tool::ClaudeCode,
            project_path,
            start_time: first_timestamp.unwrap_or_else(Utc::now),
            message_count: total_count,
            file_path: file_path.to_str().unwrap().to_string(),
            last_updated: Utc::now(),
        })
    }

    pub fn parse_messages(&self, file_path: &Path) -> Result<Vec<Message>> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();

        for (index, line) in reader.lines().enumerate() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }

            let event: Value = serde_json::from_str(&line).context("Failed to parse JSON")?;

            if let Some(msg) = Self::parse_event(&event, index) {
                messages.push(msg);
            }
        }

        Ok(messages)
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
            "system" => {
                let subtype = event.get("subtype")?.as_str()?;
                match subtype {
                    "local_command" => {
                        let stdout = event.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
                        let cmd = event
                            .get("command")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            })
                            .unwrap_or_else(|| "command".to_string());
                        (Role::ToolResult, format!("$ {}\n{}", cmd, stdout))
                    }
                    _ => return None,
                }
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
                        "tool_use" => {
                            let name = block.get("name")?.as_str()?;
                            Some(format!("[Tool: {}]", name))
                        }
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
