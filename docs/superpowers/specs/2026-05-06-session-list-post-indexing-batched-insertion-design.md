# SessionList Post-Indexing Batched Insertion - Design

**Date:** 2026-05-06  
**Status:** Proposed  
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

The batcher should:

1. fetch all sessions synchronously with the current query;
2. clear the current factory immediately;
3. queue the fetched `SessionRowInit` items in component state;
4. append a bounded number of rows per idle callback;
5. restore selection only after the final batch completes;
6. reuse the current measurement event so the same report fields can be compared before and after the experiment.

This is the narrowest change that directly targets the measured pressure point without broadening issue 145 into a general `SessionList` rewrite.

## Alternatives Considered

### Batch Every Reload Reason

Apply the same batcher to search, sidebar filters, pinning, and explicit reloads.

Trade-off: more consistent behavior, but wider behavioral change than the issue justifies. It would make regressions harder to attribute and would slow down interactive filter/search paths that were not identified as problematic.

### Diff-Based Incremental Update

Keep existing rows and compute additions/removals/reorders instead of clearing the list.

Trade-off: may ultimately be better than full clear/rebuild, but requires more state bookkeeping and a more complex correctness surface. It is too large for the next experiment after the current instrumentation report.

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

For `ReloadAfterIndexing` only:

1. select the current session ID before clearing, as today;
2. fetch the sessions synchronously, as today;
3. clear the factory immediately;
4. store all fetched rows into `pending_post_indexing_batch` instead of pushing them all at once;
5. schedule the first idle callback that inserts one batch;
6. each batch inserts at most `N` rows, then either:
   - schedules the next idle callback if rows remain, or
   - finalizes selection restoration and emits the existing measurement summary when all rows are inserted.

The initial implementation should use a fixed batch size constant. A simple starting point is preferable to adaptive heuristics in this issue.

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

### Measurement Strategy

Keep the current `sessionlist.post_indexing_reload.measured` event name and field set so the current report remains directly comparable.

Interpretation changes for the experiment:

- `push_ms` should describe only the synchronous work done before the first batch is handed back to the main loop;
- the overall win or loss should be judged mainly by `next_idle_delay_ms` and `total_reload_ms`;
- if useful, one or more `debug` events may be added for per-batch row counts and per-batch push duration, but the final `info` summary must remain the primary artifact.

The acceptance comparison is against the current measured baseline in the report, not against a new metric family.

### Error Handling And Cancellation

The batcher is best-effort and must not change user-visible semantics beyond responsiveness:

- if a new measured post-indexing reload starts while a previous batch run is still inserting rows, invalidate the previous run first;
- stale callbacks must return without mutating the list or logging a final summary;
- ordinary unmeasured reload paths may continue to replace the list immediately, which implicitly cancels any stale post-indexing batch run.

No retries, no debounce logic, and no queue shared with unrelated UI work should be introduced here.

## Testing Strategy

Automated coverage should stay narrow and behavior-focused:

- unit test for batch invalidation/cancellation helper if a helper type is extracted;
- GTK test proving `ReloadAfterIndexing` still ends with the expected rows and preserved selection behavior;
- GTK test proving a second measured reload invalidates the first batch run if this can be expressed reliably in component tests.

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
- sequential reruns show a material reduction in `next_idle_delay_ms` or `total_reload_ms` versus the current baseline;
- if no material improvement appears, the result is still useful and should guide the next narrower follow-up.

## Decision

- Proceed with a fixed-size, post-indexing-only batched insertion experiment.
- Keep fetch synchronous and keep all non-post-indexing reload paths unchanged.
- Use the existing instrumentation as the comparison framework.
- Treat this as a measured experiment, not as the final architecture for large `SessionList` updates.
