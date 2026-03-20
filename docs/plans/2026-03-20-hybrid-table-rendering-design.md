# Design: Hybrid Table Rendering with GtkTextChildAnchor

**Date:** 2026-03-20  
**Status:** Approved  
**Problem:** Tables in assistant messages are rendered as fixed-width monospace text
in the `TextBuffer`. Column widths are calculated once at render time and baked into
the text as space-padding. When the window is resized, tables do not reflow — they
overflow or leave excessive whitespace.  
**Solution:** Replace the monospace text rendering of tables with a `GtkTextChildAnchor`
embedding a real `gtk::Grid` inside the `TextView`. The rest of the markdown rendering
(text, headings, lists, code blocks, blockquotes) remains unchanged as `TextBuffer` +
`TextTag`s.

## Scope

This change affects `render_markdown_to_textview()` in `src/ui/markdown.rs` only.
All callers (`transcript_row.rs`, `tool_inspector_pane.rs`, `generic.rs`) benefit
automatically — the public signature is unchanged:

```rust
pub fn render_markdown_to_textview(
    content: &str,
    highlight_query: Option<&str>,
) -> (gtk::TextView, usize)
```

Table clipboard copy is explicitly out of scope — deferred to a separate design.

## Architecture

### Current pipeline (tables only)

```
pulldown-cmark table events → collect headers + rows
  → render_table() → pad with spaces → insert as monospace text in TextBuffer
```

### New pipeline

```
pulldown-cmark table events → collect headers + rows
  → render_table() → build gtk::Grid
  → create TextChildAnchor in TextBuffer
  → wrap Grid in ScrolledWindow
  → attach to anchor via textview.add_child_at_anchor()
```

### What changes

- `MarkdownBufferWriter::render_table()`: replaces monospace text insertion with
  Grid construction and child anchor creation.
- `render_markdown_to_textview()`: after creating the `TextView`, iterates over
  stored anchors to call `add_child_at_anchor()` for each table Grid.

### What stays the same

- All non-table markdown rendering (TextBuffer + TextTags).
- The public API signature.
- Table event collection (`table_headers`, `table_rows`, `table_row`, `inline_buf`).
- Search highlighting pass for non-table content.

## Grid Construction

### Cell layout

Each cell is a `gtk::Label` with:
- `wrap: true`, `wrap_mode: WordChar` — absorbs resize by wrapping text.
- `hexpand: true` — columns share available width equally.
- `xalign: 0.0` — left-aligned text.
- CSS class `.markdown-table-cell` for padding.

### Header row

Same `gtk::Label` setup as data cells, plus:
- `bold` via Pango attribute or CSS class `.markdown-table-header` (already in
  `style.css`).

### Separator

A `gtk::Separator` (horizontal) spans all columns between the header row and the
first data row.
Attached to the Grid at row 1 (header at row 0, data rows starting at row 2).
Spans all columns via `grid.attach(separator, 0, 1, num_cols, 1)`.

### Blockquote context

When a table appears inside a blockquote (`blockquote_depth > 0`), the Grid
receives the CSS class `.markdown-blockquote` to inherit indentation and dimmed
foreground.

## Scroll Behavior

The Grid is wrapped in a `gtk::ScrolledWindow` with:
- `hscrollbar_policy: Automatic` — horizontal scrollbar appears only when the Grid
  exceeds available width (incompressible content like URLs or file paths).
- `vscrollbar_policy: Never` — no vertical scroll; the table takes its natural height.
- `propagate_natural_width: true` — when space is sufficient, no scrollbar appears.

This gives the desired behavior: cell text wraps first, horizontal scroll only when
content cannot compress further.

## Search Highlighting

`TextTag`-based highlighting does not reach widgets inside a `TextChildAnchor`.
Table search highlighting uses **Pango markup** on the cell `gtk::Label`s instead.

### Process

1. During table Grid construction, if `highlight_query` is `Some(query)`:
   - For each cell text, find case-insensitive matches of `query`.
   - Wrap matches in `<span background="#fce94f" foreground="#1e1e1e">…</span>`.
   - Escape non-match text with `pango_escape()`.
   - Set the label with `use_markup: true`.
2. Accumulate the match count from all table cells.
3. Add this count to the total returned by `render_markdown_to_textview()`.

### Theme consistency

The highlight colors (`#fce94f` / `#1e1e1e`) match the existing `search-highlight`
TextTag defined in `create_tag_table()`.

## Anchor Attachment

`TextChildAnchor` must be created during the buffer-writing phase, but
`add_child_at_anchor()` requires the `TextView` to exist. Two-phase approach:

1. **Phase 1 (MarkdownBufferWriter::render_table):** Create the anchor in the
   buffer. Build the `ScrolledWindow` + Grid. Store both as a tuple in a
   `Vec<(gtk::TextChildAnchor, gtk::ScrolledWindow)>` on the writer.
2. **Phase 2 (render_markdown_to_textview):** After creating the `TextView`,
   iterate over stored tuples and call
   `textview.add_child_at_anchor(&scrolled_window, &anchor)` for each.

## CSS Changes

### New class

```css
.markdown-table-cell {
    padding: 4px 8px;
}
```

### Existing classes (reused)

- `.markdown-table` — applied to the Grid container.
- `.markdown-table-header` — applied to header cell labels.
- `.markdown-blockquote` — applied to tables inside blockquotes.

## File Changes

### Modified

- **`src/ui/markdown.rs`** — `MarkdownBufferWriter` gains a `pending_tables` field;
  `render_table()` rewritten; `render_markdown_to_textview()` gains anchor attachment
  loop.
- **`data/resources/style.css`** — Add `.markdown-table-cell` class.

### Unchanged

- `src/ui/transcript_row.rs`
- `src/ui/tool_inspector_pane.rs`
- `src/ui/tool_renderers/generic.rs`
- Data model, parsers, SQLite schema

## Testing

- Existing `render_table` unit tests: update to verify Grid widget is produced
  instead of monospace text.
- New test: verify `TextChildAnchor` is present in buffer after rendering a table.
- New test: verify search highlighting in table cells returns correct match count.
- Integration test with `--sessions-dir tests/fixtures`: verify tables render and
  reflow when window is resized.
