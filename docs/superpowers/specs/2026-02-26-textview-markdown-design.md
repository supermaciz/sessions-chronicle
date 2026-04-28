# Design: TextView-based Markdown Rendering

**Date:** 2026-02-26
**Status:** Implemented [#42](https://github.com/supermaciz/sessions-chronicle/pull/42)
**Problem:** Text selection in assistant messages is limited to one line/block at a time because the current markdown renderer creates separate `gtk::Label` widgets per block. GTK does not allow text selection across widget boundaries.
**Solution:** Replace the multi-widget renderer with a single `gtk::TextView` per assistant message, using `TextBuffer` + `TextTag`s for formatting.

## Architecture

### Current pipeline (to be replaced)

```
pulldown-cmark → markdown_to_blocks() → Vec<MarkdownBlock> → render_block() → gtk::Box<Labels>
```

Each markdown block becomes a separate `gtk::Label` or container widget. Text selection stops at widget boundaries.

### New pipeline

```
pulldown-cmark events → MarkdownBufferWriter → TextBuffer + TextTags → gtk::TextView
```

A single `gtk::TextView` (non-editable, no cursor) holds all message content. Formatting is applied via `TextTag`s on ranges within the buffer. Text selection works across the entire message.

### What changes

- `markdown.rs`: New function `render_markdown_to_textview()` replaces `render_markdown()`.
- `transcript_row.rs`: `render_content()` calls the new function for `Role::Assistant`.

### What stays the same

- `markdown_to_blocks()` and its unit tests remain intact.
- `render_content()` for User/ToolResult messages (single `gtk::Label`) is unchanged.
- `highlight.rs` match-finding logic is reused for search highlighting.

## TextTag Catalogue

### Inline formatting

| Tag name        | Properties                          |
|-----------------|-------------------------------------|
| `bold`          | `weight: Bold (700)`                |
| `italic`        | `style: Italic`                     |
| `strikethrough` | `strikethrough: true`               |
| `code-inline`   | `family: "monospace"`               |

### Headings

| Tag name    | Properties                                                     |
|-------------|----------------------------------------------------------------|
| `heading-1` | `scale: 1.6, weight: Bold, pixels-above-lines: 8, pixels-below-lines: 4` |
| `heading-2` | `scale: 1.4, weight: Bold, pixels-above-lines: 6, pixels-below-lines: 3` |
| `heading-3` | `scale: 1.2, weight: Bold, pixels-above-lines: 4, pixels-below-lines: 2` |
| `heading-4` | `scale: 1.1, weight: Bold`                                    |

### Block-level

| Tag name      | Properties                                                                          |
|---------------|-------------------------------------------------------------------------------------|
| `code-block`  | `family: "monospace", paragraph-background: rgba(shade), pixels-above/below: 4, left-margin: 12, right-margin: 12` |
| `code-lang`   | `scale: 0.85, foreground: dim`                                                     |
| `blockquote`  | `left-margin: 16, foreground: alpha(fg, 0.85)`                                     |
| `list-item`   | `left-margin: 24, indent: -16` (hanging indent for marker)                         |
| `table-text`  | `family: "monospace"`                                                               |
| `table-header`| `family: "monospace", weight: Bold`                                                 |

### Search

| Tag name           | Properties                                        |
|--------------------|---------------------------------------------------|
| `search-highlight` | `background: #fce94f, foreground: #1e1e1e`        |

## Rendering Function

```rust
pub fn render_markdown_to_textview(
    content: &str,
    highlight_query: Option<&str>,
) -> (gtk::TextView, usize)
```

### MarkdownBufferWriter

Internal struct that walks pulldown-cmark events and writes directly to a `TextBuffer`:

- Maintains a `tag_stack: Vec<&str>` for nested inline tags (bold inside italic, etc.).
- Tracks block context: `in_code_block`, `in_list`, `in_blockquote`, `in_table`.
- Key method: `insert_with_tags(text, &[tag_names])` inserts text at the end iterator and applies named tags to the inserted range.
- Block spacing: A single `\n` between blocks; visual spacing handled by `pixels-above/below-lines` tag properties.

### Search highlighting

Separate pass after all content is written:

1. Extract full text from buffer.
2. Find case-insensitive matches (reuse logic from `highlight.rs`).
3. Convert char offsets to `TextIter` positions.
4. Apply `search-highlight` tag on each match range.

Returns the match count.

## Block-specific rendering

### Paragraphs

Insert text with active inline tags, followed by `\n`.

### Headings

Insert text with `heading-N` tag + any inline tags, followed by `\n`.

### Lists (ordered, unordered, task)

For each item:
- Insert marker (`- `, `1. `, `[x] `, `[ ] `) as plain text.
- Insert item text with `list-item` tag + inline tags.
- Insert `\n`.

Nested lists: increase `left-margin` via a depth-specific tag variant (e.g., `list-item-2` with `left-margin: 48`).

### Code blocks

- Insert language label (if present) with `code-lang` tag + `\n`.
- Insert code text with `code-block` tag + `\n`.

### Tables

Render as monospace tabulated text:
- Calculate max column widths.
- Pad each cell with spaces.
- Header row with `table-header` tag.
- Separator row with `─` characters.
- Data rows with `table-text` tag.

### Blockquotes

Insert contained blocks with `blockquote` tag applied (indentation + dimmed foreground). No vertical bar — CSS `border-left` cannot be reproduced via TextTag.

### Horizontal rules

Insert `───────────────` with dim foreground + `\n`.

## Integration Point

Single change in `transcript_row.rs::render_content()`:

```rust
if role == Role::Assistant {
    let (textview, match_count) = markdown::render_markdown_to_textview(content, highlight_query);
    container.append(&textview);
    return match_count;
}
```

All callers (`build_message_widgets`, `ToggleExpand`, `FullContentLoaded`) work unchanged because they go through `render_content()`.

## Phasing

### Phase 1 — Core (fixes the selection bug)

- Paragraphs with inline formatting (bold, italic, strikethrough, code inline)
- Headings (h1–h4)
- Lists (ordered, unordered, task, nested)
- Links (text + URL displayed)
- Search highlighting via TextTag

### Phase 2 — Remaining blocks

- Code blocks (monospace + paragraph-background)
- Tables (monospace tabulated text)
- Blockquotes (indentation + dim)
- Horizontal rules

### Cleanup

Once Phase 2 is validated:
- Remove `render_markdown()` and `render_block()`.
- Optionally remove `markdown_to_blocks()` or keep for reference/tests.

## Testing

- `markdown_to_blocks()` unit tests: unchanged.
- `highlight.rs` unit tests: unchanged.
- New unit tests for `render_markdown_to_textview()`: verify buffer text content and tag application ranges.
- Integration tests with `--sessions-dir tests/fixtures`: validate end-to-end rendering.
