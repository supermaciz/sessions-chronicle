pub mod indexer;
pub mod schema;

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use rusqlite::{Connection, Row, ToSql};
use std::collections::HashSet;
use std::path::Path;

use crate::models::{MessagePreview, Role, Session, Tool};

pub use indexer::SessionIndexer;

fn session_from_row(row: &Row) -> rusqlite::Result<Session> {
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
        first_prompt: row.get(7)?,
    })
}

fn sanitize_search_query(raw: &str) -> Option<String> {
    let tokens: Vec<String> = raw
        .split_whitespace()
        .filter_map(|token| {
            let cleaned: String = token
                .chars()
                .filter(|ch| ch.is_alphanumeric() || *ch == '_')
                .collect();
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned)
            }
        })
        .collect();

    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" AND "))
    }
}

pub fn search_sessions(db_path: &Path, tools: &[Tool], query: &str) -> Result<Vec<Session>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    if tools.is_empty() {
        return Ok(Vec::new());
    }

    let query = query.trim();
    if query.is_empty() {
        return load_sessions(db_path, tools);
    }

    let db = Connection::open(db_path).context("Failed to open database")?;

    match search_sessions_with_query(&db, tools, query) {
        Ok(sessions) => Ok(sessions),
        Err(err) => {
            let sanitized = sanitize_search_query(query);
            if let Some(sanitized) = sanitized {
                tracing::warn!(
                    "Search query failed, retrying with sanitized query '{}': {}",
                    sanitized,
                    err
                );
                match search_sessions_with_query(&db, tools, &sanitized) {
                    Ok(sessions) => Ok(sessions),
                    Err(retry_err) => {
                        tracing::warn!(
                            "Sanitized search query failed '{}': {}",
                            sanitized,
                            retry_err
                        );
                        Ok(Vec::new())
                    }
                }
            } else {
                tracing::warn!("Search query failed and could not be sanitized: {}", err);
                Ok(Vec::new())
            }
        }
    }
}

fn search_sessions_with_query(
    db: &Connection,
    tools: &[Tool],
    query: &str,
) -> Result<Vec<Session>> {
    let (query_sql, tool_strings): (String, Vec<String>) = if tools.len() == Tool::ALL.len() {
        (
            "SELECT s.id, s.tool, s.project_path, s.start_time, s.message_count, s.file_path, s.last_updated, s.first_prompt,
                    bm25(messages) AS rank
             FROM messages
             JOIN sessions s ON s.id = messages.session_id
             WHERE messages MATCH ?1
             ORDER BY rank ASC, s.last_updated DESC"
                .to_string(),
            vec![],
        )
    } else {
        let placeholders: Vec<String> = tools.iter().map(|_| "?".to_string()).collect();
        let tool_strings: Vec<String> = tools.iter().map(|t| t.to_storage()).collect::<Vec<_>>();
        (
            format!(
                "SELECT s.id, s.tool, s.project_path, s.start_time, s.message_count, s.file_path, s.last_updated, s.first_prompt,
                        bm25(messages) AS rank
                 FROM messages
                 JOIN sessions s ON s.id = messages.session_id
                 WHERE messages MATCH ?1
                   AND s.tool IN ({})
                 ORDER BY rank ASC, s.last_updated DESC",
                placeholders.join(",")
            ),
            tool_strings,
        )
    };

    let mut stmt = db.prepare(&query_sql)?;
    let mut params: Vec<&dyn ToSql> = Vec::with_capacity(1 + tool_strings.len());
    params.push(&query);
    for tool in &tool_strings {
        params.push(tool as &dyn ToSql);
    }

    let mut rows = stmt
        .query(params.as_slice())
        .context("Failed to query search results")?;
    let mut sessions = Vec::new();
    let mut seen = HashSet::new();

    while let Some(row) = rows.next()? {
        let session = session_from_row(row)?;
        if seen.insert(session.id.clone()) {
            sessions.push(session);
        }
    }

    Ok(sessions)
}

pub fn load_sessions(db_path: &Path, tools: &[Tool]) -> Result<Vec<Session>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    if tools.is_empty() {
        return Ok(Vec::new());
    }

    let db = Connection::open(db_path).context("Failed to open database")?;

    let (query, tool_strings): (String, Vec<String>) = if tools.len() == Tool::ALL.len() {
        (
            "SELECT id, tool, project_path, start_time, message_count, file_path, last_updated, first_prompt
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
                "SELECT id, tool, project_path, start_time, message_count, file_path, last_updated, first_prompt
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
        .query_map(tool_refs.as_slice(), session_from_row)
        .context("Failed to query sessions")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to load sessions")?;

    Ok(sessions)
}

/// Load a single session by ID.
pub fn load_session(db_path: &Path, session_id: &str) -> Result<Option<Session>> {
    let start = std::time::Instant::now();
    if !db_path.exists() {
        return Ok(None);
    }

    let db = Connection::open(db_path).context("Failed to open database")?;

    let mut stmt = db.prepare(
        "SELECT id, tool, project_path, start_time, message_count, file_path, last_updated, first_prompt
         FROM sessions
         WHERE id = ?1",
    )?;

    let mut rows = stmt
        .query([session_id])
        .context("Failed to query session")?;

    let result = if let Some(row) = rows.next()? {
        Ok(Some(session_from_row(row)?))
    } else {
        Ok(None)
    };

    tracing::debug!("load_session took {:?}", start.elapsed());
    result
}

/// Load message previews for a session with pagination and truncation.
pub fn load_message_previews_for_session(
    db_path: &Path,
    session_id: &str,
    limit: usize,
    offset: usize,
    preview_len: usize,
) -> Result<Vec<MessagePreview>> {
    let start = std::time::Instant::now();
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let db = Connection::open(db_path).context("Failed to open database")?;

    let mut stmt = db.prepare(
        "SELECT
          role,
          substr(content, 1, ?2) AS content_preview,
          length(content) AS content_len,
          timestamp
        FROM messages
        WHERE session_id = ?1
        ORDER BY CAST(message_index AS INTEGER) ASC
        LIMIT ?3 OFFSET ?4",
    )?;

    let mut rows = stmt
        .query([
            &session_id as &dyn ToSql,
            &(preview_len as i64),
            &(limit as i64),
            &(offset as i64),
        ])
        .context("Failed to query message previews")?;

    let mut previews = Vec::new();
    while let Some(row) = rows.next()? {
        let role_str: String = row.get(0)?;
        let role = Role::from_storage(&role_str).unwrap_or(Role::User);
        let timestamp: i64 = row.get(3)?;

        previews.push(MessagePreview {
            role,
            content_preview: row.get(1)?,
            content_len: row.get::<_, i64>(2)? as usize,
            timestamp: Utc
                .timestamp_opt(timestamp, 0)
                .single()
                .unwrap_or_else(Utc::now),
        });
    }

    tracing::debug!(
        "load_message_previews_for_session took {:?} - {} previews",
        start.elapsed(),
        previews.len()
    );

    Ok(previews)
}
