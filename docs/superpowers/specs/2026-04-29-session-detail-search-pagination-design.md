# Session Detail Search — Pagination-Aware Navigation

**Date:** 2026-04-29  
**Status:** Accepted (revised after code review)

## Problem

In `SessionDetail`, the in-session search only operates on transcript content currently loaded via the `LoadMore` paginator. For long sessions, matches living in unloaded pages are invisible to the user: the count is wrong, navigation cannot reach them, and there is no signal that anything is missing. The search silently lies.

The fix needs to keep the perf benefits of pagination (large sessions still load incrementally) while giving the user a search that is honest about totals and able to reach every match.

## Scope

In scope:
- A new pagination-aware search flow in `src/ui/session_detail.rs` that uses the SQLite FTS5 index (`messages_fts`) as the source of truth for matching transcript items and match ordering.
- Contiguous progressive loading: when the user navigates to a match in an unloaded page, intermediate pages are loaded silently and in order.
- Visual feedback for in-progress jumps in the existing floating search nav bar (spinner + secondary "loaded / total" counter).
- Restricting inline search highlighting to FTS-matching message rows while a search is active, so tool-call previews and non-matching message rows cannot show highlights that the counter does not count.

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
5. **Per-item match counter.**  The bar shows "k of N matching items", not "occurrence k of N occurrences". Inline highlighting is illustrative inside matching message rows; it is not used for counting or navigation. Counter does not drift during loading.
6. **Highlighting is constrained to counted rows.**  When a search is active, `highlight_query` is only passed to message rows whose `transcript_items.item_index` is present in `match_positions`. It is not passed to tool-call rows, tool bursts, subagents, or non-matching message rows. This avoids visible highlights outside the counted item set.

## Architecture

Three layers of change:

- **Database layer (`src/database/`)** — new query function `find_session_match_positions` returning the ordered list of matching items in a session.
- **Model layer (`src/ui/session_detail.rs`)** — new state `match_positions`, `display_targets_by_item_index`, `pending_jump`, `loading_jump`; old per-row match accounting (`match_segments`, `total_matches`) removed.
- **View layer (search nav bar in the same file)** — spinner + secondary loaded counter; Prev/Next disabled during a jump.

No new files. No new modules.

`transcript_items.item_index` is not a display-row index. The transcript view groups adjacent tool calls into burst rows, so display indexes can diverge from source item indexes. The detail view must therefore maintain an explicit mapping from each loaded `transcript_items.item_index` to the rendered `ScrollTarget` used by the existing scroll code.

## Data flow

```
User types query
   │
   ▼
UpdateSearchQuery(Some(q))
   │   clears current match state, spawns DB job (request_id N)
   ▼
find_session_match_positions(session_id, q)
   │   returns Vec<MatchPosition> ordered by item_index
   ▼
SetMatchPositions { request_id, session_id, positions }
   │   if request_id/session_id match current → store, current_match=0
   │   reload first transcript page so row highlights use the accepted FTS result set
   ▼
If positions is non-empty, trigger jump_to(0)
   │
   ├── target display row already rendered → scroll_to_item, done
   │
   └── otherwise:
         loading_jump = true
         pending_jump = Some(0)
         if item_index is not loaded → load next page
         if item_index is loaded but not rendered → wait for render batch completion
         repeat until target display row exists
         scroll_to_item, loading_jump = false
```

Subsequent Prev/Next reuse the same `jump_to(i)` path.

## Database layer

### Query

```rust
pub struct MatchPosition {
    pub item_index: i64,
}

pub fn find_session_match_positions(
    db: &Connection,
    session_id: &str,
    query: &str,
) -> Result<Vec<MatchPosition>>;
```

Implementation: a single SQL query joining `messages_fts` (for the MATCH filter) to `messages` (to filter by `session_id`) to `transcript_items` (to retrieve the canonical source `item_index` ordering used by pagination and target resolution).

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

The query uses only message rows because `messages_fts` indexes `messages.content`. No forward-compatibility enum is introduced; future tool-call or subagent support should add new types only when that scope is accepted.

Sanitization of the query string reuses the existing global-search behavior: try the raw FTS query first, then retry with a sanitized `token AND token` query if SQLite rejects the raw query. If sanitization produces no tokens, or the sanitized retry also fails, return `Ok(vec![])`. The in-session search must never surface an FTS syntax error to the UI.

### No migration

`messages_fts` already exists (since schema v13). No new tables, no new triggers, no new indices. The existing `idx_transcript_items_session` (or equivalent ordering on `(session_id, item_index)`) is sufficient.

