use anyhow::{Context, Result};
use chrono::{Datelike, Duration, NaiveDate};
use std::collections::BTreeMap;
use std::path::Path;

use crate::models::{
    AnalyticsData, AnalyticsOverview,
    analytics::{
        ActivityDay, HeatmapData, HeatmapWeek, SessionSpanBucket, ToolSessionCount, ToolTokenUsage,
    },
    session::AiAssistant,
};

pub fn load_analytics(db_path: &Path) -> Result<AnalyticsData> {
    if !db_path.exists() {
        return Ok(AnalyticsData::default());
    }

    let db = super::open_connection(db_path)?;
    let overview = load_overview(&db)?;
    let activity_days = load_activity_days(&db)?;
    let heatmap = build_heatmap(&activity_days)?;
    let sessions_by_tool = load_sessions_by_tool(&db)?;
    let session_span_buckets = load_session_span_buckets(&db)?;
    let token_usage_by_tool = load_token_usage(&db)?;

    Ok(AnalyticsData {
        overview,
        activity_days,
        heatmap,
        sessions_by_tool,
        session_span_buckets,
        token_usage_by_tool,
    })
}

fn load_overview(db: &rusqlite::Connection) -> Result<AnalyticsOverview> {
    let sql = format!(
        "SELECT
            COUNT(*) AS total_sessions,
            COALESCE(SUM(MAX(message_count, 0)), 0) AS total_messages,
            COUNT(DISTINCT NULLIF(project_path, '')) AS distinct_projects,
            COUNT(DISTINCT {}) AS active_days
         FROM sessions
         WHERE is_subagent = 0",
        activity_group_date_sql()
    );

    db.query_row(&sql, [], |row| {
        Ok(AnalyticsOverview {
            total_sessions: row.get("total_sessions")?,
            total_messages: row.get("total_messages")?,
            distinct_projects: row.get("distinct_projects")?,
            active_days: row.get("active_days")?,
        })
    })
    .context("Failed to load analytics overview")
}

fn activity_group_date_sql() -> &'static str {
    "date(start_time, 'unixepoch', 'localtime')"
}

fn load_activity_days(db: &rusqlite::Connection) -> Result<Vec<ActivityDay>> {
    let sql = format!(
        "SELECT {} AS day, COUNT(*) AS session_count
         FROM sessions
         WHERE is_subagent = 0
         GROUP BY day
         ORDER BY day ASC",
        activity_group_date_sql()
    );

    let mut stmt = db
        .prepare(&sql)
        .context("Failed to prepare activity-days query")?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>("day")?,
                row.get::<_, i64>("session_count")?,
            ))
        })
        .context("Failed to map activity-days rows")?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to collect activity-days rows")
        .map(|rows| {
            rows.into_iter()
                .filter_map(|(day, session_count)| {
                    day.map(|day| ActivityDay { day, session_count })
                })
                .collect()
        })
}

fn build_heatmap(activity_days: &[ActivityDay]) -> Result<HeatmapData> {
    if activity_days.is_empty() {
        return Ok(HeatmapData::default());
    }

    let first_day = NaiveDate::parse_from_str(&activity_days[0].day, "%Y-%m-%d")
        .context("Failed to parse first activity day")?;
    let last_day =
        NaiveDate::parse_from_str(&activity_days[activity_days.len() - 1].day, "%Y-%m-%d")
            .context("Failed to parse last activity day")?;

    let lookup = activity_days
        .iter()
        .map(|day| (day.day.clone(), day.session_count))
        .collect::<BTreeMap<_, _>>();

    let aligned_start = first_day
        .checked_sub_signed(Duration::days(
            first_day.weekday().num_days_from_monday() as i64
        ))
        .context("Failed to align first heatmap day to Monday")?;
    let aligned_end = last_day
        .checked_add_signed(Duration::days(
            (6 - last_day.weekday().num_days_from_monday()) as i64,
        ))
        .context("Failed to align last heatmap day to Sunday")?;

    let mut normalized_days = Vec::new();
    let mut day_cursor = aligned_start;
    while day_cursor <= aligned_end {
        let day = day_cursor.format("%Y-%m-%d").to_string();
        normalized_days.push(ActivityDay {
            session_count: lookup.get(&day).copied().unwrap_or(0),
            day,
        });
        day_cursor = day_cursor
            .succ_opt()
            .context("Failed to increment heatmap day cursor")?;
    }

    let max_sessions_in_a_day = normalized_days
        .iter()
        .map(|day| day.session_count)
        .max()
        .unwrap_or(0);
    let weeks = normalized_days
        .chunks(7)
        .map(|week_days| HeatmapWeek {
            days: week_days.to_vec(),
        })
        .collect();

    Ok(HeatmapData {
        weeks,
        max_sessions_in_a_day,
    })
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
            let tool_storage_name: String = row.get("tool")?;
            let tool_display_name = AiAssistant::from_storage(&tool_storage_name)
                .map(|assistant| assistant.display_name().to_string())
                .unwrap_or(tool_storage_name);

            Ok(ToolSessionCount {
                tool: tool_display_name,
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

fn load_token_usage(db: &rusqlite::Connection) -> Result<Vec<ToolTokenUsage>> {
    // Semantics note: BOTH input_tokens AND output_tokens must be non-NULL
    // to count as a "reported" session with token data. This prevents
    // counting sessions that have only partial token coverage.
    let mut stmt = db
        .prepare(
            "SELECT
            tool,
            COUNT(*) AS total_sessions,
            SUM(CASE WHEN input_tokens IS NOT NULL AND output_tokens IS NOT NULL THEN 1 ELSE 0 END) AS reported_sessions,
            SUM(CASE WHEN input_tokens IS NOT NULL AND output_tokens IS NOT NULL THEN input_tokens ELSE 0 END) AS input_sum,
            SUM(CASE WHEN input_tokens IS NOT NULL AND output_tokens IS NOT NULL THEN output_tokens ELSE 0 END) AS output_sum
         FROM sessions
         WHERE is_subagent = 0
         GROUP BY tool
         ORDER BY total_sessions DESC, tool ASC",
        )
        .context("Failed to prepare token-usage query")?;

    let rows = stmt
        .query_map([], |row| {
            let tool_storage_name: String = row.get(0)?;
            let tool_display_name = AiAssistant::from_storage(&tool_storage_name)
                .map(|assistant| assistant.display_name().to_string())
                .unwrap_or(tool_storage_name);
            let reported_sessions: i64 = row.get(2)?;
            Ok(ToolTokenUsage {
                tool: tool_display_name,
                total_sessions: row.get::<_, i64>(1)?,
                reported_sessions,
                input_tokens: (reported_sessions > 0).then(|| row.get::<_, i64>(3).unwrap_or(0)),
                output_tokens: (reported_sessions > 0).then(|| row.get::<_, i64>(4).unwrap_or(0)),
            })
        })
        .context("Failed to map token-usage rows")?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to collect token-usage rows")
}

#[cfg(test)]
mod tests {
    use super::activity_group_date_sql;

    #[test]
    fn activity_group_sql_uses_localtime_modifier() {
        assert!(activity_group_date_sql().contains("'localtime'"));
    }
}
