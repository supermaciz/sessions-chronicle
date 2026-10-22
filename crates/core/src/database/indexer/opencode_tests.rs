//! Focused coordinator tests: retry-safe indexing, source ownership
//! arbitration, per-source cleanup, and deterministic failure injection.
//!
//! All cache inspection goes through raw SQL (never production inspection
//! APIs), and failure injection uses SQLite abort triggers plus the
//! [`super::opencode::force_unavailable`] metadata-probe seam — never sleeps
//! for WAL timing.
//!
//! The Task 2 fixture builder [`create_v2_db`] is reused for the canonical V2
//! schema bootstrap and for the fixture's fixed `ses_v2_system_only` /
//! `ses_v2_corrupt_user` sessions. The remaining local builders insert custom
//! session IDs and content that the fixed fixture cannot express (cross-source
//! ownership arbitration needs the *same* id in several sources with different
//! bodies, and the WAL tests need in-place content mutation).

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::parsers::opencode::test_support::create_v2_db;

use super::SessionIndexer;
use super::opencode::force_unavailable;

// ---------------------------------------------------------------------------
// Cache inspection helpers (SQL only).
// ---------------------------------------------------------------------------

fn owner_generation(db: &Connection, id: &str) -> Option<String> {
    db.query_row(
        "SELECT generation FROM opencode_session_sources WHERE session_id = ?1",
        [id],
        |row| row.get(0),
    )
    .ok()
}

fn cached_ids(db: &Connection) -> Vec<String> {
    let mut stmt = db
        .prepare("SELECT id FROM sessions WHERE tool = 'opencode' ORDER BY id")
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap()
}

fn cached_body(db: &Connection, id: &str) -> Option<String> {
    db.query_row(
        "SELECT content FROM messages WHERE session_id = ?1 ORDER BY message_index ASC LIMIT 1",
        [id],
        |row| row.get(0),
    )
    .ok()
}

fn cached_tool_output(db: &Connection, id: &str) -> Option<String> {
    db.query_row(
        "SELECT output_text FROM tool_calls WHERE session_id = ?1 ORDER BY rowid ASC LIMIT 1",
        [id],
        |row| row.get(0),
    )
    .ok()
}

/// `(db_mtime_ns, db_size, wal_mtime_ns, wal_size, reader_generation)`.
type StoredFp = (i64, i64, Option<i64>, Option<i64>, i64);

fn stored_fingerprint(db: &Connection, path: &Path) -> Option<StoredFp> {
    db.query_row(
        "SELECT db_mtime_ns, db_size, wal_mtime_ns, wal_size, reader_generation
         FROM opencode_source_fingerprints WHERE db_path = ?1",
        [path.to_str().unwrap()],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )
    .ok()
}

fn pinned_at(db: &Connection, id: &str) -> Option<i64> {
    db.query_row(
        "SELECT pinned_at FROM sessions WHERE id = ?1",
        [id],
        |row| row.get(0),
    )
    .ok()
}

// ---------------------------------------------------------------------------
// Source builders.
// ---------------------------------------------------------------------------

fn v2_user_data(text: &str) -> String {
    serde_json::json!({ "text": text, "time": { "created": 1_712_000_000_000_i64 } }).to_string()
}

/// Create a V2 source database with one user message per session.
///
/// Reuses the Task 2 fixture builder for the schema bootstrap, then clears the
/// fixed fixture sessions (whose IDs and content the cross-source arbitration
/// tests cannot express) and inserts the caller's own sessions.
fn create_v2_source(path: &Path, sessions: &[(&str, &str)]) -> Connection {
    let conn = create_v2_db(path);
    conn.execute_batch("DELETE FROM session_message; DELETE FROM session_v2;")
        .unwrap();
    for (id, text) in sessions {
        insert_v2_session(&conn, id, text);
    }
    conn
}

fn insert_v2_session(conn: &Connection, id: &str, user_text: &str) {
    conn.execute(
        "INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model,
                                  tokens_input, tokens_output, tokens_reasoning, tokens_cache_read,
                                  tokens_cache_write, time_created, time_updated)
         VALUES (?1, 'prj', '/tmp/proj', NULL, NULL, '0.0.0', ?2, NULL, 0, 0, 0, 0, 0, 1712000000000, 1712000000000)",
        rusqlite::params![id, format!("slug-{id}")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
         VALUES (?1, ?2, 'user', 1, 1712000000000, 1712000000000, ?3)",
        rusqlite::params![format!("msg-{id}"), id, v2_user_data(user_text)],
    )
    .unwrap();
}

