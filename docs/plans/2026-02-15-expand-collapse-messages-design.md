# Expand/Collapse Truncated Messages

## Context

Long messages in the session transcript are truncated to 2000 characters at the database query level (`substr(content, 1, 2000)`). A static label "(content truncated)" appears but users have no way to see the full content. This feature adds an inline expand/collapse toggle so users can view the complete message without leaving the transcript view.

## Design Decisions

- **Inline expand** (not lateral panel) — simple, direct, independent of the utility pane
- **No size limit** — load full content from DB, no progressive loading
- **Toggle** — expand and collapse, with cached content to avoid re-querying
- **Tool results excluded** — will be handled separately in Phase 6

## Implementation Plan

### Step 1: Add `load_message_full_content()` to database

**File:** `src/database/mod.rs`

Add a new public function:

```rust
pub fn load_message_full_content(
    db_path: &Path,
    session_id: &str,
    message_index: usize,
) -> Result<String>
```

SQL: `SELECT content FROM messages WHERE session_id = ?1 AND message_index = ?2`

Returns the full, untruncated content string.

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

Add input enum:
```rust
pub enum MessageRowMsg {
    ToggleExpand,
}
```

Change `type Input = ()` to `type Input = MessageRowMsg`.

### Step 5: Replace truncation label with toggle button

**File:** `src/ui/message_row.rs`

In the `view!` macro, replace the static `gtk::Label` "(content truncated)" with a `gtk::Button`:

```rust
gtk::Button {
    set_label: &if self.expanded { "Collapse" } else { "Show full message" },
    add_css_class: "flat",
    add_css_class: "caption",
    add_css_class: "expand-toggle",
    set_halign: gtk::Align::Start,
    set_margin_top: 4,
    #[watch]
    set_visible: self.preview.is_truncated(),
    connect_clicked => MessageRowMsg::ToggleExpand,
}
```

### Step 6: Implement `update()` for `ToggleExpand`

**File:** `src/ui/message_row.rs`

In `update()` handler for `ToggleExpand`:

1. Toggle `self.expanded`
2. If expanding and `self.full_content.is_none()`:
   - Call `load_message_full_content(&self.db_path, &self.preview.session_id, self.preview.message_index)`
   - Cache result in `self.full_content`
3. Determine the content to display: `self.full_content` if expanded, `self.preview.content_preview` if collapsed
4. Clear `content_container` children and re-render with the appropriate content (reuse the same rendering logic from `init_widgets` — extract a helper method `render_content()`)

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

Call this from both `init_widgets` (initial render) and `update` (on toggle).

### Step 8: Style the expand button

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
| `src/database/mod.rs` | Add `load_message_full_content()` |
| `src/models/message_preview.rs` | Add `session_id`, `message_index` fields |
| `src/ui/message_row.rs` | Add expand/collapse state, input, toggle button, content rendering helper |
| `src/ui/session_detail.rs` | Pass `db_path` in `MessageRowInit` |
| `data/resources/style.css` | Style for `.expand-toggle` button |

## Verification

1. `cargo test` — ensure existing tests pass (MessagePreview changes may require test updates)
2. `cargo clippy` — no new warnings
3. `cargo fmt --all` — formatting
4. Manual testing with Flatpak dev build:
   - Open a session with long messages (>2000 chars)
   - Verify "Show full message" button appears on truncated messages
   - Click to expand — full content loads and renders correctly (markdown for assistant, plain text for user)
   - Click "Collapse" — returns to truncated preview
   - Re-expand — content loads instantly from cache (no DB query)
   - Verify non-truncated messages show no button
   - Verify search highlighting works on expanded content
