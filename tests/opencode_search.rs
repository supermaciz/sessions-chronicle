use rusqlite::Connection;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sessions_chronicle::database::SessionIndexer;
use sessions_chronicle::database::search_sessions;
use sessions_chronicle::models::Tool;

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
        .index_opencode_sessions(&storage_root)
        .expect("Failed to index OpenCode sessions");

    assert_eq!(indexed_count, 2, "Should index 2 non-subagent sessions");

    let sessions = search_sessions(&db.path, &[Tool::OpenCode], "I can help you with that task")
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
        Tool::OpenCode,
        "Session should be an OpenCode session"
    );
}

#[test]
fn opencode_search_excludes_tool_output() {
    let db = TempDatabase::new();
    let storage_root = PathBuf::from("tests/fixtures/opencode_storage");

    let mut indexer = SessionIndexer::new(&db.path).expect("Failed to create indexer");
    let indexed_count = indexer
        .index_opencode_sessions(&storage_root)
        .expect("Failed to index OpenCode sessions");

    assert_eq!(indexed_count, 2, "Should index 2 non-subagent sessions");

    // Search for content that exists only in tool output (now excluded)
    let sessions = search_sessions(&db.path, &[Tool::OpenCode], "total").expect("Search failed");

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
        .index_opencode_sessions(&storage_root)
        .expect("Failed to index OpenCode sessions");

    let sessions =
        search_sessions(&db.path, &[Tool::ClaudeCode], "Hello OpenCode").expect("Search failed");

    assert_eq!(
        sessions.len(),
        0,
        "Should not find OpenCode session when filtering for ClaudeCode only"
    );
}
