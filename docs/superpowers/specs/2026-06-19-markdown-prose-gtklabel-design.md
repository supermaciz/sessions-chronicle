# Design: GtkLabel Prose Rendering for Markdown Messages

**Date:** 2026-06-19  
**Status:** Proposed  
**Issue:** [#168](https://github.com/supermaciz/sessions-chronicle/issues/168) — SessionDetail: intermittent clipped content until click or scroll  
**Problem:** Assistant markdown prose is rendered through `GtkTextView`, which is not a
synchronous height-for-width widget. When a virtualized `GtkListView` row (recycled) or
an inspector `GtkScrolledWindow` measures its child synchronously at bind time, it can read
a **stale** height, so content is clipped (or oversized) until a later interaction forces
another layout pass.  
**Solution:** Render markdown prose with synchronous `GtkLabel`/widget segments instead of
`GtkTextView`: paragraph and heading blocks become labels, list items use structured
marker/content rows, and blockquotes use grouped containers. Tables and code blocks remain widget
segments, unchanged.

## Root Cause (confirmed)

`GtkTextView` validates its `GtkTextLayout` asynchronously on idle, so `measure(Vertical, for_width)`:

- returns `0` when unallocated, for every `for_width`;
- ignores `for_width` once allocated (reports the currently allocated layout height);
- returns a **stale** height immediately after a recycled content swap, correcting only after
  the main loop pumps.

`GtkLabel`, by contrast, measures synchronously correct in the same probes: it honours
`for_width` while unallocated and returns the correct height immediately after a content swap,
with no main-loop pump. This is why the earlier `hexpand`/`vexpand`/min-height attempt did
nothing — the problem is the *measured value being stale*, not expansion.

The bug is rare/intermittent because the native post-validation `queue_resize` is usually
honoured by the `ListView`, which self-corrects within a few idle cycles. The persistent
"stuck until interaction" state is a rare *missed* `queue_resize` race. A "force a
`queue_resize` at bind time" mitigation is therefore redundant with the native one and cannot
be proven by a RED→GREEN regression test. Switching prose to `GtkLabel` eliminates the bug
class instead of racing it.

## API Verification

- **Pango markup has no block-level layout attributes.** `<span>` covers `weight`, `style`,
  `strikethrough`, `size`/`font_scale`, `foreground`/`background`, `font_family`/`face` — but
  there is **no** attribute for left margin, indentation, or per-line indent. List/blockquote
  indentation therefore cannot live in markup; it must be a widget property (`margin-start`).
- **`GtkLabel` is height-for-width.** Wrapping labels measure height from the given width
  synchronously. The docs warn about "performance problems if it contains more than a small
  number of paragraphs" — so prose is split into **small label/widget segments**, keeping each
  label small, rather than one large label per message.
- **Search highlight needs no per-match anchors.** Search navigation in `session_detail.rs`
  operates at row/item granularity (`scroll_to_item`, `match_positions`); the in-widget
  highlight is purely visual. A `<span background>` in the markup reproduces it exactly.

## Goals

- Prose content is fully visible immediately after rendering, with no click/scroll/resize
  needed (transcript rows and inspector markdown sections).
- Inline formatting (bold, italic, strikethrough, inline code, headings) is preserved.
- List hanging indent and blockquote grouping are preserved.
- Search highlighting and per-message match counting are preserved.
- The public markdown renderer name matches the new widget-based design.

## Non-Goals

- Continuous text selection across blocks. Selection becomes per segment (label, list row, or
  grouped widget), which the user has accepted. Cross-segment selection was already lost for
  tables/code blocks.
- Changing code block (`sourceview5`) or table rendering. They are already widget segments and
  are not implicated in the reported symptoms.
- Clickable links. Links keep the current behaviour: link text followed by ` (url)` dimmed.
- Auditing the `sourceview5` code-block path for the same stale-measure behaviour (out of scope;
  no reported symptom there).

## Scope

### Modified

- `src/ui/markdown.rs` — prose rendering moves from `TextBuffer` + `GtkTextView` to per-block
  `GtkLabel`/widget segments; the `MarkdownBufferWriter` becomes a markup/widget writer; the
  `MarkdownSegment::Text(TextBuffer)` variant becomes widget-backed prose/list/blockquote
  segments.
- `data/resources/style.css` — prose/indentation/blockquote/hr classes as needed (reuse
  existing `.markdown-blockquote`, `.markdown-hr`).

### Mechanically Updated

- `src/ui/session_detail/transcript/row_rendering.rs`, `src/ui/tool_inspector_pane.rs`,
  `src/ui/tool_renderers/generic.rs` — call the renamed `render_markdown` entry point.

### Unchanged

- `src/ui/session_detail.rs` — search navigation stays row/item level.
- Table rendering, code block (`sourceview5`) rendering.
- Data model, parsers, SQLite schema.

The public rendering entry point is renamed:

```rust
pub fn render_markdown(
    content: &str,
    highlight_query: Option<&str>,
) -> (gtk::Widget, usize)
```

## Architecture

### Segment model

```rust
enum MarkdownSegment {
    Prose(gtk::Widget),    // paragraph/heading labels, list rows, blockquote groups
    Table(gtk::Widget),    // unchanged
    CodeBlock(gtk::Widget) // unchanged (sourceview5)
}
```

The single-`TextView` fast path is removed. `render_markdown` always assembles a vertical
`gtk::Box` of segments. The common case (a few prose paragraphs) becomes a handful of labels in
that box; structural blocks become small widget groups.

```rust
for segment in segments {
    match segment {
        MarkdownSegment::Prose(widget)    => container.append(&widget),
        MarkdownSegment::Table(widget)    => container.append(&widget),
        MarkdownSegment::CodeBlock(widget)=> container.append(&widget),
    }
}
```

### From TextBuffer to per-block Pango markup

`MarkdownBufferWriter` is rewritten as a markup/widget writer (`MarkdownWidgetWriter`):

- The inline `tag_stack` of TextTag names becomes a **stack of open Pango spans**
  (`<b>`, `<i>`, `<s>`, `<tt>`, and `<span ...>` for heading scale/weight). Each text run is
  `pango_escape`'d, then wrapped by the active spans.
- Text accumulates into a **per-block markup `String`**, not a global buffer.
- A **paragraph or heading boundary** finalizes the current markup block: build a wrapped
  `GtkLabel` and push a `Prose` segment.
- **Lists are structured widgets, not marker-prefixed labels.** Each list item is a horizontal
  row with a marker label (`-`, `1.`, `☑`, `☐`) and a wrapped content label. This preserves the
  hanging-indent behaviour of the pre-TextView `GtkLabel` renderer while keeping measurement
  synchronous.
- **Blockquotes are grouped widgets.** A blockquote creates a vertical `gtk::Box` with
  `.markdown-blockquote`; its inner prose/list/table/code widgets are appended to that box. The
  border and padding apply once to the quote group, not independently to each paragraph.
- Headings use `<span>` `size`/`font_scale` + `weight`, reproducing the visual weight of the
  current `heading-1..4` tags.

### Block label properties

Each prose `GtkLabel` is created with:

- `use_markup: true`
- `wrap: true`, `wrap_mode: WordChar`
- `selectable: true`
- `xalign: 0.0`, `halign: Start`, `hexpand: true`
- `valign: Start`, `vexpand: false`

List content labels use the same wrapping/selectable properties. Marker labels align to the top
of the row and do not expand.

### Search highlighting

Per block, build the block as an ordered list of `(text, styles)` runs. After assembly:

1. Concatenate the block's plain text and find case-insensitive matches with
   `crate::utils::text_match::find_case_insensitive_matches` (the existing helper).
2. Emit the block markup by walking the runs and splitting them at match boundaries, wrapping
   each matched substring in `<span background=… foreground=…>` using the shared colours from
   `src/ui/highlight.rs` (not duplicated constants).
3. Accumulate the per-block match count and return the total from `render_markdown`,
   exactly as today.

Search navigation in `session_detail.rs` is unchanged: it scrolls to the matching row/item, and
the in-label highlight is purely visual.

### Theme integration

- Prose no longer uses a shared `TextTagTable` or `apply_theme_palette_to_tags`. Normal
  `GtkLabel` text inherits Adwaita foreground colours, which already adapt to light/dark via CSS.
- Markup avoids hardcoded foreground colours except for existing semantic markers such as task
  checkboxes and search highlights. Dimmed inline suffixes, such as link URLs, use Pango `alpha`
  so they remain theme-relative.
- The `StyleManager::connect_dark_notify` listener is retained **only** to restyle the
  `sourceview5::Buffer`s of code-block segments.
- Search-highlight colours remain the fixed shared colours from `highlight.rs` (already the case).

### Links and edge cases

- **Links:** unchanged — link text followed by ` (url)` in a dimmed span.
- **Empty content:** returns an empty vertical `gtk::Box` (as today).
- **Malformed markup:** impossible to break Pango — every text run is `pango_escape`'d before
  insertion.
- **Horizontal rule:** a dedicated label (`────…`, `.markdown-hr`).
- **Images:** unchanged — `[image: alt]` inline text.

## Testing

### Measurement contract tests

Realize a prose `GtkLabel` and assert that `measure(Vertical, for_width)` is **synchronous and
correct** — it honours `for_width` while unallocated and returns the correct height immediately
after a content swap, with no main-loop pump. This is the exact probe where `GtkTextView`
returned `0`/stale. (A parallel probe documenting `GtkTextView`'s stale behaviour may be kept
as a diagnostic baseline if it is stable enough for CI.)

### Regression scenario

Add the smallest feasible GTK regression harness that swaps rendered markdown content inside an
already-realized row/container and measures the resulting widget at the target width immediately
after the swap. The assertion is that prose content height changes synchronously without requiring
a main-loop idle pump.

### Unit tests

- Prose-only markdown returns a container of prose labels, not a `GtkTextView`.
- Inline markup is correct: bold, italic, strikethrough, inline code, headings.
- List items render as structured marker/content rows, preserving hanging indent.
- Blockquotes render as grouped `.markdown-blockquote` containers.
- Search highlight injects spans on matches and the returned count is exact across multiple
  blocks.
- Tables and code blocks still render as widget segments.
- Empty content returns an empty box.
- `rg "render_markdown_to_textview" src tests` returns no matches.

### Manual verification

- Run with `--sessions-dir tests/fixtures`.
- Verify transcript prose renders fully on first display (no click/scroll needed), including the
  last visible row.
- Verify the inspector `Result` section renders fully without interaction.
- Verify light/dark theme switching for prose and search highlight.
- Verify lists, nested lists, blockquotes, and inline formatting render as before.

## File Summary

### Modified

- `src/ui/markdown.rs`
- `data/resources/style.css`
- `src/ui/session_detail/transcript/row_rendering.rs`
- `src/ui/tool_inspector_pane.rs`
- `src/ui/tool_renderers/generic.rs`

### Unchanged

- `src/ui/session_detail.rs`
- Data model, parsers, SQLite schema
