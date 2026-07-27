use sessions_chronicle::database::{SessionIndexer, load_session, load_subagent};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};

const PARENT_ID: &str = "9c1f2a30-4b5d-4e6f-8a90-1b2c3d4e5f60";
const CHILD_ID: &str =
    "claude-subagent::9c1f2a30-4b5d-4e6f-8a90-1b2c3d4e5f60::areview-docs-0123456789abcdef";
const FIXTURE: &str = "tests/fixtures/claude_teammate_linkage";

/// Copies the parent transcript alone into `dir`.
fn copy_parent(dir: &Path) {
    fs::copy(
        PathBuf::from(FIXTURE).join(format!("{PARENT_ID}.jsonl")),
        dir.join(format!("{PARENT_ID}.jsonl")),
    )
    .unwrap();
}

/// Copies the nested teammate transcript alone into `dir`.
fn copy_child(dir: &Path) {
    let subagents = dir.join(PARENT_ID).join("subagents");
    fs::create_dir_all(&subagents).unwrap();
    fs::copy(
        PathBuf::from(FIXTURE)
            .join(PARENT_ID)
            .join("subagents")
            .join("agent-areview-docs-0123456789abcdef.jsonl"),
        subagents.join("agent-areview-docs-0123456789abcdef.jsonl"),
    )
    .unwrap();
}

fn assert_linked(db_path: &Path) {
    let child = load_session(db_path, CHILD_ID)
        .unwrap()
        .expect("child session should be indexed");
    assert!(child.is_subagent);
    assert_eq!(child.parent_session_id.as_deref(), Some(PARENT_ID));

    let subagent = load_subagent(db_path, PARENT_ID, "toolu_agent_100")
        .unwrap()
        .expect("parent subagent should exist");
    assert_eq!(subagent.agent_id, None);
    assert_eq!(subagent.agent_name.as_deref(), Some("review-docs"));
    assert_eq!(subagent.child_session_id.as_deref(), Some(CHILD_ID));
}

#[test]
fn indexing_teammate_subagent_links_parent_to_child_session() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    indexer.index_claude_sessions(Path::new(FIXTURE)).unwrap();

    assert_linked(temp_db.path());
}

#[test]
fn teammate_linkage_works_when_the_parent_is_indexed_first() {
    let temp_db = NamedTempFile::new().unwrap();
    let sessions_dir = TempDir::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    copy_parent(sessions_dir.path());
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    copy_child(sessions_dir.path());
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    assert_linked(temp_db.path());
}

#[test]
fn teammate_linkage_works_when_the_child_is_indexed_first() {
    let temp_db = NamedTempFile::new().unwrap();
    let sessions_dir = TempDir::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    copy_child(sessions_dir.path());
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    copy_parent(sessions_dir.path());
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    assert_linked(temp_db.path());
}

#[test]
fn duplicate_teammate_names_leave_both_subagents_unlinked() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    indexer
        .index_claude_sessions(Path::new("tests/fixtures/claude_teammate_duplicate"))
        .unwrap();

    let parent_id = "7d2e1b40-5c6e-4f70-9b01-2c3d4e5f6071";
    for tool_use_id in ["toolu_agent_200", "toolu_agent_201"] {
        let subagent = load_subagent(temp_db.path(), parent_id, tool_use_id)
            .unwrap()
            .expect("parent subagent should exist");
        assert_eq!(subagent.agent_name.as_deref(), Some("reviewer"));
        assert_eq!(
            subagent.child_session_id, None,
            "{tool_use_id} must stay unlinked: the name is ambiguous"
        );
    }
}
