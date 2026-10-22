//! Public-path acceptance tests for the native OpenCode V2 reader.
//!
//! These exercise the same public APIs the application and GNOME search
//! provider use — `SessionIndexer`, `search_sessions_for_filter`,
//! `find_session_match_positions`, `load_transcript_items`,
//! `load_context_full_content`, and the `shell_search` query path — against the
//! synthetic, source-derived V2 fixture (`tests/fixtures/opencode_v2/`).
//!
//! The fixture builder `create_source` executes the two reviewed SQL files;
//! cache inspection uses raw SQL, mirroring the crate-internal coordinator
//! tests. No production API is added.

use rusqlite::Connection;
use std::path::{Path, PathBuf};

use sessions_chronicle::SessionSources;
use sessions_chronicle::database::{
    SessionIndexer, find_session_match_positions, load_context_full_content, load_transcript_items,
    search_sessions_for_filter,
    shell_search::{ShellSearchConnection, build_match_expression, search_session_ids},
};
use sessions_chronicle::models::{AiAssistant, ContextKind, DateFilter, ProjectFilter};

/// Builds a synthetic OpenCode V2 SQLite database at `path` from the reviewed
/// fixture `schema.sql` + `sessions.sql`. The returned connection is the
/// caller's to drop once the source file is finalized.
fn create_source(path: &Path) -> Connection {
    let conn = Connection::open(path).expect("open synthetic V2 source");
    let schema =
        std::fs::read_to_string("tests/fixtures/opencode_v2/schema.sql").expect("read schema.sql");
    let sessions = std::fs::read_to_string("tests/fixtures/opencode_v2/sessions.sql")
        .expect("read sessions.sql");
    conn.execute_batch(&schema)
        .expect("apply opencode_v2 schema.sql");
    conn.execute_batch(&sessions)
        .expect("apply opencode_v2 sessions.sql");
    conn
}

/// Counts the session's dependent rows across every table the indexer writes,
/// so a stable reindex can be asserted without coupling to production APIs.
fn dependent_row_counts(cache: &Path, session_id: &str) -> Vec<(String, i64)> {
    let db = Connection::open(cache).expect("open cache for row counts");
    [
        ("sessions", "SELECT count(*) FROM sessions WHERE id = ?1"),
        (
            "messages",
            "SELECT count(*) FROM messages WHERE session_id = ?1",
        ),
        (
            "transcript_items",
            "SELECT count(*) FROM transcript_items WHERE session_id = ?1",
        ),
        (
            "context_entries",
            "SELECT count(*) FROM context_entries WHERE session_id = ?1",
        ),
        (
            "reasoning_attachments",
            "SELECT count(*) FROM reasoning_attachments WHERE session_id = ?1",
        ),
        (
            "opencode_session_sources",
            "SELECT count(*) FROM opencode_session_sources WHERE session_id = ?1",
        ),
    ]
    .iter()
    .map(|(table, sql)| {
        let count = db
            .query_row(sql, [session_id], |row| row.get::<_, i64>(0))
            .expect("count dependent rows");
        ((*table).to_string(), count)
    })
    .collect()
}

