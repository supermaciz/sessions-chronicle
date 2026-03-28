pub mod analytics;
pub mod indexer;
pub mod schema;

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use rusqlite::{Connection, Row, ToSql};
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use crate::models::{
    AiAssistant, MessagePreview, ProjectFilter, ProjectInfo, Role, Session, Subagent, ToolCall,
    ToolCallStatus, TranscriptItem, TranscriptItemKind,
};

pub use indexer::{IndexingStats, SessionIndexer};

const SQLITE_BUSY_TIMEOUT_SECS: u64 = 5;

pub(crate) fn open_connection(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path).context("Failed to open database")?;
    conn.busy_timeout(Duration::from_secs(SQLITE_BUSY_TIMEOUT_SECS))
        .context("Failed to set SQLite busy timeout")?;
    Ok(conn)
}

/// Flat preview row returned by the transcript LEFT JOIN query.
/// The caller interprets fields based on `kind`.
#[derive(Debug, Clone)]
pub struct TranscriptItemRow {
    pub item_index: i64,
    pub kind: TranscriptItemKind,
    // Message fields
    pub message_index: Option<i64>,
    pub role: Option<Role>,
    pub content_preview: Option<String>,
    pub content_len: Option<i64>,
    pub timestamp: Option<i64>,
    pub model: Option<String>,
    // ToolCall fields
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_status: Option<ToolCallStatus>,
    pub tool_summary: Option<String>,
    pub tool_input_json: Option<String>,
    pub tool_output_text: Option<String>,
    pub duration_ms: Option<i64>,
    // Subagent fields
    pub subagent_id: Option<String>,
    pub subagent_title: Option<String>,
    #[allow(dead_code)]
    pub subagent_prompt: Option<String>,
}

