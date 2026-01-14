pub mod indexer;
pub mod schema;

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use rusqlite::{Connection, ToSql};
use std::path::Path;

use crate::models::{Session, Tool};

pub use indexer::SessionIndexer;

pub fn load_sessions(db_path: &Path, tools: &[Tool]) -> Result<Vec<Session>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    if tools.is_empty() {
        return Ok(Vec::new());
    }

    let db = Connection::open(db_path).context("Failed to open database")?;

    let (query, tool_strings): (String, Vec<String>) = if tools.len() == 3 {
        (
            "SELECT id, tool, project_path, start_time, message_count, file_path, last_updated
             FROM sessions
             ORDER BY last_updated DESC"
                .to_string(),
            vec![],
        )
    } else {
        let placeholders: Vec<String> = tools.iter().map(|_| "?".to_string()).collect();
        let tool_strings: Vec<String> = tools.iter().map(|t| t.to_storage()).collect::<Vec<_>>();
        (
            format!(
                "SELECT id, tool, project_path, start_time, message_count, file_path, last_updated
                 FROM sessions
                 WHERE tool IN ({})
                 ORDER BY last_updated DESC",
                placeholders.join(",")
            ),
            tool_strings,
        )
    };

    let mut stmt = db.prepare(&query)?;

    let tool_refs: Vec<&dyn ToSql> = tool_strings.iter().map(|s| s as &dyn ToSql).collect();

    let sessions = stmt
        .query_map(tool_refs.as_slice(), |row| {
            let tool_value: String = row.get(1)?;
            let tool = Tool::from_storage(&tool_value).unwrap_or(Tool::ClaudeCode);
            let start_time: i64 = row.get(3)?;
            let last_updated: i64 = row.get(6)?;
            let message_count: i64 = row.get(4)?;

            Ok(Session {
                id: row.get(0)?,
                tool,
                project_path: row.get(2)?,
                start_time: Utc
                    .timestamp_opt(start_time, 0)
                    .single()
                    .unwrap_or_else(Utc::now),
                message_count: message_count.max(0) as usize,
                file_path: row.get(5)?,
                last_updated: Utc
                    .timestamp_opt(last_updated, 0)
                    .single()
                    .unwrap_or_else(Utc::now),
            })
        })
        .context("Failed to query sessions")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to load sessions")?;

    Ok(sessions)
}
