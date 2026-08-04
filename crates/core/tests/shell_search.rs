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
    database.seed_session("older", 100, false, &["same phrase"]);
    database.seed_session("newer-z", 200, false, &["same phrase"]);
    database.seed_session("newer-a", 200, false, &["same phrase"]);

    let (connection, _interrupt) = database.search_connection();
    let results = search_session_ids(&connection, "same").unwrap();

    assert_eq!(results, ["newer-a", "newer-z", "older"]);
}

#[test]
fn subsearch_is_empty_for_no_previous_results_and_bounded_to_twenty_ids() {
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
    assert!(
        subsearch_session_ids(&connection, "needle", &[])
            .unwrap()
            .is_empty()
    );

    let previous_ids = (0..=RESULT_LIMIT)
        .map(|index| format!("session-{index:02}"))
        .collect::<Vec<_>>();
    let results = subsearch_session_ids(&connection, "needle", &previous_ids).unwrap();

    assert_eq!(results.len(), RESULT_LIMIT);
    assert!(!results.iter().any(|id| id == "session-20"));
    assert!(
        results
            .iter()
            .all(|id| previous_ids[..RESULT_LIMIT].contains(id))
    );
}