### Tests

Integration tests in `tests/`:

- `find_session_match_positions_returns_ordered_message_matches` — fixture with three matching messages at non-contiguous `item_index`; assert order.
- `find_session_match_positions_filters_by_session` — two sessions both containing the query; assert only the requested session is returned.
- `find_session_match_positions_retries_sanitized_invalid_query` — malformed raw FTS query such as `"alpha` returns matches for sanitized `alpha`, matching global search behavior.
- `find_session_match_positions_invalid_punctuation_only_query` — query containing only invalid punctuation returns `Ok(vec![])`, never errors.
- `find_session_match_positions_empty_query` — empty / whitespace query returns `Ok(vec![])`.

## Model layer

### State changes in `SessionDetailModel`

Added:

```rust
match_positions: Vec<MatchPosition>,
display_targets_by_item_index: BTreeMap<i64, ScrollTarget>,
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
    session_id: String,
    positions: Vec<MatchPosition>,
}
```

Existing messages that change behavior:

- `UpdateSearchQuery(query)` — bumps `search_request_id`, clears `match_positions`, clears `display_targets_by_item_index` when the transcript is reloaded, spawns a DB job for the FTS query, then on result emits `SetMatchPositions`. It does not rebuild rows with the new query until accepted FTS positions are available, so row highlighting can be restricted to the accepted result set. Resets `current_match`, `pending_jump`, `loading_jump`.
- `ClearSearch` — clears `match_positions`, cancels `pending_jump`, resets `loading_jump`, reloads the current session, and restores the previous non-search highlight behavior.
- `PrevMatch` / `NextMatch` — now operate on `match_positions` index (with wraparound) and route through the jump path.
- `LoadMore` (manual button click) — unchanged for direct user clicks. Pending search jumps do not chain from `apply_next_page_rows`; they continue only after the relevant render batch has completed, so scroll targets are guaranteed to exist in the factory.

### Display target mapping

The model maintains a mapping from source transcript item indexes to display targets:

```rust
display_targets_by_item_index: BTreeMap<i64, ScrollTarget>
```

This map is rebuilt incrementally when display items are prepared:

- `DisplayTranscriptItem::Single(row)` maps `row.item_index` to `ScrollTarget { display_index, child_index: None }`.
- `DisplayTranscriptItem::ToolBurst(burst)` maps each child row's `item_index` to `ScrollTarget { display_index, child_index: Some(child_offset) }`.
- When boundary regrouping replaces the previous tail item, mappings for replaced source rows are removed before inserting replacement mappings.
- On first-page reload, session change, clear, or navigation back, the map is cleared with the transcript rows.

Although current search matches only message rows, this mapping is still required because earlier tool bursts can make source `item_index` diverge from factory `display_index`.

### Jump logic

```rust
fn jump_to(&mut self, target: usize, sender: &ComponentSender<Self>) {
    if self.match_positions.get(target).is_none() {
        return;
    }

    self.current_match = target;
    self.pending_jump = Some(target);
    self.loading_jump = true;
    self.continue_pending_jump(sender);
}

fn continue_pending_jump(&mut self, sender: &ComponentSender<Self>) {
    let Some(target) = self.pending_jump else { return };
    let Some(pos) = self.match_positions.get(target) else {
        self.pending_jump = None;
        self.loading_jump = false;
        return;
    };

    if self.loading_first_page || self.loading_next_page || self.pending_render_batch.is_some() {
        return;
    }

    if let Some(scroll_target) = self.display_targets_by_item_index.get(&pos.item_index).copied()
        && self.messages.len() > scroll_target.display_index
    {
        self.pending_jump = None;
        self.loading_jump = false;
        self.scroll_to_item.set(Some(scroll_target));
    } else if (pos.item_index as usize) >= self.loaded_count && self.has_more_messages {
        self.load_next_page(sender);
    } else if !self.has_more_messages && (pos.item_index as usize) >= self.loaded_count {
        tracing::warn!(
            item_index = pos.item_index,
            loaded_count = self.loaded_count,
            "search match position is outside loaded transcript range"
        );
        self.pending_jump = None;
        self.loading_jump = false;
    }
}
```

`continue_pending_jump` is called from these points:

- after `SetMatchPositions` stores fresh positions;
- after `apply_first_page_rows` queues the first render batch;
- after `apply_next_page_rows` queues a later render batch, only when no render batch was needed;
- after `render_next_transcript_batch` completes the final batch for a page.

