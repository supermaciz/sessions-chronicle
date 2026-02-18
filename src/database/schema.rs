use anyhow::Result;
use rusqlite::Connection;

pub fn initialize_database(conn: &Connection) -> Result<()> {
    // Create sessions table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS sessions (
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

    // Create indexes
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

    // Create FTS5 messages table
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

    // Create transcript_items table — ordered stream for detail rendering
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

    // Create tool_calls table
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

    // Create subagents table
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

    Ok(())
}
