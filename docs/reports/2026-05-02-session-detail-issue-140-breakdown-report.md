# Session Detail Issue 140 Breakdown Report

## Scope

- Date: 2026-05-02
- Target session: `019dc51a-f0cd-79c1-ba79-45fedac889c2`
- AI assistant: Codex
- Reference baseline: `docs/reports/2026-05-01-session-detail-issue-127-clean-run-log-report.md`
- Source log file: `/tmp/sessions-chronicle-issue-140-full-app-rerun-2.log`
- Measurement type: real full-app run
- Measurement command:

```bash
RUST_LOG=info,sessions_chronicle=debug sessions-chronicle > /tmp/sessions-chronicle-issue-140-full-app-rerun-2.log 2>&1
```

## Scenario

This run used the same workflow as the issue #127 clean baseline:

1. Launch the installed application with debug logs redirected to a file.
2. Wait for background indexing to complete.
3. Click session `019dc51a-f0cd-79c1-ba79-45fedac889c2`.
4. Do not perform any search before opening the session detail view.

Unlike the earlier isolated component run, this capture includes the real app navigation path, main-loop scheduling, and full GTK workload around opening the detail view.

## Cleanliness Checks

### Indexing finished before the click

The app logged:

- `Background indexing complete: indexed=1244, skipped=662, removed=0`

Relevant log line:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun-2.log` (indexing-complete line from this rerun)

### The detail view opened with no active search query

The `SetSession` payload shows:

- `search_query: None`

Relevant log lines:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun-2.log` (`SetSession` for this rerun)

## Key Measurements

### 1. Session open started normally

The app logged:

- `Session detail open started`
- `request_id=1`
- `message_count=244`
- `has_search_query=false`

Relevant log line:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun-2.log` (session-open start for this rerun)

### 2. First transcript page loading remained negligible

The first transcript page load logged:

- `load_duration_ms=2`

Relevant log line:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun-2.log:3064`

### 3. First-page preparation remained effectively free

The preparation step logged:

- `Prepared first transcript page`
- `source_row_count=75`
- `display_item_count=33`
- `build_duration_ms=0`

Relevant log line:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun-2.log:3066`

### 4. The full-app first-page delay is still present

The main completion event logged:

- `First transcript page factory push complete`

Recorded values:

- `request_id=1`
- `source_row_count=75`
- `display_item_count=33`
- `batch_count=11`
- `total_push_duration_ms=1`
- `max_push_duration_ms=0`
- `total_duration_ms=7834`
- `max_schedule_gap_ms=1440`
- `first_page_load_to_factory_push_ms=8211`

Relevant log line:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun-2.log:3306`

### 5. Row-build instrumentation is now visible in the real run

The final row-build breakdown logged:

- `row_build_count=33`
- `message_build_duration_ms=4`
- `tool_call_build_duration_ms=0`
- `tool_burst_build_duration_ms=0`
- `subagent_build_duration_ms=0`
- `total_row_build_duration_ms=4`
- `worst_row_kind=Some(Message)`
- `worst_row_build_duration_ms=1`
- `max_post_drop_residual_ms=1439`

Relevant log line:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun-2.log:3321`

### 6. Per-batch breakdown confirms scheduling dominates the delay

This rerun kept the same per-batch shape: multi-hundred-millisecond schedule gaps dominate, while measured row-build time stays near zero to one millisecond per batch.

Relevant log lines:

- See `/tmp/sessions-chronicle-issue-140-full-app-rerun-2.log` for per-batch debug lines from this rerun.

## First-Page Breakdown

The first transcript page produced:

| Metric | Value |
| --- | ---: |
| Source transcript rows | 75 |
| Display rows after grouping | 33 |
| Render batches | 11 |
| Total page render duration | 7835 ms |
| First-page load to factory push | 8211 ms |
| Total factory push duration | 1 ms |
| Max schedule gap | 1440 ms |
| Max post-drop residual | 1439 ms |

Row mix:

| Row kind | Count | Measured build time |
| --- | ---: | ---: |
| Message | 20 | 4 ms |
| ToolCall | 1 | 0 ms |
| ToolBurst | 12 | 0 ms |
| Subagent | 0 | 0 ms |

Worst row:

- Kind: `Message`
- Duration: `1 ms`

## Comparison With Prior Measurements

| Run | First-page completion | Max schedule gap | Total measured row build | Max post-drop residual |
| --- | ---: | ---: | ---: | ---: |
| Issue #127 full app clean run | ~7.9 s | 1398 ms | not measured | not measured |
| Issue #140 isolated component run | 519 ms | 69 ms | 57 ms | 68 ms |
| Issue #140 full app run | 8211 ms | 1440 ms | 4 ms | 1439 ms |

## Interpretation

This full-app run reproduces the issue #127 latency again, but now with row-build breakdown inside the real application path.

The strongest conclusions are:

- database loading is still negligible (`load_duration_ms=2`);
- first-page preparation is still negligible (`build_duration_ms=0`);
- row widget construction is also negligible in aggregate (`total_row_build_duration_ms=4`);
- the dominant delay remains between scheduled render batches on the main loop;
- `max_post_drop_residual_ms=1439` remains effectively equal to `max_schedule_gap_ms=1440`, which means the long gaps are not explained by row construction itself.

The full-app measurement therefore supports the same top-level diagnosis as issue #127, but more strongly than before: the bottleneck is not transcript fetch, transcript preparation, or row construction. It is scheduling or other main-loop contention around the detail-open path.

The isolated component run is still useful as a contrast, but it is no longer necessary to infer the problem indirectly. The real run now shows the same story directly.

## Recommendation

Prioritize #132 as a narrow full-app scheduling and main-loop investigation.

Defer #133. Markdown render caching is not supported by this run because total measured message row construction was only `4 ms`.

Defer #134. Transcript virtualization or windowing would reduce mounted rows, but the first page still mounts only `33` display rows and row construction is not the bottleneck.

Defer #138. Off-main-thread data preparation is not supported by this run because transcript fetch and first-page preparation are already negligible.

Do not close #132 from this run alone, but this report materially narrows it: the next decision-grade investigation should target why the full app experiences repeated 660-1440 ms scheduling gaps after batches are queued, not why transcript rows are expensive to build.
