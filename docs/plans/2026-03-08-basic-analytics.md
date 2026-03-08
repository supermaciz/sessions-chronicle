# Basic Analytics Dashboard Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a V1 Analytics workspace that shows overview counters, activity heatmap, tool usage, token totals, and session span distribution for top-level sessions.

**Architecture:** Keep analytics read-only on top of the existing SQLite schema. Add a dedicated `src/database/analytics.rs` query module that returns one aggregated `AnalyticsData` payload, load it off the UI thread with a new Relm4 worker, and render it in a standalone `AnalyticsView` mounted beside the existing `Sessions` workspace with `AdwViewStack`.

**Tech Stack:** Rust 2024, Relm4 0.10, libadwaita 1.8, GTK4, rusqlite, chrono, existing fixture-driven integration tests

---

## Before You Touch Code

- This plan assumes you are already in a dedicated worktree. If not, stop and run `@superpowers:using-git-worktrees` first.
- Follow `@superpowers:test-driven-development` for every task. Do not skip the failing-test step.
- Run `@superpowers:verification-before-completion` before claiming any task is done.
- After Task 6 and Task 8, run `@superpowers:requesting-code-review`.
- Read these files once before Task 1 so you do not invent architecture that the repo already has:
  - `docs/plans/2026-03-07-basic-analytics-design.md`
  - `README.md`
  - `docs/DEVELOPMENT_WORKFLOW.md`
  - `src/app/mod.rs`
  - `src/database/mod.rs`
  - `src/indexing_worker.rs`
  - `src/ui/session_list.rs`
  - `src/ui/tool_inspector_pane.rs`
- Do not add a migration. V1 analytics reads the existing `sessions` table only, so `src/database/schema.rs` should stay unchanged unless a failing test proves otherwise.

## Implementation Rules

- Keep tasks DRY and YAGNI. Add only the models, queries, worker messages, and UI state needed by the V1 design.
- Prefer pure helper functions for formatting, bucketing, and state transitions so they can be unit-tested cheaply.
- Reuse the temp-SQLite testing pattern from `tests/search_sessions.rs` and the fixture indexing pattern from `tests/opencode_search.rs`.
- Keep commits small. One task, one commit.

### Task 1: Analytics domain model and overview query

**Files:**
- Create: `src/models/analytics.rs`
- Create: `src/database/analytics.rs`
- Modify: `src/models/mod.rs:1-15`
- Modify: `src/database/mod.rs:1-18`
- Modify: `src/lib.rs:1-10`
- Test: `tests/analytics_queries.rs`

**Step 1: Write the failing test**

Create `tests/analytics_queries.rs` with a reusable temp database helper and the first overview test. Keep the helper in this file; later tasks reuse it.

```rust
use rusqlite::{params, Connection};
use sessions_chronicle::database::analytics::load_analytics;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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
        path.push(format!("sessions-chronicle-analytics-test-{}-{}.db", std::process::id(), nanos));

        let connection = Connection::open(&path).expect("failed to open temp database");
        sessions_chronicle::database::schema::initialize_database(&connection)
            .expect("failed to initialize schema");

        Self { path, connection }
    }

    fn insert_session(
        &self,
        id: &str,
        tool: &str,
        start_time: i64,
        last_updated: i64,
        message_count: i64,
        project_path: Option<&str>,
        is_subagent: bool,
    ) {
        self.connection
            .execute(
                "INSERT INTO sessions (
                    id, tool, project_path, start_time, message_count, file_path,
                    last_updated, first_prompt, parent_session_id, is_subagent
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, ?8)",
                params![
                    id,
                    tool,
                    project_path,
                    start_time,
                    message_count,
                    format!("/tmp/{id}.jsonl"),
                    last_updated,
                    if is_subagent { 1_i64 } else { 0_i64 },
                ],
            )
            .expect("failed to insert session");
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
    db.insert_session("session-a", "claude_code", 1_709_251_200, 1_709_252_400, 4, Some("/projects/alpha"), false);
    db.insert_session("session-b", "opencode", 1_709_337_600, 1_709_338_200, 2, Some("/projects/alpha"), false);
    db.insert_session("session-c", "opencode", 1_709_337_800, 1_709_338_000, 99, Some("/projects/alpha"), true);

    let analytics = load_analytics(&db.path).expect("analytics should load");

    assert_eq!(analytics.overview.total_sessions, 2);
    assert_eq!(analytics.overview.total_messages, 6);
    assert_eq!(analytics.overview.distinct_projects, 1);
    assert_eq!(analytics.overview.active_days, 2);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test analytics_queries overview_counts_exclude_subagents -- --exact`
