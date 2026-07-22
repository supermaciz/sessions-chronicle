mod helpers;

use chrono::{Days, Duration, Local, NaiveDate, TimeZone, Utc};
use helpers::TempDatabase;
use sessions_chronicle::database::{
    count_sessions_per_date_preset, load_session_by_id_for_filter, load_sessions_for_filter,
    search_sessions_for_filter,
};
use sessions_chronicle::models::{AiAssistant, DateFilter, ProjectFilter, SortOrder};

/// Local "today at 12:00" converted to UTC, used as a stable anchor for
/// building date-window fixtures across timezones.
fn local_today_midday_utc() -> chrono::DateTime<Utc> {
    let local_today = Local::now().date_naive();
    Local
        .from_local_datetime(
            &local_today
                .and_hms_opt(12, 0, 0)
                .expect("midday local time should exist"),
        )
        .earliest()
        .expect("local conversion for today")
        .with_timezone(&Utc)
}

fn seed_date_dataset(db: &TempDatabase) {
    let today_midday = local_today_midday_utc();

    let today_ts = today_midday.timestamp();
    let two_days_ago_ts = (today_midday - Duration::days(2)).timestamp();
    let ten_days_ago_ts = (today_midday - Duration::days(10)).timestamp();
    let forty_days_ago_ts = (today_midday - Duration::days(40)).timestamp();

    db.insert_project(1, "/projects/alpha", "alpha");
    db.insert_project(2, "/projects/beta", "beta");

    db.insert_session(
        "today-alpha",
        "claude_code",
        Some("/projects/alpha"),
        Some(1),
        today_ts,
        today_ts,
    );
    db.insert_session(
        "recent-beta",
        "claude_code",
        Some("/projects/beta"),
        Some(2),
        two_days_ago_ts,
        two_days_ago_ts,
    );
    db.insert_session(
        "older-alpha",
        "claude_code",
        Some("/projects/alpha"),
        Some(1),
        ten_days_ago_ts,
        ten_days_ago_ts,
    );
    db.insert_session(
        "old-unassigned",
        "claude_code",
        None,
        None,
        forty_days_ago_ts,
        forty_days_ago_ts,
    );

    db.insert_message("today-alpha", 0, "alpha needle", today_ts);
    db.insert_message("recent-beta", 0, "beta needle", two_days_ago_ts);
    db.insert_message("older-alpha", 0, "alpha needle", ten_days_ago_ts);
    db.insert_message("old-unassigned", 0, "legacy needle", forty_days_ago_ts);

    let custom_day = today_midday
        .with_timezone(&Local)
        .date_naive()
        .checked_sub_days(Days::new(2))
        .expect("custom day should exist");
    let custom_ts = Local
        .from_local_datetime(
            &custom_day
                .and_hms_opt(12, 0, 0)
                .expect("midday local time should exist"),
        )
        .earliest()
        .expect("local conversion for custom day")
        .with_timezone(&Utc)
        .timestamp();

    db.insert_session(
        "custom-edge",
        "claude_code",
        Some("/projects/alpha"),
        Some(1),
        custom_ts,
        custom_ts,
    );
    db.insert_message("custom-edge", 0, "alpha edge", custom_ts);

    db.insert_session(
        "moved-today",
        "claude_code",
        Some("/projects/alpha"),
        Some(1),
        ten_days_ago_ts,
        today_ts,
    );
    db.insert_message("moved-today", 0, "alpha moved", today_ts);

    db.insert_session(
        "started-today-stale",
        "claude_code",
        Some("/projects/alpha"),
        Some(1),
        today_ts,
        ten_days_ago_ts,
    );
    db.insert_message("started-today-stale", 0, "alpha stale", ten_days_ago_ts);
}

