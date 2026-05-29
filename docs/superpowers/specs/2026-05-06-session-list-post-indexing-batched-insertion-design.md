# SessionList Post-Indexing Batched Insertion - Design

**Date:** 2026-05-06  
**Status:** Implemented [#148](https://github.com/supermaciz/sessions-chronicle/pull/148)  
**Issue:** [#145](https://github.com/supermaciz/sessions-chronicle/issues/145)

## Context

The post-indexing reload instrumentation for issue 145 now shows a consistent bottleneck profile on a realistic local dataset:

- `fetch_ms` stays low (`9-15 ms`);
- `push_ms` stays low (`3 ms`);
- `clear_ms` is already expensive (`222-308 ms`);
- `next_idle_delay_ms` remains very large (`463-496 ms`);
- `total_reload_ms` remains high (`1216-1349 ms`).

This points away from database fetch time and away from raw `push_back` throughput. The current full clear-and-rebuild strategy appears to create enough GTK/main-loop pressure that the user sees a visible freeze after indexing completes.

The report in `docs/reports/2026-05-06-session-list-post-indexing-reload-instrumentation-report.md` recommends a narrow batched-insertion experiment as the next step.

## Hypothesis

This experiment is not trying to reduce raw `push_back` CPU time. The baseline already shows `push_ms=3 ms`, so splitting the direct push loop cannot explain a large win by itself.

The hypothesis is narrower: pushing 888 rows into a `gtk::ListBox`-backed factory in one main-loop turn triggers GTK realization, layout, allocation, and frame-clock work that is processed after the factory guard is dropped. That deferred GTK work is the likely source of the large `next_idle_delay_ms` value. By yielding between bounded insertion bursts, GTK may be able to amortize row realization/layout work across multiple main-loop turns instead of processing the full pressure spike after one monolithic rebuild.

This means the experiment can have two valid outcomes:

- perceived responsiveness improves because the main loop regains control sooner between batches;
- total wall-clock reload duration stays flat or increases because the work is spread across more idle turns.

For acceptance, responsiveness wins over absolute total duration. `next_idle_delay_ms` is the primary success metric; `total_reload_ms` is a secondary guardrail, not the sole pass/fail criterion.

## Goal

Reduce the user-visible end-of-indexing freeze by changing only the measured post-indexing `SessionList` reload path from one full repopulation pass to batched row insertion.

## Non-Goals

- No batching for search, manual filter changes, pin toggles, or ordinary `Reload` paths.
- No pagination, windowing, or virtualization in this change.
- No diff-based update engine.
- No query/index/database changes.
- No changes to `SessionDetail`.
- No persistent scheduling framework shared across unrelated components.

## Recommended Approach

Keep the existing synchronous fetch, but split the post-indexing list rebuild into small main-loop batches after the list has been cleared.

Only `SessionListMsg::ReloadAfterIndexing` uses this path. Other reload messages keep the current immediate behavior.

This deliberately leaves `clear_ms` untouched in the first experiment. `clear()` is expensive, but changing deletion strategy at the same time would make the result harder to interpret: a win could come from batched removal, batched insertion, or both. This design isolates batched insertion first. If the result is insufficient, batched clear/removal becomes the next minimal experiment before jumping to diffing or pagination.

The batcher should:

1. fetch all sessions synchronously with the current query;
2. clear the current factory immediately;
3. queue the fetched `SessionRowInit` items in component state;
4. append a bounded number of rows per idle callback;
5. restore selection only after the final batch completes;
6. reuse the current measurement event so the same report fields can be compared before and after the experiment.

This is the narrowest change that directly targets the measured pressure point without broadening issue 145 into a general `SessionList` rewrite.

Use a fixed initial batch size of `64` rows. At the measured scale of 888 rows, that yields roughly 14 insertion turns. Since baseline raw `push_ms` is only `3 ms`, the batch size is not chosen to reduce CPU loop time; it is chosen to create enough yield points for GTK/main-loop work without stretching the rebuild across dozens of tiny batches.

## Alternatives Considered

### Batch Every Reload Reason

Apply the same batcher to search, sidebar filters, pinning, and explicit reloads.

Trade-off: more consistent behavior, but wider behavioral change than the issue justifies. It would make regressions harder to attribute and would slow down interactive filter/search paths that were not identified as problematic.

### Diff-Based Incremental Update

Keep existing rows and compute additions/removals/reorders instead of clearing the list.

Trade-off: may ultimately be better than full clear/rebuild, but requires more state bookkeeping and a more complex correctness surface. It is too large for the next experiment after the current instrumentation report.

### Batch Clear Instead Of Push

Remove existing rows in chunks, then keep the current monolithic push pass.

Trade-off: this isolates the `clear_ms` cost, which is the second-largest measured phase. It is a valid follow-up if batched insertion does not reduce the post-drop idle delay enough. It is not the first experiment because the current user-visible symptom is dominated by post-drop idle delay, and batching insertion tests whether GTK row realization/layout pressure can be amortized without changing removal semantics.

### Pagination Or Windowing First

Reduce rendered row count structurally instead of changing insertion cadence.

Trade-off: valid long-term fallback if batching is insufficient, but larger in scope and more invasive to list semantics. The measured next step should stay smaller.

## Design

### Dedicated Pending Batch State

Keep the current `reload_sessions()` entry point, but add a small post-indexing-only pending batch state to `SessionList`, for example:

```rust
pending_post_indexing_batch: Option<PendingPostIndexingBatch>
```

`PendingPostIndexingBatch` should hold only what the component needs to finish one in-flight rebuild:

- queued `SessionRowInit` items still to be inserted;
- total row count;
- previously selected session ID;
- selection restoration flags accumulated until completion;
- the invalidation token for stale idle callbacks.

Only one post-indexing batch run may be active at a time. Starting a new measured reload invalidates the previous batch state and drops stale callbacks silently.

### Reload Flow

For ordinary reloads, preserve today’s behavior.

At the start of every ordinary reload path, explicitly cancel any in-flight post-indexing batch:

1. invalidate the batch token;
2. drop `pending_post_indexing_batch`;
3. continue with the current immediate reload implementation.

Cancellation must be explicit. A search/filter/pin reload that happens while a post-indexing batch is active must not race with stale callbacks that keep pushing old rows.

For `ReloadAfterIndexing` only:

1. select the current session ID before clearing, as today;
2. fetch the sessions synchronously, as today;
3. clear the factory immediately;
4. store all fetched rows into `pending_post_indexing_batch` instead of pushing them all at once;
5. schedule the first idle callback that inserts one batch;
6. each batch inserts at most `POST_INDEXING_RELOAD_BATCH_SIZE` rows, then either:
   - schedules the next idle callback if rows remain, or
   - finalizes selection restoration and emits the existing measurement summary when all rows are inserted.

The initial implementation should use a fixed batch size constant. A simple starting point is preferable to adaptive heuristics in this issue.

Use:

```rust
const POST_INDEXING_RELOAD_BATCH_SIZE: usize = 64;
```

### Batch Scheduling

Use main-loop idle callbacks so GTK can regain control between insertion bursts.

The callback contract should be:

- exit immediately if its invalidation token is stale;
- take the next bounded slice of pending rows;
- push those rows into the existing `FactoryVecDeque`;
- if rows remain, schedule one more idle callback and stop;
- if rows are exhausted, run the same selection restoration semantics as today.

This keeps the change local to `SessionList` and avoids introducing worker threads or background channels.

### Selection Restoration

Selection restoration behavior must remain unchanged from the user’s perspective:

- if the previously selected session still exists after the reload, select it;
- otherwise fall back to `ensure_selection()`;
- only perform this once, after the final batch is inserted.

Restoring selection before all rows are present would create unnecessary repeated work and could produce transient wrong selections while the list is incomplete.

If the user changes selection during the batched rebuild, the final deferred restoration must not overwrite that interaction. The implementation should track whether the list selection changed after the clear and before finalization. If it did, skip the deferred restore and preserve the user's current selection/focus state.

This can be implemented with a small boolean flag on the pending batch, set by selection-change handling while the batch is active. If that signal is not already wired in a usable place, use the minimum local signal connection needed to detect user-visible selection changes during the active batch.

### Visual Transition

The experiment accepts a visible trade-off: the list may briefly appear empty and then refill by batches instead of freezing and then appearing fully populated.

That is acceptable for this issue because the path is limited to indexing completion, which is not an explicit user-initiated search or filter action. If the batched refill is visually worse than the current freeze, the experiment should be considered a poor UX result even if the idle metric improves.

### Measurement Strategy

Keep the current `sessionlist.post_indexing_reload.measured` event name and field set so the current report remains directly comparable.

Do not redefine existing field semantics:

- `fetch_ms` remains the synchronous fetch duration before batching starts;
- `clear_ms` remains the synchronous factory clear duration;
- `push_ms` remains comparable to the old value by representing the sum of all batch push durations;
- `row_count` remains the total number of rows inserted;
- `next_idle_delay_ms` is measured from the end of the synchronous clear/setup phase until the next idle callback after control is yielded;
- `next_frame_delay_ms` is measured from the same post-setup point until the next frame callback;
- `total_reload_ms` measures from the start of the measured reload through the final post-batch summary point, after the last batch, selection handling, idle timing, and frame timing needed for the final summary.

Add new fields to make the batched path interpretable without changing existing field meaning:

- `batch_count`;
- `batch_size`;
- `max_batch_push_ms`;
- `total_batch_push_ms` as an explicit alias of the comparable `push_ms`;
- `user_selection_changed_during_batch`.

One or more `debug` events may be added for per-batch row counts and per-batch push duration, but the final `info` summary must remain the primary artifact.

The acceptance comparison is against the current measured baseline in the report, not against a new metric family.

### Error Handling And Cancellation

The batcher is best-effort and must not change user-visible semantics beyond responsiveness:

- if a new measured post-indexing reload starts while a previous batch run is still inserting rows, invalidate the previous run first;
- stale callbacks must return without mutating the list or logging a final summary;
- ordinary unmeasured reload paths must explicitly invalidate and drop any active post-indexing batch before replacing the list immediately.

No retries, no debounce logic, and no queue shared with unrelated UI work should be introduced here.

## Testing Strategy

Automated coverage should stay narrow and behavior-focused:

- deterministic unit test for the batch state transition helper used by `PendingPostIndexingBatch`;
- GTK test proving `ReloadAfterIndexing` still ends with the expected rows and preserved selection behavior;
- GTK test proving a second measured reload invalidates the first batch run;
- GTK test proving an ordinary reload cancels an active post-indexing batch before replacing the list;
- GTK test proving a user selection made during an active batch is not overwritten by final deferred restoration.

If GTK timing makes one of these tests impractical, replace it with a deterministic unit test around the extracted batch state transition helper. The replacement test must prove the same correction property: stale callbacks cannot mutate the list, ordinary reloads cancel the active batch, and user selection changes suppress deferred restoration.

Verification commands:

- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`

Manual verification remains required:

1. re-run the issue-145 logging protocol;
2. compare the new `next_idle_delay_ms`, `clear_ms`, and `total_reload_ms` against the current report;
3. confirm the end-of-indexing transition is visibly improved or unchanged.

## Acceptance Criteria

The experiment is successful when all of the following are true:

- batching is limited to `ReloadAfterIndexing`;
- ordinary reload behavior is unchanged;
- selection restoration semantics are preserved;
- the instrumentation remains directly comparable with the current report;
- sequential reruns reduce median `next_idle_delay_ms` by at least 40% versus the `469 ms` aggregate baseline, or below `250 ms`;
- median `total_reload_ms` does not exceed the `1301 ms` aggregate baseline by more than 25%, unless manual observation shows a clear responsiveness improvement and the report documents that trade-off explicitly;
- if no material improvement appears, the result is still useful and should guide the next narrower follow-up.

## Decision

- Proceed with a fixed-size, post-indexing-only batched insertion experiment.
- Keep fetch synchronous and keep all non-post-indexing reload paths unchanged.
- Use the existing instrumentation as the comparison framework.
- Treat this as a measured experiment, not as the final architecture for large `SessionList` updates.
