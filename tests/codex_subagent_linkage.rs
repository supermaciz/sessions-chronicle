use rusqlite::Connection;
use serde_json::Value;
use sessions_chronicle::database::{SessionIndexer, load_session, load_subagent};
use sessions_chronicle::models::AiAssistant;
use sessions_chronicle::session_sources::SessionSources;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir, tempdir};

const FIXTURE_BASE: &str = "tests/fixtures/codex_subagent_linkage/2026/04/18";
const PARENT_FILE: &str = "rollout-2026-04-18T13-17-00-019da0bb-541a-74e2-ae0a-6693c5e4fe04.jsonl";
const CHILD_FILE: &str = "rollout-2026-04-18T13-17-01-019da0bd-3df2-7191-a1a8-e326b55fe052.jsonl";
const PARENT_SESSION_ID: &str = "019da0bb-541a-74e2-ae0a-6693c5e4fe04";
const CHILD_SESSION_ID: &str = "019da0bd-3df2-7191-a1a8-e326b55fe052";

fn fixture_file_path(file_name: &str) -> PathBuf {
    Path::new(FIXTURE_BASE).join(file_name)
}

fn fixture_dir(include_parent: bool, include_child: bool) -> TempDir {
    let dir = tempdir().unwrap();
    let target_dir = dir.path().join("2026/04/18");
    fs::create_dir_all(&target_dir).unwrap();

    if include_parent {
        fs::copy(fixture_file_path(PARENT_FILE), target_dir.join(PARENT_FILE)).unwrap();
    }

    if include_child {
        fs::copy(fixture_file_path(CHILD_FILE), target_dir.join(CHILD_FILE)).unwrap();
    }

    dir
}

fn fixture_override_root() -> TempDir {
    let root = tempdir().unwrap();
    let target_dir = root.path().join("codex_sessions/2026/04/18");
    fs::create_dir_all(&target_dir).unwrap();

    fs::copy(fixture_file_path(PARENT_FILE), target_dir.join(PARENT_FILE)).unwrap();
    fs::copy(fixture_file_path(CHILD_FILE), target_dir.join(CHILD_FILE)).unwrap();

    root
}

fn append_harmless_parent_event(root: &TempDir) {
    let parent_path = root
        .path()
        .join("codex_sessions/2026/04/18")
        .join(PARENT_FILE);
    let mut file = OpenOptions::new().append(true).open(parent_path).unwrap();
    writeln!(
        file,
        "{{\"type\":\"event_msg\",\"timestamp\":\"2026-04-18T13:17:05Z\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"No-op incremental marker\"}}}}"
    )
    .unwrap();
}

#[test]
fn indexing_codex_parent_and_child_links_subagent_to_child_session() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    indexer
        .index_codex_sessions(Path::new("tests/fixtures/codex_subagent_linkage"))
        .unwrap();

    let parent = load_session(temp_db.path(), PARENT_SESSION_ID)
        .unwrap()
        .expect("parent session should be indexed");
    assert!(!parent.is_subagent);

    let child = load_session(temp_db.path(), CHILD_SESSION_ID)
        .unwrap()
        .expect("child session should be indexed");
    assert!(child.is_subagent);
    assert_eq!(child.parent_session_id.as_deref(), Some(PARENT_SESSION_ID));

    let subagent = load_subagent(temp_db.path(), PARENT_SESSION_ID, "call_spawn_1")
        .unwrap()
        .expect("parent subagent should be indexed");
    assert_eq!(subagent.agent_id.as_deref(), Some(CHILD_SESSION_ID));
    assert_eq!(subagent.child_session_id.as_deref(), Some(CHILD_SESSION_ID));
}

#[test]
fn indexing_codex_child_before_parent_still_links_on_second_pass() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    let child_only_dir = fixture_dir(false, true);
    let parent_only_dir = fixture_dir(true, false);

    indexer.index_codex_sessions(child_only_dir.path()).unwrap();
    indexer
        .index_codex_sessions(parent_only_dir.path())
        .unwrap();
    indexer.index_codex_sessions(child_only_dir.path()).unwrap();

    let subagent = load_subagent(temp_db.path(), PARENT_SESSION_ID, "call_spawn_1")
        .unwrap()
        .expect("parent subagent should exist after parent indexing");
    assert_eq!(subagent.child_session_id.as_deref(), Some(CHILD_SESSION_ID));
}

