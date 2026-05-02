# Session Detail Issue 140 Full-App Metrics - Design

## Context

The current `more-metrics` branch adds useful row-build instrumentation, but the main manual report for issue #140 was generated from an isolated `SessionDetail` harness rather than the real application open path.

That isolated measurement is useful as a complementary diagnostic, but it is not directly comparable with the earlier issue #127 report, which was captured by launching the app, waiting for indexing to settle, and clicking the target session.

## Goal

Make the row-build breakdown metrics available in the real application path when the app is launched with `RUST_LOG=debug`, so issue #140 can be measured with the same workflow as issue #127.

## Non-Goals

- No new telemetry framework.
- No dedicated environment flag beyond `RUST_LOG`.
- No behavior change in transcript rendering.
- No attempt to make the isolated harness the primary measurement artifact.

## Recommended Approach

Keep the existing row-build instrumentation structure, but remove the `#[cfg(debug_assertions)]` gates from the minimum set of types and code paths needed to emit and aggregate row-build events in normal application builds.

The metrics remain log-driven and only become visible when the operator opts into debug logging with `RUST_LOG=debug`.

## Alternatives Considered

### Isolated Harness Only

Keep the current `cargo test`-driven measurement as the sole issue-140 artifact.

Rejected because it removes full-app scheduling, navigation, and main-loop contention, which are the main suspects from issue #127.

### Dedicated Runtime Flag

Add a new environment variable such as `SESSION_CHRONICLE_RENDER_METRICS=1`.

Rejected because it adds configuration surface without solving a real problem. `RUST_LOG=debug` already provides an explicit opt-in channel for diagnostic output.

### Full-App Logs Plus Isolated Harness

Use the full application path as the primary measurement and keep the isolated harness as a secondary, reproducible diagnostic.

Accepted because it preserves comparability with issue #127 while keeping the new focused measurement available for follow-up analysis.

## Design

### Runtime Availability

The following row-build instrumentation pieces move out of `#[cfg(debug_assertions)]` so they are compiled into normal application builds:

- `TranscriptItemInit::item_index()`
- `TranscriptRowBuildKind`
- `TranscriptRowOutput::RowBuilt`
- `SessionDetailMsg::RowBuilt`
- the per-item batch correlation map stored in `PendingRenderBatch`
- `record_transcript_row_build`

This keeps the design small and reuses the existing instrumentation path.

### Logging Behavior

The detailed row-build and per-batch breakdown events stay at `debug` level.

That means:

- regular runs do not surface the extra metrics unless debug logging is enabled;
- full-app issue-140 captures can be produced with the same launch flow as issue #127, just with `RUST_LOG=info,sessions_chronicle=debug` or equivalent;
- no separate feature flag or settings UI is needed.

### Measurement Workflow

The primary issue-140 workflow becomes:

1. Launch the real application with debug logs enabled.
2. Wait for background indexing to finish.
3. Click the same reference session used in issue #127.
4. Read the existing first-page timing logs together with the new row-build breakdown logs.

The isolated ignored test remains available for supplementary analysis when a reproducible component-only run is useful.

### Reporting

The issue-140 report must be framed around the full-app run.

If the isolated test report is kept, it should be explicitly labeled as a complementary component-only measurement rather than the primary decision-grade artifact.

## Testing Strategy

- Run `cargo fmt --all -- --check`.
- Run focused `SessionDetail` tests.
- Run the relevant ignored/manual test only if needed to confirm the isolated harness still works.

## Acceptance Criteria

- A real app run with `RUST_LOG=debug` emits the row-build breakdown events added on `more-metrics`.
- The existing first-page completion metrics remain unchanged in meaning.
- The isolated harness still works as a complementary measurement.
- Issue-140 conclusions can be based on a real full-app capture comparable to issue #127.