#[test]
fn load_sessions_for_filter_applies_date_presets() {
    let db = TempDatabase::new("date-filter-presets");
    seed_date_dataset(&db);

    let today_sessions = load_sessions_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        &DateFilter::Today,
        SortOrder::RecentActivity,
    )
    .expect("today filter query should succeed");

    assert_eq!(today_sessions.len(), 2);
    assert!(
        today_sessions
            .iter()
            .any(|session| session.id == "today-alpha")
    );
    assert!(
        today_sessions
            .iter()
            .any(|session| session.id == "moved-today")
    );
    assert!(
        today_sessions
            .iter()
            .all(|session| session.id != "started-today-stale")
    );

    let last_7_sessions = load_sessions_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        &DateFilter::Last7Days,
        SortOrder::RecentActivity,
    )
    .expect("last-7-days filter query should succeed");

    let ids: Vec<&str> = last_7_sessions
        .iter()
        .map(|session| session.id.as_str())
        .collect();
    assert!(ids.contains(&"today-alpha"));
    assert!(ids.contains(&"recent-beta"));
    assert!(ids.contains(&"custom-edge"));
    assert!(!ids.contains(&"older-alpha"));
}

#[test]
fn custom_date_bounds_are_inclusive_and_compose_with_project_and_query() {
    let db = TempDatabase::new("date-filter-custom");
    seed_date_dataset(&db);

    let custom_day = Local::now()
        .date_naive()
        .checked_sub_days(Days::new(2))
        .expect("custom day should exist");
    let custom = DateFilter::Custom {
        from: custom_day,
        to: custom_day,
    };

    let sessions = search_sessions_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::Project(1),
        "alpha",
        &custom,
        None,
    )
    .expect("composed date/project/query search should succeed");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "custom-edge");
}

#[test]
fn session_id_lookup_respects_date_filter() {
    let db = TempDatabase::new("date-filter-id");
    seed_date_dataset(&db);

    let sessions = load_session_by_id_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        "older-alpha",
        &DateFilter::Today,
    )
    .expect("session-id lookup should succeed");
    assert!(sessions.is_empty());

    let any_time_sessions = load_session_by_id_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        "older-alpha",
        &DateFilter::AnyTime,
    )
    .expect("session-id lookup with any-time should succeed");
    assert_eq!(any_time_sessions.len(), 1);
    assert_eq!(any_time_sessions[0].id, "older-alpha");
}

#[test]
fn count_sessions_per_date_preset_uses_same_filter_context() {
    let db = TempDatabase::new("date-filter-counts");
    seed_date_dataset(&db);

    let counts = count_sessions_per_date_preset(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::Project(1),
        "alpha",
    )
    .expect("date counts should succeed");

    assert_eq!(counts.any_time, 5);
    assert_eq!(counts.today, 2);
    assert_eq!(counts.last_7_days, 3);
    assert_eq!(counts.last_30_days, 5);
    assert!(counts.this_year >= counts.last_30_days);
}

#[test]
fn count_sessions_per_date_preset_query_counts_distinct_sessions() {
    let db = TempDatabase::new("date-filter-counts-distinct");
    seed_date_dataset(&db);

    let local_today = Local::now().date_naive();
    let today_midday_ts = Local
        .from_local_datetime(
            &local_today
                .and_hms_opt(12, 0, 0)
                .expect("midday local time should exist"),
        )
        .earliest()
        .expect("local conversion for today")
        .with_timezone(&Utc)
        .timestamp();

    db.insert_message("today-alpha", 1, "alpha repeated", today_midday_ts);

    let counts = count_sessions_per_date_preset(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::Project(1),
        "alpha",
    )
    .expect("date counts should succeed");

    assert_eq!(counts.any_time, 5);
    assert_eq!(counts.today, 2);
    assert_eq!(counts.last_7_days, 3);
    assert_eq!(counts.last_30_days, 5);
}

#[test]
fn count_sessions_per_date_preset_sanitizes_invalid_query() {
    let db = TempDatabase::new("date-filter-counts-invalid-query");
    seed_date_dataset(&db);

    let counts = count_sessions_per_date_preset(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::Project(1),
        "\"alpha",
    )
    .expect("date counts should succeed with sanitized query");

    assert_eq!(counts.any_time, 5);
    assert_eq!(counts.today, 2);
    assert_eq!(counts.last_7_days, 3);
    assert_eq!(counts.last_30_days, 5);
    assert!(counts.this_year >= counts.last_30_days);
}