fn session_from_row(row: &Row) -> rusqlite::Result<Session> {
    let tool_value: String = row.get("tool")?;
    let tool = AiAssistant::from_storage(&tool_value).unwrap_or(AiAssistant::ClaudeCode);
    let start_time: i64 = row.get("start_time")?;
    let last_updated: i64 = row.get("last_updated")?;
    let message_count: i64 = row.get("message_count")?;
    let is_subagent_int: i64 = row.get("is_subagent").unwrap_or(0);

    let input_tokens: Option<i64> = row.get("input_tokens").unwrap_or(None);
    let output_tokens: Option<i64> = row.get("output_tokens").unwrap_or(None);
    let token_usage = match (input_tokens, output_tokens) {
        (Some(input), Some(output)) => Some(crate::models::TokenUsage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: row.get("cache_read_tokens").unwrap_or(None),
            cache_write_tokens: row.get("cache_write_tokens").unwrap_or(None),
            reasoning_tokens: row.get("reasoning_tokens").unwrap_or(None),
        }),
        (Some(_), None) | (None, Some(_)) => {
            tracing::warn!(
                "Inconsistent token data for session (input xor output), treating as unavailable"
            );
            None
        }
        (None, None) => None,
    };

    Ok(Session {
        id: row.get("id")?,
        tool,
        project_path: row.get("project_path")?,
        project_id: row.get("project_id").unwrap_or(None),
        start_time: Utc
            .timestamp_opt(start_time, 0)
            .single()
            .unwrap_or_else(Utc::now),
        message_count: message_count.max(0) as usize,
        file_path: row.get("file_path")?,
        last_updated: Utc
            .timestamp_opt(last_updated, 0)
            .single()
            .unwrap_or_else(Utc::now),
        first_prompt: row.get("first_prompt")?,
        parent_session_id: row.get("parent_session_id")?,
        is_subagent: is_subagent_int != 0,
        token_usage,
        edit_count: row.get::<_, i64>("edit_count").unwrap_or(0).max(0) as usize,
        read_count: row.get::<_, i64>("read_count").unwrap_or(0).max(0) as usize,
        command_count: row.get::<_, i64>("command_count").unwrap_or(0).max(0) as usize,
        ending_status: crate::models::SessionEndingStatus::from_storage(
            &row.get::<_, String>("ending_status")
                .unwrap_or_else(|_| "unknown".to_string()),
        ),
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

#[cfg_attr(not(test), allow(dead_code))]
pub fn search_sessions(db_path: &Path, tools: &[AiAssistant], query: &str) -> Result<Vec<Session>> {
    search_sessions_for_filter(db_path, tools, &ProjectFilter::AllSessions, query)
}

pub fn search_sessions_for_filter(
    db_path: &Path,
    tools: &[AiAssistant],
    project_filter: &ProjectFilter,
    query: &str,
) -> Result<Vec<Session>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    if tools.is_empty() {
        return Ok(Vec::new());
    }

    let query = query.trim();
    if query.is_empty() {
        return load_sessions_for_filter(db_path, tools, project_filter);
    }

    let db = open_connection(db_path)?;

    match search_sessions_with_query(&db, tools, project_filter, query) {
        Ok(sessions) => Ok(sessions),
        Err(err) => {
            let sanitized = sanitize_search_query(query);
            if let Some(sanitized) = sanitized {
                tracing::warn!(
                    "Search query failed, retrying with sanitized query '{}': {}",
                    sanitized,
                    err
                );
                match search_sessions_with_query(&db, tools, project_filter, &sanitized) {
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
    tools: &[AiAssistant],
    project_filter: &ProjectFilter,
    query: &str,
) -> Result<Vec<Session>> {
    let project_clause = match project_filter {
        ProjectFilter::AllSessions => String::new(),
        ProjectFilter::Project(_) => " AND s.project_id = ?".to_string(),
        ProjectFilter::Unassigned => " AND s.project_id IS NULL".to_string(),
    };

    let (query_sql, tool_strings): (String, Vec<String>) = if tools.len() == AiAssistant::ALL.len()
    {
        (
            format!(
                "SELECT s.id, s.tool, s.project_path, s.project_id, s.start_time, s.message_count, s.file_path,
                        s.last_updated, s.first_prompt, s.parent_session_id, s.is_subagent,
                        s.input_tokens, s.output_tokens, s.cache_read_tokens,
                        s.cache_write_tokens, s.reasoning_tokens,
                        s.edit_count, s.read_count, s.command_count, s.ending_status,
                        bm25(messages) AS rank
                 FROM messages
                 JOIN sessions s ON s.id = messages.session_id
                 WHERE messages MATCH ?
                   AND s.is_subagent = 0
                   {}
                 ORDER BY rank ASC, s.last_updated DESC",
                project_clause
            ),
            vec![],
        )
    } else {
        let placeholders: Vec<String> = tools.iter().map(|_| "?".to_string()).collect();
        let tool_strings: Vec<String> = tools.iter().map(|t| t.to_storage()).collect::<Vec<_>>();
        (
            format!(
                "SELECT s.id, s.tool, s.project_path, s.project_id, s.start_time, s.message_count, s.file_path,
                         s.last_updated, s.first_prompt, s.parent_session_id, s.is_subagent,
                         s.input_tokens, s.output_tokens, s.cache_read_tokens,
                         s.cache_write_tokens, s.reasoning_tokens,
                         s.edit_count, s.read_count, s.command_count, s.ending_status,
                         bm25(messages) AS rank
                  FROM messages
                  JOIN sessions s ON s.id = messages.session_id
                  WHERE messages MATCH ?
                    AND s.tool IN ({})
                    AND s.is_subagent = 0
                    {}
                  ORDER BY rank ASC, s.last_updated DESC",
                placeholders.join(","),
                project_clause
            ),
            tool_strings,
        )
    };

    let mut stmt = db.prepare(&query_sql)?;
    let mut params: Vec<&dyn ToSql> = Vec::with_capacity(2 + tool_strings.len());
    params.push(&query);
    for tool in &tool_strings {
        params.push(tool as &dyn ToSql);
    }
    let project_id = match project_filter {
        ProjectFilter::Project(id) => Some(*id),
        _ => None,
    };
    if let Some(project_id) = project_id.as_ref() {
        params.push(project_id as &dyn ToSql);
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

#[cfg_attr(not(test), allow(dead_code))]
pub fn load_sessions(db_path: &Path, tools: &[AiAssistant]) -> Result<Vec<Session>> {
    load_sessions_for_filter(db_path, tools, &ProjectFilter::AllSessions)
}

pub fn load_sessions_for_filter(
    db_path: &Path,
    tools: &[AiAssistant],
    project_filter: &ProjectFilter,
) -> Result<Vec<Session>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    if tools.is_empty() {
        return Ok(Vec::new());
    }

    let db = open_connection(db_path)?;

    let project_clause = match project_filter {
        ProjectFilter::AllSessions => String::new(),
        ProjectFilter::Project(_) => " AND project_id = ?".to_string(),
        ProjectFilter::Unassigned => " AND project_id IS NULL".to_string(),
    };

    let (query, tool_strings): (String, Vec<String>) = if tools.len() == AiAssistant::ALL.len() {
        (
            format!(
                "SELECT id, tool, project_path, project_id, start_time, message_count, file_path,
                        last_updated, first_prompt, parent_session_id, is_subagent,
                        input_tokens, output_tokens, cache_read_tokens,
                        cache_write_tokens, reasoning_tokens,
                        edit_count, read_count, command_count, ending_status
                 FROM sessions
                 WHERE is_subagent = 0
                   {}
                 ORDER BY last_updated DESC",
                project_clause
            ),
            vec![],
        )
    } else {
        let placeholders: Vec<String> = tools.iter().map(|_| "?".to_string()).collect();
        let tool_strings: Vec<String> = tools.iter().map(|t| t.to_storage()).collect::<Vec<_>>();
        (
            format!(
                "SELECT id, tool, project_path, project_id, start_time, message_count, file_path,
                         last_updated, first_prompt, parent_session_id, is_subagent,
                         input_tokens, output_tokens, cache_read_tokens,
                         cache_write_tokens, reasoning_tokens,
                         edit_count, read_count, command_count, ending_status
                  FROM sessions
                  WHERE tool IN ({})
                    AND is_subagent = 0
                    {}
                  ORDER BY last_updated DESC",
                placeholders.join(","),
                project_clause
            ),
            tool_strings,
        )
    };

    let mut stmt = db.prepare(&query)?;

    let mut params: Vec<&dyn ToSql> = Vec::with_capacity(1 + tool_strings.len());
    for tool in &tool_strings {
        params.push(tool as &dyn ToSql);
    }
    let project_id = match project_filter {
        ProjectFilter::Project(id) => Some(*id),
        _ => None,
    };
    if let Some(project_id) = project_id.as_ref() {
        params.push(project_id as &dyn ToSql);
    }

    let sessions = stmt
        .query_map(params.as_slice(), session_from_row)
        .context("Failed to query sessions")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to load sessions")?;

    Ok(sessions)
}

pub fn load_projects(db_path: &Path, tools: &[AiAssistant]) -> Result<Vec<ProjectInfo>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let db = open_connection(db_path)?;

    let (query, tool_strings): (String, Vec<String>) = if tools.is_empty() {
        (
            "SELECT p.id, p.name, p.path, 0 AS session_count
             FROM projects p
             ORDER BY p.name COLLATE NOCASE ASC"
                .to_string(),
            vec![],
        )
    } else if tools.len() == AiAssistant::ALL.len() {
        (
            "SELECT p.id, p.name, p.path, COUNT(s.id) AS session_count, MAX(s.last_updated) AS project_last_updated
             FROM projects p
             LEFT JOIN sessions s ON s.project_id = p.id
                                AND s.is_subagent = 0
             GROUP BY p.id, p.name, p.path
             ORDER BY CASE WHEN COUNT(s.id) > 0 THEN 0 ELSE 1 END,
                      project_last_updated DESC,
                       p.name COLLATE NOCASE ASC"
                .to_string(),
            vec![],
        )
    } else {
        let placeholders: Vec<String> = tools.iter().map(|_| "?".to_string()).collect();
        let tool_strings: Vec<String> = tools.iter().map(|t| t.to_storage()).collect::<Vec<_>>();
        (
            format!(
                "SELECT p.id, p.name, p.path, COUNT(s.id) AS session_count, MAX(s.last_updated) AS project_last_updated
                 FROM projects p
                 LEFT JOIN sessions s ON s.project_id = p.id
                                    AND s.is_subagent = 0
                                    AND s.tool IN ({})
                 GROUP BY p.id, p.name, p.path
                 ORDER BY CASE WHEN COUNT(s.id) > 0 THEN 0 ELSE 1 END,
                          project_last_updated DESC,
                           p.name COLLATE NOCASE ASC",
                placeholders.join(",")
            ),
            tool_strings,
        )
    };

    let mut stmt = db.prepare(&query)?;
    let tool_refs: Vec<&dyn ToSql> = tool_strings.iter().map(|s| s as &dyn ToSql).collect();
    let mut rows = stmt.query(tool_refs.as_slice())?;

    let mut projects = Vec::new();
    while let Some(row) = rows.next()? {
        let session_count: i64 = row.get(3)?;
        projects.push(ProjectInfo {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
            session_count: session_count.max(0) as usize,
        });
    }

    Ok(projects)
}

pub fn count_all_sessions(db_path: &Path, tools: &[AiAssistant]) -> Result<usize> {
    if !db_path.exists() {
        return Ok(0);
    }

    if tools.is_empty() {
        return Ok(0);
    }

    let db = open_connection(db_path)?;

    let (query, tool_strings): (String, Vec<String>) = if tools.len() == AiAssistant::ALL.len() {
        (
            "SELECT COUNT(*) FROM sessions WHERE is_subagent = 0".to_string(),
            vec![],
        )
    } else {
        let placeholders: Vec<String> = tools.iter().map(|_| "?".to_string()).collect();
        let tool_strings: Vec<String> = tools.iter().map(|t| t.to_storage()).collect::<Vec<_>>();
        (
            format!(
                "SELECT COUNT(*) FROM sessions WHERE is_subagent = 0 AND tool IN ({})",
                placeholders.join(",")
            ),
            tool_strings,
        )
    };

    let mut stmt = db.prepare(&query)?;
    let tool_refs: Vec<&dyn ToSql> = tool_strings.iter().map(|s| s as &dyn ToSql).collect();
    let count: i64 = stmt.query_row(tool_refs.as_slice(), |row| row.get(0))?;

    Ok(count.max(0) as usize)
}

pub fn count_unassigned_sessions(db_path: &Path, tools: &[AiAssistant]) -> Result<usize> {
    if !db_path.exists() {
        return Ok(0);
    }

    if tools.is_empty() {
        return Ok(0);
    }

    let db = open_connection(db_path)?;

    let (query, tool_strings): (String, Vec<String>) = if tools.len() == AiAssistant::ALL.len() {
        (
            "SELECT COUNT(*) FROM sessions
             WHERE project_id IS NULL
               AND is_subagent = 0"
                .to_string(),
            vec![],
        )
    } else {
        let placeholders: Vec<String> = tools.iter().map(|_| "?".to_string()).collect();
        let tool_strings: Vec<String> = tools.iter().map(|t| t.to_storage()).collect::<Vec<_>>();
        (
            format!(
                "SELECT COUNT(*) FROM sessions
                 WHERE project_id IS NULL
                   AND is_subagent = 0
                   AND tool IN ({})",
                placeholders.join(",")
            ),
            tool_strings,
        )
    };

    let mut stmt = db.prepare(&query)?;
    let tool_refs: Vec<&dyn ToSql> = tool_strings.iter().map(|s| s as &dyn ToSql).collect();
    let count: i64 = stmt.query_row(tool_refs.as_slice(), |row| row.get(0))?;

    Ok(count.max(0) as usize)
}

/// Check whether any unassigned (no project) non-subagent session exists in the database.
/// This is intentionally tool-agnostic: the "Unassigned" sidebar row stays visible even
/// when a tool filter makes its visible count zero, so users always know unassigned
/// sessions exist.
pub fn has_unassigned_sessions(db_path: &Path) -> Result<bool> {
    if !db_path.exists() {
        return Ok(false);
    }

    let db = open_connection(db_path)?;
    let mut stmt = db.prepare(
        "SELECT EXISTS(
             SELECT 1
             FROM sessions
             WHERE project_id IS NULL
               AND is_subagent = 0
         )",
    )?;
    let exists: i64 = stmt.query_row([], |row| row.get(0))?;

    Ok(exists != 0)
}

/// Load a single session by ID (may be a subagent session).
pub fn load_session(db_path: &Path, session_id: &str) -> Result<Option<Session>> {
    let start = std::time::Instant::now();
    if !db_path.exists() {
        return Ok(None);
    }

    let db = open_connection(db_path)?;

    let mut stmt = db.prepare(
        "SELECT id, tool, project_path, project_id, start_time, message_count, file_path,
                last_updated, first_prompt, parent_session_id, is_subagent,
                input_tokens, output_tokens, cache_read_tokens,
                cache_write_tokens, reasoning_tokens,
                edit_count, read_count, command_count, ending_status
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

/// Load the full (untruncated) content of a single message.
pub fn load_message_full_content(
    db_path: &Path,
    session_id: &str,
    message_index: usize,
) -> Result<String> {
    let db = open_connection(db_path)?;

    let mut stmt = db.prepare(
        "SELECT content FROM messages WHERE session_id = ?1 AND CAST(message_index AS INTEGER) = ?2",
    )?;

    let mut rows = stmt
        .query([&session_id as &dyn ToSql, &(message_index as i64)])
        .context("Failed to query full message content")?;

    if let Some(row) = rows.next()? {
        Ok(row.get(0)?)
    } else {
        anyhow::bail!(
            "Message not found: session={} index={}",
            session_id,
            message_index
        )
    }
}

/// Load message previews for a session with pagination and truncation.
#[allow(dead_code)]
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

    let db = open_connection(db_path)?;

    let mut stmt = db.prepare(
        "SELECT
          session_id,
          CAST(message_index AS INTEGER) AS message_index,
          role,
          substr(content, 1, ?2) AS content_preview,
          length(content) AS content_len,
          timestamp,
          model
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
        let role_str: String = row.get(2)?;
        let role = Role::from_storage(&role_str).unwrap_or(Role::User);
        let timestamp: i64 = row.get(5)?;

        previews.push(MessagePreview {
            session_id: row.get(0)?,
            message_index: row.get::<_, i64>(1)? as usize,
            role,
            content_preview: row.get(3)?,
            content_len: row.get::<_, i64>(4)? as usize,
            timestamp: Utc
                .timestamp_opt(timestamp, 0)
                .single()
                .unwrap_or_else(Utc::now),
            model: row.get(6)?,
        });
    }

    tracing::debug!(
        "load_message_previews_for_session took {:?} - {} previews",
        start.elapsed(),
        previews.len()
    );

    Ok(previews)
}

/// Load ordered transcript items for a session with pagination.
/// Returns preview rows combining message/tool_call/subagent fields via LEFT JOINs.
pub fn load_transcript_items(
    db_path: &Path,
    session_id: &str,
    limit: i64,
    offset: i64,
    preview_len: i64,
) -> Result<Vec<TranscriptItemRow>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let db = open_connection(db_path)?;

    let mut stmt = db.prepare(
        "SELECT ti.item_index, ti.kind, ti.message_index, ti.tool_call_id, ti.subagent_id,
                m.role, substr(m.content, 1, ?2) AS content_preview,
                length(m.content) AS content_len, m.timestamp, m.model,
                tc.tool_name, tc.status, tc.summary,
                substr(tc.input_json, 1, 512) AS input_json,
                substr(tc.output_text, 1, 512) AS output_text,
                tc.duration_ms,
                sa.title AS subagent_title, sa.prompt AS subagent_prompt
         FROM transcript_items ti
         LEFT JOIN messages m ON ti.session_id = m.session_id
                             AND ti.message_index = CAST(m.message_index AS INTEGER)
         LEFT JOIN tool_calls tc ON ti.session_id = tc.session_id
                                AND ti.tool_call_id = tc.id
         LEFT JOIN subagents sa ON ti.session_id = sa.session_id
                               AND ti.subagent_id = sa.id
         WHERE ti.session_id = ?1
         ORDER BY ti.item_index
         LIMIT ?3 OFFSET ?4",
    )?;

    let mut rows = stmt
        .query([&session_id as &dyn ToSql, &preview_len, &limit, &offset])
        .context("Failed to query transcript items")?;

    // Column indices matching the SELECT order above.
    const COL_ITEM_INDEX: usize = 0;
    const COL_KIND: usize = 1;
    const COL_MSG_INDEX: usize = 2;
    const COL_TOOL_CALL_ID: usize = 3;
    const COL_SUBAGENT_ID: usize = 4;
    const COL_ROLE: usize = 5;
    const COL_CONTENT_PREVIEW: usize = 6;
    const COL_CONTENT_LEN: usize = 7;
    const COL_TIMESTAMP: usize = 8;
    const COL_MODEL: usize = 9;
    const COL_TOOL_NAME: usize = 10;
    const COL_TOOL_STATUS: usize = 11;
    const COL_TOOL_SUMMARY: usize = 12;
    const COL_TOOL_INPUT_JSON: usize = 13;
    const COL_TOOL_OUTPUT_TEXT: usize = 14;
    const COL_DURATION_MS: usize = 15;
    const COL_SUBAGENT_TITLE: usize = 16;
    const COL_SUBAGENT_PROMPT: usize = 17;

    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        let kind_str: String = row.get(COL_KIND)?;
        let kind = TranscriptItemKind::from_storage(&kind_str);

        let role: Option<String> = row.get(COL_ROLE)?;
        let tool_status: Option<String> = row.get(COL_TOOL_STATUS)?;

        items.push(TranscriptItemRow {
            item_index: row.get(COL_ITEM_INDEX)?,
            kind,
            message_index: row.get(COL_MSG_INDEX)?,
            tool_call_id: row.get(COL_TOOL_CALL_ID)?,
            subagent_id: row.get(COL_SUBAGENT_ID)?,
            role: role.as_deref().and_then(Role::from_storage),
            content_preview: row.get(COL_CONTENT_PREVIEW)?,
            content_len: row.get(COL_CONTENT_LEN)?,
            timestamp: row.get(COL_TIMESTAMP)?,
            model: row.get(COL_MODEL)?,
            tool_name: row.get(COL_TOOL_NAME)?,
            tool_status: tool_status.as_deref().map(ToolCallStatus::from_storage),
            tool_summary: row.get(COL_TOOL_SUMMARY)?,
            tool_input_json: row.get(COL_TOOL_INPUT_JSON)?,
            tool_output_text: row.get(COL_TOOL_OUTPUT_TEXT)?,
            duration_ms: row.get(COL_DURATION_MS)?,
            subagent_title: row.get(COL_SUBAGENT_TITLE)?,
            subagent_prompt: row.get(COL_SUBAGENT_PROMPT)?,
        });
    }

    Ok(items)
}

/// Insert a tool call record (upsert by session_id + id).
pub fn insert_tool_call(conn: &Connection, tc: &ToolCall, session_id: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO tool_calls
         (id, session_id, subagent_id, tool_name, status, title, summary,
          input_json, output_text, error_text, started_at, ended_at, duration_ms, parser_call_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        rusqlite::params![
            tc.id,
            session_id,
            tc.subagent_id,
            tc.tool_name,
            tc.status.to_storage(),
            tc.title,
            tc.summary,
            tc.input_json,
            tc.output_text,
            tc.error_text,
            tc.started_at,
            tc.ended_at,
            tc.duration_ms,
            tc.parser_call_id,
        ],
    )
    .context("Failed to insert tool call")?;
    Ok(())
}

/// Insert a subagent record (upsert by session_id + id).
pub fn insert_subagent(conn: &Connection, sa: &Subagent, session_id: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO subagents
         (id, session_id, title, prompt, result_summary, child_session_id, parser_ref)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            sa.id,
            session_id,
            sa.title,
            sa.prompt,
            sa.result_summary,
            sa.child_session_id,
            sa.parser_ref,
        ],
    )
    .context("Failed to insert subagent")?;
    Ok(())
}

/// Insert a transcript item (upsert by session_id + item_index).
pub fn insert_transcript_item(
    conn: &Connection,
    item: &TranscriptItem,
    session_id: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO transcript_items
         (session_id, item_index, kind, message_index, tool_call_id, subagent_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            session_id,
            item.item_index,
            item.kind.to_storage(),
            item.message_index,
            item.tool_call_id,
            item.subagent_id,
        ],
    )
    .context("Failed to insert transcript item")?;
    Ok(())
}

