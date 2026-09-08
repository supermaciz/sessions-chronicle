use super::*;
use crate::models::{AiAssistant, SourceKind};
use rusqlite::{Connection, params};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::database::schema::initialize_database(&conn).unwrap();
    conn
}

fn seed(conn: &Connection, id: &str, assistant: AiAssistant, locator: &str) {
    conn.execute(
        "INSERT INTO sessions
         (id, tool, start_time, message_count, file_path, last_updated, pinned_at,
          source_last_seen_at, source_mtime_ns, source_size)
         VALUES (?1, ?2, 1, 1, ?3, 2, 3, 10, 100, 200)",
        params![id, assistant.to_storage(), locator],
    )
    .unwrap();
}

fn scan(root: &str) -> ScopeScan {
    ScopeScan {
        scope: SourceScope::PathRoot {
            assistant: AiAssistant::ClaudeCode,
            root: PathBuf::from(root),
        },
        complete: true,
        discovered_ids: HashSet::new(),
        protected_locators: HashSet::new(),
        observations: Vec::new(),
    }
}

fn observation(id: &str, locator: &str) -> SourceObservation {
    SourceObservation {
        assistant: AiAssistant::ClaudeCode,
        id: id.into(),
        locator: locator.into(),
        kind: SourceKind::TranscriptFile,
        scope: None,
        fingerprint: Some((300, 400)),
    }
}

#[derive(Debug, PartialEq)]
struct Evidence {
    missing: bool,
    detected: Option<i64>,
    seen: Option<i64>,
    kind: Option<String>,
    mtime: Option<i64>,
    size: Option<i64>,
    scope: Option<String>,
}

fn evidence(conn: &Connection, id: &str) -> Evidence {
    conn.query_row(
        "SELECT source_missing, source_missing_detected_at, source_last_seen_at,
                source_kind, source_mtime_ns, source_size, source_scope
         FROM sessions WHERE id = ?1",
        [id],
        |row| {
            Ok(Evidence {
                missing: row.get(0)?,
                detected: row.get(1)?,
                seen: row.get(2)?,
                kind: row.get(3)?,
                mtime: row.get(4)?,
                size: row.get(5)?,
                scope: row.get(6)?,
            })
        },
    )
    .unwrap()
}

