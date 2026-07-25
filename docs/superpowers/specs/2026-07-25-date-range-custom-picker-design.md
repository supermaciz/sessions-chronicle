# Custom Range Picker — Design Spec

**Date:** 2026-07-25  
**Status:** Ready for implementation  
**Exploration:** [`docs/explorations/2026-07-25-date-range-custom-picker-exploration.md`](../../explorations/2026-07-25-date-range-custom-picker-exploration.md)  
**Proposal:** B's mechanic (one `GtkCalendar` + an `AdwToggleGroup` endpoint switch) in A's container (`GtkStack` page swap)

## Goal

Replace the two side-by-side `GtkCalendar` widgets that pick a custom date range in the `DatePill` popover (`src/ui/date_pill.rs:105`), and make the resulting range readable in the user's own language.

The preset list, the filtered timestamp, the counts, and the `DateFilter` model are unchanged. `DateFilter::Custom { from, to }` keeps its inclusive-both-ends semantics and its SQL resolution untouched.

### Supersedes

`docs/superpowers/specs/2026-05-27-date-filter-design.md:25` freezes the row *"Custom range picker — Two `GtkCalendar` side by side"*. This spec replaces that row. Everything else in that spec stands, including **no persistence across launches** — the custom range resets to *Any time* on restart, so no GSettings key is introduced here.

## Why the two-calendar picker goes

Three defects, all measured rather than estimated (`GtkWidget::measure`, libadwaita 1.9.2, Cantarell 11, under `xvfb`):

1. **The popover is 622 px wide on every open.** `GtkRevealer` collapses only along its transition axis; perpendicular to it, it keeps reporting its child's full minimum width even while `reveal_child` is false. The preset list needs 154 px. The width is paid by every user who opens the pill to click *Last 7 days*.
2. **The drafted range is invisible.** `GtkCalendar` has no range selection, and `mark_day()` takes a day *number*, not a date — marking 3–9 also lights up the 3rd–9th of every other month. 84 day cells communicate strictly less than the one summary label beneath them.
3. **Neither grid is labelled.** Nothing on screen says which one is the start, and nothing at all says it to a screen reader.

## Measurements

| Surface | Size |
|---|---|
| `GtkCalendar` minimum, plain GTK stylesheet | 266 px |
| `GtkCalendar` minimum, **libadwaita stylesheet** | **293 px** |
| Popover today, collapsed | 622 × 296 |
| Popover today, revealed | 622 × 613 |
| Inline reveal, 7 preset rows | 317 × 659 |
| Inline reveal, 11 preset rows | 317 × **815** |
| **Stack page 1 (presets)** | **317 × 452** |
| **Stack page 2 (picker)** | **317 × 387** |

Available height on a maximized window at 1366×768: `768 − 32` (shell top bar) `− 47` (header bar) ≈ **689 px**.

This is what settles the container. An inline reveal fits today by 30 px, which is within the margin that a 125% text scale or a taller shell erases — and it reaches 815 px once the preset rows adopted alongside this work land (*Last 90 days*, *Last 6 months*, one row per year with sessions). The stack peaks at 452 px and is insensitive to the list growing.

Measure `GtkCalendar` only after `Adw.init()`. Without it the number is 27 px short, which is what made two independent reviews disagree by 33 px.

## Decisions (frozen)

| Topic | Choice |
|---|---|
| Container | `GtkStack`, two pages, slide-left-right |
| Picker | One `GtkCalendar` + `AdwToggleGroup` selecting the edited endpoint |
| Endpoint toggle rendering | Two lines — `.caption .dim-label` label over the date |
| Range shading in the grid | **None** |
| Auto-advance *From → To* | **None** |
| Ordering guarantee | Monotonic clamp — `from > to` unreachable |
| Page-2 state | Seeded from the active filter on every entry |
| *Clear* | Empties the draft, stays on page 2 |
| Persistence | None (inherited from the date-filter spec) |
| Date display | `glib::DateTime` + gettext-translatable format strings |

## Widget tree

