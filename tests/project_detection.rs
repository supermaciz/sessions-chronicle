use rusqlite::Connection;
use sessions_chronicle::database::{SessionIndexer, load_session};
use sessions_chronicle::project_resolver::resolve_project_path;
use std::path::Path;
use tempfile::NamedTempFile;

#[test]
fn fixture_paths_store_resolved_project_path_and_keep_raw_session_path() {
    let raw_cwd = "/home/user/project";
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    indexer
        .index_claude_sessions(Path::new("tests/fixtures/claude_sessions"))
        .unwrap();

    let session = load_session(temp_db.path(), "abc123")
        .unwrap()
        .expect("fixture session should exist");
    let project_id = session
        .project_id
        .expect("fixture session should have project_id");

    let conn = Connection::open(temp_db.path()).unwrap();
    let stored_path: String = conn
        .query_row(
            "SELECT path FROM projects WHERE id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(session.project_path.as_deref(), Some(raw_cwd));
    assert_eq!(stored_path, resolve_project_path(raw_cwd));
}

#[test]
fn reindexing_same_fixture_does_not_duplicate_project_rows() {
    let raw_cwd = "/home/user/project";
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let fixtures = Path::new("tests/fixtures/claude_sessions");

    indexer.index_claude_sessions(fixtures).unwrap();
    indexer.index_claude_sessions(fixtures).unwrap();

    let conn = Connection::open(temp_db.path()).unwrap();
    let resolved_path = resolve_project_path(raw_cwd);
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM projects WHERE path = ?1",
            [resolved_path],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 1);
}
