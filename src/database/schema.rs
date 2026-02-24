use anyhow::Result;
use rusqlite::Connection;

/// Initialize (or migrate) the database to the current schema version.
///
/// Versioning uses `PRAGMA user_version`:
///   0 – unversioned (pre-phase-1) or brand-new database
///   1 – phase-1 schema: sessions gains parent_session_id + is_subagent;
///       transcript_items, tool_calls, subagents tables added
///   2 – messages FTS5 table gains `model UNINDEXED` column
pub fn initialize_database(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if version < 1 {
        apply_v1_migration(conn)?;
    }
    if version < 2 {
        apply_v2_migration(conn)?;
    }

    Ok(())
}

/// Migrate from unversioned (v0) to v1.
///
/// Handles two cases:
/// - Fresh database (no `sessions` table yet): creates all tables outright.
/// - Pre-v1 database (existing `sessions` table missing the two new columns):
///   uses ALTER TABLE to add them; ignores "duplicate column" errors so the
///   function is safe to call even if a partial migration was interrupted.
fn apply_v1_migration(conn: &Connection) -> Result<()> {
    let sessions_exists: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='sessions'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    if sessions_exists {
        // Pre-v1 DB: add new columns to the existing sessions table.
        // "duplicate column name" is silently ignored so the migration is idempotent.
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN parent_session_id TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE sessions ADD COLUMN is_subagent INTEGER NOT NULL DEFAULT 0",
            [],
        );
    } else {
        // Fresh DB: create the sessions table with the full v1 schema.
        conn.execute(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                tool TEXT NOT NULL,
                project_path TEXT,
                start_time INTEGER NOT NULL,
                message_count INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                last_updated INTEGER NOT NULL,
                first_prompt TEXT,
                parent_session_id TEXT,
                is_subagent INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;
    }

    // Indexes on sessions (safe to create whether the table is new or migrated)
    conn.execute("CREATE INDEX IF NOT EXISTS idx_tool ON sessions(tool)", [])?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_project ON sessions(project_path)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_time ON sessions(start_time DESC)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_parent_session ON sessions(parent_session_id)",
        [],
    )?;

    // FTS5 messages table (unchanged from v0; IF NOT EXISTS is safe)
    conn.execute(
        "CREATE VIRTUAL TABLE IF NOT EXISTS messages USING fts5(
            session_id UNINDEXED,
            message_index UNINDEXED,
            role UNINDEXED,
            content,
            timestamp UNINDEXED
        )",
        [],
    )?;

    // New tables added in v1
    conn.execute(
        "CREATE TABLE IF NOT EXISTS transcript_items (
            session_id TEXT NOT NULL,
            item_index INTEGER NOT NULL,
            kind TEXT NOT NULL,
            message_index INTEGER,
            tool_call_id TEXT,
            subagent_id TEXT,
            PRIMARY KEY (session_id, item_index)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS tool_calls (
            id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            subagent_id TEXT,
            tool_name TEXT NOT NULL,
            status TEXT NOT NULL,
            title TEXT,
            summary TEXT,
            input_json TEXT,
            output_text TEXT,
            error_text TEXT,
            started_at INTEGER,
            ended_at INTEGER,
            duration_ms INTEGER,
            parser_call_id TEXT,
            PRIMARY KEY (session_id, id)
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tool_calls_subagent ON tool_calls(session_id, subagent_id)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tool_calls_parser_id ON tool_calls(session_id, parser_call_id)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS subagents (
            id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            title TEXT NOT NULL,
            prompt TEXT,
            result_summary TEXT,
            child_session_id TEXT,
            parser_ref TEXT,
            PRIMARY KEY (session_id, id)
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_subagents_session ON subagents(session_id)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_subagents_child ON subagents(session_id, child_session_id)",
        [],
    )?;

    // Stamp the version so future startups skip this migration.
    conn.execute_batch("PRAGMA user_version = 1")?;

    Ok(())
}

/// Migrate from v1 to v2: recreate the messages FTS5 table with a `model` column.
///
/// FTS5 virtual tables do not support ALTER TABLE ADD COLUMN, so the table is
/// dropped and recreated. Existing indexed messages are lost by design and will
/// be rebuilt by normal startup re-indexing when parsers run again.
///
/// PRAGMA user_version is set AFTER the transaction commits because it is not
/// transactional in SQLite.
fn apply_v2_migration(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "BEGIN IMMEDIATE;
         DROP TABLE IF EXISTS messages;
         CREATE VIRTUAL TABLE messages USING fts5(
             session_id UNINDEXED,
             message_index UNINDEXED,
             role UNINDEXED,
             content,
             timestamp UNINDEXED,
             model UNINDEXED
         );
         COMMIT;",
    )?;
    conn.execute_batch("PRAGMA user_version = 2")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn fresh_db_initializes_to_v2() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
    }

    #[test]
    fn v1_to_v2_migration_recreates_messages_with_model() {
        let conn = Connection::open_in_memory().unwrap();

        // Manually create a v1 schema
        conn.execute_batch(
            "
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                tool TEXT NOT NULL,
                project_path TEXT,
                start_time INTEGER NOT NULL,
                message_count INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                last_updated INTEGER NOT NULL,
                first_prompt TEXT,
                parent_session_id TEXT,
                is_subagent INTEGER NOT NULL DEFAULT 0
            );
            CREATE VIRTUAL TABLE messages USING fts5(
                session_id UNINDEXED,
                message_index UNINDEXED,
                role UNINDEXED,
                content,
                timestamp UNINDEXED
            );
            CREATE TABLE IF NOT EXISTS transcript_items (
                session_id TEXT NOT NULL,
                item_index INTEGER NOT NULL,
                kind TEXT NOT NULL,
                message_index INTEGER,
                tool_call_id TEXT,
                subagent_id TEXT,
                PRIMARY KEY (session_id, item_index)
            );
            CREATE TABLE IF NOT EXISTS tool_calls (
                id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                subagent_id TEXT,
                tool_name TEXT NOT NULL,
                status TEXT NOT NULL,
                title TEXT,
                summary TEXT,
                input_json TEXT,
                output_text TEXT,
                error_text TEXT,
                started_at INTEGER,
                ended_at INTEGER,
                duration_ms INTEGER,
                parser_call_id TEXT,
                PRIMARY KEY (session_id, id)
            );
            CREATE TABLE IF NOT EXISTS subagents (
                id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                title TEXT NOT NULL,
                prompt TEXT,
                result_summary TEXT,
                child_session_id TEXT,
                parser_ref TEXT,
                PRIMARY KEY (session_id, id)
            );
            PRAGMA user_version = 1;
        ",
        )
        .unwrap();

        // Insert a message in v1 schema
        conn.execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp) VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params!["s1", 0, "user", "hello", 100],
        )
        .unwrap();

        // Run migration
        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);

        // Old messages are gone (by design — will be re-indexed)
        let count: i64 = conn
            .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // New schema accepts model column
        conn.execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params!["s1", 0, "assistant", "hi", 200, "claude-opus-4-6"],
        )
        .unwrap();

        let model: Option<String> = conn
            .query_row(
                "SELECT model FROM messages WHERE session_id = 's1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(model.as_deref(), Some("claude-opus-4-6"));
    }

    #[test]
    fn message_insert_roundtrip_preserves_null_model() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();

        conn.execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params!["s1", 0, "user", "hello", 100, Option::<String>::None],
        )
        .unwrap();

        let model: Option<String> = conn
            .query_row(
                "SELECT model FROM messages WHERE session_id = 's1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(model.is_none());
    }
}
