use super::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use relm4::{Component, ComponentController};
use rusqlite::{Connection, params};

fn build_test_session(
    first_prompt: Option<&str>,
    token_usage: Option<crate::models::TokenUsage>,
    edit_count: usize,
    command_count: usize,
    read_count: usize,
) -> Session {
    use chrono::{TimeZone, Utc};

    Session {
        id: "test-session-123".to_string(),
        tool: crate::models::AiAssistant::ClaudeCode,
        project_path: Some("/tmp/project".to_string()),
        project_id: None,
        start_time: Utc.with_ymd_and_hms(2026, 3, 30, 10, 0, 0).unwrap(),
        message_count: 42,
        file_path: "/tmp/test.json".to_string(),
        last_updated: Utc.with_ymd_and_hms(2026, 3, 30, 12, 14, 0).unwrap(),
        pinned_at: None,
        first_prompt: first_prompt.map(|s| s.to_string()),
        parent_session_id: None,
        is_subagent: false,
        token_usage,
        edit_count,
        read_count,
        command_count,
        ending_status: crate::models::SessionEndingStatus::Clean,
    }
}

fn pump_main_context(condition: impl Fn() -> bool) {
    let context = gtk::glib::MainContext::default();
    let deadline = std::time::Instant::now() + Duration::from_millis(1000);
    while std::time::Instant::now() < deadline {
        if condition() {
            return;
        }

        if !context.iteration(false) {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

fn drain_main_context() {
    let context = gtk::glib::MainContext::default();
    for _ in 0..20 {
        if !context.iteration(false) {
            break;
        }
    }
}

fn seed_message_transcript(db_path: &std::path::Path, session_id: &str, count: usize) {
    let conn = Connection::open(db_path).expect("open temp db");
    crate::database::schema::initialize_database(&conn).expect("initialize db");

    for index in 0..count {
        conn.execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, 'user', ?3, ?4, NULL)",
            params![
                session_id,
                index as i64,
                format!("message {index}"),
                index as i64,
            ],
        )
        .expect("insert message");
        conn.execute(
            "INSERT INTO transcript_items (session_id, item_index, kind, message_index)
                 VALUES (?1, ?2, 'message', ?2)",
            params![session_id, index as i64],
        )
        .expect("insert transcript item");
    }
}

fn seed_search_transcript(
    db_path: &std::path::Path,
    session_id: &str,
    count: usize,
    matching_indexes: &[usize],
) {
    let conn = Connection::open(db_path).expect("open temp db");
    crate::database::schema::initialize_database(&conn).expect("initialize db");

    for index in 0..count {
        let content = if matching_indexes.contains(&index) {
            format!("needle message {index}")
        } else {
            format!("ordinary message {index}")
        };
        conn.execute(
            "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, 'user', ?3, ?4, NULL)",
            params![session_id, index as i64, content, index as i64],
        )
        .expect("insert search message");
        conn.execute(
            "INSERT INTO transcript_items (session_id, item_index, kind, message_index)
                 VALUES (?1, ?2, 'message', ?2)",
            params![session_id, index as i64],
        )
        .expect("insert search transcript item");
    }
}

fn transcript_message_row(
    item_index: i64,
    role: crate::models::Role,
    content: &str,
) -> crate::database::TranscriptItemRow {
    crate::database::TranscriptItemRow {
        item_index,
        kind: crate::models::TranscriptItemKind::Message,
        reasoning_preview: crate::models::ReasoningPreview::default(),
        message_index: Some(item_index),
        role: Some(role),
        content_preview: Some(content.to_string()),
        content_len: Some(content.len() as i64),
        timestamp: Some(item_index),
        model: None,
        tool_call_id: None,
        tool_name: None,
        tool_status: None,
        tool_summary: None,
        tool_input_json: None,
        tool_output_text: None,
        duration_ms: None,
        subagent_id: None,
        subagent_title: None,
        subagent_prompt: None,
    }
}

fn transcript_tool_row(item_index: i64, tool_name: &str) -> crate::database::TranscriptItemRow {
    crate::database::TranscriptItemRow {
        item_index,
        kind: crate::models::TranscriptItemKind::ToolCall,
        reasoning_preview: crate::models::ReasoningPreview::default(),
        message_index: None,
        role: None,
        content_preview: None,
        content_len: None,
        timestamp: None,
        model: None,
        tool_call_id: Some(format!("call-{item_index}")),
        tool_name: Some(tool_name.to_string()),
        tool_status: Some(crate::models::ToolCallStatus::Completed),
        tool_summary: Some(format!("{tool_name} summary")),
        tool_input_json: Some("{}".to_string()),
        tool_output_text: None,
        duration_ms: Some(1),
        subagent_id: None,
        subagent_title: None,
        subagent_prompt: None,
    }
}

fn transcript_subagent_row(item_index: i64, title: &str) -> crate::database::TranscriptItemRow {
    crate::database::TranscriptItemRow {
        item_index,
        kind: crate::models::TranscriptItemKind::Subagent,
        reasoning_preview: crate::models::ReasoningPreview::default(),
        message_index: None,
        role: None,
        content_preview: None,
        content_len: None,
        timestamp: None,
        model: None,
        tool_call_id: None,
        tool_name: None,
        tool_status: None,
        tool_summary: None,
        tool_input_json: None,
        tool_output_text: None,
        duration_ms: None,
        subagent_id: Some(format!("subagent-{item_index}")),
        subagent_title: Some(title.to_string()),
        subagent_prompt: Some("investigate".to_string()),
    }
}

#[test]
fn build_display_items_groups_two_tool_calls_into_one_tool_burst() {
    let rows = vec![
        transcript_message_row(0, crate::models::Role::Assistant, "hello"),
        transcript_tool_row(1, "Read"),
        transcript_tool_row(2, "Edit"),
    ];
    let matched_item_indexes = BTreeSet::new();

    let prepared = SessionDetail::build_display_items(
        rows,
        "session-1",
        None,
        &matched_item_indexes,
        Arc::new(PathBuf::from("/tmp/test.db")),
        0,
    );

    assert_eq!(prepared.items.len(), 2);
    assert!(matches!(
        prepared.items[1],
        TranscriptItemInit::ToolBurst(_)
    ));
    assert_eq!(
        prepared.display_targets_by_item_index.get(&0),
        Some(&ScrollTarget {
            display_index: 0,
            child_index: None,
        })
    );
    assert_eq!(
        prepared.display_targets_by_item_index.get(&1),
        Some(&ScrollTarget {
            display_index: 1,
            child_index: Some(0),
        })
    );
    assert_eq!(
        prepared.display_targets_by_item_index.get(&2),
        Some(&ScrollTarget {
            display_index: 1,
            child_index: Some(1),
        })
    );
}

#[test]
fn build_display_items_limits_search_highlight_to_navigable_matches() {
    let rows = vec![
        transcript_message_row(0, crate::models::Role::Assistant, "needle in counted row"),
        transcript_tool_row(1, "Read"),
        transcript_message_row(
            2,
            crate::models::Role::Assistant,
            "needle outside fts result",
        ),
    ];
    // Only item 0 is an FTS match that Next/Previous can navigate to.
    let matched_item_indexes = BTreeSet::from([0_i64]);

    let prepared = SessionDetail::build_display_items(
        rows,
        "session-1",
        Some("needle".to_string()),
        &matched_item_indexes,
        Arc::new(PathBuf::from("/tmp/test.db")),
        0,
    );

    match &prepared.items[0] {
        TranscriptItemInit::Message(message) => {
            assert_eq!(
                message.highlight_query.as_deref(),
                Some("needle"),
                "a matched message row must be highlighted"
            );
        }
        other => panic!(
            "expected first item to be a message, got {:?}",
            std::mem::discriminant(other)
        ),
    }

    match &prepared.items[1] {
        TranscriptItemInit::ToolCall(tool_call) => {
            assert_eq!(
                tool_call.highlight_query.as_deref(),
                None,
                "tool-call rows are not in the FTS match list and must not be highlighted"
            );
        }
        other => panic!(
            "expected second item to be a tool call, got {:?}",
            std::mem::discriminant(other)
        ),
    }

    match &prepared.items[2] {
        TranscriptItemInit::Message(message) => {
            assert_eq!(
                message.highlight_query.as_deref(),
                None,
                "a message row outside the FTS match list must not be highlighted"
            );
        }
        other => panic!(
            "expected third item to be a message, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

#[gtk::test]
fn session_detail_defers_initial_transcript_load_after_session_change() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    seed_message_transcript(temp_db.path(), "test-session-123", 75);

    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });

    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.session.is_some() && parts.model.transcript.loading
    });

    let context = gtk::glib::MainContext::default();
    let deadline = std::time::Instant::now() + Duration::from_millis(50);
    while std::time::Instant::now() < deadline {
        context.iteration(false);
        std::thread::sleep(Duration::from_millis(2));
    }

    let parts = controller.state().get();
    assert_eq!(parts.model.transcript.loaded_count, 0);
    assert_eq!(parts.model.messages.len(), 0);
}