```
GtkMenuButton (root, .flat)
└─ GtkPopover
   └─ GtkStack   transition-type = slide-left-right
      │          hhomogeneous = true, vhomogeneous = false, interpolate-size = true
      ├─ page "presets"                                    317 × 452
      │  └─ GtkBox v, margins 12
      │     └─ GtkListBox .boxed-list — 7 rows
      │              "Custom range…" gains a › chevron
      └─ page "custom"                                     317 × 387
         └─ GtkBox v, spacing 12, margins 12
            ├─ GtkBox h — GtkButton .flat (‹, go-previous-symbolic) + GtkLabel .heading
            ├─ AdwToggleGroup — 2 × AdwToggle
            │     child = GtkBox v : GtkLabel .caption .dim-label + GtkLabel (date)
            ├─ GtkCalendar
            ├─ GtkLabel (summary) — xalign 0, wrap, AccessibleRole::Status
            └─ GtkBox h, halign End, spacing 6 — Clear, Apply .suggested-action
```

`hhomogeneous = true` forces both pages to the same width, so the popover does not shift horizontally during the slide. `vhomogeneous = false` with `interpolate-size = true` lets the height animate between 452 and 387.

The two-line toggle is not decoration. Measured natural widths:

| Toggle variant | Natural width |
|---|---|
| One line, `From · 3 Jun` | 199 px |
| One line, `From · 28 Dec 2025` | 292 px |
| One line, German, with year | **305 px** — exceeds the calendar |
| Two lines, English | 182 px |
| Two lines, German | 199 px |

A one-line toggle makes the popover width depend on locale and on which dates are picked; in German it becomes the widest child and drives the popover. The two-line form stays under the calendar's 293 px in every locale tested, costs 7 px of height, and uses stock libadwaita classes — no custom CSS.

## State model

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeEndpoint { Start, End }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page { Presets, Custom }

