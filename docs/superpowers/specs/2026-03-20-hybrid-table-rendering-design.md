# Design: Hybrid Table Rendering with GtkTextChildAnchor

**Date:** 2026-03-20  
**Status:** Approved with implementation spike  
**Problem:** Tables in assistant messages are rendered as fixed-width monospace text
in the `TextBuffer`. Column widths are calculated once at render time and baked into
the text as space-padding. When the window is resized, tables do not reflow - they
overflow or leave excessive whitespace.  
**Solution:** Replace the monospace text rendering of tables with a `GtkTextChildAnchor`
embedding a real table widget inside the `TextView`. The rest of the markdown rendering
(text, headings, lists, code blocks, blockquotes) remains unchanged as `TextBuffer` +
`TextTag`s.

## Goals

- Tables reflow when the window is resized.
- Cell text wraps before horizontal scrolling appears.
- Horizontal scrolling is reserved for incompressible content such as long URLs or file
  paths.
- The public API stays unchanged.
- Search match counting continues to include table content.

## Non-Goals

- Preserving the current `TextView` selection and copy behavior for table content.
- Implementing table-specific `Ctrl+C`, context-menu copy, or export.
- Reworking non-table markdown rendering.

Table clipboard copy is intentionally deferred to a separate design. This change
accepts a temporary UX regression: table content rendered via anchored widgets will
not participate in native `TextView` selection/copy the same way plain buffer text
does today.

## Scope

### Modified

- `src/ui/markdown.rs` - table rendering moves from monospace text to anchored widgets;
  search match counting is extended to include table cells.
- `data/resources/style.css` - add cell padding styling for rendered table widgets.
- GTK tests in `src/ui/markdown.rs` - update table assertions for anchors/widgets.

### Unchanged

- `src/ui/transcript_row.rs`
- `src/ui/tool_inspector_pane.rs`
- `src/ui/tool_renderers/generic.rs`
- Data model, parsers, SQLite schema

All existing callers continue to use the same public API:

```rust
pub fn render_markdown_to_textview(
    content: &str,
    highlight_query: Option<&str>,
) -> (gtk::TextView, usize)
```

## Architecture

### Current pipeline (tables only)

```text
pulldown-cmark table events -> collect headers + rows
  -> render_table() -> pad with spaces -> insert as monospace text in TextBuffer
```

### New pipeline

```text
pulldown-cmark table events -> collect headers + rows
  -> render_table() -> build table widget
  -> create TextChildAnchor in TextBuffer
  -> store (anchor, widget) for deferred attachment
  -> render_markdown_to_textview() attaches widget via add_child_at_anchor()
```

### What changes

- `MarkdownBufferWriter::render_table()` stops inserting monospace table text.
- `MarkdownBufferWriter` gains a `pending_tables` field storing
  `Vec<(gtk::TextChildAnchor, gtk::Widget)>` or an equivalent concrete widget type.
- `render_markdown_to_textview()` attaches each deferred table widget after creating
  the `TextView`.

### What stays the same

- All non-table markdown rendering remains `TextBuffer` + `TextTag`s.
- Table event collection stays unchanged (`table_headers`, `table_rows`, `table_row`,
  `inline_buf`).
- The public function signature remains unchanged.

## Table Widget Construction

### Container

Each rendered table is built as:

```text
ScrolledWindow
  -> Grid
       -> header labels
       -> horizontal separator
       -> data cell labels
```

The `ScrolledWindow` is the anchored child attached to the `TextView`.

### Grid

The table body uses `gtk::Grid` with:

- `hexpand: true`
- `column_homogeneous: false` (default)
- CSS class `.markdown-table`

`column_homogeneous` is left at its default `false` so that columns size according to
their content. Each cell label sets `hexpand: true`, which distributes extra horizontal
space among all columns proportionally to their natural width. This gives wider columns
to content-heavy cells (e.g. descriptions) and narrower columns to short values (e.g.
status flags), which is the expected behavior for data tables.

### Cells

Each cell is a `gtk::Label` with:

- `wrap: true`
- `wrap_mode: WordChar`
- `xalign: 0.0`
- CSS class `.markdown-table-cell`

Header cells reuse the same base configuration and also receive
`.markdown-table-header`.

### Separator

