# Session Detail Lazy + Collapsed Tool Output Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Session Detail open quickly for large OpenCode sessions by rendering paginated previews and lazily loading full tool output only when the user expands it.

**Architecture:** Keep the existing SQLite FTS5 `messages` table as the source of truth. Add a lightweight `MessagePreview` model and DB queries that return `content_preview` (via `substr`) plus `content_len` (via `length`). Update the Session Detail UI to show previews in pages, collapse long/ToolResult content by default, and fetch full content on demand per message.

**Tech Stack:** Rust (edition 2024), Relm4/GTK4/libadwaita, rusqlite, SQLite FTS5.

---

## Preflight

### Task 0: Worktree setup (recommended)

**Files:**
- None

**Step 1: Create a dedicated worktree**

Run:
```bash
git status
git worktree add ../sessions-chronicle-lazy-detail -b feat/lazy-session-detail
```

Expected: A new clean worktree at `../sessions-chronicle-lazy-detail`.

---

## Phase 1: Evidence (measure before changing behavior)

### Task 1: Add coarse timing instrumentation

**Files:**
- Modify: `src/ui/session_detail.rs`
- Modify: `src/database/mod.rs`

**Step 1: Add DB timing logs**
- Wrap DB calls with `std::time::Instant` and log durations via `tracing::debug!`.

**Step 2: Add UI timing logs**
- Time widget construction and log: number of rows rendered, total preview bytes, max content length.

**Step 3: Manual check with two sessions**
- Open a heavy OpenCode session and a typical Claude Code session and compare timings.

---

## Phase 2: Data access (previews + full content on demand)

### Task 2: Add a `MessagePreview` model

**Files:**
- Create: `src/models/message_preview.rs`
- Modify: `src/models/mod.rs`

**Step 1: Add `MessagePreview`**

```rust
// src/models/message_preview.rs
use chrono::{DateTime, Utc};

use crate::models::Role;

#[derive(Debug, Clone)]
pub struct MessagePreview {
    pub session_id: String,
    pub index: usize,
    pub role: Role,
    pub content_preview: String,
    pub content_len: usize,
    pub timestamp: DateTime<Utc>,
}

impl MessagePreview {
    pub fn is_truncated(&self) -> bool {
        self.content_preview.len() < self.content_len
    }
}
```

**Step 2: Export it from the models module**
- Add `pub mod message_preview;` and `pub use message_preview::MessagePreview;` to `src/models/mod.rs`.

**Step 3: Commit**
```bash
git add src/models/message_preview.rs src/models/mod.rs
git commit -m "feat: add MessagePreview model"
```

---

### Task 3: Add DB functions for previews and full content

**Files:**
- Modify: `src/database/mod.rs`

**Step 1: Implement `load_message_previews_for_session`**

Add a new function with this signature:

```rust
pub fn load_message_previews_for_session(
    db_path: &Path,
    session_id: &str,
    limit: usize,
    offset: usize,
    preview_len: usize,
) -> Result<Vec<MessagePreview>>
```

