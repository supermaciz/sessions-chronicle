mod support;

use sessions_chronicle_core::database::shell_search::{
    RESULT_LIMIT, search_session_ids, subsearch_session_ids,
};
use support::TempDatabase;

#[test]
fn search_deduplicates_ranked_messages_and_excludes_subagents() {
    let database = TempDatabase::new();
    database.seed_session(
        "parent",
        100,
        false,
        &["needle appears once", "needle needle appears twice"],
    );
    database.seed_session("subagent", 200, true, &["needle appears here"]);
    database.seed_session("other", 300, false, &["unrelated text"]);

    let (connection, _interrupt) = database.search_connection();
    let results = search_session_ids(&connection, "needle").unwrap();

    assert_eq!(results, ["parent"]);
}

#[test]
fn search_orders_by_rank_then_recency_then_id() {
    let database = TempDatabase::new();
    database.seed_session("weak-match", 300, false, &["needle"]);
    database.seed_session("strong-match", 100, false, &["needle needle needle"]);
    database.seed_session("tie-newer-z", 200, false, &["same phrase"]);
    database.seed_session("tie-newer-a", 200, false, &["same phrase"]);
    database.seed_session("tie-older", 100, false, &["same phrase"]);

    let (connection, _interrupt) = database.search_connection();
    let results = search_session_ids(&connection, "needle OR same").unwrap();

    assert_eq!(
        results,
        [
            "strong-match",
            "weak-match",
            "tie-newer-a",
            "tie-newer-z",
            "tie-older"
        ]
    );
}

#[test]
fn initial_search_is_deduplicated_and_capped_at_twenty_top_level_ids() {
    let database = TempDatabase::new();
    for index in 0..=RESULT_LIMIT {
        database.seed_session(
            &format!("session-{index:02}"),
            index as i64,
            false,
            &["bounded needle"],
        );
    }

    let (connection, _interrupt) = database.search_connection();
    let results = search_session_ids(&connection, "needle").unwrap();

    assert_eq!(results.len(), RESULT_LIMIT);
    assert!(results.iter().all(|id| id.starts_with("session-")));
    assert_eq!(
        results
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        RESULT_LIMIT
    );
}

#[test]
fn subsearch_is_empty_for_no_previous_results_and_preserves_previous_order() {
    let database = TempDatabase::new();
    for index in 0..=RESULT_LIMIT {
        database.seed_session(
            &format!("session-{index:02}"),
            index as i64,
            false,
            &["bounded needle"],
        );
    }
    database.seed_session("subagent", 1000, true, &["bounded needle"]);

    let (connection, _interrupt) = database.search_connection();
    assert!(
        subsearch_session_ids(&connection, "needle", &[])
            .unwrap()
            .is_empty()
    );

    let previous_ids = vec![
        "session-01".into(),
        "unknown".into(),
        "subagent".into(),
        "session-05".into(),
        "session-02".into(),
    ];
    let results = subsearch_session_ids(&connection, "needle", &previous_ids).unwrap();

    assert_eq!(results, ["session-01", "session-05", "session-02"]);
}

#[test]
fn subsearch_skips_duplicate_previous_ids_and_keeps_later_matches() {
    let database = TempDatabase::new();
    database.seed_session("first", 100, false, &["needle"]);
    database.seed_session("later", 200, false, &["needle"]);

    let previous_ids = vec!["first".into(), "first".into(), "later".into()];
    let (connection, _interrupt) = database.search_connection();
    let results = subsearch_session_ids(&connection, "needle", &previous_ids).unwrap();

    assert_eq!(results, ["first", "later"]);
}

#[test]
fn subsearch_caps_supplied_previous_results_at_result_limit() {
    let database = TempDatabase::new();
    for index in 0..=RESULT_LIMIT {
        database.seed_session(
            &format!("session-{index:02}"),
            index as i64,
            false,
            &["bounded needle"],
        );
    }

    let previous_ids = (0..=RESULT_LIMIT)
        .map(|index| format!("session-{index:02}"))
        .collect::<Vec<_>>();
    let (connection, _interrupt) = database.search_connection();
    let results = subsearch_session_ids(&connection, "needle", &previous_ids).unwrap();

    assert_eq!(results.len(), RESULT_LIMIT);
    assert!(!results.iter().any(|id| id == "session-20"));
}

#[test]
fn metadata_excerpt_off_preserves_requested_order_duplicates_and_missing_slots() {
    let database = TempDatabase::new();
    database.seed_session("first", 100, false, &["first prompt"]);
    database.seed_session("second", 200, false, &["second prompt"]);

    let (connection, _interrupt) = database.search_connection();
    let ids = [
        "second".into(),
        "missing".into(),
        "second".into(),
        "first".into(),
    ];
    let metadata = connection.load_metadata(&ids, false, None).unwrap();

    assert_eq!(metadata.len(), ids.len());
    assert_eq!(
        metadata[0].as_ref().map(|row| row.id.as_str()),
        Some("second")
    );
    assert!(metadata[1].is_none());
    assert_eq!(
        metadata[2].as_ref().map(|row| row.id.as_str()),
        Some("second")
    );
    assert_eq!(
        metadata[3].as_ref().map(|row| row.id.as_str()),
        Some("first")
    );
}

#[test]
fn metadata_excerpt_off_does_not_require_messages_fts() {
    let database = TempDatabase::new();
    database.seed_session("session", 100, false, &["prompt"]);
    database.drop_messages_fts();

    let (connection, _interrupt) = database.search_connection();
    let metadata = connection
        .load_metadata(&["session".into()], false, None)
        .unwrap();

    assert_eq!(metadata.len(), 1);
    assert!(metadata[0].as_ref().unwrap().matched_snippet.is_none());
}

#[test]
fn metadata_excerpt_on_selects_the_best_matching_message_snippet() {
    let database = TempDatabase::new();
    database.seed_session(
        "session",
        100,
        false,
        &[
            "needle ordinary match",
            "needle needle needle strongest match",
        ],
    );

    let (connection, _interrupt) = database.search_connection();
    let metadata = connection
        .load_metadata(&["session".into()], true, Some("needle"))
        .unwrap();
    let snippet = metadata[0]
        .as_ref()
        .unwrap()
        .matched_snippet
        .as_deref()
        .unwrap();

    assert!(
        snippet.contains("strongest match"),
        "snippet was {snippet:?}"
    );
}

#[test]
fn metadata_excerpt_on_without_expression_falls_back_to_no_snippet() {
    let database = TempDatabase::new();
    database.seed_session("session", 100, false, &["needle prompt"]);

    let (connection, _interrupt) = database.search_connection();
    let metadata = connection
        .load_metadata(&["session".into()], true, None)
        .unwrap();

    assert_eq!(metadata[0].as_ref().unwrap().matched_snippet, None);
}