Expected: FAIL with an import or symbol error for `sessions_chronicle::database::analytics::load_analytics`.

**Step 3: Write minimal implementation**

Create the analytics model types and the smallest possible query implementation to satisfy the overview test.

```rust
// src/models/analytics.rs
use crate::models::Tool;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverviewMetrics {
    pub total_sessions: usize,
    pub total_messages: usize,
    pub distinct_projects: usize,
    pub active_days: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolUsageMetric {
    pub tool: Tool,
    pub sessions: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenUsageMetric {
    pub tool: Tool,
    pub total_sessions: usize,
    pub reported_sessions: usize,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionSpanBucket {
    pub label: String,
    pub sessions: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityDay {
    pub date: String,
    pub sessions: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeatmapWeek {
    pub days: Vec<ActivityDay>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeatmapData {
    pub weeks: Vec<HeatmapWeek>,
    pub max_sessions_in_a_day: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnalyticsData {
    pub overview: OverviewMetrics,
    pub sessions_by_tool: Vec<ToolUsageMetric>,
    pub tokens_by_tool: Vec<TokenUsageMetric>,
    pub session_span_buckets: Vec<SessionSpanBucket>,
    pub activity_days: Vec<ActivityDay>,
    pub heatmap: HeatmapData,
}

impl AnalyticsData {
    pub fn is_empty(&self) -> bool {
        self.overview.total_sessions == 0
    }
}
```

```rust
// src/database/analytics.rs
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

use crate::database::open_connection;
use crate::models::analytics::{AnalyticsData, OverviewMetrics};

pub fn load_analytics(db_path: &Path) -> Result<AnalyticsData> {
    if !db_path.exists() {
        return Ok(AnalyticsData::default());
    }

    let conn = open_connection(db_path)?;
    let overview = load_overview(&conn)?;

    Ok(AnalyticsData {
        overview,
        ..AnalyticsData::default()
    })
}

fn load_overview(conn: &Connection) -> Result<OverviewMetrics> {
    conn.query_row(
        "SELECT
            COUNT(*) AS total_sessions,
            COALESCE(SUM(message_count), 0) AS total_messages,
            COUNT(DISTINCT project_path) AS distinct_projects,
            COUNT(DISTINCT date(start_time, 'unixepoch', 'localtime')) AS active_days
         FROM sessions
         WHERE is_subagent = 0",
        [],
        |row| {
            Ok(OverviewMetrics {
                total_sessions: row.get::<_, i64>(0)? as usize,
                total_messages: row.get::<_, i64>(1)? as usize,
                distinct_projects: row.get::<_, i64>(2)? as usize,
                active_days: row.get::<_, i64>(3)? as usize,
            })
        },
    )
    .context("failed to load analytics overview")
}
```

Also export the new module from `src/models/mod.rs`, `src/database/mod.rs`, and `src/lib.rs`.

**Step 4: Run test to verify it passes**

