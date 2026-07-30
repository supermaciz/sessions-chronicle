use std::fs;
use std::path::{Path, PathBuf};

use sessions_chronicle::database::SessionIndexer;
use sessions_chronicle::models::{Role, ToolCallStatus, TranscriptItemKind};
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
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kimi_home"),
        temp.path(),
    );
    temp
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
    let old_prompt: String = connection
        .query_row(
            "SELECT first_prompt FROM sessions WHERE id = ?1",
            [PRIMARY_ID],
            |row| row.get(0),
        )
        .unwrap();
    let old_fingerprints: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM file_fingerprints WHERE file_path >= ?1 AND file_path < ?2",
            [
                format!("{}/", primary.display()),
                format!("{}0", primary.display()),
            ],
            |row| row.get(0),
        )
        .unwrap();
    fs::remove_file(primary.join("agents/agent-0/wire.jsonl")).unwrap();

    let result = indexer.index_all_incremental(&sources).unwrap();

    assert_eq!(result.per_source[4].errors, 1);
    assert_eq!(result.errors_detail.len(), 1);
    assert!(result.errors_detail[0].message.contains("No such file"));
    assert_eq!(
        connection
            .query_row(
                "SELECT first_prompt FROM sessions WHERE id = ?1",
                [PRIMARY_ID],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        old_prompt
    );
    assert!(session_exists(&connection, &child_id("agent-0")));
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
        old_fingerprints
    );
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
    let home_a = copied_home();
    let home_b = copied_home();
    let second_id = "session_00000000-0000-4000-8000-000000000098";
    let second_dir = home_b
        .path()
        .join("sessions/wd_primary_aaaaaaaaaaaa")
        .join(second_id);
    fs::rename(primary_dir(home_b.path()), &second_dir).unwrap();
    let database = tempfile::NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(database.path()).unwrap();

    indexer.index_kimi_sessions(home_a.path()).unwrap();
    indexer.index_kimi_sessions(home_b.path()).unwrap();
    fs::remove_dir_all(primary_dir(home_a.path())).unwrap();

    assert_eq!(indexer.index_kimi_sessions(home_a.path()).unwrap(), 6);
    let connection = rusqlite::Connection::open(database.path()).unwrap();
    assert!(!session_exists(&connection, PRIMARY_ID));
    assert!(!session_exists(&connection, &child_id("agent-0")));
    assert!(session_exists(&connection, second_id));
    assert!(session_exists(
        &connection,
        &format!("kimi-subagent::{second_id}::agent-0")
    ));
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
    let before: (String, i64, i64, i64, i64, i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT first_prompt FROM sessions WHERE id = ?1),
                (SELECT COUNT(*) FROM sessions WHERE id LIKE ?2),
                (SELECT COUNT(*) FROM transcript_items WHERE session_id = ?1 OR session_id LIKE ?2),
                (SELECT COUNT(*) FROM subagents WHERE session_id = ?1),
                (SELECT COUNT(DISTINCT project_id) FROM sessions WHERE id = ?1 OR id LIKE ?2),
                (SELECT pinned_at FROM sessions WHERE id = ?1),
                (SELECT COUNT(*) FROM file_fingerprints WHERE file_path >= ?3 AND file_path < ?4)",
            rusqlite::params![
                PRIMARY_ID,
                format!("kimi-subagent::{PRIMARY_ID}::%"),
                format!("{}/", primary.display()),
                format!("{}0", primary.display()),
            ],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .unwrap();
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
    let after: (String, i64, i64, i64, i64, i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT first_prompt FROM sessions WHERE id = ?1),
                (SELECT COUNT(*) FROM sessions WHERE id LIKE ?2),
                (SELECT COUNT(*) FROM transcript_items WHERE session_id = ?1 OR session_id LIKE ?2),
                (SELECT COUNT(*) FROM subagents WHERE session_id = ?1),
                (SELECT COUNT(DISTINCT project_id) FROM sessions WHERE id = ?1 OR id LIKE ?2),
                (SELECT pinned_at FROM sessions WHERE id = ?1),
                (SELECT COUNT(*) FROM file_fingerprints WHERE file_path >= ?3 AND file_path < ?4)",
            rusqlite::params![
                PRIMARY_ID,
                format!("kimi-subagent::{PRIMARY_ID}::%"),
                format!("{}/", primary.display()),
                format!("{}0", primary.display()),
            ],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(after, before);
}
