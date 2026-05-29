# Session Detail Issue 143 Lazy Row Hydration Design

**Status:** Superseded — TypedListView migration [#152](https://github.com/supermaciz/sessions-chronicle/pull/152) replaced this approach

## Context

GitHub issue #143 follows the #142 investigation into large transcript opens. The #142 reference Codex session measured a baseline median `max_schedule_gap_ms` of `1346 ms`. Replacing transcript rows with minimal labels reduced the median to `21 ms`, while bypassing Markdown rendering and syntax highlighting did not improve the result.

The bottleneck is therefore GTK transcript row realization and layout volume after rows are pushed into the factory, not SQLite loading, Rust row preparation, Markdown parsing, or syntax highlighting work.

`SessionDetail` currently renders transcript pages through `FactoryVecDeque<TranscriptRow>` inside a `gtk::Box` within a `gtk::ScrolledWindow`. This design keeps that architecture for v1 and defers heavy row content inside each row.

## Goals

- Keep opening the #142 reference benchmark session at or below `50 ms` median `max_schedule_gap_ms`.
- Make initially visible transcript content readable within `200 ms` of opening the session.
- Populate deferred rows progressively while scrolling, without blank rows persisting after scrolling stops.
- Preserve stable enough row heights that normal scrolling does not visibly jump or lose position.
- Keep search highlight and scroll-to-match behavior correct when target rows were initially deferred.
- Keep empty, short, malformed, and unknown transcript cases graceful.
- Preserve existing transcript pagination, grouped tool call rows, and inspector behavior.

## Non-Goals

- Do not optimize Markdown rendering or syntax highlighting as part of this issue.
- Do not migrate the transcript to `gtk::ListView` or a full virtualized list architecture in v1.
- Do not redesign transcript row visuals beyond lightweight placeholders needed for responsiveness.
- Do not dehydrate rows after hydration in v1.

## Chosen Approach

Use lazy row shells within the existing `SessionDetail` and `FactoryVecDeque<TranscriptRow>` architecture.

Each `TranscriptRow` builds a lightweight shell in `init_widgets`. Heavy content such as `TextView`, Markdown-rendered widgets, syntax-highlighted labels, and detailed `ToolBurst` children is created only when `SessionDetail` decides the row is visible, near the viewport, or required for a search jump.

This is not full virtualization. All rows for a loaded page still exist in the factory, but their expensive child widget trees are deferred until useful.

## Alternatives Considered

### A. Lazy Shell With Viewport Margin

This is the selected approach. It keeps current pagination and factory wiring, mounts cheap shells first, and hydrates rows in or near the viewport.

Benefits: smallest architecture change, targets the measured GTK realization bottleneck, preserves current search and pagination data structures.

Risks: viewport scanning and hydration scheduling must be careful enough to avoid blank rows and avoid recreating the initial freeze.

### B. Progressive Hydration Of The Whole Page

This would mount shells, then hydrate every row on the loaded page in small background batches, independent of viewport position.

Benefits: simpler final state and easier search behavior once hydration catches up.

Risks: may still do unnecessary work for the whole first page and miss the `50 ms` schedule-gap target on the reference session.

### C. GtkListView Migration

This would replace the transcript `gtk::Box` and `FactoryVecDeque` with GTK's virtualized list framework.

Benefits: GTK creates row widgets only for visible items and recycles them by design.

Risks: larger rewrite, more risk to `display_targets_by_item_index`, grouped tool call rows, search pagination, scroll-to-match, and row state. This is out of scope for v1.

## Architecture

`SessionDetail` remains responsible for session pagination, transcript search, scroll-to-match, render metrics, and hydration scheduling.

`TranscriptRow` remains the factory component for each display row. Its new responsibility is to separate shell creation from heavy content hydration.

The parent-child contract changes as follows:

- `TranscriptRow::init_widgets` creates a shell and returns immediately.
- `TranscriptRowMsg::HydrateDeferredContent { reason }` asks a row to build its heavy content.
- Hydration is idempotent at two layers:
  - The row owns an internal `hydrated: bool` flag and ignores duplicate `HydrateDeferredContent` inputs without consulting request staleness.
  - `SessionDetail` filters incoming `DeferredContentHydrated` outputs against the active transcript request id, so outputs that arrive after a session change or transcript invalidation are dropped.
- `TranscriptItemInit` includes the active transcript `request_id`. `TranscriptRow` stores it as an opaque epoch and only echoes it in outputs; it must not use it to decide whether hydration is allowed.
- `TranscriptRowOutput::DeferredContentHydrated { request_id, item_index, reason }` tells `SessionDetail` that search jumps and dependent UI can continue.
- The parent owns staleness, the child owns idempotency. The child may carry the opaque epoch only so late child outputs can be rejected safely by the parent.

The shell includes CSS classes, row-level metadata, low-cost labels, actionable inspect buttons where applicable, and a placeholder area with a conservative height estimate. The heavy subtree is appended or swapped in during hydration.

## Data Flow

The first-page open flow remains close to the existing code:

1. `SessionDetail` loads transcript rows through `load_transcript_items`.
2. `build_display_items` prepares `TranscriptItemInit` values with the current `highlight_query` for matching items.
3. Render batches push `TranscriptItemInit` values into `FactoryVecDeque<TranscriptRow>`.
4. Each row builds only its shell and emits the existing row build metric.
5. `SessionDetail` schedules initial hydration after the first page is pushed.
6. Hydration targets are selected from rows in the viewport plus a margin.
7. Subsequent scroll and resize events re-run the viewport selection.
8. Search jumps can insert a target row at the front of the hydration queue.

Hydration should be drained in small timer or idle batches. The starting default is 2 rows per tick; see `Implementation Tuning Defaults` for the full set. The batch size can be tuned after measurement, but the design must prefer schedule-gap stability over throughput.

User-initiated hydration bypasses the background batch budget. See `Interaction Behavior` for the synchronous carve-out applied to expand clicks, tool burst toggles, and inspect actions.

The existing `max_schedule_gap_ms` metric remains the primary verdict. Additional debug logs should record deferred hydration batch size, reason, duration, and remaining queue length.

## Shell Weight Validation

The #142 investigation reached `21 ms` median `max_schedule_gap_ms` by replacing rows with minimal labels. The shell described in `Row Shells` is heavier than that minimal label: it builds a header with role, model, timestamp, reasoning pill, expand button, and inspect button where applicable, plus CSS classes and a placeholder area.

Before locking the tuning defaults below, the implementation must measure the shell-only first-page push on the #142 reference Codex session and record the median `max_schedule_gap_ms` over at least 3 runs.

- If the shell-only median is at or below `50 ms`, proceed with the defaults.
- If the shell-only median is between `50 ms` and `150 ms`, reduce the initial hydration batch and shrink the viewport margin before retrying; the bottleneck is shell weight rather than hydration scheduling.
- If the shell-only median exceeds `150 ms`, the shell itself is too heavy. Collapse the header into a single markup-formatted `Label`, drop the inspect button into hydration, and remove non-essential CSS classes from the shell before re-measuring.

This gate exists because hydration tuning cannot rescue a shell that already misses the goal at first push.

## Viewport Detection

`SessionDetail` should name or otherwise retain access to the transcript `gtk::ScrolledWindow`. It can use the existing `scroll_child` and factory widget tree to determine row visibility.

The viewport calculation uses:

- `vadjustment().value()` as viewport top.
- `vadjustment().page_size()` as visible height.
- `row_widget.compute_point(reference, Point::zero())` as row Y position. The `reference` widget must be the direct child of the `gtk::Viewport` inside `gtk::ScrolledWindow` (the widget whose origin matches `vadjustment.value() == 0`); if `compute_point` returns `None` because the row is not yet in a parented chain, the row is skipped and rescheduled on the next scan.
- `row_widget.height()` as row height after allocation. Rows that have not yet been allocated (`height() == 0`) fall back to their reserved placeholder height for the intersection test.
- A vertical margin of roughly `1.5x page_size` above and below the viewport.

A row is a hydration candidate when `[row_y, row_y + row_height]` intersects `[viewport_top - margin, viewport_bottom + margin]`.

Scanning all loaded row widgets is acceptable in v1 because pages are currently 75 and 100 source rows. The scan runs after first-page push, after next-page push, on debounced `vadjustment::value` changes, on debounced viewport size changes, and before scroll-to-match.

Scroll debounce defaults to `60 ms`; `16 ms` is too tight for a sustained user scroll and produces redundant scans. Resize debounce defaults to `120 ms`. Both can be tuned after measurement but must remain coarse enough that a continuous gesture does not produce more than one scan per tick.

Each scan emits at most one `Queued deferred transcript hydration` log entry summarising the result; the log is not emitted per row.

## Scroll Anchoring

Hydrating rows above the current viewport grows their measured height beyond the placeholder reservation. Without compensation, every above-viewport hydration shifts the visible content downward by the height delta, which is perceived as the transcript "jumping" while the user reads.

The hydration scheduler must therefore anchor scroll position when it hydrates above-viewport rows. The protocol per batch:

1. Before hydration, capture `vadjustment.value()` as `pre_value` and collect the rows in the batch that are strictly above the viewport, where `row_y + pre_height <= pre_value`. Store each row's pre-hydration height.
2. Run the batch hydration.
3. After GTK has completed a layout pass for the hydrated rows, recompute the cumulative measured height of the same rows. See `Post-Hydration Layout Barrier` for the scheduling contract.
4. Compute `delta = post_above_height - pre_above_height`. If `delta != 0`, compute `max_scroll = (vadjustment.upper() - vadjustment.page_size()).max(vadjustment.lower())`, then set `vadjustment.set_value((pre_value + delta).clamp(vadjustment.lower(), max_scroll))`. Positive deltas compensate under-reserved placeholders that grew; negative deltas compensate over-reserved placeholders that shrank. The user-visible content stays put; only `vadjustment.upper` and the scrollbar thumb size change.
5. The compensation only applies to rows strictly above the current viewport top. Rows intersecting the viewport are allowed to grow naturally; the user already sees them and a small height correction there is preferable to a scroll jump that breaks the reading position.

The anchor protocol is skipped when `pre_value <= 0` (already at top), when no row in the batch sits above the viewport, or when the batch is the initial first-page hydration (no anchored reading position to preserve yet).

The first-page hydration burst is the one exception that may proceed without anchoring. Once the user has scrolled at all, anchoring is mandatory for every subsequent above-viewport hydration.

## Resize Behavior

Window resizing can change both viewport height and row wrapping. The design treats resize as a viewport invalidation, not as a row rebuild.

`SessionDetail` should observe `vadjustment::page-size` changes and a width-related notification on the scrollable content or scrolled window. These events should be debounced before scheduling hydration (default `120 ms`).

After resize, `SessionDetail` scans the viewport plus margin and hydrates any newly relevant shells. Rows that are already hydrated remain hydrated. The v1 design does not dehydrate or rebuild hydrated rows on resize.

Shell height estimation is width-independent in v1. This is a deliberate v1 simplification with two known consequences that must be acknowledged in metrics rather than fixed:

- After a large width change, unhydrated placeholders far below the viewport keep their pre-resize estimate. The error compounds across many rows, so `vadjustment.upper` will correct itself in visible chunks as the user scrolls into those regions.
- Hydrated Markdown rows reflow naturally via GTK on width change; their height delta is absorbed by GTK's allocation cycle, not by this design.

If post-resize scrolling shows jarring `upper` corrections in practice, v2 may revisit width-aware placeholder estimates; v1 accepts the drift in exchange for simpler height code.

## Row Shells

Message rows should build their header immediately, including role, assistant model when present, timestamp, reasoning pill state, and expand button visibility. The content area starts as a placeholder with a conservative height estimate based on role, preview length, truncation state, and whether the row is an assistant message likely to use Markdown.

Tool call rows should build a lightweight primary row immediately, including icon, tool call name, status, duration, reasoning pill state, and inspect button. The secondary preview is deferred in v1 and appears during hydration.

Tool burst rows should build the group header immediately. Closed burst children remain deferred. Opening a burst hydrates its children if they have not been built. Search jump into a child hydrates and opens the burst before the final scroll.

Subagent rows are already lightweight and can be effectively hydrated by the shell. They still participate in the idempotent hydration API for consistency.

## Height Stability

Shell placeholders reserve vertical space to bound post-hydration layout shifts. The estimate is biased toward over-reservation rather than under-reservation:

- An over-estimated placeholder shrinks on hydration. Below the viewport this is invisible; above the viewport, scroll anchoring (see `Scroll Anchoring`) absorbs the delta as a thumb-size change without moving visible content.
- An under-estimated placeholder grows on hydration. This produces the worst user experience: visible content is pushed off the viewport, and `vadjustment.upper` jumps.

Estimation rules:

- Reserve a minimum height per row kind.
- Estimate message content height from preview length and observed line breaks (count `\n` in the preview, then add wrapped-line estimate from remaining characters).
- Reserve more for assistant messages than for simple user text. Assistant content is the dominant Markdown source and the most likely to under-reserve.
- Cap very long previews, but cap them generously rather than tightly. The cap exists to avoid pathological multi-screen placeholders, not to be tight against typical content.
- Reserve only the header height for closed tool bursts.
- Remove or relax the placeholder height request after hydration so the row can shrink to its real measured height.

Small downward corrections after hydration are acceptable and absorbed by scroll anchoring. Persistent blank rows after scrolling stops are not acceptable and indicate a hydration scheduling bug, not an estimation bug.

## Search And Scroll-To-Match

Search match positions remain database-driven and page-aware through the existing `match_positions` and `display_targets_by_item_index` structures.

`continue_pending_jump` should keep its current page-loading behavior, then add a hydration gate before setting `scroll_to_item`:

1. If the target page is not loaded, load it as today.
2. If the target display row is loaded but not hydrated, enqueue it with search-target priority.
3. Wait for `DeferredContentHydrated { request_id, item_index }` whose `request_id` matches the active transcript request.
4. For `ToolBurst` child targets, ensure the burst is hydrated and expanded.
5. Defer `scroll_to_item` until the post-hydration layout barrier has passed. `DeferredContentHydrated` only confirms widget construction; GTK has not necessarily run measure/allocate, so the row's `height()` may still be its placeholder reservation. Without this delay, `scroll_to_item` jumps to the wrong Y position whenever the hydrated content differs from the placeholder height.
6. Set `scroll_to_item` inside the deferred callback, only after the target content needed for the match exists and has been allocated at its real height.

Rows hydrated late use the same `highlight_query` already present in their init data, so highlight rendering remains consistent with current behavior.

If the target disappears between `DeferredContentHydrated` and the deferred scroll callback because of stale requests, session changes, or boundary regrouping, the deferred callback must verify the target still maps to a current widget before scrolling and abandon the jump with a warning otherwise.

## Post-Hydration Layout Barrier

Any logic that reads row heights after hydration, including scroll anchoring and search scroll-to-match, must wait until GTK has completed layout for the changed widget tree.

Do not use `glib::idle_add_local_once` for this barrier. Idle callbacks are main-loop scheduling only and do not guarantee that GTK has run a layout phase.

Do not rely on a single `add_tick_callback` invocation either. GTK tick callbacks run during the frame clock Update phase, which is before Layout, so the first tick after changing widget content may still observe the previous allocation. Use one of these implementation patterns instead:

- Preferred: wait for a concrete allocation change on the hydrated row or scroll child, then run the deferred scroll or anchoring work once and disconnect the handler.
- Acceptable fallback: schedule a temporary `add_tick_callback` and perform the height read on the second tick after hydration, returning `ControlFlow::Continue` on the first tick and `ControlFlow::Break` after the second. The first tick gives the queued resize/layout frame a chance to complete; the second tick observes the allocation from the previous frame.

If neither pattern observes a non-zero allocated height for the target row, reschedule once and then abandon the operation with a warning rather than scrolling against placeholder geometry.

## Interaction Behavior

User-initiated hydration must not be throttled by the background hydration queue. The queue exists to spread off-screen work over many frames; an explicit user gesture is the opposite case and demands immediate response.

The synchronous carve-out applies to:

- Message expand button click.
- Tool burst expand/collapse toggle.
- Inspect button click on tool call or subagent rows.

In each case, the row's hydration runs synchronously in the same input handler before the rest of the action proceeds. For tool bursts specifically, all child rows of the burst hydrate in a single synchronous pass when the burst is opened, even if that pass exceeds the per-tick batch budget; otherwise a 30-child burst would take roughly `30 / 2 * 16 ms = 240 ms` of placeholders before showing real content, which feels broken on a click.

Concrete behavior per gesture:

- Message expand: if not yet hydrated, hydrate the preview shell synchronously, then run the existing full-content load flow. Existing full-content load failure handling and toast behavior remain unchanged.
- Tool burst expand: if not yet hydrated, hydrate the burst header (already done at shell mount) and synchronously hydrate every child tool call row, then toggle visibility. If already hydrated for search, the click only toggles visibility.
- Inspect: if the shell already carries enough data to route the inspect action (id, kind, model when relevant), route immediately. Otherwise hydrate the row synchronously, then route.

Synchronous hydration must still emit `DeferredContentHydrated { request_id, item_index, reason }` so that any pending search jump waiting on that row is unblocked.

## Error Handling

Empty sessions enqueue no hydration work and continue to render gracefully.

Short transcripts follow the same shell and hydration path, with low cost.

Malformed or unknown transcript items keep the current fallback to an empty message row and render a stable empty shell.

Hydration for stale transcript requests is ignored after session changes or explicit transcript invalidation.

Hydration requests for rows that no longer exist are ignored.

Boundary regrouping for grouped tool call rows may replace the tail row during next-page loading. Any queued hydration entry for the replaced row should be dropped or ignored when it no longer maps to a current widget.

## Metrics

Existing logs remain important:

- `Session detail open started`
- `First transcript page factory push complete`
- `Finished rendering transcript page`
- `Transcript page row-build breakdown`

New debug logs should make hydration measurable without logging transcript content:

- `Queued deferred transcript hydration` with request id, reason, target count, visible count, margin count. Emitted at most once per viewport scan, never per row.
- `Hydrated deferred transcript batch` with request id, hydrated count, duration, max row hydration duration, remaining count.
- `Deferred transcript row hydrated` with request id, item index, row kind, reason, duration, and whether it was search-critical.
- `Anchored transcript scroll on hydration` with request id, anchored row count, and applied delta in pixels. Emitted only when scroll anchoring (see `Scroll Anchoring`) actually adjusts `vadjustment.value`.

No transcript content, tool call payload, command output, or Markdown body text should be logged.

## Testing

Automated tests should cover:

- Message `TranscriptRow::init_widgets` creates a shell without initial Markdown `TextView` content.
- `HydrateDeferredContent` replaces the placeholder with rendered content and is idempotent across repeated inputs.
- `TranscriptRow` echoes its init `request_id` in `DeferredContentHydrated` outputs without using it for hydration idempotency.
- `SessionDetail` filters `DeferredContentHydrated` outputs whose request id no longer matches the active transcript request.
- Late hydration applies `highlight_query` to message content.
- Tool burst children are not built at shell mount and are built on expansion or search-target hydration.
- Tool burst expand click hydrates all children synchronously in a single pass, regardless of background batch budget.
- `SessionDetail::continue_pending_jump` waits for target hydration AND defers `scroll_to_item` until the post-hydration layout barrier has passed before setting it.
- Existing transcript pagination, search navigation, grouped tool call row, and inspector tests continue to pass.

Manual verification should cover:

- Open the #142 reference Codex session and record at least 3 runs.
- Confirm median `max_schedule_gap_ms <= 50 ms`.
- Confirm initially visible content is readable within `200 ms`.
- Scroll quickly through a large transcript and stop; no blank row should persist at rest.
- Resize the window; newly visible or near-visible rows should hydrate without full rebuild.
- Search for a match outside the initial page; the app should load, hydrate, and scroll to the target correctly. The match position must be correct on the first paint, not after a visible re-scroll.
- Scroll into the middle of a long transcript, then scroll back up. Above-viewport rows that hydrate during the back-scroll must not visibly push current content; the scrollbar thumb may shrink but text should stay anchored.
- Rapid expand of a tool burst with many children must reveal all child rows immediately, not progressively over multiple frames.

CI verification should include:

- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`

## Acceptance Criteria Mapping

- `max_schedule_gap_ms <= 50 ms`: achieved by shell-only first push and bounded hydration batches, validated by the Shell Weight Validation gate before defaults are locked.
- Initial content readable within `200 ms`: achieved by viewport-priority hydration immediately after first push.
- Progressive population during scroll: achieved by debounced viewport scans and margin-based hydration.
- Stable enough layout: achieved by over-biased placeholder estimates and scroll anchoring on above-viewport hydration.
- Search highlight and scroll-to-match: achieved by synchronous search-target hydration plus a one-tick allocation delay before `scroll_to_item`.
- Responsive user gestures: achieved by the synchronous hydration carve-out for expand, burst toggle, and inspect actions.
- Empty, short, malformed transcripts: handled by existing fallbacks plus no-op hydration queues.
- Existing tests pass: covered by preserving current pagination and row mapping contracts.

## Implementation Tuning Defaults

The implementation should start with explicit conservative defaults and tune them only if measurement shows they miss the issue goals:

- Initial hydration batch size: 2 rows per tick.
- Hydration tick delay: 16 ms.
- Viewport margin multiplier: `1.5x page_size`.
- Scroll debounce on `vadjustment::value`: 60 ms.
- Resize debounce on `page-size` and width notifications: 120 ms.
- Message placeholder lines: count `\n` in `content_preview`, plus `ceil(remaining_chars / 80)` for wrapped lines, clamped to a minimum of 2 lines.
- Message placeholder upper cap: 24 lines (generous; the cap avoids pathological multi-screen placeholders, not typical content).
- Assistant message placeholder: add 4 estimated lines before clamping to account for Markdown structure (paragraphs, code fences, lists). Assistant content is the most likely to under-reserve, and over-reservation here is preferable.
- Estimated line height: use a fixed conservative value such as 22 px rather than measuring Pango text. Slightly over the typical body line height so the placeholder rarely under-reserves.
- Tool call secondary previews: deferred until hydration.
- Tool burst child hydration on user-initiated expand: synchronous, all children in one pass, regardless of batch budget.
- Search-target hydration: synchronous on the target row, with `scroll_to_item` deferred until the post-hydration layout barrier has passed.

These constants affect performance tuning but do not change the design direction. The Shell Weight Validation gate runs before these defaults are accepted as final.
