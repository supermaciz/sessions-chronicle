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
            first_prompt TEXT
        )",
        [],
    )?;

    let has_first_prompt = {
        let mut stmt = conn.prepare("PRAGMA table_info(sessions)")?;
        let column_names = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut has_column = false;
        for column_name in column_names {
            if column_name? == "first_prompt" {
                has_column = true;
                break;
            }
        }
        has_column
    };

    if !has_first_prompt {
        conn.execute("ALTER TABLE sessions ADD COLUMN first_prompt TEXT", [])?;
    }

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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_database_adds_first_prompt_column_for_legacy_schema() {
        let conn = Connection::open_in_memory().expect("in-memory db should open");

        conn.execute(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                tool TEXT NOT NULL,
                project_path TEXT,
                start_time INTEGER NOT NULL,
                message_count INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                last_updated INTEGER NOT NULL
            )",
            [],
        )
        .expect("legacy schema should be created");

        initialize_database(&conn).expect("database initialization should succeed");

        let mut stmt = conn
            .prepare("PRAGMA table_info(sessions)")
            .expect("table_info pragma should prepare");
        let column_names = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .expect("table_info should query")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("table_info should collect");

        assert!(
            column_names.iter().any(|name| name == "first_prompt"),
            "expected first_prompt column in legacy schema migration"
        );
    }
}