Run: `cargo test --test analytics_queries overview_counts_exclude_subagents -- --exact`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/analytics_queries.rs src/models/analytics.rs src/models/mod.rs src/database/analytics.rs src/database/mod.rs src/lib.rs
git commit -m "feat: add analytics overview query"
```

### Task 2: Sessions-by-tool and session-span distribution queries

**Files:**
- Modify: `src/database/analytics.rs`
- Modify: `src/models/analytics.rs`
- Test: `tests/analytics_queries.rs`

**Step 1: Write the failing tests**

Add tests for per-tool counts and fixed span buckets. Use top-level sessions only.

```rust
#[test]
fn sessions_by_tool_and_span_buckets_are_aggregated() {
    let db = TempDatabase::new();
    db.insert_session("session-a", "claude_code", 1_709_251_200, 1_709_251_320, 4, Some("/projects/alpha"), false);
    db.insert_session("session-b", "claude_code", 1_709_337_600, 1_709_338_260, 3, Some("/projects/beta"), false);
    db.insert_session("session-c", "opencode", 1_709_424_000, 1_709_426_400, 2, Some("/projects/gamma"), false);

    let analytics = load_analytics(&db.path).expect("analytics should load");

    assert_eq!(analytics.sessions_by_tool.len(), 2);
    assert_eq!(analytics.sessions_by_tool[0].sessions, 2);
    assert_eq!(analytics.sessions_by_tool[1].sessions, 1);

    assert_eq!(analytics.session_span_buckets[0].label, "< 5 min");
    assert_eq!(analytics.session_span_buckets[0].sessions, 1);
    assert_eq!(analytics.session_span_buckets[1].label, "5-15 min");
    assert_eq!(analytics.session_span_buckets[1].sessions, 1);
    assert_eq!(analytics.session_span_buckets[3].label, "30-60 min");
    assert_eq!(analytics.session_span_buckets[3].sessions, 1);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test analytics_queries sessions_by_tool_and_span_buckets_are_aggregated -- --exact`
Expected: FAIL because `sessions_by_tool` and `session_span_buckets` are still empty.

**Step 3: Write minimal implementation**

Add two helpers inside `src/database/analytics.rs`: one SQL query for tool counts and one Rust bucketing helper for session spans.

```rust
fn load_sessions_by_tool(conn: &Connection) -> Result<Vec<ToolUsageMetric>> {
    let mut stmt = conn.prepare(
        "SELECT tool, COUNT(*) AS sessions
         FROM sessions
         WHERE is_subagent = 0
         GROUP BY tool
         ORDER BY sessions DESC, tool ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        let tool_value: String = row.get(0)?;
        let sessions: i64 = row.get(1)?;
        Ok(ToolUsageMetric {
            tool: Tool::from_storage(&tool_value).unwrap_or(Tool::ClaudeCode),
            sessions: sessions as usize,
        })
    })?;

    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

fn build_session_span_buckets(conn: &Connection) -> Result<Vec<SessionSpanBucket>> {
    let mut buckets = vec![
        SessionSpanBucket { label: "< 5 min".to_string(), sessions: 0 },
        SessionSpanBucket { label: "5-15 min".to_string(), sessions: 0 },
        SessionSpanBucket { label: "15-30 min".to_string(), sessions: 0 },
        SessionSpanBucket { label: "30-60 min".to_string(), sessions: 0 },
        SessionSpanBucket { label: "> 1 hour".to_string(), sessions: 0 },
    ];

    let mut stmt = conn.prepare(
        "SELECT MAX(last_updated - start_time, 0) AS span_seconds
         FROM sessions
         WHERE is_subagent = 0",
    )?;

    let spans = stmt.query_map([], |row| row.get::<_, i64>(0))?;
    for span in spans {
        match span? {
            0..=299 => buckets[0].sessions += 1,
            300..=899 => buckets[1].sessions += 1,
            900..=1799 => buckets[2].sessions += 1,
            1800..=3599 => buckets[3].sessions += 1,
            _ => buckets[4].sessions += 1,
        }
    }

    Ok(buckets)
}
```

Then fill those fields in `load_analytics()`.

**Step 4: Run tests to verify they pass**

Run: `cargo test --test analytics_queries sessions_by_tool_and_span_buckets_are_aggregated -- --exact`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/analytics_queries.rs src/database/analytics.rs src/models/analytics.rs
git commit -m "feat: add analytics tool and span metrics"
```

### Task 3: Local-time activity query and heatmap normalization

**Files:**
- Modify: `src/database/analytics.rs`
- Modify: `src/models/analytics.rs`
- Test: `tests/analytics_queries.rs`
- Test: `src/database/analytics.rs`

**Step 1: Write the failing tests**

Add one integration test for sparse activity days and one unit test that locks in the SQL local-time rule.

```rust
#[test]
fn activity_days_are_grouped_and_heatmap_is_zero_filled() {
    let db = TempDatabase::new();
    db.insert_session("session-a", "claude_code", 1_709_251_200, 1_709_251_800, 4, Some("/projects/alpha"), false);
    db.insert_session("session-b", "opencode", 1_709_424_000, 1_709_424_400, 2, Some("/projects/beta"), false);
    db.insert_session("session-c", "codex", 1_709_424_600, 1_709_424_900, 1, Some("/projects/gamma"), false);

    let analytics = load_analytics(&db.path).expect("analytics should load");

    assert_eq!(analytics.activity_days.len(), 2);
    assert_eq!(analytics.activity_days[0].sessions, 1);
    assert_eq!(analytics.activity_days[1].sessions, 2);
    assert_eq!(analytics.heatmap.max_sessions_in_a_day, 2);
    assert!(analytics
        .heatmap
        .weeks
        .iter()
        .flat_map(|week| week.days.iter())
        .any(|day| day.sessions == 0));
}
```

```rust
#[cfg(test)]
mod tests {
    use super::activity_group_date_sql;

    #[test]
    fn activity_group_sql_uses_localtime_modifier() {
        assert!(activity_group_date_sql().contains("'localtime'"));
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test activity_group_sql_uses_localtime_modifier activity_days_are_grouped_and_heatmap_is_zero_filled -- --exact`
Expected: FAIL because the helper and activity data are not implemented yet.

**Step 3: Write minimal implementation**

Add a tiny SQL helper, then build sparse day rows and normalize them into calendar weeks.

```rust
fn activity_group_date_sql() -> &'static str {
    "date(start_time, 'unixepoch', 'localtime')"
}

fn load_activity_days(conn: &Connection) -> Result<Vec<ActivityDay>> {
    let sql = format!(
        "SELECT {group_sql} AS local_day, COUNT(*) AS sessions
         FROM sessions
         WHERE is_subagent = 0
         GROUP BY local_day
         ORDER BY local_day ASC",
        group_sql = activity_group_date_sql(),
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(ActivityDay {
            date: row.get(0)?,
            sessions: row.get::<_, i64>(1)? as usize,
        })
    })?;

    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

fn build_heatmap(days: &[ActivityDay]) -> HeatmapData {
    if days.is_empty() {
        return HeatmapData::default();
    }

    let mut cursor = chrono::NaiveDate::parse_from_str(&days[0].date, "%Y-%m-%d").unwrap();
    let last = chrono::NaiveDate::parse_from_str(&days[days.len() - 1].date, "%Y-%m-%d").unwrap();
    let lookup: std::collections::BTreeMap<_, _> = days
        .iter()
        .map(|day| (day.date.clone(), day.sessions))
        .collect();
    let mut flat_days = Vec::new();

    while cursor <= last {
        let date = cursor.format("%Y-%m-%d").to_string();
        flat_days.push(ActivityDay {
            sessions: lookup.get(&date).copied().unwrap_or(0),
            date,
        });
        cursor = cursor.succ_opt().unwrap();
    }

    let weeks = flat_days
        .chunks(7)
        .map(|chunk| HeatmapWeek { days: chunk.to_vec() })
        .collect::<Vec<_>>();

    HeatmapData {
        max_sessions_in_a_day: flat_days.iter().map(|day| day.sessions).max().unwrap_or(0),
        weeks,
    }
}
```

Then fill `activity_days` and `heatmap` in `load_analytics()`.

**Step 4: Run tests to verify they pass**

Run: `cargo test activity_group_sql_uses_localtime_modifier activity_days_are_grouped_and_heatmap_is_zero_filled -- --exact`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/analytics_queries.rs src/database/analytics.rs src/models/analytics.rs
git commit -m "feat: add analytics activity heatmap data"
```

### Task 4: Token semantics that preserve missing vs zero data

**Files:**
- Modify: `src/database/analytics.rs`
- Modify: `src/models/analytics.rs`
- Test: `tests/analytics_queries.rs`

**Step 1: Write the failing test**

Add sessions with explicit zero tokens, partial token coverage, and no token coverage.

```rust
#[test]
fn token_totals_preserve_missing_vs_zero() {
    let db = TempDatabase::new();

    db.connection.execute(
        "INSERT INTO sessions (
            id, tool, project_path, start_time, message_count, file_path, last_updated,
            is_subagent, input_tokens, output_tokens
         ) VALUES ('session-a', 'claude_code', '/projects/alpha', 10, 4, '/tmp/a.jsonl', 20, 0, 0, 0)",
        [],
    ).unwrap();
    db.connection.execute(
        "INSERT INTO sessions (
            id, tool, project_path, start_time, message_count, file_path, last_updated,
            is_subagent, input_tokens, output_tokens
         ) VALUES ('session-b', 'claude_code', '/projects/alpha', 30, 2, '/tmp/b.jsonl', 40, 0, 120, 45)",
        [],
    ).unwrap();
    db.connection.execute(
        "INSERT INTO sessions (
            id, tool, project_path, start_time, message_count, file_path, last_updated,
            is_subagent, input_tokens, output_tokens
         ) VALUES ('session-c', 'codex', '/projects/beta', 50, 1, '/tmp/c.jsonl', 60, 0, NULL, NULL)",
        [],
    ).unwrap();

    let analytics = load_analytics(&db.path).expect("analytics should load");

    let claude = analytics.tokens_by_tool.iter().find(|row| row.tool.to_storage() == "claude_code").unwrap();
    assert_eq!(claude.total_sessions, 2);
    assert_eq!(claude.reported_sessions, 2);
    assert_eq!(claude.input_tokens, Some(120));
    assert_eq!(claude.output_tokens, Some(45));

    let codex = analytics.tokens_by_tool.iter().find(|row| row.tool.to_storage() == "codex").unwrap();
    assert_eq!(codex.total_sessions, 1);
    assert_eq!(codex.reported_sessions, 0);
    assert_eq!(codex.input_tokens, None);
    assert_eq!(codex.output_tokens, None);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test analytics_queries token_totals_preserve_missing_vs_zero -- --exact`
Expected: FAIL because `tokens_by_tool` is still empty.

**Step 3: Write minimal implementation**

Aggregate token rows by tool. Treat a row as reportable only when both `input_tokens` and `output_tokens` are non-NULL.

```rust
fn load_token_usage(conn: &Connection) -> Result<Vec<TokenUsageMetric>> {
    let mut stmt = conn.prepare(
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
    )?;

    let rows = stmt.query_map([], |row| {
        let tool_value: String = row.get(0)?;
        let reported_sessions: i64 = row.get(2)?;
        Ok(TokenUsageMetric {
            tool: Tool::from_storage(&tool_value).unwrap_or(Tool::ClaudeCode),
            total_sessions: row.get::<_, i64>(1)? as usize,
            reported_sessions: reported_sessions as usize,
            input_tokens: (reported_sessions > 0).then(|| row.get::<_, i64>(3).unwrap_or(0)),
            output_tokens: (reported_sessions > 0).then(|| row.get::<_, i64>(4).unwrap_or(0)),
        })
    })?;

    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}
