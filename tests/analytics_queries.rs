use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sessions_chronicle::database::analytics::load_analytics;
use sessions_chronicle::database::schema::initialize_database;

struct TempDatabase {
    path: PathBuf,
    connection: Connection,
}

impl TempDatabase {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        path.push(format!(
            "sessions-chronicle-test-{}-{}.db",
            std::process::id(),
            nanos
        ));

        let connection = Connection::open(&path).expect("Failed to open temp database");
        initialize_database(&connection).expect("Failed to initialize database");

        Self { path, connection }
    }

    fn seed_sessions(&self) {
        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "root-a",
                    "claude_code",
                    Some("/projects/alpha"),
                    1_700_000_000_i64,
                    5_i64,
                    "/tmp/root-a.jsonl",
                    1_700_000_100_i64,
                    0_i64,
                ],
            )
            .expect("Failed to insert root-a");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "root-b",
                    "opencode",
                    Some("/projects/beta"),
                    1_700_086_400_i64,
                    3_i64,
                    "/tmp/root-b.jsonl",
                    1_700_086_500_i64,
                    0_i64,
                ],
            )
            .expect("Failed to insert root-b");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "root-c",
                    "codex",
                    Some("/projects/alpha"),
                    1_700_086_460_i64,
                    2_i64,
                    "/tmp/root-c.jsonl",
                    1_700_086_560_i64,
                    0_i64,
                ],
            )
            .expect("Failed to insert root-c");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "subagent-a",
                    "claude_code",
                    Some("/projects/subagent-only"),
                    1_700_172_800_i64,
                    100_i64,
                    "/tmp/subagent-a.jsonl",
                    1_700_172_900_i64,
                    1_i64,
                ],
            )
            .expect("Failed to insert subagent-a");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "subagent-b",
                    "opencode",
                    Some("/projects/alpha"),
                    1_700_259_200_i64,
                    200_i64,
                    "/tmp/subagent-b.jsonl",
                    1_700_259_300_i64,
                    1_i64,
                ],
            )
            .expect("Failed to insert subagent-b");
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn overview_counts_exclude_subagents() {
    let db = TempDatabase::new();
    db.seed_sessions();

    let analytics = load_analytics(&db.path).expect("Failed to load analytics");

    assert_eq!(analytics.overview.total_sessions, 3);
    assert_eq!(analytics.overview.total_messages, 10);
    assert_eq!(analytics.overview.distinct_projects, 2);
    assert_eq!(analytics.overview.active_days, 2);
}

#[test]
fn missing_database_path_returns_default_analytics() {
    let mut missing_path = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    missing_path.push(format!(
        "sessions-chronicle-missing-analytics-{}-{}.db",
        std::process::id(),
        nanos
    ));

    let analytics = load_analytics(&missing_path).expect("Failed to load analytics");

    assert_eq!(
        analytics,
        sessions_chronicle::models::AnalyticsData::default()
    );
}

#[test]
fn overview_total_messages_clamps_negative_values_to_zero() {
    let db = TempDatabase::new();

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "root-negative",
                "claude_code",
                Some("/projects/negative"),
                1_700_000_000_i64,
                -25_i64,
                "/tmp/root-negative.jsonl",
                1_700_000_100_i64,
                0_i64,
            ],
        )
        .expect("Failed to insert root-negative");

    let analytics = load_analytics(&db.path).expect("Failed to load analytics");

    assert_eq!(analytics.overview.total_sessions, 1);
    assert_eq!(analytics.overview.total_messages, 0);
}

#[test]
fn sessions_by_tool_aggregates_top_level_sessions_only() {
    let db = TempDatabase::new();
    db.seed_sessions();

    let analytics = load_analytics(&db.path).expect("Failed to load analytics");

    assert_eq!(
        analytics.sessions_by_tool,
        vec![
            sessions_chronicle::models::analytics::ToolSessionCount {
                tool: "Claude Code".to_string(),
                session_count: 1,
            },
            sessions_chronicle::models::analytics::ToolSessionCount {
                tool: "Codex".to_string(),
                session_count: 1,
            },
            sessions_chronicle::models::analytics::ToolSessionCount {
                tool: "OpenCode".to_string(),
                session_count: 1,
            },
        ]
    );
}

