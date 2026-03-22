# Design: Code Block Widget with Segment-Based Rendering

**Date:** 2026-03-22
**Status:** Approved
**Issue:** [#73](https://github.com/supermaciz/sessions-chronicle/issues/73)
**Problem:** Blank lines inside fenced code blocks are not rendered as part of the
code block. The `code-block` TextTag uses `paragraph_background`, which GTK does not
render on empty paragraphs. This breaks the visual continuity of code blocks.
**Solution:** Replace TextTag-based code block rendering with a dedicated widget
embedded as a `MarkdownSegment`, following the same segment-splitting pattern used
for tables.

## Goals

- Blank lines inside code blocks are visually contained within the block background.
- Code block background adapts automatically to light/dark theme via CSS variables.
- Language labels remain visible above the code content.
- Search match counting continues to include code block content.
- The public API stays unchanged.

## Non-Goals

- Preserving native `TextView` selection/copy for code block content.
- Syntax highlighting (deferred — could be layered on top later).
- Copy button or code-block-specific clipboard support.

Code block text selection is intentionally deferred to a separate design, accepting
the same temporary regression as table widgets.

## Scope

### Modified

- `src/ui/markdown.rs` — code block rendering moves from TextTag to widget segment;
  `MarkdownSegment` gains a `CodeBlock` variant; assembly logic updated.
- `data/resources/style.css` — add code block widget styling classes.

### Unchanged

- `src/ui/transcript_row.rs`
- `src/ui/tool_inspector_pane.rs`
- `src/ui/tool_renderers/generic.rs`
- Data model, parsers, SQLite schema
- Table rendering (already widget-based)

All existing callers continue to use the same public API:

```rust
pub fn render_markdown_to_textview(
    content: &str,
    highlight_query: Option<&str>,
) -> (gtk::Widget, usize)
```

## Root Cause

GTK's `paragraph_background` property on a `TextTag` does not render on empty
paragraphs (paragraphs containing only a `\n`). When a code block contains blank
lines (e.g., between two function definitions), those blank lines become empty
paragraphs in the `TextBuffer`. The `code-block` tag is applied to the `\n`
characters, but GTK does not paint the paragraph background for those lines.

## Architecture

### Current pipeline (code blocks)

```text
pulldown-cmark CodeBlock events → accumulate in code_buf
  → finish_code_block() → insert_with_tags(&code, &["code-block"])
  → paragraph_background applied via TextTag (breaks on blank lines)
```

### New pipeline

```text
pulldown-cmark CodeBlock events → accumulate in code_buf
  → finish_code_block() → flush current buffer to segments
  → build code block widget (Box + Labels)
  → push CodeBlock segment
  → start fresh buffer
  → render_markdown_to_textview() assembles segments into vertical Box
```

This follows the same segment-splitting pattern used for table rendering.

### What changes

- `MarkdownBufferWriter::finish_code_block()` stops inserting tagged text into the
  buffer and instead builds a widget + pushes a segment.
- `MarkdownSegment` gains a `CodeBlock(gtk::Widget)` variant.
- The fast-path check in `render_markdown_to_textview()` is generalized from
  `has_tables` to `has_widgets`.
- The `code-block` and `code-lang` TextTags are removed from `create_tag_table()`.
- The `apply_theme_palette_to_tags()` handler drops code-block-related entries.

### What stays the same

- All non-code-block, non-table markdown rendering remains `TextBuffer` + `TextTags`.
- Code block event collection (`code_buf`, `in_code_block`) stays unchanged.
- Table rendering is unaffected.
- The public function signature is unchanged.

## Widget Structure

```text
Box (vertical, .code-block-widget)
  ├─ Label (.code-block-lang, halign: Start)     [only if language specified]
  └─ Label (.code-block-content, halign: Fill)
```

### Content Label

- `selectable: false` (accepted regression)
- `wrap: true`, `wrap_mode: WordChar` — code wraps rather than overflows
- `xalign: 0.0` — left-aligned
- `use_markup: false` — raw text, no Pango interpretation of code content
- Monospace font via CSS `.code-block-content { font-family: monospace; }`

When search highlighting is active, `use_markup` switches to `true` and the text
uses Pango spans for highlight colors.

### Language Label

- Smaller font (`font-size: 0.85em` via CSS)
- Dimmed foreground (`opacity: 0.6`)
- Only present when `CodeBlockKind::Fenced` provides a non-empty language string

## Theme Integration

The current `paragraph_background` approach requires a manual `dark-notify` handler
to switch between `DARK_CODE_BG` and `LIGHT_CODE_BG` constants.

The new CSS-based approach uses `@card_shade_color`, a libadwaita CSS variable that
automatically adapts to light/dark mode. This eliminates the need for the code-block
entries in `apply_theme_palette_to_tags()`.

## Blockquote Context

If a code block is rendered while `blockquote_depth > 0`, the outer `Box` receives
the `.markdown-blockquote` CSS class, matching how table widgets handle blockquote
context.

## Search Highlighting

Same approach as table cells: Pango markup on the Label.

When `highlight_query` is `Some(query)`:

1. The code text is scanned for case-insensitive matches using `highlight_text()`
   from `src/ui/highlight.rs`.
2. Non-matching text is escaped with `pango_escape()`.
3. Matching text is wrapped in a Pango `<span>` with the shared highlight colors.
4. The content Label switches to `use_markup: true`.
5. Match count is accumulated in a field (analogous to `table_match_count`) and
   returned via `finalize()`.

The highlight colors come from the shared source in `src/ui/highlight.rs`, not
duplicated constants.

## `finish_code_block()` Flow

1. Flush the current text buffer into `segments` as a `Text` segment (if it has
   content), same pattern as `render_table()`.
2. Extract language from `in_code_block`.
3. Build the code block widget:
   - Create outer `Box` with `.code-block-widget`
   - If language is present, create and append language `Label` with `.code-block-lang`
   - Create content `Label` with the accumulated `code_buf` (trimmed of trailing
     newlines), apply `.code-block-content`
   - If `highlight_query` is set, apply markup highlighting to the content label
   - If `blockquote_depth > 0`, add `.markdown-blockquote` to the outer Box
4. Push `MarkdownSegment::CodeBlock(widget)` to `segments`.
5. Start a fresh `TextBuffer` for subsequent text.
6. Reset `code_buf` and `in_code_block`.

## Assembly in `render_markdown_to_textview()`

The current fast-path check:

```rust
let has_tables = segments.iter().any(|s| matches!(s, MarkdownSegment::Table(_)));
```

Becomes:

```rust
let has_widgets = segments.iter().any(|s| !matches!(s, MarkdownSegment::Text(_)));
```

The segment assembly loop adds a match arm for `CodeBlock`:

```rust
MarkdownSegment::CodeBlock(widget) => {
    container.append(&widget);
}
```

## CSS Changes

### New classes

```css
.code-block-widget {
    background-color: alpha(@card_shade_color, 0.15);
    border-radius: 6px;
    padding: 8px 12px;
    margin: 4px 0;
}

.code-block-lang {
    font-size: 0.85em;
    opacity: 0.6;
    margin-bottom: 4px;
}

.code-block-content {
    font-family: monospace;
}
```

### Removed TextTags

- `code-block` — no longer needed (was: monospace + paragraph_background)
- `code-lang` — no longer needed (was: scale 0.85 + dimmed foreground)

### Existing classes reused

- `.markdown-blockquote` — applied to code blocks inside blockquotes

## UX and Regression Notes

### Accepted temporary regression

- Code block content no longer participates in native `TextView` selection, `Ctrl+C`,
  or `select-all` behavior.
- Mixed selection across plain text and code block widgets is not guaranteed.

### Behavior that must be preserved

- Search match counts still include code block content.
- Language labels remain visible above the code.
- Code blocks inside blockquotes still receive the expected visual treatment.
- Inline code (`` `code` ``) rendering is unaffected — it remains a `TextTag`.

## Testing

### Automated

- Add a test verifying that a code block with blank lines renders as a widget segment
  (not text with `code-block` tag).
- Add a test verifying that the language label is present when a language is specified
  and absent when not.
- Add a test verifying that code block search highlighting contributes to the returned
  match count.
- Add a test verifying that code blocks inside blockquotes receive the
  `.markdown-blockquote` CSS class.

### Manual verification

- Run the app with `--sessions-dir tests/fixtures`.
- Verify blank lines inside code blocks are visually contained within the block
  background.
- Verify code blocks without language labels render correctly.
- Verify window resizing wraps long code lines.
- Verify that ordinary prose and inline code still render exactly as before.

### Fixture

- Add a fixture entry with a code block containing blank lines between functions to
  `tests/fixtures/` for non-regression testing.

## File Summary

### Modified

- `src/ui/markdown.rs`
- `data/resources/style.css`

### Unchanged

- `src/ui/transcript_row.rs`
- `src/ui/tool_inspector_pane.rs`
- `src/ui/tool_renderers/generic.rs`
- Data model, parsers, SQLite schema
