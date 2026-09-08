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

#[test]
fn retained_child_session_content_remains_loadable_when_sibling_source_missing() {
    // Establish two same-named children, remove one child's source through the
    // filesystem (marking it missing), rescan, and verify:
    // 1. the available child now links
    // 2. the retained child's content remains loadable
    // 3. when we restore the other child, ambiguity returns
    let temp_db = NamedTempFile::new().unwrap();
    let sessions_dir = TempDir::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    // Index parent first
    copy_child_dup_parent(sessions_dir.path());
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();
    fs::remove_file(
        sessions_dir
            .path()
            .join(format!("{CHILD_DUP_PARENT_ID}.jsonl")),
    )
    .unwrap();

    // Index both children
    copy_child_dup_children(sessions_dir.path());
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    // Both children should be indexed but unlinked (ambiguous)
    assert_child_dup_unlinked(temp_db.path());

    // Remove one child's source file to mark it missing
    fs::remove_file(
        sessions_dir
            .path()
            .join(CHILD_DUP_PARENT_ID)
            .join("subagents")
            .join("agent-asolo-aaaaaaaaaaaaaaaa.jsonl"),
    )
    .unwrap();

    // Rescan to detect the missing source
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    // Now the remaining available child should link
    let child_b_id = format!("claude-subagent::{CHILD_DUP_PARENT_ID}::asolo-bbbbbbbbbbbbbbbb");
    let subagent = load_subagent(temp_db.path(), CHILD_DUP_PARENT_ID, "toolu_agent_300")
        .unwrap()
        .expect("parent subagent should exist");
    assert_eq!(subagent.agent_name.as_deref(), Some("solo"));
    assert_eq!(
        subagent.child_session_id.as_deref(),
        Some(child_b_id.as_str()),
        "available child should now link"
    );

    // Retained child should still be loadable
    let child_a_id = format!("claude-subagent::{CHILD_DUP_PARENT_ID}::asolo-aaaaaaaaaaaaaaaa");
    let retained_child = load_session(temp_db.path(), &child_a_id)
        .unwrap()
        .expect("retained child session should be loadable");
    assert!(retained_child.is_subagent);
    assert_eq!(
        retained_child.parent_session_id.as_deref(),
        Some(CHILD_DUP_PARENT_ID)
    );

    // Restore the other child
    copy_child_dup_one(sessions_dir.path(), "agent-asolo-aaaaaaaaaaaaaaaa.jsonl");
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    // Now ambiguity should return and the link should be retracted
    assert_child_dup_unlinked(temp_db.path());
}

#[test]
fn retained_child_session_linkage_respects_fixture_ordering() {
    // Run the same test with the opposite ordering: children indexed before parent.
    let temp_db = NamedTempFile::new().unwrap();
    let sessions_dir = TempDir::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    // Index children first
    copy_child_dup_children(sessions_dir.path());
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    // Then index parent
    copy_child_dup_parent(sessions_dir.path());
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    // Both children should be indexed but unlinked (ambiguous)
    assert_child_dup_unlinked(temp_db.path());

    // Remove one child's source file to mark it missing
    fs::remove_file(
        sessions_dir
            .path()
            .join(CHILD_DUP_PARENT_ID)
            .join("subagents")
            .join("agent-asolo-bbbbbbbbbbbbbbbb.jsonl"),
    )
    .unwrap();

    // Rescan to detect the missing source
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    // Now the remaining available child should link
    let child_a_id = format!("claude-subagent::{CHILD_DUP_PARENT_ID}::asolo-aaaaaaaaaaaaaaaa");
    let subagent = load_subagent(temp_db.path(), CHILD_DUP_PARENT_ID, "toolu_agent_300")
        .unwrap()
        .expect("parent subagent should exist");
    assert_eq!(subagent.agent_name.as_deref(), Some("solo"));
    assert_eq!(
        subagent.child_session_id.as_deref(),
        Some(child_a_id.as_str()),
        "available child should now link"
    );

    // Retained child should still be loadable
    let child_b_id = format!("claude-subagent::{CHILD_DUP_PARENT_ID}::asolo-bbbbbbbbbbbbbbbb");
    let retained_child = load_session(temp_db.path(), &child_b_id)
        .unwrap()
        .expect("retained child session should be loadable");
    assert!(retained_child.is_subagent);
    assert_eq!(
        retained_child.parent_session_id.as_deref(),
        Some(CHILD_DUP_PARENT_ID)
    );

    // Restore the other child
    copy_child_dup_one(sessions_dir.path(), "agent-asolo-bbbbbbbbbbbbbbbb.jsonl");
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    // Now ambiguity should return and the link should be retracted
    assert_child_dup_unlinked(temp_db.path());
}

