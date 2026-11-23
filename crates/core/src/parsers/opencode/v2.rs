//! Native OpenCode V2 SQLite reader.
//!
//! Reads `session_v2`, `session_message`, and optional `session_part` (see
//! upstream `packages/core/src/session/sql.ts`) through a read-only connection. A
//! shared read transaction pins one consistent snapshot for both generations;
//! it is rolled back when the last reader is dropped and the source
//! database is never modified.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Statement};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::parsers::ParsedSession;

use super::v2_projection::{V2MessageRow, V2Projection};
use super::{SessionMetadata, timestamp_from_millis};

pub(crate) struct V2Reader {
    conn: Rc<Connection>,
    db_path: PathBuf,
    has_part_table: bool,
}

impl V2Reader {
    #[cfg(test)]
    pub(crate) fn open(db_path: &Path) -> Result<Self> {
        Self::from_connection(db_path, super::sqlite_source::open_read_snapshot(db_path)?)
    }

    pub(crate) fn from_connection(db_path: &Path, conn: Rc<Connection>) -> Result<Self> {
        let has_part_table = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema
                               WHERE type = 'table' AND name = 'session_part')",
                [],
                |row| row.get(0),
            )
            .context("Failed to detect OpenCode V2 session_part table")?;
        Ok(Self {
            conn,
            db_path: db_path.to_path_buf(),
            has_part_table,
        })
    }

    pub(crate) fn list_session_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT id FROM session_v2 ORDER BY id")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Failed to list V2 sessions")?;
        Ok(ids)
    }

    pub(crate) fn parse_session(&self, session_id: &str) -> Result<ParsedSession> {
        let metadata = self.load_metadata(session_id)?;
        let mut projection = V2Projection::new(metadata, &self.db_path);

        // Stream message rows ordered by `seq, id`; never materialize the whole
        // source table into a Vec first.
        let mut stmt = self.conn.prepare(
            "SELECT id, seq, type, time_created, data
               FROM session_message
              WHERE session_id = ?1
              ORDER BY seq, id",
        )?;
        let mut rows = stmt.query([session_id])?;
        let mut part_stmt = if self.has_part_table {
            Some(self.conn.prepare_cached(
                "SELECT ordinal, data FROM session_part
                  WHERE session_id = ?1 AND message_id = ?2
                  ORDER BY ordinal",
            )?)
        } else {
            None
        };

        while let Some(row) = rows.next()? {
            let id: String = match row.get(0) {
                Ok(id) => id,
                Err(_) => {
                    tracing::warn!("V2 message row with unreadable id in session {session_id}");
                    projection.note_corrupt_row(None);
                    continue;
                }
            };
            let seq: i64 = match row.get(1) {
                Ok(seq) => seq,
                Err(_) => {
                    tracing::warn!("V2 message {id} has unreadable seq in session {session_id}");
                    projection.note_corrupt_row(None);
                    continue;
                }
            };
            let kind: String = match row.get(2) {
                Ok(kind) => kind,
                Err(_) => {
                    tracing::warn!("V2 message {id} has unreadable type in session {session_id}");
                    projection.note_corrupt_row(None);
                    continue;
                }
            };
            let created_ms: i64 = match row.get(3) {
                Ok(ms) => ms,
                Err(_) => {
                    tracing::warn!("V2 message {id} has unreadable time in session {session_id}");
                    projection.note_corrupt_row(Some(&kind));
                    continue;
                }
            };
            let timestamp = match timestamp_from_millis(created_ms) {
                Ok(ts) => ts,
                Err(_) => {
                    tracing::warn!("V2 message {id} has invalid timestamp in session {session_id}");
                    projection.note_corrupt_row(Some(&kind));
                    continue;
                }
            };
            let mut data: Value = match row.get::<_, String>(4) {
                Ok(raw) => match serde_json::from_str(&raw) {
                    Ok(value) => value,
                    Err(_) => {
                        push_or_skip_malformed(
                            &mut projection,
                            id,
                            seq,
                            timestamp,
                            kind,
                            session_id,
                        );
                        continue;
                    }
                },
                Err(_) => {
                    push_or_skip_malformed(&mut projection, id, seq, timestamp, kind, session_id);
                    continue;
                }
            };

            if kind == "assistant"
                && let (Some(statement), Some(data)) = (part_stmt.as_mut(), data.as_object_mut())
            {
                restore_assistant_parts(statement, session_id, &id, data)
                    .with_context(|| format!("Failed to read parts of V2 message {id}"))?;
            }

            projection.push(V2MessageRow {
                id,
                seq,
                timestamp,
                kind,
                data,
            });
        }

        projection.finish()
    }

    fn load_metadata(&self, session_id: &str) -> Result<SessionMetadata> {
        let mut stmt = self.conn.prepare(
            "SELECT id, directory, title, parent_id, time_created, time_updated
               FROM session_v2 WHERE id = ?1",
        )?;
        let row = stmt
            .query_row([session_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .optional()
            .context("Failed to read V2 session metadata")?;

        let Some((id, directory, title, parent_id, created_ms, updated_ms)) = row else {
            anyhow::bail!("V2 session {session_id} not found");
        };

        Ok(SessionMetadata {
            id,
            directory,
            title,
            parent_id,
            time_created: timestamp_from_millis(created_ms)?,
            time_updated: timestamp_from_millis(updated_ms)?,
        })
    }
}

/// Match OpenCode's history reader: stored parts precede any inline tail.
/// Only the current message's parts are held in memory.
fn restore_assistant_parts(
    stmt: &mut Statement<'_>,
    session_id: &str,
    message_id: &str,
    data: &mut Map<String, Value>,
) -> Result<()> {
    let mut rows = stmt.query([session_id, message_id])?;
    let mut parts = Vec::new();
    while let Some(row) = rows.next()? {
        let part = row
            .get_ref(1)
            .ok()
            .and_then(|value| value.as_str().ok())
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
        match part {
            Some(part) => parts.push(part),
            None => {
                let ordinal = row.get::<_, i64>(0).ok();
                tracing::warn!("V2 message {message_id} has malformed part at ordinal {ordinal:?}");
            }
        }
    }
    if !parts.is_empty() {
        if let Some(Value::Array(mut inline)) = data.remove("content") {
            parts.append(&mut inline);
        }
        data.insert("content".to_owned(), Value::Array(parts));
    }
    Ok(())
}

/// Handles a row whose `data` payload could not be read. A corrupt user row is
/// surfaced to the projection (as `Value::Null`) so `finish` returns
/// `IncompleteTranscript` instead of misclassifying the session as having no
/// user messages; every other malformed payload is skipped with a diagnostic.
fn push_or_skip_malformed(
    projection: &mut V2Projection,
    id: String,
    seq: i64,
    timestamp: DateTime<Utc>,
    kind: String,
    session_id: &str,
) {
    if kind == "user" {
        projection.push(V2MessageRow {
            id,
            seq,
            timestamp,
            kind,
            data: Value::Null,
        });
    } else {
        tracing::warn!("V2 message {id} in session {session_id} has malformed data");
    }
}
