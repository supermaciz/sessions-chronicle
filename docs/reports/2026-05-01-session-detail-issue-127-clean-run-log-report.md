# Session Detail Issue 127 Clean Run Log Report

## Scope

- Date: 2026-05-01
- Target session: `019dc51a-f0cd-79c1-ba79-45fedac889c2`
- AI assistant: Codex
- Source log file: `/tmp/sessions-chronicle-issue-127.log`
- Relevant design: `docs/superpowers/specs/2026-05-01-session-detail-responsiveness-instrumentation-design.md`

## Scenario

This run was captured specifically to remove the main source of noise from the earlier manual verification.

The workflow was:

1. Launch the app with a fresh redirected log file.
2. Wait for background indexing to complete.
3. Click directly on session `019dc51a-f0cd-79c1-ba79-45fedac889c2`.
4. Do not perform any search before opening the session detail view.

The application was launched with:

```bash
RUST_LOG=info,sessions_chronicle=debug sessions-chronicle > /tmp/sessions-chronicle-issue-127.log 2>&1
```

## Cleanliness Checks

This run is materially cleaner than the previous one.

### Indexing finished before the click

The app logged:

- `Background indexing complete: indexed=1238, skipped=687, removed=0`

Relevant log line:

- `/tmp/sessions-chronicle-issue-127.log:2983`

### The detail view opened with no active search query

The `SetSession` payload shows:

- `search_query: None`

Relevant log lines:

- `/tmp/sessions-chronicle-issue-127.log:3028-3031`

### No search-driven reload noise was present

This capture does not show the extra paths seen in the previous run:

- no `SearchPositionsLoaded`
- no `SetMatchPositions`
- no `Ignoring stale transcript page`
- no `Session detail search update started` around the open sequence

That makes this run suitable as the baseline manual verification for the pure session-open path.

## Key Measurements

### 1. Session metadata loading was negligible

When the session was selected, the app logged:

- `load_session took 730.509µs`

Relevant log lines:

- `/tmp/sessions-chronicle-issue-127.log:3018-3023`

### 2. First transcript page loading was also negligible

The first transcript page load logged:

- `load_duration_ms=2`

Relevant log lines:

- `/tmp/sessions-chronicle-issue-127.log:3044-3046`

### 3. First-page preparation remained effectively free

The preparation step logged:

- `Prepared first transcript page`
- `display_item_count=33`
- `build_duration_ms=0`

Relevant log line:

- `/tmp/sessions-chronicle-issue-127.log:3048`

### 4. The dominant delay was still in render completion

The key issue-127 metric logged:

- `First transcript page factory push complete`

Recorded values:

- `request_id=1`
- `source_row_count=75`
- `display_item_count=33`
- `batch_count=11`
- `total_push_duration_ms=1`
- `max_push_duration_ms=0`
- `total_duration_ms=7558`
- `max_schedule_gap_ms=1398`
- `first_page_load_to_factory_push_ms=7875`

Relevant log line:

- `/tmp/sessions-chronicle-issue-127.log:3158`

## Timeline Summary

From the log sequence:

- `Session detail open started` at line `3031`
- deferred load accepted at line `3041`
- first page loaded at line `3046`
- first page prepared at line `3048`
- first page factory push complete at line `3158`

This means:

- session metadata lookup was sub-millisecond;
- first-page transcript fetch was 2 ms;
- first-page preparation was effectively 0 ms;
- the end-to-end wait to first factory-push completion was about 7.9 seconds.

## Interpretation

This clean run confirms the core performance diagnosis from issue 127.

The user-visible delay is not explained by:

- loading session metadata;
- querying the first transcript page;
- preparing the first display-item batch.

Instead, the delay is concentrated between the moment the first page is already available and the moment the row factory finishes receiving the first page worth of items.

The two strongest signals are:

- `first_page_load_to_factory_push_ms=7875`
- `max_schedule_gap_ms=1398`

This strongly suggests that the bottleneck is in the incremental render pipeline and main-loop scheduling behavior, not in the database layer.

## Comparison With the Earlier Noisy Run

Compared with the earlier capture, this run is more trustworthy for the pure open path because it removes search-driven reload interference.

Even with that noise removed, the headline result is effectively unchanged:

- database work remains tiny;
- first-page fetch remains tiny;
- first factory push completion still lands around 7.5 to 8 seconds.

That consistency makes the diagnosis much stronger.

## Recommended Next Steps

1. Use this clean run as the reference manual verification artifact for issue 127.
2. Keep future measurements on the same session so numbers remain comparable.
3. Focus the next optimization pass on:
   - render batch scheduling,
   - expensive transcript row widget construction,
   - Markdown or `TextView` heavy rows,
   - any main-loop starvation between queued batches.
4. Do not spend time on database optimization for this issue unless a different session produces contradictory evidence.

## Summary

For the clean direct-open run of session `019dc51a-f0cd-79c1-ba79-45fedac889c2`, the logs show:

- `load_session took 730.509µs`
- first transcript page load took `2 ms`
- first-page preparation took `0 ms`
- first transcript page factory push completion took about `7.9 s`
- maximum observed scheduling gap was about `1.4 s`

This is a clean confirmation that the remaining responsiveness problem in `SessionDetail` is render-pipeline bound rather than database bound.