```

That data model gives the UI everything it needs:
- `reported_sessions == 0` means show `--` instead of `0`
- `reported_sessions < total_sessions` means show the subtitle `Based on N of M sessions that report token usage`
- explicit zero totals remain `Some(0)` and render as `0`

**Step 4: Run test to verify it passes**

Run: `cargo test --test analytics_queries token_totals_preserve_missing_vs_zero -- --exact`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/analytics_queries.rs src/database/analytics.rs src/models/analytics.rs
git commit -m "feat: add analytics token semantics"
```

### Task 5: Analytics view shell and native dashboard sections

**Files:**
- Create: `src/ui/analytics_view.rs`
- Modify: `src/ui/mod.rs:1-12`
- Modify: `data/resources/style.css:1-240`
- Test: `src/ui/analytics_view.rs`

**Step 1: Write the failing tests**

Put the state machine in plain Rust helpers inside `src/ui/analytics_view.rs` so you can test it without spinning a full GTK app.

```rust
#[cfg(test)]
mod tests {
    use super::{AnalyticsPageState, AnalyticsViewModel};
    use crate::models::analytics::AnalyticsData;

    #[test]
    fn entered_requests_refresh_when_empty() {
        let mut model = AnalyticsViewModel::default();
        assert!(model.on_entered());
        assert_eq!(model.state, AnalyticsPageState::Loading);
    }

    #[test]
    fn stale_cache_keeps_content_visible_while_refreshing() {
        let mut model = AnalyticsViewModel::from_data(AnalyticsData {
            overview: crate::models::analytics::OverviewMetrics {
                total_sessions: 1,
                total_messages: 1,
                distinct_projects: 1,
                active_days: 1,
            },
            ..AnalyticsData::default()
        });

        model.mark_stale();
        assert!(model.on_entered());
        assert_eq!(model.state, AnalyticsPageState::Ready);
        assert!(model.refresh_in_flight);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test entered_requests_refresh_when_empty stale_cache_keeps_content_visible_while_refreshing -- --exact`
