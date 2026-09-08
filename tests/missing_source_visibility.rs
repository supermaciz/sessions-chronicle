mod helpers;

use helpers::TempDatabase;
use sessions_chronicle::database::analytics::load_analytics;
use sessions_chronicle::database::{
    count_all_sessions, count_pinned_sessions, load_message_full_content,
    load_message_previews_for_session, load_projects, load_session, load_session_by_id_for_filter,
    load_sessions_for_filter, load_subagent, search_sessions_for_filter,
};
use sessions_chronicle::models::{AiAssistant, DateFilter, ProjectFilter, SortOrder};

#[test]
fn missing_sessions_remain_in_all_list_queries() {
    let db = TempDatabase::new("missing_visibility_list");

    // Seed a pinned session with a transcript. `last_updated` is the highest
    // of the two top-level sessions so it sorts first under RecentActivity.
    db.connection
        .execute(
            "INSERT INTO sessions (
            id, tool, start_time, message_count, file_path, last_updated, pinned_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                "pinned_session",
                "claude_code",
                100i64,
                2i64,
                "/tmp/session.jsonl",
                200i64,
                150i64,
            ],
        )
        .unwrap();

    db.connection
        .execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
         VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                "pinned_session",
                0i64,
                "user",
                "first message content",
                100i64,
                Option::<String>::None
            ],
        )
        .unwrap();

    db.connection
        .execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
         VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                "pinned_session",
                1i64,
                "assistant",
                "second message response",
                200i64,
                Option::<String>::None
            ],
        )
        .unwrap();

    // Seed a second, unpinned top-level session with an older `last_updated`
    // so the two-element list actually exercises sort order, not just count.
    db.connection
        .execute(
            "INSERT INTO sessions (
            id, tool, start_time, message_count, file_path, last_updated
         ) VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                "older_session",
                "claude_code",
                50i64,
                1i64,
                "/tmp/older.jsonl",
                150i64,
            ],
        )
        .unwrap();

    // Seed a subagent/child session of `pinned_session` plus a `subagents`
    // link row, so child transcript loading through the same navigation path
    // the UI uses is actually exercised, not just the parent's own rows.
    db.connection
        .execute(
            "INSERT INTO sessions (
            id, tool, start_time, message_count, file_path, last_updated,
            parent_session_id, is_subagent
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                "child_session",
                "claude_code",
                100i64,
                1i64,
                "/tmp/session.jsonl",
                180i64,
                "pinned_session",
                1i64,
            ],
        )
        .unwrap();

    db.connection
        .execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
         VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                "child_session",
                0i64,
                "assistant",
                "child transcript content",
                100i64,
                Option::<String>::None
            ],
        )
        .unwrap();

    db.connection
        .execute(
            "INSERT INTO subagents (id, session_id, title, child_session_id)
         VALUES (?, ?, ?, ?)",
            rusqlite::params!["call_1", "pinned_session", "child agent", "child_session"],
        )
        .unwrap();

    // Get baseline results
    let list_before = load_sessions_for_filter(
        &db.path,
        AiAssistant::ALL,
        &ProjectFilter::AllSessions,
        &DateFilter::AnyTime,
        SortOrder::RecentActivity,
    )
    .unwrap();

    let pinned_before =
        count_pinned_sessions(&db.path, AiAssistant::ALL, &DateFilter::AnyTime).unwrap();
    let total_before =
        count_all_sessions(&db.path, AiAssistant::ALL, &DateFilter::AnyTime).unwrap();

    let session_before = load_session_by_id_for_filter(
        &db.path,
        AiAssistant::ALL,
        &ProjectFilter::AllSessions,
        "pinned_session",
        &DateFilter::AnyTime,
    )
    .unwrap();
    assert!(
        !session_before.is_empty(),
        "Session should be found before marking missing"
    );

    // Baseline: production loaders reach the parent's messages and, through
    // the `subagents` link, the child session's own transcript.
    let previews_before =
        load_message_previews_for_session(&db.path, "pinned_session", 10, 0, 100).unwrap();
    assert_eq!(previews_before.len(), 2);
    assert_eq!(previews_before[0].content_preview, "first message content");
    assert_eq!(
        previews_before[1].content_preview,
        "second message response"
    );
    let full_before = load_message_full_content(&db.path, "pinned_session", 1).unwrap();
    assert_eq!(full_before, "second message response");

    let subagent_before = load_subagent(&db.path, "pinned_session", "call_1")
        .unwrap()
        .expect("subagent link should exist before marking missing");
    assert_eq!(
        subagent_before.child_session_id.as_deref(),
        Some("child_session")
    );
    let child_before = load_session(&db.path, "child_session")
        .unwrap()
        .expect("child session should be loadable before marking parent missing");
    assert!(child_before.is_subagent);
    let child_previews_before =
        load_message_previews_for_session(&db.path, "child_session", 10, 0, 100).unwrap();
    assert_eq!(child_previews_before.len(), 1);
    assert_eq!(
        child_previews_before[0].content_preview,
        "child transcript content"
    );

    // Mark session as missing
    db.connection
        .execute(
            "UPDATE sessions SET source_missing=1, source_missing_detected_at=300 WHERE id=?",
            rusqlite::params!["pinned_session"],
        )
        .unwrap();

    // Verify results are unchanged
    let list_after = load_sessions_for_filter(
        &db.path,
        AiAssistant::ALL,
        &ProjectFilter::AllSessions,
        &DateFilter::AnyTime,
        SortOrder::RecentActivity,
    )
    .unwrap();

    assert_eq!(
        list_before.len(),
        2,
        "Should have exactly two top-level sessions before"
    );
    assert_eq!(
        list_before.len(),
        list_after.len(),
        "Session count should remain the same"
    );
    assert_eq!(
        list_before[0].id, "pinned_session",
        "Newer session should sort first before marking missing"
    );
    assert_eq!(
        list_before[1].id, "older_session",
        "Older session should sort second before marking missing"
    );
    assert_eq!(
        list_before[0].id, list_after[0].id,
        "Session should appear in same position in list"
    );
    assert_eq!(
        list_before[1].id, list_after[1].id,
        "Second session should keep its position in list"
    );
    assert_eq!(
        list_before[0].last_updated, list_after[0].last_updated,
        "Session last_updated should not change"
    );

    let pinned_after =
        count_pinned_sessions(&db.path, AiAssistant::ALL, &DateFilter::AnyTime).unwrap();
    assert_eq!(pinned_before, 1, "Should have exactly one pinned before");
    assert_eq!(
        pinned_before, pinned_after,
        "Pinned count should be unchanged"
    );

    let total_after = count_all_sessions(&db.path, AiAssistant::ALL, &DateFilter::AnyTime).unwrap();
    assert_eq!(total_before, 2, "Should have exactly two total before");
    assert_eq!(total_before, total_after, "Total count should be unchanged");

    // Session should still be loadable by ID with unchanged message content
    let session_after = load_session_by_id_for_filter(
        &db.path,
        AiAssistant::ALL,
        &ProjectFilter::AllSessions,
        "pinned_session",
        &DateFilter::AnyTime,
    )
    .unwrap();
    assert_eq!(session_after.len(), 1, "Should find exactly one session");
    let session_after_data = &session_after[0];
    assert_eq!(
        session_after_data.message_count, 2,
        "Message count should be unchanged"
    );

    // Verify actual message content is still loadable through the same
    // production loaders the UI uses, not a raw SQL bypass.
    let previews_after =
        load_message_previews_for_session(&db.path, "pinned_session", 10, 0, 100).unwrap();
    assert_eq!(previews_after.len(), 2);
    assert_eq!(previews_after[0].content_preview, "first message content");
    assert_eq!(previews_after[1].content_preview, "second message response");
    let full_after = load_message_full_content(&db.path, "pinned_session", 1).unwrap();
    assert_eq!(full_after, "second message response");

    // The link to the child session and the child's own transcript must
    // remain reachable and loadable once the parent is retained-missing.
    let subagent_after = load_subagent(&db.path, "pinned_session", "call_1")
        .unwrap()
        .expect("subagent link should survive marking the parent missing");
    assert_eq!(
        subagent_after.child_session_id.as_deref(),
        Some("child_session")
    );
    let child_after = load_session(&db.path, "child_session")
        .unwrap()
        .expect("child session should remain loadable after parent is marked missing");
    assert!(child_after.is_subagent);
    let child_previews_after =
        load_message_previews_for_session(&db.path, "child_session", 10, 0, 100).unwrap();
    assert_eq!(child_previews_after.len(), 1);
    assert_eq!(
        child_previews_after[0].content_preview,
        "child transcript content"
    );
}

