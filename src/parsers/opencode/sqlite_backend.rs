use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

use crate::models::Role;
use crate::parsers::model::normalize_model;

use super::{
    MessageMetadata, OpenCodeBackend, PartData, SessionEntry, SessionMetadata, SessionSource,
    timestamp_from_millis,
};

pub struct SqliteBackend {
    conn: Connection,
    db_path: PathBuf,
}

impl SqliteBackend {
    pub fn open(db_path: &Path) -> Result<Self> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(db_path, flags)
            .with_context(|| format!("Failed to open OpenCode DB: {}", db_path.display()))?;

        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        Ok(Self {
            conn,
            db_path: db_path.to_path_buf(),
        })
    }
}

impl OpenCodeBackend for SqliteBackend {
    fn list_sessions(&self) -> Result<Vec<SessionEntry>> {
        let mut stmt = self.conn.prepare("SELECT id FROM session")?;
        let entries = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                Ok(SessionEntry {
                    id,
                    source: SessionSource::SqliteRow {
                        db_path: self.db_path.clone(),
                    },
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Failed to list sessions from SQLite")?;

        Ok(entries)
    }

    fn load_session_metadata(&self, entry: &SessionEntry) -> Result<SessionMetadata> {
        let mut stmt = self.conn.prepare(
            "SELECT id, directory, title, parent_id, time_created, time_updated
               FROM session WHERE id = ?1",
        )?;

        stmt.query_row([&entry.id], |row| {
            let id: String = row.get(0)?;
            let directory: Option<String> = row.get(1)?;
            let title: Option<String> = row.get(2)?;
            let parent_id: Option<String> = row.get(3)?;
            let created_ms: i64 = row.get(4)?;
            let updated_ms: i64 = row.get(5)?;

            Ok((id, directory, title, parent_id, created_ms, updated_ms))
        })
        .context("Session not found in SQLite")
        .and_then(
            |(id, directory, title, parent_id, created_ms, updated_ms)| {
                Ok(SessionMetadata {
                    id,
                    directory,
                    title,
                    time_created: timestamp_from_millis(created_ms)?,
                    time_updated: timestamp_from_millis(updated_ms)?,
                    parent_id,
                })
            },
        )
    }

    fn load_messages(&self, session_id: &str) -> Result<Vec<MessageMetadata>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, time_created, data FROM message
               WHERE session_id = ?1 ORDER BY time_created, id",
        )?;

        let messages = stmt
            .query_map([session_id], |row| {
                let id: String = row.get(0)?;
                let created_ms: i64 = row.get(1)?;
                let data_str: String = row.get(2)?;
                Ok((id, created_ms, data_str))
            })?
            .filter_map(|result| {
                let (id, created_ms, data_str) = match result {
                    Ok(tuple) => tuple,
                    Err(err) => {
                        tracing::warn!("Failed to read message row: {}", err);
                        return None;
                    }
                };

                let data: serde_json::Value = match serde_json::from_str(&data_str) {
                    Ok(v) => v,
                    Err(err) => {
                        tracing::warn!("Failed to parse message data for {}: {}", id, err);
                        return None;
                    }
                };

                let role = data.get("role").and_then(|v| v.as_str()).and_then(|role| {
                    match role.to_lowercase().as_str() {
                        "user" => Some(Role::User),
                        "assistant" => Some(Role::Assistant),
                        _ => None,
                    }
                });

                let time_created = match timestamp_from_millis(created_ms) {
                    Ok(ts) => ts,
                    Err(err) => {
                        tracing::warn!("Invalid timestamp for message {}: {}", id, err);
                        return None;
                    }
                };

                let model = normalize_model(data.get("modelID"))
                    .or_else(|| normalize_model(data.get("model").and_then(|m| m.get("modelID"))));

                Some(MessageMetadata {
                    id,
                    role,
                    time_created,
                    model,
                })
            })
            .collect();

        Ok(messages)
    }

