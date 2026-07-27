use anyhow::Result;
use rusqlite::Connection;

#[cfg(test)]
const CURRENT_DB_VERSION: i64 = 16;

fn column_exists(conn: &Connection, table_name: &str, column_name: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
        [table_name, column_name],
        |row| row.get::<_, i64>(0),
    )? > 0)
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |row| row.get::<_, i64>(0),
    )? > 0)
}

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
///   5 – clear file_fingerprints to force re-index after parser changes
///       (strip_command_tags in normalize_prompt)
///   6 – add projects table + sessions.project_id and clear file_fingerprints
///       to backfill canonical project IDs during re-index
///   7 – sessions gains activity counts (edit_count, read_count, command_count)
///       and ending_status; clear file_fingerprints to backfill during re-index
///   8 – sessions gains nullable pinned_at metadata (no fingerprint clear)
///   9 – reasoning_attachments side table and clear file_fingerprints
///   10 – clear file_fingerprints to rebuild transcripts after parser changes
///   11 – subagents gains nullable agent_id and clear file_fingerprints
///   12 – add session-list ordering indexes for faster startup/filter reloads
///   13 – replace FTS5-virtual `messages` with a b-tree source table backed
///        by an FTS5 external-content `messages_fts` index; clear
///        file_fingerprints to force reindexing from JSONL
///   14 – clear file_fingerprints to re-index after Mistral Vibe subagent
///        support: parents must be re-parsed to emit subagent rows linking the
///        child sessions now indexed from `<session>/agents/`; add an index on
///        sessions.file_path for efficient Vibe subtree pruning
///   15 – replace the top-level activity index with three deterministic sort
///        indexes (last_updated, start_time, message_count) to enable
///        deterministic session-list reading without temporary sort tables
///   16 – subagents gains a nullable agent_name for Claude Code teammate
///        linkage (v2.1.216+ dropped the `agentId:` token); clears
///        file_fingerprints so parents are re-parsed and the column is
///        populated
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
    if version < 5 {
        apply_v5_migration(conn)?;
    }
    if version < 6 {
        apply_v6_migration(conn)?;
    }
    if version < 7 {
        apply_v7_migration(conn)?;
    }
    if version < 8 {
        apply_v8_migration(conn)?;
    }
    if version < 9 {
        apply_v9_migration(conn)?;
    }
    if version < 10 {
        apply_v10_migration(conn)?;
    }
    if version < 11 {
        apply_v11_migration(conn)?;
    }
    if version < 12 {
        apply_v12_migration(conn)?;
    }
    if version < 13 {
        apply_v13_migration(conn)?;
    }
    if version < 14 {
        apply_v14_migration(conn)?;
    }
    if version < 15 {
        apply_v15_migration(conn)?;
    }
    if version < 16 {
        apply_v16_migration(conn)?;
    }

    Ok(())
}

