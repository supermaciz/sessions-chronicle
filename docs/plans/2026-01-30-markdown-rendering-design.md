# Markdown Rendering for Assistant Messages

## Context

Sessions Chronicle displays AI session messages as plain text. Assistant
responses frequently contain markdown (headings, bold, lists, code blocks,
tables) that is currently rendered as-is, hurting readability.

This design adds markdown rendering for assistant messages using native GTK4
widgets and Pango markup — no WebKit dependency.

## Design Decisions

- **Scope: assistant messages only** — User, ToolCall, and ToolResult messages
  stay plain text. Their content is rarely structured markdown.
- **Native GTK widgets** — Each markdown block maps to a dedicated GTK widget
  (Label, Grid, Box, Separator). No WebKit or WebView.
- **pulldown-cmark** — Stable, battle-tested Rust parser used by rustdoc.
  Supports GFM (tables, task lists, strikethrough) via options.
- **Pango markup for inline formatting** — Bold, italic, code, strikethrough
  rendered via Pango tags inside `gtk::Label`.
- **html2pango for escaping** — Prevents content from breaking Pango markup.
- **Prepared for syntax highlighting** — Code blocks use a dedicated CSS class
  and plain text, ready for `syntect` integration in a future iteration.

---

## Architecture

### New module: `src/ui/markdown.rs`

Single public function:

```rust
pub fn render_markdown(content: &str) -> gtk::Box
```

Takes raw markdown text, returns a vertical `gtk::Box` containing rendered
widgets.

Internal logic:

1. **Parsing** — `pulldown_cmark::Parser` with
   `Options::ENABLE_TABLES | ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS`
2. **Construction** — Iterate over `Event` items, maintain a stack (`Vec`)
   for nesting (lists in lists, bold in italic, etc.)
3. **Emission** — Each top-level block produces a GTK widget appended to the
   container

### Block mapping

| Markdown block     | GTK Widget        | Details                                      |
|--------------------|-------------------|----------------------------------------------|
| Paragraph          | `gtk::Label`      | Pango markup, wrap, selectable               |
| Heading (h1–h6)   | `gtk::Label`      | Adwaita CSS classes (`title-1` to `title-4`) |
| Fenced code block  | `gtk::Label`      | Monospace, `.code-block` CSS class           |
| List (ul/ol)       | `gtk::Box`        | Vertical; each item = horizontal box         |
| Task list          | `gtk::Box`        | CheckButton (insensitive) + label            |
| Blockquote         | `gtk::Box`        | `.markdown-blockquote` CSS class             |
| Table              | `gtk::Grid`       | Header row in bold                           |
| Horizontal rule    | `gtk::Separator`  | Horizontal                                   |

### Integration in `session_detail.rs`

In `build_message_widget()`, replace the content Label with a conditional:

```rust
if preview.role == MessageRole::Assistant {
    let rendered = markdown::render_markdown(&preview.content_preview);
    container.append(&rendered);
} else {
    container.append(&content_label);
}
```

---

## Inline Formatting & Pango Markup

### Accumulator pattern

Inline events from pulldown-cmark arrive flat:
`Start(Emphasis)` → `Text("word")` → `End(Emphasis)`. An inline buffer
(`String`) accumulates Pango markup until the parent block ends, then
flushes into a Label with `use_markup: true`.

### Inline mapping

| Markdown          | Pango markup       |
|-------------------|--------------------|
| `**bold**`        | `<b>text</b>`     |
| `*italic*`        | `<i>text</i>`     |
| `` `code` ``      | `<tt>text</tt>`   |
| `~~strikethrough~~` | `<s>text</s>`   |
| `[text](url)`     | text + URL in dim  |

### Escaping

`html2pango` escapes `<`, `>`, `&` in raw text before insertion into markup.

### Links

`gtk::Label` with Pango does not reliably support clickable links. Links are
rendered as: **link text** followed by the URL in dim-label parentheses.

### Code blocks (fenced)

No Pango markup — plain text in a `gtk::Label` with CSS class `.code-block`
and monospace font. Prepared for `syntect` colorized Pango in a future
iteration.

---

## CSS Styles

New classes added to `data/resources/style.css`:

```css
.code-block {
    font-family: monospace;
    background-color: alpha(@card_shade_color, 0.5);
    border-radius: 6px;
    padding: 12px;
}

.markdown-blockquote {
    border-left: 3px solid @accent_color;
    padding-left: 12px;
    opacity: 0.85;
}

.markdown-table {
    padding: 4px;
}

.markdown-table-header {
    font-weight: bold;
}

.markdown-hr {
    margin-top: 8px;
    margin-bottom: 8px;
}
```

Existing styles (`.message-row`, role borders, headings) are unchanged.
Headings use Adwaita built-in classes (`title-1` to `title-4`).

---

## Truncation & Edge Cases

### Truncation

`content_preview` is limited to 2000 characters with an `is_truncated` flag.
Cutting markdown mid-stream may leave unclosed blocks. pulldown-cmark handles
malformed markdown gracefully — an unclosed code fence renders as one large
code block. The existing "(content truncated)" badge signals the user.
No changes to the truncation system.

### Messages without markdown

Plain text assistant content produces a single `Paragraph` event → one
`gtk::Label`. Visually identical to current behavior. No regression.

### Long content

The `gtk::Box` returned by `render_markdown()` sits inside the existing
`ScrolledWindow` in `session_detail.rs`. No changes needed.

### HTML in content

`html2pango` escapes HTML characters. Raw HTML in assistant responses is
displayed as text, not interpreted.

---

## File Changes

### Modified

- **`Cargo.toml`** — Add `pulldown-cmark` and `html2pango`
- **`src/ui/mod.rs`** — Declare `pub mod markdown;`
- **`src/ui/session_detail.rs`** — Import `markdown::render_markdown`, conditional
  call in `build_message_widget()` for assistant role
- **`data/resources/style.css`** — Add `.code-block`, `.markdown-blockquote`,
  `.markdown-table`, `.markdown-table-header`, `.markdown-hr` classes

### Created

- **`src/ui/markdown.rs`** — `render_markdown()` function with pulldown-cmark
  parsing, Pango inline accumulator, GTK widget construction

### Unchanged

- Data model, parsers, SQLite schema
- Other UI widgets (session_list, sidebar, modals)
