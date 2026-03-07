# Basic Analytics Dashboard - Design

**Issue:** [#58 - Basic analytics](https://github.com/supermaciz/sessions-chronicle/issues/58)
**Date:** 2026-03-07
**Status:** Design
**Based on:** Proposition A from `2026-03-02-basic-analytics-exploration.md`

## Decision Summary

| Decision | Choice |
|----------|--------|
| Navigation | AdwViewSwitcher (Sessions / Analytics) |
| Chart rendering | Pure GtkSnapshot (append_color, append_fill, append_stroke, append_layout) |
| V1 scope | Heatmap + histogram + bar chart (donut deferred to V2) |
| Data layer | Dedicated `database/analytics.rs` module |
| Loading | Async Relm4 Worker |
| Theming | Hardcoded Adwaita palette + light/dark detection via StyleManager |
| Extra dependencies | None (no plotters-gtk4) |

## Navigation: AdwViewSwitcher

The current plain header title is replaced by an `AdwViewSwitcher` offering two
top-level views: **Sessions** and **Analytics**.

### Widget Hierarchy

```
AdwApplicationWindow
  AdwToastOverlay
    AdwToolbarView
      [top_bar] AdwHeaderBar
        [title_widget] AdwViewSwitcher        // NEW
          page: "Sessions"  (icon: view-list-symbolic)
          page: "Analytics" (icon: chart-line-symbolic)
        [pack_start] search_toggle            // visible on Sessions only
        [pack_end] pane_toggle, menu_button, spinner
      [content] gtk::Box (vertical)
        SearchBar                             // visible on Sessions only
        AdwViewStack                          // NEW
          page "sessions":
            AdwOverlaySplitView               // existing session list + pane
          page "analytics":
            AnalyticsDashboard                // NEW Relm4 component
```

### Adaptive Behavior

An `AdwViewSwitcherBar` at the bottom of the window takes over when the window
is too narrow. This is the standard GNOME pattern.

### Impact on Existing Code

- `SearchBar` and `pane_toggle` are hidden when on the Analytics page.
- `back_button`, `resume_button`, `parent_session_button` remain tied to the
  `NavigationView` inside the Sessions page.
- The existing `NavigationView` moves inside the "sessions" page of the
  `ViewStack`.

## Dashboard Layout

The analytics view is a `gtk::ScrolledWindow` containing a vertical `gtk::Box`.
Width is constrained via `set_halign(Center)` + `set_size_request(max_width)`
to avoid stretching on wide displays.

### Sections (Top to Bottom)

#### 1. Summary Counters

4 cards in a horizontal `gtk::FlowBox`:

- Total sessions
- Total messages
- Distinct projects
- Active days

Each card is an `AdwActionRow` inside a `gtk::Frame` with CSS class `.card`.
Value as `title` (large), label as `subtitle` (small). Pure Adwaita widgets.

#### 2. Activity Heatmap (Custom GtkSnapshot)

GitHub-style grid: 52 columns (weeks) x 7 rows (days of week).
Cells colored by intensity (sessions/day).
Labels: months on top, weekday abbreviations on the left.
Contained in an `AdwPreferencesGroup` titled "Activity".

#### 3-4. Two-Column Row

Left: **Sessions by Tool** (custom `BarChartWidget`)
Right: **Token Consumption** (Adwaita widgets)

On narrow windows, stacks vertically via `FlowBox` or breakpoint.

**Sessions by Tool:** Horizontal bars, one per tool. Color per tool. Labels left,
values right. Contained in `AdwPreferencesGroup`.

**Token Consumption:** `AdwPreferencesGroup` with `AdwActionRow` per tool.
Each row: tool name, input/output tokens in subtitle. Optional inline progress
bar for relative proportion.

#### 5. Session Length Distribution (Custom GtkSnapshot)

Vertical histogram with duration buckets: 0-5 min, 5-15 min, 15-30 min,
30-60 min, 1h+.
Bars with bucket labels at bottom, count at top.
Contained in `AdwPreferencesGroup` titled "Session Length".

### Mockup

```
+--------------------------------------------------+
| [Total Sessions] [Messages] [Projects] [Days]    |
+--------------------------------------------------+
| Activity                                          |
| Mo  [][][][][][][][][][][][][][][]...[][][][]      |
| Tu  [][][][][][][][][][][][][][][]...[][][][]      |
| ...                                               |
| Su  [][][][][][][][][][][][][][][]...[][][][]      |
+--------------------------------------------------+
| Sessions by Tool          | Token Consumption     |
| Claude    ████████████ 42 | Claude  120k / 80k    |
| OpenCode  █████ 15        | OpenCode 30k / 20k    |
| Codex     ██ 6            | ...                   |
| Vibe      █ 3             |                       |
+--------------------------------------------------+
| Session Length                                    |
|    ██                                             |
| ██ ██                                             |
| ██ ██ ██ █                                        |
| 0-5 5-15 15-30 30-60 1h+                         |
+--------------------------------------------------+
```

## Custom GtkSnapshot Widgets

Three custom widgets, each implementing `WidgetImpl::snapshot()`.

### HeatmapWidget (`src/ui/analytics/heatmap.rs`)

- Subclass of `gtk::Widget` via `glib::wrapper!`
- Data: `Vec<(NaiveDate, u32)>` (date, count)
- `snapshot()`: iterates 52x7 grid, `append_color()` per cell with intensity-
  interpolated color
- Month labels: `append_layout()` with `pango::Layout`
- `measure()`: returns fixed size based on `cell_size * grid + padding`
- Tooltip on hover via `set_has_tooltip(true)` + `query-tooltip` signal
  showing "X sessions on YYYY-MM-DD"

### BarChartWidget (`src/ui/analytics/bar_chart.rs`)

- Subclass of `gtk::Widget`
- Data: `Vec<(String, u32, gdk::RGBA)>` (label, value, color)
- `snapshot()`: horizontal bars via `append_color()`, labels via
  `append_layout()`
- Bars proportional to max value
- Height: `n_bars * (bar_height + spacing)`

### HistogramWidget (`src/ui/analytics/histogram.rs`)

- Subclass of `gtk::Widget`
- Data: `Vec<(String, u32)>` (bucket_label, count)
- `snapshot()`: vertical bars via `append_color()`, bucket labels at bottom
  and counts at top via `append_layout()`
- Width: `n_buckets * (bar_width + spacing)`

### Common Widget Pattern

```rust
mod imp {
    use gtk::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct HeatmapWidget {
        pub(super) data: RefCell<Vec<(chrono::NaiveDate, u32)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HeatmapWidget {
        const NAME: &'static str = "ScHeatmapWidget";
        type Type = super::HeatmapWidget;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for HeatmapWidget {}

    impl WidgetImpl for HeatmapWidget {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            // Pure GtkSnapshot drawing:
            // append_color() for cells
            // append_layout() for labels
        }

        fn measure(
            &self,
            orientation: gtk::Orientation,
            _for_size: i32,
        ) -> (i32, i32, i32, i32) {
            // Return (minimum, natural, min_baseline, nat_baseline)
        }
    }
}
```

## Theming: `src/ui/analytics/colors.rs`

```rust
pub struct ChartColors {
    pub heatmap_levels: [gdk::RGBA; 5],  // transparent -> green_5
    pub tool_colors: HashMap<Tool, gdk::RGBA>,
    pub histogram_bar: gdk::RGBA,
    pub text: gdk::RGBA,
    pub text_dim: gdk::RGBA,
}

pub fn chart_palette(is_dark: bool) -> ChartColors;
```

- Heatmap: 5 green levels from the Adwaita palette
- Bar chart: per-tool colors (blue for Claude, teal for OpenCode, etc.)
- Dark mode detection via `adw::StyleManager::default().is_dark()`
- Reconnect to `notify::dark` signal to call `queue_draw()` on theme change

## Data Layer: `src/database/analytics.rs`

### Types

```rust
pub struct SummaryCounters {
    pub total_sessions: u32,
    pub total_messages: u32,
    pub distinct_projects: u32,
    pub active_days: u32,
}

pub struct ToolBreakdown {
    pub tool: Tool,
    pub session_count: u32,
}

pub struct DailyActivity {
    pub date: NaiveDate,
    pub session_count: u32,
}

pub struct TokenBreakdown {
    pub tool: Tool,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

pub struct LengthBucket {
    pub label: String,
    pub min_seconds: u64,
    pub max_seconds: u64,
    pub count: u32,
}
```

### Query Functions

```rust
pub fn get_summary_counters(conn: &Connection) -> Result<SummaryCounters>;
pub fn get_sessions_by_tool(conn: &Connection) -> Result<Vec<ToolBreakdown>>;
pub fn get_daily_activity(conn: &Connection, days: u32) -> Result<Vec<DailyActivity>>;
pub fn get_token_consumption(conn: &Connection) -> Result<Vec<TokenBreakdown>>;
pub fn get_session_length_distribution(conn: &Connection) -> Result<Vec<LengthBucket>>;
```

### Key SQL

- **Summary:** `SELECT COUNT(*), SUM(message_count), COUNT(DISTINCT project_path), COUNT(DISTINCT date(start_time, 'unixepoch')) FROM sessions WHERE is_subagent = 0`
- **By tool:** `SELECT tool, COUNT(*) FROM sessions WHERE is_subagent = 0 GROUP BY tool ORDER BY COUNT(*) DESC`
- **Daily activity:** `SELECT date(start_time, 'unixepoch') as d, COUNT(*) FROM sessions WHERE is_subagent = 0 AND start_time >= ? GROUP BY d`
- **Token consumption:** `SELECT tool, SUM(input_tokens), SUM(output_tokens) FROM sessions WHERE is_subagent = 0 GROUP BY tool`
- **Session length:** `SELECT (last_updated - start_time) as duration_secs FROM sessions WHERE is_subagent = 0` then bucket in Rust

All queries filter `is_subagent = 0` to count only top-level sessions.
Heatmap covers the last 365 days by default.
Duration buckets are defined in Rust for flexibility.

## Async Worker: `src/analytics_worker.rs`

Follows the same pattern as `IndexingWorker`:

```rust
pub enum AnalyticsWorkerInput {
    LoadData(PathBuf),
}

pub enum AnalyticsWorkerOutput {
    DataReady(AnalyticsData),
    Failed,
}

pub struct AnalyticsData {
    pub counters: SummaryCounters,
    pub tools: Vec<ToolBreakdown>,
    pub activity: Vec<DailyActivity>,
    pub tokens: Vec<TokenBreakdown>,
    pub length_dist: Vec<LengthBucket>,
}
```

## Analytics Dashboard Component

`src/ui/analytics/mod.rs` -- Relm4 `SimpleComponent`.

- **Init:** receives `PathBuf` (db_path)
- **Model:** `Option<AnalyticsData>` + `loading: bool`
- **Messages:**
  - `Refresh` -- sends `LoadData` to worker
  - `DataReady(AnalyticsData)` -- updates model, calls `queue_draw()`
  - `LoadFailed` -- shows error toast

### Lifecycle

1. User switches to Analytics view -> `App` sends `Refresh`
2. Dashboard shows a spinner
3. Worker executes all 5 SQL queries in one pass
4. `DataReady` arrives, spinner hides, widgets update
5. Data is cached in model -- no re-query on view switch without changes

### Cache Invalidation

After `IndexingCompleted` in `App`, if the user is on the Analytics view,
a `Refresh` is automatically triggered.

## File Structure

```
src/ui/analytics/
  mod.rs              -- AnalyticsDashboard component
  heatmap.rs          -- HeatmapWidget (GtkSnapshot)
  bar_chart.rs        -- BarChartWidget (GtkSnapshot)
  histogram.rs        -- HistogramWidget (GtkSnapshot)
  colors.rs           -- Adwaita palette + dark mode detection
src/analytics_worker.rs  -- Async worker
src/database/analytics.rs -- SQL queries
```

## Testing

### Unit Tests (`src/database/analytics.rs`)

Each query function tested with an in-memory SQLite database.
Fixture: insert sessions with different tools, dates, tokens, durations.
Verify counters, breakdowns, and buckets.

### Integration Tests (`tests/`)

End-to-end: load existing fixtures (`tests/fixtures/`), run analytics queries,
verify invariants (total sessions > 0, known tools present).

No UI tests for GtkSnapshot widgets (headless GTK is too complex).
Manual verification with `--sessions-dir tests/fixtures`.

### Manual Verification

- `flatpak-builder --run ... sessions-chronicle --sessions-dir tests/fixtures`
  and switch to Analytics view
- Verify light/dark theming by toggling via GNOME Settings
- Verify adaptive behavior by resizing the window
  (ViewSwitcher -> ViewSwitcherBar transition)

### CI

Existing checks (`cargo fmt`, `cargo clippy`, `cargo test`) cover the new code
without additional configuration.

## V2 Scope (Deferred)

- Donut chart for models used
- Top projects list
- Subagent usage breakdown
- Week-over-week deltas on summary counters
- Date range filtering