Expected: FAIL because `AnalyticsViewModel` and `AnalyticsPageState` do not exist yet.

**Step 3: Write minimal implementation**

Build a new `SimpleComponent` that owns an `adw::StatusPage` for empty/error/loading states and a scrollable content column for ready state. Start with native widgets only; leave the activity section as a placeholder container that Task 6 will replace.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AnalyticsPageState {
    #[default]
    Loading,
    Ready,
    Empty,
    Error,
}

#[derive(Debug, Default)]
struct AnalyticsViewModel {
    state: AnalyticsPageState,
    data: Option<AnalyticsData>,
    stale: bool,
    refresh_in_flight: bool,
}

impl AnalyticsViewModel {
    fn from_data(data: AnalyticsData) -> Self {
        Self {
            state: AnalyticsPageState::Ready,
            data: Some(data),
            stale: false,
            refresh_in_flight: false,
        }
    }

    fn on_entered(&mut self) -> bool {
        if self.data.is_none() {
            self.state = AnalyticsPageState::Loading;
            self.refresh_in_flight = true;
            return true;
        }

        if self.stale {
            self.refresh_in_flight = true;
            return true;
        }

        false
    }

    fn mark_stale(&mut self) {
        self.stale = true;
    }
}

pub enum AnalyticsViewMsg {
    Entered,
    LoadingStarted,
    Loaded(AnalyticsData),
    LoadFailed(String),
    MarkStale,
    Retry,
}

