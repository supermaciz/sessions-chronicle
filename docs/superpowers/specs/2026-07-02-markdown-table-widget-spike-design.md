# MarkdownTable Widget Spike Design (#176 follow-up)

**Date:** 2026-07-02  
**Issue:** [#176](https://github.com/supermaciz/sessions-chronicle/issues/176) (investigation closed)  
**Predecessor report:** `docs/reports/2026-06-29-markdown-table-wrapping-issue-176-report.md`  
**Status:** Implemented [#181](https://github.com/supermaciz/sessions-chronicle/pull/181)  

## Goal

Prove that a custom widget can render **wrapped table cells** with a **stable row
height** (no #149-style blank space) and a **conditional horizontal scrollbar**,
replacing the current
`GtkScrolledWindow(propagate_natural_height) -> GtkGrid -> GtkLabel(wrap=false)`
renderer (`src/ui/markdown.rs:821-829`).

The #176 investigation concluded that the excess-height failure originates in
`GtkScrolledWindow` propagating the natural height of a non-scrollable child whose
height depends on width. The fix is a widget that owns its own height-for-width
measurement instead of delegating to `GtkScrolledWindow`.

## Non-goals (YAGNI for this spike)

- Intelligent width distribution across columns (spare horizontal room shared
  proportionally) — this is the "option 2" refinement, deferred.
- Configurable dense/prose table modes.
- Final wiring into `render_table`. The spike may live alongside the current
  renderer; we only switch the production path once measurements are proven.

## Architecture

A single custom widget, `MarkdownTable`, implemented as a `glib` subclass of
`GtkWidget` with a custom layout (custom `LayoutManager`, or GTK's
`measure(orientation, for_size)` / `size_allocate(width, height, baseline)`
hooks implemented directly on the widget). It owns two kinds of children:

```
MarkdownTable (GtkWidget subclass)
  ├─ cell grid : GtkLabel (wrap=true, width bounded to its column min width)
  └─ GtkScrollbar (horizontal, visible only on overflow)
```

The widget places its cell labels itself in columns (rather than delegating to an
internal `GtkGrid`), because controlling measurement requires owning placement.

**Spike escape hatch:** if manual placement of labels proves too heavy, an
acceptable fallback is an internal `GtkGrid` child translated by
`-hadjustment.value` and clipped by the widget. Manual placement is preferred;
the fallback is documented so the spike is not blocked on this detail.

## Layout rule (option 3: fixed minimum column width)

- Each column has a **fixed minimum width** (constant, e.g.
  `COLUMN_MIN_WIDTH = 120px`, tunable during the spike).
- `measure(orientation, for_size)`:
  - total required width = `Σ column_width + column_spacing`.
  - each cell wraps to its column width, so its height-for-width is computed at
    the **effective table column width** chosen by this layout rule, not at an
    unconstrained natural width. For option 3, that effective width is the fixed
    column minimum.
  - each row's height = max of its cells' heights.
  - the widget's reported height is the sum of row heights (plus header separator
    and row spacing) — independent of the widget's allocated width beyond the
    expected wrap.
- `size_allocate(width, height, baseline)`:
  - if `width >= total_width` → no scrollbar; columns keep their minimum width.
    Any surplus width is left unused on the right (proportional distribution is
    deferred to option 2).
  - if `width < total_width` → scrollbar visible; columns are drawn translated by
    `-hadjustment.value` and **clipped** to the allocated width (for example via
    GTK 4 widget overflow clipping).

## Horizontal scrolling

- A `GtkAdjustment` with `upper = total_width` and `page_size = allocated_width`,
  updated after every `size_allocate`.
- A `GtkScrollbar` bound to that adjustment, visible only when
  `upper > page_size`.

## Behavior to preserve

- **Search highlighting:** cells stay `GtkLabel`s with Pango markup, reusing the
  existing `create_table_label` path (`src/ui/markdown.rs:745`). Unchanged.
- **CSS:** the classes `markdown-table`, `markdown-table-cell`, and
  `markdown-table-header` are kept.
- **Header separator** is kept.

## Validation (spike exit criteria)

Adapt the existing reproduction test
`table_scroller_wrapped_cells_still_overrequest_height` (which measured 3278 px
wrapped vs 353 px non-wrapped at 360 px width) into a new test that:

1. builds the `MarkdownTable` widget with the same 15-row prose-heavy fixture;
2. **fails if** the widget's natural height at a transcript-like width (360 px)
   explodes the way the old wrapped variant did;
3. asserts the reported height is approximately the sum of the row heights
   produced by the fixed effective column width, and stays **stable** when
   re-measured at different transcript-like widths;
4. confirms no residual blank space below the table.

Command:

```sh
cargo test <new_test_name> -- --nocapture
```

The assertion message must report the measured heights for repeatable comparison,
matching the style of the existing reproduction test.

## Definition of done for the spike

- `MarkdownTable` widget compiles and passes the new measurement test.
- `cargo fmt --all -- --check`, `cargo clippy --all -- -D warnings`, and
  `cargo test --all --no-fail-fast` pass.
- A short note records whether manual placement or the internal-grid fallback was
  used, and whether the measurements justify wiring the widget into
  `render_table` as a follow-up.
