# SessionList Post-Indexing Reload Instrumentation - Design

**Date:** 2026-05-06  
**Status:** Proposed  
**Issue:** [#145](https://github.com/supermaciz/sessions-chronicle/issues/145)

## Context

GitHub issue 145 tracks a brief visible freeze in `SessionList` at the end of background indexing.

The likely path is the reload triggered from `App::handle_indexing_completed`: once indexing finishes, the app emits `SessionListMsg::Reload`, and `SessionList::reload_sessions()` synchronously fetches sessions, clears the row factory, pushes all `SessionRow` entries, and restores selection on the main thread.

This is a separate problem from the recent `SessionDetail` responsiveness work. The first job is to measure which part of the post-indexing reload causes the freeze, before deciding whether to invest in batching, diff-based updates, pagination, or asynchronous fetching.

## Goal

Add focused instrumentation that makes the post-indexing `SessionList` freeze measurable and actionable.

The instrumentation must distinguish:

- database fetch time;
- factory clear time;
- row insertion time and row count;
- selection restoration behavior;
- delay after `drop(guard)` until the next main-loop idle callback;
- delay after `drop(guard)` until the next GTK frame callback (`WidgetExt::add_tick_callback`, fired by the widget's `FrameClock`).

The resulting logs and report must support a narrow recommendation for the next implementation step.

## Non-Goals

- No `SessionList` optimization in this issue.
- No `SessionDetail` transcript rendering changes.
- No persistent telemetry subsystem or metrics storage.
- No instrumentation of unrelated reload paths (search, filter, pin) beyond what is needed to disambiguate the measured cycle.
- No batching, diff-based updates, pagination, or windowing implementation before the bottleneck is measured.

## Recommended Approach

Use small inline instrumentation in the existing post-indexing reload flow, with structured `tracing` fields and `Instant` timestamps, in the same style as the recent `SessionDetail` responsiveness instrumentation.

Measure a single reload cycle tagged `reason = "post_indexing_completion"`, starting when `handle_indexing_completed` requests the reload and ending after both the post-`drop(guard)` idle and frame callbacks have fired (or been declared unavailable).

Keep the instrumentation close to the measured code paths. A reusable metrics framework would be broader than the issue requires and would invite drift between measurement and code.

## Alternatives Considered

### Measure `reload_sessions()` Only

Log `fetch_sessions`, `guard.clear()`, and `push_back` time only.

Trade-off: tiny diff, but misses the GTK/layout work after `drop(guard)` — which is the most likely source of a brief end-of-indexing freeze. Insufficient on its own.

### Generic `SessionListMetrics` Subsystem

A reusable struct tracking every reload reason and aggregating across them.

Trade-off: turns a narrow diagnosis into framework work and adds shared state that ages badly when reload causes overlap.

### Instrument Every Reload Reason Equally

Same heavy instrumentation on indexing, pinning, search, filter reloads.

Trade-off: more comparable, but adds noise that hides the target symptom. Other reloads stay out of scope.

## Instrumentation Design

### Tagging The Measured Cycle

`App` reaches `SessionList` through its Relm4 `Controller`, so the post-indexing measurement context must be passed as a component input, not by mutating `SessionList` state directly from `App`.

Add a narrow dedicated input for the measured path, for example:

```rust
SessionListMsg::ReloadAfterIndexing {
    assistants: Vec<AiAssistant>,
    project_filter: ProjectFilter,
    context: IndexingReloadContext,
}
```

This message:

- applies the current filter state from `App`;
- marks the reload as the measured post-indexing cycle;
- immediately runs `reload_sessions()`.

Ordinary reloads (`Reload`, search, pin, manual filter changes) remain unmeasured and keep their current behavior. The dedicated message exists only because the indexing-completion path needs to carry extra context across the component boundary cleanly.

`App` is the canonical owner of active AI assistant filters and `project_filter`. `SessionList` stores only the latest applied copy needed to render and reload from its local component state.

`IndexingReloadContext` carries:

- `indexed`, `skipped`, `removed`;
- whether `pending_reindex_feedback` was active;
- whether indexing reported errors.

If `refresh_sidebar_projects()` changes the retained project filter, `App` should emit the dedicated measured message with the updated `assistants` / `project_filter` values instead of routing through a separate `SetFilters` followed by an implicit reload. That keeps one unambiguous measured cycle and avoids consuming measurement state on an intermediate message.

### Cycle Identity And Invalidation

Keep a dedicated in-flight measurement state on `SessionList` until the summary has been emitted, for example:

```rust
active_post_indexing_measurement: Option<ActiveMeasurement>
```

`ActiveMeasurement` holds:

- the immutable `IndexingReloadContext`;
- synchronous timings collected during `reload_sessions()`;
- slots for `next_idle_delay_ms` and `next_frame_delay_ms`;
- a small invalidation token shared with the idle/frame callback closures.

At most one measured post-indexing cycle is tracked at a time.

Invalidation rules:

- if a new measured cycle starts before the previous one completed, `SessionList` first invalidates the existing `active_post_indexing_measurement`, then replaces it; stale callbacks discover this through the shared invalidation token and exit without emitting;
- if the widget is dropped before the frame callback fires, the callback exits silently;
- if the idle or frame callback never fires, no summary is emitted (best-effort by design).

The invalidation token can be as simple as a `Rc<Cell<bool>>` flag that the new cycle marks `false` before replacing the previous `ActiveMeasurement`. A numeric `reload_id` is not required.

### Synchronous Reload Phases

Inside the measured branch of `reload_sessions()`, capture:

- `previously_selected_id.is_some()`;
- active AI assistant filters (enum/count, not user data);
- `project_filter` presence;
- search query presence and length (not the text);
- `fetch_sessions_duration_ms`;
- `factory_clear_duration_ms`;
- `row_push_duration_ms`;
- `row_count`;
- `selection_restore_attempted`;
- `selection_restore_succeeded`;
- `ensure_selection_fallback_ran`.

The split must make it obvious whether the synchronous cost is dominated by DB fetch, factory clear, or row push.

### Post-Drop Main-Loop And Frame Timing

Immediately after `drop(guard)`, capture `t_after_drop = Instant::now()` and schedule:

- `glib::idle_add_local_once(...)` → records `next_idle_delay_ms = elapsed since t_after_drop`;
- `list_widget.add_tick_callback(...)` → records `next_frame_delay_ms` on first invocation, then returns `ControlFlow::Break`.

Interpretation:

- a large `next_idle_delay_ms` indicates the main loop did not regain control promptly after the synchronous reload work;
- a large `next_frame_delay_ms` with moderate synchronous timings points at GTK realization, layout, or paint-adjacent cost.

A frame callback only means the widget's `FrameClock` advanced; it does not prove all visual work is on screen. The summary event wording must reflect that.

### Final Summary Event

When both idle and frame callbacks have either fired or been marked unavailable, emit one `info` event `sessionlist.post_indexing_reload.measured` from the still-active `ActiveMeasurement` with:

- trigger context fields;
- filter/search context;
- `fetch_ms`, `clear_ms`, `push_ms`, `row_count`;
- selection restoration fields;
- `next_idle_delay_ms` (or `unavailable`);
- `next_frame_delay_ms` (or `unavailable`);
- `total_reload_ms` (from start of measured cycle to the last captured callback).

Sub-phase events may use `debug` if needed; the final summary alone should be sufficient to write the report.

If a newer measured cycle invalidates the active one before its callbacks fire, the stale summary is dropped — never emitted as if current.

## Error Handling

Instrumentation is best-effort and must not affect user behavior:

- if the measured reload path does not actually run, no behavior changes and nothing is logged;
- if the list widget is unavailable when scheduling the frame callback, that field is marked `unavailable` and the summary still fires after the idle callback;
- no retries, no debouncing, no scheduling changes are added.

## Data Safety

Logs must not contain session titles, transcript content, search text, tool call payloads, raw command output, or any other user data beyond existing safe metadata.

Acceptable: counts, durations, booleans, enum-like reload reasons or filter states, row counts, indexing counts, search query length.

## Reproduction And Reporting

Suggested run:

```bash
RUST_LOG=info,sessions_chronicle=debug ~/.local/bin/sessions-chronicle > /tmp/sessions-chronicle-issue-145.log 2>&1
```

Protocol:

1. Clean launch.
2. Wait for background indexing to complete.
3. Observe the transition from indexing completion into the `SessionList` reload.
4. Repeat 3+ times.
5. Use a representative real session dataset; fixtures only if they reach comparable scale.

Report (in `docs/reports/`) must include date, environment, build/run commands, dataset description, run-by-run measurements, median + worst run for the key fields, whether the freeze is user-noticeable at the tested scale, and the recommended next step.

### Initial Interpretation Thresholds

These are starting reference points, to be recalibrated from the measured median and worst-run values in the report:

- ~16 ms = one display frame at 60 Hz; below this, no perceptible jank;
- ~100 ms = the commonly cited threshold above which a UI transition feels non-instantaneous (Nielsen, *Response Times: The 3 Important Limits*);
- ~250 ms is treated here as clearly problematic for an end-of-indexing transition that occurs without explicit user action.

The report defines the operational target threshold for the next fix using its own measurements rather than these defaults.

## Decision Rules And Acceptance

The report recommends the narrowest next implementation step justified by the measurements:

| Dominant phase | Recommended next investigation |
|---|---|
| `fetch_ms` | asynchronous fetch or query/index improvement |
| `clear_ms` | rethink full clear/rebuild as the update strategy |
| `push_ms` | batched insertion or incremental update |
| post-drop idle / frame delay | reduce GTK realization/layout pressure (batched insertion, pagination, windowing) |
| no single dominant phase | smallest next experiment, not a broad rewrite |

The instrumentation work is accepted when:

- the synchronous and post-drop phases above are individually visible in logs;
- a reproducible run captures the freeze at realistic scale;
- the report concludes whether the freeze is user-noticeable and recommends one of the rows above (or an explicit "no action") with measured evidence;
- no user-facing behavior changes beyond logging.

## Testing Strategy

Diagnostics-only change. Light automated checks:

- `cargo fmt --all -- --check`;
- targeted unit test for the `ActiveMeasurement` invalidation helper if one is extracted;
- `cargo test --all --no-fail-fast` only if the diff touches enough shared logic to justify it.

Manual verification is primary. Success = logs clearly separate synchronous reload cost from post-drop main-loop/frame delay.

## Implementation Decisions

- Keep instrumentation inline near `handle_indexing_completed` and `SessionList::reload_sessions()`.
- Pass the post-indexing measurement context through a narrow dedicated `SessionListMsg`, because `App` reaches `SessionList` through a Relm4 controller boundary.
- Keep a separate `active_post_indexing_measurement: Option<ActiveMeasurement>` alive until idle/frame completion, plus a lightweight invalidation flag; no numeric IDs.
- Use `glib::idle_add_local_once` and `WidgetExt::add_tick_callback` (one-shot) for post-drop timings.
- `info` for the final summary event, `debug` for supporting phase logs.

These choices keep issue 145 a diagnosis, with room for the later fix to follow measured evidence.
