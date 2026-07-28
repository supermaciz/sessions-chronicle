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

const CHILD_DUP_FIXTURE: &str = "tests/fixtures/claude_teammate_child_duplicate";
const CHILD_DUP_PARENT_ID: &str = "3f4a5b60-7c8d-4e9f-a0b1-c2d3e4f5a6b7";

/// Copies the parent transcript alone into `dir`.
fn copy_child_dup_parent(dir: &Path) {
    fs::copy(
        PathBuf::from(CHILD_DUP_FIXTURE).join(format!("{CHILD_DUP_PARENT_ID}.jsonl")),
        dir.join(format!("{CHILD_DUP_PARENT_ID}.jsonl")),
    )
    .unwrap();
}

/// Copies both same-named nested teammate transcripts into `dir`.
fn copy_child_dup_children(dir: &Path) {
    for name in [
        "agent-asolo-aaaaaaaaaaaaaaaa.jsonl",
        "agent-asolo-bbbbbbbbbbbbbbbb.jsonl",
    ] {
        copy_child_dup_one(dir, name);
    }
}

/// Copies a single named nested teammate transcript into `dir`.
fn copy_child_dup_one(dir: &Path, file_name: &str) {
    let subagents = dir.join(CHILD_DUP_PARENT_ID).join("subagents");
    fs::create_dir_all(&subagents).unwrap();
    fs::copy(
        PathBuf::from(CHILD_DUP_FIXTURE)
            .join(CHILD_DUP_PARENT_ID)
            .join("subagents")
            .join(file_name),
        subagents.join(file_name),
    )
    .unwrap();
}

fn assert_child_dup_unlinked(temp_db: &Path) {
    let subagent = load_subagent(temp_db, CHILD_DUP_PARENT_ID, "toolu_agent_300")
        .unwrap()
        .expect("parent subagent should exist");
    assert_eq!(subagent.agent_name.as_deref(), Some("solo"));
    assert_eq!(
        subagent.child_session_id, None,
        "solo must stay unlinked: two child transcripts share the name"
    );
}

#[test]
fn duplicate_child_transcripts_leave_the_single_teammate_row_unlinked() {
    // Covers the sibling-count guard in `link_teammate_child_tx`, which
    // `duplicate_teammate_names_leave_both_subagents_unlinked` does not reach:
    // that fixture's ambiguity is on the parent side (two subagent rows named
    // "reviewer"), so it returns on `parent_rows.len() != 1` before ever
    // touching `siblings.len() > 1`. Here the parent declares exactly ONE
    // teammate named "solo", so `parent_rows.len() == 1` passes and only the
    // sibling guard can prevent a mislink between the two same-named children.
    //
    // The parent is indexed first, on its own, so that both children are
    // later indexed through the `is_subagent` / `link_teammate_child_tx`
    // branch (rather than the top-level parent loop, which has its own,
    // separate ambiguity guard and would mask a regression in this one).
    let temp_db = NamedTempFile::new().unwrap();
    let sessions_dir = TempDir::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    copy_child_dup_parent(sessions_dir.path());
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    // `index_claude_sessions` always walks and reprocesses every file in
    // `sessions_dir` (it is not incremental), so leaving the parent file in
    // place would make the second call also reprocess it. Reprocessing wipes
    // and rebuilds its `subagents` rows via `replace_session_contents_tx`,
    // and the top-level parent loop has its own separate ambiguity guard
    // that would relink or mask the result, hiding whatever
    // `link_teammate_child_tx` actually did with the two children. Removing
    // the parent file isolates the second call to the children alone, so
    // only the `is_subagent` / `link_teammate_child_tx` path runs.
    fs::remove_file(
        sessions_dir
            .path()
            .join(format!("{CHILD_DUP_PARENT_ID}.jsonl")),
    )
    .unwrap();
    copy_child_dup_children(sessions_dir.path());
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    assert_child_dup_unlinked(temp_db.path());
}

