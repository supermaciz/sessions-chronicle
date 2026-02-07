# Search Term Highlighting — Design Document

**Date**: 2026-02-07
**Status**: Validated
**Proposal**: A (Inline Highlight + Floating Search Navigation Bar)
**Exploration**: [2026-02-07-search-highlighting-exploration.md](2026-02-07-search-highlighting-exploration.md)

---

## Overview

When a user searches for a term and opens a session from the results, highlight every occurrence of the search term inline in the detail view. A floating navigation bar provides match count, prev/next navigation, and a close button.

## Architecture

Four areas of change:

1. **Query propagation** — pass search term from `App` → `SessionDetail` → `MessageRow`
2. **Highlight rendering** — Pango markup wrapping in plain labels and markdown output
3. **Floating nav bar** — `gtk::Overlay` over the `ScrolledWindow`
4. **Scroll-to-match** — `compute_point()` + `vadjustment` for navigation

## 1. Query Propagation

### Data flow

```
App (holds search_query: String)
  │
  ├─ AppMsg::SessionSelected(id)
  │   └─ SessionDetailMsg::SetSession { id, search_query: Option<String> }
  │
  └─ AppMsg::SearchQueryChanged(query)
      └─ SessionDetailMsg::UpdateSearchQuery(Option<String>)
```

### Changes

- **`SessionDetailMsg::SetSession`** — becomes a struct variant carrying `id: String` and `search_query: Option<String>`.
- **`SessionDetailMsg::UpdateSearchQuery(Option<String>)`** — new variant for live clearing.
- **`SessionDetail` model** — new field `search_query: Option<String>`.
- **`MessageRowInit`** — new field `highlight_query: Option<String>`.
- **`App::update()`** — includes `self.search_query.clone()` in `SetSession`. Emits `UpdateSearchQuery(None)` on search clear.

### Rebuild strategy

When `UpdateSearchQuery` changes or clears the query, `SessionDetail` rebuilds the message factory (`guard().clear()` + re-push all messages). Same path as the existing `SetSession` reload.

## 2. Highlight Rendering

New module: `src/ui/highlight.rs`.

### `highlight_text(text, query) -> (String, usize)`

For plain text messages (user, tool_call, tool_result):

- Takes raw text, escapes to Pango, wraps case-insensitive matches with `<span background="#fce94f" foreground="#1e1e1e">…</span>`.
- Returns the highlighted Pango markup and the match count.

```rust
pub fn highlight_text(text: &str, query: &str) -> (String, usize) {
    // 1. Find all case-insensitive matches in raw text
    // 2. For each segment: pango_escape(non-match), then
    //    <span ...>pango_escape(match)</span>
    // 3. Return (markup, count)
}
```

### `highlight_in_markup(markup, query) -> (String, usize)`

For already-rendered Pango markup (markdown output):

- Parses character by character, skipping `<…>` tags and `&…;` entities.
- Highlights only within visible text segments.
- Returns highlighted markup and match count.

```rust
pub fn highlight_in_markup(markup: &str, query: &str) -> (String, usize) {
    // 1. Walk the markup string
    // 2. Copy <tags> and &entities; verbatim
    // 3. In text segments, case-insensitive match and wrap
    // 4. Return (highlighted_markup, count)
}
```

### Integration

- **`MessageRow::init_widgets()`** — for non-assistant roles: call `highlight_text()`, use `label.set_markup()`.
- **`markdown::render_markdown()`** — gains `highlight_query: Option<&str>`. After each block's Pango string is built, applies `highlight_in_markup()`.
- **Code blocks** — skipped in v1 (no highlighting inside code fences).

### Theme colors

Hardcoded for v1: `background="#fce94f"`, `foreground="#1e1e1e"` (Tango yellow). Works on both light and dark Adwaita.

## 3. Match Tracking

### Factory output

```rust
pub enum MessageRowOutput {
    MatchCount { index: DynamicIndex, count: usize },
}
```

Each `MessageRow` sends its match count to `SessionDetail` after rendering.

### Aggregation in SessionDetail

```rust
struct SessionDetail {
    // ...
    search_query: Option<String>,
    match_counts: Vec<usize>,     // per-message match counts
    current_match: usize,         // 0-based global match index
    total_matches: usize,         // sum of match_counts
}
```