#[test]
fn custom_date_bounds_include_both_ends() {
    let db = TempDatabase::new("date-filter-inclusive");
    let from = NaiveDate::from_ymd_opt(2025, 1, 10).unwrap();
    let to = NaiveDate::from_ymd_opt(2025, 1, 11).unwrap();

    let from_ts = Local
        .from_local_datetime(&from.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .unwrap()
        .with_timezone(&Utc)
        .timestamp();
    let to_end_ts = Local
        .from_local_datetime(&to.and_hms_opt(23, 59, 59).unwrap())
        .earliest()
        .unwrap()
        .with_timezone(&Utc)
        .timestamp();

    db.insert_session("from-bound", "claude_code", None, None, from_ts, from_ts);
    db.insert_message("from-bound", 0, "boundary", from_ts);
    db.insert_session("to-bound", "claude_code", None, None, to_end_ts, to_end_ts);
    db.insert_message("to-bound", 0, "boundary", to_end_ts);

    let sessions = load_sessions_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        &DateFilter::Custom { from, to },
        SortOrder::RecentActivity,
    )
    .expect("custom range query should succeed");

    let ids: Vec<&str> = sessions.iter().map(|session| session.id.as_str()).collect();
    assert!(ids.contains(&"from-bound"));
    assert!(ids.contains(&"to-bound"));
}

#[test]
fn load_sessions_for_filter_applies_yesterday_preset() {
    let db = TempDatabase::new("date-filter-yesterday");

    let today_midday = local_today_midday_utc();

    let today_ts = today_midday.timestamp();
    let yesterday_ts = (today_midday - Duration::days(1)).timestamp();

    db.insert_session(
        "today-session",
        "claude_code",
        None,
        None,
        today_ts,
        today_ts,
    );
    db.insert_message("today-session", 0, "today needle", today_ts);

    db.insert_session(
        "yesterday-session",
        "claude_code",
        None,
        None,
        yesterday_ts,
        yesterday_ts,
    );
    db.insert_message("yesterday-session", 0, "yesterday needle", yesterday_ts);

    let yesterday_sessions = load_sessions_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        &DateFilter::Yesterday,
        SortOrder::RecentActivity,
    )
    .expect("yesterday filter query should succeed");

    assert_eq!(yesterday_sessions.len(), 1);
    assert_eq!(yesterday_sessions[0].id, "yesterday-session");

    let today_sessions = load_sessions_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        &DateFilter::Today,
        SortOrder::RecentActivity,
    )
    .expect("today filter query should succeed");

    assert_eq!(today_sessions.len(), 1);
    assert_eq!(today_sessions[0].id, "today-session");
}

#[test]
fn count_sessions_per_date_preset_counts_yesterday_separately() {
    let db = TempDatabase::new("date-filter-yesterday-counts");

    let today_midday = local_today_midday_utc();

    let today_ts = today_midday.timestamp();
    let yesterday_ts = (today_midday - Duration::days(1)).timestamp();
    let two_days_ago_ts = (today_midday - Duration::days(2)).timestamp();

    db.insert_session(
        "today-session",
        "claude_code",
        None,
        None,
        today_ts,
        today_ts,
    );
    db.insert_message("today-session", 0, "needle", today_ts);

    db.insert_session(
        "yesterday-session",
        "claude_code",
        None,
        None,
        yesterday_ts,
        yesterday_ts,
    );
    db.insert_message("yesterday-session", 0, "needle", yesterday_ts);

    db.insert_session(
        "two-days-ago-session",
        "claude_code",
        None,
        None,
        two_days_ago_ts,
        two_days_ago_ts,
    );
    db.insert_message("two-days-ago-session", 0, "needle", two_days_ago_ts);

    let counts = count_sessions_per_date_preset(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::AllSessions,
        "",
    )
    .expect("date counts should succeed");

    assert_eq!(counts.any_time, 3);
    assert_eq!(counts.today, 1);
    assert_eq!(counts.yesterday, 1);
    assert_eq!(counts.last_7_days, 3);
    assert!(counts.this_year >= counts.last_7_days);
}