/// Migrate from unversioned (v0) to v1.
///
/// Handles two cases:
/// - Fresh database (no `sessions` table yet): creates all tables outright.
/// - Pre-v1 database (existing `sessions` table missing the two new columns):
///   uses ALTER TABLE to add only missing columns, so the function is safe to
///   call even if a partial migration was interrupted.
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
        // Pre-v1 DB: add only the missing columns to the existing sessions table.
        for (column_name, column_def) in &[
            ("parent_session_id", "TEXT"),
            ("is_subagent", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !column_exists(conn, "sessions", column_name)? {
                conn.execute(
                    &format!("ALTER TABLE sessions ADD COLUMN {column_name} {column_def}"),
                    [],
                )?;
            }
        }
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
/// Uses individual ALTER TABLE statements for columns that are not already
/// present, so the migration stays idempotent after partial runs.
fn apply_v3_migration(conn: &Connection) -> Result<()> {
    for (column_name, column_def) in &[
        ("input_tokens", "INTEGER"),
        ("output_tokens", "INTEGER"),
        ("cache_read_tokens", "INTEGER"),
        ("cache_write_tokens", "INTEGER"),
        ("reasoning_tokens", "INTEGER"),
    ] {
        if !column_exists(conn, "sessions", column_name)? {
            conn.execute(
                &format!("ALTER TABLE sessions ADD COLUMN {column_name} {column_def}"),
                [],
            )?;
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

/// Migrate from v4 to v5.
///
/// Clears `file_fingerprints` so the next incremental index becomes a full
/// re-index.  This is needed because `normalize_prompt` now calls
/// `strip_command_tags`, and previously cached `first_prompt` values may
/// contain raw command-tag markup.
fn apply_v5_migration(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM file_fingerprints", [])?;
    conn.execute_batch("PRAGMA user_version = 5")?;
    Ok(())
}

/// Migrate from v5 to v6.
///
/// Adds canonical project storage and links sessions to projects via
/// `sessions.project_id`. Clears `file_fingerprints` to force re-index so
/// existing sessions can be backfilled with canonical project IDs.
fn apply_v6_migration(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS projects (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_projects_name ON projects(name)",
        [],
    )?;

    if !column_exists(conn, "sessions", "project_id")? {
        conn.execute(
            "ALTER TABLE sessions ADD COLUMN project_id INTEGER REFERENCES projects(id)",
            [],
        )?;
    }

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_project_id ON sessions(project_id)",
        [],
    )?;

    conn.execute("DELETE FROM file_fingerprints", [])?;
    conn.execute_batch("PRAGMA user_version = 6")?;
    Ok(())
}

/// Migrate from v6 to v7.
///
/// Adds denormalized activity counts and ending status to sessions.
/// Clears `file_fingerprints` to force re-index so existing sessions are
/// backfilled with activity data.
fn apply_v7_migration(conn: &Connection) -> Result<()> {
    for (col_name, col_def) in &[
        ("edit_count", "INTEGER DEFAULT 0"),
        ("read_count", "INTEGER DEFAULT 0"),
        ("command_count", "INTEGER DEFAULT 0"),
        ("ending_status", "TEXT DEFAULT 'unknown'"),
    ] {
        if !column_exists(conn, "sessions", col_name)? {
            conn.execute(
                &format!("ALTER TABLE sessions ADD COLUMN {col_name} {col_def}"),
                [],
            )?;
        }
    }

    conn.execute("DELETE FROM file_fingerprints", [])?;
    conn.execute_batch("PRAGMA user_version = 7")?;
    Ok(())
}

/// Migrate from v7 to v8.
///
/// Adds nullable `pinned_at` session metadata used for user-controlled pinning.
/// Does not clear fingerprints because this column is user state, not parser-derived.
fn apply_v8_migration(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "sessions", "pinned_at")? {
        conn.execute(
            "ALTER TABLE sessions ADD COLUMN pinned_at INTEGER DEFAULT NULL",
            [],
        )?;
    }

    conn.execute_batch("PRAGMA user_version = 8")?;
    Ok(())
}

/// Migrate from v8 to v9.
///
/// Adds transcript-item-level reasoning attachments and clears fingerprints so
/// sessions are reindexed with reasoning extraction enabled.
fn apply_v9_migration(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS reasoning_attachments (
            session_id TEXT NOT NULL,
            transcript_item_index INTEGER NOT NULL,
            visible_text TEXT,
            summary_text TEXT,
            has_encrypted_content INTEGER NOT NULL DEFAULT 0,
            source_model TEXT,
            source_timestamp INTEGER,
            PRIMARY KEY (session_id, transcript_item_index)
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_reasoning_attachments_session
         ON reasoning_attachments(session_id)",
        [],
    )?;

    conn.execute("DELETE FROM file_fingerprints", [])?;
    conn.execute_batch("PRAGMA user_version = 9")?;
    Ok(())
}

/// Migrate from v9 to v10.
///
/// Clears `file_fingerprints` so the next incremental index rebuilds session
/// transcript rows and counts after parser/output-shape changes.
fn apply_v10_migration(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM file_fingerprints", [])?;
    conn.execute_batch("PRAGMA user_version = 10")?;
    Ok(())
}

/// Migrate from v10 to v11.
///
/// Adds nullable durable `agent_id` to subagents and an index on
/// `(session_id, agent_id)` for fast lookups. Clears `file_fingerprints` to
/// force re-index so agent IDs are backfilled into persisted subagent rows.
fn apply_v11_migration(conn: &Connection) -> Result<()> {
    let subagents_exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='subagents'",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;

    if subagents_exists {
        if !column_exists(conn, "subagents", "agent_id")? {
            conn.execute("ALTER TABLE subagents ADD COLUMN agent_id TEXT", [])?;
        }

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_subagents_agent ON subagents(session_id, agent_id)",
            [],
        )?;
    }

    let file_fingerprints_exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='file_fingerprints'",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;

    if file_fingerprints_exists {
        conn.execute("DELETE FROM file_fingerprints", [])?;
    }

    conn.execute_batch("PRAGMA user_version = 11")?;
    Ok(())
}

/// Migrate from v11 to v12.
///
/// Adds ordering indexes used by the session list queries. These avoid scanning
/// all sessions and building temporary sort tables on every reload.
fn apply_v12_migration(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_top_level_last_updated
         ON sessions(is_subagent, last_updated DESC)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_project_last_updated
         ON sessions(is_subagent, project_id, last_updated DESC)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_tool_last_updated
         ON sessions(is_subagent, tool, last_updated DESC)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_pinned_last_updated
         ON sessions(is_subagent, pinned_at, last_updated DESC)",
        [],
    )?;

    conn.execute_batch("PRAGMA user_version = 12")?;
    Ok(())
}

