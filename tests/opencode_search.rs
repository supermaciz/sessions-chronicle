use rusqlite::Connection;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sessions_chronicle::database::SessionIndexer;
use sessions_chronicle::database::search_sessions_for_filter;
use sessions_chronicle::models::{AiAssistant, ProjectFilter};

struct TempDatabase {
    path: PathBuf,
}

impl TempDatabase {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        path.push(format!(
            "sessions-chronicle-opencode-test-{}-{}.db",
            std::process::id(),
            nanos
        ));
        let connection = Connection::open(&path).expect("Failed to open temp database");
        sessions_chronicle::database::schema::initialize_database(&connection)
            .expect("Failed to initialize database");

        drop(connection);
        Self { path }
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn opencode_search_finds_text_part_content() {
    let db = TempDatabase::new();
    let storage_root = PathBuf::from("tests/fixtures/opencode_storage");

    let mut indexer = SessionIndexer::new(&db.path).expect("Failed to create indexer");
    let indexed_count = indexer
        .index_opencode_sessions(&storage_root, &[])
        .expect("Failed to index OpenCode sessions");

    assert_eq!(
        indexed_count, 4,
        "Should index 4 sessions (3 visible + 1 subagent)"
    );

    let sessions = search_sessions_for_filter(
        &db.path,
        &[AiAssistant::OpenCode],
        &ProjectFilter::AllSessions,
        false,
        "I can help you with that task",
    )
    .expect("Search failed");

    assert_eq!(
        sessions.len(),
        1,
        "Should find exactly one session with 'I can help you with that task'"
    );
    assert_eq!(
        sessions[0].id, "session-001",
        "Should find correct OpenCode session"
    );
    assert_eq!(
        sessions[0].tool,
        AiAssistant::OpenCode,
        "Session should be an OpenCode session"
    );
}

#[test]
fn opencode_search_excludes_tool_output() {
    let db = TempDatabase::new();
    let storage_root = PathBuf::from("tests/fixtures/opencode_storage");

    let mut indexer = SessionIndexer::new(&db.path).expect("Failed to create indexer");
    let indexed_count = indexer
        .index_opencode_sessions(&storage_root, &[])
        .expect("Failed to index OpenCode sessions");

    assert_eq!(
        indexed_count, 4,
        "Should index 4 sessions (3 visible + 1 subagent)"
    );

    // Search for content that exists only in tool output (now excluded)
    let sessions = search_sessions_for_filter(
        &db.path,
        &[AiAssistant::OpenCode],
        &ProjectFilter::AllSessions,
        false,
        "total",
    )
    .expect("Search failed");

    assert_eq!(
        sessions.len(),
        0,
        "Should not find sessions when searching for tool output content"
    );
}

#[test]
fn opencode_search_respects_tool_filter() {
    let db = TempDatabase::new();
    let storage_root = PathBuf::from("tests/fixtures/opencode_storage");

    let mut indexer = SessionIndexer::new(&db.path).expect("Failed to create indexer");
    indexer
        .index_opencode_sessions(&storage_root, &[])
        .expect("Failed to index OpenCode sessions");

    let sessions = search_sessions_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        false,
        "Hello OpenCode",
    )
    .expect("Search failed");

    assert_eq!(
        sessions.len(),
        0,
        "Should not find OpenCode session when filtering for ClaudeCode only"
    );
}

#[test]
fn opencode_dual_read_sqlite_only_session_is_searchable() {
    let db = TempDatabase::new();
    let storage_root = PathBuf::from("tests/fixtures/opencode_storage");
    let opencode_db = storage_root.join("opencode.db");

    let mut indexer = SessionIndexer::new(&db.path).expect("Failed to create indexer");
    indexer
        .index_opencode_sessions(&storage_root, &[opencode_db.clone()])
        .expect("Failed to index");

    let sessions = search_sessions_for_filter(
        &db.path,
        &[AiAssistant::OpenCode],
        &ProjectFilter::AllSessions,
        false,
        "This session only exists in SQLite",
    )
    .expect("Search failed");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "session-sqlite-only");
}

#[test]
fn opencode_dual_read_total_session_count() {
    let db = TempDatabase::new();
    let storage_root = PathBuf::from("tests/fixtures/opencode_storage");
    let opencode_db = storage_root.join("opencode.db");

    let mut indexer = SessionIndexer::new(&db.path).expect("Failed to create indexer");
    let count = indexer
        .index_opencode_sessions(&storage_root, &[opencode_db.clone()])
        .expect("Failed to index");

    assert_eq!(count, 6);
}
