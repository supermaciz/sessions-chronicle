//! Native OpenCode V2 SQLite reader.
//!
//! Reads the `session_v2` / `session_message` tables (see the pinned upstream
//! `packages/core/src/session/sql.ts`) through a read-only connection. A
//! deferred read transaction opened in [`V2Reader::open`] pins one consistent
//! snapshot for the reader's lifetime; it is rolled back on drop and the source
//! database is never modified.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::parsers::ParsedSession;

use super::v2_projection::{V2MessageRow, V2Projection};
use super::{SessionMetadata, timestamp_from_millis};

pub(crate) struct V2Reader {
    conn: Connection,
    db_path: PathBuf,
}

impl V2Reader {
    pub(crate) fn open(db_path: &Path) -> Result<Self> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(db_path, flags)
            .with_context(|| format!("Failed to open OpenCode V2 DB: {}", db_path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // Begin a deferred read transaction so enumeration and every parse see
        // one consistent snapshot. It is rolled back on drop.
        conn.execute_batch("BEGIN")?;
        Ok(Self {
            conn,
            db_path: db_path.to_path_buf(),
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
            let data: Value = match row.get::<_, String>(4) {
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

impl Drop for V2Reader {
    fn drop(&mut self) {
        let _ = self.conn.execute_batch("ROLLBACK");
    }
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
