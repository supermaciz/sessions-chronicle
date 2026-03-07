# Basic Analytics Dashboard - Design

**Issue:** [#58 - Basic analytics](https://github.com/supermaciz/sessions-chronicle/issues/58)  
**Date:** 2026-03-07  
**Status:** Design  
**Based on:** Proposition A from `2026-03-02-basic-analytics-exploration.md`

## Decision Summary

This design targets a low-risk V1 analytics page that fits the current Sessions Chronicle architecture and can evolve incrementally.

| Decision | Choice |
|----------|--------|
| Product framing | Analytics is a secondary top-level view focused on glanceable usage insights |
| V1 scope | Summary counters, activity heatmap, sessions by tool, token consumption, session span distribution |
| Navigation | `AdwViewSwitcher` with `Sessions` and `Analytics`, justified as two peer workspaces |
| Rendering strategy | Widget-first UI, with only one required custom visualization in V1 |
| Data layer | Dedicated `src/database/analytics.rs` query module |
| Loading | Background Relm4 worker returning one aggregated `AnalyticsData` payload |
| Theme behavior | Follow libadwaita appearance, with chart colors derived for light/dark modes |
| Non-goals for V1 | Models used, top projects, subagent breakdown, date filters, week-over-week deltas |

## Product Goal

The Analytics view gives users a quick understanding of how they use Sessions Chronicle data over time, without turning the app into a full reporting tool.

V1 is intentionally narrow:
- answer "how much", "when", and "with which tool"
- stay readable on desktop and narrow windows
- avoid a dashboard architecture that requires multiple custom chart widgets before the first release

## Why This Scope

Issue `#58` describes a broader analytics surface, but not every metric needs to land in the first dashboard version.  
For V1, the design prioritizes:
- high-signal metrics already available in the database
- visuals that remain understandable without filters or drill-down
- low rendering complexity
- clear semantics for incomplete or missing token data

This keeps the first release useful while leaving room to extend the page later without redesigning the navigation model.

## Navigation

The app gains a top-level `Analytics` view alongside `Sessions`.

This uses:
- `AdwViewStack` for the two top-level pages
- `AdwViewSwitcher` in the header bar on wide windows
- `AdwViewSwitcherBar` on narrow windows

Although GNOME HIG generally positions view switchers as strongest with three to five views, this design still uses one because `Sessions` and `Analytics` are true peers: one is the operational browsing workspace, the other is the observational insights workspace.

To keep this choice justified, the Analytics page must feel substantial enough to deserve top-level placement. If the content is reduced to a few textual rows, the design should fall back to a pushed page instead of a top-level view.

## Header Bar Behavior

When the visible page is `Sessions`, the current session-oriented controls remain unchanged.

When the visible page is `Analytics`:
- the session search UI is hidden
- pane-related controls are hidden
- detail-only actions are hidden
- indexing progress remains visible because it affects analytics freshness too

This keeps the header bar semantically aligned with the active workspace instead of carrying session-only actions into analytics.

## Dashboard Layout

The Analytics page is a vertically scrollable view built for readability first, not density first.

The root layout is:
- `gtk::ScrolledWindow`
- containing an `AdwClampScrollable` or equivalent clamped content container
- with a single vertical content column
- grouped into clearly separated sections

This avoids overly wide dashboard rows on desktop while keeping the page comfortable on narrow windows.

## Section Order

The V1 page is organized from highest signal to lowest interpretation cost.

### 1. Overview

A compact overview section shows four key counters:
- Total sessions
- Total messages
- Distinct projects
- Active days

These are presented as static summary cards, not interactive rows.  
They should read as dashboard metrics, not as settings or navigation targets.

### 2. Activity

A single activity visualization shows session counts over time using a heatmap.

This is the primary custom visualization in V1 because it communicates longitudinal usage at a glance and adds clear product value that is hard to reproduce with built-in rows alone.

### 3. Tool Breakdown

A tool usage section shows sessions by tool using native widgets first:
- one row per tool
- count displayed explicitly
- optional progress bar for relative proportion

If a visual chart is still desired in V1, this section may become the second custom graphic later, but it is not required for the initial design target.

### 4. Token Consumption

A token section shows totals by tool in a boxed-list style presentation:
- tool name
- input tokens
- output tokens
- optional note when token data is partially unavailable

This section should prioritize correctness and comparability over visual flourish.

### 5. Session Span Distribution

A final section shows the distribution of session spans across a small set of fixed buckets.

This can initially be presented either as:
- a simple native grouped summary, or
- a lightweight histogram if the heatmap implementation proves maintainable

The important design constraint is that this metric appears clearly labeled as session span, not active work duration.

## Responsive Behavior

The layout must degrade gracefully without bespoke mobile redesign.

Behavior:
- overview cards wrap naturally into multiple rows
- lower sections collapse into a single vertical column
- no section should rely on side-by-side placement to remain understandable
- custom visualizations must have a minimum readable size and should not compress below that threshold

The design should prefer vertical stacking over dense two-column compositions for V1.  
A two-column row may look attractive on large screens, but it increases layout complexity and often collapses awkwardly when mixed with custom-drawn widgets.

## Loading, Empty, and Error States

The Analytics page needs explicit states instead of only showing or hiding a spinner.

### Loading

When analytics data is being computed:
- show the page shell immediately
- render section placeholders or a centered loading state
- avoid large layout jumps when data arrives

### Empty

If the database has no indexed top-level sessions yet:
- show an `AdwStatusPage`
- explain that analytics appears after sessions are indexed

### Error

If analytics loading fails:
- show an in-page error state for the analytics content area
- allow retry
- optionally pair this with a toast, but the page itself must remain understandable after the toast disappears

This is especially important because analytics is a whole workspace, not a transient panel.

## Data Semantics

The analytics design must define metric meaning explicitly so the UI does not imply false precision.

### Session Filtering

All V1 analytics are computed from top-level sessions only.

Subagent sessions are excluded from headline metrics and charts because:
- they are implementation detail for many workflows
- they would distort user-facing counts
- the current product primarily presents top-level sessions as the main unit of browsing

A later version may add explicit subagent analytics as its own section.

### Time Interpretation

Daily activity is based on the session start date.

The design must explicitly choose whether dates are grouped in:
- local time, or
- UTC

V1 should use local time because users interpret activity as part of their own calendar, not as storage timestamps.

This choice should be documented in the analytics query layer and reflected consistently in tests.

### Token Semantics

Token totals must distinguish:
- known zero values
- unknown or missing values
- unavailable values for tools or sessions that do not report tokens

The UI must never silently treat missing token data as zero.  
Where completeness is partial, the design should communicate this with supporting text rather than presenting deceptively precise totals.

### Project Semantics

If project-based counters or lists are added later, the design should define whether the displayed label is:
- full path
- basename only
- a normalized friendly project name

V1 avoids this ambiguity by not making project ranking a first-class section.

### Duration Semantics

The metric currently derivable from stored fields is elapsed session span:
`last_updated - start_time`

This is useful, but it is not a true measure of focused active work.  
Therefore the dashboard should label it as:
- Session span, or
- Elapsed session duration

It should not be labeled simply as "session length" without clarification.

## Rendering Strategy

V1 uses a widget-first rendering strategy.

That means:
- built-in libadwaita widgets for counters and ranked breakdowns
- a custom-rendered heatmap as the only required bespoke visualization
- a second custom visualization only if the first implementation remains clearly maintainable

This keeps the first release visually meaningful without forcing the design to depend on three new custom widgets at once.

### Why Not Three Custom Widgets in V1

GTK4 `snapshot()` rendering is valid and powerful, but it increases:
- widget subclassing complexity
- measurement and sizing complexity
- accessibility work
- maintenance burden for theme changes and responsive behavior

For a V1 simple target, custom rendering should be reserved for the one chart that most clearly benefits from it.

### Heatmap Recommendation

The heatmap is the strongest candidate for custom rendering because:
- it adds high information density
- it is difficult to reproduce with native rows
- it visually differentiates the analytics view from the rest of the app

If the heatmap proves too expensive to maintain, the fallback should be a textual activity summary rather than immediately adding more custom charts elsewhere.

## Architecture Fit

The design remains aligned with the current application structure:
- analytics queries are isolated from existing browsing and search queries
- background loading follows the existing Relm4 worker pattern already used for indexing
- the Analytics workspace sits beside Sessions without changing the internal session detail navigation model

At the component level, the page should receive one aggregated `AnalyticsData` payload rather than refreshing each section independently.

This keeps the view stable, limits UI churn, and matches the current model where heavier work is performed off the UI thread.

## Cache and Refresh Behavior

If cached analytics data exists and indexing has not changed the underlying dataset, switching back to the Analytics page should reuse the cached payload immediately.

After indexing completes, analytics data should be marked stale and refreshed on next view entry, or refreshed immediately if the Analytics page is currently visible.

This preserves perceived performance while keeping the dashboard trustworthy.

## Accessibility Expectations

Custom visualizations must not rely on color alone.

V1 should guarantee:
- explicit labels for all summary metrics
- textual counts next to tool breakdowns
- tooltip or accessible description support for heatmap cells
- a readable non-chart interpretation of every custom visualization nearby in the layout

The design does not need full assistive-technology implementation detail yet, but it must state that charts are supplementary, not the only source of meaning.

## Testing Strategy

Because custom-drawn GTK widgets are harder to test directly, V1 testing should focus on data transformation correctness.

Priority test areas:
- summary counter aggregation
- top-level session filtering
- daily grouping behavior under the chosen timezone rule
- token completeness handling
- session span bucketization
- heatmap normalization from sparse daily data into a full calendar grid

Manual verification should then confirm:
- theme behavior in light and dark modes
- chart readability at narrow widths
- correct empty, loading, and error states
- refresh behavior after indexing completes

This keeps the design honest: correctness is proven mostly in data logic, while rendering is verified as presentation.

## Deferred Scope

The following items are explicitly out of scope for V1:

- model distribution
- top projects ranking
- subagent usage breakdown
- week-over-week deltas
- date range filtering
- export features
- cost estimation

These are intentionally deferred because they either:
- add semantic ambiguity
- require additional UI controls or drill-down
- increase the dashboard surface before the V1 information architecture is proven

## Open Questions

The design intentionally leaves a small number of questions to validate before implementation:

1. Is the Analytics page substantial enough to justify top-level placement with a two-view `AdwViewSwitcher`?
2. Should the session activity heatmap group days in local time or UTC?
3. Should V1 include one custom visualization only, or also a second lightweight chart if the first proves maintainable?
4. What user-facing copy should explain partial token availability without making the dashboard feel unreliable?

These questions should be resolved in the final reviewed design text before implementation planning begins.

## Final Recommendation

Proceed with a conservative V1 analytics dashboard built around:
- top-level `Sessions` and `Analytics` workspaces
- a narrow, readable, vertically stacked page layout
- native libadwaita widgets for most sections
- one high-value custom heatmap visualization
- explicit metric semantics for time, tokens, and session filtering

This design gives the product a meaningful analytics surface while keeping risk concentrated in one place instead of spreading it across navigation, rendering, and data semantics simultaneously.

## Success Criteria

The V1 design is successful if it delivers all of the following:

- users can understand their overall usage at a glance
- the page feels native to the existing GNOME/libadwaita application
- metrics are semantically clear and do not imply false precision
- the dashboard remains readable on narrow windows
- the design can grow later without reworking the navigation model
