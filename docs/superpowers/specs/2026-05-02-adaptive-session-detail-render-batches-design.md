# Adaptive SessionDetail Render Batches - Design

## Context

GitHub issue #132 tracks a focused performance improvement for `SessionDetail`: replace fixed-size transcript render batches with adaptive time-budgeted batches.

The issue #127 instrumentation showed that the slow path is not SQLite. In the measured large Codex session, session metadata and first transcript page queries completed in roughly `1-3 ms`, while first transcript page factory push completion took roughly `7.8 s` and showed large `max_schedule_gap_ms` values. The likely dominant costs are GTK-side transcript row construction, Markdown or `TextView`-heavy row setup, factory push batching, and main-loop scheduling gaps.

`SessionDetail` currently queues prepared transcript rows in `PendingRenderBatch` and renders them through `FactoryVecDeque<TranscriptRow>`. Each scheduled render pass pushes up to `RENDER_BATCH_SIZE = 3` rows, then schedules the next pass with `glib::timeout_add_local_once` after `RENDER_BATCH_DELAY_MS = 16`.

GTK widgets and GObjects must remain on the main thread. Relm4 factories are still the right rendering mechanism for this scope; this design only changes how much work each main-loop render pass performs.

## Goal

Make transcript rendering smoother and more predictable by using elapsed main-loop time per batch instead of a fixed row count.

The user-approved target is a `6 ms` render-batch budget, implemented with the smallest safe change to the current pipeline.

## Non-Goals

- No full transcript virtualization.
- No `GtkListView` migration.
- No `AsyncComponent` or factory architecture replacement.
- No redesign of transcript rows, Markdown rendering, grouped tool calls, search navigation, or pagination.
- No database-side optimization.
- No off-main-thread GTK widget work.

## Recommended Approach

Use a minimal adaptive batching loop inside `SessionDetail::render_next_transcript_batch`.

Replace the fixed `for _ in 0..RENDER_BATCH_SIZE` loop with a loop that:

- always pushes at least one row when rows remain;
- continues pushing while elapsed push time stays below `RENDER_BATCH_BUDGET_MS = 6`;
- stops at `RENDER_BATCH_MAX_ITEMS = 8`, so very cheap rows cannot monopolize one render pass;
- preserves the current `PendingRenderBatch`, `FactoryVecDeque`, request invalidation, scheduling delay, grouped tool call handling, search jump continuation, and render metrics.

This directly addresses issue #132 while keeping the diff small and low-risk.

## Alternatives Considered

### Strict scheduler redesign

Add a richer scheduler with dynamic delays, moving averages, and automatic tuning after slow rows.

Trade-off: this could improve long-term control, but it would add state and behavior that is not necessary for #132. It also risks making search, pagination, and stale request handling harder to reason about.

### Off-main-thread preparation

Move more transcript row preparation into a Relm4 command, worker, or blocking task before rows are pushed into the factory.

Trade-off: this may become valuable later, but GTK widgets and GObjects cannot leave the main thread. The current evidence points first to render scheduling and factory/widget work, so off-main-thread preparation should be a follow-up exploration after #132 provides a better baseline.

## Rendering Rules

The adaptive batch loop should follow these rules:

1. Record `push_started_at = Instant::now()` before acquiring or using the factory guard.
2. Pop and push one item if available.
3. After the first item, stop when either elapsed push time is greater than or equal to `6 ms`, the batch reaches `RENDER_BATCH_MAX_ITEMS = 8`, or there are no pending items.
4. Keep existing progress counters: `rendered_this_batch`, `rendered_items`, `batch_count`, `remaining_items`, `total_push_duration`, `max_push_duration`, and `max_schedule_gap`.
5. Keep scheduling the next batch with the current `RENDER_BATCH_DELAY_MS` while items remain.
6. Keep final page completion behavior unchanged when no items remain.

The budget is not a hard guarantee for pathological rows. A single expensive row may exceed `6 ms`, but the loop must still make progress by pushing at least one row per pass.

## Metrics And Logging

Keep the existing issue #127 metrics and add enough fields to make the new behavior visible.

For per-batch logs, include:

- `rendered_this_batch`;
- `batch_budget_ms`;
- `batch_max_items`;
- `push_duration_ms`;
- `schedule_gap_ms`;
- `remaining_items`;
- a boolean such as `budget_exceeded` when push duration is at least the budget.

For final page logs, keep:

- `batch_count`;
- `total_push_duration_ms`;
- `max_push_duration_ms`;
- `total_duration_ms`;
- `max_schedule_gap_ms`;
- row-kind counts;
- first-page load-to-factory-push timing.

No transcript content, tool call payload, command output, or Markdown body should be logged.

## Expected Behavior

Large transcript rendering should stay more responsive because each scheduled render pass performs work bounded by elapsed time rather than by an arbitrary row count.

Cheap rows may render more than three at a time, improving throughput. Expensive rows should cause smaller batches, improving main-loop fairness.

Search-driven reloads, grouped tool call bursts, pagination, and pending search jumps should continue to behave as they do today because the surrounding render pipeline is unchanged.

## Testing Strategy

Automated checks should focus on preserving behavior and compile safety:

- `cargo fmt --all -- --check`;
- `cargo test session_detail::tests -- --nocapture`;
- `cargo test --all --no-fail-fast` if the edit touches shared transcript row behavior or broader UI code.

Manual verification should compare logs before and after the change on a large real-world session or representative fixture:

- open a large session with no active search query;
- load more transcript rows;
- run a search that jumps beyond the first page;
- inspect logs for `rendered_this_batch`, `push_duration_ms`, `batch_budget_ms`, `budget_exceeded`, `max_push_duration_ms`, and `max_schedule_gap_ms`.

## Acceptance Criteria

- Fixed `RENDER_BATCH_SIZE` row-count batching is replaced by adaptive time-budgeted batching.
- The render budget is `6 ms` per batch.
- Each non-empty render pass pushes at least one row.
- `RENDER_BATCH_MAX_ITEMS = 8` prevents unbounded row pushes in one pass.
- Existing pagination, grouped tool calls, search highlighting, and search jump behavior remain correct.
- Logs or render metrics make batch size and budget behavior visible.
- GTK widgets, GObjects, and factory mutations remain on the main thread.
- No transcript content or sensitive user data is added to logs.

## Follow-Up

A separate follow-up issue should explore off-main-thread transcript data preparation after #132 lands. That follow-up should use #132 as the baseline and only move owned, UI-safe data preparation out of the main thread while keeping GTK widget creation and factory mutation on the main thread.
