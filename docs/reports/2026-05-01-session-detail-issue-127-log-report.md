# Session Detail Issue 127 Log Report

## Scope

- Date: 2026-05-01
- Target session: `019dc51a-f0cd-79c1-ba79-45fedac889c2`
- AI assistant: Codex
- Source log file: `/tmp/sessions-chronicle-issue-127.log`
- Relevant design: `docs/superpowers/specs/2026-05-01-session-detail-responsiveness-instrumentation-design.md`

## Scenario

This run captured structured tracing for a long Codex session opened in `SessionDetail`.

The session metadata visible in the logs was:

- `session_id`: `019dc51a-f0cd-79c1-ba79-45fedac889c2`
- `message_count`: `244`
- `first_prompt`: `Ton avis sur la branche`
- `file_path`: `/home/mcizo/.codex/sessions/2026/04/25/rollout-2026-04-25T16-46-10-019dc51a-f0cd-79c1-ba79-45fedac889c2.jsonl`

The application was launched with:

```bash
RUST_LOG=info,sessions_chronicle=debug sessions-chronicle > /tmp/sessions-chronicle-issue-127.log 2>&1
```

## Key Observations

### 1. Session loading from the database was not the bottleneck

When the session was selected, the app logged:

- `load_session took 755.427µs`

This indicates the initial session metadata fetch was effectively negligible compared with the user-visible delay.

Relevant log lines:

- `/tmp/sessions-chronicle-issue-127.log:3062-3064`

### 2. First transcript page query time was also very small

The first transcript page loads recorded:

- `load_duration_ms=1` for request `2`
- `load_duration_ms=3` for request `4`

This again suggests the dominant delay is not the transcript page query itself.

Relevant log lines:

- `/tmp/sessions-chronicle-issue-127.log:3093-3095`
- `/tmp/sessions-chronicle-issue-127.log:3248-3250`

### 3. The dominant cost was between page preparation and first factory push completion

The strongest signal in this run was the gap between a fast first-page load and a much later `First transcript page factory push complete` event.

First open cycle:

- `Session detail open started` at line `3073`
- `Loaded first transcript page` at line `3095`
- `Prepared first transcript page` at line `3097`
- `First transcript page factory push complete` at line `3211`
- `open_to_factory_push_ms=7835`
- `total_duration_ms=7792`
- `max_schedule_gap_ms=1409`

Second open/reload cycle:

- `Loaded first transcript page` at line `3250`
- `Prepared first transcript page` at line `3252`
- `First transcript page factory push complete` at line `3362`
- `open_to_factory_push_ms=19045`
- `total_duration_ms=7664`
- `max_schedule_gap_ms=1380`

The consistent pattern is:

- data load is fast;
- preparation is fast;
- user-visible readiness is delayed by the incremental render pipeline and/or main-loop scheduling gaps.

### 4. Search state polluted this capture

This run did not start from a perfectly clean detail state.

The logs show an active search query for the same session ID:

- `SearchQueryChanged("id:019dc51a-f0cd-79c1-ba79-45fedac889c2")`
- `Session detail search update started`
- `SearchPositionsLoaded`
- `SetMatchPositions`

The logs also show a stale request being ignored:

- `Ignoring stale transcript page for session 019dc51a-f0cd-79c1-ba79-45fedac889c2 at offset 0`

This means the capture includes at least one search-driven reload path in addition to the plain open path.

Relevant log lines:

- `/tmp/sessions-chronicle-issue-127.log:3043-3055`
- `/tmp/sessions-chronicle-issue-127.log:3082-3092`
- `/tmp/sessions-chronicle-issue-127.log:3243-3247`

### 5. Tool inspector selection looked fast in this sample

After selecting a tool call in the session detail view, the inspector emitted:

- `Session detail inspector tool call selected`
- `Inspector tool call selection started`
- `ToolCall ... load_duration_ms: 1`

This suggests the inspected tool-call fetch itself was fast for this sample and does not appear to be the main bottleneck compared with transcript rendering.

Relevant log lines:

- `/tmp/sessions-chronicle-issue-127.log:3369-3380`
- `/tmp/sessions-chronicle-issue-127.log:3386`

## Interpretation

This log run supports the original issue-127 hypothesis:

- the slow part is not session metadata loading;
- the slow part is not first-page SQL retrieval;
- the dominant delay is inside the `SessionDetail` render pipeline after data is already available.

More specifically, the measured gap points to work and scheduling in this area:

- transcript row construction;
- Markdown or `TextView`-heavy row setup;
- row factory push batching;
- main-loop scheduling gaps between render batches.

The `max_schedule_gap_ms` values above one second are especially important because they suggest the end-to-end wait is not just raw CPU time spent building rows. The main loop is also taking large pauses between scheduled render batches.

## Limitations

- This was not a clean-room run after indexing completion.
- The capture included an active search query, which triggered an additional reload path.
- The `open_to_factory_push_ms` value from the second cycle (`19045`) should not be treated as a pure session-open metric because it spans a reloaded state, not only the initial open.

## Recommended Next Steps

1. Re-run the same scenario after indexing completes and with no active search query in `SessionDetail`.
2. Capture a clean open of the same session and compare:
   - `open_to_factory_push_ms`
   - `total_duration_ms`
   - `max_schedule_gap_ms`
3. If the clean run still shows multi-second delays, focus the next optimization pass on render scheduling and expensive row/widget construction rather than database work.
4. Keep the inspector instrumentation, but prioritize transcript rendering analysis first because the current sample does not show inspector-side loading as the primary issue.

## Summary

For session `019dc51a-f0cd-79c1-ba79-45fedac889c2`, the logs show that `SessionDetail` responsiveness is dominated by first-page render completion rather than database access.

The strongest metrics from this run were:

- `load_session took 755.427µs`
- first transcript page load in `1-3 ms`
- first factory push completion in roughly `7.8 s`
- large batch scheduling gaps of roughly `1.4 s`

That is exactly the kind of signal issue 127 instrumentation was meant to expose.