Use a query that is robust even if `message_index` is stored as TEXT:

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
LIMIT ?3 OFFSET ?4;
```

Map fields:
- `role`: `Role::from_storage(&role_str).unwrap_or(Role::User)`
- `timestamp`: same conversion approach as `load_messages_for_session`

**Step 2: Implement `load_message_content_for_session_index`**

Add a new function:

```rust
pub fn load_message_content_for_session_index(
    db_path: &Path,
    session_id: &str,
    message_index: usize,
) -> Result<Option<String>>
```

Query:

```sql
SELECT content
FROM messages
WHERE session_id = ?1 AND CAST(message_index AS INTEGER) = ?2
LIMIT 1;
```

**Step 3: Commit**
```bash
git add src/database/mod.rs
git commit -m "feat: add preview and full-content message loaders"
```

---

### Task 4: Add DB tests for numeric ordering and truncation

**Files:**
- Modify: `src/database/mod.rs` (append tests)

**Step 1: Create a temporary DB and initialize schema**
- Use `tempfile::NamedTempFile` + `schema::initialize_database(&conn)`.

**Step 2: Insert messages with indices 2, 10, 1**
- Verify `load_message_previews_for_session(..., limit=100, offset=0)` returns indexes `[1, 2, 10]`.

**Step 3: Insert a 10_000 char content and verify truncation**
- `preview_len = 2000` => `content_preview.len() <= 2000` and `content_len == 10000`.

**Step 4: Verify full content fetch**
- `load_message_content_for_session_index(..., 10)` returns the full string.

**Step 5: Run tests**
Run:
```bash
cargo test
```
Expected: PASS.

**Step 6: Commit**
```bash
git add src/database/mod.rs
git commit -m "test: cover preview loaders ordering and truncation"
```

---

## Phase 3: UI behavior (collapsed by default, lazy expansion)

### Task 5: Switch Session Detail to preview-based loading

**Files:**
- Modify: `src/ui/session_detail.rs`

**Step 1: Replace `messages: Vec<Message>` with preview state**
- Add fields:
  - `message_previews: Vec<MessagePreview>`
  - `full_content_by_index: HashMap<usize, String>`
  - `page_size: usize` (default 200)
  - `preview_len: usize` (default 2000)

**Step 2: In `SetSession`, load only the first page of previews**
- Keep loading session metadata as-is.
- Load previews with `limit=page_size`, `offset=0`.

**Step 3: Commit**
```bash
git add src/ui/session_detail.rs
git commit -m "feat: load session detail as paginated previews"
```

---

### Task 6: Add "Load more" pagination to Session Detail

**Files:**
- Modify: `src/ui/session_detail.rs`

**Step 1: Add a message**
- `SessionDetailMsg::LoadMore`

**Step 2: Implement update handler**
- Query next page with `offset = message_previews.len()`.
- Append previews to the vector.

**Step 3: Add a button at the bottom**
- Render a "Load more" button if the last fetch returned `page_size` items.

**Step 4: Commit**
```bash
git add src/ui/session_detail.rs
git commit -m "feat: add load-more pagination to session detail"
```

---

### Task 7: Collapse tool output by default and lazily load full content

**Files:**
- Modify: `src/ui/session_detail.rs`

**Step 1: Add a row action message**
- `SessionDetailMsg::LoadFullContent { message_index: usize }`

**Step 2: Render logic (preview vs full)**
- If `full_content_by_index` contains `message_index`, show the full string.
- Otherwise show `content_preview`.

**Step 3: Default collapsed policy**
- For `Role::ToolResult`, always start collapsed (even if not truncated) and show:
  - a short preview
  - a button label "Show full" (or "Show" if short)

**Step 4: On click "Show full"**
- Call `load_message_content_for_session_index`.
- Store it in `full_content_by_index`.
- Re-render.

**Step 5: Verify manually**
- Open a heavy OpenCode session.
- Confirm initial render is fast.
- Expand 2-3 tool results; confirm they load and display full content.

**Step 6: Commit**
```bash
git add src/ui/session_detail.rs
git commit -m "feat: lazy-load full tool output on expand"
```

---

## Phase 4: Verification and polish

### Task 8: Run formatting, tests, and lint

**Files:**
- None (verification only)

**Step 1: Format**
```bash
cargo fmt --all
```

**Step 2: Test**
```bash
cargo test
```

**Step 3: Lint**
```bash
cargo clippy
```

Expected: All commands succeed.

---

## Optional improvements (do only if needed)

### Task 9 (optional): Make DB + UI loading asynchronous to avoid UI stalls

**Files:**
- Modify: `src/ui/session_detail.rs`

**Idea:** Show the loading state immediately, spawn the DB fetch on a background thread, then send results back to the component.

---

### Task 10 (optional): Switch message rendering to `gtk::ListView` virtualization

**Files:**
- Modify: `src/ui/session_detail.rs`

**Goal:** Avoid building a widget tree proportional to message count.

---

## Notes / Constraints

- Keep full content accessible: never truncate permanently in the DB.
- Keep previews small and cheap to layout; prioritize `Role::ToolResult` collapse.
- Cast `message_index` for reliable ordering.
- Prefer incremental commits with `type: ...` messages (repo convention).