#[test]
fn retained_child_not_stranded_when_parent_is_reparsed() {
    // CRITICAL: reparsing a parent must not strand a link to a retained child.
    //
    // Uses a SINGLE teammate child (not the two-child ambiguous fixture) so
    // that once the child's source disappears there are genuinely ZERO
    // present candidates named "solo" — `child_sessions_named` returns `[]`
    // and `link_claude_subagents_tx` has no sibling to fall back on. A test
    // that leaves a second, present child in play (as an earlier version of
    // this test did) would pass even without the fix: `link_claude_subagents_tx`
    // would simply re-link that sibling after the reparse wipes the row.
    //
    // Scenario mirrors the regression this guards against: parent linked to
    // child C → C's transcript is deleted (C retained, link intact) → the
    // user resumes the parent so its own transcript changes → the reparse
    // wipes and rebuilds the parent's `subagents` rows → with zero present
    // candidates, only `replace_session_contents_preserving_links_tx`
    // snapshotting the link before the wipe can restore it.
    let temp_db = NamedTempFile::new().unwrap();
    let sessions_dir = TempDir::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    // Index parent and its single "solo" child together.
    copy_child_dup_parent(sessions_dir.path());
    copy_child_dup_one(sessions_dir.path(), "agent-asolo-aaaaaaaaaaaaaaaa.jsonl");
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    let child_a_id = format!("claude-subagent::{CHILD_DUP_PARENT_ID}::asolo-aaaaaaaaaaaaaaaa");
    let subagent = load_subagent(temp_db.path(), CHILD_DUP_PARENT_ID, "toolu_agent_300")
        .unwrap()
        .expect("parent subagent should exist");
    assert_eq!(
        subagent.child_session_id.as_deref(),
        Some(child_a_id.as_str()),
        "the sole present child should link"
    );

    // Delete the child's source: it becomes retained (missing), and the
    // existing link must be preserved even though there are now zero present
    // candidates named "solo".
    fs::remove_file(
        sessions_dir
            .path()
            .join(CHILD_DUP_PARENT_ID)
            .join("subagents")
            .join("agent-asolo-aaaaaaaaaaaaaaaa.jsonl"),
    )
    .unwrap();
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    // Genuinely reparse the parent by appending a line to its own transcript,
    // changing its size/mtime fingerprint so the scan cannot fingerprint-skip
    // it (a no-op remove-and-recreate of identical content would not force a
    // real reparse of the same content).
    let parent_path = sessions_dir
        .path()
        .join(format!("{CHILD_DUP_PARENT_ID}.jsonl"));
    let mut contents = fs::read_to_string(&parent_path).unwrap();
    contents.push_str(&format!(
        "{{\"type\":\"user\",\"timestamp\":\"2026-07-27T09:00:05.000Z\",\"cwd\":\"/home/user/project\",\"sessionId\":\"{CHILD_DUP_PARENT_ID}\",\"version\":\"2.1.220\",\"message\":{{\"content\":\"Resume solo\"}}}}\n"
    ));
    fs::write(&parent_path, contents).unwrap();
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    // The retained child must still be loadable.
    let retained_child = load_session(temp_db.path(), &child_a_id)
        .unwrap()
        .expect("retained child must be loadable after parent reparse");
    assert!(retained_child.is_subagent);

    // The link to the retained child must survive the parent's reparse.
    let subagent_after = load_subagent(temp_db.path(), CHILD_DUP_PARENT_ID, "toolu_agent_300")
        .unwrap()
        .expect("parent subagent should exist");
    assert_eq!(
        subagent_after.child_session_id.as_deref(),
        Some(child_a_id.as_str()),
        "parent reparse with zero present candidates must not strand the retained child link"
    );
}