#[test]
fn session_span_buckets_use_fixed_labels_and_clamp_negative_spans() {
    let db = TempDatabase::new();

    let sessions = [
        ("root-under-5", 1_700_000_000_i64, 1_700_000_060_i64, 0_i64),
        ("root-5-15", 1_700_000_000_i64, 1_700_000_600_i64, 0_i64),
        ("root-15-30", 1_700_000_000_i64, 1_700_001_200_i64, 0_i64),
        ("root-30-60", 1_700_000_000_i64, 1_700_002_400_i64, 0_i64),
        ("root-over-1h", 1_700_000_000_i64, 1_700_004_000_i64, 0_i64),
        ("root-negative", 1_700_000_100_i64, 1_700_000_000_i64, 0_i64),
        (
            "subagent-over-1h",
            1_700_000_000_i64,
            1_700_004_000_i64,
            1_i64,
        ),
    ];

    for (id, start_time, last_updated, is_subagent) in sessions {
        db.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    id,
                    "claude_code",
                    Some("/projects/spans"),
                    start_time,
                    1_i64,
                    format!("/tmp/{id}.jsonl"),
                    last_updated,
                    is_subagent,
                ],
            )
            .expect("Failed to insert test session");
    }

    let analytics = load_analytics(&db.path).expect("Failed to load analytics");

    assert_eq!(
        analytics.session_span_buckets,
        vec![
            sessions_chronicle::models::analytics::SessionSpanBucket {
                bucket: "< 5 min".to_string(),
                session_count: 2,
            },
            sessions_chronicle::models::analytics::SessionSpanBucket {
                bucket: "5-15 min".to_string(),
                session_count: 1,
            },
            sessions_chronicle::models::analytics::SessionSpanBucket {
                bucket: "15-30 min".to_string(),
                session_count: 1,
            },
            sessions_chronicle::models::analytics::SessionSpanBucket {
                bucket: "30-60 min".to_string(),
                session_count: 1,
            },
            sessions_chronicle::models::analytics::SessionSpanBucket {
                bucket: "> 1 hour".to_string(),
                session_count: 1,
            },
        ]
    );
}

#[test]
fn activity_days_are_grouped_and_heatmap_is_zero_filled() {
    let db = TempDatabase::new();

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "activity-a",
                "claude_code",
                Some("/projects/activity"),
                1_709_251_200_i64,
                4_i64,
                "/tmp/activity-a.jsonl",
                1_709_251_800_i64,
                0_i64,
            ],
        )
        .expect("Failed to insert activity-a");

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "activity-b",
                "opencode",
                Some("/projects/activity"),
                1_709_424_000_i64,
                2_i64,
                "/tmp/activity-b.jsonl",
                1_709_424_400_i64,
                0_i64,
            ],
        )
        .expect("Failed to insert activity-b");

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "activity-c",
                "codex",
                Some("/projects/activity"),
                1_709_424_600_i64,
                1_i64,
                "/tmp/activity-c.jsonl",
                1_709_424_900_i64,
                0_i64,
            ],
        )
        .expect("Failed to insert activity-c");

    let analytics = load_analytics(&db.path).expect("Failed to load analytics");

    assert_eq!(analytics.activity_days.len(), 2);
    assert_eq!(analytics.activity_days[0].session_count, 1);
    assert_eq!(analytics.activity_days[1].session_count, 2);
    assert_eq!(analytics.heatmap.max_sessions_in_a_day, 2);
    assert!(
        analytics
            .heatmap
            .weeks
            .iter()
            .flat_map(|week| week.days.iter())
            .any(|day| day.session_count == 0)
    );
}

#[test]
fn analytics_loading_ignores_invalid_grouped_activity_day_rows() {
    let db = TempDatabase::new();

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "activity-valid",
                "claude_code",
                Some("/projects/activity"),
                1_709_251_200_i64,
                1_i64,
                "/tmp/activity-valid.jsonl",
                1_709_251_800_i64,
                0_i64,
            ],
        )
        .expect("Failed to insert activity-valid");

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "activity-invalid",
                "opencode",
                Some("/projects/activity"),
                "not-a-timestamp",
                1_i64,
                "/tmp/activity-invalid.jsonl",
                1_709_251_900_i64,
                0_i64,
            ],
        )
        .expect("Failed to insert activity-invalid");

    let analytics = load_analytics(&db.path).expect("Failed to load analytics");

    assert_eq!(analytics.activity_days.len(), 1);
    assert_eq!(analytics.activity_days[0].session_count, 1);
    assert_eq!(analytics.overview.total_sessions, 2);
}