/// Migrate from v12 to v13.
///
/// Replace the FTS5-virtual `messages` table inherited from v0 with a
/// b-tree source-of-truth `messages` table backed by an FTS5
/// external-content `messages_fts` index. The transcript read path no
/// longer needs runtime table routing or CAST(message_index AS INTEGER).
///
/// Existing message data is intentionally not preserved: clearing
/// `file_fingerprints` causes the indexer to repopulate from JSONL on the
/// next run. JSONL files are the authoritative source.
///
/// Dependent transcript tables (`transcript_items`, `tool_calls`,
/// `reasoning_attachments`, `subagents`) are wiped in the same step so the
/// transcript LEFT JOINs don't render empty rows during the reindex window.
/// `sessions` is preserved to keep user state (pinned_at, etc.).
fn apply_v13_migration(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "BEGIN IMMEDIATE;
         DROP TRIGGER IF EXISTS messages_ai;
         DROP TRIGGER IF EXISTS messages_ad;
         DROP TRIGGER IF EXISTS messages_au;
         DROP TABLE IF EXISTS messages_fts;
         DROP TABLE IF EXISTS messages;",
    )?;

    for table in [
        "transcript_items",
        "tool_calls",
        "reasoning_attachments",
        "subagents",
    ] {
        if table_exists(conn, table)? {
            conn.execute(&format!("DELETE FROM {table}"), [])?;
        }
    }

    conn.execute_batch(
        "CREATE TABLE messages (
             id            INTEGER PRIMARY KEY AUTOINCREMENT,
             session_id    TEXT NOT NULL,
             message_index INTEGER NOT NULL,
             role          TEXT NOT NULL,
             content       TEXT NOT NULL,
             timestamp     INTEGER NOT NULL,
             model         TEXT,
             UNIQUE(session_id, message_index)
         );

         CREATE VIRTUAL TABLE messages_fts USING fts5(
             content,
             content='messages',
             content_rowid='id'
         );

         CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
             INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
         END;
         CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
             INSERT INTO messages_fts(messages_fts, rowid, content)
                 VALUES('delete', old.id, old.content);
         END;
         CREATE TRIGGER messages_au AFTER UPDATE ON messages BEGIN
             INSERT INTO messages_fts(messages_fts, rowid, content)
                 VALUES('delete', old.id, old.content);
             INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
         END;

         DELETE FROM file_fingerprints;
         PRAGMA user_version = 13;
         COMMIT;",
    )?;

    Ok(())
}