#[test]
fn v2_end_to_end_context_is_not_searchable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("opencode.db");
    let conn = create_source(&source);
    drop(conn);

    let cache = dir.path().join("cache.db");
    let json_root = dir.path().join("json"); // no legacy JSON present
    let mut indexer = SessionIndexer::new(&cache).expect("create indexer");
    let indexed = indexer
        .index_opencode_sessions(&json_root, &[source.clone()])
        .expect("index V2 fixture");
    assert!(
        indexed > 0,
        "the V2 fixture must index at least one session"
    );

    // Genuine user and assistant terms find `ses_v2`.
    for term in ["real prompt", "final answer"] {
        let sessions = search_sessions_for_filter(
            &cache,
            &[AiAssistant::OpenCode],
            &ProjectFilter::AllSessions,
            term,
            &DateFilter::AnyTime,
            None,
        )
        .expect("search failed");
        assert!(
            sessions.iter().any(|s| s.id == "ses_v2"),
            "genuine term {term:?} must find ses_v2"
        );
    }

    // A term that appears only in context text matches no global result.
    let sessions = search_sessions_for_filter(
        &cache,
        &[AiAssistant::OpenCode],
        &ProjectFilter::AllSessions,
        "contextonlyneedle",
        &DateFilter::AnyTime,
        None,
    )
    .expect("search failed");
    assert!(
        sessions.is_empty(),
        "context-only term must not match any session"
    );

    // The in-transcript search path reports no match position for the context
    // term, while a genuine term still does.
    {
        let db = Connection::open(&cache).expect("open cache");
        assert!(
            find_session_match_positions(&db, "ses_v2", "contextonlyneedle")
                .expect("match positions")
                .is_empty(),
            "context-only term must produce no match positions"
        );
        assert!(
            !find_session_match_positions(&db, "ses_v2", "real prompt")
                .expect("match positions")
                .is_empty(),
            "genuine term must produce match positions"
        );
    }

    // The GNOME search-provider query path sees neither a result nor a snippet
    // for the context term, with excerpts both disabled and enabled.
    let (shell, _interrupt) = ShellSearchConnection::open_read_only(&cache)
        .expect("open shell search")
        .expect("shell search connection");
    let needle = build_match_expression(&["contextonlyneedle".to_string()])
        .expect("build context-only expression");
    let genuine =
        build_match_expression(&["real prompt".to_string()]).expect("build genuine expression");

    // Excerpts disabled: no ranked result for the context term.
    assert!(
        search_session_ids(&shell, &needle)
            .expect("ranked search")
            .is_empty(),
        "context-only term must yield no GNOME result"
    );
    // Excerpts enabled: no snippet when the session is looked up directly.
    let metadata = shell
        .load_metadata(&["ses_v2".to_string()], true, Some(&needle))
        .expect("metadata with excerpts");
    assert!(
        metadata[0]
            .as_ref()
            .expect("ses_v2 row")
            .matched_snippet
            .is_none(),
        "context-only term must yield no GNOME snippet"
    );

    // Positive control: the genuine term yields a ranked result and a snippet.
    assert!(
        search_session_ids(&shell, &genuine)
            .expect("ranked search")
            .iter()
            .any(|id| id == "ses_v2"),
        "genuine term must rank ses_v2"
    );
    let metadata = shell
        .load_metadata(&["ses_v2".to_string()], true, Some(&genuine))
        .expect("metadata with excerpts");
    assert!(
        metadata[0]
            .as_ref()
            .expect("ses_v2 row")
            .matched_snippet
            .is_some(),
        "genuine term must yield a GNOME snippet"
    );

    // Reindexing twice leaves the dependent-row counts stable: replacement is
    // atomic and accumulates no duplicate rows.
    let first = dependent_row_counts(&cache, "ses_v2");
    indexer
        .index_opencode_sessions(&json_root, &[source.clone()])
        .expect("second index");
    let second = dependent_row_counts(&cache, "ses_v2");
    indexer
        .index_opencode_sessions(&json_root, &[source.clone()])
        .expect("third index");
    let third = dependent_row_counts(&cache, "ses_v2");
    assert_eq!(first, second, "reindex must not change dependent rows");
    assert_eq!(second, third, "reindex must not change dependent rows");
    assert!(
        first
            .iter()
            .any(|(table, count)| table == "context_entries" && *count > 0),
        "ses_v2 must have context entries"
    );
}