pub enum AnalyticsViewOutput {
    RefreshRequested,
}
```

Render the ready state with:
- overview metric cards
- activity section container (placeholder widget for now)
- sessions-by-tool boxed list with progress bars
- token boxed list rows with subtitle text
- session span boxed list rows with progress bars

Add CSS classes in `data/resources/style.css` for:
- `.analytics-page`
- `.analytics-section`
- `.analytics-metric-card`
- `.analytics-metric-value`
- `.analytics-metric-label`
- `.analytics-section-title`
- `.analytics-progress-row`

**Step 4: Run tests to verify they pass**

Run: `cargo test entered_requests_refresh_when_empty stale_cache_keeps_content_visible_while_refreshing -- --exact`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/ui/analytics_view.rs src/ui/mod.rs data/resources/style.css
git commit -m "feat: add analytics dashboard shell"
```

### Task 6: Custom heatmap widget and accessibility helpers

**Files:**
- Create: `src/ui/analytics_heatmap.rs`
- Modify: `src/ui/analytics_view.rs`
- Modify: `src/ui/mod.rs:1-13`
- Modify: `data/resources/style.css:1-240`
- Test: `src/ui/analytics_heatmap.rs`

**Step 1: Write the failing tests**

Test only pure helpers and public widget setters. Do not try to snapshot pixels in unit tests.

```rust
#[cfg(test)]
mod tests {
    use super::{cell_accessible_label, intensity_class};
    use crate::models::analytics::ActivityDay;

    #[test]
    fn accessible_label_describes_empty_and_non_empty_cells() {
        assert_eq!(
            cell_accessible_label(&ActivityDay { date: "2026-03-01".to_string(), sessions: 0 }),
            "2026-03-01: no sessions"
        );
        assert_eq!(
            cell_accessible_label(&ActivityDay { date: "2026-03-02".to_string(), sessions: 3 }),
            "2026-03-02: 3 sessions"
        );
    }

    #[test]
    fn intensity_class_scales_against_max_day() {
        assert_eq!(intensity_class(0, 4), "heatmap-cell-empty");
        assert_eq!(intensity_class(1, 4), "heatmap-cell-low");
        assert_eq!(intensity_class(4, 4), "heatmap-cell-high");
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test accessible_label_describes_empty_and_non_empty_cells intensity_class_scales_against_max_day -- --exact`
Expected: FAIL because the widget module does not exist yet.

**Step 3: Write minimal implementation**

Create the first custom widget in the repo. Keep it tiny: immutable draw data, simple `measure`, simple `snapshot`, helper functions for label/intensity, and a setter that queues redraw.

```rust
use gtk::glib;
use gtk::subclass::prelude::*;
use relm4::gtk;
use std::cell::RefCell;

use crate::models::analytics::{ActivityDay, HeatmapData};

fn cell_accessible_label(day: &ActivityDay) -> String {
    if day.sessions == 0 {
        format!("{}: no sessions", day.date)
    } else {
        format!("{}: {} sessions", day.date, day.sessions)
    }
}

fn intensity_class(sessions: usize, max_sessions: usize) -> &'static str {
    if sessions == 0 || max_sessions == 0 {
        "heatmap-cell-empty"
    } else if sessions * 4 >= max_sessions * 3 {
        "heatmap-cell-high"
    } else if sessions * 2 >= max_sessions {
        "heatmap-cell-medium"
    } else {
        "heatmap-cell-low"
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct AnalyticsHeatmap {
        pub data: RefCell<HeatmapData>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AnalyticsHeatmap {
        const NAME: &'static str = "ScAnalyticsHeatmap";
        type Type = super::AnalyticsHeatmap;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for AnalyticsHeatmap {}

    impl WidgetImpl for AnalyticsHeatmap {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => (280, 420, -1, -1),
                gtk::Orientation::Vertical => (120, 180, -1, -1),
                _ => (120, 180, -1, -1),
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            super::snapshot_heatmap(&widget, snapshot, &self.data.borrow());
        }
    }
}

glib::wrapper! {
    pub struct AnalyticsHeatmap(ObjectSubclass<imp::AnalyticsHeatmap>)
        @extends gtk::Widget;
}
```

Then replace the activity placeholder in `src/ui/analytics_view.rs` with the real widget plus a short textual legend nearby so the chart is not the only source of meaning.

Add CSS classes for `.heatmap-cell-empty`, `.heatmap-cell-low`, `.heatmap-cell-medium`, and `.heatmap-cell-high` in `data/resources/style.css`.

**Step 4: Run tests to verify they pass**

