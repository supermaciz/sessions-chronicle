use anyhow::Result;
use rusqlite::Connection;

/// Initialize (or migrate) the database to the current schema version.
///
/// Versioning uses `PRAGMA user_version`:
///   0 – unversioned (pre-phase-1) or brand-new database
///   1 – phase-1 schema: sessions gains parent_session_id + is_subagent;
///       transcript_items, tool_calls, subagents tables added
///   2 – messages FTS5 table gains `model UNINDEXED` column
///   3 – sessions gains token usage columns (input_tokens, output_tokens,
///       cache_read_tokens, cache_write_tokens, reasoning_tokens)
///   4 – file_fingerprints table for incremental indexing
pub fn initialize_database(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if version < 1 {
        apply_v1_migration(conn)?;
    }
    if version < 2 {
        apply_v2_migration(conn)?;
    }
    if version < 3 {
        apply_v3_migration(conn)?;
    }
    if version < 4 {
        apply_v4_migration(conn)?;
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

/// Migrate from v2 to v3: add token usage columns to the sessions table.
///
/// Uses individual ALTER TABLE statements; "duplicate column name" errors are
/// ignored so the migration is idempotent (safe after partial runs).
/// Any other error is propagated immediately.
fn apply_v3_migration(conn: &Connection) -> Result<()> {
    let columns = [
        "ALTER TABLE sessions ADD COLUMN input_tokens INTEGER",
        "ALTER TABLE sessions ADD COLUMN output_tokens INTEGER",
        "ALTER TABLE sessions ADD COLUMN cache_read_tokens INTEGER",
        "ALTER TABLE sessions ADD COLUMN cache_write_tokens INTEGER",
        "ALTER TABLE sessions ADD COLUMN reasoning_tokens INTEGER",
    ];
    for sql in columns {
        match conn.execute(sql, []) {
            Ok(_) => {}
            Err(e) if e.to_string().contains("duplicate column name") => {}
            Err(e) => return Err(e.into()),
        }
    }
    conn.execute_batch("PRAGMA user_version = 3")?;
    Ok(())
}

/// Migrate from v3 to v4: add file fingerprint storage for incremental indexing.
fn apply_v4_migration(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS file_fingerprints (
            file_path TEXT PRIMARY KEY,
            mtime_ns INTEGER NOT NULL,
            size INTEGER NOT NULL
        )",
        [],
    )?;

    conn.execute_batch("PRAGMA user_version = 4")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn fresh_db_initializes_to_v4() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);

        let table_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='file_fingerprints'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(table_exists, 1);
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
        assert_eq!(version, 4);

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
    fn v2_to_v3_migration_adds_token_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 2").unwrap();
        conn.execute(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY, tool TEXT NOT NULL, project_path TEXT,
                start_time INTEGER NOT NULL, message_count INTEGER NOT NULL,
                file_path TEXT NOT NULL, last_updated INTEGER NOT NULL,
                first_prompt TEXT, parent_session_id TEXT,
                is_subagent INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE messages USING fts5(
                session_id UNINDEXED, message_index UNINDEXED, role UNINDEXED,
                content, timestamp UNINDEXED, model UNINDEXED
            )",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, tool, start_time, message_count, file_path, last_updated)
             VALUES ('s1', 'claude_code', 100, 5, '/tmp/s.jsonl', 200)",
            [],
        )
        .unwrap();

        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);

        conn.execute(
            "UPDATE sessions SET input_tokens = 1000, output_tokens = 500 WHERE id = 's1'",
            [],
        )
        .unwrap();
        let (input, output): (Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT input_tokens, output_tokens FROM sessions WHERE id = 's1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(input, Some(1000));
        assert_eq!(output, Some(500));
    }

    #[test]
    fn v4_migration_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();
        initialize_database(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);
    }

    #[test]
    fn v3_to_v4_migration_creates_file_fingerprints_table() {
        let conn = Connection::open_in_memory().unwrap();

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
                is_subagent INTEGER NOT NULL DEFAULT 0,
                input_tokens INTEGER,
                output_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_write_tokens INTEGER,
                reasoning_tokens INTEGER
            );
            PRAGMA user_version = 3;
            ",
        )
        .unwrap();

        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);

        let table_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='file_fingerprints'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(table_exists, 1);
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