#[test]
fn indexing_codex_reindex_replaces_parent_subagents_without_duplicates() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    let parent_only_dir = fixture_dir(true, false);
    let child_only_dir = fixture_dir(false, true);

    indexer
        .index_codex_sessions(parent_only_dir.path())
        .unwrap();
    indexer.index_codex_sessions(child_only_dir.path()).unwrap();
    indexer
        .index_codex_sessions(parent_only_dir.path())
        .unwrap();
    indexer.index_codex_sessions(child_only_dir.path()).unwrap();

    let conn = Connection::open(temp_db.path()).unwrap();
    let subagent_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM subagents WHERE session_id = ?1",
            [PARENT_SESSION_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(subagent_count, 2);

    let linked_count: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM subagents
             WHERE session_id = ?1 AND child_session_id = ?2",
            rusqlite::params![PARENT_SESSION_ID, CHILD_SESSION_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(linked_count, 2);
}

#[test]
fn indexing_codex_duplicate_thread_rows_share_the_same_child_session_id() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    let parent_only_dir = fixture_dir(true, false);
    let child_only_dir = fixture_dir(false, true);

    indexer
        .index_codex_sessions(parent_only_dir.path())
        .unwrap();
    indexer.index_codex_sessions(child_only_dir.path()).unwrap();

    let conn = Connection::open(temp_db.path()).unwrap();
    let matching_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM subagents
             WHERE session_id = ?1 AND agent_id = ?2",
            rusqlite::params![PARENT_SESSION_ID, CHILD_SESSION_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(matching_rows, 2);

    let distinct_child_ids: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT child_session_id)
             FROM subagents
             WHERE session_id = ?1 AND agent_id = ?2",
            rusqlite::params![PARENT_SESSION_ID, CHILD_SESSION_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(distinct_child_ids, 1);
}

#[test]
fn codex_incremental_reindex_keeps_existing_child_link_when_child_file_is_skipped() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let root = fixture_override_root();
    let sources = SessionSources::resolve(Some(root.path()));

    let first_run = indexer.index_all_incremental(&sources).unwrap();
    let first_codex = first_run
        .per_source
        .iter()
        .find(|result| result.assistant == AiAssistant::Codex)
        .expect("Codex source result should exist");
    assert_eq!(first_codex.indexed, 2);

    let initial_subagent = load_subagent(temp_db.path(), PARENT_SESSION_ID, "call_spawn_1")
        .unwrap()
        .expect("parent subagent should exist after initial index");
    assert_eq!(
        initial_subagent.child_session_id.as_deref(),
        Some(CHILD_SESSION_ID)
    );

    append_harmless_parent_event(&root);

    let second_run = indexer.index_all_incremental(&sources).unwrap();
    let second_codex = second_run
        .per_source
        .iter()
        .find(|result| result.assistant == AiAssistant::Codex)
        .expect("Codex source result should exist");
    assert_eq!(second_codex.indexed, 1);
    assert_eq!(second_codex.skipped, 1);

    let subagent = load_subagent(temp_db.path(), PARENT_SESSION_ID, "call_spawn_1")
        .unwrap()
        .expect("parent subagent should exist after incremental reindex");
    assert_eq!(subagent.agent_id.as_deref(), Some(CHILD_SESSION_ID));
    assert_eq!(subagent.child_session_id.as_deref(), Some(CHILD_SESSION_ID));
}

const TASK_BASE: &str = "tests/fixtures/codex_subagent_linkage/2026/10/15";
const ROOT_ID: &str = "00000000-0000-7000-8000-000000000201";
const ANALYZE_ID: &str = "00000000-0000-7000-8000-000000000202";
const CHECK_ID: &str = "00000000-0000-7000-8000-000000000203";
const TASK_PARENT: &str = "task-parent";
const TASK_CHILD: &str = "task-child";
const TASK_GRANDCHILD: &str = "task-grandchild";

