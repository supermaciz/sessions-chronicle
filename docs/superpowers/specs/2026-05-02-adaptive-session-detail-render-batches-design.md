# Adaptive SessionDetail Render Batches - Design

## Context

GitHub issue #132 tracks a focused performance improvement for `SessionDetail`: replace fixed-size transcript render batches with adaptive time-budgeted batches.

The issue #127 instrumentation showed that the slow path is not SQLite. In the measured large Codex session, session metadata and first transcript page queries completed in roughly `1-3 ms`, while first transcript page factory push completion took roughly `7.8 s` and showed large `max_schedule_gap_ms` values. The likely dominant costs are GTK-side transcript row construction, Markdown or `TextView`-heavy row setup, factory push batching, and main-loop scheduling gaps.

`SessionDetail` currently queues prepared transcript rows in `PendingRenderBatch` and renders them through `FactoryVecDeque<TranscriptRow>`. Each scheduled render pass pushes up to `RENDER_BATCH_SIZE = 3` rows, then schedules the next pass with `glib::timeout_add_local_once` after `RENDER_BATCH_DELAY_MS = 16`.

GTK widgets and GObjects must remain on the main thread. Relm4 factories are still the right rendering mechanism for this scope; this design only changes how much work each main-loop render pass performs.

## Goal

Reduce wasted scheduling overhead on cheap rows by replacing the fixed `RENDER_BATCH_SIZE = 3` cap with an elapsed-time budget per batch.

This is a throughput improvement for the common case where transcript rows are inexpensive to enqueue. With 75 inexpensive rows on the first page, the current code performs `25` scheduled passes, each separated by `RENDER_BATCH_DELAY_MS = 16 ms`, adding roughly `400 ms` of pure scheduling delay before factory push completes. Allowing more rows per pass when push duration stays under budget cuts that scheduling tax.

This change is **not expected to materially help** sessions whose dominant cost is per-row widget construction or Markdown/`TextView` layout. When a single row already costs more than the budget, the loop falls back to "push one row, schedule next pass" — identical in shape to today. Reducing per-row cost or moving widget layout off the critical path is a separate, larger effort tracked under the follow-up.

The user-approved budget target is `6 ms`, implemented with the smallest safe change to the current pipeline.

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

### Choice Of Constants

`RENDER_BATCH_BUDGET_MS = 6` is roughly one-third of a `60 Hz` frame (`16.6 ms`). It leaves headroom for the rest of the main loop work (input, animation, layout fallout from the previous push) within the same frame, while being large enough that several cheap row pushes fit in one pass. Tighter values (e.g. `3 ms`) gave little additional fairness in informal experimentation and risked making the loop indistinguishable from the current size-3 behavior on machines where each push already costs `~1 ms`.

`RENDER_BATCH_MAX_ITEMS = 8` is a defensive ceiling on top of the time budget, not a target. With `INITIAL_PAGE_SIZE = 75`, it caps the first page at no fewer than `⌈75 / 8⌉ = 10` scheduled passes, preserving at least nine `RENDER_BATCH_DELAY_MS` yield points within the page so input and animation still get scheduling slots even when every row is essentially free.

### Measurement Caveat

The budget is measured as elapsed time around `guard.push_back()` calls inside `render_next_transcript_batch`. This captures factory enqueue cost but **not** the widget construction, Pango layout, or `GtkTextView` setup that Relm4 applies after `drop(guard)` and after the callback returns to the main loop. In other words, `push_duration_ms` is a lower bound on the real per-pass main-loop cost.

The honest ground-truth signal for main-loop fairness is `schedule_gap_ms`, the wall-clock delay between successive `render_next_transcript_batch` invocations, which is already tracked. This design intentionally controls on `push_duration_ms` because it is the cost the loop can directly attribute to its own work; reacting to `schedule_gap_ms` (e.g. shrinking the next batch when the previous gap exceeded a threshold) is a deliberate next iteration, kept out of scope here to preserve a small, low-risk diff.

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
- `push_duration_ms`;
- `schedule_gap_ms`;
- `remaining_items`;
- a boolean such as `budget_exceeded` when push duration is at least the budget.

