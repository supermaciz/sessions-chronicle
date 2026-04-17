use rusqlite::Connection;
use sessions_chronicle::database::{SessionIndexer, load_session, load_subagent};
use std::path::Path;
use tempfile::NamedTempFile;

#[test]
fn indexing_claude_nested_subagent_links_parent_to_child_session() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    indexer
        .index_claude_sessions(Path::new("tests/fixtures/claude_subagent_linkage"))
        .unwrap();

    let parent = load_session(temp_db.path(), "65ce34ec-2589-4f2a-aad3-f536cf8b2906")
        .unwrap()
        .expect("parent session should be indexed");
    assert!(!parent.is_subagent);

    let child = load_session(
        temp_db.path(),
        "claude-subagent::65ce34ec-2589-4f2a-aad3-f536cf8b2906::a41c0fb07beb52ed6",
    )
    .unwrap()
    .expect("child session should be indexed");
    assert!(child.is_subagent);
    assert_eq!(
        child.parent_session_id.as_deref(),
        Some("65ce34ec-2589-4f2a-aad3-f536cf8b2906")
    );

    let subagent = load_subagent(
        temp_db.path(),
        "65ce34ec-2589-4f2a-aad3-f536cf8b2906",
        "toolu_agent_001",
    )
    .unwrap()
    .expect("parent subagent should exist");
    assert_eq!(subagent.agent_id.as_deref(), Some("a41c0fb07beb52ed6"));
    assert_eq!(
        subagent.child_session_id.as_deref(),
        Some("claude-subagent::65ce34ec-2589-4f2a-aad3-f536cf8b2906::a41c0fb07beb52ed6")
    );

    let conn = Connection::open(temp_db.path()).unwrap();
    let session_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(session_count, 2);
}
