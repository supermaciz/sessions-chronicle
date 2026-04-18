use rusqlite::Connection;
use sessions_chronicle::database::{SessionIndexer, load_session, load_subagent};
use sessions_chronicle::models::AiAssistant;
use sessions_chronicle::session_sources::SessionSources;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir, tempdir};

const FIXTURE_BASE: &str = "tests/fixtures/codex_subagent_linkage/2026/04/18";
const PARENT_FILE: &str = "rollout-2026-04-18T13-17-00-019da0bb-541a-74e2-ae0a-6693c5e4fe04.jsonl";
const CHILD_FILE: &str = "rollout-2026-04-18T13-17-01-019da0bd-3df2-7191-a1a8-e326b55fe052.jsonl";
const PARENT_SESSION_ID: &str = "019da0bb-541a-74e2-ae0a-6693c5e4fe04";
const CHILD_SESSION_ID: &str = "019da0bd-3df2-7191-a1a8-e326b55fe052";

fn fixture_file_path(file_name: &str) -> PathBuf {
    Path::new(FIXTURE_BASE).join(file_name)
}

fn fixture_dir(include_parent: bool, include_child: bool) -> TempDir {
    let dir = tempdir().unwrap();
    let target_dir = dir.path().join("2026/04/18");
    fs::create_dir_all(&target_dir).unwrap();

    if include_parent {
        fs::copy(fixture_file_path(PARENT_FILE), target_dir.join(PARENT_FILE)).unwrap();
    }

    if include_child {
        fs::copy(fixture_file_path(CHILD_FILE), target_dir.join(CHILD_FILE)).unwrap();
    }

    dir
}

fn fixture_override_root() -> TempDir {
    let root = tempdir().unwrap();
    let target_dir = root.path().join("codex_sessions/2026/04/18");
    fs::create_dir_all(&target_dir).unwrap();

    fs::copy(fixture_file_path(PARENT_FILE), target_dir.join(PARENT_FILE)).unwrap();
    fs::copy(fixture_file_path(CHILD_FILE), target_dir.join(CHILD_FILE)).unwrap();

    root
}

fn append_harmless_parent_event(root: &TempDir) {
    let parent_path = root
        .path()
        .join("codex_sessions/2026/04/18")
        .join(PARENT_FILE);
    let mut file = OpenOptions::new().append(true).open(parent_path).unwrap();
    writeln!(
        file,
        "{{\"type\":\"event_msg\",\"timestamp\":\"2026-04-18T13:17:05Z\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"No-op incremental marker\"}}}}"
    )
    .unwrap();
}

#[test]
fn indexing_codex_parent_and_child_links_subagent_to_child_session() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    indexer
        .index_codex_sessions(Path::new("tests/fixtures/codex_subagent_linkage"))
        .unwrap();

    let parent = load_session(temp_db.path(), PARENT_SESSION_ID)
        .unwrap()
        .expect("parent session should be indexed");
    assert!(!parent.is_subagent);

    let child = load_session(temp_db.path(), CHILD_SESSION_ID)
        .unwrap()
        .expect("child session should be indexed");
    assert!(child.is_subagent);
    assert_eq!(child.parent_session_id.as_deref(), Some(PARENT_SESSION_ID));

    let subagent = load_subagent(temp_db.path(), PARENT_SESSION_ID, "call_spawn_1")
        .unwrap()
        .expect("parent subagent should be indexed");
    assert_eq!(subagent.agent_id.as_deref(), Some(CHILD_SESSION_ID));
    assert_eq!(subagent.child_session_id.as_deref(), Some(CHILD_SESSION_ID));
}

#[test]
fn indexing_codex_child_before_parent_still_links_on_second_pass() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    let child_only_dir = fixture_dir(false, true);
    let parent_only_dir = fixture_dir(true, false);

    indexer.index_codex_sessions(child_only_dir.path()).unwrap();
    indexer
        .index_codex_sessions(parent_only_dir.path())
        .unwrap();
    indexer.index_codex_sessions(child_only_dir.path()).unwrap();

    let subagent = load_subagent(temp_db.path(), PARENT_SESSION_ID, "call_spawn_1")
        .unwrap()
        .expect("parent subagent should exist after parent indexing");
    assert_eq!(subagent.child_session_id.as_deref(), Some(CHILD_SESSION_ID));
}

#[test]
fn indexing_codex_reindex_replaces_parent_subagents_without_duplicates() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    let parent_only_dir = fixture_dir(true, false);
    let child_only_dir = fixture_dir(false, true);

    indexer
        .index_codex_sessions(parent_only_dir.path())
        .unwrap();
    indexer.index_codex_sessions(child_only_dir.path()).unwrap();
    indexer
        .index_codex_sessions(parent_only_dir.path())
        .unwrap();
    indexer.index_codex_sessions(child_only_dir.path()).unwrap();

    let conn = Connection::open(temp_db.path()).unwrap();
    let subagent_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM subagents WHERE session_id = ?1",
            [PARENT_SESSION_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(subagent_count, 2);

    let linked_count: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM subagents
             WHERE session_id = ?1 AND child_session_id = ?2",
            rusqlite::params![PARENT_SESSION_ID, CHILD_SESSION_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(linked_count, 2);
}

#[test]
fn indexing_codex_duplicate_thread_rows_share_the_same_child_session_id() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    let parent_only_dir = fixture_dir(true, false);
    let child_only_dir = fixture_dir(false, true);

    indexer
        .index_codex_sessions(parent_only_dir.path())
        .unwrap();
    indexer.index_codex_sessions(child_only_dir.path()).unwrap();

    let conn = Connection::open(temp_db.path()).unwrap();
    let matching_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM subagents
             WHERE session_id = ?1 AND agent_id = ?2",
            rusqlite::params![PARENT_SESSION_ID, CHILD_SESSION_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(matching_rows, 2);

    let distinct_child_ids: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT child_session_id)
             FROM subagents
             WHERE session_id = ?1 AND agent_id = ?2",
            rusqlite::params![PARENT_SESSION_ID, CHILD_SESSION_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(distinct_child_ids, 1);
}

#[test]
fn codex_incremental_reindex_keeps_existing_child_link_when_child_file_is_skipped() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let root = fixture_override_root();
    let sources = SessionSources::resolve(Some(root.path()));

    let first_run = indexer.index_all_incremental(&sources).unwrap();
    let first_codex = first_run
        .per_source
        .iter()
        .find(|result| result.assistant == AiAssistant::Codex)
        .expect("Codex source result should exist");
    assert_eq!(first_codex.indexed, 2);

    let initial_subagent = load_subagent(temp_db.path(), PARENT_SESSION_ID, "call_spawn_1")
        .unwrap()
        .expect("parent subagent should exist after initial index");
    assert_eq!(
        initial_subagent.child_session_id.as_deref(),
        Some(CHILD_SESSION_ID)
    );

    append_harmless_parent_event(&root);

    let second_run = indexer.index_all_incremental(&sources).unwrap();
    let second_codex = second_run
        .per_source
        .iter()
        .find(|result| result.assistant == AiAssistant::Codex)
        .expect("Codex source result should exist");
    assert_eq!(second_codex.indexed, 1);
    assert_eq!(second_codex.skipped, 1);

    let subagent = load_subagent(temp_db.path(), PARENT_SESSION_ID, "call_spawn_1")
        .unwrap()
        .expect("parent subagent should exist after incremental reindex");
    assert_eq!(subagent.agent_id.as_deref(), Some(CHILD_SESSION_ID));
    assert_eq!(subagent.child_session_id.as_deref(), Some(CHILD_SESSION_ID));
}
