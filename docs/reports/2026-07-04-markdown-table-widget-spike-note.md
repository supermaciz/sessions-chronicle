# MarkdownTable Widget Spike Note

**Date:** 2026-07-04
**Spec:** `docs/superpowers/specs/2026-07-02-markdown-table-widget-spike-design.md`
**Issue:** #176 follow-up
**PR:** #181

## Result

The spike uses manual placement in a `gtk::Widget` subclass. `MarkdownTable` owns wrapping `gtk::Label` cell children and a horizontal `gtk::Scrollbar`, implements `WidgetImpl::measure` and `WidgetImpl::size_allocate` directly, and measures row heights at the fixed effective column width of 120 px. It behaves as a self-contained internal horizontal scroller: it reports a shrinkable minimum width (one column) with the full fixed-column table width as its natural width, declares `SizeRequestMode::HeightForWidth`, shows its scrollbar only on overflow, and repositions its cells when the scroll adjustment changes.

The spike deliberately does **not** switch the production `render_table` path to `MarkdownTable`; that wiring is a separate follow-up so it can be reviewed with UI screenshots and fixture-driven manual verification.

## Measurement

`cargo test markdown_table::tests -- --nocapture` passes. The height tests assert the honest invariant: the *content* height is width-independent (fixed columns never re-wrap), and an underallocation below the table width adds exactly `SCROLLBAR_HEIGHT` on top of that content height.

Actual measured heights in this environment came in around 3,200-3,300 px of content for the 15-row, 3-column prose-heavy fixture, versus an initial 1,200 px sanity bound assumed in the design. This reflects the local font metrics wrapping the fixture's long prose text into more lines per cell than assumed, not a bug in the widget: the height is exactly the sum of the fixed-column row heights. The explosion-sanity bound was raised to 4,000 px accordingly.

## Review findings and correction

The first-pass implementation passed its tests but the tests were misleading, and PR review (Codex bot, two P2 findings) caught two real defects. Both are fixed:

1. **Non-shrinkable minimum width.** `measure(Horizontal)` originally reported `total_width` as *both* minimum and natural. In GTK4 the minimum is a hard constraint: ancestors must honor it, and GTK clamps the orthogonal (vertical) measurement to it as well. The consequence was that the `allocated_width < total_width` overflow branch never ran in a real transcript, so the internal scrollbar never engaged (the widget behaved like the old full-width `GtkGrid`), and the reserved height omitted `SCROLLBAR_HEIGHT` under a forced underallocation. The symptom was visible during the spike as a `Gtk-WARNING: ... needs at least 384`. Fixed by reporting `COLUMN_MIN_WIDTH` as the minimum and adding `SizeRequestMode::HeightForWidth` so GTK re-measures the height for the width it actually allocates.

2. **No re-allocation on scroll.** The cell `x_offset` is derived from the adjustment value only inside `size_allocate`, but nothing connected `value-changed`, so dragging the scrollbar moved the thumb while the cells stayed put. Fixed by connecting `adjustment::value-changed` to `queue_allocate()` in `constructed`, per the GtkScrollable contract.

A third rendering defect of the same family was found and fixed: children were not clipped to the viewport (`GtkWidget` overflow defaults to visible), so scrolled-away columns painted over surrounding transcript content. Fixed by setting `overflow = Hidden` in the constructor, with a regression test.

**Lesson:** the original height/scroll tests passed only because they called the `measure`/`size_allocate` vfuncs directly, which bypasses GTK's real layout — the min-width clamping, the parent-driven scroll loop, and the paint/clip stage. Unit tests that drive vfuncs in isolation validate layout math but not live widget behavior, which is exactly where all three bugs lived. The suite now includes an underallocation test, a shrinkable-minimum test, a scroll-reposition test (asserting via `compute_bounds` that a cell shifts left when the scroll value increases), and a clip test.

## Known deferral: header separator

The header/data separator space is reserved in the height math (`HEADER_SEPARATOR_HEIGHT`) but not painted in this spike — reserving the space is the correct spike-level behavior (it proves the height accounts for the separator), but no `gtk::Separator` is drawn, unlike the current grid renderer. Drawing it (a themed separator child, or a line in a custom `snapshot()`) is deferred to the `render_table` wiring follow-up, where the choice can be validated against UI screenshots. This is tracked as a P3 review note on PR #181.

## Decision

With the two review findings fixed and covered, the widget now genuinely demonstrates the design's central claim — stable wrapped-cell height plus a conditional internal horizontal scrollbar. This justifies wiring `MarkdownTable` into `render_table` as a follow-up, kept separate from this spike so the production rendering change can be reviewed with UI screenshots and fixture-driven manual verification.
