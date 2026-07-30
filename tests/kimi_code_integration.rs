use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sessions_chronicle::database::SessionIndexer;
use sessions_chronicle::models::{Role, SourceStatus, ToolCallStatus, TranscriptItemKind};
use sessions_chronicle::parsers::kimi_code::KimiCodeParser;
use sessions_chronicle::session_sources::SessionSources;
use tempfile::TempDir;

const PRIMARY_ID: &str = "session_00000000-0000-4000-8000-000000000001";

fn copy_dir(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn copied_home() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    copy_fixture_home(temp.path());
    temp
}

fn copy_fixture_home(destination: &Path) {
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kimi_home"),
        destination,
    );
}

fn primary_dir(home: &Path) -> PathBuf {
    home.join("sessions/wd_primary_aaaaaaaaaaaa")
        .join(PRIMARY_ID)
}

fn child_id(agent_id: &str) -> String {
    format!("kimi-subagent::{PRIMARY_ID}::{agent_id}")
}

fn session_exists(connection: &rusqlite::Connection, session_id: &str) -> bool {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
            [session_id],
            |row| row.get(0),
        )
        .unwrap()
}

fn all_sources(kimi_home: &Path) -> SessionSources {
    let empty = kimi_home.join("empty");
    SessionSources {
        claude_dir: empty.clone(),
        opencode_storage_root: empty.clone(),
        opencode_db_paths: Vec::new(),
        codex_dir: empty.clone(),
        vibe_dir: empty,
        kimi_home: kimi_home.to_path_buf(),
        override_mode: true,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct BundleSnapshot {
    main_prompt: String,
    session_project_ids: BTreeMap<String, Option<i64>>,
    session_pins: BTreeMap<String, Option<i64>>,
    messages: Vec<(String, i64, String, String, i64, Option<String>)>,
    transcript_items: Vec<(
        String,
        i64,
        String,
        Option<i64>,
        Option<String>,
        Option<String>,
    )>,
    subagent_links: BTreeMap<(String, String), Option<String>>,
    fingerprints: BTreeMap<String, (i64, i64)>,
}

fn bundle_snapshot(connection: &rusqlite::Connection, session_dir: &Path) -> BundleSnapshot {
    let child_pattern = format!("kimi-subagent::{PRIMARY_ID}::%");
    let session_project_ids = connection
        .prepare("SELECT id, project_id FROM sessions WHERE id = ?1 OR id LIKE ?2 ORDER BY id")
        .unwrap()
        .query_map(rusqlite::params![PRIMARY_ID, &child_pattern], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()
        .unwrap();
    let session_pins = connection
        .prepare("SELECT id, pinned_at FROM sessions WHERE id = ?1 OR id LIKE ?2 ORDER BY id")
        .unwrap()
        .query_map(rusqlite::params![PRIMARY_ID, &child_pattern], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()
        .unwrap();
    let messages = connection
        .prepare(
            "SELECT session_id, message_index, role, content, timestamp, model
             FROM messages WHERE session_id = ?1 OR session_id LIKE ?2
             ORDER BY session_id, message_index",
        )
        .unwrap()
        .query_map(rusqlite::params![PRIMARY_ID, &child_pattern], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let transcript_items = connection
        .prepare(
            "SELECT session_id, item_index, kind, message_index, tool_call_id, subagent_id
             FROM transcript_items WHERE session_id = ?1 OR session_id LIKE ?2
             ORDER BY session_id, item_index",
        )
        .unwrap()
        .query_map(rusqlite::params![PRIMARY_ID, &child_pattern], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let subagent_links = connection
        .prepare(
            "SELECT session_id, id, child_session_id FROM subagents
             WHERE session_id = ?1 OR session_id LIKE ?2 ORDER BY session_id, id",
        )
        .unwrap()
        .query_map(rusqlite::params![PRIMARY_ID, &child_pattern], |row| {
            Ok(((row.get(0)?, row.get(1)?), row.get(2)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()
        .unwrap();
    let fingerprints = connection
        .prepare(
            "SELECT file_path, mtime_ns, size FROM file_fingerprints
             WHERE file_path >= ?1 AND file_path < ?2 ORDER BY file_path",
        )
        .unwrap()
        .query_map(
            [
                format!("{}/", session_dir.display()),
                format!("{}0", session_dir.display()),
            ],
            |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))),
        )
        .unwrap()
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()
        .unwrap();

    BundleSnapshot {
        main_prompt: connection
            .query_row(
                "SELECT first_prompt FROM sessions WHERE id = ?1",
                [PRIMARY_ID],
                |row| row.get(0),
            )
            .unwrap(),
        session_project_ids,
        session_pins,
        messages,
        transcript_items,
        subagent_links,
        fingerprints,
    }
}

#[test]
fn rich_fixture_bundle_round_trips_normalized_content() {
    let home = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kimi_home");
    let session_dir =
        home.join("sessions/wd_primary_aaaaaaaaaaaa/session_00000000-0000-4000-8000-000000000001");
    let bundle = KimiCodeParser::new(&home)
        .parse_session_dir(&session_dir)
        .unwrap();

    assert_eq!(bundle.main.session.tool.to_storage(), "kimi_code");
    assert_eq!(
        bundle.main.session.project_path.as_deref(),
        Some("/tmp/kimi-fixture-primary")
    );
    assert_eq!(
        bundle
            .main
            .messages
            .iter()
            .filter(|m| m.role == Role::User)
            .count(),
        1
    );
    assert!(
        bundle
            .main
            .tool_calls
            .iter()
            .any(|call| call.status == ToolCallStatus::Error)
    );
    assert!(
        bundle
            .main
            .tool_calls
            .iter()
            .any(|call| call.status == ToolCallStatus::Pending)
    );
    assert!(
        bundle
            .main
            .transcript_items
            .iter()
            .any(|item| item.kind == TranscriptItemKind::Subagent)
    );
    assert_eq!(bundle.children.len(), 3);
    assert!(
        bundle
            .session_ids
            .contains("kimi-subagent::session_00000000-0000-4000-8000-000000000001::agent-nested")
    );
}

#[test]
fn project_fallback_and_directory_identity_fixtures_follow_precedence() {
    let home = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kimi_home");
    let parser = KimiCodeParser::new(&home);
    let parse = |bucket: &str, id: &str| {
        parser
            .parse_session_dir(&home.join("sessions").join(bucket).join(id))
            .unwrap()
    };

    assert_eq!(
        parse(
            "wd_index_cccccccccc",
            "session_00000000-0000-4000-8000-000000000003"
        )
        .main
        .session
        .project_path
        .as_deref(),
        Some("/tmp/kimi-fixture-index")
    );
    assert_eq!(
        parse(
            "wd_workspace_dddddddddddd",
            "session_00000000-0000-4000-8000-000000000004"
        )
        .main
        .session
        .project_path
        .as_deref(),
        Some("/tmp/kimi-fixture-workspace")
    );
    let conflict = parse(
        "wd_conflict_eeeeeeeeeeee",
        "session_00000000-0000-4000-8000-000000000005",
    );
    assert_eq!(
        conflict.main.session.id,
        "session_00000000-0000-4000-8000-000000000005"
    );
    assert_eq!(
        conflict.main.session.project_path.as_deref(),
        Some("/tmp/kimi-fixture-workdir")
    );
}

#[test]
fn indexes_every_kimi_bundle_and_persists_their_children() {
    let home = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kimi_home");
    let database = tempfile::NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(database.path()).unwrap();

    assert_eq!(indexer.index_kimi_sessions(&home).unwrap(), 7);

    let connection = rusqlite::Connection::open(database.path()).unwrap();
    let sessions: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE tool = 'kimi_code'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let children: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE tool = 'kimi_code' AND is_subagent = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(sessions, 10);
    assert_eq!(children, 3);
}

#[test]
fn declared_child_addition_and_removal_replace_the_bundle_children() {
    let home = copied_home();
    let database = tempfile::NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(database.path()).unwrap();
    let primary = primary_dir(home.path());

    indexer.index_kimi_sessions(home.path()).unwrap();
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(primary.join("state.json")).unwrap()).unwrap();
    state["agents"]["agent-new"] = serde_json::json!({"type": "sub", "parentAgentId": "main"});
    fs::create_dir_all(primary.join("agents/agent-new")).unwrap();
    fs::copy(
        primary.join("agents/agent-0/wire.jsonl"),
        primary.join("agents/agent-new/wire.jsonl"),
    )
    .unwrap();
    fs::write(
        primary.join("state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();

    assert_eq!(indexer.index_kimi_sessions(home.path()).unwrap(), 7);
    let connection = rusqlite::Connection::open(database.path()).unwrap();
    assert!(session_exists(&connection, &child_id("agent-new")));
    assert_eq!(
        connection
            .query_row(
                "SELECT parent_session_id FROM sessions WHERE id = ?1",
                [child_id("agent-new")],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        PRIMARY_ID
    );

    state["agents"].as_object_mut().unwrap().remove("agent-1");
    fs::remove_dir_all(primary.join("agents/agent-1")).unwrap();
    fs::write(
        primary.join("state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();

    assert_eq!(indexer.index_kimi_sessions(home.path()).unwrap(), 7);
    assert!(!session_exists(&connection, &child_id("agent-1")));
    assert!(session_exists(&connection, &child_id("agent-new")));
}

#[test]
fn missing_declared_child_journal_preserves_bundle_and_reports_a_diagnostic() {
    let home = copied_home();
    let database = tempfile::NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(database.path()).unwrap();
    let sources = all_sources(home.path());
    let primary = primary_dir(home.path());

    indexer.index_all_incremental(&sources).unwrap();
    let connection = rusqlite::Connection::open(database.path()).unwrap();
    let before = bundle_snapshot(&connection, &primary);
    fs::remove_file(primary.join("agents/agent-0/wire.jsonl")).unwrap();

    let result = indexer.index_all_incremental(&sources).unwrap();

    assert_eq!(result.per_source[4].errors, 1);
    assert_eq!(result.errors_detail.len(), 1);
    assert!(result.errors_detail[0].message.contains("No such file"));
    assert_eq!(bundle_snapshot(&connection, &primary), before);
}

#[test]
fn invalid_utf8_after_valid_records_preserves_bundle_and_reports_one_diagnostic() {
    let home = copied_home();
    let database = tempfile::NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(database.path()).unwrap();
    let sources = all_sources(home.path());
    let primary = primary_dir(home.path());

    indexer.index_all_incremental(&sources).unwrap();
    let connection = rusqlite::Connection::open(database.path()).unwrap();
    let before = bundle_snapshot(&connection, &primary);
    fs::OpenOptions::new()
        .append(true)
        .open(primary.join("agents/main/wire.jsonl"))
        .unwrap()
        .write_all(b"\n{\"type\":\"unknown.future.record\"}\n\xff\n")
        .unwrap();

    let result = indexer.index_all_incremental(&sources).unwrap();

    assert_eq!(result.per_source[4].errors, 1);
    assert_eq!(result.errors_detail.len(), 1);
    assert_eq!(bundle_snapshot(&connection, &primary), before);
}

#[test]
fn injection_only_bundle_prunes_sessions_contents_links_and_fingerprints() {
    let home = copied_home();
    let database = tempfile::NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(database.path()).unwrap();
    let primary = primary_dir(home.path());
    let sources = all_sources(home.path());

    indexer.index_kimi_sessions(home.path()).unwrap();
    fs::write(
        primary.join("agents/main/wire.jsonl"),
        r#"{"type":"turn.prompt","time":1785319201000,"input":[{"type":"text","text":"Injected"}],"origin":{"kind":"injection"}}"#,
    )
    .unwrap();

    let result = indexer.index_all_incremental(&sources).unwrap();
    assert_eq!(result.per_source[4].errors, 0);
    assert_eq!(result.per_source[4].removed, 4);
    let connection = rusqlite::Connection::open(database.path()).unwrap();
    assert!(!session_exists(&connection, PRIMARY_ID));
    assert!(!session_exists(&connection, &child_id("agent-0")));
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1 OR session_id LIKE ?2",
                rusqlite::params![PRIMARY_ID, format!("kimi-subagent::{PRIMARY_ID}::%")],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM subagents WHERE session_id = ?1",
                [PRIMARY_ID],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM file_fingerprints WHERE file_path >= ?1 AND file_path < ?2",
                [
                    format!("{}/", primary.display()),
                    format!("{}0", primary.display())
                ],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn source_root_pruning_does_not_delete_bundles_from_another_kimi_home() {
    let roots = tempfile::tempdir().unwrap();
    let home_a = roots.path().join("a");
    let home_b = roots.path().join("a2");
    copy_fixture_home(&home_a);
    copy_fixture_home(&home_b);
    let second_id = "session_00000000-0000-4000-8000-000000000098";
    let second_dir = home_b
        .join("sessions/wd_primary_aaaaaaaaaaaa")
        .join(second_id);
    fs::rename(primary_dir(&home_b), &second_dir).unwrap();
    let database = tempfile::NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(database.path()).unwrap();

    indexer.index_kimi_sessions(&home_a).unwrap();
    indexer.index_kimi_sessions(&home_b).unwrap();
    let connection = rusqlite::Connection::open(database.path()).unwrap();
    let foreign_path = home_a.join("sessions/wd_foreign/session_foreign/transcript.jsonl");
    connection
        .execute(
            "INSERT INTO sessions
             (id, tool, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES ('foreign-under-a', 'claude_code', 0, 0, ?1, 0, 0)",
            [foreign_path.to_str().unwrap()],
        )
        .unwrap();
    fs::remove_dir_all(primary_dir(&home_a)).unwrap();

    assert_eq!(indexer.index_kimi_sessions(&home_a).unwrap(), 6);
    assert!(!session_exists(&connection, PRIMARY_ID));
    assert!(!session_exists(&connection, &child_id("agent-0")));
    assert!(session_exists(&connection, second_id));
    assert!(session_exists(
        &connection,
        &format!("kimi-subagent::{second_id}::agent-0")
    ));
    assert!(session_exists(&connection, "foreign-under-a"));
}

#[test]
fn equal_raw_child_ids_in_distinct_mains_are_namespaced() {
    let home = copied_home();
    let second_id = "session_00000000-0000-4000-8000-000000000099";
    let second = home
        .path()
        .join("sessions/wd_collision_999999999999")
        .join(second_id);
    copy_dir(&primary_dir(home.path()), &second);
    let database = tempfile::NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(database.path()).unwrap();

    assert_eq!(indexer.index_kimi_sessions(home.path()).unwrap(), 8);
    let connection = rusqlite::Connection::open(database.path()).unwrap();
    let first = child_id("agent-0");
    let second = format!("kimi-subagent::{second_id}::agent-0");
    assert!(session_exists(&connection, &first));
    assert!(session_exists(&connection, &second));
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE id IN (?1, ?2)",
                [&first, &second],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        2
    );
}

#[test]
fn failed_bundle_replacement_rolls_back_all_existing_bundle_data() {
    let home = copied_home();
    let database = tempfile::NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(database.path()).unwrap();
    let primary = primary_dir(home.path());

    indexer.index_kimi_sessions(home.path()).unwrap();
    let connection = rusqlite::Connection::open(database.path()).unwrap();
    connection
        .execute(
            "UPDATE sessions SET pinned_at = 123 WHERE id = ?1",
            [PRIMARY_ID],
        )
        .unwrap();
    let before = bundle_snapshot(&connection, &primary);
    connection
        .execute(
            &format!(
                "CREATE TRIGGER abort_kimi_child BEFORE INSERT ON messages \
                 WHEN NEW.session_id = '{}' BEGIN SELECT RAISE(ABORT, 'test rollback'); END",
                child_id("agent-0")
            ),
            [],
        )
        .unwrap();
    let main_journal = primary.join("agents/main/wire.jsonl");
    let journal = fs::read_to_string(&main_journal)
        .unwrap()
        .replace("Inspect parser safety", "Replacement prompt must roll back");
    fs::write(main_journal, journal).unwrap();

    assert_eq!(indexer.index_kimi_sessions(home.path()).unwrap(), 6);
    assert_eq!(bundle_snapshot(&connection, &primary), before);
}

#[cfg(unix)]
#[test]
fn symlinked_sessions_root_cannot_index_an_external_fixture_tree() {
    use std::os::unix::fs::symlink;

    let configured_home = tempfile::tempdir().unwrap();
    let external_home = copied_home();
    symlink(
        external_home.path().join("sessions"),
        configured_home.path().join("sessions"),
    )
    .unwrap();
    let database = tempfile::NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(database.path()).unwrap();
    let sources = all_sources(configured_home.path());

    let result = indexer.index_all_incremental(&sources).unwrap();

    assert_eq!(result.per_source[4].indexed, 0);
    assert_eq!(result.per_source[4].errors, 1);
    assert_eq!(result.per_source[4].status, SourceStatus::Failed);
    assert_eq!(result.errors_detail.len(), 1);
    assert_eq!(
        result.errors_detail[0].location.as_deref(),
        configured_home.path().join("sessions").to_str()
    );
    let connection = rusqlite::Connection::open(database.path()).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE tool = 'kimi_code'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[cfg(unix)]
#[test]
fn declared_child_fifo_is_diagnosed_without_blocking_and_preserves_bundle() {
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::Duration;

    let home = copied_home();
    let database = tempfile::NamedTempFile::new().unwrap();
    let primary = primary_dir(home.path());
    let child_journal = primary.join("agents/agent-0/wire.jsonl");
    let mut indexer = SessionIndexer::new(database.path()).unwrap();
    indexer.index_kimi_sessions(home.path()).unwrap();
    let connection = rusqlite::Connection::open(database.path()).unwrap();
    let before = bundle_snapshot(&connection, &primary);
    fs::remove_file(&child_journal).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(&child_journal)
            .status()
            .unwrap()
            .success()
    );

    let database_path = database.path().to_path_buf();
    let kimi_home = home.path().to_path_buf();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = SessionIndexer::new(&database_path)
            .and_then(|mut indexer| indexer.index_all_incremental(&all_sources(&kimi_home)));
        sender.send(result).unwrap();
    });
    let result = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("Kimi indexing blocked while opening a declared child FIFO")
        .unwrap();

    assert_eq!(result.per_source[4].errors, 1);
    assert_eq!(result.per_source[4].status, SourceStatus::Degraded);
    assert_eq!(result.errors_detail.len(), 1);
    assert_eq!(
        result.errors_detail[0].location.as_deref(),
        child_journal.to_str()
    );
    assert_eq!(bundle_snapshot(&connection, &primary), before);
}