#[test]
fn zero_present_candidates_preserves_retained_child_link() {
    // Test the 0 => arm: when there are zero present children, the existing
    // link to a retained child must be preserved.
    let temp_db = NamedTempFile::new().unwrap();
    let sessions_dir = TempDir::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    // Index parent and one child
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

    // Child A is linked
    let child_a_id = format!("claude-subagent::{CHILD_DUP_PARENT_ID}::asolo-aaaaaaaaaaaaaaaa");
    let subagent = load_subagent(temp_db.path(), CHILD_DUP_PARENT_ID, "toolu_agent_300")
        .unwrap()
        .expect("parent subagent should exist");
    assert_eq!(
        subagent.child_session_id.as_deref(),
        Some(child_a_id.as_str())
    );

    // Delete the child's source (now retained/missing)
    fs::remove_file(
        sessions_dir
            .path()
            .join(CHILD_DUP_PARENT_ID)
            .join("subagents")
            .join("agent-asolo-aaaaaaaaaaaaaaaa.jsonl"),
    )
    .unwrap();

    // Rescan with zero present children
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    // Link must still point to the retained child
    let subagent_after = load_subagent(temp_db.path(), CHILD_DUP_PARENT_ID, "toolu_agent_300")
        .unwrap()
        .expect("parent subagent should exist");
    assert_eq!(
        subagent_after.child_session_id.as_deref(),
        Some(child_a_id.as_str()),
        "zero present candidates must preserve existing retained link"
    );

    // Retained child must still be loadable
    let retained = load_session(temp_db.path(), &child_a_id)
        .unwrap()
        .expect("retained child must be loadable");
    assert!(retained.is_subagent);
}

#[test]
fn multiple_candidates_retract_automatic_link() {
    // Test the _ => arm: when multiple present children have the same name,
    // any existing automatic link must be retracted (set to NULL).
    let temp_db = NamedTempFile::new().unwrap();
    let sessions_dir = TempDir::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    // Index parent and one child
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

    // Child A is linked
    let child_a_id = format!("claude-subagent::{CHILD_DUP_PARENT_ID}::asolo-aaaaaaaaaaaaaaaa");
    let subagent = load_subagent(temp_db.path(), CHILD_DUP_PARENT_ID, "toolu_agent_300")
        .unwrap()
        .expect("parent subagent should exist");
    assert_eq!(
        subagent.child_session_id.as_deref(),
        Some(child_a_id.as_str())
    );

    // Add the second same-named child (now multiple candidates)
    copy_child_dup_one(sessions_dir.path(), "agent-asolo-bbbbbbbbbbbbbbbb.jsonl");
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    // Link must be retracted (set to NULL) because of ambiguity
    let subagent_after = load_subagent(temp_db.path(), CHILD_DUP_PARENT_ID, "toolu_agent_300")
        .unwrap()
        .expect("parent subagent should exist");
    assert_eq!(
        subagent_after.child_session_id, None,
        "multiple candidates must retract automatic link"
    );
}