fn task_file(name: &str) -> String {
    format!("rollout-2026-10-15T10-00-00-{name}.jsonl")
}

/// Copies the named task fixtures into a fresh `2026/10/15` session tree.
fn task_fixture_dir(names: &[&str]) -> TempDir {
    let dir = tempdir().unwrap();
    let target = dir.path().join("2026/10/15");
    fs::create_dir_all(&target).unwrap();
    for name in names {
        fs::copy(
            Path::new(TASK_BASE).join(task_file(name)),
            target.join(task_file(name)),
        )
        .unwrap();
    }
    dir
}

/// Writes an edited copy of a task fixture under a new file name.
fn write_variant(dir: &TempDir, source: &str, target: &str, edit: impl Fn(&mut Vec<Value>)) {
    let text = fs::read_to_string(Path::new(TASK_BASE).join(task_file(source))).unwrap();
    let mut records: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    edit(&mut records);
    let out = records
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        dir.path().join("2026/10/15").join(task_file(target)),
        format!("{out}\n"),
    )
    .unwrap();
}

fn replace_in_value(value: &mut Value, from: &str, to: &str) {
    match value {
        Value::String(text) => *text = text.replace(from, to),
        Value::Array(items) => items.iter_mut().for_each(|v| replace_in_value(v, from, to)),
        Value::Object(map) => map.values_mut().for_each(|v| replace_in_value(v, from, to)),
        _ => {}
    }
}

fn replace_all(records: &mut [Value], replacements: &[(&str, &str)]) {
    for record in records {
        for (from, to) in replacements {
            replace_in_value(record, from, to);
        }
    }
}

/// Sets the JSON output of the function call with `call_id`.
fn set_spawn_output(records: &mut [Value], call_id: &str, output: Value) {
    let mut found = false;
    for record in records {
        let payload = &mut record["payload"];
        if payload["type"] == "function_call_output" && payload["call_id"] == call_id {
            payload["output"] = Value::String(output.to_string());
            found = true;
        }
    }
    assert!(found, "no output for {call_id}");
}

fn index_dir(indexer: &mut SessionIndexer, dir: &TempDir) {
    indexer.index_codex_sessions(dir.path()).unwrap();
}

fn assert_child_link(db: &Path, parent: &str, call_id: &str, expected: Option<&str>) {
    let subagent = load_subagent(db, parent, call_id)
        .unwrap()
        .unwrap_or_else(|| panic!("subagent {call_id} of {parent} should exist"));
    assert_eq!(
        subagent.child_session_id.as_deref(),
        expected,
        "link of {call_id}"
    );
}

fn assert_unlinked_in_sql(db: &Path, parent: &str) {
    let conn = Connection::open(db).unwrap();
    let linked: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM subagents
             WHERE session_id = ?1 AND child_session_id IS NOT NULL",
            [parent],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(linked, 0, "no row of {parent} may be linked");
    let rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM subagents WHERE session_id = ?1 AND child_session_id IS NULL",
            [parent],
            |row| row.get(0),
        )
        .unwrap();
    assert!(rows > 0, "{parent} should still have its rows");
}

fn permutations<'a>(items: &[&'a str]) -> Vec<Vec<&'a str>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut result = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let mut rest = items.to_vec();
        rest.remove(index);
        for mut tail in permutations(&rest) {
            tail.insert(0, item);
            result.push(tail);
        }
    }
    result
}