Run: `cargo test accessible_label_describes_empty_and_non_empty_cells intensity_class_scales_against_max_day -- --exact`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/ui/analytics_heatmap.rs src/ui/analytics_view.rs src/ui/mod.rs data/resources/style.css
git commit -m "feat: add analytics heatmap widget"
```

### Task 7: Analytics worker, view switcher navigation, and refresh-on-indexing logic

**Files:**
- Create: `src/analytics_worker.rs`
- Create: `src/app/handlers/analytics.rs`
- Modify: `src/app/handlers/mod.rs:1-4`
- Modify: `src/app/types.rs:1-61`
- Modify: `src/app/mod.rs:57-125`
- Modify: `src/app/mod.rs:153-258`
- Modify: `src/app/mod.rs:263-328`
- Modify: `src/app/mod.rs:570-626`
- Modify: `src/app/handlers/indexing.rs:11-67`
- Modify: `src/app/handlers/navigation.rs:13-94`
- Test: `src/app/handlers/analytics.rs`
- Test: `src/app/mod.rs`

**Step 1: Write the failing tests**

Add pure app-state helpers before touching the live window wiring.

```rust
#[cfg(test)]
mod tests {
    use super::{header_visibility_for_workspace, AppWorkspace};

    #[test]
    fn analytics_workspace_hides_session_only_header_controls() {
        let visibility = header_visibility_for_workspace(AppWorkspace::Analytics, true, true);
        assert!(!visibility.back_button);
        assert!(!visibility.search_toggle);
        assert!(!visibility.resume_button);
        assert!(!visibility.parent_button);
        assert!(!visibility.pane_toggle);
        assert!(visibility.indexing_spinner);
    }