#[test]
fn heatmap_weeks_are_calendar_aligned_and_full_weeks() {
    let db = TempDatabase::new();

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "week-start",
                "claude_code",
                Some("/projects/heatmap"),
                1_709_510_400_i64,
                1_i64,
                "/tmp/week-start.jsonl",
                1_709_510_800_i64,
                0_i64,
            ],
        )
        .expect("Failed to insert week-start");

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "week-end",
                "opencode",
                Some("/projects/heatmap"),
                1_709_683_200_i64,
                1_i64,
                "/tmp/week-end.jsonl",
                1_709_683_600_i64,
                0_i64,
            ],
        )
        .expect("Failed to insert week-end");

    let analytics = load_analytics(&db.path).expect("Failed to load analytics");

    assert!(!analytics.heatmap.weeks.is_empty());
    assert!(
        analytics
            .heatmap
            .weeks
            .iter()
            .all(|week| week.days.len() == 7)
    );

    let first_week_first_day =
        NaiveDate::parse_from_str(&analytics.heatmap.weeks[0].days[0].day, "%Y-%m-%d")
            .expect("Failed to parse first heatmap day");
    assert_eq!(first_week_first_day.weekday(), chrono::Weekday::Mon);

    let last_week = analytics
        .heatmap
        .weeks
        .last()
        .expect("Missing last heatmap week");
    let last_week_last_day = NaiveDate::parse_from_str(&last_week.days[6].day, "%Y-%m-%d")
        .expect("Failed to parse last heatmap day");
    assert_eq!(last_week_last_day.weekday(), chrono::Weekday::Sun);
}

#[test]
fn token_totals_preserve_missing_vs_zero() {
    let db = TempDatabase::new();

    // Session with explicit 0 tokens (known zero values)
    db.connection.execute(
        "INSERT INTO sessions (
            id, tool, project_path, start_time, message_count, file_path, last_updated,
            is_subagent, input_tokens, output_tokens
         ) VALUES ('session-a', 'claude_code', '/projects/alpha', 10, 4, '/tmp/a.jsonl', 20, 0, 0, 0)",
        [],
    ).unwrap();

    // Session with partial coverage (has token data)
    db.connection.execute(
        "INSERT INTO sessions (
            id, tool, project_path, start_time, message_count, file_path, last_updated,
            is_subagent, input_tokens, output_tokens
         ) VALUES ('session-b', 'claude_code', '/projects/alpha', 30, 2, '/tmp/b.jsonl', 40, 0, 120, 45)",
        [],
    ).unwrap();

    // Session with no token coverage (NULL values - unavailable)
    db.connection.execute(
        "INSERT INTO sessions (
            id, tool, project_path, start_time, message_count, file_path, last_updated,
            is_subagent, input_tokens, output_tokens
         ) VALUES ('session-c', 'codex', '/projects/beta', 50, 1, '/tmp/c.jsonl', 60, 0, NULL, NULL)",
        [],
    ).unwrap();

    let analytics = load_analytics(&db.path).expect("analytics should load");

    // Claude Code: 2 sessions, both report tokens (one with 0, one with values)
    let claude = analytics
        .token_usage_by_tool
        .iter()
        .find(|row| row.tool == "Claude Code")
        .unwrap();
    assert_eq!(claude.total_sessions, 2);
    assert_eq!(claude.reported_sessions, 2);
    // Aggregation: 0 + 120 = 120 input tokens, 0 + 45 = 45 output tokens
    assert_eq!(
        claude.input_tokens,
        Some(120),
        "input_tokens should sum to 0 + 120 = 120"
    );
    assert_eq!(
        claude.output_tokens,
        Some(45),
        "output_tokens should sum to 0 + 45 = 45"
    );

    // Codex: 1 session, no token data reported
    let codex = analytics
        .token_usage_by_tool
        .iter()
        .find(|row| row.tool == "Codex")
        .unwrap();
    assert_eq!(codex.total_sessions, 1);
    assert_eq!(codex.reported_sessions, 0);
    assert_eq!(codex.input_tokens, None);
    assert_eq!(codex.output_tokens, None);
}

