use std::collections::VecDeque;

use super::SessionIndexer;

fn write_opencode_json_session(root: &std::path::Path, id: &str) -> std::path::PathBuf {
    let metadata = root
        .join("session")
        .join("project")
        .join(format!("{id}.json"));
    let message = root.join("message").join(id).join("message-1.json");
    let part = root.join("part").join("message-1").join("part-1.json");
    std::fs::create_dir_all(metadata.parent().unwrap()).unwrap();
    std::fs::create_dir_all(message.parent().unwrap()).unwrap();
    std::fs::create_dir_all(part.parent().unwrap()).unwrap();
    std::fs::write(
        &metadata,
        format!(r#"{{"id":"{id}","time":{{"created":1700000000000,"updated":1700000000000}}}}"#),
    )
    .unwrap();
    std::fs::write(
        message,
        r#"{"id":"message-1","role":"user","time":{"created":1700000000000}}"#,
    )
    .unwrap();
    std::fs::write(
        &part,
        r#"{"id":"part-1","type":"text","order":1,"text":"retained transcript"}"#,
    )
    .unwrap();
    part
}

#[test]
fn opencode_missing_former_part_file_is_diagnostic_and_retains_content() {
    // A former part file is an indexed dependency.  Removing it while the
    // session metadata stays unchanged must be an incomplete read, not a
    // successful but shorter replacement transcript.
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("storage");
    let index_db = temp.path().join("index.db");
    let part = write_opencode_json_session(&root, "json-retained");
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer
        .index_opencode_sessions_incremental(&root, &[])
        .unwrap();
    let before = crate::database::load_session(&index_db, "json-retained")
        .unwrap()
        .unwrap();
    std::fs::remove_file(part).unwrap();

    let result = indexer
        .index_opencode_sessions_incremental(&root, &[])
        .unwrap();
    let retained = crate::database::load_session(&index_db, "json-retained")
        .unwrap()
        .unwrap();
    assert!(result.errors > 0);
    assert!(!retained.source.missing);
    assert_eq!(retained.message_count, before.message_count);
    assert_eq!(retained.source.mtime_ns, before.source.mtime_ns);
}

#[test]
fn opencode_disappeared_part_beats_added_dependency_change() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("storage");
    let index_db = temp.path().join("index.db");
    let part = write_opencode_json_session(&root, "part-precedence");
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer
        .index_opencode_sessions_incremental(&root, &[])
        .unwrap();
    let before = crate::database::load_session(&index_db, "part-precedence")
        .unwrap()
        .unwrap();
    std::fs::remove_file(part).unwrap();
    std::fs::write(
        root.join("part").join("message-1").join("part-2.json"),
        r#"{"id":"part-2","type":"text","order":1,"text":"new transcript"}"#,
    )
    .unwrap();

    let result = indexer
        .index_opencode_sessions_incremental(&root, &[])
        .unwrap();
    let retained = crate::database::load_session(&index_db, "part-precedence")
        .unwrap()
        .unwrap();
    assert!(result.errors > 0);
    assert_eq!(retained.message_count, before.message_count);
    assert_eq!(retained.first_prompt, before.first_prompt);
}

#[test]
fn opencode_missing_message_directory_is_diagnostic_and_retains_content() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("storage");
    let index_db = temp.path().join("index.db");
    write_opencode_json_session(&root, "missing-message-dir");
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer
        .index_opencode_sessions_incremental(&root, &[])
        .unwrap();
    let before = crate::database::load_session(&index_db, "missing-message-dir")
        .unwrap()
        .unwrap();
    std::fs::remove_dir_all(root.join("message").join("missing-message-dir")).unwrap();

    let result = indexer
        .index_opencode_sessions_incremental(&root, &[])
        .unwrap();
    let retained = crate::database::load_session(&index_db, "missing-message-dir")
        .unwrap()
        .unwrap();
    assert!(result.errors > 0);
    assert!(!retained.source.missing);
    assert_eq!(retained.message_count, before.message_count);
    assert_eq!(retained.source.mtime_ns, before.source.mtime_ns);
}