pub struct DatePill {
    current_filter: DateFilter,
    counts: DateCounts,
    draft_from: Option<NaiveDate>,
    draft_to: Option<NaiveDate>,
    active_endpoint: RangeEndpoint,   // new
    page: Page,                       // replaces custom_revealed
    listbox: gtk::ListBox,
    popover: gtk::Popover,
    stack: gtk::Stack,                // new
    calendar_handler: glib::SignalHandlerId,  // new, for the guard
}
```

### Messages

`CustomFromPicked` and `CustomToPicked` collapse into a single `CustomDayPicked`, routed by `active_endpoint`.

| Message | Change |
|---|---|
| `PopoverOpened` | unchanged |
| `CountsReceived(DateCounts)` | unchanged |
| `OpenViaShortcut` | unchanged — always opens on page 1 |
| `PresetSelected(DateFilter)` | unchanged |
| `CustomRangeRowSelected` | now seeds the draft and switches to page 2 |
| `BackToPresets` | **new** |
| `CustomDayPicked(NaiveDate)` | **replaces** `CustomFromPicked` / `CustomToPicked` |
| `CustomEndpointChanged(RangeEndpoint)` | **new** |
| `CustomApplyClicked` | unchanged |
| `CustomClearClicked` | unchanged |

`DatePillOutput`, `DateFilter`, `DateCounts`, and the whole `src/database/` layer are untouched.

### The clamp

Extracted as a pure function so it is testable without a display:

```rust
fn apply_pick(
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    endpoint: RangeEndpoint,
    day: NaiveDate,
) -> (Option<NaiveDate>, Option<NaiveDate>)
```

| Active endpoint | Effect |
|---|---|
| `Start` | `from = day`; if `to` is `Some(t)` and `day > t` then `to = day`; if `to` is `None` then `to = day` |
| `End` | `to = day`; if `from` is `Some(f)` and `day < f` then `from = day`; if `from` is `None` then `from = day` |

**Invariant:** after any pick, both endpoints are `Some` and `from <= to`.

Two consequences follow, and both are part of the contract:

- *Apply* becomes sensitive after the **first** day picked, instead of requiring two.
- The `"Start date must be on or before end date"` branch of `custom_info_text` becomes unreachable and is **deleted**, along with the out-of-order assertions in `valid_custom_filter_requires_both_dates_in_order` (`src/ui/date_pill.rs:495`). `valid_custom_filter` itself stays as the boundary check between draft and `DateFilter`.

### Seeding and reset

On every entry to page 2 (`CustomRangeRowSelected`):

- if `current_filter` is `Custom { from, to }` → `draft_from = Some(from)`, `draft_to = Some(to)`, calendar shows `from`'s month;
- otherwise → both drafts `None`, calendar shows the current month;
- `active_endpoint = Start` in both cases.

On popover close, only `page` resets to `Presets`. The drafts need no cleanup because entry to page 2 always overwrites them — that is what makes stale draft state structurally impossible rather than merely unlikely. This is a behaviour change: today a draft survives closing the popover and even survives applying a different preset, so it can contradict the pill label.

### Signal guard

Seeding and endpoint switching both set the calendar's date programmatically, which re-emits `day-selected` and would write the value straight back into the endpoint just left. Store the handler id at connect time and wrap programmatic writes in `block_signal` / `unblock_signal`. A stored `SignalHandlerId` is preferred over a shared `Cell<bool>`: it scopes the suppression to exactly one signal on one widget.

## Interaction

| Gesture | Effect |
|---|---|
| Click *Custom range…* | slide to page 2, focus the calendar |
| Click a toggle | change the edited endpoint; the calendar jumps to that endpoint's date |
| Click a day | write to the active endpoint, apply the clamp |
| Click `‹` | return to page 1; the draft stays in memory |
| *Clear* | empty both endpoints, stay on page 2, *Apply* goes insensitive |
| *Apply* | emit `FilterChanged`, close the popover, reset `page` to `Presets` |

**No auto-advance from *From* to *To*.** `GtkCalendar`'s only per-day signal is `day-selected` and it fires on every arrow keypress, so auto-advance would flip the active endpoint mid-navigation for keyboard users. Making it safe needs a custom key controller over the grid, which is not worth saving pointer users one click.

## Keyboard

`Ctrl+Shift+D` opens the popover on page 1 with the active preset row focused, exactly as today (`focus_current_row_when_ready`).

Page 2 tab order: back button → toggle group → calendar → *Clear* → *Apply*. `←→` switches endpoint inside `AdwToggleGroup` (native). `PgUp` / `PgDn` change month.

**Escape must have two successive meanings** — return to page 1, then close the popover. This requires a `GtkShortcutController` on the page-2 root with `propagation_phase = Capture` and a handler returning `Propagation::Stop`, so it runs before `GtkPopover`'s autohide. Without the capture phase the popover closes on the first Escape and the draft is lost silently. This is the one part of the design that can fail quietly, so it gets its own test.

**A `GtkStack` page change does not move focus.** Both directions need an explicit `grab_focus()` — the calendar on entering page 2, the *Custom range…* row on returning — or focus stays on a now-invisible widget and keyboard navigation dies.

## Accessibility

- The calendar's accessible label follows the active endpoint — *"Start date"* / *"End date"* — via `update_property(Property::Label, …)`, translated. This is what fixes the unlabelled-grid defect.
- Each `AdwToggle` gets an **explicit** accessible label recomposing its two visual lines: *"Start date, 28 December 2025"*. Without it a screen reader announces two disconnected labels.
- The summary label is given `AccessibleRole::Status`, GTK 4's equivalent of `aria-live="polite"`, so every pick is announced as a resolved range rather than a bare date. There is no live-region *property* in `gtk::AccessibleProperty`; the role is the mechanism.
- The back button is icon-only and therefore requires an accessible label.
- The active toggle is identified by libadwaita's own toggle styling, not a custom colour, so it survives high contrast.

## Date display

### Where the formatting lives

`src/models/` is entirely GTK-free today — no file in it imports `gtk`, `glib`, or `relm4`. `DateFilter::pill_label()` sits in `src/models/date_filter.rs` but is consumed only by `src/ui/date_pill.rs`; every other reference is one of its own tests. It is display code already on the wrong side of that boundary, and localizing it in place would require importing both `glib` and `gettext` into the model layer.

`pill_label` therefore **moves** to `src/ui/date_pill.rs` as a free function:

```rust
fn filter_label(filter: &DateFilter, today: NaiveDate) -> String
```

`DateFilter` keeps `resolve()` and `is_active()` — pure domain, no toolkit dependency. Its tests that assert on label text move with the function. `src/models/date_filter.rs` is consequently **not** added to `po/POTFILES.in`; only `src/ui/date_pill.rs` is.

### The two defects being fixed

`format_date` currently calls `date.format("%b %-d")` through `chrono`, which is compiled without `unstable-locales`. That emits **English month abbreviations regardless of locale**, in an application that otherwise binds gettext in `src/main.rs:77`. It also omits the year, so a range spanning a year boundary renders as `Dec 28 - Jan 4` — ambiguous about which December, and identical for 2024→2025 and 2025→2026.

Display formatting moves to `glib::DateTime`, which formats through the locale already installed by `setlocale(LC_ALL, "")`. `chrono` remains the source of truth for `NaiveDate` and all range logic; only the display path changes.

GLib gives localized month **names** but not their **order** — `%b %-d` still yields "Jun 3" in French, and `%x` would force numeric dates with a mandatory year. The idiomatic GNOME answer is to make the format string itself translatable:

```rust
// Translators: strftime format for a date without a year, e.g. "Jun 3".
// Reorder for your locale — French uses "%-d %b" to produce "3 juin".
gettext("%b %-d")
```

Two translatable format strings, one without a year and one with. Selection rule, as a pure function returning an enum so it is testable independently of the runner's locale:

- both endpoints in the current year → the no-year format for both;
- otherwise → the with-year format for **both** endpoints, never mixed;
- `from == to` → a single date, preserving today's behaviour.

### Translatability, and an inconsistency to close while we are here

`src/ui/date_pill.rs` is not listed in `po/POTFILES.in` and contains **no** `gettext` calls at all — every preset label, button label, and tooltip is hardcoded English. The sibling `src/ui/sort_pill.rs`, written more recently, wraps all of its strings and is listed. The two pills in the same header bar follow different rules.

This spec therefore requires:

- `src/ui/date_pill.rs` added to `po/POTFILES.in`;
- every string it introduces wrapped in `gettext` — the back button's accessible label, the *"Custom range"* heading, the *"Start date"* / *"End date"* endpoint labels, and the two date format strings;
- the **existing** strings in `date_pill.rs` wrapped as well.

The last item is adjacent scope, taken deliberately: the widget construction in `init` is being rewritten anyway, the change is mechanical, and shipping new translatable strings into a file whose existing strings are not translatable would leave the popover half-localized in a way that is harder to notice later. `po/LINGUAS` is currently empty, so nothing needs retranslating — this is groundwork, not churn.

## Out of scope

- The additive preset rows (*Last 90 days*, *Last 6 months*, one row per year with sessions). Adopted by the same exploration, but they change the `DateFilter` enum, `DateCounts`, and the counting SQL, and they make the row count dynamic — which affects `current_row_index`. They get their own spec. **This spec's stack container is what makes them affordable**, since it removes the height ceiling.
- Range shading in the grid. Rejected; the acceptance test if it is ever revisited is in the exploration.
- Activity brush (Proposal C) and deleting the picker (Proposal D's subtractive half).

## Testing

Pure functions, no display required:

- `apply_pick` — table-driven: empty other endpoint, inversion on `Start`, inversion on `End`, same day, idempotent re-pick.
- The year-format selection rule — same-year range, cross-year range, range in a past year, `from == to`.
- `valid_custom_filter` — retained, minus the now-unreachable ordering assertions.

Under `#[gtk::test]`:

- the stack exposes two pages, and `CustomRangeRowSelected` switches to `"custom"`;
- entering page 2 seeds the draft from an applied `Custom` filter, and leaves it empty for a preset filter;
- `CustomEndpointChanged` moves the calendar to that endpoint's date **without** re-emitting `CustomDayPicked` — this is the signal-guard test;
- *Apply* is sensitive after a single pick;
- Escape on page 2 returns to page 1 without closing the popover.

The two existing tests that walk the tree for a `GtkRevealer` (`find_revealer`, `custom_range_activation_keeps_custom_row_selected`) become `GtkStack` lookups.

## Verification before PR

Run with `--sessions-dir tests/fixtures`, then confirm:

- the popover width does not change between page 1 and page 2;
- picking a start after the current end pushes the end rather than showing an error;
- one pick makes *Apply* sensitive immediately;
- the whole picker is reachable with `Tab` and arrows only;
- Escape returns to page 1, a second Escape closes;
- 200% text scale and high contrast;
- the narrow breakpoint;
- expanded height at 768 px.

Plus the Definition of Done from `AGENTS.md`: `cargo fmt --all -- --check`, `cargo clippy --all -- -D warnings`, `cargo test --all --no-fail-fast`, and updated screenshots for the UI change.