#[test]
fn parent_reparse_with_newly_declared_duplicate_name_clears_restored_link() {
    // CRITICAL: `link_claude_subagents_tx`'s parent-side loop (indexer.rs, the
    // `!parsed.session.is_subagent` branch) only warned when the SAME parse
    // declares a name more than once ("solo" used by 2 subagents"); it never
    // cleared `child_session_id`. That was safe only while content
    // replacement always wiped the row first. Now that ordinary reparses use
    // `replace_session_contents_preserving_links_tx`, a stale link restored
    // onto that exact subagent id survives the warn-and-continue unless the
    // guard actively clears it.
    //
    // Scenario: parent P has one teammate "solo" (call id t1) linked to
    // child A. P is resumed and now declares a SECOND, genuinely new "solo"
    // invocation (call id t2) in the same transcript. Reparsing P restores
    // t1 -> A (t1 survived with no new explicit target), then the
    // newly-ambiguous declaration must actively unlink both t1 and t2.
    let temp_db = NamedTempFile::new().unwrap();
    let sessions_dir = TempDir::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    const PARENT_ID: &str = "6b1c2d30-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let parent_path = sessions_dir.path().join(format!("{PARENT_ID}.jsonl"));
    fs::write(
        &parent_path,
        format!(
            "{{\"type\":\"user\",\"timestamp\":\"2026-08-01T09:00:00.000Z\",\"cwd\":\"/tmp/project\",\"sessionId\":\"{PARENT_ID}\",\"version\":\"2.1.220\",\"message\":{{\"content\":\"Run solo\"}}}}\n\
             {{\"type\":\"assistant\",\"timestamp\":\"2026-08-01T09:00:01.000Z\",\"cwd\":\"/tmp/project\",\"sessionId\":\"{PARENT_ID}\",\"version\":\"2.1.220\",\"message\":{{\"content\":[{{\"type\":\"tool_use\",\"id\":\"toolu_g1\",\"name\":\"Agent\",\"input\":{{\"description\":\"Run solo\",\"prompt\":\"Do the job\",\"name\":\"solo\",\"subagent_type\":\"general-purpose\",\"model\":\"sonnet\"}}}}]}}}}\n\
             {{\"type\":\"user\",\"timestamp\":\"2026-08-01T09:00:02.000Z\",\"cwd\":\"/tmp/project\",\"sessionId\":\"{PARENT_ID}\",\"version\":\"2.1.220\",\"toolUseResult\":{{\"status\":\"teammate_spawned\",\"teammate_id\":\"solo@session-{PARENT_ID}\",\"agent_id\":\"solo@session-{PARENT_ID}\",\"agent_type\":\"general-purpose\",\"model\":\"sonnet\",\"name\":\"solo\",\"team_name\":\"session-{PARENT_ID}\",\"is_splitpane\":false,\"plan_mode_required\":false}},\"message\":{{\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_g1\",\"content\":[{{\"type\":\"text\",\"text\":\"Spawned successfully.\"}}]}}]}}}}\n"
        ),
    )
    .unwrap();

    let subagents_dir = sessions_dir.path().join(PARENT_ID).join("subagents");
    fs::create_dir_all(&subagents_dir).unwrap();
    fs::write(
        subagents_dir.join("agent-asolo-aaaaaaaaaaaaaaaa.jsonl"),
        format!(
            "{{\"parentUuid\":null,\"isSidechain\":true,\"agentId\":\"asolo-aaaaaaaaaaaaaaaa\",\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"Do the job\"}},\"timestamp\":\"2026-08-01T09:00:03.000Z\",\"cwd\":\"/tmp/project\",\"sessionId\":\"{PARENT_ID}\",\"version\":\"2.1.220\"}}\n\
             {{\"parentUuid\":\"msg-1\",\"isSidechain\":true,\"agentId\":\"asolo-aaaaaaaaaaaaaaaa\",\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":\"Done\"}},\"timestamp\":\"2026-08-01T09:00:04.000Z\",\"cwd\":\"/tmp/project\",\"sessionId\":\"{PARENT_ID}\",\"version\":\"2.1.220\"}}\n"
        ),
    )
    .unwrap();

    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    let child_a_id = format!("claude-subagent::{PARENT_ID}::asolo-aaaaaaaaaaaaaaaa");
    let subagent = load_subagent(temp_db.path(), PARENT_ID, "toolu_g1")
        .unwrap()
        .expect("parent subagent should exist");
    assert_eq!(
        subagent.child_session_id.as_deref(),
        Some(child_a_id.as_str()),
        "the sole declared, sole present child should link"
    );

    // Resume the parent: append a second, genuinely new "solo" invocation
    // (t2) to the SAME transcript, making the name ambiguous by declaration.
    let mut contents = fs::read_to_string(&parent_path).unwrap();
    contents.push_str(&format!(
        "{{\"type\":\"assistant\",\"timestamp\":\"2026-08-01T09:01:00.000Z\",\"cwd\":\"/tmp/project\",\"sessionId\":\"{PARENT_ID}\",\"version\":\"2.1.220\",\"message\":{{\"content\":[{{\"type\":\"tool_use\",\"id\":\"toolu_g2\",\"name\":\"Agent\",\"input\":{{\"description\":\"Run solo again\",\"prompt\":\"Do another job\",\"name\":\"solo\",\"subagent_type\":\"general-purpose\",\"model\":\"sonnet\"}}}}]}}}}\n\
         {{\"type\":\"user\",\"timestamp\":\"2026-08-01T09:01:01.000Z\",\"cwd\":\"/tmp/project\",\"sessionId\":\"{PARENT_ID}\",\"version\":\"2.1.220\",\"toolUseResult\":{{\"status\":\"teammate_spawned\",\"teammate_id\":\"solo2@session-{PARENT_ID}\",\"agent_id\":\"solo2@session-{PARENT_ID}\",\"agent_type\":\"general-purpose\",\"model\":\"sonnet\",\"name\":\"solo\",\"team_name\":\"session-{PARENT_ID}\",\"is_splitpane\":false,\"plan_mode_required\":false}},\"message\":{{\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_g2\",\"content\":[{{\"type\":\"text\",\"text\":\"Spawned successfully.\"}}]}}]}}}}\n"
    ));
    fs::write(&parent_path, contents).unwrap();
    indexer.index_claude_sessions(sessions_dir.path()).unwrap();

    let t1_after = load_subagent(temp_db.path(), PARENT_ID, "toolu_g1")
        .unwrap()
        .expect("t1 subagent row should still exist");
    assert_eq!(
        t1_after.child_session_id, None,
        "restored link to t1 must be actively cleared once \"solo\" is declared twice"
    );
    let t2_after = load_subagent(temp_db.path(), PARENT_ID, "toolu_g2")
        .unwrap()
        .expect("t2 subagent row should exist");
    assert_eq!(
        t2_after.child_session_id, None,
        "the newly declared duplicate must also be unlinked"
    );
}