#[test]
fn opencode_completed_json_walk_marks_missing_metadata_session_retained() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("storage");
    let index_db = temp.path().join("index.db");
    write_opencode_json_session(&root, "removed-metadata");
    let metadata = root
        .join("session")
        .join("project")
        .join("removed-metadata.json");
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer.index_opencode_sessions(&root, &[]).unwrap();
    std::fs::remove_file(metadata).unwrap();

    indexer.index_opencode_sessions(&root, &[]).unwrap();
    let retained = crate::database::load_session(&index_db, "removed-metadata")
        .unwrap()
        .unwrap();
    assert!(retained.source.missing);
    assert_eq!(retained.source.scope, None);
}

#[test]
fn opencode_malformed_metadata_makes_json_enumeration_incomplete() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("storage");
    let index_db = temp.path().join("index.db");
    write_opencode_json_session(&root, "bad-metadata");
    let metadata = root
        .join("session")
        .join("project")
        .join("bad-metadata.json");
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer.index_opencode_sessions(&root, &[]).unwrap();
    std::fs::write(metadata, "not json").unwrap();

    indexer.index_opencode_sessions(&root, &[]).unwrap();
    let retained = crate::database::load_session(&index_db, "bad-metadata")
        .unwrap()
        .unwrap();
    assert!(!retained.source.missing);
}

#[test]
fn opencode_sqlite_completed_enumeration_marks_only_removed_record_missing() {
    // This catches the old global stale-prune behavior: a completed SQLite
    // identity snapshot must reconcile its own database scope and retain the
    // transcript when a row disappears.
    let temp = tempfile::tempdir().unwrap();
    let source_db = temp.path().join("source.db");
    let index_db = temp.path().join("index.db");
    let json_root = temp.path().join("storage");
    std::fs::create_dir_all(&json_root).unwrap();
    let db = super::tests::create_opencode_sqlite_db(&source_db);
    super::tests::insert_opencode_session(&db, "retained", 1_700_000_000_000);
    let mut indexer = SessionIndexer::new(&index_db).unwrap();

    indexer
        .index_opencode_sessions(&json_root, std::slice::from_ref(&source_db))
        .unwrap();
    db.execute("DELETE FROM session WHERE id = 'retained'", [])
        .unwrap();
    indexer
        .index_opencode_sessions(&json_root, std::slice::from_ref(&source_db))
        .unwrap();

    let retained = crate::database::load_session(&index_db, "retained")
        .unwrap()
        .unwrap();
    assert!(retained.source.missing);
    assert_eq!(retained.source.scope.as_deref(), source_db.to_str());
    assert_eq!(
        (retained.source.mtime_ns, retained.source.size),
        (None, None)
    );
}

#[test]
fn opencode_unavailable_database_does_not_reconcile_another_database_scope() {
    // A failure to enumerate database A must not make its formerly indexed
    // rows look absent merely because database B is healthy.
    let temp = tempfile::tempdir().unwrap();
    let source_a = temp.path().join("a.db");
    let source_b = temp.path().join("b.db");
    let index_db = temp.path().join("index.db");
    let json_root = temp.path().join("storage");
    std::fs::create_dir_all(&json_root).unwrap();
    let db_a = super::tests::create_opencode_sqlite_db(&source_a);
    super::tests::insert_opencode_session(&db_a, "from-a", 1_700_000_000_000);
    let db_b = super::tests::create_opencode_sqlite_db(&source_b);
    super::tests::insert_opencode_session(&db_b, "from-b", 1_700_000_100_000);
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer
        .index_opencode_sessions(&json_root, &[source_a.clone(), source_b.clone()])
        .unwrap();

    drop(db_a);
    std::fs::remove_file(&source_a).unwrap();
    indexer
        .index_opencode_sessions(&json_root, &[source_a, source_b])
        .unwrap();

    let a = crate::database::load_session(&index_db, "from-a")
        .unwrap()
        .unwrap();
    let b = crate::database::load_session(&index_db, "from-b")
        .unwrap()
        .unwrap();
    assert!(!a.source.missing);
    assert!(!b.source.missing);
}