#[gtk::test]
fn session_open_timestamp_tracks_active_session_lifecycle() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    seed_message_transcript(temp_db.path(), "test-session-123", 75);

    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });

    pump_main_context(|| {
        controller
            .state()
            .get()
            .model
            .transcript
            .load_started_at
            .is_some()
    });
    assert!(
        controller
            .state()
            .get()
            .model
            .transcript
            .load_started_at
            .is_some()
    );

    controller.emit(SessionDetailMsg::Clear);
    pump_main_context(|| controller.state().get().model.session.is_none());
    assert!(
        controller
            .state()
            .get()
            .model
            .transcript
            .load_started_at
            .is_none()
    );
}

#[gtk::test]
fn clear_message_clears_rows_and_targets() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    seed_message_transcript(temp_db.path(), "test-session-123", 75);

    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });

    pump_main_context(|| {
        let parts = controller.state().get();
        !parts.model.transcript.loading && parts.model.messages.len() as usize == 75
    });

    controller.emit(SessionDetailMsg::Clear);
    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.session.is_none()
    });

    let parts = controller.state().get();
    assert_eq!(parts.model.messages.len(), 0);
    assert!(
        parts
            .model
            .transcript
            .display_targets_by_item_index
            .is_empty()
    );
}

#[test]
fn search_positions_loaded_debug_includes_load_duration() {
    let cmd = SessionDetailCmd::SearchPositionsLoaded {
        request_id: 7,
        session_id: "session-1".to_string(),
        load_duration_ms: 12,
        result: Ok(vec![MatchPosition { item_index: 3 }]),
    };

    let debug = format!("{cmd:?}");
    assert!(debug.contains("load_duration_ms"));
    assert!(debug.contains("12"));
}