A `gtk::Separator` spans all columns between the header row and the first data row.

### Blockquote context

If a table is rendered while `blockquote_depth > 0`, the outer table widget receives
the `.markdown-blockquote` CSS class so the embedded widget visually inherits the
blockquote treatment.

## Resize and Scroll Behavior

### Intended behavior

The desired resize behavior is:

1. The table uses the available horizontal space when it can.
2. Cell text wraps as the available width shrinks.
3. Horizontal scrolling appears only when content cannot reasonably compress further.

### ScrolledWindow configuration

The anchored table widget uses a `gtk::ScrolledWindow` with:

- `hscrollbar_policy: Automatic`
- `vscrollbar_policy: Never`
- `propagate_natural_width: true`

This configuration supports the target behavior, but does not by itself guarantee that
anchored widgets will always size exactly as intended inside `GtkTextView`.

### Implementation spike requirement

Embedded widgets attached through `GtkTextChildAnchor` can have non-obvious sizing
behavior inside `GtkTextView`. The implementation must validate that the anchored
`ScrolledWindow` receives enough width information for label wrapping to occur before
horizontal scrolling.

If the natural anchor behavior is insufficient, the implementation may add an internal
width-sync mechanism between the `TextView` allocation and the anchored table widget,
without changing the public API or the overall design.

## Search Highlighting

`TextTag`-based highlighting continues to apply to normal buffer text only. It does not
reach widgets rendered through `TextChildAnchor`.

Table highlighting therefore uses label markup.

### Process

1. During table widget construction, if `highlight_query` is `Some(query)`, each cell is
   scanned for case-insensitive matches.
2. Non-matching text is escaped with `pango_escape()`.
3. Matching text is wrapped in a Pango markup span using the same colors as the existing
   `search-highlight` tag.
4. The number of matches found in all table cells is accumulated and added to the total
   returned by `render_markdown_to_textview()`.

### Color source

The highlight colors used for table-cell markup must come from a single shared source,
not duplicated magic values in separate code paths.

## Anchor Attachment

`TextChildAnchor` must be created while writing into the buffer, but
`add_child_at_anchor()` requires the `TextView` instance.

The rendering therefore stays two-phase:

1. **Phase 1 (`MarkdownBufferWriter::render_table`)**  
   Create the anchor in the buffer, build the table widget, and store both in
   `pending_tables`.
2. **Phase 2 (`render_markdown_to_textview`)**  
   After creating the `TextView`, iterate over `pending_tables` and call
   `textview.add_child_at_anchor(&widget, &anchor)`.

## UX and Regression Notes

### Accepted temporary regression

- Table content is no longer guaranteed to participate in native `TextView` selection,
  `Ctrl+C`, or `select-all` behavior.
- Mixed selection across plain text and table widgets is not guaranteed.

### Behavior that must be preserved

- Search match counts still include table content.
- Link text inside table cells remains readable, including the existing
  `Label (URL)` behavior.
- Tables inside blockquotes still receive the expected visual treatment.

The deferred copy/select behavior should be covered by a future dedicated design doc.

## CSS Changes

### New class

```css
.markdown-table-cell {
    padding: 4px 8px;
}
```

### Existing classes reused

- `.markdown-table`
- `.markdown-table-header`
- `.markdown-blockquote`

## Testing

### Automated

- Update existing table tests so they no longer expect monospace separator text.
- Add a test verifying that rendering a table creates at least one
  `TextChildAnchor`.
- Add a test verifying that the created anchor has an attached widget.
- Add a test verifying that table search highlighting contributes to the returned match
  count.
- Add a test verifying that links inside table cells still render as visible
  `label (url)` text.
- Add a test verifying that tables rendered inside blockquotes receive the expected CSS
  class.

### Manual verification

- Run the app against `tests/fixtures`.
- Resize the window with narrow and wide tables.
- Verify that ordinary prose still renders exactly as before.
- Verify that long URLs or file paths trigger horizontal scrolling only when wrapping is
  no longer enough.

## File Summary

### Modified

- `src/ui/markdown.rs`
- `data/resources/style.css`

### Unchanged

- `src/ui/transcript_row.rs`
- `src/ui/tool_inspector_pane.rs`
- `src/ui/tool_renderers/generic.rs`
- Data model, parsers, SQLite schema
