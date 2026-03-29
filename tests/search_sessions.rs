use rusqlite::Connection;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sessions_chronicle::database::schema::initialize_database;
use sessions_chronicle::database::search_sessions_for_filter;
use sessions_chronicle::models::{AiAssistant, ProjectFilter};

struct TempDatabase {
    path: PathBuf,
    connection: Connection,
}

impl TempDatabase {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        path.push(format!(
            "sessions-chronicle-test-{}-{}.db",
            std::process::id(),
            nanos
        ));
        let connection = Connection::open(&path).expect("Failed to open temp database");
        initialize_database(&connection).expect("Failed to initialize database");

        Self { path, connection }
    }

    fn seed(&self) {
        self.connection
            .execute(
                "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
                rusqlite::params![1_i64, "/projects/alpha", "alpha"],
            )
            .expect("Failed to insert project alpha");

        self.connection
            .execute(
                "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
                rusqlite::params![2_i64, "/projects/beta", "beta"],
            )
            .expect("Failed to insert project beta");

        self.connection
            .execute(
                "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
                rusqlite::params![3_i64, "/projects/gamma", "gamma"],
            )
            .expect("Failed to insert project gamma");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "session-a",
                    "claude_code",
                    Some("/projects/alpha"),
                    Some(1_i64),
                    10_i64,
                    3_i64,
                    "/tmp/session-a.jsonl",
                    30_i64,
                ],
            )
            .expect("Failed to insert session A");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "session-b",
                    "opencode",
                    Some("/projects/beta"),
                    Some(2_i64),
                    20_i64,
                    2_i64,
                    "/tmp/session-b.jsonl",
                    40_i64,
                ],
            )
            .expect("Failed to insert session B");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "session-c",
                    "codex",
                    Some("/projects/gamma"),
                    Some(3_i64),
                    30_i64,
                    1_i64,
                    "/tmp/session-c.jsonl",
                    50_i64,
                ],
            )
            .expect("Failed to insert session C");

        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "session-a",
                    0_i64,
                    "user",
                    "alpha alpha alpha",
                    10_i64,
                    Option::<String>::None
                ],
            )
            .expect("Failed to insert message A1");

        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "session-b",
                    0_i64,
                    "assistant",
                    "alpha beta",
                    20_i64,
                    Option::<String>::None
                ],
            )
            .expect("Failed to insert message B1");

        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "session-c",
                    0_i64,
                    "assistant",
                    "gamma",
                    30_i64,
                    Option::<String>::None
                ],
            )
            .expect("Failed to insert message C1");
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn search_sessions_orders_by_relevance() {
    let db = TempDatabase::new();
    db.seed();

    let sessions = search_sessions_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode, AiAssistant::OpenCode],
        &ProjectFilter::AllSessions,
        "alpha",
    )
    .expect("Search failed");
    let ids: Vec<&str> = sessions.iter().map(|session| session.id.as_str()).collect();

    assert_eq!(ids, vec!["session-a", "session-b"]);
    assert_eq!(sessions[0].project_id, Some(1));
}

#[test]
fn search_sessions_respects_tool_filter() {
    let db = TempDatabase::new();
    db.seed();

    let sessions = search_sessions_for_filter(
        &db.path,
        &[AiAssistant::OpenCode],
        &ProjectFilter::AllSessions,
        "alpha",
    )
    .expect("Search failed");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "session-b");
    assert_eq!(sessions[0].project_id, Some(2));
}

#[test]
fn search_sessions_sanitizes_invalid_query() {
    let db = TempDatabase::new();
    db.seed();

    let sessions = search_sessions_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        "\"alpha",
    )
    .expect("Search failed");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "session-a");
    assert_eq!(sessions[0].project_id, Some(1));
}