#[test]
fn inspector_visibility_output_carries_state() {
    let output = SessionDetailOutput::InspectorVisibilityChanged(true);
    assert!(matches!(
        output,
        SessionDetailOutput::InspectorVisibilityChanged(true)
    ));
}

#[gtk::test]
fn inspect_tool_call_opens_inspector_when_session_active() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });
    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.session.is_some()
    });

    controller.emit(SessionDetailMsg::InspectToolCall("call-123".to_string()));
    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.inspector_open
    });

    let parts = controller.state().get();
    assert!(parts.model.inspector_open);
}

#[gtk::test]
fn close_inspector_resets_inspector_open() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });
    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.session.is_some()
    });

    controller.emit(SessionDetailMsg::InspectToolCall("call-1".to_string()));
    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.inspector_open
    });

    controller.emit(SessionDetailMsg::CloseInspector);
    pump_main_context(|| {
        let parts = controller.state().get();
        !parts.model.inspector_open
    });

    let parts = controller.state().get();
    assert!(!parts.model.inspector_open);
}

#[gtk::test]
fn uncollapsing_split_does_not_open_inspector() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });
    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.session.is_some()
    });

    // The responsive breakpoint collapses the inspector split when the detail
    // area is narrow and uncollapses it once there is room (e.g. as the page
    // slides in during navigation). AdwOverlaySplitView shows the sidebar
    // automatically on uncollapse unless it is pinned, which would otherwise
    // flip `inspector_open` to true with no selection — opening an empty pane.
    {
        let parts = controller.state().get();
        parts.widgets.inspector_split.set_collapsed(true);
        parts.widgets.inspector_split.set_collapsed(false);
    }
    // Give any spurious show-sidebar notify a chance to propagate into the model.
    drain_main_context();

    let parts = controller.state().get();
    assert!(
        !parts.model.inspector_open,
        "uncollapsing the split must not open the inspector"
    );
}

