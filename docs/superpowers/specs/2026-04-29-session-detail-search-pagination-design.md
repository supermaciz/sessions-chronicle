# Session Detail Search — Pagination-Aware Navigation

**Date:** 2026-04-29  
**Status:** Accepted

## Problem

In `SessionDetail`, the in-session search only operates on transcript content currently loaded via the `LoadMore` paginator. For long sessions, matches living in unloaded pages are invisible to the user: the count is wrong, navigation cannot reach them, and there is no signal that anything is missing. The search silently lies.

The fix needs to keep the perf benefits of pagination (large sessions still load incrementally) while giving the user a search that is honest about totals and able to reach every match.

## Scope

In scope:
- A new pagination-aware search flow in `src/ui/session_detail.rs` that uses the SQLite FTS5 index (`messages_fts`) as the source of truth for total match count and match ordering.
- Contiguous progressive loading: when the user navigates to a match in an unloaded page, intermediate pages are loaded silently and in order.
- Visual feedback for in-progress jumps in the existing floating search nav bar (spinner + secondary "loaded / total" counter).
- Disabling tool-call inline highlighting while a search is active, to keep visible highlights aligned with what the counter counts.

Out of scope:
- Extending FTS to index tool-call content (`tool_calls.input_json`, `output_text`) or subagent content. Discussed and explicitly deferred (see "Decisions").
- Any "results-only" view that hides non-matching transcript items.
- Caching match positions across sessions.
- Sub-row occurrence navigation (e.g., navigating to occurrence 3 of 5 within a single message).
- Any change to the global (sidebar) search UX.

## Decisions

The brainstorming dialogue resolved these trade-offs explicitly:

1. **UX model — progressive loading driven by navigation.**  Total match count is announced immediately (FTS-derived); each Prev/Next click loads any pages required to reach the next match.
2. **Contiguous loading on jumps.**  Skipping from match #3 (page 1) to match #4 (page 5) loads pages 2–4 silently. Preserves transcript continuity and reuses existing pagination machinery.
3. **No FTS schema migration.**  Search scope is limited to `messages.content`, matching what is already in `messages_fts`. Indexing tool-call detail would create matches invisible in the inline transcript (only the inspector pane shows full content), reproducing the same UX problem ruled out for subagents in Q4.
4. **Subagents not indexed.**  Same reasoning: they are not highlighted inline today.
5. **Per-item match counter.**  The bar shows "k of N matching items", not "occurrence k of N occurrences". Sub-row occurrences remain visually highlighted but are not individually navigable. Counter does not drift during loading.
6. **Tool-call inline highlighting suppressed during search.**  When a search is active, `highlight_query` is no longer passed to tool-call rows. Cohérent with the count: what is visually highlighted matches what is counted.

## Architecture

Three layers of change:

- **Database layer (`src/database/`)** — new query function `find_session_match_positions` returning the ordered list of matching items in a session.
- **Model layer (`src/ui/session_detail.rs`)** — new state `match_positions`, `pending_jump`, `loading_jump`; old per-row match accounting (`match_segments`, `total_matches`) removed.
- **View layer (search nav bar in the same file)** — spinner + secondary loaded counter; Prev/Next disabled during a jump.

No new files. No new modules.

## Data flow

```
User types query
   │
   ▼
UpdateSearchQuery(Some(q))
   │   spawns DB job (request_id N)
   ▼
find_session_match_positions(session_id, q)
   │   returns Vec<MatchPosition> ordered by item_index
   ▼
SetMatchPositions { request_id, positions }
   │   if request_id matches current → store, current_match=0
   ▼
Trigger jump_to(0)
   │
   ├── target.item_index < loaded_count → scroll_to_item, done
   │
   └── otherwise:
        loading_jump = true
        pending_jump = Some(0)
        loop: emit LoadMore until loaded_count > target.item_index
        scroll_to_item, loading_jump = false
```

Subsequent Prev/Next reuse the same `jump_to(i)` path.

## Database layer

### Query

```rust
pub struct MatchPosition {
    pub item_index: i64,
    pub kind: MatchKind, // currently always Message; reserved for future expansion
}

pub enum MatchKind {
    Message,
}

pub fn find_session_match_positions(
    db: &Connection,
    session_id: &str,
    query: &str,
) -> Result<Vec<MatchPosition>>;
```