#[test]
fn completed_scope_retains_content_and_first_absence_time() {
    let mut conn = database();
    seed(&conn, "a", AiAssistant::ClaudeCode, "/root/a.jsonl");
    conn.execute_batch(
        "INSERT INTO messages(session_id,message_index,role,content,timestamp)
         VALUES ('a',0,'user','retained needle',1);
         INSERT INTO transcript_items(session_id,item_index,kind,message_index)
         VALUES ('a',0,'message',0);
         INSERT INTO tool_calls(id,session_id,tool_name,status,output_text)
         VALUES ('call','a','read','completed','retained output');
         INSERT INTO subagents(id,session_id,title,prompt)
         VALUES ('agent','a','retained agent','retained prompt');
         INSERT INTO reasoning_attachments(session_id,transcript_item_index,visible_text)
         VALUES ('a',0,'retained reasoning');
         INSERT INTO file_fingerprints(file_path,mtime_ns,size)
         VALUES ('/root/a.jsonl',100,200);",
    )
    .unwrap();
    let scan = scan("/root");
    assert_eq!(reconcile_scope(&mut conn, &scan, 20).unwrap(), 1);
    assert_eq!(reconcile_scope(&mut conn, &scan, 30).unwrap(), 0);
    let state: (bool, i64, i64, i64) = conn
        .query_row(
            "SELECT source_missing,source_missing_detected_at,last_updated,pinned_at
         FROM sessions WHERE id='a'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(state, (true, 20, 2, 3));
    for (query, expected) in [
        (
            "SELECT m.content FROM messages_fts JOIN messages m ON m.id=messages_fts.rowid WHERE messages_fts MATCH 'needle' AND m.session_id='a'",
            "retained needle",
        ),
        (
            "SELECT kind FROM transcript_items WHERE session_id='a' AND item_index=0",
            "message",
        ),
        (
            "SELECT output_text FROM tool_calls WHERE session_id='a' AND id='call'",
            "retained output",
        ),
        (
            "SELECT prompt FROM subagents WHERE session_id='a' AND id='agent'",
            "retained prompt",
        ),
        (
            "SELECT visible_text FROM reasoning_attachments WHERE session_id='a' AND transcript_item_index=0",
            "retained reasoning",
        ),
    ] {
        let value: String = conn.query_row(query, [], |r| r.get(0)).unwrap();
        assert_eq!(value, expected);
    }
    let fingerprint: (i64, i64) = conn
        .query_row(
            "SELECT mtime_ns,size FROM file_fingerprints WHERE file_path='/root/a.jsonl'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(fingerprint, (100, 200));
    assert_eq!(evidence(&conn, "a").seen, Some(10));
}

#[test]
fn incomplete_scope_preserves_old_evidence_but_accepts_observations() {
    let mut conn = database();
    seed(&conn, "a", AiAssistant::ClaudeCode, "/root/a.jsonl");
    seed(&conn, "b", AiAssistant::ClaudeCode, "/root/b.jsonl");
    let old = evidence(&conn, "a");
    let mut scan = scan("/root");
    scan.complete = false;
    assert_eq!(reconcile_scope(&mut conn, &scan, 20).unwrap(), 0);
    assert_eq!(evidence(&conn, "a"), old);
    scan.observations.push(observation("b", "/root/b.jsonl"));
    assert_eq!(reconcile_scope(&mut conn, &scan, 30).unwrap(), 0);
    assert_eq!(evidence(&conn, "a"), old);
    assert_eq!(evidence(&conn, "b").seen, Some(30));
}

#[test]
fn discovery_records_presence_without_clearing_missing() {
    let mut conn = database();
    seed(&conn, "a", AiAssistant::ClaudeCode, "/root/a.jsonl");
    let mut scan = scan("/root");
    scan.discovered_ids.insert("a".into());
    scan.observations.push(observation("a", "/root/a.jsonl"));
    assert_eq!(reconcile_scope(&mut conn, &scan, 20).unwrap(), 0);
    assert!(!evidence(&conn, "a").missing);
    assert_eq!(evidence(&conn, "a").seen, Some(20));
    conn.execute(
        "UPDATE sessions SET source_missing=1,source_missing_detected_at=25",
        [],
    )
    .unwrap();
    assert_eq!(reconcile_scope(&mut conn, &scan, 30).unwrap(), 0);
    let state = evidence(&conn, "a");
    assert!(state.missing);
    assert_eq!(state.detected, Some(25));
    assert_eq!(state.seen, Some(30));
    assert_eq!((state.mtime, state.size), (Some(300), Some(400)));
}

#[test]
fn successful_content_transaction_restores_availability_without_changing_pin_or_sort_time() {
    let mut conn = database();
    seed(&conn, "a", AiAssistant::ClaudeCode, "/root/a.jsonl");
    reconcile_scope(&mut conn, &scan("/root"), 20).unwrap();
    let tx = conn.transaction().unwrap();
    tx.execute("INSERT INTO messages(session_id,message_index,role,content,timestamp) VALUES ('a',0,'user','new content',1)", []).unwrap();
    assert_eq!(
        record_observation_tx(&tx, &observation("a", "/root/a.jsonl"), 30, true).unwrap(),
        1
    );
    assert_eq!(
        record_observation_tx(&tx, &observation("a", "/root/a.jsonl"), 31, true).unwrap(),
        0
    );
    tx.commit().unwrap();
    let state = evidence(&conn, "a");
    assert!(!state.missing);
    assert_eq!(state.detected, None);
    assert_eq!(state.seen, Some(31));
    let times: (i64, i64) = conn
        .query_row(
            "SELECT pinned_at,last_updated FROM sessions WHERE id='a'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(times, (3, 2));
}

#[test]
fn failed_content_transaction_rolls_back_source_content_and_fingerprint() {
    let mut conn = database();
    seed(&conn, "a", AiAssistant::ClaudeCode, "/root/a.jsonl");
    conn.execute_batch("INSERT INTO messages(session_id,message_index,role,content,timestamp) VALUES ('a',0,'user','original',1);
        INSERT INTO file_fingerprints(file_path,mtime_ns,size) VALUES ('/root/a.jsonl',100,200);").unwrap();
    reconcile_scope(&mut conn, &scan("/root"), 20).unwrap();
    let old = evidence(&conn, "a");
    {
        let tx = conn.transaction().unwrap();
        tx.execute(
            "UPDATE messages SET content='replacement' WHERE session_id='a'",
            [],
        )
        .unwrap();
        tx.execute("UPDATE file_fingerprints SET mtime_ns=300,size=400", [])
            .unwrap();
        assert_eq!(
            record_observation_tx(&tx, &observation("a", "/root/a.jsonl"), 30, true).unwrap(),
            1
        );
        assert!(
            tx.execute("INSERT INTO sessions(id) VALUES ('a')", [])
                .is_err()
        );
    }
    assert_eq!(evidence(&conn, "a"), old);
    let content: String = conn
        .query_row(
            "SELECT content FROM messages WHERE session_id='a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(content, "original");
    let fingerprint: (i64, i64) = conn
        .query_row("SELECT mtime_ns,size FROM file_fingerprints", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(fingerprint, (100, 200));
}

#[test]
fn scope_failure_rolls_back_observations_and_partial_absence_updates() {
    let mut conn = database();
    for id in ["a", "b", "seen"] {
        seed(&conn, id, AiAssistant::ClaudeCode, &format!("/root/{id}"));
    }
    conn.execute_batch("CREATE TRIGGER fail_absence BEFORE UPDATE OF source_missing ON sessions WHEN NEW.id='b' AND NEW.source_missing=1 BEGIN SELECT RAISE(ABORT,'forced failure'); END;").unwrap();
    let mut scan = scan("/root");
    scan.discovered_ids.insert("seen".into());
    scan.observations.push(observation("seen", "/root/seen"));
    let old = ["a", "b", "seen"].map(|id| evidence(&conn, id));
    assert!(reconcile_scope(&mut conn, &scan, 20).is_err());
    assert_eq!(["a", "b", "seen"].map(|id| evidence(&conn, id)), old);
}

#[test]
fn path_scopes_are_boundary_safe_case_sensitive_and_assistant_specific() {
    let mut conn = database();
    for (id, assistant, locator) in [
        ("inside", AiAssistant::ClaudeCode, "/root/a"),
        ("nested", AiAssistant::ClaudeCode, "/root/nested/a"),
        ("sibling", AiAssistant::ClaudeCode, "/root-other/a"),
        ("case", AiAssistant::ClaudeCode, "/Root/a"),
        ("root", AiAssistant::ClaudeCode, "/root"),
        ("assistant", AiAssistant::Codex, "/root/codex"),
        ("db", AiAssistant::ClaudeCode, "/root/database"),
    ] {
        seed(&conn, id, assistant, locator);
    }
    conn.execute("UPDATE sessions SET source_scope='/db' WHERE id='db'", [])
        .unwrap();
    assert_eq!(reconcile_scope(&mut conn, &scan("/root/"), 20).unwrap(), 2);
    for id in ["inside", "nested"] {
        assert!(evidence(&conn, id).missing);
    }
    for id in ["sibling", "case", "root", "assistant", "db"] {
        assert!(!evidence(&conn, id).missing);
    }
    assert_eq!(
        path_prefix_bounds(Path::new("/")),
        Some(("/".into(), "0".into()))
    );
    assert!(path_prefix_bounds(Path::new("relative")).is_none());
}

#[test]
fn protected_directories_cover_physical_descendants_only() {
    let mut conn = database();
    for (id, locator) in [
        ("bundle", "/root/bundle"),
        ("physical", "/root/bundle/a"),
        ("sibling", "/root/bundle-other/a"),
        ("logical", "/root/elsewhere/a"),
    ] {
        seed(&conn, id, AiAssistant::ClaudeCode, locator);
    }
    conn.execute(
        "UPDATE sessions SET parent_session_id='bundle' WHERE id='logical'",
        [],
    )
    .unwrap();
    let mut scan = scan("/root");
    scan.protected_locators.insert("/root/bundle".into());
    assert_eq!(reconcile_scope(&mut conn, &scan, 20).unwrap(), 2);
    for id in ["bundle", "physical"] {
        assert!(!evidence(&conn, id).missing);
    }
    for id in ["sibling", "logical"] {
        assert!(evidence(&conn, id).missing);
    }
}

#[test]
fn losing_native_id_owner_cannot_update_winning_locator_or_assistant() {
    let mut conn = database();
    seed(&conn, "a", AiAssistant::ClaudeCode, "/loser/a");
    conn.execute("UPDATE sessions SET file_path='/winner/a',source_missing=1,source_missing_detected_at=15 WHERE id='a'", []).unwrap();
    let old = evidence(&conn, "a");
    let mut losing_scan = scan("/loser");
    losing_scan.observations.push(observation("a", "/loser/a"));
    assert_eq!(reconcile_scope(&mut conn, &losing_scan, 20).unwrap(), 0);
    assert_eq!(evidence(&conn, "a"), old);
    let tx = conn.transaction().unwrap();
    assert_eq!(
        record_observation_tx(&tx, &observation("a", "/loser/a"), 30, true).unwrap(),
        0
    );
    let mut wrong_assistant = observation("a", "/winner/a");
    wrong_assistant.assistant = AiAssistant::Codex;
    assert_eq!(
        record_observation_tx(&tx, &wrong_assistant, 30, true).unwrap(),
        0
    );
    tx.commit().unwrap();
    assert_eq!(evidence(&conn, "a"), old);
    let tx = conn.transaction().unwrap();
    assert_eq!(
        record_observation_tx(&tx, &observation("a", "/winner/a"), 30, true).unwrap(),
        1
    );
    tx.commit().unwrap();
    assert!(!evidence(&conn, "a").missing);
}

fn database_scan(path: &str) -> ScopeScan {
    ScopeScan {
        scope: SourceScope::Database { path: path.into() },
        ..scan("/unused")
    }
}

fn database_observation(id: &str, path: &str) -> SourceObservation {
    SourceObservation {
        assistant: AiAssistant::OpenCode,
        kind: SourceKind::DatabaseRecord,
        scope: Some(path.into()),
        ..observation(id, path)
    }
}

#[test]
fn database_absence_is_exact_scoped_and_adopts_only_matching_legacy_rows() {
    let mut conn = database();
    for (id, assistant, path, scope) in [
        ("a", AiAssistant::OpenCode, "/a.db", Some("/a.db")),
        ("b", AiAssistant::OpenCode, "/b.db", Some("/b.db")),
        ("legacy", AiAssistant::OpenCode, "/a.db", None),
        ("legacy-other", AiAssistant::OpenCode, "/b.db", None),
        ("file", AiAssistant::OpenCode, "/a.db/record.json", None),
        ("assistant", AiAssistant::ClaudeCode, "/a.db", None),
    ] {
        seed(&conn, id, assistant, path);
        conn.execute(
            "UPDATE sessions SET source_scope=?1 WHERE id=?2",
            params![scope, id],
        )
        .unwrap();
    }
    let mut scan = database_scan("/a.db");
    scan.complete = false;
    assert_eq!(reconcile_scope(&mut conn, &scan, 20).unwrap(), 0);
    assert_eq!(evidence(&conn, "legacy").scope, None);
    scan.complete = true;
    assert_eq!(reconcile_scope(&mut conn, &scan, 30).unwrap(), 2);
    assert_eq!(evidence(&conn, "legacy").scope.as_deref(), Some("/a.db"));
    for id in ["a", "legacy"] {
        assert!(evidence(&conn, id).missing);
    }
    for id in ["b", "legacy-other", "file", "assistant"] {
        assert!(!evidence(&conn, id).missing);
    }
}

#[test]
fn database_observation_cannot_steal_another_scope_even_at_the_same_locator() {
    let mut conn = database();
    seed(&conn, "a", AiAssistant::OpenCode, "/a.db");
    conn.execute("UPDATE sessions SET source_scope='/b.db' WHERE id='a'", [])
        .unwrap();
    let old = evidence(&conn, "a");
    let tx = conn.transaction().unwrap();
    assert_eq!(
        record_observation_tx(&tx, &database_observation("a", "/a.db"), 20, true).unwrap(),
        0
    );
    let mut file_observation = observation("a", "/a.db");
    file_observation.assistant = AiAssistant::OpenCode;
    assert_eq!(
        record_observation_tx(&tx, &file_observation, 20, true).unwrap(),
        0
    );
    tx.commit().unwrap();
    assert_eq!(evidence(&conn, "a"), old);
}

#[test]
fn observations_outside_the_scanned_scope_are_ignored() {
    let mut conn = database();
    seed(&conn, "outside", AiAssistant::ClaudeCode, "/other/a");
    seed(&conn, "db", AiAssistant::OpenCode, "/b.db");
    let outside = evidence(&conn, "outside");
    let db = evidence(&conn, "db");
    let mut root_scan = scan("/root");
    root_scan
        .observations
        .push(observation("outside", "/other/a"));
    assert_eq!(reconcile_scope(&mut conn, &root_scan, 20).unwrap(), 0);
    let mut db_scan = database_scan("/a.db");
    db_scan
        .observations
        .push(database_observation("db", "/b.db"));
    assert_eq!(reconcile_scope(&mut conn, &db_scan, 20).unwrap(), 0);
    assert_eq!(evidence(&conn, "outside"), outside);
    assert_eq!(evidence(&conn, "db"), db);
}

#[test]
fn non_file_sources_do_not_store_a_fabricated_fingerprint() {
    let mut conn = database();
    for (id, kind) in [
        ("directory", SourceKind::SessionDirectory),
        ("bundle", SourceKind::SessionBundle),
        ("database", SourceKind::DatabaseRecord),
    ] {
        let mut observation = observation(id, &format!("/root/{id}"));
        observation.kind = kind;
        if kind == SourceKind::DatabaseRecord {
            observation.assistant = AiAssistant::OpenCode;
            observation.scope = Some(observation.locator.clone());
        }
        seed(
            &conn,
            id,
            observation.assistant,
            observation.locator.to_str().unwrap(),
        );
        let tx = conn.transaction().unwrap();
        assert_eq!(
            record_observation_tx(&tx, &observation, 20, false).unwrap(),
            0
        );
        tx.commit().unwrap();
        let state = evidence(&conn, id);
        assert_eq!(state.seen, Some(20));
        assert_eq!(state.mtime, None);
        assert_eq!(state.size, None);
    }
}

#[test]
fn legacy_unknown_kind_and_null_evidence_remain_unknown_after_absence() {
    let mut conn = database();
    conn.execute_batch(
        "INSERT INTO sessions(id,tool,start_time,message_count,file_path,last_updated,source_kind)
        VALUES ('a','claude_code',1,0,'/root/a',2,'future_kind');",
    )
    .unwrap();
    assert_eq!(reconcile_scope(&mut conn, &scan("/root"), 20).unwrap(), 1);
    let state = evidence(&conn, "a");
    assert_eq!(state.kind.as_deref(), Some("future_kind"));
    assert_eq!(state.seen, None);
    assert_eq!(state.mtime, None);
    assert_eq!(state.size, None);
    let session = conn
        .query_row(
            "SELECT * FROM sessions WHERE id='a'",
            [],
            crate::database::session_from_row,
        )
        .unwrap();
    assert!(session.source.missing);
    assert_eq!(session.source.kind, None);
    assert_eq!(session.source.last_seen_at, None);
}

#[cfg(unix)]
#[test]
fn non_utf8_paths_cannot_alias_lossy_sql_locators() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let invalid = PathBuf::from(OsString::from_vec(b"/root/\xff".to_vec()));
    assert!(path_prefix_bounds(&invalid).is_none());
    let mut conn = database();
    seed(&conn, "a", AiAssistant::ClaudeCode, "/root/�");
    let old = evidence(&conn, "a");
    let mut observation = observation("a", "/unused");
    observation.locator = invalid;
    let tx = conn.transaction().unwrap();
    assert!(record_observation_tx(&tx, &observation, 20, false).is_err());
    tx.commit().unwrap();
    assert_eq!(evidence(&conn, "a"), old);
}