#[test]
fn opencode_database_deletion_reconciles_only_the_matching_database_scope() {
    let temp = tempfile::tempdir().unwrap();
    let source_a = temp.path().join("a.db");
    let source_b = temp.path().join("b.db");
    let index_db = temp.path().join("index.db");
    let json_root = temp.path().join("storage");
    std::fs::create_dir_all(&json_root).unwrap();
    let db_a = super::tests::create_opencode_sqlite_db(&source_a);
    super::tests::insert_opencode_session(&db_a, "kept-a", 1_700_000_000_000);
    let db_b = super::tests::create_opencode_sqlite_db(&source_b);
    super::tests::insert_opencode_session(&db_b, "removed-b", 1_700_000_100_000);
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer
        .index_opencode_sessions(&json_root, &[source_a.clone(), source_b.clone()])
        .unwrap();
    db_b.execute("DELETE FROM session WHERE id = 'removed-b'", [])
        .unwrap();

    indexer
        .index_opencode_sessions(&json_root, &[source_a, source_b])
        .unwrap();
    assert!(
        !crate::database::load_session(&index_db, "kept-a")
            .unwrap()
            .unwrap()
            .source
            .missing
    );
    assert!(
        crate::database::load_session(&index_db, "removed-b")
            .unwrap()
            .unwrap()
            .source
            .missing
    );
}

#[test]
fn opencode_database_enumeration_backfills_legacy_scope_when_content_is_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let source_db = temp.path().join("source.db");
    let index_db = temp.path().join("index.db");
    let json_root = temp.path().join("storage");
    std::fs::create_dir_all(&json_root).unwrap();
    let source = super::tests::create_opencode_sqlite_db(&source_db);
    super::tests::insert_opencode_session(&source, "legacy-scope", 1_700_000_000_000);
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer
        .index_opencode_sessions_incremental(&json_root, std::slice::from_ref(&source_db))
        .unwrap();
    indexer
        .db
        .execute(
            "UPDATE sessions SET source_scope = NULL WHERE id = 'legacy-scope'",
            [],
        )
        .unwrap();

    let result = indexer
        .index_opencode_sessions_incremental(&json_root, std::slice::from_ref(&source_db))
        .unwrap();
    let restored = crate::database::load_session(&index_db, "legacy-scope")
        .unwrap()
        .unwrap();
    assert!(result.skipped > 0);
    assert_eq!(restored.source.scope.as_deref(), source_db.to_str());
}

#[test]
fn opencode_sqlite_identity_query_failure_keeps_prior_source_state() {
    let temp = tempfile::tempdir().unwrap();
    let source_db = temp.path().join("source.db");
    let index_db = temp.path().join("index.db");
    let json_root = temp.path().join("storage");
    std::fs::create_dir_all(&json_root).unwrap();
    let source = super::tests::create_opencode_sqlite_db(&source_db);
    super::tests::insert_opencode_session(&source, "query-failure", 1_700_000_000_000);
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer
        .index_opencode_sessions(&json_root, std::slice::from_ref(&source_db))
        .unwrap();
    source.execute("DROP TABLE session", []).unwrap();

    indexer
        .index_opencode_sessions(&json_root, std::slice::from_ref(&source_db))
        .unwrap();
    assert!(
        !crate::database::load_session(&index_db, "query-failure")
            .unwrap()
            .unwrap()
            .source
            .missing
    );
}

#[test]
fn opencode_sqlite_undecodable_identity_keeps_database_scope_incomplete() {
    let temp = tempfile::tempdir().unwrap();
    let source_db = temp.path().join("source.db");
    let index_db = temp.path().join("index.db");
    let json_root = temp.path().join("storage");
    std::fs::create_dir_all(&json_root).unwrap();
    let source = super::tests::create_opencode_sqlite_db(&source_db);
    super::tests::insert_opencode_session(&source, "decode-failure", 1_700_000_000_000);
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer
        .index_opencode_sessions(&json_root, std::slice::from_ref(&source_db))
        .unwrap();
    source
        .execute(
            "INSERT INTO session (id, time_created, time_updated) VALUES (?1, ?2, ?3)",
            rusqlite::params![vec![0xff_u8], 1_700_000_000_000_i64, 1_700_000_000_000_i64],
        )
        .unwrap();

    indexer
        .index_opencode_sessions(&json_root, std::slice::from_ref(&source_db))
        .unwrap();
    assert!(
        !crate::database::load_session(&index_db, "decode-failure")
            .unwrap()
            .unwrap()
            .source
            .missing
    );
}

