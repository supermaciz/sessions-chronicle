use anyhow::{Context, Result};
use std::path::Path;

use crate::models::{
    AnalyticsData, AnalyticsOverview,
    analytics::{SessionSpanBucket, ToolSessionCount},
};

pub fn load_analytics(db_path: &Path) -> Result<AnalyticsData> {
    if !db_path.exists() {
        return Ok(AnalyticsData::default());
    }

    let db = super::open_connection(db_path)?;
    let overview = load_overview(&db)?;
    let sessions_by_tool = load_sessions_by_tool(&db)?;
    let session_span_buckets = load_session_span_buckets(&db)?;

    Ok(AnalyticsData {
        overview,
        sessions_by_tool,
        session_span_buckets,
        ..AnalyticsData::default()
    })
}

fn load_overview(db: &rusqlite::Connection) -> Result<AnalyticsOverview> {
    db.query_row(
        "SELECT
            COUNT(*) AS total_sessions,
            COALESCE(SUM(MAX(message_count, 0)), 0) AS total_messages,
            COUNT(DISTINCT NULLIF(project_path, '')) AS distinct_projects,
            COUNT(DISTINCT date(start_time, 'unixepoch', 'localtime')) AS active_days
         FROM sessions
         WHERE is_subagent = 0",
        [],
        |row| {
            Ok(AnalyticsOverview {
                total_sessions: row.get("total_sessions")?,
                total_messages: row.get("total_messages")?,
                distinct_projects: row.get("distinct_projects")?,
                active_days: row.get("active_days")?,
            })
        },
    )
    .context("Failed to load analytics overview")
}

fn load_sessions_by_tool(db: &rusqlite::Connection) -> Result<Vec<ToolSessionCount>> {
    let mut stmt = db
        .prepare(
            "SELECT tool, COUNT(*) AS session_count
             FROM sessions
             WHERE is_subagent = 0
             GROUP BY tool
             ORDER BY session_count DESC, tool ASC",
        )
        .context("Failed to prepare sessions-by-tool query")?;

    let rows = stmt
        .query_map([], |row| {
            Ok(ToolSessionCount {
                tool: row.get("tool")?,
                session_count: row.get("session_count")?,
            })
        })
        .context("Failed to map sessions-by-tool rows")?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to collect sessions-by-tool rows")
}

fn load_session_span_buckets(db: &rusqlite::Connection) -> Result<Vec<SessionSpanBucket>> {
    let counts = db
        .query_row(
            "SELECT
                COALESCE(SUM(CASE WHEN session_span_seconds < 300 THEN 1 ELSE 0 END), 0) AS less_than_5,
                COALESCE(SUM(CASE WHEN session_span_seconds >= 300 AND session_span_seconds < 900 THEN 1 ELSE 0 END), 0) AS between_5_15,
                COALESCE(SUM(CASE WHEN session_span_seconds >= 900 AND session_span_seconds < 1800 THEN 1 ELSE 0 END), 0) AS between_15_30,
                COALESCE(SUM(CASE WHEN session_span_seconds >= 1800 AND session_span_seconds < 3600 THEN 1 ELSE 0 END), 0) AS between_30_60,
                COALESCE(SUM(CASE WHEN session_span_seconds >= 3600 THEN 1 ELSE 0 END), 0) AS over_1_hour
             FROM (
                SELECT MAX(last_updated - start_time, 0) AS session_span_seconds
                FROM sessions
                WHERE is_subagent = 0
             )",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>("less_than_5")?,
                    row.get::<_, i64>("between_5_15")?,
                    row.get::<_, i64>("between_15_30")?,
                    row.get::<_, i64>("between_30_60")?,
                    row.get::<_, i64>("over_1_hour")?,
                ))
            },
        )
        .context("Failed to load session span buckets")?;

    Ok(vec![
        SessionSpanBucket {
            bucket: "< 5 min".to_string(),
            session_count: counts.0,
        },
        SessionSpanBucket {
            bucket: "5-15 min".to_string(),
            session_count: counts.1,
        },
        SessionSpanBucket {
            bucket: "15-30 min".to_string(),
            session_count: counts.2,
        },
        SessionSpanBucket {
            bucket: "30-60 min".to_string(),
            session_count: counts.3,
        },
        SessionSpanBucket {
            bucket: "> 1 hour".to_string(),
            session_count: counts.4,
        },
    ])
}
