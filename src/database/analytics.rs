use anyhow::{Context, Result};
use std::path::Path;

use crate::models::{AnalyticsData, AnalyticsOverview};

pub fn load_analytics_data(db_path: &Path) -> Result<AnalyticsData> {
    if !db_path.exists() {
        return Ok(AnalyticsData::default());
    }

    let db = super::open_connection(db_path)?;
    let overview = load_overview(&db)?;

    Ok(AnalyticsData {
        overview,
        ..AnalyticsData::default()
    })
}

fn load_overview(db: &rusqlite::Connection) -> Result<AnalyticsOverview> {
    db.query_row(
        "SELECT
            COUNT(*) AS total_sessions,
            COALESCE(SUM(message_count), 0) AS total_messages,
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
