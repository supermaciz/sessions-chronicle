use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sessions_chronicle::database::SessionIndexer;
use sessions_chronicle::database::analytics::load_analytics;
use sessions_chronicle::database::schema::initialize_database;
use sessions_chronicle::session_sources::SessionSources;

struct TempDatabase {
    path: PathBuf,
}

impl TempDatabase {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        path.push(format!(
            "sessions-chronicle-analytics-integration-{}-{}.db",
            std::process::id(),
            nanos
        ));

        let connection = Connection::open(&path).expect("Failed to open temp database");
        initialize_database(&connection).expect("Failed to initialize database");
        drop(connection);

        Self { path }
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn fixture_index_produces_non_empty_analytics_payload() {
    let db = TempDatabase::new();
    let fixtures_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let sources = SessionSources::resolve(Some(fixtures_path.as_path()));

    let mut indexer = SessionIndexer::new(&db.path).expect("Failed to create indexer");
    let stats = indexer
        .index_all_incremental(&sources)
        .expect("Failed to index fixture sessions");

    assert!(
        stats.totals.indexed > 0,
        "Expected fixtures to index at least one session"
    );

    let analytics = load_analytics(&db.path).expect("Analytics should load");

    assert!(analytics.overview.total_sessions > 0);
    assert!(analytics.overview.total_messages > 0);
    assert!(analytics.overview.total_sessions <= stats.totals.indexed as i64);

    assert!(!analytics.sessions_by_tool.is_empty());
    assert!(!analytics.session_span_buckets.is_empty());
    assert!(!analytics.activity_days.is_empty());
    assert!(!analytics.heatmap.weeks.is_empty());
    assert!(analytics.heatmap.max_sessions_in_a_day > 0);
    assert!(analytics.heatmap.display_start_day.is_some());
    assert!(analytics.heatmap.display_end_day.is_some());

    let sessions_by_tool_total: i64 = analytics
        .sessions_by_tool
        .iter()
        .map(|row| row.session_count)
        .sum();
    assert_eq!(sessions_by_tool_total, analytics.overview.total_sessions);

    let session_span_buckets_total: i64 = analytics
        .session_span_buckets
        .iter()
        .map(|row| row.session_count)
        .sum();
    assert_eq!(
        session_span_buckets_total,
        analytics.overview.total_sessions
    );

    let activity_days_total: i64 = analytics
        .activity_days
        .iter()
        .map(|row| row.session_count)
        .sum();
    assert_eq!(activity_days_total, analytics.overview.total_sessions);

    let heatmap_total: i64 = analytics
        .heatmap
        .weeks
        .iter()
        .flat_map(|week| week.days.iter())
        .map(|day| day.session_count)
        .sum();
    // Bounded heatmap may exclude old activity outside the visible window
    assert!(heatmap_total <= analytics.overview.total_sessions);

    assert!(
        analytics
            .token_usage_by_tool
            .iter()
            .all(|row| row.reported_sessions <= row.total_sessions),
        "Expected token usage reported_sessions to be <= total_sessions for every row"
    );

    // Verify that tool names use display names instead of storage names
    let display_names = [
        "Claude Code",
        "OpenCode",
        "Codex",
        "Mistral Vibe",
        "Kimi Code",
    ];

    for tool_data in &analytics.sessions_by_tool {
        assert!(
            display_names.contains(&tool_data.tool.as_str()),
            "Tool name '{}' should be a display name, not a storage name",
            tool_data.tool
        );
    }

    for tool_data in &analytics.token_usage_by_tool {
        assert!(
            display_names.contains(&tool_data.tool.as_str()),
            "Tool name '{}' should be a display name, not a storage name",
            tool_data.tool
        );
    }

    assert!(
        analytics
            .sessions_by_tool
            .iter()
            .any(|row| row.tool == "Kimi Code"),
        "Expected fixture indexing to produce a Kimi Code session-count row"
    );
    assert!(
        analytics
            .token_usage_by_tool
            .iter()
            .any(|row| row.tool == "Kimi Code"),
        "Expected fixture indexing to produce a Kimi Code token-usage row"
    );
}
