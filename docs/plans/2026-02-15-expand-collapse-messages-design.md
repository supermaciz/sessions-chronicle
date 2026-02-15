# Expand/Collapse Truncated Messages

## Context

Long messages in the session transcript are truncated to 2000 characters at the database query level (`substr(content, 1, 2000)`). A static label "(content truncated)" appears but users have no way to see the full content. This feature adds an inline expand/collapse toggle so users can view the complete message without leaving the transcript view.

## Design Decisions

- **Inline expand** (not lateral panel) — simple, direct, independent of the utility pane
- **No size limit** — load full content from DB, no progressive loading
- **Toggle** — expand and collapse, with cached content to avoid re-querying
- **Tool results excluded** — hide toggle for `Role::ToolResult` (handled separately in Phase 6)
- **Search counts stay correct** — expanding/collapsing can change highlighted match totals, so match counters must update per row

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

### Step 3: Enrich `MessageRowInit` with DB path

**File:** `src/ui/message_row.rs`

Add `db_path: PathBuf` to `MessageRowInit`. This is passed through from `SessionDetail` so the row can load full content on demand.

**File:** `src/ui/session_detail.rs`

Pass `self.db_path.clone()` when constructing `MessageRowInit` in both `load_first_page()` and `LoadMore` handler.

### Step 4: Add expand/collapse state and input to `MessageRow`

**File:** `src/ui/message_row.rs`

Model changes:
- Add `db_path: PathBuf` field
- Add `expanded: bool` field (default `false`)
- Add `full_content: Option<String>` field (cached full content)
- Add `rendered_match_count: usize` field (last rendered count, used to emit updates)

Add input enum:
```rust
pub enum MessageRowMsg {
    ToggleExpand,
}
```

Change `type Input = ()` to `type Input = MessageRowMsg`.

Update output enum so SessionDetail can update match counts for a specific row:

```rust
pub enum MessageRowOutput {
    MatchCountChanged {
        message_index: usize,
        count: usize,
    },
}
```

### Step 5: Replace truncation label with toggle button

**File:** `src/ui/message_row.rs`

In the `view!` macro, replace the static `gtk::Label` "(content truncated)" with a `gtk::Button`:

```rust
gtk::Button {
    #[watch]
    set_label: &if self.expanded { "Collapse" } else { "Show full message" },
    add_css_class: "flat",
    add_css_class: "caption",
    add_css_class: "expand-toggle",
    set_halign: gtk::Align::Start,
    set_margin_top: 4,
    #[watch]
    set_visible: self.preview.is_truncated() && self.preview.role != Role::ToolResult,
    connect_clicked => MessageRowMsg::ToggleExpand,
}
```

### Step 6: Implement `update_with_view()` for `ToggleExpand`

**File:** `src/ui/message_row.rs`

In `update_with_view()` handler for `ToggleExpand`:

1. On toggle, branch by current state:
   - If currently collapsed, attempt expand
   - If currently expanded, collapse immediately
2. Expand path:
   - If `self.full_content.is_none()`, call `load_message_full_content(&self.db_path, &self.preview.session_id, self.preview.message_index)`
   - If function returns `Ok(Some(content))`, cache in `self.full_content` and set `self.expanded = true`
   - If `Ok(None)` or `Err(_)`, log warning/error and keep collapsed state (`self.expanded = false`)
3. Determine displayed content: expanded -> cached full content, collapsed -> `self.preview.content_preview`
4. Re-render `content_container` via shared helper
5. If rendered match count changed, emit `MessageRowOutput::MatchCountChanged { message_index, count }`

### Step 7: Extract content rendering helper

**File:** `src/ui/message_row.rs`

Extract the content rendering logic from `init_widgets` into a reusable method:

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

Call this from both `init_widgets` (initial render) and `update_with_view` (on toggle).

### Step 8: Keep SessionDetail match counters in sync

**File:** `src/ui/session_detail.rs`

`SessionDetail` currently appends match counts as rows are built. Expand/collapse can change a row's highlighted count, so counts must be replaced, not only appended.

Model changes:
- Replace `match_counts: Vec<usize>` with `BTreeMap<usize, usize>` keyed by `message_index`

Message/output wiring changes:
- Update forwarding from `MessageRowOutput::MatchCountChanged { message_index, count }`
- Update `SessionDetailMsg::MatchCount` to carry `(message_index, count)`
- On receipt, insert/replace count for that `message_index`, then recompute `total_matches`

Navigation helper changes:
- Update `find_message_for_match()` to iterate counts in ascending `message_index` order and return the loaded row offset for scrolling.

### Step 9: Style the expand button

**File:** `data/resources/style.css`

```css
.expand-toggle {
    padding: 2px 8px;
    min-height: 0;
    font-size: 0.85em;
}
```

## Files Modified

| File | Change |
|------|--------|
| `src/database/mod.rs` | Add `load_message_full_content()` and include identifiers in preview query |
| `src/models/message_preview.rs` | Add `session_id`, `message_index` fields |
| `src/ui/message_row.rs` | Add expand/collapse state, input, toggle button, content rendering helper, per-row match count output |
| `src/ui/session_detail.rs` | Pass `db_path` in `MessageRowInit` and replace match counting with keyed updates |
| `data/resources/style.css` | Style for `.expand-toggle` button |
| `tests/message_preview.rs` | Add coverage for `load_message_full_content()` and identifier fields |

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