#[test]
fn opencode_recreated_sqlite_record_with_parse_failure_stays_missing_and_retained() {
    let temp = tempfile::tempdir().unwrap();
    let source_db = temp.path().join("source.db");
    let index_db = temp.path().join("index.db");
    let json_root = temp.path().join("storage");
    std::fs::create_dir_all(&json_root).unwrap();
    let source = super::tests::create_opencode_sqlite_db(&source_db);
    super::tests::insert_opencode_session(&source, "recreated", 1_700_000_000_000);
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer
        .index_opencode_sessions(&json_root, std::slice::from_ref(&source_db))
        .unwrap();
    let before = crate::database::load_session(&index_db, "recreated")
        .unwrap()
        .unwrap();
    source
        .execute("DELETE FROM session WHERE id = 'recreated'", [])
        .unwrap();
    indexer
        .index_opencode_sessions(&json_root, std::slice::from_ref(&source_db))
        .unwrap();
    source
        .execute("DELETE FROM message WHERE session_id = 'recreated'", [])
        .unwrap();
    source.execute("DELETE FROM part", []).unwrap();
    super::tests::insert_opencode_session(&source, "recreated", 1_700_000_100_000);
    source
        .execute(
            "UPDATE message SET data = 'not json' WHERE session_id = 'recreated'",
            [],
        )
        .unwrap();

    indexer
        .index_opencode_sessions(&json_root, std::slice::from_ref(&source_db))
        .unwrap();
    let retained = crate::database::load_session(&index_db, "recreated")
        .unwrap()
        .unwrap();
    assert!(retained.source.missing);
    assert_eq!(retained.message_count, before.message_count);
}

fn assert_missing_lifecycle(
    assistant: &str,
    fixture: &str,
    fts_term: &str,
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
    let fts_count: i64 = indexer
        .db
        .query_row(
            "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH ?1",
            [fts_term],
            |row| row.get(0),
        )
        .unwrap_or(0);
    assert!(fts_count > 0);

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
    let retained_fts_count: i64 = indexer
        .db
        .query_row(
            "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH ?1",
            [fts_term],
            |row| row.get(0),
        )
        .unwrap_or(0);
    assert_eq!(retained_fts_count, fts_count);

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
        "refactor",
        |indexer, root, errors| indexer.index_claude_sessions_internal(root, true, errors),
    );
}