/// Load a single tool call by session_id and id.
pub fn load_tool_call(
    db_path: &Path,
    session_id: &str,
    tool_call_id: &str,
) -> Result<Option<ToolCall>> {
    if !db_path.exists() {
        return Ok(None);
    }
    let db = open_connection(db_path)?;
    let mut stmt = db.prepare(
        "SELECT id, session_id, subagent_id, tool_name, status, title, summary,
                input_json, output_text, error_text, started_at, ended_at,
                duration_ms, parser_call_id
         FROM tool_calls
         WHERE session_id = ?1 AND id = ?2",
    )?;
    let mut rows = stmt
        .query(rusqlite::params![session_id, tool_call_id])
        .context("Failed to query tool call")?;
    if let Some(row) = rows.next()? {
        let status_str: String = row.get(4)?;
        Ok(Some(ToolCall {
            id: row.get(0)?,
            session_id: row.get(1)?,
            subagent_id: row.get(2)?,
            tool_name: row.get(3)?,
            status: ToolCallStatus::from_storage(&status_str),
            title: row.get(5)?,
            summary: row.get(6)?,
            input_json: row.get(7)?,
            output_text: row.get(8)?,
            error_text: row.get(9)?,
            started_at: row.get(10)?,
            ended_at: row.get(11)?,
            duration_ms: row.get(12)?,
            parser_call_id: row.get(13)?,
        }))
    } else {
        Ok(None)
    }
}