### Resolving global match → message index

```rust
fn find_message_for_match(counts: &[usize], global_index: usize) -> (usize, usize) {
    let mut remaining = global_index;
    for (i, &count) in counts.iter().enumerate() {
        if remaining < count {
            return (i, remaining);
        }
        remaining -= count;
    }
    (counts.len().saturating_sub(1), 0)
}
```

Card-level scroll precision (scroll to the message row, not the exact span within it).

## 4. Floating Navigation Bar

### Widget tree

```
gtk::Overlay
├── gtk::ScrolledWindow (main child — existing)
│   └── gtk::Box (vertical)
│       ├── metadata card
│       ├── messages_box (factory)
│       └── "Load more" button
│
└── [overlay] gtk::Box .search-nav-bar
    ├── gtk::Label   (search term in quotes)
    ├── gtk::Button  (prev — go-up-symbolic)
    ├── gtk::Label   (counter: "2 / 7")
    ├── gtk::Button  (next — go-down-symbolic)
    └── gtk::Button  (close — window-close-symbolic)
```

### Positioning

```rust
add_overlay = &gtk::Box {
    set_halign: gtk::Align::Center,
    set_valign: gtk::Align::Start,
    add_css_class: "search-nav-bar",
    #[watch]
    set_visible: model.search_query.is_some(),
}
```

### Behavior

- **Visible** when `search_query.is_some()` (even if total_matches == 0).
- **Counter** shows `"2 / 7"` (1-indexed display).
- **Prev/Next** wrap around. Disabled when total_matches == 0.
- **Close** emits `SessionDetailMsg::ClearSearch` — clears highlights and hides bar, but does NOT clear the global search (session list stays filtered).

### New message variants

```rust
pub enum SessionDetailMsg {
    SetSession { id: String, search_query: Option<String> },
    UpdateSearchQuery(Option<String>),
    LoadMore,
    ResumeClicked,
    PrevMatch,
    NextMatch,
    ClearSearch,
    Clear,
}
```

## 5. Scroll-to-Match

### Mechanism

Uses `widget.compute_point()` to translate a message row's position to scroll coordinates, then sets `vadjustment.set_value()`.

```rust
fn scroll_to_match(&self, widgets: &SessionDetailWidgets, message_index: usize) {
    // 1. Get the factory widget at message_index
    // 2. Defer to glib::idle_add_local_once (wait for layout)
    // 3. compute_point() from message widget to scroll child
    // 4. Set vadjustment to position match ~1/3 from top
}
```

### When scroll happens

- **On session open from search** — auto-scroll to first match.
- **On PrevMatch / NextMatch** — scroll to message containing the target match.

### Edge cases

- No matches: no scroll, nav bar shows "0 matches".
- Matches beyond loaded page (200 messages): not addressed in v1.

## 6. CSS Additions

```css
.search-nav-bar {
    background-color: alpha(@headerbar_bg_color, 0.95);
    border-bottom: 1px solid alpha(@borders, 0.3);
    padding: 6px 12px;
    border-radius: 0 0 8px 8px;
    margin-top: 0;
}

.search-nav-bar .match-counter {
    font-variant-numeric: tabular-nums;
    min-width: 60px;
}
```

## 7. File Changes

| File | Change |
|------|--------|
| `src/ui/highlight.rs` | **NEW** — `highlight_text()`, `highlight_in_markup()` |
| `src/ui/mod.rs` | Add `pub mod highlight;` |
| `src/ui/session_detail.rs` | `gtk::Overlay` wrapper, nav bar, new messages, match state, scroll logic |
| `src/ui/message_row.rs` | `MessageRowInit.highlight_query`, `MessageRowOutput`, highlight integration |
| `src/ui/markdown.rs` | `render_markdown()` gains `highlight_query` param |
| `src/app.rs` | Pass `search_query` in `SetSession`, emit `UpdateSearchQuery` on clear |
| `data/resources/style.css` | `.search-nav-bar`, `.match-counter` styles |

No new crate dependencies.

## 8. Scope Exclusions (v1)

- No highlighting inside code blocks.
- No highlight-level scroll precision (card-level only).
- No match tracking for content beyond the 200-message page limit.
- No animated scroll transitions.