/// Creates one SQLite source that serves both V1 (`session`/`message`/`part`)
/// and V2 (`session_v2`/`session_message`) for the same session id, mirroring a
/// migrated OpenCode database whose legacy V1 tables are frozen in place.
fn create_mixed_source(path: &Path, id: &str, v1_body: &str, v2_body: &str) -> Connection {
    let conn = Connection::open(path).expect("open mixed source");
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
         );
         CREATE TABLE session_v2 (
             id TEXT PRIMARY KEY, project_id TEXT NOT NULL, directory TEXT NOT NULL,
             title TEXT, parent_id TEXT, version TEXT NOT NULL, slug TEXT NOT NULL,
             model TEXT, tokens_input INTEGER NOT NULL DEFAULT 0,
             tokens_output INTEGER NOT NULL DEFAULT 0,
             tokens_reasoning INTEGER NOT NULL DEFAULT 0,
             tokens_cache_read INTEGER NOT NULL DEFAULT 0,
             tokens_cache_write INTEGER NOT NULL DEFAULT 0,
             time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL
         );
         CREATE TABLE session_message (
             id TEXT PRIMARY KEY, session_id TEXT NOT NULL, type TEXT NOT NULL,
             seq INTEGER NOT NULL, time_created INTEGER NOT NULL,
             time_updated INTEGER NOT NULL, data TEXT NOT NULL,
             UNIQUE (session_id, seq)
         );",
    )
    .expect("create mixed source schema");

    // Frozen V1 representation.
    conn.execute(
        "INSERT INTO session (id, directory, title, parent_id, time_created, time_updated)
         VALUES (?1, '/tmp/proj', NULL, NULL, 1712000000000, 1712000000000)",
        [id],
    )
    .expect("insert V1 session");
    let v1_msg = format!("msg-{id}");
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, data)
         VALUES (?1, ?2, 1712000000000, '{\"role\":\"user\"}')",
        rusqlite::params![v1_msg, id],
    )
    .expect("insert V1 message");
    conn.execute(
        "INSERT INTO part (id, message_id, data) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            format!("part-{id}"),
            v1_msg,
            serde_json::json!({ "type": "text", "order": 1, "text": v1_body }).to_string(),
        ],
    )
    .expect("insert V1 part");

    // Current V2 representation.
    conn.execute(
        "INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model,
                                 tokens_input, tokens_output, tokens_reasoning, tokens_cache_read,
                                 tokens_cache_write, time_created, time_updated)
         VALUES (?1, 'prj', '/tmp/proj', NULL, NULL, '0.0.0', ?2, NULL, 0, 0, 0, 0, 0,
                 1712000000000, 1712000000000)",
        rusqlite::params![id, format!("slug-{id}")],
    )
    .expect("insert V2 session");
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
         VALUES (?1, ?2, 'user', 1, 1712000000000, 1712000000000, ?3)",
        rusqlite::params![
            format!("msg2-{id}"),
            id,
            serde_json::json!({ "text": v2_body, "time": { "created": 1_712_000_000_000_i64 } })
                .to_string(),
        ],
    )
    .expect("insert V2 message");

    conn
}

fn file_stamp(path: &Path) -> (i64, i64) {
    let metadata = std::fs::metadata(path).expect("stat source");
    let mtime_ns = metadata
        .modified()
        .expect("mtime")
        .duration_since(std::time::UNIX_EPOCH)
        .expect("mtime after epoch")
        .as_nanos();
    (
        i64::try_from(mtime_ns).expect("mtime_ns fits"),
        i64::try_from(metadata.len()).expect("size fits"),
    )
}

fn wal_path(db_path: &Path) -> PathBuf {
    let mut wal = db_path.as_os_str().to_os_string();
    wal.push("-wal");
    wal.into()
}

