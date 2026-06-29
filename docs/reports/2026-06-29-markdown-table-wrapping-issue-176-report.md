# Markdown Table Wrapping Investigation (#176)

**Date:** 2026-06-29  
**Issue:** [#176](https://github.com/supermaciz/sessions-chronicle/issues/176)  
**Status:** Investigation complete  

## Question

Can markdown table cells be wrapped again now that the transcript uses `GtkListView`,
without regressing the excess vertical space fixed by #149?

## Current Renderer

Assistant markdown tables are rendered in `src/ui/markdown.rs` as:

```text
GtkScrolledWindow
  -> GtkViewport (inserted by GtkScrolledWindow for non-Scrollable children)
    -> GtkGrid
      -> GtkLabel cells
```

The production renderer keeps table cell labels non-wrapping:

- `GtkLabel::set_wrap(false)`
- `GtkScrolledWindow::set_propagate_natural_height(true)`
- horizontal scrollbar policy `Automatic`
- vertical scrollbar policy `Never`

That matches #149: table height stays independent from available width, and wide
content scrolls horizontally.

## Documentation Notes

The GTK 4.23.1 `GtkScrolledWindow` documentation says `GtkScrolledWindow` wraps
non-`GtkScrollable` children in `GtkViewport`, exposes horizontal and vertical
adjustments, and can use external adjustments or custom scrolling when its layout
does not fit the application. The same page also documents
`propagate-natural-height` as the property that calculates and propagates the
child's natural height through the scrolled window.

Source: <https://docs.gtk.org/gtk4/class.ScrolledWindow.html>

## Reproduction

Added a GTK unit reproduction in `src/ui/markdown.rs`:

- `measured_table_scroller(wrap_cells: bool)` builds a local
  `GtkScrolledWindow -> GtkGrid -> GtkLabel` table with 15 rows and prose-heavy
  cells.
- `table_scroller_wrapped_cells_still_overrequest_height` measures vertical
  natural height at a transcript-like width of 360 px.

Command:

```sh
cargo test table_scroller_wrapped_cells_still_overrequest_height -- --nocapture
```

Result on 2026-06-29:

| Variant | Natural height |
| --- | ---: |
| Wrapped cells | 3278 px |
| Non-wrapped cells | 353 px |

This is a 9.3x height request for the wrapped variant. The reproduction measures
the table widget (`GtkScrolledWindow -> GtkGrid -> GtkLabel`) in isolation, not
through the full `GtkListView` row chain. By inference, the current `GtkListView`
transcript implementation does not remove the #149 failure mode: the over-request
originates in the table widget's own height-for-width measurement, which the
surrounding `GtkListView` row cannot correct.

## Findings

### Re-enabling wrapping still breaks layout

Yes. The isolated reproduction keeps the current renderer's outer table shape. The
wrapped variant changes label wrapping plus the width constraints needed to force
the wrap (`width_chars`/`max_width_chars`), which the production non-wrapping cells
do not set. Even so, wrapped labels still cause the table `ScrolledWindow` to
request far more natural height than the non-wrapping renderer.

### The issue is still `GtkScrolledWindow` natural-height propagation

The failure appears when `GtkScrolledWindow` propagates the natural height of a
non-scrollable child whose height depends on width. GTK inserts a `GtkViewport`
for the `GtkGrid`, then the wrapped labels make the grid's natural height vary
with the width chosen during measurement.

This matches the #149 diagnosis and the current documentation for
`propagate-natural-height`.

### Custom horizontal scrolling is plausible but not a small renderer tweak

A custom table widget could avoid asking `GtkScrolledWindow` to propagate child
natural height. The likely shape is:

```text
GtkBox vertical
  -> clipped table viewport
       -> GtkGrid translated by hadjustment.value
  -> GtkScrollbar horizontal, bound to the same GtkAdjustment
```

That approach needs a custom layout/measurement boundary that:

- measures the wrapped grid against the actual allocated viewport width;
- clips horizontal overflow;
- keeps row height tied to the final allocation, not to the grid's unconstrained
  natural-width request;
- updates the horizontal adjustment upper/page size after allocation;
- preserves search highlighting and table CSS.

This is feasible as a follow-up spike, but it should be treated as a dedicated
widget implementation, not a one-line change to `GtkScrolledWindow`.

### Two table modes should wait

A mode split is probably useful eventually:

- dense/technical tables: current non-wrapping horizontal scroll;
- prose-heavy tables: wrapping custom table widget.

However, adding a mode now would expose a broken wrapped path unless the custom
measurement boundary exists first.

## Recommendation

Keep production table cells non-wrapping for now. This preserves the #149 fix.

For a future implementation, build a dedicated `MarkdownTable` widget or helper
container that owns horizontal scrolling without `GtkScrolledWindow` natural-height
propagation. Only after that spike demonstrates stable measurement should the app
consider automatic or user-visible wrapping modes.

## Verification

```sh
cargo test table_scroller_wrapped_cells_still_overrequest_height -- --nocapture
```

Passed. If the assertion fails, its failure message reports the wrapped and
non-wrapped natural heights for repeatable comparison.
