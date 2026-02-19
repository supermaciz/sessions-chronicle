use anyhow::Result;
use rusqlite::Connection;

/// Initialize (or migrate) the database to the current schema version.
///
/// Versioning uses `PRAGMA user_version`:
///   0 – unversioned (pre-phase-1) or brand-new database
///   1 – phase-1 schema: sessions gains parent_session_id + is_subagent;
///       transcript_items, tool_calls, subagents tables added
pub fn initialize_database(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if version < 1 {
        apply_v1_migration(conn)?;
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
