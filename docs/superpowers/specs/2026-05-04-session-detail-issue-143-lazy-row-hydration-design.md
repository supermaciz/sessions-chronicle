# Session Detail Issue 143 Lazy Row Hydration Design

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
- `TranscriptRowMsg::HydrateDeferredContent` asks a row to build its heavy content.
- Hydration is idempotent; a hydrated row ignores duplicate requests.
- `TranscriptRowOutput::DeferredContentHydrated { item_index }` tells `SessionDetail` that search jumps can continue.
- Hydration requests include the current transcript request context at the parent level so stale work can be ignored after session changes.

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

Hydration should be drained in small timer or idle batches, starting with 1 to 3 rows per tick. The batch size can be tuned after measurement, but the design must prefer schedule-gap stability over throughput.

The existing `max_schedule_gap_ms` metric remains the primary verdict. Additional debug logs should record deferred hydration batch size, reason, duration, and remaining queue length.

## Viewport Detection

`SessionDetail` should name or otherwise retain access to the transcript `gtk::ScrolledWindow`. It can use the existing `scroll_child` and factory widget tree to determine row visibility.

The viewport calculation uses:

- `vadjustment().value()` as viewport top.
- `vadjustment().page_size()` as visible height.
- `row_widget.compute_point(scroll_child, Point::zero())` as row Y position.
- `row_widget.height()` as row height after allocation.
- A vertical margin of roughly `1.5x page_size` above and below the viewport.

A row is a hydration candidate when `[row_y, row_y + row_height]` intersects `[viewport_top - margin, viewport_bottom + margin]`.

Scanning all loaded row widgets is acceptable in v1 because pages are currently 75 and 100 source rows. The scan runs after first-page push, after next-page push, on debounced `vadjustment::value` changes, on debounced viewport size changes, and before scroll-to-match.

## Resize Behavior

Window resizing can change both viewport height and row wrapping. The design treats resize as a viewport invalidation, not as a row rebuild.

`SessionDetail` should observe `vadjustment::page-size` changes and a width-related notification on the scrollable content or scrolled window. These events should be debounced before scheduling hydration.

After resize, `SessionDetail` scans the viewport plus margin and hydrates any newly relevant shells. Rows that are already hydrated remain hydrated. The v1 design does not dehydrate or rebuild hydrated rows on resize.

Shell height estimation should be width-independent in v1. Resize should trigger viewport scanning and hydration only; it should not recalculate placeholder heights or rebuild hydrated rows.

## Row Shells

Message rows should build their header immediately, including role, assistant model when present, timestamp, reasoning pill state, and expand button visibility. The content area starts as a placeholder with a conservative height estimate based on role, preview length, truncation state, and whether the row is an assistant message likely to use Markdown.

Tool call rows should build a lightweight primary row immediately, including icon, tool call name, status, duration, reasoning pill state, and inspect button. The secondary preview is deferred in v1 and appears during hydration.

Tool burst rows should build the group header immediately. Closed burst children remain deferred. Opening a burst hydrates its children if they have not been built. Search jump into a child hydrates and opens the burst before the final scroll.

Subagent rows are already lightweight and can be effectively hydrated by the shell. They still participate in the idempotent hydration API for consistency.

## Height Stability

Shell placeholders should reserve enough vertical space to avoid large post-hydration jumps.

The estimate should be conservative but bounded:

- Reserve a minimum height per row kind.
- Estimate message content height from preview length and expected line count.
- Reserve more for assistant Markdown than for simple user text.
- Cap very long previews so placeholders do not become huge.
- Reserve only the header height for closed tool bursts.
- Remove or relax the placeholder height request after hydration.

Small height corrections after hydration are acceptable. The viewport margin and priority hydration should make visible blank space short-lived and prevent persistent blank rows after scrolling stops.

## Search And Scroll-To-Match

Search match positions remain database-driven and page-aware through the existing `match_positions` and `display_targets_by_item_index` structures.

