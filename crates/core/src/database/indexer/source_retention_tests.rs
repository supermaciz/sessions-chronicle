use std::collections::VecDeque;

use super::SessionIndexer;

fn assert_missing_lifecycle(
    assistant: &str,
    fixture: &str,
    index: impl Fn(
        &mut SessionIndexer,
        &std::path::Path,
        &mut VecDeque<crate::models::IndexingError>,
    ) -> anyhow::Result<super::IndexingStats>,
) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join(assistant);
    std::fs::create_dir(&root).unwrap();
    let path = root.join(if assistant == "codex" {
        "rollout-sample.jsonl"
    } else {
        "sample.jsonl"
    });
    std::fs::copy(crate::fixture_path(fixture), &path).unwrap();
    let db_path = temp.path().join("index.db");
    let mut indexer = SessionIndexer::new(&db_path).unwrap();
    let mut errors = VecDeque::new();

    index(&mut indexer, &root, &mut errors).unwrap();
    let id: String = indexer
        .db
        .query_row("SELECT id FROM sessions LIMIT 1", [], |row| row.get(0))
        .unwrap();
    let before = crate::database::load_session(&db_path, &id)
        .unwrap()
        .unwrap();
    let transcript_count: i64 = indexer
        .db
        .query_row(
            "SELECT count(*) FROM transcript_items WHERE session_id = ?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();

    let skipped = index(&mut indexer, &root, &mut errors).unwrap();
    assert!(skipped.skipped > 0);
    assert_eq!(skipped.source_state_changes, 0);

    let backup = temp.path().join("saved.jsonl");
    std::fs::rename(&path, &backup).unwrap();
    let missing = index(&mut indexer, &root, &mut errors).unwrap();
    assert_eq!(missing.removed, 0);
    assert_eq!(missing.source_state_changes, 1);
    let retained = crate::database::load_session(&db_path, &id)
        .unwrap()
        .unwrap();
    assert!(retained.source.missing);
    assert_eq!(retained.last_updated, before.last_updated);
    assert_eq!(retained.source.size, before.source.size);
    let retained_transcript_count: i64 = indexer
        .db
        .query_row(
            "SELECT count(*) FROM transcript_items WHERE session_id = ?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained_transcript_count, transcript_count);

    let repeated = index(&mut indexer, &root, &mut errors).unwrap();
    assert_eq!(repeated.source_state_changes, 0);
    let repeated_state = crate::database::load_session(&db_path, &id)
        .unwrap()
        .unwrap();
    assert_eq!(
        repeated_state.source.missing_detected_at,
        retained.source.missing_detected_at
    );

    let full = match assistant {
        "claude" => indexer.index_claude_sessions(&root).map(|_| ()),
        "codex" => indexer.index_codex_sessions(&root).map(|_| ()),
        _ => unreachable!(),
    };
    full.unwrap();
    assert!(
        crate::database::load_session(&db_path, &id)
            .unwrap()
            .unwrap()
            .source
            .missing
    );

    std::fs::rename(&backup, &path).unwrap();
    index(&mut indexer, &root, &mut errors).unwrap();
    let returned = crate::database::load_session(&db_path, &id)
        .unwrap()
        .unwrap();
    assert!(!returned.source.missing);
    assert_eq!(returned.source.missing_detected_at, None);
}

#[test]
fn claude_missing_source_survives_incremental_and_returns() {
    assert_missing_lifecycle(
        "claude",
        "claude_sessions/sample-session.jsonl",
        |indexer, root, errors| indexer.index_claude_sessions_internal(root, true, errors),
    );
}

#[test]
fn codex_missing_source_survives_incremental_and_returns() {
    assert_missing_lifecycle(
        "codex",
        "codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl",
        |indexer, root, errors| indexer.index_codex_sessions_internal(root, true, errors),
    );
}

fn assert_move_and_replacement(
    assistant: &str,
    first_fixture: &str,
    replacement_fixture: &str,
    index: impl Fn(
        &mut SessionIndexer,
        &std::path::Path,
        &mut VecDeque<crate::models::IndexingError>,
    ) -> anyhow::Result<super::IndexingStats>,
) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join(assistant);
    std::fs::create_dir(&root).unwrap();
    let file_name = if assistant == "codex" {
        "rollout-source.jsonl"
    } else {
        "source.jsonl"
    };
    let path = root.join(file_name);
    std::fs::copy(crate::fixture_path(first_fixture), &path).unwrap();
    let db_path = temp.path().join("index.db");
    let mut indexer = SessionIndexer::new(&db_path).unwrap();
    let mut errors = VecDeque::new();
    index(&mut indexer, &root, &mut errors).unwrap();
    let original_id: String = indexer
        .db
        .query_row("SELECT id FROM sessions", [], |row| row.get(0))
        .unwrap();

    let moved_dir = root.join("moved");
    std::fs::create_dir(&moved_dir).unwrap();
    let moved = moved_dir.join(file_name);
    std::fs::rename(&path, &moved).unwrap();
    index(&mut indexer, &root, &mut errors).unwrap();
    let moved_state = crate::database::load_session(&db_path, &original_id)
        .unwrap()
        .unwrap();
    assert_eq!(moved_state.file_path, moved.display().to_string());
    assert!(!moved_state.source.missing);

    std::fs::copy(crate::fixture_path(replacement_fixture), &moved).unwrap();
    index(&mut indexer, &root, &mut errors).unwrap();
    let old = crate::database::load_session(&db_path, &original_id)
        .unwrap()
        .unwrap();
    assert!(old.source.missing);
    let present: i64 = indexer
        .db
        .query_row(
            "SELECT count(*) FROM sessions WHERE file_path = ?1 AND source_missing = 0",
            [moved.display().to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(present, 1);
}

#[test]
fn claude_moves_and_replacements_preserve_the_old_identity() {
    assert_move_and_replacement(
        "claude",
        "claude_sessions/sample-session.jsonl",
        "claude_sessions/tool-calls-session.jsonl",
        |indexer, root, errors| indexer.index_claude_sessions_internal(root, true, errors),
    );
}

#[test]
fn codex_moves_and_replacements_preserve_the_old_identity() {
    assert_move_and_replacement(
        "codex",
        "codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl",
        "codex_sessions/2026/02/18/rollout-2026-02-18T10-00-00-codex-tools-session.jsonl",
        |indexer, root, errors| indexer.index_codex_sessions_internal(root, true, errors),
    );
}

#[test]
fn failed_claude_return_keeps_the_retained_session_missing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("claude");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("sample.jsonl");
    std::fs::copy(
        crate::fixture_path("claude_sessions/sample-session.jsonl"),
        &path,
    )
    .unwrap();
    let db_path = temp.path().join("index.db");
    let mut indexer = SessionIndexer::new(&db_path).unwrap();
    let mut errors = VecDeque::new();
    indexer
        .index_claude_sessions_internal(&root, true, &mut errors)
        .unwrap();
    let id: String = indexer
        .db
        .query_row("SELECT id FROM sessions", [], |row| row.get(0))
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    indexer
        .index_claude_sessions_internal(&root, true, &mut errors)
        .unwrap();
    std::fs::write(&path, "not jsonl\n").unwrap();
    let failed = indexer
        .index_claude_sessions_internal(&root, true, &mut errors)
        .unwrap();
    assert!(failed.errors > 0);
    assert!(
        crate::database::load_session(&db_path, &id)
            .unwrap()
            .unwrap()
            .source
            .missing
    );
}

#[test]
fn codex_roots_reconcile_independently() {
    let temp = tempfile::tempdir().unwrap();
    let codex_home = temp.path().join("codex");
    let sessions = codex_home.join("sessions");
    let archived = codex_home.join("archived_sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::create_dir_all(&archived).unwrap();
    let active_path = sessions.join("rollout-active.jsonl");
    let archived_path = archived.join("rollout-archived.jsonl");
    let fixture = crate::fixture_path(
        "codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl",
    );
    std::fs::copy(&fixture, &active_path).unwrap();
    std::fs::copy(
        crate::fixture_path(
            "codex_sessions/2026/02/18/rollout-2026-02-18T10-00-00-codex-tools-session.jsonl",
        ),
        &archived_path,
    )
    .unwrap();
    let db_path = temp.path().join("index.db");
    let mut indexer = SessionIndexer::new(&db_path).unwrap();
    let mut errors = VecDeque::new();
    indexer
        .index_codex_sessions_internal(&sessions, true, &mut errors)
        .unwrap();
    std::fs::remove_file(&active_path).unwrap();
    let result = indexer
        .index_codex_sessions_internal(&sessions, true, &mut errors)
        .unwrap();
    assert_eq!(result.source_state_changes, 1);
}

#[test]
fn fixture_override_roots_share_a_database_without_cross_scope_absence() {
    let temp = tempfile::tempdir().unwrap();
    let first_root = temp.path().join("first");
    let second_root = temp.path().join("second");
    std::fs::create_dir(&first_root).unwrap();
    std::fs::create_dir(&second_root).unwrap();
    let first_path = first_root.join("first.jsonl");
    let second_path = second_root.join("second.jsonl");
    std::fs::copy(
        crate::fixture_path("claude_sessions/sample-session.jsonl"),
        &first_path,
    )
    .unwrap();
    std::fs::copy(
        crate::fixture_path("claude_sessions/tool-calls-session.jsonl"),
        &second_path,
    )
    .unwrap();
    let db_path = temp.path().join("index.db");
    let mut indexer = SessionIndexer::new(&db_path).unwrap();
    let mut errors = VecDeque::new();
    indexer
        .index_claude_sessions_internal(&first_root, true, &mut errors)
        .unwrap();
    indexer
        .index_claude_sessions_internal(&second_root, true, &mut errors)
        .unwrap();
    let second_id: String = indexer
        .db
        .query_row(
            "SELECT id FROM sessions WHERE file_path = ?1",
            [second_path.display().to_string()],
            |row| row.get(0),
        )
        .unwrap();
    std::fs::remove_file(&first_path).unwrap();
    let result = indexer
        .index_claude_sessions_internal(&first_root, true, &mut errors)
        .unwrap();
    assert_eq!(result.source_state_changes, 1);
    assert!(
        !crate::database::load_session(&db_path, &second_id)
            .unwrap()
            .unwrap()
            .source
            .missing
    );
}