#[test]
fn missing_sessions_remain_in_search_results() {
    let db = TempDatabase::new("missing_visibility_search");

    // Seed a session with searchable content
    db.connection
        .execute(
            "INSERT INTO sessions (
            id, tool, start_time, message_count, file_path, last_updated
         ) VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                "search_session",
                "claude_code",
                100i64,
                1i64,
                "/tmp/session.jsonl",
                200i64,
            ],
        )
        .unwrap();

    db.connection
        .execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
         VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                "search_session",
                0i64,
                "user",
                "searchable needle word",
                100i64,
                Option::<String>::None
            ],
        )
        .unwrap();

    // Get baseline search results
    let search_before = search_sessions_for_filter(
        &db.path,
        AiAssistant::ALL,
        &ProjectFilter::AllSessions,
        "needle",
        &DateFilter::AnyTime,
        None,
    )
    .unwrap();

    assert_eq!(search_before.len(), 1);
    assert_eq!(search_before[0].id, "search_session");

    // Mark session as missing
    db.connection
        .execute(
            "UPDATE sessions SET source_missing=1, source_missing_detected_at=300 WHERE id=?",
            rusqlite::params!["search_session"],
        )
        .unwrap();

    // Verify search results are unchanged
    let search_after = search_sessions_for_filter(
        &db.path,
        AiAssistant::ALL,
        &ProjectFilter::AllSessions,
        "needle",
        &DateFilter::AnyTime,
        None,
    )
    .unwrap();

    assert_eq!(
        search_before.len(),
        search_after.len(),
        "Search result count should be unchanged"
    );
    assert_eq!(search_after[0].id, "search_session");
}

