# Session Detail Responsiveness Instrumentation - Design

## Context

GitHub issue 127 tracks focused instrumentation for `SessionDetail` responsiveness. The remaining problem is not known to be database-bound; the likely cost is synchronous GTK work around transcript row creation, Markdown and `TextView` rendering, render-batch scheduling, clear/rebuild paths, search updates, and inspector-side rerenders.

Some render-batch metrics already exist in `src/ui/session_detail.rs`: queued transcript render batches, per-batch rendering, final page render metrics, row-kind counts, push durations, and maximum schedule gap. This design fills the remaining gaps without changing user-facing behavior.

## Goal

Add targeted tracing instrumentation that makes the dominant `SessionDetail` costs visible on large sessions.

The instrumentation must cover:

- session open;
- first visible transcript content;
- transcript render batches;
- clear and rebuild paths;
- in-session search updates;
- inspector interactions and inspector-side rerenders.

## Non-Goals

- No transcript rendering optimization.
- No search behavior change.
- No database query redesign.
- No inspector pane replacement.
- No virtualization, `GtkListView`, or `AsyncComponent` migration.
- No persistent analytics schema or user-facing metrics UI.

## Recommended Approach

Use targeted inline instrumentation with `Instant` and structured `tracing` fields in the existing `SessionDetail` and `ToolInspectorPane` code paths.

This is preferred over a new metrics abstraction because the issue needs diagnostic logs, not a durable telemetry subsystem. Keeping the instrumentation close to the measured work reduces risk and keeps the diff small.

## Alternatives Considered

### Dedicated `SessionDetailMetrics` State

A small internal struct could track `opened_at`, `first_page_loaded_at`, `first_render_finished_at`, and search/rebuild timings.

Trade-off: this would improve correlation, but it adds state that is easy to make stale across request invalidation, navigation back, and search reloads. It is more structure than the current issue requires.

### Span-Heavy Tracing Model

Large paths could be wrapped in `tracing` spans and analyzed with external collectors.

Trade-off: this is cleaner for long-term observability, but broader and more intrusive than needed. The current app primarily uses logs, so structured event fields are enough.

## Instrumentation Design

### Session Open

When `SessionDetailMsg::SetSession` is handled, log a `Session detail open started` event with:

- `request_id` after transcript invalidation;
- `session_id`;
- `message_count`;
- whether a search query is already active;
- `query_len` when applicable.

The deferred first-page load path should log `Session detail deferred first page load started` when the delayed message is accepted. Include:

- `request_id`;
- `session_id`;
- configured delay in milliseconds;
- actual elapsed delay if a start timestamp is available without adding fragile state.

If exact elapsed delay would require extra model state, log the configured delay only. The render-batch metrics will still expose main-loop starvation through schedule gaps.

### First Visible Transcript Content

The existing `Finished rendering transcript page` event should remain the final render-page metric. When `offset == 0`, it also represents the first fully rendered visible transcript content for the first page.

Add a specific `First transcript page visible` event immediately before or after that final event for `offset == 0`, with:

- `request_id`;
- `source_row_count`;
- `display_item_count`;
- `batch_count`;
- `total_duration_ms`;
- `total_push_duration_ms`;
- `max_push_duration_ms`;
- `max_schedule_gap_ms`.

This event should not imply a GTK frame has been painted. It means the first page has been pushed into the transcript row factory and is available for GTK to render.

### Render Batches

Keep the existing render-batch instrumentation:

- `Queued transcript render batch`;
- `Rendered transcript batch`;
- `Finished rendering transcript page`.

No new behavior is required here. The only acceptable changes are small field additions needed to correlate with the new session-open and search/rebuild events.

### Clear And Rebuild Paths

Instrument clear paths that can synchronously remove many GTK rows:

- `start_first_page_load`, before and after `clear_messages_safely`;
- `clear_for_navigation_back`;
- `SessionDetailMsg::Clear`;
- search-driven reloads that call `reload_current_session`.

Events should include:

- reason, for example `open`, `search`, `clear_search`, `navigation_back`, or `component_clear`;
- number of factory rows before clear;
- duration of `clear_messages_safely`;
- whether pending render state was present.

If possible, use a small internal enum or string literal reason passed to the existing helper methods. Do not add broad refactoring only to support these labels.

### Search Updates