#[test]
fn codex_missing_source_survives_incremental_and_returns() {
    assert_missing_lifecycle(
        "codex",
        "codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl",
        "Summarize",
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

fn assert_ineligible_replacement_prunes_only_the_present_owner(
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
    let path = root.join(if assistant == "codex" {
        "rollout-ineligible.jsonl"
    } else {
        "ineligible.jsonl"
    });
    std::fs::copy(crate::fixture_path(first_fixture), &path).unwrap();
    let db_path = temp.path().join("index.db");
    let mut indexer = SessionIndexer::new(&db_path).unwrap();
    let mut errors = VecDeque::new();
    index(&mut indexer, &root, &mut errors).unwrap();
    let old_id: String = indexer
        .db
        .query_row("SELECT id FROM sessions", [], |row| row.get(0))
        .unwrap();

    std::fs::copy(crate::fixture_path(replacement_fixture), &path).unwrap();
    index(&mut indexer, &root, &mut errors).unwrap();
    let present_id: String = indexer
        .db
        .query_row(
            "SELECT id FROM sessions WHERE source_missing = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(old_id, present_id);

    std::fs::write(&path, b"").unwrap();
    let result = index(&mut indexer, &root, &mut errors).unwrap();
    assert_eq!(result.removed, 1);
    assert_eq!(result.errors, 0);
    assert!(
        crate::database::load_session(&db_path, &old_id)
            .unwrap()
            .unwrap()
            .source
            .missing
    );
    assert!(
        crate::database::load_session(&db_path, &present_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn ineligible_claude_replacement_prunes_only_the_present_owner() {
    assert_ineligible_replacement_prunes_only_the_present_owner(
        "claude",
        "claude_sessions/sample-session.jsonl",
        "claude_sessions/tool-calls-session.jsonl",
        |indexer, root, errors| indexer.index_claude_sessions_internal(root, true, errors),
    );
}

#[test]
fn ineligible_codex_replacement_prunes_only_the_present_owner() {
    assert_ineligible_replacement_prunes_only_the_present_owner(
        "codex",
        "codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl",
        "codex_sessions/2026/02/18/rollout-2026-02-18T10-00-00-codex-tools-session.jsonl",
        |indexer, root, errors| indexer.index_codex_sessions_internal(root, true, errors),
    );
}

#[test]
fn failed_codex_return_keeps_the_retained_session_missing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("codex");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("rollout-sample.jsonl");
    std::fs::copy(
        crate::fixture_path(
            "codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl",
        ),
        &path,
    )
    .unwrap();
    let db_path = temp.path().join("index.db");
    let mut indexer = SessionIndexer::new(&db_path).unwrap();
    let mut errors = VecDeque::new();
    indexer
        .index_codex_sessions_internal(&root, true, &mut errors)
        .unwrap();
    let id: String = indexer
        .db
        .query_row("SELECT id FROM sessions", [], |row| row.get(0))
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    indexer
        .index_codex_sessions_internal(&root, true, &mut errors)
        .unwrap();
    std::fs::write(&path, b"").unwrap();
    let failed = indexer
        .index_codex_sessions_internal(&root, true, &mut errors)
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
fn unavailable_configured_codex_root_reports_not_found() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("index.db");
    let mut indexer = SessionIndexer::new(&db_path).unwrap();
    let mut sources = crate::session_sources::SessionSources::resolve(Some(temp.path()));
    sources.codex_dir = temp.path().join("missing-codex-root");

    let result = indexer.index_all_incremental(&sources).unwrap();
    let codex = result
        .per_source
        .iter()
        .find(|source| source.assistant == crate::models::AiAssistant::Codex)
        .unwrap();
    assert_eq!(codex.status, crate::models::SourceStatus::NotFound);
    assert_eq!(codex.display_path, sources.codex_dir.display().to_string());
}

fn assert_insert_failure_protects_the_scope(
    assistant: &str,
    fixture: &str,
    other_fixture: &str,
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
    let parsed_path = root.join(file_name);
    let absent_path = root.join(if assistant == "codex" {
        "rollout-absent.jsonl"
    } else {
        "absent.jsonl"
    });
    std::fs::copy(crate::fixture_path(fixture), &parsed_path).unwrap();
    std::fs::copy(crate::fixture_path(other_fixture), &absent_path).unwrap();
    let db_path = temp.path().join("index.db");
    let mut indexer = SessionIndexer::new(&db_path).unwrap();
    let mut errors = VecDeque::new();
    index(&mut indexer, &root, &mut errors).unwrap();
    let id: String = indexer
        .db
        .query_row(
            "SELECT id FROM sessions WHERE file_path = ?1",
            [absent_path.display().to_string()],
            |row| row.get(0),
        )
        .unwrap();

    std::fs::remove_file(&absent_path).unwrap();
    indexer
        .db
        .execute_batch(
            "CREATE TRIGGER fail_session_update BEFORE UPDATE ON sessions
             BEGIN SELECT RAISE(ABORT, 'forced insert failure'); END;",
        )
        .unwrap();
    std::fs::write(
        &parsed_path,
        format!("{}\n", std::fs::read_to_string(&parsed_path).unwrap()),
    )
    .unwrap();
    let result = index(&mut indexer, &root, &mut errors).unwrap();
    assert!(result.errors > 0);
    assert!(
        !crate::database::load_session(&db_path, &id)
            .unwrap()
            .unwrap()
            .source
            .missing
    );
}

#[test]
fn claude_insert_failure_protects_other_present_scope_rows() {
    assert_insert_failure_protects_the_scope(
        "claude",
        "claude_sessions/sample-session.jsonl",
        "claude_sessions/tool-calls-session.jsonl",
        |indexer, root, errors| indexer.index_claude_sessions_internal(root, true, errors),
    );
}

#[test]
fn codex_insert_failure_protects_other_present_scope_rows() {
    assert_insert_failure_protects_the_scope(
        "codex",
        "codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl",
        "codex_sessions/2026/02/18/rollout-2026-02-18T10-00-00-codex-tools-session.jsonl",
        |indexer, root, errors| indexer.index_codex_sessions_internal(root, true, errors),
    );
}

#[test]
fn unowned_ineligible_claude_file_protects_the_scope() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("claude");
    std::fs::create_dir(&root).unwrap();
    let retained_path = root.join("retained.jsonl");
    std::fs::copy(
        crate::fixture_path("claude_sessions/sample-session.jsonl"),
        &retained_path,
    )
    .unwrap();
    let db_path = temp.path().join("index.db");
    let mut indexer = SessionIndexer::new(&db_path).unwrap();
    let mut errors = VecDeque::new();
    indexer
        .index_claude_sessions_internal(&root, true, &mut errors)
        .unwrap();
    let retained_id: String = indexer
        .db
        .query_row("SELECT id FROM sessions", [], |row| row.get(0))
        .unwrap();

    std::fs::remove_file(&retained_path).unwrap();
    std::fs::write(root.join("unowned-ineligible.jsonl"), b"").unwrap();
    let result = indexer
        .index_claude_sessions_internal(&root, true, &mut errors)
        .unwrap();

    assert!(result.errors > 0);
    assert!(
        !crate::database::load_session(&db_path, &retained_id)
            .unwrap()
            .unwrap()
            .source
            .missing
    );
}

use super::tests::{vibe_plain_messages, vibe_task_messages, write_vibe_session_dir};

/// Build the canonical parent/child Vibe fixture and index it once.
/// Returns `(sessions_dir, index_db, indexer, parent_dir, child_dir)`.
fn indexed_vibe_tree(
    temp: &tempfile::TempDir,
) -> (
    std::path::PathBuf,
    std::path::PathBuf,
    SessionIndexer,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let sessions_dir = temp.path().join("vibe");
    let parent_dir = sessions_dir.join("session_parent");
    let child_dir = parent_dir.join("agents").join("comique_20260101_000100");
    write_vibe_session_dir(
        &parent_dir,
        "parent-session",
        None,
        &vibe_task_messages("comique", "call_1"),
    );
    write_vibe_session_dir(
        &child_dir,
        "child-session",
        Some("comique"),
        &vibe_plain_messages(),
    );
    let index_db = temp.path().join("index.db");
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer
        .index_vibe_sessions_incremental(&sessions_dir)
        .unwrap();
    (sessions_dir, index_db, indexer, parent_dir, child_dir)
}

fn vibe_missing(index_db: &std::path::Path, id: &str) -> bool {
    crate::database::load_session(index_db, id)
        .unwrap()
        .unwrap()
        .source
        .missing
}

#[test]
fn vibe_moved_child_directory_is_retained_with_its_parent_link() {
    let temp = tempfile::tempdir().unwrap();
    let (sessions_dir, index_db, mut indexer, _parent_dir, child_dir) = indexed_vibe_tree(&temp);
    let before = crate::database::load_session(&index_db, "child-session")
        .unwrap()
        .unwrap();

    std::fs::rename(&child_dir, temp.path().join("saved-child")).unwrap();
    let mut errors = VecDeque::new();
    let stats = indexer
        .index_vibe_sessions_internal(&sessions_dir, true, &mut errors)
        .unwrap();

    let child = crate::database::load_session(&index_db, "child-session")
        .unwrap()
        .unwrap();
    assert!(child.source.missing);
    assert_eq!(child.last_updated, before.last_updated);
    assert_eq!(stats.removed, 0);
    let linked =
        crate::database::load_subagent(&index_db, "parent-session", "parent-session-call_1")
            .unwrap()
            .unwrap();
    assert_eq!(linked.child_session_id.as_deref(), Some("child-session"));
}

#[test]
fn vibe_root_session_disappearance_is_retained_and_stable_across_scans() {
    let temp = tempfile::tempdir().unwrap();
    let (sessions_dir, index_db, mut indexer, parent_dir, _child_dir) = indexed_vibe_tree(&temp);

    std::fs::remove_dir_all(&parent_dir).unwrap();
    let first = indexer
        .index_vibe_sessions_incremental(&sessions_dir)
        .unwrap();
    let detected: i64 = indexer
        .db
        .query_row(
            "SELECT source_missing_detected_at FROM sessions WHERE id = 'parent-session'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    // The parent directory physically covers the child, so both go missing at
    // once and a repeated scan reports no further transition.
    assert_eq!(first.source_state_changes, 2);
    assert!(vibe_missing(&index_db, "parent-session"));
    assert!(vibe_missing(&index_db, "child-session"));

    let second = indexer
        .index_vibe_sessions_incremental(&sessions_dir)
        .unwrap();
    assert_eq!(second.source_state_changes, 0);
    let still: i64 = indexer
        .db
        .query_row(
            "SELECT source_missing_detected_at FROM sessions WHERE id = 'parent-session'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(still, detected);
}

#[test]
fn vibe_returned_directory_clears_missing_after_reindex() {
    let temp = tempfile::tempdir().unwrap();
    let (sessions_dir, index_db, mut indexer, _parent_dir, child_dir) = indexed_vibe_tree(&temp);
    let saved = temp.path().join("saved-child");

    std::fs::rename(&child_dir, &saved).unwrap();
    indexer
        .index_vibe_sessions_incremental(&sessions_dir)
        .unwrap();
    assert!(vibe_missing(&index_db, "child-session"));

    std::fs::rename(&saved, &child_dir).unwrap();
    let restored = indexer
        .index_vibe_sessions_incremental(&sessions_dir)
        .unwrap();

    assert!(!vibe_missing(&index_db, "child-session"));
    assert_eq!(restored.source_state_changes, 1);
}

#[test]
fn vibe_directory_missing_meta_json_is_diagnostic_and_retains_the_session() {
    let temp = tempfile::tempdir().unwrap();
    let (sessions_dir, index_db, mut indexer, _parent_dir, child_dir) = indexed_vibe_tree(&temp);

    std::fs::remove_file(child_dir.join("meta.json")).unwrap();
    let stats = indexer
        .index_vibe_sessions_incremental(&sessions_dir)
        .unwrap();

    assert!(stats.errors > 0);
    assert_eq!(stats.removed, 0);
    assert!(!vibe_missing(&index_db, "child-session"));
}

#[test]
fn vibe_directory_missing_messages_jsonl_is_diagnostic_and_retains_the_session() {
    let temp = tempfile::tempdir().unwrap();
    let (sessions_dir, index_db, mut indexer, _parent_dir, child_dir) = indexed_vibe_tree(&temp);

    std::fs::remove_file(child_dir.join("messages.jsonl")).unwrap();
    let stats = indexer
        .index_vibe_sessions_incremental(&sessions_dir)
        .unwrap();

    assert!(stats.errors > 0);
    assert!(!vibe_missing(&index_db, "child-session"));
}

#[test]
fn vibe_unreadable_agents_directory_never_marks_children_missing() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let (sessions_dir, index_db, mut indexer, parent_dir, _child_dir) = indexed_vibe_tree(&temp);
    let agents_dir = parent_dir.join("agents");

    std::fs::set_permissions(&agents_dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    let stats = indexer
        .index_vibe_sessions_incremental(&sessions_dir)
        .unwrap();
    std::fs::set_permissions(&agents_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(stats.errors > 0);
    assert!(!vibe_missing(&index_db, "child-session"));
    assert!(!vibe_missing(&index_db, "parent-session"));
}

#[test]
fn vibe_missing_child_under_relocated_logical_parent_is_scoped_physically() {
    // A logical parent relationship never authorizes absence: only the
    // physical directory that disappeared covers its own descendants.
    let temp = tempfile::tempdir().unwrap();
    let sessions_dir = temp.path().join("vibe");
    let parent_dir = sessions_dir.join("session_parent");
    let sibling_dir = sessions_dir.join("session_sibling");
    write_vibe_session_dir(
        &parent_dir,
        "parent-session",
        None,
        &vibe_task_messages("comique", "call_1"),
    );
    write_vibe_session_dir(
        &sibling_dir,
        "sibling-session",
        None,
        &vibe_plain_messages(),
    );
    let index_db = temp.path().join("index.db");
    let mut indexer = SessionIndexer::new(&index_db).unwrap();
    indexer
        .index_vibe_sessions_incremental(&sessions_dir)
        .unwrap();

    std::fs::remove_dir_all(&parent_dir).unwrap();
    indexer
        .index_vibe_sessions_incremental(&sessions_dir)
        .unwrap();

    assert!(vibe_missing(&index_db, "parent-session"));
    assert!(!vibe_missing(&index_db, "sibling-session"));
}
