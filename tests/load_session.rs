use rusqlite::Connection;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sessions_chronicle::database::load_session;
use sessions_chronicle::database::schema::initialize_database;
use sessions_chronicle::models::Role;

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

    fn seed_with_messages(&self) {
        // Insert a session
        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, first_prompt)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "test-session",
                    "claude_code",
                    Some("/projects/test"),
                    1000_i64,
                    4_i64,
                    "/tmp/test-session.jsonl",
                    2000_i64,
                    Some("Help me refactor this code"),
                ],
            )
            .expect("Failed to insert session");

        // Insert messages in non-sequential order to test ordering
        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    "test-session",
                    2_i64,
                    "toolcall",
                    "Calling read_file",
                    1200_i64
                ],
            )
            .expect("Failed to insert message 2");

        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    "test-session",
                    0_i64,
                    "user",
                    "Hello, please help me",
                    1000_i64
                ],
            )
            .expect("Failed to insert message 0");

        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    "test-session",
                    3_i64,
                    "toolresult",
                    "File contents here",
                    1300_i64
                ],
            )
            .expect("Failed to insert message 3");

        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    "test-session",
                    1_i64,
                    "assistant",
                    "I'll help you with that",
                    1100_i64
                ],
            )
            .expect("Failed to insert message 1");
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn load_session_returns_existing_session() {
    let db = TempDatabase::new();
    db.seed_with_messages();

    let session = load_session(&db.path, "test-session")
        .expect("Failed to load session")
        .expect("Session should exist");

    assert_eq!(session.id, "test-session");
    assert_eq!(session.project_path, Some("/projects/test".to_string()));
    assert_eq!(session.message_count, 4);
    assert_eq!(
        session.first_prompt.as_deref(),
        Some("Help me refactor this code")
    );
}

#[test]
fn load_session_returns_none_for_nonexistent() {
    let db = TempDatabase::new();
    db.seed_with_messages();

    let session = load_session(&db.path, "nonexistent").expect("Failed to load session");

    assert!(session.is_none());
}

#[test]
fn role_from_storage_parses_correctly() {
    assert_eq!(Role::from_storage("user"), Some(Role::User));
    assert_eq!(Role::from_storage("assistant"), Some(Role::Assistant));
    assert_eq!(Role::from_storage("toolcall"), Some(Role::ToolCall));
    assert_eq!(Role::from_storage("toolresult"), Some(Role::ToolResult));

    // Test tolerant aliases
    assert_eq!(Role::from_storage("tool_call"), Some(Role::ToolCall));
    assert_eq!(Role::from_storage("tool_result"), Some(Role::ToolResult));

    // Test case insensitivity
    assert_eq!(Role::from_storage("USER"), Some(Role::User));
    assert_eq!(Role::from_storage("Assistant"), Some(Role::Assistant));

    // Test invalid values
    assert_eq!(Role::from_storage("invalid"), None);
    assert_eq!(Role::from_storage(""), None);
}