Implementation: a single SQL query joining `messages_fts` (for the MATCH filter) to `messages` (to filter by `session_id`) to `transcript_items` (to retrieve the canonical `item_index` ordering used by the transcript view).

```sql
SELECT ti.item_index
FROM messages_fts
JOIN messages m ON m.id = messages_fts.rowid
JOIN transcript_items ti
  ON ti.session_id = m.session_id
 AND ti.message_index = m.message_index
WHERE messages_fts MATCH ?
  AND m.session_id = ?
ORDER BY ti.item_index ASC;
```

The `MatchKind` enum is introduced as a forward-compatibility hook: future expansions (tool calls, subagents) extend it without changing call sites that just need ordered positions.

Sanitization of the query string reuses the existing helper used by the global search path (`search_sessions_with_query`), so syntactically invalid FTS queries do not error — they return an empty list.

### No migration

`messages_fts` already exists (since schema v13). No new tables, no new triggers, no new indices. The existing `idx_transcript_items_session` (or equivalent ordering on `(session_id, item_index)`) is sufficient.

### Tests

Integration tests in `tests/`:

- `find_session_match_positions_returns_ordered_message_matches` — fixture with three matching messages at non-contiguous `item_index`; assert order.
- `find_session_match_positions_filters_by_session` — two sessions both containing the query; assert only the requested session is returned.
- `find_session_match_positions_handles_invalid_query` — query containing FTS5 syntax errors returns `Ok(vec![])`, never errors.
- `find_session_match_positions_empty_query` — empty / whitespace query returns `Ok(vec![])`.

## Model layer

### State changes in `SessionDetailModel`

Added:

```rust
match_positions: Vec<MatchPosition>,
pending_jump: Option<usize>,   // index into match_positions
loading_jump: bool,
search_request_id: u64,        // monotonic, used to discard stale results
```

Removed (or repurposed; see below):

- `match_segments: HashMap<usize, Vec<usize>>` — no longer needed; the source of truth is `match_positions`.
- `total_matches: usize` — replaced by `match_positions.len()`.
- The `MatchSegmentsChanged` plumbing from `TranscriptRow` to `SessionDetail` — child rows still report internally for their own rendering needs, but the parent no longer aggregates.

`current_match: usize` is preserved, but its semantics shift from "occurrence index" to "index into `match_positions`".

### New messages

```rust
SessionDetailMsg::SetMatchPositions {
    request_id: u64,
    positions: Vec<MatchPosition>,
}
```

Existing messages that change behavior:

- `UpdateSearchQuery(query)` — bumps `search_request_id`, clears `match_positions`, spawns a DB job for the FTS query, then on result emits `SetMatchPositions`. Resets `current_match`, `pending_jump`, `loading_jump`.
- `ClearSearch` — clears `match_positions`, cancels `pending_jump`, restores tool-call highlighting.
- `PrevMatch` / `NextMatch` — now operate on `match_positions` index (with wraparound) and route through the jump path.
- `LoadMore` (manual button click) — unchanged, but post-load the model checks if a `pending_jump` target is now within `loaded_count` and resolves it.

### Jump logic

```rust
fn jump_to(&mut self, target: usize, sender: &ComponentSender<Self>) {
    let Some(pos) = self.match_positions.get(target) else { return };
    self.current_match = target;

    if (pos.item_index as usize) < self.loaded_count {
        self.pending_jump = None;
        self.loading_jump = false;
        self.scroll_to_item.set(Some(/* row at item_index */));
    } else {
        self.pending_jump = Some(target);
        self.loading_jump = true;
        // Kick off one LoadMore; chain continues via apply_next_page_rows hook.
        self.load_next_page(sender);
    }
}
```

In `apply_next_page_rows` (after the existing logic that updates `loaded_count`):

```rust
if let Some(target) = self.pending_jump {
    let pos = &self.match_positions[target];
    if (pos.item_index as usize) < self.loaded_count {
        self.pending_jump = None;
        self.loading_jump = false;
        self.scroll_to_item.set(Some(/* row at item_index */));
    } else if self.has_more_messages {
        self.load_next_page(sender);
    } else {
        // Out of pages but still not loaded — defensive fallback.
        self.pending_jump = None;
        self.loading_jump = false;
    }
}
```

