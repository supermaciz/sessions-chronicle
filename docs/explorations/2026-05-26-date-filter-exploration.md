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
approach. Five wireframes were sketched initially (two GNOME-HIG, three
experimental), and a sixth variant (F) was added after a Mii Beta design
review as a refinement of B. All wireframes share the same lo-fi style
(black/white sketch, a single blue accent for the active selection, a single
warm-red for annotations).

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

## Variant F — Date pill with progressive disclosure (GNOME HIG, refined)

A response to the Mii Beta critique of A→C: B is the right surface, but the
calendar should not be loaded by default. The popover opens on a **preset
list** (the 90% path), and the calendar only appears when the user picks
*Custom range* — revealed inline by a `GtkRevealer`, not in a second surface.

![Variant F — Date pill, progressive disclosure](<../mockups/date-filter/F _ Date pill progressive disclosure.svg>)

### Behaviour

- Header-bar pill, left of the search entry. Label reflects the active range
  (*"Last 7 days"*, *"Apr 5 – Apr 17"*) or collapses to the calendar icon
  alone when *Any time* is active — minimal idle cost on the header.
- Popover opens with a single vertical `ListBox`: *Any time · Today · Last 7
  days · Last 30 days · This year*, with **session counts per preset** as
  trailing badges (precomputed at index time, not on popover open).
- A divider separates the relative presets from a single *Custom range…* row.
- Selecting *Custom range…* expands the popover in place via `GtkRevealer`,
  exposing an `AdwCalendar`/`GtkCalendar`, a *From / To* summary, and
  *Clear* / *Apply* actions. The list stays visible above — focus chain stays
  inside the same popover.
- When a range is active, the pill carries an inline ✕ that clears the
  filter **without reopening** the popover — the most frequent gesture after
  filtering gets the shortest path.
- **No info banner above the session list.** The pill itself is the state
  surface. One filter, one place to read it from, one place to clear it.

### Trade-offs

| Pros | Cons |
|------|------|
| Single surface for both the 90% (presets) and 10% (custom range) paths | Adds a new widget to the header bar (same concern as B) |
| Calendar grid only rendered when needed — popover stays small on first open | `GtkRevealer` repositioning needs care to avoid jumpy animations |
| Per-preset session counts give quick stats without a viz strip | Counts must be maintained as the index updates — extra background work |
| Clear-from-pill removes the most frequent round-trip through the popover | Pill is a fourth element competing for header attention next to search |
| Pure native widgets — no `DrawingArea`, no custom hit-testing, no a11y bespoke story | Slightly more wiring than A (revealer + range model + pill state) |

### What makes F different from B

B mixes three zones in one popover (chip row, full calendar grid, action
bar). F is a single vertical list that grows on demand. B also keeps a
banner above the list to communicate state; F dissolves that into the pill.
B treats presets and free range as siblings; F treats free range as a
specialisation of the preset list, which matches the actual usage
distribution.

---

## Comparison matrix

| Aspect | A · Sidebar | B · Header + Calendar | C · Histogram | D · Timeline | E · Heatmap | F · Pill + disclosure |
|--------|-------------|-----------------------|---------------|--------------|-------------|------------------------|
| HIG conformance | ✅ High | ✅ High | ⚠️ Custom widget | ⚠️ Replaces scrollbar | ⚠️ Custom widget | ✅ High |
| Surface always visible | ✅ Yes | ❌ Behind a button | ✅ Yes (strip) | ✅ Yes (rail) | ❌ Behind a button | ❌ Behind a pill |
| Free range support | Via custom dialog | ✅ Native | ✅ Native (drag) | ❌ Month-only | ✅ Native (brush) | ✅ Native (inline reveal) |
| Day-level precision | ✅ | ✅ | ⚠️ Bucket-dependent | ❌ | ✅ | ✅ |
| Doubles as data viz | ❌ | ❌ | ✅ | ✅ | ✅ | ⚠️ Per-preset counts only |
| Implementation cost | Low | Medium | High | High | Highest | Low-Medium |
| Keyboard story | ✅ Out of the box | ✅ With shortcuts | ⚠️ Needs design | ⚠️ Needs design | ⚠️ Needs design | ✅ Out of the box |
| Composes with existing filters | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Number of surfaces for full feature | 2 (sidebar + dialog) | 1 (popover) | 1 (strip) | 1 (rail) | 1 (popover) | 1 (popover, progressive) |

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

**Primary: Variant F.** **HIG-safe alternative: Variant A.**

Two design reviews (Mii Beta GTK Designer, UI Designer) converged on F and A
as the only serious candidates; C, D, E are filed as inspiration for the
analytics workspace tracked in
[`2026-03-02-basic-analytics-exploration.md`](2026-03-02-basic-analytics-exploration.md),
not for #85.

### Why F is the primary

- **Single surface** for both usage modes: relative presets (the dominant
  case) and free range (the long tail). The calendar grid is only loaded
  when the user picks *Custom range…* — honest about how the feature is
  actually used.
- Preserves the **per-preset counts** that made A attractive, and the
  **native calendar interaction** that made B attractive, without paying
  for C/D/E's custom widgets or accessibility debt.
- **Clear-from-pill** removes the most frequent round-trip through the
  popover.
- The A→C path the earlier draft recommended multiplied surfaces — sidebar
  section + custom-range dialog + histogram strip — yielding three places
  to talk about dates in the same view. F collapses that to one.

### Why A is the HIG-safe alternative

Variant F introduces three behaviours that are not in the confort zone of
GNOME's native popover model — popover that resizes after open, focus
chain that must be reprogrammed after `GtkRevealer` reveal, and header-bar
density pressure at the narrow breakpoint. None of these are blocking, but
together they push F's real implementation cost into **medium** territory
(QA visual + a11y, not lines of code). If that cost is unacceptable, or if
header-bar density is the deal-breaker, fall back to A.

When falling back to A, apply the UI Designer's refinements rather than
the original A spec:

1. **Custom range opens a popover anchored on the row, not a modal dialog.**
   Activating the *Custom range…* `AdwActionRow` should `popup()` a
   `GtkPopover` containing an `AdwCalendar` and *From / To* fields, anchored
   on the row itself. This keeps the interaction in the sidebar's spatial
   context — no modal context switch — while staying inside a native
   pattern (`AdwActionRow` + ancillary popover, used elsewhere in
   libadwaita-based apps).
2. **Keep the per-preset counts as trailing badges** on each `AdwActionRow`
   (already in the A spec), to match the analytics workspace styling.
3. **No info banner above the session list.** Borrow F's idea: the active
   row in the sidebar is the state surface. One filter, one place to read
   it from. This avoids the third "place to talk about dates" the earlier
   draft introduced.
4. **`Ctrl+Shift+D`** focuses the Date section's first preset row,
   mirroring how `Ctrl+F` focuses the search entry.

### Open decisions before implementation

The five open questions above still apply to both F and A. The two most
load-bearing for the variant choice are:

- **Header density** at narrow breakpoint — if the search entry is already
  fighting for space, F's pill compounds the problem and A becomes
  preferable.
- **Per-preset counts** — whether we want to precompute daily counts at
  index time. Both F and the refined A rely on these; if precompute is
  out of scope for #85, both lose a chunk of their value and B becomes
  the simpler path.

## Decision

_Pending review._