#[test]
fn missing_sessions_remain_in_analytics() {
    let db = TempDatabase::new("missing_visibility_analytics");

    // Seed a session with token data
    db.connection
        .execute(
            "INSERT INTO sessions (
            id, tool, start_time, message_count, file_path, last_updated,
            input_tokens, output_tokens
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                "analytics_session",
                "claude_code",
                100i64,
                1i64,
                "/tmp/session.jsonl",
                200i64,
                1000i64,
                500i64,
            ],
        )
        .unwrap();

    // Get baseline analytics
    let analytics_before = load_analytics(&db.path).unwrap();
    let total_before = analytics_before.overview.total_sessions;
    let messages_before = analytics_before.overview.total_messages;

    assert!(total_before > 0);
    assert!(messages_before > 0);

    // Mark session as missing
    db.connection
        .execute(
            "UPDATE sessions SET source_missing=1, source_missing_detected_at=300 WHERE id=?",
            rusqlite::params!["analytics_session"],
        )
        .unwrap();

    // Verify analytics are unchanged
    let analytics_after = load_analytics(&db.path).unwrap();
    assert_eq!(
        analytics_before.overview.total_sessions, analytics_after.overview.total_sessions,
        "Total sessions in analytics should be unchanged"
    );
    assert_eq!(
        analytics_before.overview.total_messages, analytics_after.overview.total_messages,
        "Total messages in analytics should be unchanged"
    );
}

#[test]
fn missing_sessions_remain_in_projects() {
    let db = TempDatabase::new("missing_visibility_projects");

    // Seed a project and session
    db.connection
        .execute(
            "INSERT INTO projects (id, path, name) VALUES (?, ?, ?)",
            rusqlite::params![1i64, "/projects/alpha", "alpha"],
        )
        .unwrap();

    db.connection
        .execute(
            "INSERT INTO sessions (
            id, tool, start_time, message_count, file_path, last_updated, project_id, project_path
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                "project_session",
                "claude_code",
                100i64,
                1i64,
                "/tmp/session.jsonl",
                200i64,
                1i64,
                "/projects/alpha",
            ],
        )
        .unwrap();

    // Get baseline projects
    let projects_before = load_projects(&db.path, AiAssistant::ALL, &DateFilter::AnyTime).unwrap();
    assert_eq!(
        projects_before.len(),
        1,
        "Should have exactly one project before"
    );
    assert_eq!(projects_before[0].name, "alpha");

    // Mark session as missing
    db.connection
        .execute(
            "UPDATE sessions SET source_missing=1, source_missing_detected_at=300 WHERE id=?",
            rusqlite::params!["project_session"],
        )
        .unwrap();

    // Verify projects are unchanged
    let projects_after = load_projects(&db.path, AiAssistant::ALL, &DateFilter::AnyTime).unwrap();
    assert_eq!(
        projects_before.len(),
        projects_after.len(),
        "Project count should be unchanged"
    );
    assert_eq!(
        projects_before[0].name, projects_after[0].name,
        "Project name should be unchanged"
    );
    assert_eq!(
        projects_after.len(),
        1,
        "Should have exactly one project after"
    );
}