/// A session with a user message and an assistant tool call whose output text
/// can be mutated independently of message count and timestamps.
fn insert_v2_tool_session(conn: &Connection, id: &str, tool_output: &str) {
    conn.execute(
        "INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model,
                                  tokens_input, tokens_output, tokens_reasoning, tokens_cache_read,
                                  tokens_cache_write, time_created, time_updated)
         VALUES (?1, 'prj', '/tmp/proj', NULL, NULL, '0.0.0', ?2, NULL, 0, 0, 0, 0, 0, 1712000000000, 1712000000000)",
        rusqlite::params![id, format!("slug-{id}")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
         VALUES (?1, ?2, 'user', 1, 1712000000000, 1712000000000, ?3)",
        rusqlite::params![format!("msg-{id}-user"), id, v2_user_data("run the tool")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
         VALUES (?1, ?2, 'assistant', 2, 1712000000000, 1712000000000, ?3)",
        rusqlite::params![format!("msg-{id}-asst"), id, v2_tool_data(tool_output),],
    )
    .unwrap();
}

fn v2_tool_data(output: &str) -> String {
    serde_json::json!({
        "agent": "primary",
        "content": [{
            "type": "tool",
            "id": "call_1",
            "name": "bash",
            "state": {
                "status": "completed",
                "input": { "command": "ls" },
                "content": [{ "type": "text", "text": output }]
            },
            "time": { "created": 1_712_000_000_000_i64 }
        }],
        "time": { "created": 1_712_000_000_000_i64 }
    })
    .to_string()
}

fn set_v2_tool_output(path: &Path, id: &str, output: &str) {
    let conn = Connection::open(path).unwrap();
    conn.execute(
        "UPDATE session_message SET data = ?1
         WHERE session_id = ?2 AND type = 'assistant'",
        rusqlite::params![v2_tool_data(output), id],
    )
    .unwrap();
}

fn file_stamp_of(path: &Path) -> (i64, i64) {
    let meta = std::fs::metadata(path).unwrap();
    let mtime_ns = meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    (
        i64::try_from(mtime_ns).unwrap(),
        i64::try_from(meta.len()).unwrap(),
    )
}

fn wal_path_of(db: &Path) -> PathBuf {
    let mut wal = db.as_os_str().to_os_string();
    wal.push("-wal");
    wal.into()
}

/// Seed a fingerprint row whose DB/WAL stamps match the source files but whose
/// reader generation is stale, simulating a pre-migration cache.
fn seed_fingerprint(indexer: &SessionIndexer, path: &Path, reader_generation: i64) {
    let (db_mtime_ns, db_size) = file_stamp_of(path);
    let wal = wal_path_of(path);
    let (wal_mtime_ns, wal_size) = if wal.exists() {
        let (mtime_ns, size) = file_stamp_of(&wal);
        (Some(mtime_ns), Some(size))
    } else {
        (None, None)
    };
    indexer
        .db
        .execute(
            "INSERT INTO opencode_source_fingerprints
                (db_path, db_mtime_ns, db_size, wal_mtime_ns, wal_size, reader_generation)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                path.to_str().unwrap(),
                db_mtime_ns,
                db_size,
                wal_mtime_ns,
                wal_size,
                reader_generation,
            ],
        )
        .unwrap();
}

fn create_v1_source(path: &Path, sessions: &[(&str, &str)]) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (
            id TEXT PRIMARY KEY, directory TEXT, title TEXT, parent_id TEXT,
            time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL
         );
         CREATE TABLE message (
            id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL, data TEXT NOT NULL
         );
         CREATE TABLE part (
            id TEXT PRIMARY KEY, message_id TEXT NOT NULL, data TEXT NOT NULL
         );",
    )
    .unwrap();
    for (id, text) in sessions {
        insert_v1_session(&conn, id, text);
    }
    conn
}

fn insert_v1_session(conn: &Connection, id: &str, text: &str) {
    conn.execute(
        "INSERT INTO session (id, directory, title, parent_id, time_created, time_updated)
         VALUES (?1, '/tmp/proj', NULL, NULL, 1712000000000, 1712000000000)",
        [id],
    )
    .unwrap();
    let msg_id = format!("msg-{id}");
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, data)
         VALUES (?1, ?2, 1712000000000, '{\"role\":\"user\"}')",
        rusqlite::params![msg_id, id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, data) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            format!("part-{id}"),
            msg_id,
            serde_json::json!({ "type": "text", "order": 1, "text": text }).to_string(),
        ],
    )
    .unwrap();
}

/// Create a single legacy JSON session under `root`, returning the session file path.
fn create_json_session(root: &Path, id: &str, text: &str) -> PathBuf {
    let session_path = root
        .join("session")
        .join("project-a")
        .join(format!("{id}.json"));
    std::fs::create_dir_all(session_path.parent().unwrap()).unwrap();
    std::fs::write(
        &session_path,
        serde_json::json!({
            "id": id,
            "directory": "/tmp/proj",
            "time": { "created": 1_712_000_000_000_i64, "updated": 1_712_000_000_000_i64 }
        })
        .to_string(),
    )
    .unwrap();

    let msg_id = format!("msg-{id}");
    let msg_path = root.join("message").join(id).join(format!("{msg_id}.json"));
    std::fs::create_dir_all(msg_path.parent().unwrap()).unwrap();
    std::fs::write(
        &msg_path,
        serde_json::json!({
            "id": msg_id,
            "sessionID": id,
            "role": "user",
            "time": { "created": 1_712_000_000_000_i64 }
        })
        .to_string(),
    )
    .unwrap();

    let part_path = root
        .join("part")
        .join(&msg_id)
        .join(format!("part-{id}.json"));
    std::fs::create_dir_all(part_path.parent().unwrap()).unwrap();
    std::fs::write(
        &part_path,
        serde_json::json!({ "id": format!("part-{id}"), "order": 1, "type": "text", "text": text })
            .to_string(),
    )
    .unwrap();

    session_path
}