When `SessionDetailMsg::UpdateSearchQuery` is handled, log `Session detail search update started` with:

- `session_id` when a session is active;
- normalized query presence;
- `query_len`;
- previous match count;
- whether the update will load match positions or reload without a query.

When `SearchPositionsLoaded` is handled, log `Session detail search positions loaded` with:

- `request_id`;
- `session_id`;
- success or error;
- `match_count`;
- load duration if recorded in the command output.

If the command output does not currently carry load duration, add a `load_duration_ms` field to `SessionDetailCmd::SearchPositionsLoaded`. This mirrors the existing transcript page load timing.

When `SetMatchPositions` starts the first-page reload and optional jump, log:

- `match_count`;
- whether an initial jump is scheduled;
- current loaded row count before rebuild.

### Inspector Interactions

In `SessionDetail`, log the user-facing inspector interactions before sending messages to `ToolInspectorPane`:

- inspect tool call;
- inspect subagent;
- inspect reasoning;
- toggle inspector;
- close inspector;
- widget visibility changes.

Fields should include `session_id`, selected object ID or transcript item index, previous open state, and new open state where applicable.

### Inspector-Side Updates

In `ToolInspectorPane`, instrument:

- selection start for tool call, subagent, and reasoning;
- database load completion for each selection type;
- `post_view` duration;
- renderer selection and widget generation duration;
- subagent tool row rebuild duration and count;
- drill-down load and render duration.

Use structured fields such as:

- `request_id`;
- `session_id`;
- `tool_call_id`;
- `subagent_id`;
- `transcript_item_index`;
- `renderer_kind`;
- `subagent_tools_count`;
- `duration_ms`.

Avoid logging full prompt, result, input, output, or error text. Session transcripts are user data and can be large or sensitive.

## Data Safety

Instrumentation must not log transcript content, tool input/output, Markdown text, file contents, command output, or raw error payloads beyond existing error messages.

IDs, counts, durations, row kinds, and renderer kinds are acceptable. Existing error logs may remain unchanged.

## Expected Log Shape

The logs should make these comparisons possible from a single run:

- session open to first transcript page queued;
- database load versus display-item preparation;
- preparation versus factory push time;
- factory push time versus schedule gaps;
- clear duration before rebuild;
- search position query duration versus search-triggered rebuild;
- inspector DB load versus inspector render/update cost.

## Testing Strategy

Automated tests should stay minimal because this issue adds diagnostics rather than behavior.

Run focused checks:

- `cargo fmt --all -- --check`;
- `cargo test session_detail::tests -- --nocapture`;
- if `ToolInspectorPane` changes are broad enough to risk compile regressions, run `cargo test --all --no-fail-fast`.

Manual verification should use logs on at least two or three slow scenarios:

- open a large real-world or fixture session;
- search for a term with matches outside the first page;
- open inspector details for a large terminal/diff/results tool call or subagent.

Suggested launch command:

```bash
RUST_LOG=info,sessions_chronicle=debug /home/mcizo/.local/bin/sessions-chronicle > /tmp/sessions-chronicle-issue-127.log 2>&1
```

The resulting log should contain all issue-127 coverage areas: open, first visible transcript content, render batches, clear/rebuild, search update, and inspector-side updates.

## Acceptance Mapping

- Logs exist for session open: `Session detail open started` and deferred first-page load events.
- Logs exist for first visible transcript content: `First transcript page visible` for `offset == 0`.
- Logs exist for render batches: existing queued/per-batch/final render events remain.
- Logs exist for clear/rebuild: clear duration and reason events around `clear_messages_safely`.
- Logs exist for search updates: search update, positions load, and match application events.
- Logs exist for inspector-side updates: interaction, load, post-view, renderer, and rebuild events.
- Slow scenarios can be measured manually with structured logs and no user-facing behavior changes.

## Implementation Decisions

- Do not add a broad metrics state object. Use local `Instant::now()` measurements and existing request IDs unless one timestamp field is necessary for an accurate stage duration.
- Measure clear duration with a small wrapper/helper around `clear_messages_safely` that accepts a static reason string. Avoid changing clear behavior.
- Use `info` for lifecycle milestones that map to issue-127 acceptance criteria, and `debug` for high-frequency or noisy details such as individual inspector render passes.

These choices keep the implementation focused on diagnostics and avoid turning issue 127 into a telemetry framework.