/// Migrate from v13 to v14.
///
/// Clears `file_fingerprints` so the next incremental index re-parses every
/// session. This is required for Mistral Vibe subagent support: previously
/// unchanged parent sessions are skipped by the incremental indexer, so without
/// a fingerprint clear they would never emit the subagent rows that link the
/// child sessions now discovered under `<session>/agents/`. Adds an index on
/// `sessions.file_path` to serve prefix-range lookups used when pruning deleted
/// Mistral Vibe subagent subtrees.
fn apply_v14_migration(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_file_path ON sessions(file_path)",
        [],
    )?;
    conn.execute("DELETE FROM file_fingerprints", [])?;
    conn.execute_batch("PRAGMA user_version = 14")?;
    Ok(())
}

/// Migrate from v14 to v15.
///
/// Replace the top-level activity index and add indexes for deterministic
/// session-list reading orders. Filter-specific activity indexes remain intact.
fn apply_v15_migration(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_sessions_top_level_last_updated;
         CREATE INDEX IF NOT EXISTS idx_sessions_top_level_last_updated_id
             ON sessions(is_subagent, last_updated DESC, id DESC);
         CREATE INDEX IF NOT EXISTS idx_sessions_top_level_start_time_id
             ON sessions(is_subagent, start_time DESC, id DESC);
         CREATE INDEX IF NOT EXISTS idx_sessions_top_level_message_count_id
             ON sessions(is_subagent, message_count DESC, id DESC);
         PRAGMA user_version = 15;",
    )?;
    Ok(())
}