    fn session_has_task_tool(
        &self,
        session_id: &str,
        _messages: &[MessageMetadata],
    ) -> Result<bool> {
        let mut stmt = self.conn.prepare(
            "SELECT EXISTS(\
                 SELECT 1 FROM part \
                 WHERE session_id = ?1 \
                 AND json_extract(data, '$.type') = 'tool' \
                 AND json_extract(data, '$.tool') = 'task'\
             )",
        )?;
        let exists: i64 = stmt.query_row([session_id], |row| row.get(0))?;
        Ok(exists != 0)
    }

    fn load_parts(&self, message_id: &str) -> Result<Vec<PartData>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, data FROM part WHERE message_id = ?1 ORDER BY id")?;

        let parts = stmt
            .query_map([message_id], |row| {
                let id: String = row.get(0)?;
                let data_str: String = row.get(1)?;
                Ok((id, data_str))
            })?
            .filter_map(|result| {
                let (id, data_str) = match result {
                    Ok(tuple) => tuple,
                    Err(err) => {
                        tracing::warn!("Failed to read part row: {}", err);
                        return None;
                    }
                };

                let raw: serde_json::Value = match serde_json::from_str(&data_str) {
                    Ok(v) => v,
                    Err(err) => {
                        tracing::warn!("Failed to parse part data for {}: {}", id, err);
                        return None;
                    }
                };

                let kind = raw
                    .get("type")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)?;

                let order = raw.get("order").and_then(|v| v.as_i64());

                Some(PartData {
                    id,
                    kind,
                    order,
                    raw,
                })
            })
            .collect();

        Ok(parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::opencode::OpenCodeBackend;
    use std::path::{Path, PathBuf};

    fn fixture_db() -> PathBuf {
        PathBuf::from("tests/fixtures/opencode_storage/opencode.db")
    }

    #[test]
    fn list_sessions_finds_all_sqlite_sessions() {
        let db_path = fixture_db();
        let backend = SqliteBackend::open(&db_path).unwrap();
        let sessions = backend.list_sessions().unwrap();

        assert_eq!(sessions.len(), 3);
        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"session-001"));
        assert!(ids.contains(&"session-sqlite-only"));
        assert!(ids.contains(&"session-sqlite-subagent"));
    }

    #[test]
    fn load_session_metadata_reads_fields() {
        let db_path = fixture_db();
        let backend = SqliteBackend::open(&db_path).unwrap();
        let sessions = backend.list_sessions().unwrap();
        let entry = sessions
            .iter()
            .find(|s| s.id == "session-sqlite-only")
            .unwrap();

        let meta = backend.load_session_metadata(entry).unwrap();
        assert_eq!(meta.id, "session-sqlite-only");
        assert_eq!(meta.directory.as_deref(), Some("/projects/beta"));
        assert_eq!(meta.title.as_deref(), Some("SQLite-only session"));
        assert!(meta.parent_id.is_none());
    }

    #[test]
    fn load_session_metadata_reads_subagent_parent_id() {
        let db_path = fixture_db();
        let backend = SqliteBackend::open(&db_path).unwrap();
        let sessions = backend.list_sessions().unwrap();
        let entry = sessions
            .iter()
            .find(|s| s.id == "session-sqlite-subagent")
            .unwrap();

        let meta = backend.load_session_metadata(entry).unwrap();
        assert_eq!(meta.parent_id.as_deref(), Some("session-sqlite-only"));
    }

    #[test]
    fn load_messages_returns_correct_count_and_roles() {
        let db_path = fixture_db();
        let backend = SqliteBackend::open(&db_path).unwrap();
        let messages = backend.load_messages("session-sqlite-only").unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Some(crate::models::Role::User));
        assert_eq!(messages[1].role, Some(crate::models::Role::Assistant));
    }

    #[test]
    fn load_parts_returns_text_parts() {
        let db_path = fixture_db();
        let backend = SqliteBackend::open(&db_path).unwrap();
        let parts = backend.load_parts("msg-sqlite-only-001").unwrap();

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].kind, "text");
        assert_eq!(
            parts[0].raw.get("text").and_then(|v| v.as_str()),
            Some("This session only exists in SQLite")
        );
    }

    #[test]
    fn open_fails_gracefully_for_missing_db() {
        let result = SqliteBackend::open(Path::new("/nonexistent/opencode.db"));
        assert!(result.is_err());
    }
}
