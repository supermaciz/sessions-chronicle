# Date Filter: Design Exploration

**Issue:** [#85](https://github.com/supermaciz/sessions-chronicle/issues/85) — Date filter for search and session browsing  
**Date:** 2026-05-26  
**Status:** Open  
**Source:** Wireframes produced via Claude Design (claude.ai/design), handoff bundle `sessions-chronicle-dater-filter`.

## Context

Project and AI-assistant filters are already wired through search and browsing
(`src/models/project_filter.rs`, `src/ui/sidebar.rs`, `src/ui/session_list.rs`).
The remaining structured filter is **date range**, which today forces users to
guess keywords for questions like *"what happened last week in this project?"*
or *"show me Codex sessions from this month"*.

This exploration maps the design space — it does not commit to a single
approach. Five wireframes were sketched: two aligned with GNOME HIG, three
more experimental. All wireframes share the same lo-fi style (black/white
sketch, a single blue accent for the active selection, a single warm-red for
annotations).

### Key questions this exploration answers

1. **Surface placement** — sidebar section, header bar button, popover, or a
   new viz strip above the list?
2. **Range model** — preset list only, free range picker, or both?
3. **Discoverability vs. density** — how much screen space the filter takes
   when idle.
4. **Cross-filter behaviour** — composition with existing project and
   assistant filters, and with the search field.
5. **Stretch value** — does the filter also expose activity patterns
   (histogram / heatmap / sparkbar)?

---

## Variant A — Sidebar preset list (GNOME HIG)

A new **Date** section in the sidebar, sitting between **AI Assistants** and
**Projects**, reusing the exact same `ListBox` row pattern (label + badge
count).

![Variant A — Sidebar preset list](<../mockups/date-filter/A _ Sidebar preset list.png>)

### Behaviour

- Preset rows: *Any time · Today · Last 7 days · Last 30 days · This year ·
  Custom range…*
- Single-select inside the section; the active row gets the accent background
  and an accent-coloured count badge.
- *Custom range…* opens a secondary dialog (or expands inline) with a
  start/end picker.
- An info banner appears above the session list:
  *"Showing 23 sessions · Last 7 days"* with a `clear ✕` affordance.
- Composes naturally with the existing assistant and project filters — all
  three sections are independent selections in the same sidebar.

### Trade-offs

| Pros | Cons |
|------|------|
| Zero new GTK pattern — reuses the assistant/project list look | Sidebar gets a third independent section; vertical space pressure on small windows |
| Counts per preset double as quick at-a-glance stats | Free range needs a secondary surface (popover or dialog) |
| Keyboard navigation works out of the box | Only one date scope at a time — no "last 7d OR last month" composition |
| Smallest implementation: a `ListBox` of `ActionRow` + a date-range model | Long range labels (e.g. "Apr 14 – Apr 21") truncate at sidebar width |

---

## Variant B — Header-bar Date button + AdwCalendar popover (GNOME HIG)

A pill button (*"📅 Apr 14 – Apr 21 ▾"*) lives in the header bar next to the
search entry. Clicking it opens an `AdwCalendar`-style popover with a month
grid, range selection, quick-preset chips, and Clear/Apply actions.

![Variant B — Calendar popover](<../mockups/date-filter/B _ Header bar _ calendar popover.png>)

### Behaviour

- The button's label always reflects the active range (or *"Any date"*).
- Popover content: month grid with start/end highlighting, preset chips
  (*Today · 7d · 30d · YTD*), session-count preview, *Clear* + *Apply*.
- Sidebar layout is untouched.
- Info banner still appears above the list for symmetry with the other
  filters.

### Trade-offs

| Pros | Cons |
|------|------|
| Closest to GNOME conventions (Files, Calendar, Contacts use popovers for range pickers) | Adds visual weight to the header bar — competes with search for attention |
| Full free range without leaving the popover | More widgets to wire (popover, calendar, chips, two buttons) |
| Easy to dismiss (Esc / outside click) — no permanent sidebar real-estate cost | Less discoverable than a sidebar section on cold start |
| Pairs well with keyboard shortcut (e.g. `Ctrl+Shift+D`) | Calendar grid in a small popover can feel cramped on dense months |

---

## Variant C — Brushable histogram strip (Creative)

A mini activity bar chart sits between the header bar and the session list.
Each bar is a day/week bucket of session count. Two blue handles delimit the
selected range and drag to resize it.

![Variant C — Brushable histogram](<../mockups/date-filter/C _ Brushable histogram strip.png>)

### Behaviour

- Strip caption: *"Activity over time — drag to filter · Apr 5 – Apr 17 · 23
  sessions"*.
- Inside-brush bars use the accent colour; outside bars are muted.
- Click outside the brush to recentre; double-click to clear.
- Doubles as an at-a-glance health check — quiet weeks vs. spikes are visible
  without opening any picker.

### Trade-offs

| Pros | Cons |
|------|------|
| Filter and data viz in one widget — the filter teaches the user about their own activity | No native GTK widget — needs a custom drawing widget or `gtk::DrawingArea` |
| Direct manipulation feels fast for exploratory questions ("the busy week three weeks ago") | Brush UX is non-standard on GNOME; needs careful keyboard alternative |
| Cross-filter friendly: the histogram itself can refresh under project / assistant filters | Pixel-precise drag on day-level buckets is hard; usually forces week buckets |
| Visually distinctive — would also serve as marketing asset | Permanent vertical real-estate cost above the list |

---

## Variant D — Vertical timeline scrubber (Creative)

Replaces the right-side scrollbar with a **timeline rail**: a stack of
years/months, each with a small sparkbar of activity. Tap a month to focus
that month, shift-tap to extend into a range.

![Variant D — Vertical timeline scrubber](<../mockups/date-filter/D _ Vertical timeline scrubber.png>)

### Behaviour

- Rail is ~90px wide, anchored to the right edge.
- Each month shows: month label + a tiny activity bar whose height encodes
  count.
- Active months get the accent background + a left accent bar.
- Info banner: *"Showing 31 sessions · Mar – May 2026"*.
- The rail also functions as a navigation aid for the session list itself
  (jumping to a month scrolls the list).

### Trade-offs

| Pros | Cons |
|------|------|
| Filter, scrubber, and sparkline collapsed into a single edge — high information density per pixel | Removes the conventional scrollbar — accessibility and discoverability risks |
| Beautiful on long histories — feels like a "spine" of the app | Day-level precision is impossible; resolution is month-only |
| Natural mouse target on the side opposite the sidebar (balanced layout) | Custom widget, custom hit-testing, custom keyboard story |
| Implicit "what month am I looking at?" anchor while scrolling | Year/month bands compete visually with session rows |

---

## Variant E — Heatmap popover picker (Creative)

A GitHub-style contribution-heatmap popover (week columns × 7 day rows)
opened from a header-bar pill. Brushing across cells defines a range; clicking
an empty cell scopes to a single day.

![Variant E — Heatmap popover picker](<../mockups/date-filter/E _ Heatmap popover picker.png>)

### Behaviour

- 30 weeks × 7 days of cells, coloured by activity intensity (5 levels).
- A red lasso outlines the brushed window: *"Apr 5 – May 4"*.
- Bottom row: legend (*less … more*), *Reset*, *Apply*.
- Behaves like Variant B as far as the rest of the UI is concerned (button in
  header, info banner above the list), but the picker carries activity data.

### Trade-offs

| Pros | Cons |
|------|------|
| Combines Variant B's tidy surface placement with Variant C's data viz value | Most widgets of the five — heatmap, brush, legend, buttons |
| Brush-on-cells is more legible than a histogram brush at day-level resolution | Heatmap colour ramp has accessibility implications (contrast, colour-blind users) |
| One picker that works for both single-day and multi-day questions | Visual style departs from GNOME HIG — needs a justification |
| Strong showcase widget — supports the "introspect your work" framing of the app | Implementation cost is the highest of the five |

---

## Comparison matrix

| Aspect | A · Sidebar | B · Header + Calendar | C · Histogram | D · Timeline | E · Heatmap |
|--------|-------------|-----------------------|---------------|--------------|-------------|
| HIG conformance | ✅ High | ✅ High | ⚠️ Custom widget | ⚠️ Replaces scrollbar | ⚠️ Custom widget |
| Surface always visible | ✅ Yes | ❌ Behind a button | ✅ Yes (strip) | ✅ Yes (rail) | ❌ Behind a button |
| Free range support | Via custom dialog | ✅ Native | ✅ Native (drag) | ❌ Month-only | ✅ Native (brush) |
| Day-level precision | ✅ | ✅ | ⚠️ Bucket-dependent | ❌ | ✅ |
| Doubles as data viz | ❌ | ❌ | ✅ | ✅ | ✅ |
| Implementation cost | Low | Medium | High | High | Highest |
| Keyboard story | ✅ Out of the box | ✅ With shortcuts | ⚠️ Needs design | ⚠️ Needs design | ⚠️ Needs design |
| Composes with existing filters | ✅ | ✅ | ✅ | ✅ | ✅ |

## Open questions

- **Multi-selection** — should ranges be unions (e.g. "last week + March"),
  or strictly a single contiguous range like the project filter is single-
  select today?
- **Empty state** — what does the filter show on a fresh install with zero
  sessions indexed? (Presets with zero counts, or hidden until data exists?)
- **Persistence** — should the active range survive across launches, like
  the project filter does?
- **Keyboard shortcut** — `Ctrl+Shift+D` for the date filter, mirroring
  `Ctrl+F` for search?
- **Activity data source** — for C/D/E, do we precompute daily counts at
  index time, or aggregate on demand from the existing session rows?

## Recommendation

Two-step path that minimises risk while leaving the door open to a creative
variant later:

1. **Ship Variant A first.** Lowest implementation cost, reuses the
   sidebar pattern already validated by the project filter, and immediately
   resolves the issue's stated need. Adds *Custom range…* via a small
   `AdwDialog` with two date pickers — no popover infrastructure required.
2. **Layer Variant C (brushable histogram)** as a follow-up once daily
   counts are precomputed (the same data backs analytics work in
   [`2026-03-02-basic-analytics-exploration.md`](2026-03-02-basic-analytics-exploration.md)).
   It elevates the filter into a discovery tool without changing the
   underlying date-range model.

Variant B is the natural fallback if sidebar density becomes a concern before
A ships. Variants D and E are filed as inspiration for a future
"activity dashboard" surface rather than the first cut of #85.

## Decision

_Pending review._
