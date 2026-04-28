# Expand/Collapse Truncated Messages

**Status:** Implemented [#35](https://github.com/supermaciz/sessions-chronicle/pull/35)

## Context

Long messages in the session transcript are truncated to 2000 characters at the database query level (`substr(content, 1, 2000)`). A static label "(content truncated)" appears but users have no way to see the full content. This feature adds an inline expand/collapse toggle so users can view the complete message without leaving the transcript view.

## Design Decisions

- **Inline expand** (not lateral panel) — simple, direct, independent of the utility pane
- **No size limit** — load full content from DB, no progressive loading
- **Toggle** — expand and collapse, with cached content to avoid re-querying
- **Async fetch on first expand** — avoid blocking GTK main loop while loading very large message bodies
- **Tool results excluded** — hide toggle for `Role::ToolResult` (handled separately in Phase 6)
- **Search counts stay correct** — expanding/collapsing can change highlighted match totals, so match counters must update per row
- **User-visible failure feedback** — failed full-content loads should show toast feedback, not logs only

## Implementation Plan

### Step 1: Add `load_message_full_content()` to database

**File:** `src/database/mod.rs`

Add a new public function:

```rust
pub fn load_message_full_content(
    db_path: &Path,
    session_id: &str,
    message_index: usize,
) -> Result<Option<String>>
```

SQL:
`SELECT content FROM messages WHERE session_id = ?1 AND CAST(message_index AS INTEGER) = ?2`

Returns the full, untruncated content string when present. Returns `Ok(None)` if the message is missing.

### Step 2: Enrich `MessagePreview` with identifiers

**File:** `src/models/message_preview.rs`

Add two fields to `MessagePreview`:
- `session_id: String`
- `message_index: usize`

These are needed so `MessageRow` can request the full content from the DB.

**File:** `src/database/mod.rs`

Update `load_message_previews_for_session()` SQL to also select `session_id` and `message_index`, and populate the new fields when constructing `MessagePreview`.

Updated SQL:

```sql
SELECT
  session_id,
  CAST(message_index AS INTEGER) AS message_index,
  role,
  substr(content, 1, ?2) AS content_preview,
  length(content) AS content_len,
  timestamp
FROM messages
WHERE session_id = ?1
ORDER BY CAST(message_index AS INTEGER) ASC
LIMIT ?3 OFFSET ?4
```

Column index offsets shift by 2 compared to the current query (currently 0–3, becomes 0–5). Update the `row.get(...)` calls accordingly.

### Step 3: Enrich `MessageRowInit` with DB path

**File:** `src/ui/message_row.rs`

Add `db_path: Arc<PathBuf>` to `MessageRowInit`. Using `Arc` avoids a heap allocation per row — the path is immutable shared state and `Arc::clone` is a cheap atomic increment.

**File:** `src/ui/session_detail.rs`

Store `db_path` as `Arc<PathBuf>` in the `SessionDetail` model (wrap it once in `init()`). Pass `self.db_path.clone()` (Arc clone) when constructing `MessageRowInit` in both `load_first_page()` and `LoadMore` handler.

### Step 4: Add expand/collapse state and input to `MessageRow`

**File:** `src/ui/message_row.rs`

Model changes:
- Add `db_path: Arc<PathBuf>` field
- Add `expanded: bool` field (default `false`)
- Add `full_content: Option<String>` field (cached full content)
- Add `loading_full_content: bool` field (disable toggle and show loading state)
- Add `rendered_match_count: usize` field (last rendered count, used to emit updates)

Add command output enum for background DB fetch:

```rust
pub enum MessageRowCmd {
    FullContentLoaded(Result<Option<String>>),
}
```

Change `type CommandOutput = ()` to `type CommandOutput = MessageRowCmd`.

Add input enum:
```rust
pub enum MessageRowMsg {
    ToggleExpand,
}
```

Change `type Input = ()` to `type Input = MessageRowMsg`.

Replace the existing output enum so SessionDetail can update match counts for a specific row:

```rust
pub enum MessageRowOutput {
    MatchCountChanged {
        message_index: usize,
        count: usize,
    },
    ExpandLoadFailed {
        message_index: usize,
    },
}
```

This replaces the current `MatchCount { count }` variant. Both emission sites must change:
- **`init_widgets`** (initial render): the existing `MatchCount` emission becomes `MatchCountChanged { message_index: self.preview.message_index, count: match_count }`
- **`update_with_view`** (on toggle, Step 7): emit `MatchCountChanged` when rendered match count differs from cached value

**Important:** `sender.output(...)` returns `Result<(), O>`. Use `.ok()` to discard the error when the receiver has been dropped (e.g. `sender.output(MessageRowOutput::MatchCountChanged { ... }).ok();`).

### Step 5: Replace truncation label with toggle button

**File:** `src/ui/message_row.rs`

In the `view!` macro, replace the static `gtk::Label` "(content truncated)" with a `gtk::Button`:

```rust
gtk::Button {
    #[watch]
    set_label: &if self.loading_full_content {
        "Loading..."
    } else if self.expanded {
        "Collapse"
    } else {
        "Show full message"
    },
    add_css_class: "flat",
    add_css_class: "caption",
    add_css_class: "expand-toggle",
    set_halign: gtk::Align::Start,
    set_margin_top: 4,
    #[watch]
    set_sensitive: !self.loading_full_content,
    #[watch]
    set_visible: self.preview.is_truncated() && self.preview.role != Role::ToolResult,
    connect_clicked => MessageRowMsg::ToggleExpand,
}
```

### Step 6: Extract content rendering helper

**File:** `src/ui/message_row.rs`

Extract the content rendering logic from `init_widgets` into a reusable helper **before** implementing the toggle handler, since both `init_widgets` and the toggle need to render content. Call it with `&widgets.content_container`:

```rust
fn render_content(
    container: &gtk::Box,
    content: &str,
    role: Role,
    highlight_query: Option<&str>,
) -> usize  // returns match_count
```

This method:
1. Clears `container` children
2. Renders markdown (assistant) or plain text (other roles) with optional highlighting
3. Returns the highlight match count

Call this from both `init_widgets` (initial render) and `update_with_view` (on toggle, Step 7).

### Step 7: Implement `update_with_view()` and `update_cmd_with_view()` for expand/collapse

**File:** `src/ui/message_row.rs`

#### Input handling: `update_with_view()`

Override `update_with_view()` instead of `update()` because we need direct access to widgets (specifically `content_container`) to clear and re-render children via `render_content()`. **Important:** when you override `update_with_view`, you **replace** the default pipeline (`update` + `update_view`) entirely — the runtime never calls `update()`. We must call `self.update_view(widgets, sender)` before returning from each handled path so `#[watch]` macros on the toggle button label and visibility re-evaluate.

Handler for `ToggleExpand`:

1. On toggle, branch by current state:
   - If currently collapsed, attempt expand
   - If currently expanded, collapse immediately
2. Expand path:
   - If `self.full_content.is_some()`, expand immediately without DB access
   - If `self.full_content.is_none()`, set `self.loading_full_content = true` and dispatch background fetch using `sender.spawn_oneshot_command(...)`
   - After scheduling the command, call `self.update_view(widgets, sender)` and return immediately (do not block the GTK thread)
3. Collapse path:
   - Set `self.expanded = false`
   - Re-render collapsed preview content using `render_content()`
4. If rendered match count changed, update `self.rendered_match_count` and emit `MessageRowOutput::MatchCountChanged { message_index, count }` (use `.ok()`)
5. For non-early-return paths, call `self.update_view(widgets, sender)` at the end to flush `#[watch]` updates

Skeleton:

```rust
fn update_with_view(
    &mut self,
    widgets: &mut Self::Widgets,
    message: Self::Input,
    sender: FactorySender<Self>,
) {
    match message {
        MessageRowMsg::ToggleExpand => {
            // ... steps 1-5 ...
        }
    }
    // IMPORTANT: for non-early-return paths, trigger #[watch] updates
    self.update_view(widgets, sender);
}
```

#### Command handling: `update_cmd_with_view()`

Override `update_cmd_with_view()` instead of `update_cmd()` because we need `&mut widgets.content_container` to re-render children after the async DB fetch completes. Same as above: overriding replaces the default pipeline, so we must call `self.update_view(widgets, sender)` ourselves.

- On `FullContentLoaded(Ok(Some(content)))`: cache content, set `expanded = true`, set `loading_full_content = false`, re-render expanded content via `render_content()`, emit match update if count changed (`.ok()`)
- On `FullContentLoaded(Ok(None))`: set `expanded = false`, set `loading_full_content = false`, keep preview content rendered, emit `ExpandLoadFailed { message_index }` (`.ok()`)
- On `FullContentLoaded(Err(err))`: log error, set `expanded = false`, set `loading_full_content = false`, keep preview content rendered, emit `ExpandLoadFailed { message_index }` (`.ok()`)
- Call `self.update_view(widgets, sender)` at the end to flush `#[watch]` updates

### Step 8: Keep SessionDetail match counters in sync and surface load failures

**File:** `src/ui/session_detail.rs`

`SessionDetail` currently appends match counts as rows are built. Expand/collapse can change a row's highlighted count, so counts must be replaced, not only appended.

Model changes:
- Replace `match_counts: Vec<usize>` with `BTreeMap<usize, usize>` keyed by `message_index`

Message/output wiring changes:
- Update `SessionDetailMsg::MatchCount` to carry `(message_index, count)` instead of a bare `usize`
- Add `SessionDetailMsg::ShowExpandLoadFailure` variant
- Update the factory forwarding closure in `init()`:

```rust
.forward(sender.input_sender(), |output| match output {
    MessageRowOutput::MatchCountChanged { message_index, count } =>
        SessionDetailMsg::MatchCount(message_index, count),
    MessageRowOutput::ExpandLoadFailed { .. } =>
        SessionDetailMsg::ShowExpandLoadFailure,
})
```

- On `MatchCount(message_index, count)`, insert/replace count for that `message_index` in the `BTreeMap`, then recompute `total_matches` as the sum of all values
- Keep existing auto-scroll behavior: when `total_matches` transitions from `0` to `> 0`, jump to the first match

Toast UX changes:

- Add an `adw::ToastOverlay` as the **root of the detail page content** in `SessionDetail`, wrapping the existing `gtk::Overlay` (`detail_overlay`) inside that stack page. Store a `#[name(toast_overlay)]` reference in the widgets struct.
- On `ShowExpandLoadFailure`, show a short non-blocking toast: `toast_overlay.add_toast(adw::Toast::new("Could not load full message."));`

Navigation helper changes:
- Update `find_message_for_match()` to accept the `BTreeMap<usize, usize>` and iterate in ascending `message_index` order.
- Use `enumerate()` while iterating map entries so the helper can return the **loaded row offset** (factory child position), not the raw `message_index` key.
- Keep returning `(loaded_row_offset, local_match_index)` so existing scroll logic stays index-based against `messages.observe_children()`.

### Step 9: Style the expand button

**File:** `data/resources/style.css`

```css
.message-row .expand-toggle {
    padding: 2px 8px;
    min-height: 0;
    font-size: 0.85em;
}

.message-row .expand-toggle:disabled {
    opacity: 0.7;
}
```

Keep Adwaita defaults for colors/borders (do not override `color`, `background`, or `border-color`) to reduce style conflicts with `flat` buttons.

## Files Modified

| File | Change |
|------|--------|
| `src/database/mod.rs` | Add `load_message_full_content()` and include identifiers in preview query |
| `src/models/message_preview.rs` | Add `session_id`, `message_index` fields |
| `src/ui/message_row.rs` | Add expand/collapse state, async load command flow, toggle loading state, content rendering helper, per-row match count output |
| `src/ui/session_detail.rs` | Pass `db_path` in `MessageRowInit`, replace match counting with keyed updates, and show toast on load failure |
| `data/resources/style.css` | Style for `.expand-toggle` button |
| `tests/message_preview.rs` | Add coverage for `load_message_full_content()` (happy path, missing session, missing message index, very large content) and identifier fields |

## Verification

1. `cargo test` — ensure existing tests pass (MessagePreview changes may require test updates)
2. `cargo clippy` — no new warnings
3. `cargo fmt --all` — formatting
4. Manual testing with Flatpak dev build:
    - Open a session with long messages (>2000 chars)
    - Verify "Show full message" button appears on truncated messages
    - Verify truncated `TOOL RESULT` messages do **not** show the toggle button
    - Click to expand — full content loads and renders correctly (markdown for assistant, plain text for user)
    - Click "Collapse" — returns to truncated preview
    - Re-expand — content loads instantly from cache (no DB query)
    - Verify non-truncated messages show no button
    - Verify search highlighting works on expanded content
    - Verify match counter and next/prev navigation update correctly after expand and collapse
    - While first expand is loading, verify toggle shows `Loading...` and is disabled
    - Force a missing/full-content lookup failure case and verify row stays collapsed, app remains responsive, and a toast appears
    - Open GTK inspector to verify `.message-row .expand-toggle` does not break Adwaita flat-button visuals in light and dark themes