`continue_pending_jump` should keep its current page-loading behavior, then add a hydration gate before setting `scroll_to_item`:

1. If the target page is not loaded, load it as today.
2. If the target display row is loaded but not hydrated, enqueue it with search-target priority.
3. Wait for `DeferredContentHydrated { item_index }`.
4. For `ToolBurst` child targets, ensure the burst is hydrated and expanded.
5. Set `scroll_to_item` only after the target content needed for the match exists.

Rows hydrated late use the same `highlight_query` already present in their init data, so highlight rendering remains consistent with current behavior.

If the target disappears because of stale requests, session changes, or boundary regrouping, the jump should be abandoned with a warning rather than panicking.

## Interaction Behavior

Message expansion remains lazy. If the user clicks expand before preview hydration, the row hydrates its preview shell first, then runs the existing full-content load flow. Existing full-content load failure handling and toast behavior remain unchanged.

Tool burst expansion builds child tool call widgets on demand. If the burst was already hydrated for search, the user click only toggles visibility.

Inspector actions remain available from shell metadata for tool call and subagent rows. If a shell does not yet include enough data for an inspect action, hydration should be triggered before routing the action.

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

- `Queued deferred transcript hydration` with request id, reason, target count, visible count, margin count.
- `Hydrated deferred transcript batch` with request id, hydrated count, duration, max row hydration duration, remaining count.
- `Deferred transcript row hydrated` with item index, row kind, reason, duration, and whether it was search-critical.

No transcript content, tool call payload, command output, or Markdown body text should be logged.

## Testing

Automated tests should cover:

- Message `TranscriptRow::init_widgets` creates a shell without initial Markdown `TextView` content.
- `HydrateDeferredContent` replaces the placeholder with rendered content and is idempotent.
- Late hydration applies `highlight_query` to message content.
- Tool burst children are not built at shell mount and are built on expansion or search-target hydration.
- `SessionDetail::continue_pending_jump` waits for target hydration before setting `scroll_to_item`.
- Existing transcript pagination, search navigation, grouped tool call row, and inspector tests continue to pass.

Manual verification should cover:

- Open the #142 reference Codex session and record at least 3 runs.
- Confirm median `max_schedule_gap_ms <= 50 ms`.
- Confirm initially visible content is readable within `200 ms`.
- Scroll quickly through a large transcript and stop; no blank row should persist at rest.
- Resize the window; newly visible or near-visible rows should hydrate without full rebuild.
- Search for a match outside the initial page; the app should load, hydrate, and scroll to the target correctly.

CI verification should include:

- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`

## Acceptance Criteria Mapping

- `max_schedule_gap_ms <= 50 ms`: achieved by shell-only first push and bounded hydration batches.
- Initial content readable within `200 ms`: achieved by viewport-priority hydration immediately after first push.
- Progressive population during scroll: achieved by debounced viewport scans and margin-based hydration.
- Stable enough layout: achieved by conservative bounded placeholders and no v1 dehydration.
- Search highlight and scroll-to-match: achieved by search-priority hydration gate before final scroll.
- Empty, short, malformed transcripts: handled by existing fallbacks plus no-op hydration queues.
- Existing tests pass: covered by preserving current pagination and row mapping contracts.

## Implementation Tuning Defaults

The implementation should start with explicit conservative defaults and tune them only if measurement shows they miss the issue goals:

- Initial hydration batch size: 2 rows per tick.
- Hydration tick delay: 16 ms.
- Viewport margin multiplier: `1.5x page_size`.
- Message placeholder lines: `ceil(content_preview.len() / 100)`, clamped to 2 through 12 lines.
- Assistant message placeholder: add 2 estimated lines before clamping to account for Markdown structure.
- Estimated line height: use a fixed conservative value such as 20 px rather than measuring Pango text.
- Tool call secondary previews: deferred until hydration.

These constants affect performance tuning but do not change the design direction.
