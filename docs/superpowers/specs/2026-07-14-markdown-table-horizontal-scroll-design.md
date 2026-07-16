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

Three additions to the existing custom widget, leaving the layout machinery
untouched:

1. A module-level helper `apply_horizontal_scroll`, alongside
   `total_table_width` / `calculate_layout`, holding the decision and adjustment
   update logic so it is testable without synthesizing real scroll events.
2. A `gtk::EventControllerScroll` attached in `imp::constructed()`, a thin layer
   that reads `dx`/`dy`, the delta unit, and the `Shift` modifier and delegates
   to the helper.
3. Per-widget gesture state that locks a continuous trackpad sequence to its
   initial horizontal or vertical intent. This prevents small diagonal noise in
   a vertical swipe from trapping the transcript scroll.

Output flows through the existing `Adjustment`. The existing
`connect_value_changed` handler already calls `queue_allocate`, so no new render
path is introduced: mutating `adjustment.value()` is enough to reposition the
cells. The `size_allocate` clamp remains the authoritative upper bound when the
allocated width changes.

## Scroll Helper Contract

```rust
/// Apply a scroll delta to the table's horizontal adjustment.
/// Returns `true` if the movement was consumed (the event must not bubble),
/// `false` if it should propagate (vertical page scroll).
fn apply_horizontal_scroll(
    adjustment: &gtk::Adjustment,
    dx: f64,
    dy: f64,
    shift: bool,
    unit: gdk::ScrollUnit,
    gesture: &mut ScrollGesture,
) -> bool
```

Rules:

- **Explicit `Shift` intent:** use `dy` as the horizontal delta. This follows
  `GtkScrolledWindow`, which swaps the two axes when `Shift` is held.
- **Unmodified intent:** compare the axes before choosing a delta. A gesture is
  horizontal only when `abs(dx) > abs(dy)`; ties favor vertical propagation.
  Therefore the small incidental `dx` commonly present in a vertical trackpad
  swipe does not capture the transcript scroll.
- **Continuous gesture lock:** on `scroll-begin`, start with undecided intent.
  The first non-zero emission classifies the sequence using the rule above and
  keeps that intent through `scroll-end`. A vertical sequence always returns
  `false`; a horizontal sequence keeps using `dx`, even if later emissions are
  slightly more vertical. Events outside a begin/end sequence, such as a
  discrete wheel event, are classified independently.
- **No overflow** (`upper - lower <= page_size`) → return `false`: nothing to
  scroll, the event propagates.
- **No horizontal delta** → return `false`: the page scrolls normally.
- **Delta normalization:** follow `GtkScrolledWindow` as the initial behavior:
  - `gdk::ScrollUnit::Wheel` → multiply the delta by
    `page_size.powf(2.0 / 3.0)`;
  - `gdk::ScrollUnit::Surface` → multiply the surface-pixel delta by `2.5`.
    Keep `2.5` as a named constant and document that it mirrors GTK's current
    `MAGIC_SCROLL_FACTOR`; it is an application behavior choice, not a stable
    public GTK constant.