#[gtk::test]
fn session_detail_transcript_list_is_direct_scrolled_window_child() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

    let parts = controller.state().get();
    assert_eq!(
        parts.widgets.transcript_scroller.child(),
        Some(parts.model.messages.view.clone().upcast())
    );
}

#[gtk::test]
fn session_detail_summary_popover_hosts_summary_root() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

    let parts = controller.state().get();
    assert_eq!(
        parts.widgets.summary_popover.child(),
        Some(parts.widgets.summary.widget().clone().upcast())
    );
    assert_eq!(
        parts.widgets.summary_popover.width_request(),
        SUMMARY_POPOVER_WIDTH
    );
    assert!(
        parts
            .widgets
            .summary
            .widget()
            .clone()
            .downcast::<gtk::ScrolledWindow>()
            .is_ok(),
        "summary template root should be a bounded ScrolledWindow"
    );
}

#[gtk::test]
fn session_detail_content_column_contains_only_transcript_scroller() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

    let parts = controller.state().get();
    let content = parts
        .widgets
        .transcript_scroller
        .parent()
        .and_then(|widget| widget.downcast::<gtk::Box>().ok())
        .expect("transcript scroller should be inside the detail content box");

    assert_eq!(content.observe_children().n_items(), 1);
    assert_eq!(
        content.first_child(),
        Some(parts.widgets.transcript_scroller.clone().upcast())
    );
}

#[gtk::test]
fn update_search_query_populates_match_positions_and_reloads_first_page() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    seed_search_transcript(temp_db.path(), "test-session-123", 85, &[10, 80]);

    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });
    pump_main_context(|| {
        let parts = controller.state().get();
        !parts.model.transcript.loading && parts.model.transcript.loaded_count == 85
    });

    controller.emit(SessionDetailMsg::UpdateSearchQuery(Some(
        "needle".to_string(),
    )));
    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.search.match_positions.len() == 2
            && !parts.model.transcript.loading
            && parts
                .model
                .transcript
                .display_targets_by_item_index
                .contains_key(&10)
    });

    let parts = controller.state().get();
    let indexes: Vec<i64> = parts
        .model
        .search
        .match_positions
        .iter()
        .map(|p| p.item_index)
        .collect();
    assert_eq!(indexes, vec![10, 80]);
    assert_eq!(parts.model.search.current_match, 0);
    assert_eq!(parts.model.search.query.as_deref(), Some("needle"));
}

#[gtk::test]
fn search_highlight_is_limited_to_navigable_message_matches() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    seed_search_transcript(temp_db.path(), "test-session-123", 12, &[3, 9]);

    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });
    pump_main_context(|| {
        let parts = controller.state().get();
        !parts.model.transcript.loading && parts.model.transcript.loaded_count == 12
    });

    controller.emit(SessionDetailMsg::UpdateSearchQuery(Some(
        "needle".to_string(),
    )));
    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.search.match_positions.len() == 2 && !parts.model.transcript.loading
    });

    let parts = controller.state().get();
    let highlighted: Vec<(i64, bool)> = parts
        .model
        .messages
        .iter()
        .filter_map(|item| {
            let data = item.borrow();
            data.transcript_item_index
                .map(|idx| (idx, data.highlight_query.is_some()))
        })
        .collect();

    assert_eq!(highlighted.len(), 12);
    for (idx, has_highlight) in highlighted {
        let is_navigable_match = idx == 3 || idx == 9;
        assert_eq!(
            has_highlight, is_navigable_match,
            "row {idx} highlight must match whether Next/Previous can reach it"
        );
    }
}