#[test]
fn tool_with_all_null_tokens_appears_in_results() {
    let db = TempDatabase::new();

    // Tool A with NULL tokens
    db.connection.execute(
        "INSERT INTO sessions (
            id, tool, project_path, start_time, message_count, file_path, last_updated,
            is_subagent, input_tokens, output_tokens
         ) VALUES ('session-1', 'tool_a', '/projects/test', 10, 4, '/tmp/1.jsonl', 20, 0, NULL, NULL)",
        [],
    ).unwrap();

    db.connection.execute(
        "INSERT INTO sessions (
            id, tool, project_path, start_time, message_count, file_path, last_updated,
            is_subagent, input_tokens, output_tokens
         ) VALUES ('session-2', 'tool_a', '/projects/test', 30, 2, '/tmp/2.jsonl', 40, 0, NULL, NULL)",
        [],
    ).unwrap();

    // Tool B with actual token data
    db.connection
        .execute(
            "INSERT INTO sessions (
            id, tool, project_path, start_time, message_count, file_path, last_updated,
            is_subagent, input_tokens, output_tokens
         ) VALUES ('session-3', 'tool_b', '/projects/test', 50, 1, '/tmp/3.jsonl', 60, 0, 100, 50)",
            [],
        )
        .unwrap();

    let analytics = load_analytics(&db.path).expect("analytics should load");

    // Both tools should appear in results, even tool_a with all NULL tokens
    assert_eq!(
        analytics.token_usage_by_tool.len(),
        2,
        "both tools should appear in results"
    );

    // Tool A: appears with NULL token sums
    let tool_a = analytics
        .token_usage_by_tool
        .iter()
        .find(|row| row.tool == "tool_a")
        .expect("tool_a should be in results");
    assert_eq!(tool_a.total_sessions, 2);
    assert_eq!(
        tool_a.reported_sessions, 0,
        "sessions with NULL tokens are not reported"
    );
    assert_eq!(
        tool_a.input_tokens, None,
        "NULL input tokens when no reported sessions"
    );
    assert_eq!(
        tool_a.output_tokens, None,
        "NULL output tokens when no reported sessions"
    );

    // Tool B: has token data
    let tool_b = analytics
        .token_usage_by_tool
        .iter()
        .find(|row| row.tool == "tool_b")
        .expect("tool_b should be in results");
    assert_eq!(tool_b.total_sessions, 1);
    assert_eq!(tool_b.reported_sessions, 1);
    assert_eq!(tool_b.input_tokens, Some(100));
    assert_eq!(tool_b.output_tokens, Some(50));
}

#[test]
fn heatmap_bounded_to_six_months_when_activity_exceeds_six_months() {
    let db = TempDatabase::new();

    // Create sessions spanning more than 6 months (about 7 months apart)
    // Old session: March 2024
    let old_timestamp = NaiveDate::from_ymd_opt(2024, 3, 15)
        .unwrap()
        .and_hms_opt(10, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
        .timestamp();

    // Recent session: October 2024 (about 7 months later)
    let recent_timestamp = NaiveDate::from_ymd_opt(2024, 10, 20)
        .unwrap()
        .and_hms_opt(10, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
        .timestamp();

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "old-session",
                "claude_code",
                Some("/projects/old"),
                old_timestamp,
                1_i64,
                "/tmp/old.jsonl",
                old_timestamp + 300,
                0_i64,
            ],
        )
        .expect("Failed to insert old session");

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "recent-session",
                "opencode",
                Some("/projects/recent"),
                recent_timestamp,
                1_i64,
                "/tmp/recent.jsonl",
                recent_timestamp + 300,
                0_i64,
            ],
        )
        .expect("Failed to insert recent session");

    let analytics = load_analytics(&db.path).expect("Failed to load analytics");

    assert!(
        !analytics.heatmap.weeks.is_empty(),
        "heatmap should have weeks"
    );

    // Get the first and last visible days in the heatmap
    let first_week_first_day =
        NaiveDate::parse_from_str(&analytics.heatmap.weeks[0].days[0].day, "%Y-%m-%d")
            .expect("Failed to parse first heatmap day");

    let last_week = analytics
        .heatmap
        .weeks
        .last()
        .expect("Missing last heatmap week");
    let last_week_last_day = NaiveDate::parse_from_str(&last_week.days[6].day, "%Y-%m-%d")
        .expect("Failed to parse last heatmap day");

    // Calculate the expected window: 6 months before the last activity day
    let last_activity_day = NaiveDate::from_ymd_opt(2024, 10, 20).unwrap();
    let six_months_ago = last_activity_day - chrono::Months::new(6);

    // The first visible day should be within the last 6 months window (after week alignment to Monday)
    assert!(
        first_week_first_day >= six_months_ago,
        "first visible day {} should be within 6 months of last activity (>= {})",
        first_week_first_day,
        six_months_ago
    );

    // The last visible day should be aligned to Sunday and cover the last activity week
    assert!(
        last_week_last_day >= last_activity_day,
        "last visible day {} should cover the last activity day {}",
        last_week_last_day,
        last_activity_day
    );
    assert_eq!(last_week_last_day.weekday(), chrono::Weekday::Sun);

    // Old activity (March 2024) should not be represented in the heatmap
    let old_activity_day = NaiveDate::from_ymd_opt(2024, 3, 15).unwrap();
    let all_heatmap_days: Vec<_> = analytics
        .heatmap
        .weeks
        .iter()
        .flat_map(|week| week.days.iter())
        .filter(|day| day.session_count > 0)
        .map(|day| NaiveDate::parse_from_str(&day.day, "%Y-%m-%d").unwrap())
        .collect();

    assert!(
        !all_heatmap_days.contains(&old_activity_day),
        "old activity day {} should not appear in heatmap days with sessions",
        old_activity_day
    );

    // Verify that display range metadata is populated for non-empty heatmaps
    assert!(
        analytics.heatmap.display_start_day.is_some(),
        "display_start_day should be populated for non-empty heatmap"
    );
    assert!(
        analytics.heatmap.display_end_day.is_some(),
        "display_end_day should be populated for non-empty heatmap"
    );

    let display_start = NaiveDate::parse_from_str(
        analytics.heatmap.display_start_day.as_ref().unwrap(),
        "%Y-%m-%d",
    )
    .expect("Failed to parse display_start_day");
    let display_end = NaiveDate::parse_from_str(
        analytics.heatmap.display_end_day.as_ref().unwrap(),
        "%Y-%m-%d",
    )
    .expect("Failed to parse display_end_day");

    // display_start_day should match the first visible heatmap day
    assert_eq!(
        display_start, first_week_first_day,
        "display_start_day should match the first visible heatmap day"
    );

    // display_end_day should match the last visible heatmap day
    assert_eq!(
        display_end, last_week_last_day,
        "display_end_day should match the last visible heatmap day"
    );
}