### Stale-result handling

`UpdateSearchQuery` increments `search_request_id`. The DB job carries the request id; on completion, `SetMatchPositions` is dropped if its `request_id` does not match the current one. This mirrors the pattern already used for transcript page loads.

### Tool-call highlighting suppression

In the existing `build_display_items` / item construction path, when a search is active, `highlight_query` is passed to `MessageItemInit` but **not** to `ToolCallItemInit` / `ToolBurstItemInit` children. Concretely: the `tool_highlight_query` field carried to tool-call rows becomes `None` whenever `self.search_query.is_some()`.

Rationale: the search counter only counts message matches. Visually highlighting tool-call previews would create user-visible occurrences that are not counted, reproducing the very confusion this design exists to avoid.

When `search_query` returns to `None` (clear), highlighting in tool calls returns to its current behavior (driven by other flows that may reuse `tool_highlight_query`, e.g., the inspector preview).

## View layer

The existing floating `search-nav-bar` is preserved. Two additions:

1. **Spinner** — a `gtk::Spinner` placed inline to the left of the term label, bound to `loading_jump`. CSS reserves a fixed width to prevent layout shift on appear/disappear.
2. **Secondary counter** — a small dim label adjacent to the main "k / N" counter, visible only when `loading_jump` and the loaded-match count is less than `match_positions.len()`. Format: `"({loaded}/{total} chargés)"`. The loaded count is recomputed cheaply on each page-load arrival:

   ```rust
   fn loaded_match_count(&self) -> usize {
       self.match_positions
           .iter()
           .filter(|p| (p.item_index as usize) < self.loaded_count)
           .count()
   }
   ```

Prev/Next buttons gain `set_sensitive: !model.loading_jump` to prevent queueing of jumps.

## Edge cases

- **Empty match list** — bar shows "0 résultat", Prev/Next disabled, no jump triggered.
- **Query while session changing** — `SetSession` fully resets state; any in-flight DB job for the old session is dropped via `request_id` mismatch.
- **Page-load failure during a jump** — existing error toast path runs; we additionally clear `pending_jump` and `loading_jump`, leaving `current_match` unchanged so the user can retry.
- **All matches already loaded** — `loading_jump` never becomes true; the spinner and secondary counter never appear; behavior is indistinguishable from the current implementation for short sessions.
- **`has_more_messages` becomes false before reaching target** — defensive: clear `pending_jump`, log a warning. Should not happen if FTS positions are coherent with the transcript, but protects against schema drift.
- **Manual `LoadMore` click during a pending jump** — the jump resolves naturally on the next `apply_next_page_rows`; no special handling required.

## Tests

Database (added to existing integration test file or new file):

- `find_session_match_positions_returns_ordered_message_matches`
- `find_session_match_positions_filters_by_session`
- `find_session_match_positions_handles_invalid_query`
- `find_session_match_positions_empty_query`

Model (in `session_detail.rs` test module, leveraging the existing `SessionDetail::test_*` helpers):

- `update_search_query_populates_match_positions_and_jumps_to_first`
- `jump_to_loaded_match_scrolls_without_loading`
- `jump_to_unloaded_match_triggers_progressive_loading`
- `clear_search_resets_state_and_restores_tool_highlight`
- `stale_search_result_is_discarded`
- `prev_next_wrap_around`

Manual verification:

- `cargo fmt --all -- --check && cargo clippy --all -- -D warnings && cargo test --all --no-fail-fast`
- Run with `--sessions-dir tests/fixtures` on a long fixture session; type a query whose matches span multiple pages; verify counter, spinner, navigation correctness.
- Capture screenshots of the search nav bar in three states: idle / loading / resolved.

## Definition of done

- All new and existing tests pass.
- `cargo fmt`, `cargo clippy -D warnings`, `cargo test` all clean.
- Manual session-fixture run confirms: search counter is correct from t=0, spinner appears during cross-page jumps, tool-call rows are not highlighted while a search is active, clearing the search restores prior behavior.
- Updated screenshot of the search nav bar attached to the PR.