- **Otherwise:** add the normalized amount and clamp to `[lower, upper -
  page_size]`, then call `set_value` and return `true`. Consume **even at the
  edge** (value already at max) so a horizontal swipe against the boundary does
  not jerk the page — but only when overflow exists.

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
    if apply_horizontal_scroll(
        &obj.imp().adjustment,
        dx,
        dy,
        shift,
        ctrl.unit(),
        &mut obj.imp().scroll_gesture.borrow_mut(),
    ) {
        glib::Propagation::Stop
    } else {
        glib::Propagation::Proceed
    }
});
obj.add_controller(controller);
```

`scroll-begin` resets `scroll_gesture` and marks a continuous sequence active;
`scroll-end` clears it. The exact storage type is an implementation detail, but
the state belongs to each `MarkdownTable`, not to the module globally.

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

- dominant positive `dx` without `Shift` → `value` increases, returns `true`;
- negative `dx` → `value` decreases, clamped at `lower`, returns `true`;
- `Shift` + `dy` → scrolls horizontally (vertical→horizontal remap), returns
  `true`;
- `dy` without `Shift` → `value` unchanged, returns `false` (propagates to page);
- dominant `dy` with a small non-zero `dx` → value unchanged, returns `false`;
- after a continuous gesture is classified vertically, later diagonal deltas
  remain unconsumed through `scroll-end`;
- after a continuous gesture is classified horizontally, later diagonal deltas
  remain horizontal through `scroll-end`;
- table without overflow (`page_size >= upper`) → `value` unchanged, returns
  `false`, even with a non-zero `dx`;
- overflow + delta pushing past the max → `value` clamped to `upper -
  page_size`, returns `true` (consumed at the edge);
- a `Wheel` delta uses `page_size.powf(2.0 / 3.0)`;
- a `Surface` delta uses surface pixels multiplied by `2.5`.

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
- vertical two-finger trackpad swipe, including a slightly diagonal one → the
  page scrolls and the table stays still;
- `Shift` + wheel → scrolls horizontally;
- plain vertical wheel over the table → **the page** scrolls, not the table;
- at the left/right boundary, a horizontal swipe does not jerk the page;
- the header separator stays pinned, no blank space below the table.

## Decision

Add an `EventControllerScroll` that drives the existing horizontal adjustment
through a testable `apply_horizontal_scroll` helper. Normalize wheel and surface
deltas like `GtkScrolledWindow`, and lock continuous gestures to their initial
dominant axis so vertical trackpad scrolling still reaches the transcript. This
keeps the custom widget's adjustment, clamp, and queue-allocate machinery.
Wrapping the table in a `GtkScrolledWindow` was rejected because the custom
widget exists precisely to avoid that scroller's height-for-width blank-space
problems.

## Amendments (2026-07-16)

**Normative.** The design above is left as approved on 2026-07-14. Where an
amendment conflicts with it, the amendment is the current behavior. Later
amendments go below this one under their own date.

### A locked gesture reporting no horizontal delta stays consumed

The **Continuous gesture lock** rule keeps a horizontal sequence horizontal
"even if later emissions are slightly more vertical". The **No horizontal delta**
rule returns `false` unconditionally. A frame in the middle of a locked
horizontal pan reporting `dx == 0.0` with a non-zero `dy` satisfies both: the
lock says consume, the zero-delta rule says propagate.

The first implementation followed the zero-delta rule literally, so that frame
bubbled to the transcript scroller and moved the page vertically mid-pan.
Resolved in favor of the lock: the guard returns the gesture's active state, so a
locked gesture consumes until `scroll-end`, while a discrete wheel event with
`dx == 0.0` still propagates (it is classified `Vertical` and returns one branch
earlier). This matches the **Otherwise** rule's own rationale for consuming at
the edge — a locked gesture must not jerk the page even when the adjustment does
not move.

Read the **No horizontal delta** rule as applying to unlocked classification
only.

### A locked gesture keeps its initial delta source

The modifier state used for the first non-zero emission is part of the gesture
lock. A horizontal gesture started without `Shift` keeps using `dx` through
`scroll-end`; a gesture started with `Shift` keeps using `dy`. Pressing or
releasing `Shift` mid-sequence must not switch the delta source, which would make
the table jump according to incidental movement on the other axis.

Outside a continuous gesture, each event still uses its current modifier state.
In particular, `Shift` with only a non-zero `dx` swaps that movement away from
the table's horizontal axis and propagates, matching `GtkScrolledWindow`.

## Implementation Notes

Background, not rules. Nothing here changes the behavior specified above.

### The gesture lock has no equivalent on GTK's scroll path

Worth knowing for anyone tempted to delete `ScrollGesture` as
not-invented-by-GTK: `GtkScrolledWindow` does no axis locking in
`scroll_controller_scroll`. It applies both axes independently every frame,
because it owns both adjustments and never has to decide whether to propagate.

GTK does have the concept, but only for touch. `GtkGesturePan` is defined as
drags "locked to happen along one axis", and rejects off-axis sequences with
`GTK_EVENT_SEQUENCE_DENIED`. `GtkScrolledWindow` attaches it in
`gtk_scrolled_window_check_attach_pan_gesture()` only when scrolling is
restricted to a single axis — our exact situation — so that nested scrollers win
perpendicular drags.

The lock lives in the `GtkGesture` claimed/denied machinery, which
`GtkEventControllerScroll` does not have. `ScrollGesture` reimplements
`GtkGesturePan`'s semantics on the one path where GTK does not offer them; the
active-state return above is the analogue of a claimed sequence.

The same model is standard on the web, under the name *scroll latching*: WebKit
and Chromium pick the target scroller at gesture begin and do not re-decide
mid-gesture, rubber-banding at the extent instead of chaining to the parent.

- <https://gitlab.gnome.org/GNOME/gtk/-/raw/main/gtk/gtkgesturepan.c>
- <https://gitlab.gnome.org/GNOME/gtk/-/raw/main/gtk/gtkscrolledwindow.c>
- <https://trac.webkit.org/wiki/Scrolling>