    #[test]
    fn indexing_completion_marks_analytics_stale() {
        let state = super::post_indexing_analytics_state(false, true);
        assert!(state.mark_stale);
        assert!(state.refresh_now);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test analytics_workspace_hides_session_only_header_controls indexing_completion_marks_analytics_stale -- --exact`
Expected: FAIL because the workspace helpers do not exist yet.

**Step 3: Write minimal implementation**

Add the new worker, workspace enum, and app wiring.

```rust
// src/analytics_worker.rs
use relm4::{ComponentSender, Worker};
use std::path::PathBuf;

use crate::database::analytics::load_analytics;
use crate::models::analytics::AnalyticsData;

pub struct AnalyticsWorker {
    db_path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum AnalyticsWorkerInput {
    Load,
}

#[derive(Debug, Clone)]
pub enum AnalyticsWorkerOutput {
    Loaded(AnalyticsData),
    Failed(String),
}

impl Worker for AnalyticsWorker {
    type Init = PathBuf;
    type Input = AnalyticsWorkerInput;
    type Output = AnalyticsWorkerOutput;

    fn init(db_path: Self::Init, _sender: ComponentSender<Self>) -> Self {
        Self { db_path }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            AnalyticsWorkerInput::Load => match load_analytics(&self.db_path) {
                Ok(data) => {
                    let _ = sender.output(AnalyticsWorkerOutput::Loaded(data));
                }
                Err(err) => {
                    let _ = sender.output(AnalyticsWorkerOutput::Failed(err.to_string()));
                }
            },
        }
    }
}
```

```rust
// src/app/types.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AppWorkspace {
    Sessions,
    Analytics,
}
```

In `src/app/mod.rs` wire these pieces together:
- create `AnalyticsView` controller and `AnalyticsWorker` worker next to the existing `SessionList`, `SessionDetail`, and `IndexingWorker`
- replace the single-session content root with `adw::ViewStack`
- keep the existing sessions navigation tree as the `Sessions` page inside that stack
- add a second `Analytics` page that hosts `analytics_view.widget()`
- add `adw::ViewSwitcher` in the header bar title widget and `adw::ViewSwitcherBar` at the bottom of `adw::ToolbarView`
- use the libadwaita adaptive pattern from the `ViewSwitcher`/`ViewSwitcherBar` docs: `set_stack`, `set_policy(adw::ViewSwitcherPolicy::Wide)`, and reveal the bottom bar only on narrow layouts
- forward `AnalyticsViewOutput::RefreshRequested` to `AnalyticsWorkerInput::Load`
- when indexing completes, call the existing session-list reload path, mark analytics stale, and refresh immediately if the visible workspace is `Analytics`

Use a small helper to keep header behavior deterministic:

```rust
struct HeaderVisibility {
    back_button: bool,
    search_toggle: bool,
    resume_button: bool,
    parent_button: bool,
    pane_toggle: bool,
    indexing_spinner: bool,
}

fn header_visibility_for_workspace(
    workspace: AppWorkspace,
    detail_visible: bool,
    parent_session_visible: bool,
) -> HeaderVisibility {
    match workspace {
        AppWorkspace::Sessions => HeaderVisibility {
            back_button: detail_visible,
            search_toggle: true,
            resume_button: detail_visible,
            parent_button: detail_visible && parent_session_visible,
            pane_toggle: true,
            indexing_spinner: true,
        },
        AppWorkspace::Analytics => HeaderVisibility {
            back_button: false,
            search_toggle: false,
            resume_button: false,
            parent_button: false,
            pane_toggle: false,
            indexing_spinner: true,
        },
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test analytics_workspace_hides_session_only_header_controls indexing_completion_marks_analytics_stale -- --exact`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/analytics_worker.rs src/app/handlers/analytics.rs src/app/handlers/mod.rs src/app/types.rs src/app/mod.rs src/app/handlers/indexing.rs src/app/handlers/navigation.rs
git commit -m "feat: wire analytics workspace into app"
```

### Task 8: Fixture integration coverage and full verification

**Files:**
- Create: `tests/analytics_integration.rs`
- Modify: `src/ui/analytics_view.rs` (only if verification exposes missing state or copy)
- Modify: `src/app/mod.rs` (only if verification exposes missing refresh behavior)
- Test: `tests/analytics_integration.rs`

**Step 1: Write the failing integration test**

Index the real fixture set, then assert that the analytics payload is non-empty and still excludes subagents from headline metrics.

```rust
use std::path::Path;

use sessions_chronicle::database::analytics::load_analytics;
use sessions_chronicle::database::SessionIndexer;
use sessions_chronicle::session_sources::SessionSources;

#[test]
fn fixture_index_produces_non_empty_analytics_payload() {
    let db = super::TempDatabase::new();
    let sources = SessionSources::resolve(Some(Path::new("tests/fixtures")));

    let mut indexer = SessionIndexer::new(&db.path).expect("failed to create indexer");
    let stats = indexer
        .index_all_incremental(&sources)
        .expect("failed to index fixture sessions");

    assert!(stats.indexed > 0);

    let analytics = load_analytics(&db.path).expect("analytics should load");

    assert!(analytics.overview.total_sessions > 0);
    assert!(!analytics.sessions_by_tool.is_empty());
    assert!(!analytics.session_span_buckets.is_empty());
    assert!(analytics.heatmap.max_sessions_in_a_day > 0);
}
```

If `TempDatabase` is private to `tests/analytics_queries.rs`, copy the helper into `tests/analytics_integration.rs`. Do not share it through production code.

**Step 2: Run test to verify it fails**

Run: `cargo test --test analytics_integration fixture_index_produces_non_empty_analytics_payload -- --exact`
Expected: FAIL until the full query surface and app wiring are complete.

**Step 3: Write minimal implementation fixes**

Fix only the gaps exposed by the integration test and manual checks:
- empty-state copy should say analytics appears after indexing
- error state should stay in-page and offer retry
- token subtitle should say `Based on N of M sessions that report token usage`
- analytics should refresh after indexing completes if the Analytics page is visible
- cached analytics should remain visible while a refresh is running

**Step 4: Run the full verification suite**

Run these commands in order:

```bash
cargo fmt --all
cargo clippy --all -- -D warnings
cargo test --all --no-fail-fast
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures
```

Expected:
- `cargo fmt --all` exits 0
- `cargo clippy --all -- -D warnings` exits 0
- `cargo test --all --no-fail-fast` exits 0
- the Flatpak run launches and shows a second top-level `Analytics` workspace

Manual checks in the running app:
- `Sessions` and `Analytics` switch cleanly on desktop width
- narrow width shows the bottom `ViewSwitcherBar`
- Analytics hides session-only header controls but keeps the indexing spinner
- empty/error/loading states are understandable without a toast
- heatmap remains readable at narrow widths
- token section shows `--` for tools with unavailable token data and never turns missing data into zero
- reindex from Preferences marks analytics stale and refreshes the page

**Step 5: Commit**

If Step 4 required code changes:

```bash
git add tests/analytics_integration.rs src/ui/analytics_view.rs src/app/mod.rs
git commit -m "test: verify analytics dashboard end to end"
```

If Step 4 did not require code changes, skip the commit and move straight to code review.

## Done Checklist

- `src/database/analytics.rs` is the only new database module
- `src/database/schema.rs` is unchanged
- `src/models/analytics.rs` contains the full payload shape for the page
- `src/analytics_worker.rs` returns one aggregated payload
- `src/ui/analytics_view.rs` owns loading, empty, error, and ready states
- `src/ui/analytics_heatmap.rs` is the only custom widget added for V1 analytics
- `src/app/mod.rs` mounts a new top-level `Analytics` workspace beside `Sessions`
- `tests/analytics_queries.rs` covers aggregation semantics
- `tests/analytics_integration.rs` covers fixture-driven end-to-end analytics loading
- full verification commands from `docs/DEVELOPMENT_WORKFLOW.md` pass