fn new_indexer() -> (tempfile::TempDir, SessionIndexer) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cache.db");
    let indexer = SessionIndexer::new(&db_path).unwrap();
    (dir, indexer)
}

fn install_abort_trigger(indexer: &SessionIndexer, id: &str) {
    let name = id.replace('-', "_");
    indexer
        .db
        .execute_batch(&format!(
            "CREATE TRIGGER abort_{name} BEFORE INSERT ON sessions
             WHEN NEW.id = '{id}'
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;"
        ))
        .unwrap();
}

fn drop_abort_trigger(indexer: &SessionIndexer, id: &str) {
    let name = id.replace('-', "_");
    indexer
        .db
        .execute_batch(&format!("DROP TRIGGER IF EXISTS abort_{name};"))
        .unwrap();
}

fn install_delete_abort_trigger(indexer: &SessionIndexer, id: &str) {
    let name = id.replace('-', "_");
    indexer
        .db
        .execute_batch(&format!(
            "CREATE TRIGGER abort_delete_{name} BEFORE DELETE ON sessions
             WHEN OLD.id = '{id}'
             BEGIN SELECT RAISE(ABORT, 'injected cleanup failure'); END;"
        ))
        .unwrap();
}

fn drop_delete_abort_trigger(indexer: &SessionIndexer, id: &str) {
    let name = id.replace('-', "_");
    indexer
        .db
        .execute_batch(&format!("DROP TRIGGER IF EXISTS abort_delete_{name};"))
        .unwrap();
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

#[test]
fn opencode_v2_wins_mixed_sources() {
    let source_dir = tempfile::tempdir().unwrap();
    let json_root = source_dir.path().join("json");
    let v2_path = source_dir.path().join("v2.db");
    let v1_path = source_dir.path().join("v1.db");

    create_v2_source(
        &v2_path,
        &[("ses_shared", "v2 body"), ("ses_v2_only", "v2 only")],
    );
    create_v1_source(
        &v1_path,
        &[("ses_shared", "v1 body"), ("ses_v1_only", "v1 only")],
    );
    create_json_session(&json_root, "ses_shared", "json body");
    create_json_session(&json_root, "ses_json_only", "json only");

    for db_order in [
        vec![v2_path.clone(), v1_path.clone()],
        vec![v1_path.clone(), v2_path.clone()],
    ] {
        let (_dir, mut indexer) = new_indexer();
        let count = indexer
            .index_opencode_sessions(&json_root, &db_order)
            .unwrap();

        assert_eq!(count, 4);
        assert_eq!(
            owner_generation(&indexer.db, "ses_shared").as_deref(),
            Some("v2")
        );
        assert_eq!(
            cached_body(&indexer.db, "ses_shared").as_deref(),
            Some("v2 body")
        );
        assert_eq!(
            owner_generation(&indexer.db, "ses_v1_only").as_deref(),
            Some("v1")
        );
        assert_eq!(
            owner_generation(&indexer.db, "ses_json_only").as_deref(),
            Some("json")
        );
    }
}

#[test]
fn opencode_unchanged_owner_blocks_changed_json() {
    let source_dir = tempfile::tempdir().unwrap();
    let json_root = source_dir.path().join("json");
    let v1_path = source_dir.path().join("v1.db");
    create_v1_source(&v1_path, &[("ses_shared", "v1 body")]);
    create_json_session(&json_root, "ses_shared", "json body");

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions(&json_root, &[v1_path.clone()])
        .unwrap();
    assert_eq!(
        owner_generation(&indexer.db, "ses_shared").as_deref(),
        Some("v1")
    );

    // Change the JSON representation; the unchanged V1 owner must still win.
    create_json_session(&json_root, "ses_shared", "json body changed");
    let second = indexer
        .index_opencode_sessions_incremental(&json_root, &[v1_path.clone()])
        .unwrap();
    assert!(second.skipped > 0, "unchanged V1 source should be skipped");

    assert_eq!(
        owner_generation(&indexer.db, "ses_shared").as_deref(),
        Some("v1")
    );
    assert_eq!(
        cached_body(&indexer.db, "ses_shared").as_deref(),
        Some("v1 body")
    );
}

#[test]
fn opencode_v1_owner_promotes_to_v2() {
    let source_dir = tempfile::tempdir().unwrap();
    let v1_path = source_dir.path().join("v1.db");
    let v2_path = source_dir.path().join("v2.db");
    create_v1_source(&v1_path, &[("ses_shared", "v1 body")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions(&source_dir.path().join("missing"), &[v1_path.clone()])
        .unwrap();
    assert_eq!(
        owner_generation(&indexer.db, "ses_shared").as_deref(),
        Some("v1")
    );

    // A V2 candidate appears; the retained V1 owner must be promoted.
    create_v2_source(&v2_path, &[("ses_shared", "v2 body")]);
    let second = indexer
        .index_opencode_sessions_incremental(
            &source_dir.path().join("missing"),
            &[v1_path.clone(), v2_path.clone()],
        )
        .unwrap();
    assert!(second.indexed > 0);

    assert_eq!(
        owner_generation(&indexer.db, "ses_shared").as_deref(),
        Some("v2")
    );
    assert_eq!(
        cached_body(&indexer.db, "ses_shared").as_deref(),
        Some("v2 body")
    );
}

#[test]
fn opencode_two_channels_prune_independently() {
    let source_dir = tempfile::tempdir().unwrap();
    let channel_a = source_dir.path().join("opencode.db");
    let channel_b = source_dir.path().join("opencode-dev.db");
    create_v2_source(&channel_a, &[("ses_a", "a")]);
    create_v2_source(&channel_b, &[("ses_b", "b")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(
            &source_dir.path().join("missing"),
            &[channel_a.clone(), channel_b.clone()],
        )
        .unwrap();
    assert_eq!(cached_ids(&indexer.db), vec!["ses_a", "ses_b"]);

    // Re-read only channel A: channel B's sessions must not be pruned even
    // though channel B was not discovered this run.
    let second = indexer
        .index_opencode_sessions_incremental(
            &source_dir.path().join("missing"),
            &[channel_a.clone()],
        )
        .unwrap();
    assert_eq!(second.removed, 0);
    assert_eq!(cached_ids(&indexer.db), vec!["ses_a", "ses_b"]);
}

#[test]
fn opencode_failure_retries_without_source_write() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    create_v2_source(
        &v2_path,
        &[("ses_a", "good a"), ("ses_failed", "previous good body")],
    );

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    let before_failure = stored_fingerprint(&indexer.db, &v2_path).expect("fingerprint committed");

    // Mutate the source so a reindex is required, then make the second session's
    // cache write fail.
    set_v2_tool_output(&v2_path, "ses_failed", "never");
    {
        let conn = Connection::open(&v2_path).unwrap();
        conn.execute(
            "UPDATE session_message SET data = ?1 WHERE session_id = 'ses_failed' AND type = 'user'",
            [v2_user_data("new body")],
        )
        .unwrap();
    }

    install_abort_trigger(&indexer, "ses_failed");
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();

    assert_eq!(
        cached_body(&indexer.db, "ses_failed").as_deref(),
        Some("previous good body")
    );
    assert_eq!(
        stored_fingerprint(&indexer.db, &v2_path),
        Some(before_failure)
    );

    // Remove the injected fault, do not touch the source, and retry.
    drop_abort_trigger(&indexer, "ses_failed");
    let second_run = indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert!(second_run.indexed > 0);
    assert_eq!(
        cached_body(&indexer.db, "ses_failed").as_deref(),
        Some("new body")
    );
}

#[test]
fn opencode_late_persistence_failure_does_not_commit_fingerprint() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    create_v2_source(&v2_path, &[("ses_a", "a"), ("ses_b", "b")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    let before_failure = stored_fingerprint(&indexer.db, &v2_path).expect("fingerprint committed");

    // Mutate both sessions so the whole source reindexes; fail only the later one.
    {
        let conn = Connection::open(&v2_path).unwrap();
        conn.execute(
            "UPDATE session_message SET data = ?1 WHERE session_id = 'ses_a' AND type = 'user'",
            [v2_user_data("a v2")],
        )
        .unwrap();
        conn.execute(
            "UPDATE session_message SET data = ?1 WHERE session_id = 'ses_b' AND type = 'user'",
            [v2_user_data("b v2")],
        )
        .unwrap();
    }

    install_abort_trigger(&indexer, "ses_b");
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();

    // The earlier session succeeded, but the later failure must leave the
    // previous fingerprint intact and preserve the failed session's old cache.
    assert_eq!(
        stored_fingerprint(&indexer.db, &v2_path),
        Some(before_failure)
    );
    assert_eq!(cached_body(&indexer.db, "ses_a").as_deref(), Some("a v2"));
    assert_eq!(cached_body(&indexer.db, "ses_b").as_deref(), Some("b"));
}

#[test]
fn opencode_discovery_error_retains_cache() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    create_v2_source(&v2_path, &[("ses_keep", "keep body")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert_eq!(cached_ids(&indexer.db), vec!["ses_keep"]);

    // Simulate an inaccessible discovery directory: the metadata probe reports
    // PermissionDenied (Unavailable), so the source must be retained.
    force_unavailable(&v2_path);
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert_eq!(cached_ids(&indexer.db), vec!["ses_keep"]);
}

#[test]
fn opencode_last_db_removal_prunes() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    create_v2_source(&v2_path, &[("ses_gone", "gone")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert_eq!(cached_ids(&indexer.db), vec!["ses_gone"]);

    std::fs::remove_file(&v2_path).unwrap();
    let second = indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert_eq!(second.removed, 1);
    assert!(cached_ids(&indexer.db).is_empty());
}

#[test]
fn opencode_last_db_removal_prunes_when_path_omitted() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    create_v2_source(&v2_path, &[("ses_gone", "gone")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert_eq!(cached_ids(&indexer.db), vec!["ses_gone"]);

    // Delete the file AND omit it from `db_paths`: the owner is undiscovered,
    // so only the per-file metadata probe can confirm its absence.
    std::fs::remove_file(&v2_path).unwrap();
    let second = indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[])
        .unwrap();
    assert_eq!(second.removed, 1);
    assert!(cached_ids(&indexer.db).is_empty());
}

#[test]
fn opencode_undiscovered_owner_retained_when_unavailable() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    create_v2_source(&v2_path, &[("ses_keep", "keep body")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert_eq!(cached_ids(&indexer.db), vec!["ses_keep"]);

    // The cached owner's path is undiscovered (`db_paths = []`) and its
    // metadata probe reports Unavailable: it must still be retained.
    force_unavailable(&v2_path);
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[])
        .unwrap();
    assert_eq!(cached_ids(&indexer.db), vec!["ses_keep"]);
}

#[test]
fn opencode_json_owner_retained_when_storage_root_unavailable() {
    let source_dir = tempfile::tempdir().unwrap();
    let json_root = source_dir.path().join("json");
    create_json_session(&json_root, "ses_json", "json body");

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&json_root, &[])
        .unwrap();
    assert_eq!(cached_ids(&indexer.db), vec!["ses_json"]);

    // The JSON storage root becomes unreadable and no DB paths are configured:
    // the cached JSON owner must still be retained via its per-file presence.
    force_unavailable(&json_root);
    indexer
        .index_opencode_sessions_incremental(&json_root, &[])
        .unwrap();
    assert_eq!(cached_ids(&indexer.db), vec!["ses_json"]);
}

#[test]
fn opencode_ineligible_does_not_fall_back() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    let v1_path = source_dir.path().join("v1.db");

    // The V2 source is the Task 2 fixture, which contains `ses_v2_system_only`
    // (no user message → ineligible); V1 supplies the same id with a user.
    create_v2_db(&v2_path);
    create_v1_source(&v1_path, &[("ses_v2_system_only", "v1 fallback body")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions(
            &source_dir.path().join("missing"),
            &[v2_path.clone(), v1_path.clone()],
        )
        .unwrap();

    // The ineligible V2 session reserves priority and is pruned; the V1 row
    // must not reintroduce it.
    assert!(
        !cached_ids(&indexer.db)
            .iter()
            .any(|id| id == "ses_v2_system_only")
    );
}

#[test]
fn opencode_ineligible_not_reintroduced_on_unchanged_run() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    let json_root = source_dir.path().join("json");

    // V2 DB A holds X with no user message (ineligible); a lower-priority JSON
    // source also holds X.
    let conn = create_v2_db(&v2_path);
    conn.execute_batch("DELETE FROM session_message; DELETE FROM session_v2;")
        .unwrap();
    conn.execute(
        "INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model,
                                  tokens_input, tokens_output, tokens_reasoning, tokens_cache_read,
                                  tokens_cache_write, time_created, time_updated)
         VALUES ('ses_x', 'prj', '/tmp/proj', NULL, NULL, '0.0.0', 'slug-x', NULL,
                 0, 0, 0, 0, 0, 1712000000000, 1712000000000)",
        [],
    )
    .unwrap();
    drop(conn);
    create_json_session(&json_root, "ses_x", "json body");

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions(&json_root, &[v2_path.clone()])
        .unwrap();
    assert!(!cached_ids(&indexer.db).iter().any(|id| id == "ses_x"));

    // Run 2: A is unchanged, so it must still reserve X's priority and keep it
    // pruned rather than letting the JSON source reintroduce it.
    let second = indexer
        .index_opencode_sessions_incremental(&json_root, &[v2_path.clone()])
        .unwrap();
    assert!(!cached_ids(&indexer.db).iter().any(|id| id == "ses_x"));
    assert_eq!(second.indexed, 0);
}

#[test]
fn opencode_unchanged_v1_falls_back_after_v2_removed() {
    let source_dir = tempfile::tempdir().unwrap();
    let v1_path = source_dir.path().join("v1.db");
    let v2_path = source_dir.path().join("v2.db");
    create_v1_source(&v1_path, &[("ses_shared", "v1 body")]);
    create_v2_source(&v2_path, &[("ses_shared", "v2 body")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions(
            &source_dir.path().join("missing"),
            &[v1_path.clone(), v2_path.clone()],
        )
        .unwrap();
    assert_eq!(
        owner_generation(&indexer.db, "ses_shared").as_deref(),
        Some("v2")
    );
    assert_eq!(
        cached_body(&indexer.db, "ses_shared").as_deref(),
        Some("v2 body")
    );

    // Delete the V2 DB; the V1 DB is unchanged.
    std::fs::remove_file(&v2_path).unwrap();

    // Run 2: after the V2 owner's confirmed absence, the unchanged V1 source
    // must fall back and re-own X instead of letting it disappear.
    indexer
        .index_opencode_sessions_incremental(
            &source_dir.path().join("missing"),
            &[v1_path.clone(), v2_path.clone()],
        )
        .unwrap();

    assert_eq!(
        owner_generation(&indexer.db, "ses_shared").as_deref(),
        Some("v1")
    );
    assert_eq!(
        cached_body(&indexer.db, "ses_shared").as_deref(),
        Some("v1 body")
    );
}

#[test]
fn opencode_json_falls_back_when_sqlite_owner_confirmed_missing() {
    let source_dir = tempfile::tempdir().unwrap();
    let json_root = source_dir.path().join("json");
    let v2_path = source_dir.path().join("v2.db");

    // Run 1: JSON owns X and commits its file fingerprint.
    create_json_session(&json_root, "ses_shared", "json body");
    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&json_root, &[])
        .unwrap();
    assert_eq!(
        owner_generation(&indexer.db, "ses_shared").as_deref(),
        Some("json")
    );

    // Run 2: a V2 source takes ownership, leaving the JSON fingerprint intact.
    create_v2_source(&v2_path, &[("ses_shared", "v2 body")]);
    indexer
        .index_opencode_sessions_incremental(&json_root, &[v2_path.clone()])
        .unwrap();
    assert_eq!(
        owner_generation(&indexer.db, "ses_shared").as_deref(),
        Some("v2")
    );

    // Delete the V2 DB, leaving the JSON file (and its fingerprint) untouched.
    std::fs::remove_file(&v2_path).unwrap();

    // Run 3: the confirmed-missing V2 owner must fall back to the fresh JSON
    // file even though its fingerprint still matches (should_reindex == false).
    indexer
        .index_opencode_sessions_incremental(&json_root, &[v2_path.clone()])
        .unwrap();

    assert_eq!(
        owner_generation(&indexer.db, "ses_shared").as_deref(),
        Some("json")
    );
    assert_eq!(
        cached_body(&indexer.db, "ses_shared").as_deref(),
        Some("json body")
    );
}

#[test]
fn opencode_corrupt_user_retains_previous_owner() {
    let source_dir = tempfile::tempdir().unwrap();
    let v1_path = source_dir.path().join("v1.db");
    let v2_path = source_dir.path().join("v2.db");
    create_v1_source(&v1_path, &[("ses_v2_corrupt_user", "v1 body")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions(&source_dir.path().join("missing"), &[v1_path.clone()])
        .unwrap();

    // The V2 fixture's `ses_v2_corrupt_user` must be a retained failure, not an
    // erase-and-fall-back to the V1 owner.
    create_v2_db(&v2_path);

    indexer
        .index_opencode_sessions_incremental(
            &source_dir.path().join("missing"),
            &[v1_path.clone(), v2_path.clone()],
        )
        .unwrap();

    assert_eq!(
        owner_generation(&indexer.db, "ses_v2_corrupt_user").as_deref(),
        Some("v1")
    );
    assert_eq!(
        cached_body(&indexer.db, "ses_v2_corrupt_user").as_deref(),
        Some("v1 body")
    );
}

#[test]
fn opencode_unreadable_user_time_retains_cached_owner() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    create_v2_source(&v2_path, &[("ses_time", "good body")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert_eq!(cached_ids(&indexer.db), vec!["ses_time"]);

    // Make the only user row's `time_created` out of range so the reader cannot
    // establish the timestamp. This must surface as an incomplete transcript
    // (retained failure), not a "no user messages" ineligibility that prunes
    // the last good cache.
    {
        let conn = Connection::open(&v2_path).unwrap();
        conn.execute(
            "UPDATE session_message SET time_created = 9223372036854775807
             WHERE session_id = 'ses_time' AND type = 'user'",
            [],
        )
        .unwrap();
    }

    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();

    assert_eq!(cached_ids(&indexer.db), vec!["ses_time"]);
    assert_eq!(
        cached_body(&indexer.db, "ses_time").as_deref(),
        Some("good body")
    );
    assert_eq!(
        owner_generation(&indexer.db, "ses_time").as_deref(),
        Some("v2")
    );
}

#[test]
fn opencode_cleanup_failure_skips_fingerprint_commit() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    create_v2_source(&v2_path, &[("ses_a", "a"), ("ses_gone", "gone")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    let before = stored_fingerprint(&indexer.db, &v2_path).expect("fingerprint committed");

    // Remove `ses_gone` from the source so it must be pruned, then make that
    // prune delete fail deterministically.
    {
        let conn = Connection::open(&v2_path).unwrap();
        conn.execute(
            "DELETE FROM session_message WHERE session_id = 'ses_gone'",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM session_v2 WHERE id = 'ses_gone'", [])
            .unwrap();
    }
    install_delete_abort_trigger(&indexer, "ses_gone");

    let result = indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();

    // A cleanup failure must prevent the fingerprint commit, not abort the run.
    assert_eq!(stored_fingerprint(&indexer.db, &v2_path), Some(before));
    assert!(result.errors > 0, "cleanup failure must be recorded");
    assert!(cached_ids(&indexer.db).iter().any(|id| id == "ses_gone"));

    // Remove the fault and retry: cleanup succeeds and the fingerprint commits.
    drop_delete_abort_trigger(&indexer, "ses_gone");
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert!(!cached_ids(&indexer.db).iter().any(|id| id == "ses_gone"));
    assert_ne!(stored_fingerprint(&indexer.db, &v2_path), Some(before));
}

#[test]
fn opencode_unavailable_v2_blocks_lower_priority() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    let v1_path = source_dir.path().join("v1.db");
    create_v2_source(&v2_path, &[("ses_shared", "v2 body")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();

    // A newly discovered lower-priority V1 source must not overwrite an
    // unavailable V2 owner.
    create_v1_source(&v1_path, &[("ses_shared", "v1 body")]);
    force_unavailable(&v2_path);
    indexer
        .index_opencode_sessions_incremental(
            &source_dir.path().join("missing"),
            &[v2_path.clone(), v1_path.clone()],
        )
        .unwrap();

    assert_eq!(
        owner_generation(&indexer.db, "ses_shared").as_deref(),
        Some("v2")
    );
    assert_eq!(
        cached_body(&indexer.db, "ses_shared").as_deref(),
        Some("v2 body")
    );
}

#[test]
fn opencode_snapshot_fingerprint_detects_concurrent_write() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");

    // WAL mode with a kept-open writer so later writes stay in the -wal and the
    // main DB mtime does not advance.
    let writer = create_v2_source(&v2_path, &[]);
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    insert_v2_tool_session(&writer, "ses_mut", "first output");

    let (_dir, mut indexer) = new_indexer();
    let first = indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert_eq!(first.indexed, 1);
    assert_eq!(
        cached_tool_output(&indexer.db, "ses_mut").as_deref(),
        Some("first output")
    );

    // Mutate tool output without changing message count or timestamps; only the
    // WAL changes, so the fingerprint must still detect it.
    let db_mtime_before = std::fs::metadata(&v2_path).unwrap().modified().unwrap();
    set_v2_tool_output(&v2_path, "ses_mut", "second output");
    let db_mtime_after = std::fs::metadata(&v2_path).unwrap().modified().unwrap();
    assert_eq!(db_mtime_after, db_mtime_before, "WAL must hold the change");

    let second = indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert!(second.indexed > 0, "WAL-only change must trigger a reindex");
    assert_eq!(
        cached_tool_output(&indexer.db, "ses_mut").as_deref(),
        Some("second output")
    );
    drop(writer);
}

#[test]
fn opencode_wal_lifecycle_reindexes() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");

    // Start without WAL: fingerprint records `wal = None`.
    let conn = create_v2_source(&v2_path, &[("ses_w", "wal body")]);
    drop(conn);

    let (_dir, mut indexer) = new_indexer();
    let first = indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert_eq!(first.indexed, 1);
    assert_eq!(stored_fingerprint(&indexer.db, &v2_path).unwrap().2, None);

    // Switch to WAL and add a session with a kept-open writer: the -wal file
    // appears and changes, so the source must reindex.
    let writer = Connection::open(&v2_path).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    insert_v2_session(&writer, "ses_w2", "second");
    let second = indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert!(second.indexed > 0);
    assert_eq!(cached_ids(&indexer.db), vec!["ses_w", "ses_w2"]);
    assert!(
        stored_fingerprint(&indexer.db, &v2_path)
            .unwrap()
            .2
            .is_some()
    );

    // Checkpoint and close the writer: the -wal disappears, another reindex.
    writer
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    drop(writer);
    let third = indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert!(
        third.indexed > 0,
        "WAL disappearance must trigger a reindex"
    );
}

#[test]
fn opencode_migration_reindexes_once() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    create_v2_source(&v2_path, &[("ses_mig", "migrated body")]);

    let (_dir, mut indexer) = new_indexer();
    // Simulate a pre-migration cache: the source was last read by an older
    // reader (generation 1) but the DB/WAL stamps still match. Only the reader
    // generation mismatch must force the one-time reindex.
    seed_fingerprint(&indexer, &v2_path, 1);

    let first = indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert!(
        first.indexed > 0,
        "generation mismatch must force a reindex"
    );

    let second = indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert_eq!(second.indexed, 0);
    assert!(second.skipped > 0);

    let unchanged_third_run = indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    assert_eq!(unchanged_third_run.indexed, 0);
}

#[test]
fn opencode_generation_two_cache_refreshes_unchanged_source_once() {
    let source_dir = tempfile::tempdir().unwrap();
    let source = source_dir.path().join("v2.db");
    drop(create_v2_source(&source, &[("ses_refresh", "source body")]));
    let source_bytes = std::fs::read(&source).unwrap();
    let source_stamp = file_stamp_of(&source);
    let (_dir, mut indexer) = new_indexer();
    let missing = source_dir.path().join("missing");
    indexer
        .index_opencode_sessions_incremental(&missing, &[source.clone()])
        .unwrap();
    // Simulate an already indexed generation-2 cache, not a source edit.
    indexer
        .db
        .execute(
            "UPDATE messages SET content = 'stale projection' WHERE session_id = 'ses_refresh'",
            [],
        )
        .unwrap();
    indexer
        .db
        .execute(
            "UPDATE opencode_source_fingerprints SET reader_generation = 2",
            [],
        )
        .unwrap();

    let refreshed = indexer
        .index_opencode_sessions_incremental(&missing, &[source.clone()])
        .unwrap();
    assert_eq!(
        refreshed.indexed, 1,
        "generation-2 cache must be reprojected"
    );
    assert_eq!(
        cached_body(&indexer.db, "ses_refresh").as_deref(),
        Some("source body")
    );
    let second = indexer
        .index_opencode_sessions_incremental(&missing, &[source.clone()])
        .unwrap();
    assert_eq!(second.indexed, 0);
    assert_eq!(second.skipped, 1);
    assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
    assert_eq!(file_stamp_of(&source), source_stamp);
}

#[test]
fn opencode_replacement_preserves_pin() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    create_v2_source(&v2_path, &[("ses_pin", "pin body")]);

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();
    indexer
        .db
        .execute(
            "UPDATE sessions SET pinned_at = 1234 WHERE id = 'ses_pin'",
            [],
        )
        .unwrap();

    // Force a re-read by mutating the source, then replace; the pin survives.
    {
        let conn = Connection::open(&v2_path).unwrap();
        conn.execute(
            "UPDATE session_message SET data = ?1 WHERE session_id = 'ses_pin' AND type = 'user'",
            [v2_user_data("pin body v2")],
        )
        .unwrap();
    }
    indexer
        .index_opencode_sessions_incremental(&source_dir.path().join("missing"), &[v2_path.clone()])
        .unwrap();

    assert_eq!(pinned_at(&indexer.db, "ses_pin"), Some(1234));
    assert_eq!(
        cached_body(&indexer.db, "ses_pin").as_deref(),
        Some("pin body v2")
    );
}

/// Overwrite `path` with zeroes of the same length and restore its mtime, so
/// its fingerprint is unchanged but opening it as SQLite would fail.
fn corrupt_keeping_fingerprint(path: &Path) {
    let modified = std::fs::metadata(path).unwrap().modified().unwrap();
    let len = std::fs::metadata(path).unwrap().len() as usize;
    std::fs::write(path, vec![0u8; len]).unwrap();
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
}

#[test]
fn opencode_unchanged_source_is_not_opened() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    drop(create_v2_source(&v2_path, &[("ses_quiet", "quiet body")]));
    let missing = source_dir.path().join("missing");

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&missing, &[v2_path.clone()])
        .unwrap();
    let enumerated: Vec<String> = {
        let mut stmt = indexer
            .db
            .prepare("SELECT session_id || '=' || generation FROM opencode_source_sessions")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    assert_eq!(enumerated, ["ses_quiet=v2"]);

    // Any attempt to open the source would now fail and be reported.
    corrupt_keeping_fingerprint(&v2_path);
    let second = indexer
        .index_opencode_sessions_incremental(&missing, &[v2_path.clone()])
        .unwrap();

    assert_eq!(second.errors, 0, "an unchanged source must not be opened");
    assert_eq!(second.skipped, 1);
    assert_eq!(second.removed, 0);
    assert_eq!(
        cached_body(&indexer.db, "ses_quiet").as_deref(),
        Some("quiet body")
    );
}

#[test]
fn opencode_inconsistent_wal_fingerprint_rereads_source() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    drop(create_v2_source(&v2_path, &[("ses_wal", "wal body")]));
    let missing = source_dir.path().join("missing");

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&missing, &[v2_path.clone()])
        .unwrap();
    indexer
        .db
        .execute(
            "UPDATE opencode_source_fingerprints SET wal_mtime_ns = 1, wal_size = NULL",
            [],
        )
        .unwrap();

    let second = indexer
        .index_opencode_sessions_incremental(&missing, &[v2_path.clone()])
        .unwrap();

    assert_eq!(second.errors, 0);
    assert_eq!(second.indexed, 1, "a damaged fingerprint row means re-read");
    let third = indexer
        .index_opencode_sessions_incremental(&missing, &[v2_path.clone()])
        .unwrap();
    assert_eq!(third.indexed, 0);
}

#[test]
fn opencode_owner_load_failure_keeps_cache() {
    let source_dir = tempfile::tempdir().unwrap();
    let v2_path = source_dir.path().join("v2.db");
    drop(create_v2_source(&v2_path, &[("ses_kept", "kept body")]));
    let missing = source_dir.path().join("missing");

    let (_dir, mut indexer) = new_indexer();
    indexer
        .index_opencode_sessions_incremental(&missing, &[v2_path.clone()])
        .unwrap();
    indexer
        .db
        .execute_batch("DROP TABLE opencode_session_sources")
        .unwrap();

    // The run reports the error instead of aborting the caller, and prunes
    // nothing even though the only source has disappeared.
    std::fs::remove_file(&v2_path).unwrap();
    let stats = indexer
        .index_opencode_sessions_incremental(&missing, &[v2_path.clone()])
        .unwrap();

    assert_eq!(stats.errors, 1);
    assert_eq!(stats.removed, 0);
    assert_eq!(
        cached_body(&indexer.db, "ses_kept").as_deref(),
        Some("kept body")
    );
}
