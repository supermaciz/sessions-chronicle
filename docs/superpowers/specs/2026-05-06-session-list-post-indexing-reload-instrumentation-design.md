# SessionList Post-Indexing Reload Instrumentation - Design

**Date:** 2026-05-06  
**Status:** Proposed  
**Issue:** [#145](https://github.com/supermaciz/sessions-chronicle/issues/145)

## Context

GitHub issue 145 tracks a brief visible freeze in `SessionList` at the end of background indexing.

The likely path is the reload triggered from `App::handle_indexing_completed`: once indexing finishes, the app can emit `SessionListMsg::Reload`, and `SessionList::reload_sessions()` then synchronously fetches sessions, clears the row factory, pushes all `SessionRow` entries, and restores selection on the main thread.

This is a separate problem from the recent `SessionDetail` responsiveness work. The issue is not yet whether `SessionList` needs batching, diff-based updates, pagination, or asynchronous fetching. The first job is to measure which part of the post-indexing completion reload causes the brief user-visible freeze.

## Goal

Add focused instrumentation that makes the post-indexing completion `SessionList` freeze measurable and actionable.

The instrumentation must distinguish:

- database fetch time;
- factory clear time;
- total row insertion time and row count;
- selection restoration behavior;
- delay after `drop(guard)` until the next main-loop idle callback;
- delay after `drop(guard)` until the next GTK frame callback.

The resulting logs and report must support a narrow recommendation for the next implementation step.

## Non-Goals

- No `SessionList` optimization in this issue.
- No `SessionDetail` transcript rendering changes.
- No redesign of the session list UI.
- No persistent telemetry subsystem or metrics storage.
- No broad instrumentation for every `SessionList` reload path beyond what is needed for comparison or safety.
- No batching, diff-based updates, pagination, or windowing implementation before the bottleneck is measured.

## Recommended Approach

Use small inline instrumentation in the existing post-indexing reload flow, with structured `tracing` fields and `Instant` timestamps, following the same general style used for the recent `SessionDetail` responsiveness instrumentation.

The preferred design is to measure a single reload cycle tagged as `reason = "post_indexing_completion"`, starting from the reload triggered after `handle_indexing_completed` and ending after the first observed idle/frame callback following the row-factory push.

This is preferred over a reusable metrics framework because issue 145 needs a targeted diagnosis, not a new observability layer. Keeping the instrumentation close to the measured code paths keeps the diff small, reduces invalidation risk, and makes the logs easier to interpret.

## Alternatives Considered

### Measure `reload_sessions()` Only

This would log `fetch_sessions`, `guard.clear()`, and `push_back` time only.

Trade-off: the diff would stay tiny, but it would miss the likely GTK/layout work that happens after `drop(guard)`. That would be insufficient for an issue whose visible symptom is a brief freeze at the end of indexing.

### Add A Generic `SessionListMetrics` Subsystem

A reusable struct or helper module could track every reload reason and aggregate metrics across them.

Trade-off: this could be useful later, but it is broader than the issue requires and adds state that is easier to make stale when multiple reloads overlap. It would turn a narrow investigation into framework work.

### Instrument Every Reload Reason Equally

All reloads could receive the same heavy instrumentation, regardless of whether they are caused by indexing, pinning, search, or filter changes.

Trade-off: this would improve comparability, but it would also add noise and make the target symptom harder to isolate. The post-indexing completion path should be the primary measured flow, with other reloads remaining out of scope or lightly tagged only if needed to avoid ambiguity.

## Instrumentation Design

### Trigger Context

When `App::handle_indexing_completed` triggers a `SessionList` reload, pass the post-indexing completion context through a dedicated reload message, for example `SessionListMsg::ReloadAfterIndexing(IndexingReloadContext)`, instead of overloading the existing generic `SessionListMsg::Reload` path.

The context should include:

- `indexed`;
- `skipped`;
- `removed`;
- whether `pending_reindex_feedback` was active;
- whether indexing completed with any errors;
- whether the dedicated reload was emitted directly from completion handling or after project refresh/filter propagation.

This keeps ordinary reloads unambiguous and makes the measured cycle explicitly tied to the end-of-indexing freeze. The context exists to explain why the measured reload happened. It should not introduce new behavior.

If the current project refresh flow causes filters to be resent before the list reloads, the final reload that is still caused by indexing completion should carry the same dedicated context. The plan should choose the smallest code path that preserves current filter behavior while avoiding a broad reload-message refactor.

### Reload Cycle Identity

When `SessionList` starts a measured post-indexing completion reload, assign a simple `reload_id` or equivalent request identifier.

The identifier is used only to correlate:

- the synchronous reload timings;
- the post-`drop(guard)` idle callback;
- the post-`drop(guard)` frame callback;
- invalidation when a newer reload replaces the measured one.

At most one measured post-indexing completion cycle needs to be tracked at a time.

### Synchronous Reload Phases

Inside `SessionList::reload_sessions()`, capture the following separately for the measured post-indexing completion cycle:

- whether `previously_selected_id` was present;
- active AI assistant filters;
- current `project_filter`;
- whether a search query is present and its length;
- `fetch_sessions_duration_ms`;
- `factory_clear_duration_ms`;
- `row_push_duration_ms`;
- `row_count`;
- whether selection restoration was attempted;
- whether selection restoration succeeded;
- whether fallback `ensure_selection()` ran.

The instrumentation should make it obvious whether the synchronous cost is dominated by database fetch, factory clear, or row insertion.

### Post-Drop Main-Loop And Frame Timing

Immediately after `drop(guard)`, schedule two best-effort follow-up measurements:

- a `glib::idle_add_local` callback to capture `next_idle_delay_ms`;
- a GTK tick or frame callback on the list widget to capture `next_frame_delay_ms`.

These callbacks exist to expose work that happens after row insertion has completed from Rust's point of view.

Interpretation:

- a large `next_idle_delay_ms` suggests the main loop did not regain control promptly after the synchronous reload work;
- a large `next_frame_delay_ms` with moderate synchronous timings suggests GTK realization, layout, or paint-adjacent work is the likely user-visible cost.

The wording of these events must avoid overstating what is measured. A frame callback means GTK advanced to the next frame cycle, not necessarily that all visual work is complete.

### Final Summary Event

Emit one final `info` event for the measured cycle, for example `SessionList post-indexing reload measured`, containing:

- `reload_id`;
- trigger context from indexing completion;
- filter/search context;
- `fetch_ms`;
- `clear_ms`;
- `push_ms`;
- `row_count`;
- selection restoration fields;
- `next_idle_delay_ms` when captured;
- `next_frame_delay_ms` when captured;
- `total_reload_ms` for the measured cycle.

Additional sub-phase events may use `debug` if needed, but the final summary event should be sufficient for the report.

Because the idle and frame callbacks complete separately, the measured cycle should keep the synchronous timings plus optional callback results until a final summary can be emitted. Emit the final summary when both idle and frame timings have been captured. If one callback cannot be scheduled because the widget is unavailable, mark that field as unavailable and emit the summary after the remaining callback is captured. If a newer reload invalidates the cycle first, discard the stale pending summary rather than logging it as current.

## Error Handling And Invalidation

The instrumentation must be best-effort and must not affect user behavior.

Rules:

- if indexing completes but the measured reload path does not actually run, no behavior changes and no fatal logging path is added;
- if the target widget is unavailable for the frame callback, the cycle may log incomplete post-drop data rather than failing;
- if a newer reload supersedes the measured cycle before idle or frame callbacks fire, the older cycle is invalidated by `reload_id` and must not report stale timings as current;
- no retries, debouncing, waiting, or scheduling changes are added for this issue.

This keeps the measurement reliable on the normal path without introducing a functional dependency on tracing state.

## Data Safety

Instrumentation must not log session titles, transcript content, search text, tool call payloads, raw command output, or other user data beyond existing safe metadata.

Acceptable fields include:

- counts;
- durations;
- boolean flags;
- enum-like reload reasons or filter states;
- row counts;
- issue-level indexing counts.

If the presence of a search query is useful context, log only presence and length, not the query text.

## Reproduction And Reporting

Manual verification should focus on the exact symptom: the brief `SessionList` freeze at the end of indexing.

Suggested run pattern:

```bash
RUST_LOG=info,sessions_chronicle=debug ~/.local/bin/sessions-chronicle > /tmp/sessions-chronicle-issue-145.log 2>&1
```

Suggested protocol:

1. Start from a clean launch.
2. Wait for background indexing to complete.
3. Observe the transition from indexing completion into the `SessionList` reload.
4. Repeat at least two or three times.
5. Prefer a large real session dataset; use representative fixture data only if it can reproduce similar scale.

The resulting report in `docs/reports/` should include:

- date and environment;
- build and run commands;
- dataset description;
- run-by-run measurements;
- median or observed range for the important fields;
- whether the freeze is user-noticeable at the tested scale;
- the recommended smallest next step.

Initial interpretation threshold:

- treat `total_reload_ms`, `next_idle_delay_ms`, or `next_frame_delay_ms` above roughly 100 ms as potentially user-noticeable;
- treat values above roughly 250 ms as clearly problematic for the end-of-indexing transition;
- use the measured median and observed worst run to define the target threshold for the next fix in the report.

## Decision Rules For The Follow-Up Recommendation

The report should recommend the narrowest next implementation path supported by the measurements:

- if `fetch_ms` dominates, investigate asynchronous fetch or query/index improvements;
- if `clear_ms` dominates, investigate whether full clear/rebuild is the wrong update strategy;
- if `push_ms` dominates, investigate batched insertion or a smaller incremental update path;
- if post-drop idle or frame delay dominates, investigate GTK-facing cost first, such as batched row insertion, pagination, windowing, or another minimal experiment that reduces realization/layout pressure;
- if no single phase dominates, recommend the smallest next experiment instead of a broad rewrite.

The recommendation should explicitly avoid a large refactor unless the measurements justify it.

## Testing Strategy

Automated tests should remain light because the issue adds diagnostics, not user-facing behavior.

Recommended checks:

- `cargo fmt --all -- --check`;
- targeted tests only for any small extracted state or invalidation helper, if one is introduced;
- `cargo test --all --no-fail-fast` only if the final code diff touches enough shared logic to justify it.

Manual verification is the primary validation method for this issue. The important success condition is that the logs clearly distinguish synchronous reload cost from post-drop main-loop/frame delay.

## Acceptance Mapping

- Logs distinguish DB fetch time, factory clear time, row push time, and post-drop idle/frame delay.
- Measurements include row count, filter/search context, indexing completion counts, and selection restoration behavior.
- A reproducible run captures the end-of-indexing freeze on a large session set.
- The report states whether the freeze is user-noticeable at the tested scale and defines the threshold for the next fix.
- The result recommends the narrowest justified next path: async fetch, better query/indexing, diff-based update, batched insertion, pagination/windowing, or a smaller follow-up experiment.
- No intended user-facing behavior change is introduced beyond instrumentation.

## Implementation Decisions

- Keep the instrumentation inline near `handle_indexing_completed` and `SessionList::reload_sessions()`.
- Measure only the post-indexing completion reload heavily; avoid turning every reload into a high-noise diagnostic event.
- Use a single lightweight reload identity to correlate synchronous and asynchronous timing points.
- Use `info` for the final summary event and `debug` for any supporting phase logs.
- Prefer the smallest additional state needed to invalidate stale callbacks and publish one coherent summary.

These decisions keep issue 145 focused on diagnosis and preserve room for a later fix to be chosen from measured evidence instead of guesswork.