`SetMatchPositions` stores the accepted FTS positions, triggers the first-page transcript reload, and then calls `jump_to(0)`. `continue_pending_jump` returns while `loading_first_page` is true, then resumes from the first-page load/render completion hooks.

The important invariant: never set `scroll_to_item` until the target display row exists in the factory. Loaded source rows are not enough, because rendering is batched.

When the target is still unloaded, `continue_pending_jump` requests exactly one page. The next continuation waits until that page is rendered before deciding whether to scroll or load another page. This avoids calling `load_next_page` while `pending_render_batch.is_some()`, which the current component explicitly rejects.

On page-load failure during a pending jump:

```rust
self.pending_jump = None;
self.loading_jump = false;
// Existing error toast/logging path still runs.
```

### Stale-result handling

`UpdateSearchQuery` increments `search_request_id`. The DB job carries both the request id and session id; on completion, `SetMatchPositions` is dropped unless both still match the active model. This mirrors the pattern already used for transcript page loads and protects query-while-session-changing cases.

### Search highlighting constraints

In the existing `build_display_items` / item construction path, when a search is active, `highlight_query` is passed only to `MessageItemInit` rows whose source `transcript_items.item_index` is in `match_positions`.

It is not passed to `ToolCallItemInit` / `ToolBurstItemInit` children, subagent rows, or message rows that FTS did not match. Concretely: the tool-call `highlight_query` field becomes `None` whenever `self.search_query.is_some()`.

Rationale: the search counter counts FTS-matching message items. The current inline highlighter is a literal case-insensitive substring highlighter, while FTS has token/query syntax. Therefore inline highlights are a visual aid inside counted rows, not the source of truth for counts. Restricting highlights to counted rows prevents visible matches outside the counted set.

When `search_query` returns to `None` (clear), highlighting in tool calls returns to its current behavior (driven by other flows that may reuse `tool_highlight_query`, e.g., the inspector preview).

## View layer

The existing floating `search-nav-bar` is preserved. Two additions:

1. **Spinner** — a `gtk::Spinner` placed inline to the left of the term label, bound to `loading_jump`. CSS reserves a fixed width to prevent layout shift on appear/disappear.
2. **Secondary counter** — a small dim label adjacent to the main "k / N" counter, visible only when `loading_jump` and the loaded-match count is less than `match_positions.len()`. Format: `"({loaded}/{total} chargés)"`. The loaded count is recomputed cheaply from the display-target map:

   ```rust
   fn loaded_match_count(&self) -> usize {
       self.match_positions
           .iter()
           .filter(|p| self.display_targets_by_item_index.contains_key(&p.item_index))
           .count()
   }
   ```

Prev/Next buttons gain `set_sensitive: !model.loading_jump` to prevent queueing of jumps.

## Edge cases

- **Empty match list** — bar shows "0 résultat", Prev/Next disabled, no jump triggered.
- **Query while session changing** — `SetSession` fully resets state; any in-flight DB job for the old session is dropped via `request_id` or `session_id` mismatch.
- **Page-load failure during a jump** — existing error toast path runs; we additionally clear `pending_jump` and `loading_jump`, leaving `current_match` unchanged so the user can retry.
- **All matches already rendered** — `loading_jump` clears immediately; the spinner and secondary counter never appear; behavior is indistinguishable from the current implementation for short sessions.
- **`has_more_messages` becomes false before reaching target** — defensive: clear `pending_jump`, log a warning. Should not happen if FTS positions are coherent with the transcript, but protects against schema drift.
- **Manual `LoadMore` click during a pending jump** — the jump resolves naturally when the manually loaded page finishes rendering; no special handling required.

## Tests

Database (added to existing integration test file or new file):

- `find_session_match_positions_returns_ordered_message_matches`
- `find_session_match_positions_filters_by_session`
- `find_session_match_positions_retries_sanitized_invalid_query`
- `find_session_match_positions_invalid_punctuation_only_query`
- `find_session_match_positions_empty_query`

Model (in `session_detail.rs` test module, leveraging the existing `SessionDetail::test_*` helpers):

- `update_search_query_populates_match_positions_and_jumps_to_first`
- `jump_to_loaded_match_scrolls_without_loading`
- `jump_to_loaded_but_unrendered_match_waits_for_render_batch`
- `jump_to_unloaded_match_triggers_progressive_loading_after_each_render_batch`
- `display_target_mapping_handles_grouped_tool_bursts_before_message_matches`
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
