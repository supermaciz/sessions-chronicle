# MarkdownTable Production Wiring Design (#182)

**Date:** 2026-07-09  
**Issue:** [#182](https://github.com/supermaciz/sessions-chronicle/issues/182)  
**Predecessor spike:** [#181](https://github.com/supermaciz/sessions-chronicle/pull/181)  
**Spike note:** `docs/reports/2026-07-04-markdown-table-widget-spike-note.md`  
**Status:** Design approved, pending implementation plan

## Goal

Wire the `MarkdownTable` custom widget into the production markdown table render
path so transcript tables use wrapped cells with stable height and an internal
horizontal scrollbar.

The current production path in `src/ui/markdown.rs::render_table` still builds a
`GtkScrolledWindow -> GtkGrid -> GtkLabel` table with non-wrapping cells. That
keeps the old #149 height workaround, but it means the #181 spike is not visible
in the shipping UI. Issue #182 closes that gap.

## Non-goals

- Do not redesign column sizing beyond the current fixed-width column rule in
  `MarkdownTable`.
- Do not add dense/prose table modes.
- Do not add a vertical table scroller unless fixture-driven manual verification
  proves the large honest table height breaks transcript ergonomics.
- Do not change markdown parsing semantics.

## Architecture

`render_table` becomes an adapter from parsed markdown table state to
`MarkdownTable`:

```text
MarkdownBufferWriter::render_table
  -> MarkdownTable::new(headers, rows, query)
  -> table.match_count()
  -> append MarkdownSegment::Table(table.upcast())
```

The old production renderer removes these responsibilities from
`src/ui/markdown.rs`:

- creating `gtk::Grid`;
- creating per-cell labels directly;
- creating a `gtk::Separator` in the grid;
- wrapping the grid in `gtk::ScrolledWindow`.

`MarkdownTable` remains the owner of table layout and scrolling behavior. It
continues to own wrapping `gtk::Label` children, a horizontal `gtk::Scrollbar`,
and a `gtk::Adjustment`. Its `WidgetImpl::measure` and
`WidgetImpl::size_allocate` keep the height-for-width boundary out of
`GtkScrolledWindow`, preserving the #181 spike result.

## Header Separator

Use a `gtk::Separator` child as the default implementation for the header/data
separator.

Rationale:

- It preserves the themed GTK look of the current grid renderer.
- It avoids hardcoding colors in a custom `snapshot()` implementation.
- It makes the separator inspectable in tests as a real widget.

Implementation shape:

- Add `separator: gtk::Separator` to the `MarkdownTable` subclass state.
- Parent it in `ObjectImpl::constructed` alongside the scrollbar.
- Unparent it in `ObjectImpl::dispose`.
- In `size_allocate`, place the separator immediately after row 0 when
  `row_count > 1`.
- Set the separator visible only when there is at least one body row.

**Reserve the separator's measured natural height, not a hardcoded constant.**
`HEADER_SEPARATOR_HEIGHT = 1` is a spike-era placeholder. A real themed
`gtk::Separator` has a theme-, CSS-, and accessibility-dependent natural height
(often more than one pixel once min-height and margins apply), so reserving a
flat `1` px risks the painted separator overflowing into `ROW_SPACING` or the
first data row. This must follow the existing scrollbar precedent: just as
`measured_scrollbar_height` reads the scrollbar's own natural height, add a
`measured_separator_height` helper that measures the separator vertically and
feed that into `calculate_layout` instead of the constant. Keep
`HEADER_SEPARATOR_HEIGHT` only as the pre-styled fallback (mirroring
`SCROLLBAR_HEIGHT`), not as the authoritative reserved height.

The separator should span the allocated `width` (the viewport), not
`layout.total_width`, and should stay pinned at `x = 0` rather than sharing the
cells' horizontal scroll translation. It is a header/data divider for the
visible viewport, so it should remain a fixed rule under the header while the
cells scroll beneath it, rather than sliding sideways with the content. It stays
clipped by the table widget's existing `gtk::Overflow::Hidden` viewport
behavior.

If the `gtk::Separator` child causes unstable measurement or theming problems in
the real app, fall back to drawing a one-pixel line in `WidgetImpl::snapshot`.
That fallback must first try to use a style-derived border color. If gtk-rs does
not expose a clean color lookup for this widget, the implementation should add a
dedicated CSS class for the drawn separator and document the exact limitation in
the PR notes.

## Data Flow

`MarkdownBufferWriter` already collects table state into `table_headers` and
`table_rows`. Production wiring keeps that model unchanged.

`MarkdownTable::new` receives:

- `headers: &[String]` from `table_headers`;
- `rows: &[Vec<String>]` from `table_rows`;
- `query: &str` from `highlight_query.as_deref().unwrap_or("")`.

`MarkdownTable::new` continues to create all cell labels through
`create_table_label(..., wraps = true)`, aggregate per-cell search counts, and
fill missing cells in short rows with an empty string. `render_table` adds
`table.match_count()` into `self.table_match_count` before appending the widget.

## Behavior To Preserve

- Empty-header tables still return early without appending a table widget.
- Table cells use Pango markup for search highlighting.
- CSS classes stay stable: `markdown-table`, `markdown-table-cell`, and
  `markdown-table-header`.
- The widget remains shrinkable to one fixed column width and exposes the full
  table width as natural width.
- Narrow allocations show the internal horizontal scrollbar and clip scrolled
  columns.
- Search match counts include table header and body cells.

## Watch Points

The spike measured roughly 3,200-3,300 px content height for the 15-row
prose-heavy fixture. That is an honest height for fixed 120 px wrapped columns,
but it must be checked in the running transcript list.

The manual verification should answer whether the outer transcript scroll handles
large tables acceptably. A max-height plus internal vertical scrollbar is not in
scope for the first production wiring pass; it should only be added if the real
app verification shows unusable scroll ergonomics.

## Testing

Update `src/ui/markdown.rs` tests that currently encode the old renderer. The
old path threads `gtk::Grid`, `gtk::GridLayoutChild`, and `gtk::ScrolledWindow`
through both tests and their shared helpers, so the churn is larger than a
handful of assertion flips. Every one of the following must be addressed or the
suite will fail to compile or panic (e.g. `GridLayoutChild` downcasts that
`unwrap()`):

Direct test bodies:

- `table_renders_as_separate_widget` should expect `MarkdownTable` instead of a
  `GtkScrolledWindow`.
- `table_scroller_does_not_expand_vertically` should become a custom-table
  layout assertion, not a `GtkScrolledWindow` assertion.
- `table_cells_do_not_wrap` should invert to require wrapped table cell labels.
- `table_search_count_includes_widget_cells` should remain, and should continue
  to pass through `MarkdownTable::match_count()`.
- `table_grid_positions_correct` cannot simply invert: `MarkdownTable` has no
  `gtk::Grid` and no `GridLayoutChild`, so cell placement is done by hand in
  `size_allocate` via `gsk::Transform`. Rewrite it as a bounds/translation
  assertion (following `markdown_table_repositions_cells_when_scroll_value_changes`
  in `markdown_table.rs`) or delete it in favor of the widget-level layout tests.
- `table_two_columns_has_correct_labels` should keep asserting label text but
  read it through the new widget rather than the grid.
- `table_inside_blockquote_group` and `blockquote_can_group_table_and_code_widgets`
  currently assert `find_widgets_of_type::<gtk::Grid>` inside the quote; retarget
  them to `MarkdownTable`.

Shared helpers (update once, fixes many callers):

- `find_table_widgets` currently downcasts to `gtk::ScrolledWindow`; it must
  find `MarkdownTable` instances instead.
- `collect_grid_positions` is built entirely on `gtk::Grid` +
  `GridLayoutChild::column()/row()`; it has no analog on `MarkdownTable` and
  should be removed with `table_grid_positions_correct`, or replaced with a
  transform-based position collector.
- `table_label_texts` should keep working as long as it walks label children,
  but verify it is not scoped to a `ScrolledWindow`/`Grid` subtree.

Grep for `gtk::Grid`, `GridLayoutChild`, and `ScrolledWindow` in
`src/ui/markdown.rs` tests before implementing to confirm the full inventory;
none of the table-renderer references should survive the wiring. `ScrolledWindow`
references for unrelated widgets, such as code blocks, should remain covered by
their existing tests.

Add or keep `src/ui/markdown_table.rs` tests for:

- separator child existence and visibility when body rows exist;
- no visible separator for a header-only table;
- stable height accounting that includes the measured separator height once
  (assert against `measured_separator_height`, not the `HEADER_SEPARATOR_HEIGHT`
  fallback constant, so a themed separator height cannot silently drift);
- horizontal scroll repositioning still affecting cells, while the separator
  stays pinned at `x = 0` and spans the allocated width rather than scrolling.

Run the existing focused widget tests and full CI-parity checks:

```sh
cargo test markdown_table::tests -- --nocapture
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
cargo test --all --no-fail-fast
```

## Manual Verification

Run the app against fixtures:

```sh
"$HOME/.local/bin/sessions-chronicle" --sessions-dir tests/fixtures
```

Verify in the running UI:

- wrapped cells render at the fixed column width;
- wide tables show the internal horizontal scrollbar only when underallocated;
- the header/data separator is visible and theme-consistent;
- table search hits still contribute to the match counter;
- large prose-heavy tables do not produce blank space below the table;
- transcript scrolling remains usable with the large honest table height.

Capture before/after screenshots for the issue/PR because the previous spike
bugs appeared only in live layout, not in direct vfunc tests.

## Decision

Proceed with the minimal production wiring: replace the old grid/scroller table
path in `render_table` with `MarkdownTable`, add a themed `gtk::Separator` child
inside `MarkdownTable` whose reserved height is measured (not the placeholder
`HEADER_SEPARATOR_HEIGHT` constant) and which stays pinned to the viewport
instead of scrolling with the cells, and update tests — including the
`gtk::Grid`/`GridLayoutChild`/`ScrolledWindow`-based helpers — to encode the new
wrapped-cell contract.