/// Load a single subagent by session_id and id.
pub fn load_subagent(
    db_path: &Path,
    session_id: &str,
    subagent_id: &str,
) -> Result<Option<Subagent>> {
    if !db_path.exists() {
        return Ok(None);
    }
    let db = open_connection(db_path)?;
    let mut stmt = db.prepare(
        "SELECT id, session_id, title, prompt, result_summary, child_session_id, parser_ref
         FROM subagents
         WHERE session_id = ?1 AND id = ?2",
    )?;
    let mut rows = stmt
        .query(rusqlite::params![session_id, subagent_id])
        .context("Failed to query subagent")?;
    if let Some(row) = rows.next()? {
        Ok(Some(Subagent {
            id: row.get(0)?,
            session_id: row.get(1)?,
            title: row.get(2)?,
            prompt: row.get(3)?,
            result_summary: row.get(4)?,
            child_session_id: row.get(5)?,
            parser_ref: row.get(6)?,
        }))
    } else {
        Ok(None)
    }
}

/// Load all tool calls owned by a subagent, ordered by rowid (insertion order).
pub fn load_tool_calls_for_subagent(
    db_path: &Path,
    session_id: &str,
    subagent_id: &str,
) -> Result<Vec<ToolCall>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }
    let db = open_connection(db_path)?;
    let mut stmt = db.prepare(
        "SELECT id, session_id, subagent_id, tool_name, status, title, summary,
                input_json, output_text, error_text, started_at, ended_at,
                duration_ms, parser_call_id
         FROM tool_calls
         WHERE session_id = ?1 AND subagent_id = ?2
         ORDER BY rowid",
    )?;
    let mut rows = stmt
        .query(rusqlite::params![session_id, subagent_id])
        .context("Failed to query subagent tool calls")?;
    let mut tools = Vec::new();
    while let Some(row) = rows.next()? {
        let status_str: String = row.get(4)?;
        tools.push(ToolCall {
            id: row.get(0)?,
            session_id: row.get(1)?,
            subagent_id: row.get(2)?,
            tool_name: row.get(3)?,
            status: ToolCallStatus::from_storage(&status_str),
            title: row.get(5)?,
            summary: row.get(6)?,
            input_json: row.get(7)?,
            output_text: row.get(8)?,
            error_text: row.get(9)?,
            started_at: row.get(10)?,
            ended_at: row.get(11)?,
            duration_ms: row.get(12)?,
            parser_call_id: row.get(13)?,
        });
    }
    Ok(tools)
}
