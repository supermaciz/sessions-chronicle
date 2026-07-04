# MarkdownTable Widget Spike Note

**Date:** 2026-07-04
**Spec:** `docs/superpowers/specs/2026-07-02-markdown-table-widget-spike-design.md`
**Issue:** #176 follow-up

## Result

The spike uses manual placement in a `gtk::Widget` subclass. `MarkdownTable` owns wrapping `gtk::Label` cell children and a horizontal `gtk::Scrollbar`, implements `WidgetImpl::measure` and `WidgetImpl::size_allocate` directly, and measures row heights at the fixed effective column width of 120 px.

## Measurement

`cargo test table_widget_wrapped_cells_keep_stable_height -- --nocapture` passes. The test asserts that the 15-row prose-heavy fixture reports the exact fixed-column row-sum height, stays stable at transcript-like widths, and remains below an explosion threshold.

While implementing, actual measured heights in this environment came in around 3,200-3,300 px for the 15-row, 3-column prose-heavy fixture (versus an initial 1,200 px sanity bound assumed in the design). This reflects the local font metrics wrapping the fixture's long prose text into more lines per cell than assumed, not a bug in the widget: the height is exactly the sum of the fixed-column row heights, and it stays perfectly stable across 360/420/720 px query widths, which is the actual exit criterion. The explosion-sanity bound was raised to 4,000 px accordingly.

GTK also clamps any `measure(Vertical, for_size)` request below the widget's own reported horizontal minimum (the fixed total column width) up to that minimum before invoking our callback, confirmed by a `Gtk-WARNING` during test runs. Tests that independently recompute an "expected" layout for comparison account for this clamping.

## Decision

The measurements justify wiring `MarkdownTable` into `render_table` as a follow-up. Keep that follow-up separate from this spike so production rendering changes can be reviewed with UI screenshots and fixture-driven manual verification.
