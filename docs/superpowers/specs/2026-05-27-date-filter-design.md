# Date Filter — Design Spec

**Issue:** [#85](https://github.com/supermaciz/sessions-chronicle/issues/85)  
**Date:** 2026-05-27  
**Status:** Approved (design)  
**Exploration:** [`docs/explorations/2026-05-26-date-filter-exploration.md`](../explorations/2026-05-26-date-filter-exploration.md)  
**Variant:** F — Date pill with progressive disclosure

## Goal

Add a structured date filter to session browsing and search, composing with
the existing project, AI-assistant, and search filters via AND.

The variant choice (F) and the wireframes are settled in the exploration
doc. This spec covers the implementation contract.

## Decisions (frozen)

| Topic | Choice |
|---|---|
| Filtered timestamp | `sessions.last_updated` |
| Per-preset counts | Computed on popover open (5 `COUNT(*)`) |
| Persistence across launches | None — resets to *Any time* |
| Preset list | *Any time · Today · Last 7 days · Last 30 days · This year · Custom range…* |
| Custom range picker | Two `GtkCalendar` side by side |
| Keyboard shortcut | `Ctrl+Shift+D` |
| Component placement | New `DatePill` Relm4 Component, emits to App |
| Header placement | `pack_start` on the main `adw::HeaderBar`, after `search_toggle` |
| Visibility | Sessions workspace **and** not in detail view |

Open questions from the exploration that this spec **closes**: date source,
counts, persistence, preset list, keyboard shortcut, range widget,
architecture. The remaining open question from the exploration — header
density at narrow breakpoints — is accepted: the pill is small, sits next
to an already-compact `search_toggle`, and the `AdwViewSwitcher` title
collapses on narrow widths.

## Data model

New module `src/models/date_filter.rs`:

```rust
use chrono::{DateTime, NaiveDate, Utc};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DateFilter {
    #[default]
    AnyTime,
    Today,
    Last7Days,
    Last30Days,
    ThisYear,
    Custom { from: NaiveDate, to: NaiveDate }, // inclusive both ends
}

impl DateFilter {
    /// Resolve the filter into a half-open UTC window `[start, end)`,
    /// anchored on the user's local "now". Returns `None` for `AnyTime`.
    pub fn resolve(&self, now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>)>;

    /// Pill label. Empty when `AnyTime`.
    pub fn pill_label(&self) -> String;

    pub fn is_active(&self) -> bool { !matches!(self, Self::AnyTime) }
}
```

Resolution rules:

- Presets are resolved **at query time**, not at click time — `Today` must
  stay correct across midnight.
- The local timezone anchors the relative presets (`chrono::Local`), then
  the window is converted to UTC for the SQL comparison (`last_updated`
  is stored in UTC).
- `Custom { from, to }` resolves to `[from 00:00 local → (to + 1 day) 00:00 local]`,
  converted to UTC.
- Same-day `Custom { from: d, to: d }` is allowed and means "that single day".

Add `chrono` to `Cargo.toml` if not already present (currently used
transitively — confirm before implementation).

## Component

New file `src/ui/date_pill.rs`, a Relm4 `Component`.

### Model

```rust
pub struct DatePill {
    current: DateFilter,
    draft: DateFilter,
    custom_from: Option<NaiveDate>,
    custom_to: Option<NaiveDate>,
    counts: DateCounts,                  // last computed counts, displayed as badges
    custom_revealed: bool,                // GtkRevealer state
    // widget refs: pill_button, clear_button, popover, listbox,
    //              revealer, calendar_from, calendar_to, apply_button
}

pub enum DatePillInput {
    PopoverOpened,                       // request counts from App
    CountsReceived(DateCounts),
    PresetSelected(DateFilter),          // Any time | Today | Last7 | Last30 | ThisYear
    CustomRangeRowSelected,               // expand the revealer
    CustomFromPicked(NaiveDate),
    CustomToPicked(NaiveDate),
    CustomApplyClicked,
    CustomClearClicked,                   // resets draft inside the popover only
    ClearFromPill,                        // ✕ on the pill → AnyTime, close popover
    OpenViaShortcut,                      // Ctrl+Shift+D
}

pub enum DatePillOutput {
    FilterChanged(DateFilter),
    CountsRequested,
}
```

### Behaviour

- **Preset row click** → `current = preset`, popover closes, emit
  `FilterChanged`. The custom-range revealer collapses.
- **Custom range row click** → revealer expands; `draft` becomes
  `Custom { from, to }` *only on Apply*. Popover stays open.
- **Calendar pickers** update `custom_from` / `custom_to`. Apply button is
  `sensitive` only when both are set and `from ≤ to`.
- **Apply** → `current = Custom { from, to }`, popover closes, emit
  `FilterChanged`.
- **`✕` on the pill** (visible only when `is_active`) → `current = AnyTime`,
  popover stays closed, emit `FilterChanged`. The pill collapses to a
  calendar icon with no label.
- **`Ctrl+Shift+D`** → popover opens, ListBox row corresponding to
  `current` gets focus.
- **Popover open** → emit `CountsRequested`, App computes and sends back
  `CountsReceived`, badges update.

### Pill widget

```
[ 📅 ]                          // AnyTime — icon only
[ 📅 Last 7 days   ✕ ]          // active preset
[ 📅 Apr 5 – Apr 17  ✕ ]        // active custom
```

The pill is a `gtk::MenuButton` whose child is a horizontal `gtk::Box`
containing the icon, an optional label, and an optional `✕` button. The
`✕` is a separate inner `GtkButton` (not the menu button itself) so its
click does not toggle the popover.

### Popover content

```
┌────────────────────────────┐
│ ○ Any time                 │
│ ● Today                · 3 │
│ ○ Last 7 days         · 12 │
│ ○ Last 30 days        · 47 │
│ ○ This year          · 142 │
│ ───────────────────────── │
│ ○ Custom range…            │
│ ┌─ revealer (collapsed) ─┐ │
│ │ [Cal From][Cal To]     │ │
│ │ Apr 5  →  Apr 17       │ │
│ │       [Clear] [Apply]  │ │
│ └────────────────────────┘ │
└────────────────────────────┘
```

Implementation: `GtkPopover` → `GtkBox(Vertical)` containing a `GtkListBox`
(single-select, `selection-mode: browse`) and a `GtkRevealer` whose child
is the calendar pane.

## Database

### Function signatures

In `src/database/mod.rs`, extend the three filter-aware entry points with
a `date_filter` parameter:

```rust
pub fn search_sessions_for_filter(
    db_path: &Path,
    tools: &[AiAssistant],
    project_filter: &ProjectFilter,
    date_filter: &DateFilter,         // new
    query: &str,
) -> Result<Vec<Session>>;

pub fn load_sessions_for_filter(
    db_path: &Path,
    tools: &[AiAssistant],
    project_filter: &ProjectFilter,
    date_filter: &DateFilter,         // new
) -> Result<Vec<Session>>;

pub fn load_session_by_id_for_filter(
    db_path: &Path,
    tools: &[AiAssistant],
    project_filter: &ProjectFilter,
    date_filter: &DateFilter,         // new
    session_id: &str,
) -> Result<Vec<Session>>;
```

### SQL clause

Helper in `src/database/mod.rs`:

```rust
fn date_filter_clause(date_filter: &DateFilter) -> (String, Vec<DateTime<Utc>>) {
    match date_filter.resolve(Utc::now()) {
        None => (String::new(), vec![]),
        Some((start, end)) => (
            " AND last_updated >= ? AND last_updated < ?".to_string(),
            vec![start, end],
        ),
    }
}
```

The clause is concatenated after `project_clause`, before the FTS
sub-query for search. Bindings are pushed in order
`(tools…, project?, date_start?, date_end?, fts?)`.

### Counts

New function:

```rust
pub fn count_sessions_per_date_preset(
    db_path: &Path,
    tools: &[AiAssistant],
    project_filter: &ProjectFilter,
    query: &str,
) -> Result<DateCounts>;

pub struct DateCounts {
    pub any_time: usize,
    pub today: usize,
    pub last_7_days: usize,
    pub last_30_days: usize,
    pub this_year: usize,
}
```

Implementation: 5 separate `COUNT(*)` queries, each applying the existing
`tools / project / query` filters plus the preset's date window. The
counts respect the current search context.

### Index

`src/database/schema.rs` must ensure
`CREATE INDEX IF NOT EXISTS idx_sessions_last_updated ON sessions(last_updated)`
exists. If a similar index already exists for sorting, no change is needed.

## Wiring

### App state

In `src/app/mod.rs`:

```rust
pub struct App {
    …
    date_pill: Controller<DatePill>,
    selected_date_filter: DateFilter,   // default AnyTime, not persisted
}

pub enum AppMsg {
    …
    DateFilterChanged(DateFilter),
    DateCountsRequested,
}
```

On `DateFilterChanged(new)`:

1. `self.selected_date_filter = new`
2. Forward to `SessionList` via a new `SessionListMsg::DateFilterChanged`.
3. `SessionList` re-fetches with all four filters.

On `DateCountsRequested`: App calls
`count_sessions_per_date_preset(db, tools, project, query)` synchronously
and pushes `DatePillInput::CountsReceived` to the pill component.

### SessionList changes

`SessionList` gains a `date_filter: DateFilter` field and a new input
variant `DateFilterChanged(DateFilter)`. All call sites of
`search_sessions_for_filter`, `load_sessions_for_filter`, and
`load_session_by_id_for_filter` receive `&self.date_filter`.

### Visibility

The pill must appear only on the Sessions workspace **and not** in detail
view. The loupe stays visible in detail view; the date pill does not.

Add a field to `WorkspaceHeaderVisibility` (`src/app/types.rs`):

```rust
pub(super) struct WorkspaceHeaderVisibility {
    pub(super) search_ui_visible: bool,
    pub(super) date_filter_visible: bool,   // new
    pub(super) pane_controls_visible: bool,
    pub(super) detail_actions_visible: bool,
    pub(super) indexing_progress_visible: bool,
}
```

Compute in `workspace_header_visibility` (`src/app/helpers.rs`):

- Analytics branch: `date_filter_visible: false`.
- Sessions branch: `date_filter_visible: !detail_visible`.

Expose `is_date_filter_visible(&self) -> bool` on App, next to
`is_search_ui_visible`.

### View placement

In the `view!` macro of `src/app/mod.rs`, right after the `search_toggle`
block (around line 282):

```rust
#[name = "date_pill"]
pack_start = model.date_pill.widget() {
    #[watch]
    set_visible: model.is_date_filter_visible(),
},
```

### Keyboard shortcut

Register an action `win.open-date-filter` with accelerator
`<Primary><Shift>D`. The action's `enabled` state mirrors
`is_date_filter_visible()` so the shortcut is inert in Analytics / detail.
Activating it sends `DatePillInput::OpenViaShortcut`.

## Composition with other filters

Strict AND with the existing dimensions:

```
sessions matching = tools ∩ project ∩ date ∩ query
```

No OR, no multi-range. `DateFilter` stays a single-value enum.

## Edge cases

- **Invalid custom range** (`to < from`): Apply is disabled; an info label
  reads "Pick a start and end date". No toast.
- **Same-day custom range** (`from == to`): allowed; pill shows `"Apr 5"`.
- **Empty database**: counts are all 0; rows remain clickable.
- **Timezone change during the session**: `resolve()` reads `Local::now()`
  at every query — automatic.
- **Reindexing while filter active**: existing `SessionList` re-fetch
  after indexing keeps the active `date_filter` — no special handling.

## Accessibility

- Pill tooltip: `"Filter by date (Ctrl+Shift+D)"` when inactive, `"Date:
  <label>"` when active.
- `✕` button has its own tooltip `"Clear date filter"` and an
  `accessible_label`.
- ListBox is keyboard-navigable (built-in). Enter activates a preset.
- Focus chain after revealer expansion: the two `GtkCalendar` then the
  action buttons. This is the trickiest part of variant F per the
  exploration; verify manually before merge.

## Testing

- **Integration** (`tests/date_filter.rs`): fixture with sessions at known
  dates, verify each preset's result set and inclusive bounds of custom
  range. Inject `now` into `DateFilter::resolve(now)` to keep tests
  deterministic.
- **Unit** on `DateFilter::resolve`: midnight transition, last day of the
  year, one leap-year case.
- **Unit** on `pill_label`: formatting per default locale, same-day case.
- **Composition**: test combining `project + date + query` against a
  fixture, verifying the AND.
- **UI**: manual verification with `--sessions-dir tests/fixtures`,
  screenshots in the PR (existing project process).

## Out of scope (deferred to a follow-up)

- Per-preset count precomputation at index time. The on-open computation
  is the baseline; precompute is a perf optimisation only if needed.
- Multi-range / OR composition.
- Persistence across launches.
- A histogram / heatmap visualisation (tracked in the analytics
  workspace exploration).

## File touch list

New:

- `src/models/date_filter.rs`
- `src/ui/date_pill.rs`
- `tests/date_filter.rs`

Modified:

- `src/models/mod.rs` — export `DateFilter`, `DateCounts`
- `src/ui/mod.rs` — export `DatePill`
- `src/database/mod.rs` — extend three function signatures, add counts function and helper
- `src/database/schema.rs` — ensure `last_updated` index
- `src/ui/session_list.rs` — accept `date_filter`, forward to DB calls
- `src/app/mod.rs` — `DatePill` controller, state, view placement, action registration
- `src/app/types.rs` — `date_filter_visible` field
- `src/app/helpers.rs` — populate `date_filter_visible`
- `src/app/handlers/analytics.rs` — add `is_date_filter_visible`
- `Cargo.toml` — confirm `chrono` is a direct dependency