#[test]
fn heatmap_shows_full_range_when_activity_less_than_six_months() {
    let db = TempDatabase::new();

    // Create sessions spanning less than 6 months (about 2 months apart)
    // First session: August 2024
    let first_timestamp = NaiveDate::from_ymd_opt(2024, 8, 10)
        .unwrap()
        .and_hms_opt(10, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
        .timestamp();

    // Second session: October 2024 (about 2 months later)
    let second_timestamp = NaiveDate::from_ymd_opt(2024, 10, 15)
        .unwrap()
        .and_hms_opt(10, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
        .timestamp();

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "first-session",
                "claude_code",
                Some("/projects/first"),
                first_timestamp,
                1_i64,
                "/tmp/first.jsonl",
                first_timestamp + 300,
                0_i64,
            ],
        )
        .expect("Failed to insert first session");

    db.connection
        .execute(
            "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "second-session",
                "opencode",
                Some("/projects/second"),
                second_timestamp,
                1_i64,
                "/tmp/second.jsonl",
                second_timestamp + 300,
                0_i64,
            ],
        )
        .expect("Failed to insert second session");

    let analytics = load_analytics(&db.path).expect("Failed to load analytics");

    assert!(
        !analytics.heatmap.weeks.is_empty(),
        "heatmap should have weeks"
    );

    // Get the first visible day in the heatmap
    let first_week_first_day =
        NaiveDate::parse_from_str(&analytics.heatmap.weeks[0].days[0].day, "%Y-%m-%d")
            .expect("Failed to parse first heatmap day");

    // The earliest real activity day (August 10, 2024) should still be included
    let first_activity_day = NaiveDate::from_ymd_opt(2024, 8, 10).unwrap();

    // The heatmap should cover from before the first activity day (aligned to Monday)
    // The first heatmap day should be <= the first activity day
    assert!(
        first_week_first_day <= first_activity_day,
        "first visible day {} should be <= first activity day {}",
        first_week_first_day,
        first_activity_day
    );

    // The first activity day should be within the heatmap range
    let last_week = analytics
        .heatmap
        .weeks
        .last()
        .expect("Missing last heatmap week");
    let last_week_last_day = NaiveDate::parse_from_str(&last_week.days[6].day, "%Y-%m-%d")
        .expect("Failed to parse last heatmap day");

    assert!(
        first_activity_day >= first_week_first_day && first_activity_day <= last_week_last_day,
        "first activity day {} should be within heatmap range [{}, {}]",
        first_activity_day,
        first_week_first_day,
        last_week_last_day
    );
}
