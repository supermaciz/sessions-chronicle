# Design: Pin Filter (Issue #109)

**Date:** 2026-04-02  
**Issue:** [#109 — feat: favorite sessions for quick revisit](https://github.com/supermaciz/sessions-chronicle/issues/109)  
**Exploration:** [2026-04-02-favorite-sessions-exploration.md](2026-04-02-favorite-sessions-exploration.md)  
**Decision:** Proposal F — Pin Filter  
**Type:** Implementation design

## Problem

Some sessions are worth revisiting soon. Today the only way to get back to
them is to search or navigate again, adding small but repeated friction. This
is a **quick-access problem**, not a discovery problem.

## Scope

- Pin/unpin sessions via context menu or `Ctrl+D`.
- Pinned rows gain a suffix icon and subtle CSS styling.
- Sidebar gains a composable "Pinned Only" checkbox filter.
- `Ctrl+D` only acts in the Sessions workspace, with an explicit target
  resolution rule.
- Detail header bar shows a pin toggle button (`pack_start`, after back)
  with toast feedback.
- No ordering, folders/tags, smart suggestions, or sync.

---

## 1. Schema — Migration v8

Add one column to `sessions`:

```sql
ALTER TABLE sessions ADD COLUMN pinned_at TEXT DEFAULT NULL;
PRAGMA user_version = 8;
```

- **Type**: `TEXT` storing ISO 8601 timestamps (e.g., `2026-04-03T14:30:00Z`),
  or `NULL` for unpinned. A timestamp (rather than boolean) enables future
  pin-order sorting without a second migration.
- **Index**: None in v1. The filter `pinned_at IS NOT NULL` is efficient on a
  small result set. An index can be added later if needed.
- **No fingerprint clear**: `pinned_at` is user-set metadata, not derived from
  parsing. This is the first migration that does not clear `file_fingerprints`.

## 2. Indexer Safety — Preserving `pinned_at` Across Re-index

The current indexer uses `INSERT OR REPLACE` for sessions
(`src/database/indexer.rs:664`). This deletes and re-inserts the row, which
would destroy `pinned_at`.

**Solution**: Change the session upsert to `INSERT ... ON CONFLICT(id) DO UPDATE`,
explicitly listing the columns to update and **excluding `pinned_at`**:

```sql
INSERT INTO sessions (
    id, tool, project_path, project_id, start_time, message_count,
    file_path, last_updated, first_prompt, parent_session_id, is_subagent,
    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
    reasoning_tokens, edit_count, read_count, command_count, ending_status
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
          ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
ON CONFLICT(id) DO UPDATE SET
    tool = excluded.tool,
    project_path = excluded.project_path,
    project_id = excluded.project_id,
    start_time = excluded.start_time,
    message_count = excluded.message_count,
    file_path = excluded.file_path,
    last_updated = excluded.last_updated,
    first_prompt = excluded.first_prompt,
    parent_session_id = excluded.parent_session_id,
    is_subagent = excluded.is_subagent,
    input_tokens = excluded.input_tokens,
    output_tokens = excluded.output_tokens,
    cache_read_tokens = excluded.cache_read_tokens,
    cache_write_tokens = excluded.cache_write_tokens,
    reasoning_tokens = excluded.reasoning_tokens,
    edit_count = excluded.edit_count,
    read_count = excluded.read_count,
    command_count = excluded.command_count,
    ending_status = excluded.ending_status;
```

- **New sessions**: get `pinned_at = NULL` (column default).
- **Existing sessions**: `pinned_at` is untouched by re-index.

## 3. Database — Toggle & Query Functions

### `toggle_pin(db_path, session_id) -> Result<bool>`

Atomically flips the pin state. Returns `true` if now pinned.

```sql
UPDATE sessions
SET pinned_at = CASE
    WHEN pinned_at IS NULL THEN ?1
    ELSE NULL
END
WHERE id = ?2
```

### `count_pinned_sessions(db_path, tools) -> Result<usize>`

Returns the number of pinned non-subagent sessions matching the current tool
filter. Called alongside `load_sidebar_project_data()`.

```sql
SELECT COUNT(*) FROM sessions
WHERE pinned_at IS NOT NULL AND is_subagent = 0
  [AND tool IN (...)]
```

### Filter clause additions

Both `load_sessions_for_filter()` and `search_sessions_for_filter()` gain a
`pinned_only: bool` parameter. When `true`, append:

```sql
AND pinned_at IS NOT NULL
```

Full composed query example:

```sql
WHERE is_subagent = 0
  AND tool IN (?, ?, ...)        -- tool filter
  AND project_id = ?             -- project filter (when active)
  AND pinned_at IS NOT NULL      -- pin filter (when active)
ORDER BY last_updated DESC
```

## 4. Session Model

`Session` struct gains one field:

```rust
pub pinned_at: Option<DateTime<Utc>>,
```

All session-reading queries include `pinned_at` in their `SELECT` list.
The `SessionRow` reads `pinned_at` to decide icon visibility, CSS class,
and context menu label.

## 5. Sidebar — Pin Filter Row

### Layout

Own section between AI Assistants and Projects, with separators above and
below (matching existing separator style):

```
┌─────────────────────────┐
│ Filters            title-4 │
│─────────────────────────│
│ AI Assistants      heading │
│ ☑ Claude Code              │
│ ☑ OpenCode                 │
│ ☑ Codex                    │
│ ☑ Mistral Vibe             │
│─────────────────────────│
│ Pinned             heading │
│ ☑ Pinned Only         (3)  │
│─────────────────────────│
│ Projects           heading │
│ All Sessions          (42) │
│ ...                        │
└─────────────────────────┘
```

### Widgets

- `gtk::Label` "Pinned" with `.heading` CSS class.
- `gtk::ListBox` with `selection_mode: None` and CSS class
  `"pinned-sidebar-list"`.
- Single `adw::ActionRow` inside:
  - **Prefix**: `gtk::CheckButton` (default: OFF). Row's `activatable_widget`
    set to the check, so clicking anywhere on the row toggles it.
  - **Title**: "Pinned Only"
  - **Suffix**: `gtk::Label` with `.project-badge` CSS class showing pinned
    session count (respects current tool filters). Shows "0" when no pins.

### Behavior

- Always visible, never hidden. When count is 0 the row is sensitive but
  shows "0" — the user can still check it (they see an empty list).
- Checking emits `SidebarOutput::FiltersChanged` with `pinned_only: true`.
  Unchecking emits `pinned_only: false`.
- Count badge updates whenever sessions are reloaded (same lifecycle as
  project counts).

### Model changes

| Type | Change |
|---|---|
| `Sidebar` struct | + `pinned_only: bool`, `pinned_count: usize` |
| `SidebarMsg` | + `PinnedOnlyToggled(bool)`, pinned count in `ProjectsLoaded` |
| `SidebarOutput::FiltersChanged` | + `pinned_only: bool` |

## 6. Filter Composition — Data Flow

```
Sidebar (CheckButton toggled)
  → SidebarMsg::PinnedOnlyToggled(bool)
  → self.pinned_only = bool
  → emit_filters_changed()
  → SidebarOutput::FiltersChanged { tools, project_filter, pinned_only }

App (init.rs forwarding)
  → AppMsg::FiltersChanged { tools, project_filter, pinned_only }
  → self.filter_state.pinned_only = pinned_only
  → emit_session_list_filters()

SessionList
  → SessionListMsg::SetFilters { tools, project_filter, pinned_only }
  → self.pinned_only = pinned_only
  → reload_sessions()
```

### Types touched

| Type | Change |
|---|---|
| `SidebarOutput::FiltersChanged` | + `pinned_only: bool` |
| `AppMsg::FiltersChanged` | + `pinned_only: bool` |
| `FilterState` | + `pinned_only: bool` (default `false`) |
| `SessionListMsg::SetFilters` | + `pinned_only: bool` |
| `SessionList` struct | + `pinned_only: bool` |

## 7. Session Row — Pin Visual & Context Menu

### Suffix icon

Pinned rows gain a `pin-symbolic` `gtk::Image` (16px, `@accent_color`)
inserted as a suffix **before** the ending-status icon:

```
[pin-symbolic] [ending-status-icon?] [go-next-symbolic]
```

Only present on pinned rows. Unpinned rows unchanged.

### CSS styling

Pinned rows get a `.pinned-row` CSS class on the root `gtk::Box`:

```css
.pinned-row {
    border-left: 2px solid @accent_color;
}
```

Theme-adaptive via `@accent_color`. Provides scannable visual lane without
background tint that would clash with selection highlight.

### Context menu

The existing `gio::Menu` gains a second action, placed **before**
"Resume in Terminal":

- **Action name**: `row.toggle-pin`
- **Label**: "Pin" when unpinned, "Unpin" when pinned (determined at
  construction from `self.session.pinned_at.is_some()`).
- **Output**: `SessionRowOutput::TogglePinRequested(String)` carrying
  the session ID.

### Detail view — Pin toggle button

The shared `adw::HeaderBar` gains a `gtk::ToggleButton` for pin state,
visible only in detail mode. Placed at `pack_start` after the back button
to avoid adding density to the already busy right side:

```
[← Back] [Pin]  [Search]  |  Sessions / Analytics  |  [Spinner] [Menu] [Pane] [Resume] [Parent]
```

**Widget:**
- `gtk::ToggleButton` with `set_icon_name("pin-symbolic")`, CSS class `.flat`.
- `#[watch] set_active: model.active_session_pinned`.
- `#[watch] set_visible: model.detail_visible && model.are_detail_actions_visible()`.
- Tooltip: "Pin session (Ctrl+D)" when unpinned, "Unpin session (Ctrl+D)"
  when pinned.
- Signal: `connect_clicked => AppMsg::TogglePinShortcutRequested` (same
  code path as `Ctrl+D`).

**Why `pack_start`:** Back and Pin only appear in detail mode — they
appear and disappear together, so grouping them on the left is natural.
The right side (menu, pane, resume, parent) stays untouched.

**Model change:** `App` gains `active_session_pinned: bool` (default
`false`), derived from the currently active detail session.

- Set from `session.pinned_at.is_some()` whenever `active_session` changes:
  initial detail open, opening a child session, returning to parent, and
  clearing detail state on navigate-back.
- Updated after each successful pin toggle so the header button remains a
  model-driven view, not a second source of truth.

**Toast feedback:** After each successful toggle, a 2-second `adw::Toast`
confirms the action: "Session pinned" / "Session unpinned". If the database
write fails, `active_session_pinned` stays unchanged and an error toast is
shown instead. The button is model-driven, so any transient GTK toggle state
is corrected on the next render.

### Keyboard shortcut — `Ctrl+D`

Wired as an app-level keyboard shortcut (not per-row), since the row
factory doesn't own keyboard focus. Because app accelerators are global, the
handler must explicitly scope the action and resolve the target session:

- **Sessions workspace only**. If the active workspace is Analytics, `Ctrl+D`
  is a no-op.
- **Detail view wins**. If a session detail page is open, toggle that
  `active_session`.
- **Otherwise use list selection**. If the user is on the session list, ask
  `SessionList` for the currently selected row's session ID and toggle that.
- **No selection**: no-op.

This avoids ambiguous "active vs selected" behavior and prevents toggling a
stale detail session while the user is looking at another workspace.

### Detail behavior under `Pinned Only`

If `pinned_only = true` and the user unpins the **currently open detail
session**, the detail view **stays open**. The session row is removed from the
filtered list in the background, but the user keeps reading the detail until
they navigate back. On return, the unpinned session is simply absent from the
list.

- Rationale: forcing an immediate navigation back feels abrupt, especially
  when the user is triaging pins one by one. Staying on the detail mirrors
  email-client behavior (archiving a message doesn't close it — it disappears
  from the inbox on return).
- Toggling from the list already behaves naturally: the row disappears after
  reload and selection falls back to the next available row (or empty state).

### Pin toggle data flow

```
Context menu on row
  → SessionRowOutput::TogglePinRequested(session_id)
  → App toggles that explicit session_id directly

Keyboard shortcut / detail pin toggle
  → AppMsg::TogglePinShortcutRequested
  → App resolves target:
     detail open → active_session.id
     otherwise → SessionListOutput::SelectedSessionForPin(session_id)
  → App toggles the resolved session_id

Shared completion path
  → Database: UPDATE sessions SET pinned_at = ...
  → success:
     update active_session_pinned when the affected session is the active detail
     show toast: "Session pinned" / "Session unpinned"
     refresh session list + sidebar pin count
  → failure:
     leave active_session_pinned unchanged
     show error toast
  → If detail session was unpinned while pinned_only = true:
     remove row from filtered list; detail view stays open
```

### Additional types touched

| Type | Change |
|---|---|
| `AppMsg` | + `TogglePinShortcutRequested`, pin-toggle completion handling |
| `App` struct | + `active_session_pinned: bool` |
| `SessionListMsg` / `SessionListOutput` | + request/response pair for selected session ID |

### Empty state behavior

`SessionList` empty-state logic must treat `pinned_only` as a first-class
filter input.

- If `pinned_only = true` and the filtered result set is empty, show a
  filter-specific empty state such as:
  - **Title**: "No pinned sessions"
  - **Description**: "Pin sessions from the list to keep them easy to revisit"
- Do **not** reuse the generic first-run state ("No Sessions Yet"), because
  the absence of results is caused by an active filter, not by missing source
  data.

## 8. Test & Verification Plan

### Unit tests (database)

1. **Migration v8 applies cleanly**: v7 database → `initialize_database()`
   → verify `PRAGMA user_version = 8` and `pinned_at` column exists with
   `NULL` default.
2. **Migration v8 is idempotent**: Run twice — no error.
3. **toggle_pin flips state**: Insert session, toggle → non-NULL. Toggle
   again → NULL.
4. **Re-index preserves pinned_at**: Insert session, set `pinned_at`,
   re-index via `ON CONFLICT` upsert → `pinned_at` preserved.
5. **pinned_only filter**: 3 sessions, pin 1. `pinned_only: true` → 1
   result. `pinned_only: false` → 3 results.
6. **count_pinned_sessions**: 3 sessions, pin 2 → count 2. Unpin 1
   → count 1.
7. **Pin filter composes with tool filter**: Sessions from 2 assistants,
   pin one of each. `pinned_only: true` + single tool → 1 result.
8. **Detail stays open after unpin**: With `pinned_only: true`, open a
   pinned session in detail, unpin it → detail view remains open. Navigate
   back → session is absent from the filtered list.
9. **Empty state copy for pinned filter**: `pinned_only: true` with zero
   matches shows a filter-specific empty state rather than the generic first-run
   "No Sessions Yet" state.
10. **Header pin state resyncs across session changes**: Open pinned session A,
    then navigate to unpinned child session B and back to parent A →
    `active_session_pinned` and header toggle follow the currently active session
    each time.
11. **Pin-toggle failure preserves model state**: Simulate `toggle_pin()`
    failure while detail is open → header toggle re-renders to the prior state
    and an error toast is shown.

### Manual verification (`--sessions-dir tests/fixtures`)

1. Right-click session → "Pin" appears in context menu.
2. Click "Pin" → row gains left border accent + pin icon suffix.
   Context menu now shows "Unpin".
3. Sidebar "Pinned Only" count badge increments.
4. Check "Pinned Only" → list filters to pinned sessions only.
5. Uncheck → all sessions visible again.
6. `Ctrl+D` on selected row toggles pin state in the Sessions workspace.
7. Switch to Analytics, press `Ctrl+D` → no session is toggled.
8. With `Pinned Only` enabled, open a pinned session in detail and unpin it
   → detail stays open. Navigate back → session absent from list.
9. With `Pinned Only` enabled and no pinned sessions, verify a filter-specific
   empty state is shown.
10. Open a pinned session detail → pin toggle button (left of header bar)
    shows pressed/active state. Click it → button flips to inactive, toast
    "Session unpinned" appears.
11. Open an unpinned session detail → pin toggle shows inactive. Click it
    → button flips to active, toast "Session pinned" appears.
12. Pin toggle button is hidden on the session list view and in Analytics.
13. Navigate from a pinned detail session to an unpinned child session and back
    → header toggle state follows the active session each time.
14. Force a pin-toggle failure → pin state remains unchanged and an error toast
    appears.
15. Pin session, quit, relaunch → pin persists.
16. Pin session, trigger re-index → pin persists.
17. Verify light and dark theme: left border + pin icon use
    `@accent_color` correctly.
