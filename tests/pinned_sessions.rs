mod helpers;

use helpers::TempDatabase;
use sessions_chronicle::database::{
    count_pinned_sessions, load_sessions_for_filter, search_sessions_for_filter, toggle_pin,
};
use sessions_chronicle::models::{AiAssistant, DateFilter, ProjectFilter, SortOrder};

fn insert_session(db: &TempDatabase, id: &str, tool: &str, pinned_at: Option<i64>) {
    db.connection
        .execute(
            "INSERT INTO sessions (
                id, tool, start_time, message_count, file_path, last_updated, pinned_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                id,
                tool,
                1_i64,
                1_i64,
                format!("/tmp/{id}.jsonl"),
                2_i64,
                pinned_at,
            ],
        )
        .unwrap();
}

#[test]
fn toggle_pin_flips_state_and_returns_new_state() {
    let db = TempDatabase::new("pin-toggle");
    insert_session(&db, "session-a", "claude_code", None);

    assert!(toggle_pin(&db.path, "session-a").unwrap());
    assert!(!toggle_pin(&db.path, "session-a").unwrap());
}

#[test]
fn count_pinned_sessions_respects_tool_filter() {
    let db = TempDatabase::new("pin-count");
    insert_session(&db, "claude-pinned", "claude_code", Some(10));
    insert_session(&db, "open-pinned", "opencode", Some(20));
    insert_session(&db, "open-unpinned", "opencode", None);

    assert_eq!(
        count_pinned_sessions(&db.path, AiAssistant::ALL, &DateFilter::AnyTime).unwrap(),
        2
    );
    assert_eq!(
        count_pinned_sessions(&db.path, &[AiAssistant::ClaudeCode], &DateFilter::AnyTime).unwrap(),
        1
    );
}

#[test]
fn load_sessions_for_filter_uses_pinned_project_filter() {
    let db = TempDatabase::new("pin-filter-load");
    insert_session(&db, "a", "claude_code", Some(10));
    insert_session(&db, "b", "claude_code", None);
    insert_session(&db, "c", "opencode", None);

    let pinned = load_sessions_for_filter(
        &db.path,
        AiAssistant::ALL,
        &ProjectFilter::Pinned,
        &DateFilter::AnyTime,
        SortOrder::RecentActivity,
    )
    .unwrap();

    assert_eq!(
        pinned
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a"]
    );
}

#[test]
fn search_sessions_for_filter_uses_pinned_project_filter() {
    let db = TempDatabase::new("pin-filter-search");
    insert_session(&db, "a", "claude_code", Some(10));
    insert_session(&db, "b", "claude_code", None);

    db.connection
        .execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params!["a", 0_i64, "user", "needle", 1_i64, Option::<String>::None],
        )
        .unwrap();
    db.connection
        .execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params!["b", 0_i64, "user", "needle", 1_i64, Option::<String>::None],
        )
        .unwrap();

    let sessions = search_sessions_for_filter(
        &db.path,
        AiAssistant::ALL,
        &ProjectFilter::Pinned,
        "needle",
        &DateFilter::AnyTime,
        None,
    )
    .unwrap();

    let pinned_ids: Vec<String> = sessions.into_iter().map(|session| session.id).collect();
    assert_eq!(pinned_ids, vec!["a"]);
}

#[test]
fn pinned_filter_returns_sessions_across_all_projects() {
    let db = TempDatabase::new("pin-cross-project");

    db.connection
        .execute(
            "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
            rusqlite::params![1_i64, "/projects/alpha", "alpha"],
        )
        .unwrap();
    db.connection
        .execute(
            "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
            rusqlite::params![2_i64, "/projects/beta", "beta"],
        )
        .unwrap();

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated, pinned_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                "alpha-pin",
                "claude_code",
                Some("/projects/alpha"),
                Some(1_i64),
                10_i64,
                1_i64,
                "/tmp/alpha-pin.jsonl",
                100_i64,
                Some(111_i64),
            ],
        )
        .unwrap();
    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated, pinned_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                "beta-pin",
                "claude_code",
                Some("/projects/beta"),
                Some(2_i64),
                20_i64,
                1_i64,
                "/tmp/beta-pin.jsonl",
                200_i64,
                Some(222_i64),
            ],
        )
        .unwrap();
    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated, pinned_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                "alpha-unpinned",
                "claude_code",
                Some("/projects/alpha"),
                Some(1_i64),
                30_i64,
                1_i64,
                "/tmp/alpha-unpinned.jsonl",
                300_i64,
                Option::<i64>::None,
            ],
        )
        .unwrap();

    let sessions = load_sessions_for_filter(
        &db.path,
        AiAssistant::ALL,
        &ProjectFilter::Pinned,
        &DateFilter::AnyTime,
        SortOrder::RecentActivity,
    )
    .unwrap();

    let ids: Vec<&str> = sessions.iter().map(|session| session.id.as_str()).collect();
    assert_eq!(ids, vec!["beta-pin", "alpha-pin"]);
}
