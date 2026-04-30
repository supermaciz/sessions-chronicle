use rusqlite::Connection;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sessions_chronicle::database::schema::initialize_database;
use sessions_chronicle::database::{
    find_session_match_positions, load_session_by_id_for_filter, search_sessions_for_filter,
};
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

fn seed_match_session(db: &TempDatabase, session_id: &str, tool: &str, last_updated: i64) {
    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                session_id,
                tool,
                Some(format!("/projects/{session_id}")),
                last_updated - 10,
                0_i64,
                format!("/tmp/{session_id}.jsonl"),
                last_updated,
            ],
        )
        .expect("Failed to insert match-position session");
}

fn insert_message_item(
    db: &TempDatabase,
    session_id: &str,
    message_index: i64,
    item_index: i64,
    content: &str,
) {
    db.connection
        .execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
             VALUES (?1, ?2, 'assistant', ?3, ?4, NULL)",
            rusqlite::params![session_id, message_index, content, message_index],
        )
        .expect("Failed to insert message item message");

    db.connection
        .execute(
            "INSERT INTO transcript_items (session_id, item_index, kind, message_index)
             VALUES (?1, ?2, 'message', ?3)",
            rusqlite::params![session_id, item_index, message_index],
        )
        .expect("Failed to insert message transcript item");
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

#[test]
fn load_session_by_id_for_filter_matches_exact_id() {
    let db = TempDatabase::new();
    db.seed();

    let sessions = load_session_by_id_for_filter(
        &db.path,
        &AiAssistant::ALL,
        &ProjectFilter::AllSessions,
        "session-b",
    )
    .expect("Session ID lookup failed");
    let ids: Vec<&str> = sessions.iter().map(|session| session.id.as_str()).collect();

    assert_eq!(ids, vec!["session-b"]);
}

#[test]
fn load_session_by_id_for_filter_requires_exact_id() {
    let db = TempDatabase::new();
    db.seed();

    let sessions = load_session_by_id_for_filter(
        &db.path,
        &AiAssistant::ALL,
        &ProjectFilter::AllSessions,
        "session",
    )
    .expect("Session ID lookup failed");

    assert!(sessions.is_empty());
}

#[test]
fn load_session_by_id_for_filter_respects_tool_filter() {
    let db = TempDatabase::new();
    db.seed();

    let matching = load_session_by_id_for_filter(
        &db.path,
        &[AiAssistant::OpenCode],
        &ProjectFilter::AllSessions,
        "session-b",
    )
    .expect("Session ID lookup with matching tool failed");
    let blocked = load_session_by_id_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        "session-b",
    )
    .expect("Session ID lookup with blocked tool failed");

    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].id, "session-b");
    assert!(blocked.is_empty());
}

#[test]
fn load_session_by_id_for_filter_respects_project_filter() {
    let db = TempDatabase::new();
    db.seed();

    let matching = load_session_by_id_for_filter(
        &db.path,
        &AiAssistant::ALL,
        &ProjectFilter::Project(2),
        "session-b",
    )
    .expect("Session ID lookup with matching project failed");
    let blocked = load_session_by_id_for_filter(
        &db.path,
        &AiAssistant::ALL,
        &ProjectFilter::Project(1),
        "session-b",
    )
    .expect("Session ID lookup with blocked project failed");

    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].id, "session-b");
    assert!(blocked.is_empty());
}

#[test]
fn load_session_by_id_for_filter_respects_pinned_filter() {
    let db = TempDatabase::new();
    db.seed();
    db.connection
        .execute(
            "UPDATE sessions SET pinned_at = ?1 WHERE id = ?2",
            rusqlite::params![123_i64, "session-b"],
        )
        .expect("Failed to pin fixture session");

    let matching = load_session_by_id_for_filter(
        &db.path,
        &AiAssistant::ALL,
        &ProjectFilter::Pinned,
        "session-b",
    )
    .expect("Pinned session ID lookup failed");
    let blocked = load_session_by_id_for_filter(
        &db.path,
        &AiAssistant::ALL,
        &ProjectFilter::Pinned,
        "session-a",
    )
    .expect("Unpinned session ID lookup failed");

    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].id, "session-b");
    assert!(blocked.is_empty());
}

#[test]
fn load_session_by_id_for_filter_returns_empty_for_unknown_id() {
    let db = TempDatabase::new();
    db.seed();

    let sessions = load_session_by_id_for_filter(
        &db.path,
        &AiAssistant::ALL,
        &ProjectFilter::AllSessions,
        "missing-session",
    )
    .expect("Unknown session ID lookup failed");

    assert!(sessions.is_empty());
}

#[test]
fn find_session_match_positions_returns_ordered_message_matches() {
    let db = TempDatabase::new();
    seed_match_session(&db, "session-match", "claude_code", 100);
    insert_message_item(&db, "session-match", 0, 8, "needle later item");
    insert_message_item(&db, "session-match", 1, 2, "needle earlier item");
    insert_message_item(&db, "session-match", 2, 5, "no matching token here");
    insert_message_item(&db, "session-match", 3, 13, "needle final item");

    let positions = find_session_match_positions(&db.connection, "session-match", "needle")
        .expect("match position query should succeed");
    let item_indexes: Vec<i64> = positions
        .iter()
        .map(|position| position.item_index)
        .collect();

    assert_eq!(item_indexes, vec![2, 8, 13]);
}

#[test]
fn find_session_match_positions_filters_by_session() {
    let db = TempDatabase::new();
    seed_match_session(&db, "session-one", "claude_code", 100);
    seed_match_session(&db, "session-two", "opencode", 200);
    insert_message_item(&db, "session-one", 0, 4, "shared needle in first session");
    insert_message_item(&db, "session-two", 0, 7, "shared needle in second session");

    let positions = find_session_match_positions(&db.connection, "session-one", "needle")
        .expect("match position query should succeed");

    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].item_index, 4);
}

#[test]
fn find_session_match_positions_retries_sanitized_invalid_query() {
    let db = TempDatabase::new();
    seed_match_session(&db, "session-sanitize", "claude_code", 100);
    insert_message_item(&db, "session-sanitize", 0, 3, "alpha survives sanitization");

    let positions = find_session_match_positions(&db.connection, "session-sanitize", "\"alpha")
        .expect("sanitized query should not surface an FTS syntax error");

    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].item_index, 3);
}

#[test]
fn find_session_match_positions_invalid_punctuation_only_query() {
    let db = TempDatabase::new();
    seed_match_session(&db, "session-punctuation", "claude_code", 100);
    insert_message_item(
        &db,
        "session-punctuation",
        0,
        3,
        "alpha exists but punctuation does not",
    );

    let positions = find_session_match_positions(&db.connection, "session-punctuation", "\"*()")
        .expect("punctuation-only query should return an empty result");

    assert!(positions.is_empty());
}

#[test]
fn find_session_match_positions_empty_query() {
    let db = TempDatabase::new();
    seed_match_session(&db, "session-empty", "claude_code", 100);
    insert_message_item(&db, "session-empty", 0, 3, "alpha exists");

    let blank = find_session_match_positions(&db.connection, "session-empty", "   ")
        .expect("blank query should return an empty result");
    let empty = find_session_match_positions(&db.connection, "session-empty", "")
        .expect("empty query should return an empty result");

    assert!(blank.is_empty());
    assert!(empty.is_empty());
}