#[gtk::test]
fn stale_search_result_is_discarded() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    seed_search_transcript(temp_db.path(), "test-session-123", 75, &[5]);

    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });
    pump_main_context(|| controller.state().get().model.session.is_some());

    controller.emit(SessionDetailMsg::UpdateSearchQuery(Some(
        "needle".to_string(),
    )));
    controller.emit(SessionDetailMsg::UpdateSearchQuery(Some(
        "ordinary".to_string(),
    )));
    let active_request = controller.state().get().model.search.request_id;
    controller.emit(SessionDetailMsg::SetMatchPositions {
        request_id: active_request.wrapping_sub(1),
        session_id: "test-session-123".to_string(),
        positions: vec![crate::database::MatchPosition { item_index: 5 }],
    });

    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.search.query.as_deref() == Some("ordinary")
    });

    let parts = controller.state().get();
    assert!(parts.model.search.match_positions.is_empty());
}

#[gtk::test]
fn list_view_row_widget_from_child_accepts_realized_widget_child() {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 0).upcast::<gtk::Widget>();

    let resolved = SessionDetail::list_view_row_widget_from_child(row.clone().upcast())
        .expect("direct widget child should resolve");

    assert_eq!(resolved, row);
}

/// Builds a realized `GtkListView` whose rows carry `transcript-row-{index}`
/// names, mirroring how the typed transcript view names its row roots.
fn realized_named_list_view(row_count: u32) -> (gtk::Window, gtk::ListView) {
    let items: Vec<String> = (0..row_count).map(|i| i.to_string()).collect();
    let item_refs: Vec<&str> = items.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&item_refs);
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, list_item| {
        let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
        list_item.set_child(Some(&gtk::Label::new(Some("row"))));
    });
    factory.connect_bind(|_, list_item| {
        let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
        let pos = list_item.position();
        if let Some(child) = list_item.child() {
            child.set_widget_name(&format!("{TRANSCRIPT_ROW_WIDGET_NAME_PREFIX}{pos}"));
        }
    });

    let selection = gtk::NoSelection::new(Some(model));
    let list_view = gtk::ListView::new(Some(selection), Some(factory));
    let window = gtk::Window::new();
    window.set_default_size(400, 600);
    window.set_child(Some(&list_view));
    window.present();

    let deadline = std::time::Instant::now() + Duration::from_millis(800);
    let context = gtk::glib::MainContext::default();
    while std::time::Instant::now() < deadline && list_view.observe_children().n_items() == 0 {
        context.iteration(true);
    }

    (window, list_view)
}

#[gtk::test]
fn observed_row_widget_finds_named_row_through_list_item_wrapper() {
    let (window, list_view) = realized_named_list_view(3);

    let resolved = SessionDetail::observed_row_widget_for_display_index(&list_view, 1)
        .expect("row 1 widget must be resolvable from the realized ListView");
    assert_eq!(
        resolved.widget_name().as_str(),
        "transcript-row-1",
        "the resolved widget must be the named transcript row root, \
             not the GtkListView's internal list-item wrapper"
    );

    window.destroy();
}

#[gtk::test]
fn row_widget_matches_display_index_uses_bound_row_identity() {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 0).upcast::<gtk::Widget>();
    row.set_widget_name("transcript-row-7");

    assert!(SessionDetail::row_widget_matches_display_index(&row, 7));
    assert!(!SessionDetail::row_widget_matches_display_index(&row, 8));
}

#[gtk::test]
fn clamped_scroll_value_preserves_valid_value_and_caps_overflow() {
    let adjustment = gtk::Adjustment::new(25.0, 0.0, 100.0, 1.0, 10.0, 30.0);

    assert_eq!(SessionDetail::clamped_scroll_value(&adjustment, 25.0), 25.0);
    assert_eq!(SessionDetail::clamped_scroll_value(&adjustment, -10.0), 0.0);
    assert_eq!(SessionDetail::clamped_scroll_value(&adjustment, 90.0), 70.0);
}

