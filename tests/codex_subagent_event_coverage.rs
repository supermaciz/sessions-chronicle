use rusqlite::Connection;
use sessions_chronicle::database::{SessionIndexer, load_session, load_subagent, load_tool_call};
use std::path::Path;
use tempfile::NamedTempFile;

const RESPONSE_ITEM_FIXTURE: &str = "tests/fixtures/codex_subagent_linkage/2026/05/18";
const RESPONSE_ITEM_PARENT: &str = "019e3829-1153-77d3-acc5-8d683325f21d";
const RESPONSE_ITEM_CHILD: &str = "019e382d-e986-7b62-9f97-b015c5cc70f5";

const RESUME_FIXTURE: &str = "tests/fixtures/codex_sessions/2026/05/19";
const RESUME_SESSION: &str = "019e3d94-0000-7000-8000-000000000001";

fn subagent_count(db_path: &Path, session_id: &str) -> i64 {
    let conn = Connection::open(db_path).unwrap();
    conn.query_row(
        "SELECT COUNT(*) FROM subagents WHERE session_id = ?1",
        [session_id],
        |row| row.get(0),
    )
    .unwrap()
}

// Codex 0.130.0 rollouts can persist subagent work as `response_item`
// `function_call` / `function_call_output` pairs (`spawn_agent` / `wait_agent`)
// instead of `event_msg` `collab_*` events. The child rollout still links back
// through `session_meta.payload.source.subagent.thread_spawn.parent_thread_id`.
#[test]
fn codex_response_item_spawn_wait_indexes_as_tool_calls_and_links_child() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    indexer
        .index_codex_sessions(Path::new(RESPONSE_ITEM_FIXTURE))
        .unwrap();

    let parent = load_session(temp_db.path(), RESPONSE_ITEM_PARENT)
        .unwrap()
        .expect("parent session should be indexed");
    assert!(!parent.is_subagent);

    let child = load_session(temp_db.path(), RESPONSE_ITEM_CHILD)
        .unwrap()
        .expect("child session should be indexed");
    assert!(child.is_subagent);
    assert_eq!(
        child.parent_session_id.as_deref(),
        Some(RESPONSE_ITEM_PARENT)
    );

    let spawn = load_tool_call(temp_db.path(), RESPONSE_ITEM_PARENT, "call_spawn_ri_1")
        .unwrap()
        .expect("spawn_agent should be indexed as a tool call");
    assert_eq!(spawn.tool_name, "spawn_agent");

    let wait = load_tool_call(temp_db.path(), RESPONSE_ITEM_PARENT, "call_wait_ri_1")
        .unwrap()
        .expect("wait_agent should be indexed as a tool call");
    assert_eq!(wait.tool_name, "wait_agent");

    // Current parser behavior: the response-item `spawn_agent` / `wait_agent`
    // form is not yet mapped into parent-side `Subagent` rows. This guards that
    // gap so a future enrichment change updates the assertion deliberately.
    assert_eq!(subagent_count(temp_db.path(), RESPONSE_ITEM_PARENT), 0);
}

// Synthetic fixture: no real `collab_resume_end` event exists in captured
// rollouts, so this is derived from the upstream `CollabResumeEndEvent` struct
// (codex-rs/protocol/src/protocol.rs). It guards that an unhandled `collab_*`
// event is ignored gracefully without breaking indexing of the spawn row.
#[test]
fn codex_collab_resume_end_is_ignored_without_breaking_indexing() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    indexer
        .index_codex_sessions(Path::new(RESUME_FIXTURE))
        .unwrap();

    let session = load_session(temp_db.path(), RESUME_SESSION)
        .unwrap()
        .expect("resume fixture session should be indexed");
    assert!(!session.is_subagent);

    let subagent = load_subagent(temp_db.path(), RESUME_SESSION, "call_spawn_1")
        .unwrap()
        .expect("collab_agent_spawn_end should still produce a subagent row");
    assert_eq!(subagent.title, "Sartre");

    // Current parser behavior: `collab_resume_end` is not used to enrich
    // parent-side subagents, so the spawn row carries no result summary.
    assert_eq!(subagent.result_summary, None);
}