/// Seeds the local cache to represent a pre-V2 (v18-era) V1 index of `id`:
/// a V1 body, a `v1` owner generation, a pin, and a source fingerprint whose
/// DB/WAL stamps match the (unchanged) source but whose reader generation is
/// stale (`1`), exactly what the v19 migration leaves behind before the first
/// new-reader run.
fn seed_v18_v1_cache(cache: &Connection, id: &str, source: &Path, v1_body: &str, pin: i64) {
    cache
        .execute(
            "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count,
                                   file_path, last_updated, first_prompt, parent_session_id,
                                   is_subagent, input_tokens, output_tokens, cache_read_tokens,
                                   cache_write_tokens, reasoning_tokens, edit_count, read_count,
                                   command_count, ending_status, agent_path, pinned_at)
             VALUES (?1, 'opencode', '/tmp/proj', NULL, 1712000000000, 1, ?2, 1712000000000,
                     ?3, NULL, 0, NULL, NULL, NULL, NULL, NULL, 0, 0, 0, 'unknown', NULL, ?4)",
            rusqlite::params![id, source.to_str().expect("source path utf8"), v1_body, pin],
        )
        .expect("seed session");
    cache
        .execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
             VALUES (?1, 0, 'user', ?2, 1712000000000, NULL)",
            rusqlite::params![id, v1_body],
        )
        .expect("seed V1 message");
    cache
        .execute(
            "INSERT INTO transcript_items (session_id, item_index, kind, message_index,
                                           tool_call_id, subagent_id, context_id)
             VALUES (?1, 0, 'message', 0, NULL, NULL, NULL)",
            [id],
        )
        .expect("seed transcript item");
    cache
        .execute(
            "INSERT INTO opencode_session_sources (session_id, generation) VALUES (?1, 'v1')",
            [id],
        )
        .expect("seed owner generation");

    let (db_mtime_ns, db_size) = file_stamp(source);
    let wal = wal_path(source);
    let (wal_mtime_ns, wal_size) = if wal.exists() {
        let (mtime_ns, size) = file_stamp(&wal);
        (Some(mtime_ns), Some(size))
    } else {
        (None, None)
    };
    cache
        .execute(
            "INSERT INTO opencode_source_fingerprints
                 (db_path, db_mtime_ns, db_size, wal_mtime_ns, wal_size, reader_generation)
             VALUES (?1, ?2, ?3, ?4, ?5, 1)",
            rusqlite::params![
                source.to_str().expect("source path utf8"),
                db_mtime_ns,
                db_size,
                wal_mtime_ns,
                wal_size,
            ],
        )
        .expect("seed stale source fingerprint");
}

#[test]
fn v2_migration_preserves_pin_and_refreshes_unchanged_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("opencode.db");

    // An unchanged mixed source: V1 ("v1 body") frozen alongside V2 ("v2 body").
    let conn = create_mixed_source(&source, "ses_mig", "v1 body", "v2 body");
    drop(conn);

    let cache = dir.path().join("cache.db");
    let mut indexer = SessionIndexer::new(&cache).expect("create indexer");

    // A v18-era V1 cache of that source, pinned by the user.
    {
        let conn = Connection::open(&cache).expect("open cache for seeding");
        seed_v18_v1_cache(&conn, "ses_mig", &source, "v1 body", 1234);
    }

    // The first new-reader (incremental) run reindexes the unchanged source
    // because its reader generation is stale, then promotes V1 -> V2.
    let sources = SessionSources {
        claude_dir: dir.path().join("no-claude"),
        opencode_storage_root: dir.path().join("no-json"),
        opencode_db_paths: vec![source.clone()],
        codex_dir: dir.path().join("no-codex"),
        vibe_dir: dir.path().join("no-vibe"),
        kimi_home: dir.path().join("no-kimi"),
        override_mode: true,
    };
    let result = indexer
        .index_all_incremental(&sources)
        .expect("incremental run");
    assert!(
        result.totals.indexed >= 1,
        "stale reader generation must force a reindex"
    );

    let db = Connection::open(&cache).expect("open cache");
    let generation: String = db
        .query_row(
            "SELECT generation FROM opencode_session_sources WHERE session_id = 'ses_mig'",
            [],
            |row| row.get(0),
        )
        .expect("read generation");
    assert_eq!(generation, "v2", "V2 must replace the V1 owner");

    let body: String = db
        .query_row(
            "SELECT content FROM messages WHERE session_id = 'ses_mig' ORDER BY message_index ASC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("read cached body");
    assert_eq!(body, "v2 body", "V2 content must replace the V1 content");

    let pin: Option<i64> = db
        .query_row(
            "SELECT pinned_at FROM sessions WHERE id = 'ses_mig'",
            [],
            |row| row.get(0),
        )
        .expect("read pin");
    assert_eq!(pin, Some(1234), "the user pin must survive the reindex");
}