#[test]
fn message_full_content_target_rejects_stale_session_or_message() {
    let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
    let prepared = SessionDetail::build_display_items(
        vec![transcript_message_row(
            0,
            crate::models::Role::User,
            "preview",
        )],
        "session-1",
        None,
        &BTreeSet::new(),
        Arc::new(PathBuf::from("/tmp/test.db")),
        0,
    );
    let item = TranscriptItemData::from_init(
        prepared.items.into_iter().next().expect("message item"),
        sender,
    );

    assert!(SessionDetail::message_full_content_target_matches(
        &item,
        Some("session-1"),
        "session-1",
        0,
    ));
    assert!(!SessionDetail::message_full_content_target_matches(
        &item,
        Some("session-2"),
        "session-1",
        0,
    ));
    assert!(!SessionDetail::message_full_content_target_matches(
        &item,
        Some("session-1"),
        "session-1",
        1,
    ));
}

#[test]
fn message_full_content_failure_resets_expansion() {
    let (sender, _receiver) = relm4::channel::<SessionDetailMsg>();
    let prepared = SessionDetail::build_display_items(
        vec![transcript_message_row(
            0,
            crate::models::Role::User,
            "preview",
        )],
        "session-1",
        None,
        &BTreeSet::new(),
        Arc::new(PathBuf::from("/tmp/test.db")),
        0,
    );
    let item = TranscriptItemData::from_init(
        prepared.items.into_iter().next().expect("message item"),
        sender,
    );
    item.expanded.set(true);

    SessionDetail::reset_message_expansion_after_full_content_failure(&item);

    assert!(!item.expanded.get());
}

#[gtk::test]
fn jump_to_loaded_match_scrolls_without_loading() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    seed_search_transcript(temp_db.path(), "test-session-123", 75, &[10, 20]);

    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });
    pump_main_context(|| {
        let parts = controller.state().get();
        !parts.model.transcript.loading && parts.model.transcript.loaded_count == 75
    });

    let active_request = controller.state().get().model.search.request_id;
    controller.emit(SessionDetailMsg::SetMatchPositions {
        request_id: active_request,
        session_id: "test-session-123".to_string(),
        positions: vec![
            MatchPosition { item_index: 10 },
            MatchPosition { item_index: 20 },
        ],
    });
    pump_main_context(|| {
        let parts = controller.state().get();
        !parts.model.search.loading_jump
            && parts
                .model
                .transcript
                .display_targets_by_item_index
                .contains_key(&10)
    });

    let parts = controller.state().get();
    assert_eq!(parts.model.search.current_match, 0);
    assert!(!parts.model.search.loading_jump);
    assert!(
        parts
            .model
            .transcript
            .display_targets_by_item_index
            .contains_key(&10)
    );
}

#[gtk::test]
fn jump_to_loaded_match_with_typed_view_waits_for_display() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    seed_search_transcript(temp_db.path(), "test-session-123", 75, &[70]);

    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });
    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.transcript.loaded_count == 75 && !parts.model.transcript.loading
    });

    let active_request = controller.state().get().model.search.request_id;
    controller.emit(SessionDetailMsg::SetMatchPositions {
        request_id: active_request,
        session_id: "test-session-123".to_string(),
        positions: vec![MatchPosition { item_index: 70 }],
    });

    pump_main_context(|| {
        let parts = controller.state().get();
        !parts.model.search.loading_jump
    });

    let parts = controller.state().get();
    assert!(!parts.model.search.loading_jump);
    assert!(
        parts
            .model
            .transcript
            .display_targets_by_item_index
            .contains_key(&70)
    );
}

#[gtk::test]
fn prev_next_wrap_around_match_positions() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    seed_search_transcript(temp_db.path(), "test-session-123", 75, &[2, 4]);

    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: Some("needle".to_string()),
    });
    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.search.match_positions.len() == 2 && !parts.model.search.loading_jump
    });

    controller.emit(SessionDetailMsg::PrevMatch);
    pump_main_context(|| controller.state().get().model.search.current_match == 1);
    controller.emit(SessionDetailMsg::NextMatch);
    pump_main_context(|| controller.state().get().model.search.current_match == 0);

    let parts = controller.state().get();
    assert_eq!(parts.model.search.current_match, 0);
}