#[test]
fn duplicate_child_transcripts_stay_unlinked_regardless_of_processing_order() {
    // Proves the retraction in `link_teammate_child_tx` converges on
    // unlinked from three different orderings, each indexed one file at a
    // time so the order is deterministic rather than left to walkdir:
    // child A discovered before child B, child B before child A, and both
    // children already indexed before the parent is (re-)indexed.
    for order in ["a_then_b", "b_then_a", "children_then_parent"] {
        let temp_db = NamedTempFile::new().unwrap();
        let sessions_dir = TempDir::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

        match order {
            "a_then_b" => {
                copy_child_dup_parent(sessions_dir.path());
                indexer.index_claude_sessions(sessions_dir.path()).unwrap();
                fs::remove_file(
                    sessions_dir
                        .path()
                        .join(format!("{CHILD_DUP_PARENT_ID}.jsonl")),
                )
                .unwrap();

                copy_child_dup_one(sessions_dir.path(), "agent-asolo-aaaaaaaaaaaaaaaa.jsonl");
                indexer.index_claude_sessions(sessions_dir.path()).unwrap();

                copy_child_dup_one(sessions_dir.path(), "agent-asolo-bbbbbbbbbbbbbbbb.jsonl");
                indexer.index_claude_sessions(sessions_dir.path()).unwrap();
            }
            "b_then_a" => {
                copy_child_dup_parent(sessions_dir.path());
                indexer.index_claude_sessions(sessions_dir.path()).unwrap();
                fs::remove_file(
                    sessions_dir
                        .path()
                        .join(format!("{CHILD_DUP_PARENT_ID}.jsonl")),
                )
                .unwrap();

                copy_child_dup_one(sessions_dir.path(), "agent-asolo-bbbbbbbbbbbbbbbb.jsonl");
                indexer.index_claude_sessions(sessions_dir.path()).unwrap();

                copy_child_dup_one(sessions_dir.path(), "agent-asolo-aaaaaaaaaaaaaaaa.jsonl");
                indexer.index_claude_sessions(sessions_dir.path()).unwrap();
            }
            "children_then_parent" => {
                copy_child_dup_children(sessions_dir.path());
                indexer.index_claude_sessions(sessions_dir.path()).unwrap();

                copy_child_dup_parent(sessions_dir.path());
                indexer.index_claude_sessions(sessions_dir.path()).unwrap();
            }
            _ => unreachable!(),
        }

        assert_child_dup_unlinked(temp_db.path());
    }
}

#[test]
fn retraction_is_scoped_to_the_ambiguous_name_and_does_not_touch_a_sibling() {
    // The retraction UPDATE is `WHERE session_id = ?1 AND agent_name = ?2`.
    // Prove that scoping actually holds: the same parent also spawns a
    // "helper" teammate with its own, unambiguous child. Once "solo"
    // becomes ambiguous and gets retracted, "helper" must still be linked.
    let temp_db = NamedTempFile::new().unwrap();
    let sessions_dir = TempDir::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    copy_child_dup_parent(sessions_dir.path());
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();
    fs::remove_file(
        sessions_dir
            .path()
            .join(format!("{CHILD_DUP_PARENT_ID}.jsonl")),
    )
    .unwrap();

    copy_child_dup_one(sessions_dir.path(), "agent-ahelper-cccccccccccccccc.jsonl");
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    copy_child_dup_children(sessions_dir.path());
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    assert_child_dup_unlinked(temp_db.path());

    let helper_child_id =
        format!("claude-subagent::{CHILD_DUP_PARENT_ID}::ahelper-cccccccccccccccc");
    let helper = load_subagent(temp_db.path(), CHILD_DUP_PARENT_ID, "toolu_agent_301")
        .unwrap()
        .expect("helper subagent should exist");
    assert_eq!(helper.agent_name.as_deref(), Some("helper"));
    assert_eq!(
        helper.child_session_id.as_deref(),
        Some(helper_child_id.as_str()),
        "retracting the ambiguous \"solo\" link must not touch \"helper\"'s legitimate link"
    );
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