/// Builds a source with one `ses_large` session: a single 1 MiB system context
/// message followed by 10,000 user messages. Returns the connection (dropped by
/// the caller) and the exact context body.
fn create_large_source(path: &Path, message_count: i64) -> (Connection, String) {
    let mut conn = Connection::open(path).expect("open large source");
    let schema =
        std::fs::read_to_string("tests/fixtures/opencode_v2/schema.sql").expect("read schema.sql");
    conn.execute_batch(&schema)
        .expect("apply opencode_v2 schema.sql");

    // Exactly 1 MiB with a distinctive prefix so "exact body" and "body absent
    // from previews" can be asserted without reconstructing it by hand.
    let mut body = String::with_capacity(1024 * 1024);
    body.push_str("SYSTEM-CONTEXT-BODY:");
    while body.len() < 1024 * 1024 {
        body.push('z');
    }
    assert_eq!(body.len(), 1024 * 1024);

    conn.execute(
        "INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model,
                                 tokens_input, tokens_output, tokens_reasoning, tokens_cache_read,
                                 tokens_cache_write, time_created, time_updated)
         VALUES ('ses_large', 'prj', '/tmp/proj', NULL, NULL, '0.0.0', 'ses-large', NULL, 0, 0, 0,
                 0, 0, 1712000000000, 1712000000000)",
        [],
    )
    .expect("insert large session");

    // System context message (seq 0) holds the whole 1 MiB body.
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
         VALUES ('msg_large_context', 'ses_large', 'system', 0, 1712000000000, 1712000000000, ?1)",
        [
            serde_json::json!({ "text": body, "time": { "created": 1_712_000_000_000_i64 } })
                .to_string(),
        ],
    )
    .expect("insert context message");

    // 10,000 user messages in one transaction.
    let tx = conn.transaction().expect("begin transaction");
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
                 VALUES (?1, 'ses_large', 'user', ?2, ?3, ?3, ?4)",
            )
            .expect("prepare user insert");
        for i in 1..=message_count {
            let ts = 1_712_000_000_000_i64 + i;
            let data = serde_json::json!({
                "text": format!("user message {i}"),
                "time": { "created": ts },
            })
            .to_string();
            stmt.execute(rusqlite::params![format!("msg_large_{i}"), i, ts, data])
                .expect("insert user message");
        }
    }
    tx.commit().expect("commit transaction");

    (conn, body)
}

#[test]
fn v2_large_transcript_keeps_context_bodies_lazy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("opencode.db");
    let (conn, body) = create_large_source(&source, 10_000);
    drop(conn);

    let cache = dir.path().join("cache.db");
    let mut indexer = SessionIndexer::new(&cache).expect("create indexer");
    indexer
        .index_opencode_sessions(&dir.path().join("no-json"), &[source])
        .expect("index large source");

    // A requested transcript page returns exactly the requested rows, in
    // `item_index` order, with the context row first.
    let page = load_transcript_items(&cache, "ses_large", 50, 0, 512).expect("load page");
    assert_eq!(page.len(), 50, "the requested page length must be honored");
    for (position, row) in page.iter().enumerate() {
        assert_eq!(
            row.item_index, position as i64,
            "rows must be ordered by item_index"
        );
    }

    // The context row carries only metadata — never its body — in the preview.
    let context_row = &page[0];
    assert_eq!(
        context_row.context.as_ref().expect("context preview").kind,
        ContextKind::System
    );
    assert!(
        context_row.content_preview.is_none(),
        "context rows expose no message content preview"
    );
    assert!(
        page.iter().all(|row| row
            .content_preview
            .as_deref()
            .is_none_or(|preview| !preview.starts_with("SYSTEM-CONTEXT-BODY:"))),
        "the 1 MiB context body must never appear in a row preview"
    );

    // A genuine message is still present and previewed normally.
    assert_eq!(page[1].content_preview.as_deref(), Some("user message 1"));

    // The full body is available only via the explicit lazy lookup, unchanged.
    let loaded = load_context_full_content(&cache, "ses_large", "msg_large_context")
        .expect("load context body");
    assert_eq!(loaded.as_deref(), Some(body.as_str()));
    assert_eq!(loaded.expect("body present").len(), 1024 * 1024);

    // The reader's cursor loop (`crates/core/src/parsers/opencode/v2.rs`,
    // `V2Reader::parse_session`) streams `session_message` rows through
    // `V2Projection::push` with `while let Some(row) = rows.next()?` and never
    // collects the source table into a vector before projecting, so this test
    // asserts no runtime memory threshold — only the normalized session-sized
    // result, which is permitted by the spec.
}
