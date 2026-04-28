# Design: Syntax Highlighting for Code Blocks with GtkSourceView

**Date:** 2026-04-09  
**Status:** Implemented [#118](https://github.com/supermaciz/sessions-chronicle/pull/118)  
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
- Large or user-configurable language alias mapping tables.
- `guess_language()` fallback for blocks without a language tag.

## Scope

### Modified

- `src/ui/markdown.rs` -- `finish_code_block()` uses `sourceview5::Buffer` /
  `sourceview5::View`; theme handler extended; fenced-language normalization
  added for common markdown info strings; search highlight priority made explicit.
- `src/main.rs` -- initialize GtkSourceView once during app startup.
- `Cargo.toml` -- add a `sourceview5` dependency version compatible with the
  existing `relm4 0.10` / `gtk4 0.10` stack.
- `Cargo.lock` -- lock the resolved `sourceview5` and transitive dependency
  versions.
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
- `main()` initializes GtkSourceView once immediately after `gtk::init()`.
- A small normalization layer maps common fenced-code aliases to GtkSourceView
  language IDs before lookup.
- Each `sourceview5::Buffer` is stored as a weak reference for theme tracking.
- The existing `connect_dark_notify` handler is extended to update style schemes
  on all live `SourceBuffer` instances.
- Search highlighting keeps using `TextTag`s, but the `search-highlight` tag
  priority is explicitly raised instead of relying on default tag ordering.

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

Use the first fenced info token that `markdown.rs` already extracts today, then:

1. Normalize common markdown aliases to GtkSourceView IDs.
2. Lookup via `sourceview5::LanguageManager::default().language(normalized_id)`.
3. If lookup succeeds, set the buffer language and enable syntax highlighting.
4. If lookup fails or no info string is present, leave the block as plain
   monospace text with syntax highlighting disabled.

The alias table stays intentionally small and only covers high-frequency
markdown fence names that are unlikely to match GtkSourceView language IDs
directly. Initial mappings:

- `js` -> `javascript`
- `ts` -> `typescript`
- `py` -> `python`
- `sh`, `shell`, `bash`, `zsh` -> `sh`
- `rs` -> `rust`
- `yml` -> `yaml`
- `c++` -> `cpp`

No fallback to `guess_language()` for untagged blocks.

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

- `MarkdownBufferWriter` keeps a `Vec<glib::WeakRef<sourceview5::Buffer>>`.
- During `finish_code_block()`, push the newly created buffer into that `Vec`.
- `finalize()` returns those weak refs alongside segments and match counts.
- In the `connect_dark_notify` closure, iterate over weak refs, upgrade each one,
  apply the appropriate style scheme, and prune dead refs opportunistically.

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

Do not rely on default tag ordering. The implementation must explicitly raise the
shared `search-highlight` `TextTag` priority high enough that its background
remains visible on top of syntax-coloring tags in code blocks.

No changes to:
- Match-finding logic in `src/ui/highlight.rs`
- `code_block_match_count` accumulator
- The returned match count from `render_markdown_to_textview()`

## Dependencies

### Cargo.toml

Add `sourceview5`, but do **not** hardcode `0.11.0` unless the rest of the GTK
stack is upgraded to match it.

Constraint:

- The selected `sourceview5` crate version must be compatible with the existing
  `relm4 0.10` / `gtk4 0.10.x` / `glib 0.21.x` dependency set already used by the
  application.

Implementation note:

- After selecting the compatible crate version, commit the resulting
  `Cargo.lock` update.

### App startup

GtkSourceView requires explicit library initialization. Call the Rust binding's
`sourceview5::init()` function (or the binding equivalent if the exact API name
differs for the selected crate version) once in `src/main.rs`, immediately after
`gtk::init()`.

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
- Test that a common alias (e.g. fenced `ts`) resolves to the expected
  GtkSourceView language ID.
- Test that an unknown or absent language produces a `SourceBuffer` with
  `highlight_syntax() == false`.
- Test that search highlighting on a `SourceBuffer` returns the correct match count.
- Test that a code-block search match still carries the `search-highlight` tag
  after syntax highlighting is enabled.

### Manual verification

- App startup still succeeds with GtkSourceView initialized.
- Run with `--sessions-dir tests/fixtures` and verify coloring on Rust, Python, JSON
  code blocks.
- Verify common alias fences such as `ts` and `py` color correctly.
- Toggle dark/light theme -- coloring follows the switch.
- Code block without language -- monospace, no coloring (no regression).
- Search in a code block -- highlights visible on top of syntax colors.

## File Summary

### Modified

- `src/ui/markdown.rs`
- `src/main.rs`
- `Cargo.toml`
- `Cargo.lock`
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