/// Migrate from v15 to v16.
///
/// Adds a nullable `agent_name` to `subagents`. Claude Code v2.1.216+ spawns
/// subagents as background teammates and no longer emits the `agentId:` token
/// the linkage relied on; the teammate `name` is the only value shared by the
/// parent transcript and the nested child file.
///
/// Clears `file_fingerprints`: teammate sessions already indexed hold
/// `subagents` rows with a null `agent_id`, and adding a column does not
/// repair them — the parents must be re-parsed.
fn apply_v16_migration(conn: &Connection) -> Result<()> {
    if table_exists(conn, "subagents")? {
        if !column_exists(conn, "subagents", "agent_name")? {
            conn.execute("ALTER TABLE subagents ADD COLUMN agent_name TEXT", [])?;
        }

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_subagents_agent_name
             ON subagents(session_id, agent_name)",
            [],
        )?;
    }

    if table_exists(conn, "file_fingerprints")? {
        conn.execute("DELETE FROM file_fingerprints", [])?;
    }

    conn.execute_batch("PRAGMA user_version = 16")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn index_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            > 0
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            > 0
    }

    fn trigger_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            > 0
    }

    fn table_sql(conn: &Connection, name: &str) -> String {
        conn.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name = ?1",
            [name],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    }

    fn index_columns(conn: &Connection, name: &str) -> Vec<String> {
        let mut stmt = conn.prepare(&format!("PRAGMA index_info({name})")).unwrap();
        stmt.query_map([], |row| row.get::<_, String>(2))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn fresh_db_initializes_to_latest() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);

        let pinned_column_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('sessions') WHERE name='pinned_at'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pinned_column_exists, 1);

        let file_fingerprints_table_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='file_fingerprints'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(file_fingerprints_table_exists, 1);

        let reasoning_table_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='reasoning_attachments'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(reasoning_table_exists, 1);

        let has_encrypted_content_column: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('reasoning_attachments') WHERE name='has_encrypted_content'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_encrypted_content_column, 1);

        let encrypted_content_column: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('reasoning_attachments') WHERE name='encrypted_content'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(encrypted_content_column, 0);

        assert!(table_exists(&conn, "messages"));
        assert!(table_exists(&conn, "messages_fts"));
        assert!(!table_exists(&conn, "message_cache"));
        assert!(trigger_exists(&conn, "messages_ai"));
        assert!(trigger_exists(&conn, "messages_ad"));
        assert!(trigger_exists(&conn, "messages_au"));
    }

    #[test]
    fn migration_creates_session_list_ordering_indexes() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();

        assert!(!index_exists(&conn, "idx_sessions_top_level_last_updated"));
        for (name, columns) in [
            (
                "idx_sessions_top_level_last_updated_id",
                vec!["is_subagent", "last_updated", "id"],
            ),
            (
                "idx_sessions_top_level_start_time_id",
                vec!["is_subagent", "start_time", "id"],
            ),
            (
                "idx_sessions_top_level_message_count_id",
                vec!["is_subagent", "message_count", "id"],
            ),
        ] {
            assert!(index_exists(&conn, name));
            assert_eq!(index_columns(&conn, name), columns);
        }
        assert!(index_exists(&conn, "idx_sessions_project_last_updated"));
        assert!(index_exists(&conn, "idx_sessions_tool_last_updated"));
        assert!(index_exists(&conn, "idx_sessions_pinned_last_updated"));
    }

    #[test]
    fn v11_to_v13_migration_preserves_sessions_clears_fingerprints() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();
        conn.execute_batch(
            "DROP INDEX idx_sessions_top_level_last_updated_id;
             DROP INDEX idx_sessions_top_level_start_time_id;
             DROP INDEX idx_sessions_top_level_message_count_id;
             DROP INDEX idx_sessions_project_last_updated;
             DROP INDEX idx_sessions_tool_last_updated;
             DROP INDEX idx_sessions_pinned_last_updated;
             PRAGMA user_version = 11;",
        )
        .unwrap();

        conn.execute(
            "INSERT INTO sessions (id, tool, start_time, message_count, file_path, last_updated)
             VALUES ('kept', 'opencode', 1, 2, '/tmp/kept.jsonl', 3)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO file_fingerprints (file_path, mtime_ns, size)
             VALUES ('/tmp/kept.jsonl', 4, 5)",
            [],
        )
        .unwrap();

        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);

        let session_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE id = 'kept'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(session_count, 1);

        let fingerprint_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM file_fingerprints WHERE file_path = '/tmp/kept.jsonl'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(fingerprint_count, 0);
    }

    #[test]
    fn v13_migration_creates_messages_and_fts_index() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);

        assert!(table_exists(&conn, "messages"));
        assert!(table_exists(&conn, "messages_fts"));
        assert!(!table_exists(&conn, "message_cache"));

        let id_type: String = conn
            .query_row(
                "SELECT type FROM pragma_table_info('messages') WHERE name = 'id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let id_pk: i64 = conn
            .query_row(
                "SELECT pk FROM pragma_table_info('messages') WHERE name = 'id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let message_index_type: String = conn
            .query_row(
                "SELECT type FROM pragma_table_info('messages') WHERE name = 'message_index'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let timestamp_type: String = conn
            .query_row(
                "SELECT type FROM pragma_table_info('messages') WHERE name = 'timestamp'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(id_type.to_uppercase(), "INTEGER");
        assert_eq!(id_pk, 1);
        assert_eq!(message_index_type.to_uppercase(), "INTEGER");
        assert_eq!(timestamp_type.to_uppercase(), "INTEGER");

        let unique_index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_index_list('messages') WHERE origin = 'u'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unique_index_count, 1);

        let fts_sql = table_sql(&conn, "messages_fts");
        assert!(fts_sql.contains("USING fts5"));
        assert!(fts_sql.contains("content='messages'"));
        assert!(fts_sql.contains("content_rowid='id'"));

        assert!(trigger_exists(&conn, "messages_ai"));
        assert!(trigger_exists(&conn, "messages_ad"));
        assert!(trigger_exists(&conn, "messages_au"));
    }

    #[test]
    fn v11_to_v13_clears_fingerprints_and_drops_old_messages() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();

        conn.execute_batch(
            "DROP TRIGGER IF EXISTS messages_ai;
             DROP TRIGGER IF EXISTS messages_ad;
             DROP TRIGGER IF EXISTS messages_au;
             DROP TABLE IF EXISTS messages_fts;
             DROP TABLE IF EXISTS messages;
             DROP TABLE IF EXISTS message_cache;
             CREATE VIRTUAL TABLE messages USING fts5(
                 session_id UNINDEXED,
                 message_index UNINDEXED,
                 role UNINDEXED,
                 content,
                 timestamp UNINDEXED,
                 model UNINDEXED
             );
             PRAGMA user_version = 11;",
        )
        .unwrap();

        conn.execute(
            "INSERT INTO messages
             (session_id, message_index, role, content, timestamp, model)
             VALUES ('s1', '42', 'assistant', 'old body', '1234', 'gpt-fixture')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO file_fingerprints (file_path, mtime_ns, size)
             VALUES ('/tmp/old.jsonl', 10, 20)",
            [],
        )
        .unwrap();

        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);
        assert!(table_exists(&conn, "messages"));
        assert!(table_exists(&conn, "messages_fts"));
        assert!(!table_exists(&conn, "message_cache"));

        let message_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        let fingerprint_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM file_fingerprints", [], |row| {
                row.get(0)
            })
            .unwrap();

        assert_eq!(message_count, 0);
        assert_eq!(fingerprint_count, 0);
    }

    #[test]
    fn v13_to_v14_migration_clears_fingerprints_and_indexes_file_path() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();

        conn.execute_batch(
            "DROP INDEX idx_sessions_file_path;
             PRAGMA user_version = 13;",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO file_fingerprints (file_path, mtime_ns, size)
             VALUES ('/tmp/old.jsonl', 10, 20)",
            [],
        )
        .unwrap();

        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);

        let fingerprint_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM file_fingerprints", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(fingerprint_count, 0);
        assert!(index_exists(&conn, "idx_sessions_file_path"));
        assert_eq!(
            index_columns(&conn, "idx_sessions_file_path"),
            vec!["file_path"]
        );
    }

    #[test]
    fn messages_fts_stays_in_sync_via_triggers() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();

        conn.execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                "s1",
                0_i64,
                "user",
                "old searchable token",
                100_i64,
                Option::<String>::None
            ],
        )
        .unwrap();

        let old_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'old'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_count, 1);

        conn.execute(
            "UPDATE messages SET content = 'new searchable token' WHERE session_id = 's1' AND message_index = 0",
            [],
        )
        .unwrap();

        let old_count_after_update: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'old'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let new_count_after_update: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'new'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_count_after_update, 0);
        assert_eq!(new_count_after_update, 1);

        conn.execute("DELETE FROM messages WHERE session_id = 's1'", [])
            .unwrap();

        let new_count_after_delete: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'new'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(new_count_after_delete, 0);
    }

    #[test]
    fn v8_to_v9_migration_creates_reasoning_attachments_and_clears_fingerprints() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();
        conn.execute_batch("PRAGMA user_version = 8").unwrap();

        conn.execute(
            "INSERT INTO file_fingerprints (file_path, mtime_ns, size) VALUES ('fixture.jsonl', 1, 1)",
            [],
        )
        .unwrap();

        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);

        let reasoning_table_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='reasoning_attachments'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(reasoning_table_exists, 1);

        let fingerprint_count: i64 = conn
            .query_row("SELECT count(*) FROM file_fingerprints", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fingerprint_count, 0);
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
        assert_eq!(version, CURRENT_DB_VERSION);

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
        assert_eq!(version, CURRENT_DB_VERSION);

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
        assert_eq!(version, CURRENT_DB_VERSION);
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
        assert_eq!(version, CURRENT_DB_VERSION);

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
    fn v4_to_v5_migration_clears_file_fingerprints() {
        let conn = Connection::open_in_memory().unwrap();

        // Set up a v4 database with a fingerprint row.
        initialize_database(&conn).unwrap();
        conn.execute_batch("PRAGMA user_version = 4").unwrap();
        conn.execute(
            "INSERT INTO file_fingerprints (file_path, mtime_ns, size) VALUES ('a.jsonl', 1, 100)",
            [],
        )
        .unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM file_fingerprints", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);

        // Re-run migrations — v5 should clear the table.
        initialize_database(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);
        let count: i64 = conn
            .query_row("SELECT count(*) FROM file_fingerprints", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn v5_to_v6_migration_uses_authentic_fixture_and_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();

        // Build an authentic v5 schema fixture.
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
            CREATE VIRTUAL TABLE messages USING fts5(
                session_id UNINDEXED,
                message_index UNINDEXED,
                role UNINDEXED,
                content,
                timestamp UNINDEXED,
                model UNINDEXED
            );
            CREATE TABLE transcript_items (
                session_id TEXT NOT NULL,
                item_index INTEGER NOT NULL,
                kind TEXT NOT NULL,
                message_index INTEGER,
                tool_call_id TEXT,
                subagent_id TEXT,
                PRIMARY KEY (session_id, item_index)
            );
            CREATE TABLE tool_calls (
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
            CREATE TABLE subagents (
                id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                title TEXT NOT NULL,
                prompt TEXT,
                result_summary TEXT,
                child_session_id TEXT,
                parser_ref TEXT,
                PRIMARY KEY (session_id, id)
            );
            CREATE TABLE file_fingerprints (
                file_path TEXT PRIMARY KEY,
                mtime_ns INTEGER NOT NULL,
                size INTEGER NOT NULL
            );
            PRAGMA user_version = 5;
            ",
        )
        .unwrap();

        conn.execute(
            "INSERT INTO file_fingerprints (file_path, mtime_ns, size) VALUES ('x.jsonl', 1, 1)",
            [],
        )
        .unwrap();

        let projects_exists_before: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='projects'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(projects_exists_before, 0);

        let project_id_columns_before: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('sessions') WHERE name = 'project_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(project_id_columns_before, 0);

        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);

        let projects_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='projects'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(projects_exists, 1);

        let sessions_columns: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(sessions)").unwrap();
            stmt.query_map([], |row| row.get(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert!(sessions_columns.iter().any(|name| name == "project_id"));

        let project_id_columns_after: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('sessions') WHERE name = 'project_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(project_id_columns_after, 1);

        let fingerprint_count: i64 = conn
            .query_row("SELECT count(*) FROM file_fingerprints", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fingerprint_count, 0);

        // Re-running initialize_database should keep schema stable at the latest version.
        initialize_database(&conn).unwrap();

        let version_after_second_run: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version_after_second_run, CURRENT_DB_VERSION);

        let pinned_column_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('sessions') WHERE name='pinned_at'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pinned_column_count, 1);

        let projects_exists_after_second_run: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='projects'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(projects_exists_after_second_run, 1);

        let project_id_columns_after_second_run: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('sessions') WHERE name = 'project_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(project_id_columns_after_second_run, 1);

        let fingerprint_count_after_second_run: i64 = conn
            .query_row("SELECT count(*) FROM file_fingerprints", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fingerprint_count_after_second_run, 0);
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

    #[test]
    fn v7_migration_adds_activity_and_ending_columns() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (id, tool, start_time, message_count, file_path, last_updated,
             is_subagent, edit_count, read_count, command_count, ending_status)
             VALUES ('test', 'claude_code', 0, 0, '/tmp/f', 0, 0, 5, 3, 2, 'clean')",
            [],
        )
        .unwrap();

        let (edit, read, cmd, ending): (i64, i64, i64, String) = conn
            .query_row(
                "SELECT edit_count, read_count, command_count, ending_status FROM sessions WHERE id = 'test'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();

        assert_eq!(edit, 5);
        assert_eq!(read, 3);
        assert_eq!(cmd, 2);
        assert_eq!(ending, "clean");
    }

    #[test]
    fn v9_migration_is_idempotent_and_clears_file_fingerprints() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();

        conn.execute_batch("PRAGMA user_version = 7").unwrap();
        conn.execute(
            "INSERT INTO file_fingerprints (file_path, mtime_ns, size) VALUES ('fixture.jsonl', 1, 1)",
            [],
        )
        .unwrap();

        initialize_database(&conn).unwrap();
        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);

        let fingerprint_count: i64 = conn
            .query_row("SELECT count(*) FROM file_fingerprints", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(fingerprint_count, 0);
    }

    #[test]
    fn v9_to_v10_migration_clears_file_fingerprints() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();

        conn.execute_batch("PRAGMA user_version = 9").unwrap();
        conn.execute(
            "INSERT INTO file_fingerprints (file_path, mtime_ns, size) VALUES ('fixture.jsonl', 1, 1)",
            [],
        )
        .unwrap();

        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);

        let fingerprint_count: i64 = conn
            .query_row("SELECT count(*) FROM file_fingerprints", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(fingerprint_count, 0);
    }

    #[test]
    fn v10_to_v11_migration_adds_subagent_agent_id_column() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();

        conn.execute_batch("PRAGMA user_version = 10").unwrap();
        conn.execute(
            "INSERT INTO file_fingerprints (file_path, mtime_ns, size) VALUES ('fixture.jsonl', 1, 1)",
            [],
        )
        .unwrap();

        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);

        let agent_id_column_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('subagents') WHERE name='agent_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(agent_id_column_count, 1);

        let agent_index_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='idx_subagents_agent'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(agent_index_count, 1);

        let fingerprint_count: i64 = conn
            .query_row("SELECT count(*) FROM file_fingerprints", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(fingerprint_count, 0);
    }

    #[test]
    fn v14_to_v15_replaces_top_level_sort_indexes() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();
        conn.execute_batch(
            "DROP INDEX idx_sessions_top_level_last_updated_id;
             DROP INDEX idx_sessions_top_level_start_time_id;
             DROP INDEX idx_sessions_top_level_message_count_id;
             CREATE INDEX idx_sessions_top_level_last_updated
                 ON sessions(is_subagent, last_updated DESC);
             PRAGMA user_version = 14;",
        )
        .unwrap();

        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);
        assert!(!index_exists(&conn, "idx_sessions_top_level_last_updated"));
        assert!(index_exists(
            &conn,
            "idx_sessions_top_level_last_updated_id"
        ));
        assert!(index_exists(&conn, "idx_sessions_top_level_start_time_id"));
        assert!(index_exists(
            &conn,
            "idx_sessions_top_level_message_count_id"
        ));
    }

    #[test]
    fn v15_to_v16_adds_subagent_agent_name_and_clears_fingerprints() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();

        conn.execute_batch("PRAGMA user_version = 15").unwrap();
        conn.execute(
            "INSERT INTO file_fingerprints (file_path, mtime_ns, size) VALUES ('fixture.jsonl', 1, 1)",
            [],
        )
        .unwrap();

        initialize_database(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_DB_VERSION);

        let agent_name_column_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('subagents') WHERE name='agent_name'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(agent_name_column_count, 1);

        assert!(index_exists(&conn, "idx_subagents_agent_name"));

        let fingerprint_count: i64 = conn
            .query_row("SELECT count(*) FROM file_fingerprints", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(fingerprint_count, 0);
    }

    #[test]
    fn unfiltered_named_orders_do_not_build_temporary_sort_tables() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();

        for order_by in [
            "last_updated DESC, id DESC",
            "start_time ASC, id ASC",
            "start_time DESC, id DESC",
            "message_count DESC, id DESC",
        ] {
            let sql = format!(
                "EXPLAIN QUERY PLAN SELECT id FROM sessions
                 WHERE is_subagent = 0 ORDER BY {order_by}"
            );
            let mut stmt = conn.prepare(&sql).unwrap();
            let details = stmt
                .query_map([], |row| row.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(
                details
                    .iter()
                    .all(|detail| !detail.contains("USE TEMP B-TREE FOR ORDER BY")),
                "{order_by} used a temporary sort: {details:?}"
            );
        }
    }
}
