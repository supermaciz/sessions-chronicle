mod helpers;

use helpers::TempDatabase;
use sessions_chronicle::database::{
    count_pinned_sessions, load_sessions_for_filter, search_sessions_for_filter, toggle_pin,
};
use sessions_chronicle::models::{AiAssistant, ProjectFilter};

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
        count_pinned_sessions(&db.path, AiAssistant::ALL).unwrap(),
        2
    );
    assert_eq!(
        count_pinned_sessions(&db.path, &[AiAssistant::ClaudeCode]).unwrap(),
        1
    );
}

#[test]
fn load_sessions_for_filter_respects_pinned_only() {
    let db = TempDatabase::new("pin-filter-load");
    insert_session(&db, "a", "claude_code", Some(10));
    insert_session(&db, "b", "claude_code", None);
    insert_session(&db, "c", "opencode", None);

    let pinned = load_sessions_for_filter(&db.path, AiAssistant::ALL, &ProjectFilter::AllSessions)
        .unwrap()
        .into_iter()
        .filter(|session| session.pinned_at.is_some())
        .collect::<Vec<_>>();

    assert_eq!(
        pinned
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a"]
    );
}

#[test]
fn search_sessions_for_filter_respects_pinned_only() {
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
        &ProjectFilter::AllSessions,
        "needle",
    )
    .unwrap();

    let pinned_ids: Vec<String> = sessions
        .into_iter()
        .filter(|session| session.pinned_at.is_some())
        .map(|session| session.id)
        .collect();
    assert_eq!(pinned_ids, vec!["a"]);
}
