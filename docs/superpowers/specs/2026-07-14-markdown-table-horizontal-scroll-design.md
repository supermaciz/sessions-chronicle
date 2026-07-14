# Markdown Table Horizontal Scroll Design

**Date:** 2026-07-14  
**Status:** Approved, pending implementation plan

## Goal

Let a wide markdown table in a transcript scroll horizontally with the mouse
wheel and trackpad, without forcing the user to grab the scrollbar thumb.
Supported gestures:

- horizontal two-finger trackpad swipe;
- dedicated horizontal mouse wheel;
- `Shift` + vertical mouse wheel.

The transcript's vertical scroll is never captured: a plain vertical wheel over
a table still scrolls the page.

## Context

The `MarkdownTable` custom widget (wired in `#183`) already owns a horizontal
`gtk::Scrollbar` bound to a `gtk::Adjustment`. Wide tables expose that
scrollbar, but it can only be moved by dragging its thumb. The column
readability design (`2026-07-11`) explicitly deferred wheel handling:

> Do not add `Shift`+mouse-wheel handling; track that as a separate issue.

No `EventControllerScroll` is attached to the widget today. This design adds
one, driving the existing adjustment.

## Non-goals

- No custom momentum/inertia; GTK delivers the deltas as-is.
- No edge overflow affordance (fade/shadow); the on-overflow scrollbar remains
  the affordance.
- No internal vertical scrolling inside the table.
- No change to column width, layout measurement, or markdown parsing.
- No plain (unmodified) vertical wheel capture — that would trap the page
  scroll.

## Architecture

Two additions to the existing custom widget, leaving the layout machinery
untouched:

1. A module-level **pure function** `apply_horizontal_scroll`, alongside
   `total_table_width` / `calculate_layout`, holding all decision logic so it is
   testable without synthesizing real scroll events.
2. A `gtk::EventControllerScroll` attached in `imp::constructed()`, a thin layer
   that reads `dx`/`dy` and the `Shift` modifier and delegates to the pure
   function.

Output flows through the existing `Adjustment`. The existing
`connect_value_changed` handler already calls `queue_allocate`, so no new render
path is introduced: mutating `adjustment.value()` is enough to reposition the
cells. The `size_allocate` clamp remains the authoritative upper bound when the
allocated width changes.

## Pure Function Contract

```rust
/// Apply a scroll delta to the table's horizontal adjustment.
/// Returns `true` if the movement was consumed (the event must not bubble),
/// `false` if it should propagate (vertical page scroll).
fn apply_horizontal_scroll(
    adjustment: &gtk::Adjustment,
    dx: f64,
    dy: f64,
    shift: bool,
) -> bool
```

Rules:

- **Horizontal intent:** `delta = if shift { dy } else { dx }`. This is the
  exact mapping of the three chosen gestures — trackpad / horizontal wheel via
  `dx` (no `Shift`), `Shift` + vertical wheel via `dy`.
- **No overflow** (`upper - lower <= page_size`) → return `false`: nothing to
  scroll, the event propagates.
- **`delta == 0.0`** (purely vertical gesture without `Shift`) → return `false`:
  the page scrolls normally.
- **Otherwise:** `value = (value + delta * step_increment).clamp(lower, upper -
  page_size)`, then `set_value`, return `true`. Consume **even at the edge**
  (value already at max) so a horizontal swipe against the boundary does not
  jerk the page — but only when overflow exists.
- `step_increment` is read from the `Adjustment` (already set to `24.0` in
  `size_allocate`) — single source of truth, no duplicated constant.

Natural sign: positive `dy`/`dx` scrolls right, consistent with the scrollbar.

## Controller Wiring

In `imp::constructed()`:

```rust
let controller = gtk::EventControllerScroll::new(
    gtk::EventControllerScrollFlags::BOTH_AXES,
);
let weak = obj.downgrade();
controller.connect_scroll(move |ctrl, dx, dy| {
    let Some(obj) = weak.upgrade() else { return glib::Propagation::Proceed };
    let shift = ctrl
        .current_event_state()
        .contains(gdk::ModifierType::SHIFT_MASK);
    if apply_horizontal_scroll(&obj.imp().adjustment, dx, dy, shift) {
        glib::Propagation::Stop
    } else {
        glib::Propagation::Proceed
    }
});
obj.add_controller(controller);
```

Default (bubble) phase: the child `GtkLabel`s do not consume scroll, so the
event reaches the widget.

## Behavior To Preserve

- Dragging the scrollbar keeps working unchanged (same `Adjustment`).
- The `size_allocate` clamp (existing lines around 267-270) stays the
  authoritative upper bound on width change; the pure function applies the same
  bound.
- No change to height/measurement, separator, clipping, or search match counts.
- Vertical transcript scrolling over a table stays intact (returns `Proceed`).

## Testing

Unit tests on `apply_horizontal_scroll` (no synthesized events) in
`markdown_table.rs::tests`:

- positive `dx` without `Shift` → `value` increases, returns `true`;
- negative `dx` → `value` decreases, clamped at `lower`, returns `true`;
- `Shift` + `dy` → scrolls horizontally (vertical→horizontal remap), returns
  `true`;
- `dy` without `Shift` → `value` unchanged, returns `false` (propagates to page);
- table without overflow (`page_size >= upper`) → `value` unchanged, returns
  `false`, even with a non-zero `dx`;
- overflow + delta pushing past the max → `value` clamped to `upper -
  page_size`, returns `true` (consumed at the edge);
- delta uses the adjustment's `step_increment` (assert `value` moves by
  `delta * step_increment`).

A light wiring test asserts a `BOTH_AXES` `EventControllerScroll` is attached to
the widget (iterate the widget's controllers).

Run:

```sh
cargo test markdown_table::tests -- --nocapture
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
cargo test --all --no-fail-fast
```

## Manual Verification

On the wide-table session (dev build: `meson install -C builddir` then
`~/.local/bin/sessions-chronicle`):

- horizontal two-finger trackpad swipe → the table scrolls; off-screen columns
  stay clipped;
- `Shift` + wheel → scrolls horizontally;
- plain vertical wheel over the table → **the page** scrolls, not the table;
- at the left/right boundary, a horizontal swipe does not jerk the page;
- the header separator stays pinned, no blank space below the table.

## Decision

Add an `EventControllerScroll` that drives the existing horizontal adjustment
through a pure, testable `apply_horizontal_scroll` function. This is the
smallest change that delivers wheel/trackpad horizontal scrolling while reusing
the custom widget's adjustment, clamp, and queue-allocate machinery. Wrapping
the table in a `GtkScrolledWindow` was rejected because the custom widget exists
precisely to avoid that scroller's height-for-width blank-space problems.