use sessions_chronicle::database::{
    SessionIndexer, load_sessions_for_filter, search_sessions_for_filter,
};
use sessions_chronicle::models::{AiAssistant, DateFilter, ProjectFilter, SortOrder};
use std::path::Path;
use tempfile::NamedTempFile;

const FIXTURE: &str = "tests/fixtures/claude_recent_events";
const SESSION_ID: &str = "6e10c428-58df-4ad1-9ef0-73fb3ef741b2";

fn search(db_path: &Path, token: &str) -> Vec<String> {
    search_sessions_for_filter(
        db_path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        token,
        &DateFilter::AnyTime,
        None,
    )
    .unwrap()
    .into_iter()
    .map(|session| session.id)
    .collect()
}

#[test]
fn indexes_recent_events_without_blob_messages() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    indexer.index_claude_sessions(Path::new(FIXTURE)).unwrap();

    let sessions = load_sessions_for_filter(
        temp_db.path(),
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        &DateFilter::AnyTime,
        SortOrder::RecentActivity,
    )
    .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, SESSION_ID);
    assert_eq!(sessions[0].message_count, 4);
    assert_eq!(
        sessions[0].first_prompt.as_deref(),
        Some("Explain recent event indexing")
    );

    assert!(search(temp_db.path(), "blobuseronlytoken").is_empty());
    assert!(search(temp_db.path(), "blobassistantonlytoken").is_empty());
    assert_eq!(
        search(temp_db.path(), "notificationonlytoken"),
        vec![SESSION_ID]
    );
    assert_eq!(search(temp_db.path(), "skillonlytoken"), vec![SESSION_ID]);
}
