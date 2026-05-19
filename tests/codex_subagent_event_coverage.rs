use rusqlite::Connection;
use sessions_chronicle::database::{
    SessionIndexer, load_all_transcript_items, load_session, load_subagent, load_tool_call,
};
use sessions_chronicle::models::TranscriptItemKind;
use std::path::Path;
use tempfile::NamedTempFile;

const RESPONSE_ITEM_FIXTURE: &str = "tests/fixtures/codex_subagent_linkage/2026/05/18";
const RESPONSE_ITEM_PARENT: &str = "019e3829-1153-77d3-acc5-8d683325f21d";
const RESPONSE_ITEM_CHILD: &str = "019e382d-e986-7b62-9f97-b015c5cc70f5";

const RESUME_FIXTURE: &str = "tests/fixtures/codex_sessions/2026/05/19";
const RESUME_SESSION: &str = "019e3d94-0000-7000-8000-000000000001";

const SPAWN_NO_AGENT_FIXTURE: &str = "tests/fixtures/codex_subagent_linkage/2026/05/20";
const SPAWN_NO_AGENT_PARENT: &str = "019e3f00-0000-7000-8000-000000000001";

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
fn codex_response_item_spawn_wait_indexes_as_subagent_and_links_child() {
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

    assert!(
        load_tool_call(temp_db.path(), RESPONSE_ITEM_PARENT, "call_spawn_ri_1")
            .unwrap()
            .is_none(),
        "spawn_agent should not be indexed as a generic tool call"
    );
    assert!(
        load_tool_call(temp_db.path(), RESPONSE_ITEM_PARENT, "call_wait_ri_1")
            .unwrap()
            .is_none(),
        "wait_agent should not be indexed as a generic tool call"
    );

    assert_eq!(subagent_count(temp_db.path(), RESPONSE_ITEM_PARENT), 1);

    let subagent = load_subagent(temp_db.path(), RESPONSE_ITEM_PARENT, "call_spawn_ri_1")
        .unwrap()
        .expect("response-item spawn_agent should become a subagent");
    assert_eq!(subagent.title, "Nord");
    assert_eq!(subagent.agent_id.as_deref(), Some(RESPONSE_ITEM_CHILD));
    assert_eq!(
        subagent.child_session_id.as_deref(),
        Some(RESPONSE_ITEM_CHILD)
    );
    assert_eq!(
        subagent.prompt.as_deref(),
        Some("Advise the next milestone")
    );
    assert_eq!(
        subagent.result_summary.as_deref(),
        Some("Recommend the project timeline view next.")
    );
}

#[test]
fn codex_response_item_spawn_wait_transcript_contains_subagent_not_tool_calls() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    indexer
        .index_codex_sessions(Path::new(RESPONSE_ITEM_FIXTURE))
        .unwrap();

    let transcript = load_all_transcript_items(temp_db.path(), RESPONSE_ITEM_PARENT, 512).unwrap();

    let subagent_rows: Vec<_> = transcript
        .iter()
        .filter(|item| item.kind == TranscriptItemKind::Subagent)
        .collect();
    assert_eq!(subagent_rows.len(), 1);
    assert_eq!(
        subagent_rows[0].subagent_id.as_deref(),
        Some("call_spawn_ri_1")
    );
    assert_eq!(subagent_rows[0].subagent_title.as_deref(), Some("Nord"));

    let tool_names: Vec<_> = transcript
        .iter()
        .filter_map(|item| item.tool_name.as_deref())
        .collect();
    assert!(!tool_names.contains(&"spawn_agent"));
    assert!(!tool_names.contains(&"wait_agent"));
}

// Codex rollouts can persist a `spawn_agent` whose `function_call_output`
// omits `agent_id` (a rejected/failed spawn, or an unexpected output shape).
// The begin event already short-circuits before `push_tool_call`, so the spawn
// must still degrade to an unlinked subagent row rather than vanishing from
// both the transcript and the subagent tables.
#[test]
fn codex_response_item_spawn_without_agent_id_degrades_to_unlinked_subagent() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    indexer
        .index_codex_sessions(Path::new(SPAWN_NO_AGENT_FIXTURE))
        .unwrap();

    let parent = load_session(temp_db.path(), SPAWN_NO_AGENT_PARENT)
        .unwrap()
        .expect("parent session should be indexed");
    assert!(!parent.is_subagent);

    assert!(
        load_tool_call(
            temp_db.path(),
            SPAWN_NO_AGENT_PARENT,
            "call_spawn_noagent_1"
        )
        .unwrap()
        .is_none(),
        "spawn_agent should not regress to a generic tool call"
    );

    assert_eq!(subagent_count(temp_db.path(), SPAWN_NO_AGENT_PARENT), 1);

    let subagent = load_subagent(
        temp_db.path(),
        SPAWN_NO_AGENT_PARENT,
        "call_spawn_noagent_1",
    )
    .unwrap()
    .expect("spawn_agent without agent_id should still become a subagent");
    assert_eq!(subagent.title, "product-manager");
    assert_eq!(subagent.agent_id, None);
    assert_eq!(subagent.child_session_id, None);
    assert_eq!(
        subagent.prompt.as_deref(),
        Some("Advise the next milestone")
    );
}

// Synthetic fixture: no real `collab_resume_end` event exists in captured
// rollouts, so this is derived from the upstream `CollabResumeEndEvent` struct
// (codex-rs/protocol/src/protocol.rs). It guards that resume enriches an
// existing spawn row without creating a separate subagent.
#[test]
fn codex_collab_resume_end_enriches_existing_subagent() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    indexer
        .index_codex_sessions(Path::new(RESUME_FIXTURE))
        .unwrap();

    let session = load_session(temp_db.path(), RESUME_SESSION)
        .unwrap()
        .expect("resume fixture session should be indexed");
    assert!(!session.is_subagent);

    assert_eq!(subagent_count(temp_db.path(), RESUME_SESSION), 1);

    let subagent = load_subagent(temp_db.path(), RESUME_SESSION, "call_spawn_1")
        .unwrap()
        .expect("collab_agent_spawn_end should still produce a subagent row");
    assert_eq!(subagent.title, "Sartre");
    assert_eq!(
        subagent.result_summary.as_deref(),
        Some("Parser changes look correct.")
    );
}
