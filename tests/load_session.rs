use rusqlite::Connection;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sessions_chronicle::database::load_session;
use sessions_chronicle::database::schema::initialize_database;
use sessions_chronicle::database::{
    load_session_by_id_for_filter, load_sessions_for_filter, search_sessions_for_filter,
};
use sessions_chronicle::models::{
    AiAssistant, DateFilter, ProjectFilter, Role, SortOrder, SourceKind,
};

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
        self.connection
            .execute(
                "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
                rusqlite::params![42_i64, "/projects/test", "test"],
            )
            .expect("Failed to insert project");

        // Insert a session
        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated, first_prompt)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    "test-session",
                    "claude_code",
                    Some("/projects/test"),
                    Some(42_i64),
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
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "test-session",
                    2_i64,
                    "toolcall",
                    "Calling read_file",
                    1200_i64,
                    Option::<String>::None
                ],
            )
            .expect("Failed to insert message 2");

        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "test-session",
                    0_i64,
                    "user",
                    "Hello, please help me",
                    1000_i64,
                    Option::<String>::None
                ],
            )
            .expect("Failed to insert message 0");

        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "test-session",
                    3_i64,
                    "toolresult",
                    "File contents here",
                    1300_i64,
                    Option::<String>::None
                ],
            )
            .expect("Failed to insert message 3");

        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "test-session",
                    1_i64,
                    "assistant",
                    "I'll help you with that",
                    1100_i64,
                    Option::<String>::None
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
    assert_eq!(session.project_id, Some(42));
    assert_eq!(session.message_count, 4);
    assert_eq!(
        session.first_prompt.as_deref(),
        Some("Help me refactor this code")
    );
}

#[test]
fn load_session_round_trips_kimi_code_storage_value() {
    let db = TempDatabase::new();
    db.connection
        .execute(
            "INSERT INTO sessions
             (id, tool, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES ('session_kimi', 'kimi_code', 1, 1, '/tmp/kimi', 2, 0)",
            [],
        )
        .unwrap();

    let session = load_session(&db.path, "session_kimi")
        .unwrap()
        .expect("Kimi session should load");

    assert_eq!(
        session.tool,
        sessions_chronicle::models::AiAssistant::KimiCode
    );
}

#[test]
fn load_session_maps_pinned_at_to_utc_datetime() {
    let db = TempDatabase::new();

    db.connection
        .execute(
            "INSERT INTO sessions (
                id, tool, project_path, project_id, start_time, message_count,
                file_path, last_updated, first_prompt, pinned_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                "pinned-session",
                "claude_code",
                Some("/projects/test"),
                Option::<i64>::None,
                1000_i64,
                4_i64,
                "/tmp/test-session.jsonl",
                2000_i64,
                Some("Help me refactor this code"),
                1_717_171_717_i64,
            ],
        )
        .expect("Failed to insert pinned session");

    let session = load_session(&db.path, "pinned-session")
        .expect("Failed to load session")
        .expect("Session should exist");

    assert_eq!(session.pinned_at.unwrap().timestamp(), 1_717_171_717);
}

#[test]
fn session_queries_return_persisted_source_evidence() {
    let db = TempDatabase::new();
    db.connection
        .execute(
            "INSERT INTO sessions (
                id, tool, start_time, message_count, file_path, last_updated,
                source_missing, source_missing_detected_at, source_last_seen_at,
                source_kind, source_mtime_ns, source_size, source_scope
             ) VALUES (
                'evidence-session', 'claude_code', 1, 1, '/tmp/evidence.jsonl', 2,
                1, 1_700_000_000, 1_700_000_100,
                'transcript_file', 123_456_789, 456, '/sessions/claude'
             )",
            [],
        )
        .expect("Failed to insert source evidence");
    db.connection
        .execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp)
             VALUES ('evidence-session', 0, 'user', 'source evidence query', 1)",
            [],
        )
        .unwrap();

    let assert_evidence = |session: sessions_chronicle::models::Session| {
        assert!(session.source.missing);
        assert_eq!(
            session.source.missing_detected_at.unwrap().timestamp(),
            1_700_000_000
        );
        assert_eq!(
            session.source.last_seen_at.unwrap().timestamp(),
            1_700_000_100
        );
        assert_eq!(session.source.kind, Some(SourceKind::TranscriptFile));
        assert_eq!(session.source.mtime_ns, Some(123_456_789));
        assert_eq!(session.source.size, Some(456));
        assert_eq!(session.source.scope.as_deref(), Some("/sessions/claude"));
    };

    assert_evidence(load_session(&db.path, "evidence-session").unwrap().unwrap());
    assert_evidence(
        load_sessions_for_filter(
            &db.path,
            AiAssistant::ALL,
            &ProjectFilter::AllSessions,
            &DateFilter::AnyTime,
            SortOrder::default(),
        )
        .unwrap()
        .pop()
        .unwrap(),
    );
    assert_evidence(
        search_sessions_for_filter(
            &db.path,
            AiAssistant::ALL,
            &ProjectFilter::AllSessions,
            "source",
            &DateFilter::AnyTime,
            None,
        )
        .unwrap()
        .pop()
        .unwrap(),
    );
    assert_evidence(
        load_session_by_id_for_filter(
            &db.path,
            AiAssistant::ALL,
            &ProjectFilter::AllSessions,
            "evidence-session",
            &DateFilter::AnyTime,
        )
        .unwrap()
        .pop()
        .unwrap(),
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
