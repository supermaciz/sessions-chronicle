#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnalyticsData {
    pub overview: AnalyticsOverview,
    pub activity_days: Vec<ActivityDay>,
    pub heatmap: HeatmapData,
    pub sessions_by_tool: Vec<ToolSessionCount>,
    pub token_usage_by_tool: Vec<ToolTokenUsage>,
    pub session_span_buckets: Vec<SessionSpanBucket>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnalyticsOverview {
    pub total_sessions: i64,
    pub total_messages: i64,
    pub distinct_projects: i64,
    pub active_days: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityDay {
    pub day: String,
    pub session_count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeatmapWeek {
    pub days: Vec<ActivityDay>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeatmapData {
    pub weeks: Vec<HeatmapWeek>,
    pub max_sessions_in_a_day: i64,
    pub display_start_day: Option<String>,
    pub display_end_day: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolSessionCount {
    pub tool: String,
    pub session_count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolTokenUsage {
    pub tool: String,
    pub total_sessions: i64,
    pub reported_sessions: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionSpanBucket {
    pub bucket: String,
    pub session_count: i64,
}