fn stored_agent_path(db: &Path, session_id: &str) -> Option<String> {
    Connection::open(db)
        .unwrap()
        .query_row(
            "SELECT agent_path FROM sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn codex_task_name_links_in_either_order() {
    for order in [[TASK_PARENT, TASK_CHILD], [TASK_CHILD, TASK_PARENT]] {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

        for name in order {
            index_dir(&mut indexer, &task_fixture_dir(&[name]));
        }

        assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", Some(ANALYZE_ID));
        let child = load_session(temp_db.path(), ANALYZE_ID).unwrap().unwrap();
        assert_eq!(
            child.first_prompt.as_deref(),
            Some("Delegated task: analyze")
        );
        assert_eq!(
            stored_agent_path(temp_db.path(), ANALYZE_ID).as_deref(),
            Some("/root/analyze")
        );
        let subagent = load_subagent(temp_db.path(), ROOT_ID, "spawn_analyze")
            .unwrap()
            .unwrap();
        assert_eq!(subagent.prompt.as_deref(), Some("Analyze the parser"));
        assert_eq!(subagent.agent_id, None);
    }
}

#[test]
fn codex_task_name_duplicate_child_retracts_link() {
    let duplicate = |dir: &TempDir| {
        write_variant(dir, TASK_CHILD, "task-child-dup", |records| {
            replace_all(records, &[("8000-000000000202", "8000-0000000002a2")]);
        });
    };
    let parent = task_fixture_dir(&[TASK_PARENT]);
    let child = task_fixture_dir(&[TASK_CHILD]);
    let dup = task_fixture_dir(&[]);
    duplicate(&dup);

    // Parent, child, then a second child with the same parent and path.
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    index_dir(&mut indexer, &parent);
    index_dir(&mut indexer, &child);
    assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", Some(ANALYZE_ID));
    index_dir(&mut indexer, &dup);
    assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", None);
    assert_unlinked_in_sql(temp_db.path(), ROOT_ID);

    // A parent reindex leaves the ambiguous row unlinked.
    index_dir(&mut indexer, &parent);
    assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", None);

    // Reverse arrival: both children first, then the parent.
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    index_dir(&mut indexer, &dup);
    index_dir(&mut indexer, &child);
    index_dir(&mut indexer, &parent);
    assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", None);
    assert_unlinked_in_sql(temp_db.path(), ROOT_ID);
}

#[test]
fn codex_task_name_duplicate_parent_rows_stay_unlinked() {
    let parent = task_fixture_dir(&[]);
    write_variant(&parent, TASK_PARENT, TASK_PARENT, |records| {
        let spawn = records
            .iter()
            .find(|r| {
                r["payload"]["call_id"] == "spawn_analyze" && r["payload"]["name"] == "spawn_agent"
            })
            .unwrap()
            .clone();
        let mut second = spawn;
        replace_all(
            std::slice::from_mut(&mut second),
            &[("spawn_analyze", "spawn_analyze_2")],
        );
        records.push(second);
        let mut output = serde_json::json!({
            "timestamp": "2026-10-15T10:00:05Z",
            "type": "response_item",
            "payload": {
                "type": "function_call_output",
                "call_id": "spawn_analyze_2",
                "output": serde_json::json!({"task_name": "/root/analyze"}).to_string(),
            },
        });
        records.push(output.take());
    });
    let child = task_fixture_dir(&[TASK_CHILD]);

    for parent_first in [true, false] {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        if parent_first {
            index_dir(&mut indexer, &parent);
            index_dir(&mut indexer, &child);
        } else {
            index_dir(&mut indexer, &child);
            index_dir(&mut indexer, &parent);
        }
        assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", None);
        assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze_2", None);
        assert_unlinked_in_sql(temp_db.path(), ROOT_ID);
    }
}

#[test]
fn codex_task_name_matches_full_path_and_immediate_parent() {
    let decoys = task_fixture_dir(&[]);
    // Same leaf `check`, different full path, correct immediate parent.
    write_variant(&decoys, TASK_GRANDCHILD, "decoy-leaf", |records| {
        replace_all(
            records,
            &[
                ("8000-000000000203", "8000-0000000002b1"),
                ("/root/analyze/check", "/root/other/check"),
            ],
        );
    });
    // Same full path, but the immediate parent is the root, not `analyze`.
    write_variant(&decoys, TASK_GRANDCHILD, "decoy-parent", |records| {
        replace_all(
            records,
            &[
                ("8000-000000000203", "8000-0000000002b2"),
                ("8000-000000000202", "8000-000000000201"),
            ],
        );
    });
    // Shares only a path suffix with the root's `analyze` spawn.
    write_variant(&decoys, TASK_GRANDCHILD, "decoy-suffix", |records| {
        replace_all(
            records,
            &[
                ("8000-000000000203", "8000-0000000002b3"),
                ("8000-000000000202", "8000-000000000201"),
                ("/root/analyze/check", "/x/root/analyze"),
            ],
        );
    });

    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    index_dir(&mut indexer, &decoys);
    index_dir(&mut indexer, &task_fixture_dir(&[TASK_PARENT]));
    index_dir(&mut indexer, &task_fixture_dir(&[TASK_CHILD]));

    assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", Some(ANALYZE_ID));
    assert_child_link(temp_db.path(), ANALYZE_ID, "spawn_check", None);

    // The genuine grandchild still links once it arrives.
    index_dir(&mut indexer, &task_fixture_dir(&[TASK_GRANDCHILD]));
    assert_child_link(temp_db.path(), ANALYZE_ID, "spawn_check", Some(CHECK_ID));
}

fn assert_nested_links(order: &[&str]) {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    for name in order {
        index_dir(&mut indexer, &task_fixture_dir(&[name]));
    }
    assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", Some(ANALYZE_ID));
    assert_child_link(temp_db.path(), ANALYZE_ID, "spawn_check", Some(CHECK_ID));
}

#[test]
fn codex_nested_delegation_links_all_edges_in_every_order() {
    let all = [TASK_PARENT, TASK_CHILD, TASK_GRANDCHILD];
    let orders = permutations(&all);
    assert_eq!(orders.len(), 6);

    for order in &orders {
        assert_nested_links(order);
    }

    // Same permutations with synthetic `agent_id` outputs (legacy regression).
    let with_ids = task_fixture_dir(&[]);
    write_variant(&with_ids, TASK_PARENT, TASK_PARENT, |records| {
        set_spawn_output(
            records,
            "spawn_analyze",
            serde_json::json!({"agent_id": ANALYZE_ID}),
        );
    });
    write_variant(&with_ids, TASK_CHILD, TASK_CHILD, |records| {
        set_spawn_output(
            records,
            "spawn_check",
            serde_json::json!({"agent_id": CHECK_ID}),
        );
    });
    fs::copy(
        Path::new(TASK_BASE).join(task_file(TASK_GRANDCHILD)),
        with_ids
            .path()
            .join("2026/10/15")
            .join(task_file(TASK_GRANDCHILD)),
    )
    .unwrap();

    for order in &orders {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        for name in order {
            let dir = task_fixture_dir(&[]);
            fs::copy(
                with_ids.path().join("2026/10/15").join(task_file(name)),
                dir.path().join("2026/10/15").join(task_file(name)),
            )
            .unwrap();
            index_dir(&mut indexer, &dir);
        }
        assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", Some(ANALYZE_ID));
        assert_child_link(temp_db.path(), ANALYZE_ID, "spawn_check", Some(CHECK_ID));
    }

    // A clean full index keeps one row per spawn and the same links.
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let everything = task_fixture_dir(&all);
    index_dir(&mut indexer, &everything);
    index_dir(&mut indexer, &everything);
    assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", Some(ANALYZE_ID));
    assert_child_link(temp_db.path(), ANALYZE_ID, "spawn_check", Some(CHECK_ID));
    let conn = Connection::open(temp_db.path()).unwrap();
    let rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM subagents WHERE session_id IN (?1, ?2)",
            [ROOT_ID, ANALYZE_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 2);
}

#[test]
fn codex_agent_id_link_precedes_task_name_fallback() {
    let decoy_path = "/root/decoy";
    let variants = task_fixture_dir(&[]);
    // Both identifiers resolve to the same child.
    write_variant(&variants, TASK_PARENT, "both-parent", |records| {
        set_spawn_output(
            records,
            "spawn_analyze",
            serde_json::json!({"agent_id": ANALYZE_ID, "task_name": "/root/analyze"}),
        );
    });
    // The ID names `analyze`, the task name names a different child.
    write_variant(&variants, TASK_PARENT, "conflict-parent", |records| {
        set_spawn_output(
            records,
            "spawn_analyze",
            serde_json::json!({"agent_id": ANALYZE_ID, "task_name": decoy_path}),
        );
    });
    write_variant(&variants, TASK_CHILD, "decoy-child", |records| {
        replace_all(
            records,
            &[
                ("8000-000000000202", "8000-0000000002c1"),
                ("/root/analyze", decoy_path),
            ],
        );
    });
    // The ID resolves to nothing, the task name to the real child.
    write_variant(&variants, TASK_PARENT, "ghost-parent", |records| {
        set_spawn_output(
            records,
            "spawn_analyze",
            serde_json::json!({"agent_id": "ghost-id", "task_name": "/root/analyze"}),
        );
    });

    let pick = |name: &str| {
        let dir = task_fixture_dir(&[]);
        let source = if name == TASK_CHILD {
            Path::new(TASK_BASE).join(task_file(name))
        } else {
            variants.path().join("2026/10/15").join(task_file(name))
        };
        fs::copy(source, dir.path().join("2026/10/15").join(task_file(name))).unwrap();
        dir
    };

    for order in permutations(&["both-parent", TASK_CHILD]) {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        for name in &order {
            index_dir(&mut indexer, &pick(name));
        }
        assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", Some(ANALYZE_ID));
    }

    for order in permutations(&["conflict-parent", TASK_CHILD, "decoy-child"]) {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        for name in &order {
            index_dir(&mut indexer, &pick(name));
        }
        assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", Some(ANALYZE_ID));
    }

    for order in permutations(&["ghost-parent", TASK_CHILD]) {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        for name in &order {
            index_dir(&mut indexer, &pick(name));
        }
        assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", Some(ANALYZE_ID));
    }
}

#[test]
fn codex_task_name_parent_only_incremental_reindex_keeps_link() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let root = tempdir().unwrap();
    let target = root.path().join("codex_sessions/2026/10/15");
    fs::create_dir_all(&target).unwrap();
    for name in [TASK_PARENT, TASK_CHILD] {
        fs::copy(
            Path::new(TASK_BASE).join(task_file(name)),
            target.join(task_file(name)),
        )
        .unwrap();
    }
    let sources = SessionSources::resolve(Some(root.path()));

    let first = indexer.index_all_incremental(&sources).unwrap();
    let first_codex = first
        .per_source
        .iter()
        .find(|result| result.assistant == AiAssistant::Codex)
        .unwrap();
    assert_eq!(first_codex.indexed, 2);
    assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", Some(ANALYZE_ID));

    let mut file = OpenOptions::new()
        .append(true)
        .open(target.join(task_file(TASK_PARENT)))
        .unwrap();
    writeln!(
        file,
        "{{\"type\":\"event_msg\",\"timestamp\":\"2026-10-15T10:00:30Z\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"marker\"}}}}"
    )
    .unwrap();

    let second = indexer.index_all_incremental(&sources).unwrap();
    let second_codex = second
        .per_source
        .iter()
        .find(|result| result.assistant == AiAssistant::Codex)
        .unwrap();
    assert_eq!(second_codex.indexed, 1);
    assert_eq!(second_codex.skipped, 1);
    assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", Some(ANALYZE_ID));

    // A duplicate path discovered later retracts the earlier name-based link.
    write_variant(&task_fixture_dir(&[]), TASK_CHILD, TASK_CHILD, |_| {});
    let text = fs::read_to_string(target.join(task_file(TASK_CHILD))).unwrap();
    fs::write(
        target.join(task_file("task-child-dup")),
        text.replace("8000-000000000202", "8000-0000000002d2"),
    )
    .unwrap();
    indexer.index_all_incremental(&sources).unwrap();
    assert_child_link(temp_db.path(), ROOT_ID, "spawn_analyze", None);
}