`batch_budget_ms` and `batch_max_items` are constants for the duration of a session and should not be repeated on every per-batch log line. Include them once, either on the first per-page log or in the final-page summary, to keep batch logs readable.

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

Cheap rows may render up to `RENDER_BATCH_MAX_ITEMS = 8` per pass instead of `3`, reducing the number of `RENDER_BATCH_DELAY_MS` yields between page start and final factory push. On a `75`-row first page where every push completes well under budget, this should drop `batch_count` from `25` to roughly `10` and trim the scheduling tax visible in `total_duration_ms`.

Expensive rows produce a batch of size `1` and behave the same as today; the time budget does not retroactively shrink the cost of a row already pushed. Per-pass latency for those rows is unchanged, and the only observable difference is that `budget_exceeded = true` will appear in the per-batch log.

Search-driven reloads, grouped tool call bursts, pagination, and pending search jumps should continue to behave as they do today because the surrounding render pipeline is unchanged.

## Testing Strategy

### Existing Tests To Update

The first-page render test in `src/ui/session_detail.rs` currently asserts `metrics.batch_count == INITIAL_PAGE_SIZE.div_ceil(RENDER_BATCH_SIZE)` (around line `2880`). Under adaptive batching this exact equality no longer holds, because cheap rows will pack into fewer passes. Replace it with a range invariant of the form:

```text
⌈INITIAL_PAGE_SIZE / RENDER_BATCH_MAX_ITEMS⌉  ≤  metrics.batch_count  ≤  INITIAL_PAGE_SIZE
```

This keeps the test useful (every pass renders at least one row, no pass exceeds the cap) without coupling it to the exact constants.

### New Test

Add a unit test that proves the at-least-one-row-per-pass invariant when the budget is already exceeded entering the loop. The simplest formulation is to either:

- exercise the loop with `RENDER_BATCH_BUDGET_MS = 0` (or a test-only zero-budget shim), and assert that with `N` queued items the render produces `N` batches each with `rendered_this_batch == 1`; or
- run with the real budget against a synthesized list of items where the first push takes longer than the budget (e.g. a deliberately slow `TranscriptItemInit` variant in test scope), and assert that subsequent passes still each push exactly one row until drained.

Whichever formulation is cleaner against the test scaffolding wins; the property to lock down is "budget exhaustion never starves rendering."

### Suite Runs

- `cargo fmt --all -- --check`;
- `cargo test session_detail::tests -- --nocapture`;
- `cargo test --all --no-fail-fast` if the edit touches shared transcript row behavior or broader UI code.

### Manual Verification

Compare logs before and after the change on a large real-world session or representative fixture:

- open a large session with no active search query;
- load more transcript rows;
- run a search that jumps beyond the first page;
- inspect logs for `rendered_this_batch`, `push_duration_ms`, `budget_exceeded`, `max_push_duration_ms`, and `max_schedule_gap_ms`. Pay particular attention to whether `max_schedule_gap_ms` actually drops; if it does not, the `push_duration_ms` measurement is missing real cost (see Measurement Caveat) and the follow-up should consider gating on `schedule_gap_ms` directly.

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

Two follow-up issues should be opened after #132 lands, using its post-merge metrics as the baseline:

1. **Schedule-gap-driven batching.** If `max_schedule_gap_ms` does not improve in line with the reduction in `batch_count`, the `push_duration_ms` budget is under-measuring the real per-pass cost (widget construction, Pango layout, `TextView` setup happen after `drop(guard)`). The natural next iteration is to feed `schedule_gap_ms` from the previous pass back into the next pass, e.g. shrinking `MAX_ITEMS` to `1` after a gap above some threshold.

2. **Off-main-thread data preparation.** Move only owned, UI-safe data preparation off the main thread, while keeping GTK widget creation and factory mutation on it. The concrete candidates worth investigating are `SessionDetail::build_display_items` and the matched-index computation used for search highlighting; both are pure transformations on owned data and do not touch `GObject`. Markdown parsing, `GtkTextView` setup, and any `gtk::*` widget construction must stay on the main thread.
