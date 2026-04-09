# Design: Syntax Highlighting for Code Blocks with GtkSourceView

**Date:** 2026-04-09  
**Status:** Approved  
**Issue:** [#49](https://github.com/supermaciz/sessions-chronicle/issues/49)  
**Problem:** Code blocks in the transcript view render in monospace without syntax
coloring, making large code snippets hard to read.  
**Solution:** Replace `gtk::TextBuffer` / `gtk::TextView` with `sourceview5::Buffer` /
`sourceview5::View` in fenced code block rendering, providing native GNOME syntax
highlighting with automatic dark/light theme support.

## Goals

- Fenced code blocks with a known language display syntax-colored content.
- Coloring follows the active GNOME theme (light/dark) dynamically.
- Search highlighting continues to work on code block content.
- The public API (`render_markdown_to_textview`) stays unchanged.

## Non-Goals

- Syntax highlighting in the tool inspector pane (deferred).
- Syntax-aware diff rendering (deferred).
- Line numbers in transcript code blocks.
- Custom language alias mapping (e.g. `ts` -> `typescript`).
- `guess_language()` fallback for blocks without a language tag.

## Scope

### Modified

- `src/ui/markdown.rs` -- `finish_code_block()` uses `sourceview5::Buffer` /
  `sourceview5::View`; theme handler extended.
- `Cargo.toml` -- add `sourceview5` dependency.
- `build-aux/io.github.supermaciz.sessionschronicle.Devel.json` -- add
  `gtksourceview-5` Flatpak module.
- `build-aux/io.github.supermaciz.sessionschronicle.json` -- same.
- `meson.build` -- add `gtksourceview-5` pkg-config dependency.

### Unchanged

- `src/ui/transcript_row.rs`
- `src/ui/tool_inspector_pane.rs`
- `src/ui/tool_renderers/`
- `src/ui/highlight.rs`
- `data/resources/style.css`
- Data model, parsers, SQLite schema

## Architecture

### Current pipeline (code blocks)

```text
pulldown-cmark CodeBlock events -> accumulate in code_buf
  -> finish_code_block() -> gtk::TextBuffer + gtk::TextView
  -> MarkdownSegment::CodeBlock(widget)
```

### New pipeline

```text
pulldown-cmark CodeBlock events -> accumulate in code_buf
  -> finish_code_block() -> sourceview5::Buffer + sourceview5::View
  -> language detection via LanguageManager
  -> style scheme via StyleSchemeManager (Adwaita / Adwaita-dark)
  -> MarkdownSegment::CodeBlock(widget)
```

### What changes

- `finish_code_block()` creates a `sourceview5::Buffer` instead of `gtk::TextBuffer`
  and a `sourceview5::View` instead of `gtk::TextView`.
- Each `sourceview5::Buffer` is stored as a weak reference for theme tracking.
- The existing `connect_dark_notify` handler is extended to update style schemes
  on all live `SourceBuffer` instances.

### What stays the same

- Widget tree structure: `Box > Label? > ScrolledWindow > View`.
- `MarkdownSegment::CodeBlock(gtk::Widget)` variant.
- All non-code-block markdown rendering.
- Search highlighting via `apply_search_highlight()`.
- The public function signature.

## Widget Structure

```text
Box (vertical, .code-block-widget)
  +-- Label (.code-block-lang, halign: Start)     [only if language specified]
  +-- ScrolledWindow (.code-block-scroller)
       +-- sourceview5::View (.code-block-content)
           - editable: false
           - cursor_visible: false
           - monospace: true
           - show_line_numbers: false
           - highlight_syntax: true (if language found) / false (otherwise)
           - highlight_current_line: false
           - wrap_mode: None
```

## Language Detection

Lookup via `sourceview5::LanguageManager::default().language(info_string)`:

- If the info string matches a known language ID -> set language on buffer,
  enable syntax highlighting.
- If no match or no info string -> disable syntax highlighting. The block
  renders as plain monospace text (current behavior).

No custom alias table. Common aliases (`js`, `py`, `sh`, `c`, `cpp`, `ts`) are
natively handled by GtkSourceView language definitions.

## Theme Integration

### Initial scheme selection

At `SourceBuffer` creation time, read `adw::StyleManager::default().is_dark()` and
apply the corresponding style scheme:

- Dark mode: `StyleSchemeManager::default().scheme("Adwaita-dark")`
- Light mode: `StyleSchemeManager::default().scheme("Adwaita")`

### Dynamic theme switching

The existing `connect_dark_notify` handler (line 780 of `markdown.rs`) updates
`TextTag` colors when the theme changes. Extend it to also update `SourceBuffer`
style schemes:

- During `finish_code_block()`, push a `glib::WeakRef<sourceview5::Buffer>` into
  a shared `Vec`.
- In the `connect_dark_notify` closure, iterate over weak refs, upgrade each one,
  and apply the appropriate style scheme.
- Dead weak refs (destroyed buffers) are silently skipped.

This follows the existing pattern: one handler, no widget tree traversal.

### CSS

No CSS changes. The existing classes apply as-is:

- `.code-block-widget` -- background via `@card_shade_color`, border-radius, padding
- `.code-block-scroller` / `.code-block-content` -- transparent background

The `SourceView` style scheme controls token colors (keywords, strings, comments).
The block background comes from CSS, not the style scheme -- visual consistency is
preserved.

## Search Highlighting

`sourceview5::Buffer` inherits from `gtk::TextBuffer`. The existing
`apply_search_highlight()` function works without modification on a `SourceBuffer`.

Search highlight `TextTag` priority is higher than syntax coloring tags, so the
yellow highlight background remains visible on top of syntax colors.

No changes to:
- `apply_search_highlight()` in `src/ui/highlight.rs`
- `code_block_match_count` accumulator
- The returned match count from `render_markdown_to_textview()`

## Dependencies

### Cargo.toml

```toml
sourceview5 = "0.11.0"
```

### Flatpak manifests

Add a `gtksourceview-5` module **before** the `sessions-chronicle` module in both
`build-aux/*.json` manifests:

```json
{
  "name": "gtksourceview-5",
  "buildsystem": "meson",
  "config-opts": [
    "-Ddocumentation=false",
    "-Dintrospection=disabled",
    "-Dvapi=false"
  ],
  "sources": [
    {
      "type": "git",
      "url": "https://gitlab.gnome.org/GNOME/gtksourceview.git",
      "tag": "5.14.2"
    }
  ]
}
```

- `introspection=disabled` and `vapi=false` to reduce build time.
- Pinned tag for reproducible builds.

### Meson

Add `gtksourceview-5` to the `dependency()` declarations in `meson.build`.

## Testing

### Automated

- Test that `finish_code_block()` with a known language (e.g. `rust`) produces a
  `SourceBuffer` with `highlight_syntax() == true` and a language set.
- Test that an unknown or absent language produces a `SourceBuffer` with
  `highlight_syntax() == false`.
- Test that search highlighting on a `SourceBuffer` returns the correct match count.

### Manual verification

- Run with `--sessions-dir tests/fixtures` and verify coloring on Rust, Python, JSON
  code blocks.
- Toggle dark/light theme -- coloring follows the switch.
- Code block without language -- monospace, no coloring (no regression).
- Search in a code block -- highlights visible on top of syntax colors.

## File Summary

### Modified

- `src/ui/markdown.rs`
- `Cargo.toml`
- `build-aux/io.github.supermaciz.sessionschronicle.Devel.json`
- `build-aux/io.github.supermaciz.sessionschronicle.json`
- `meson.build`

### Unchanged

- `src/ui/transcript_row.rs`
- `src/ui/tool_inspector_pane.rs`
- `src/ui/tool_renderers/`
- `src/ui/highlight.rs`
- `data/resources/style.css`
- Data model, parsers, SQLite schema
