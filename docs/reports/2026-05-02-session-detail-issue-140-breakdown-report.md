# Session Detail Issue 140 Breakdown Report

## Scope

- Date: 2026-05-02
- Target session: `019dc51a-f0cd-79c1-ba79-45fedac889c2`
- AI assistant: Codex
- Reference baseline: `docs/reports/2026-05-01-session-detail-issue-127-clean-run-log-report.md`
- Source log file: `/tmp/sessions-chronicle-issue-140-full-app-rerun.log`
- Measurement type: real full-app run
- Measurement command:

```bash
RUST_LOG=info,sessions_chronicle=debug sessions-chronicle > /tmp/sessions-chronicle-issue-140-full-app-rerun.log 2>&1
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

- `/tmp/sessions-chronicle-issue-140-full-app-rerun.log:2999`

### The detail view opened with no active search query

The `SetSession` payload shows:

- `search_query: None`

Relevant log lines:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun.log:3044-3047`

## Key Measurements

### 1. Session open started normally

The app logged:

- `Session detail open started`
- `request_id=1`
- `message_count=244`
- `has_search_query=false`

Relevant log line:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun.log:3047`

### 2. First transcript page loading remained negligible

The first transcript page load logged:

- `load_duration_ms=1`

Relevant log line:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun.log:3062`

### 3. First-page preparation remained effectively free

The preparation step logged:

- `Prepared first transcript page`
- `source_row_count=75`
- `display_item_count=33`
- `build_duration_ms=0`

Relevant log line:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun.log:3064`

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
- `total_duration_ms=7452`
- `max_schedule_gap_ms=1361`
- `first_page_load_to_factory_push_ms=7766`

Relevant log line:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun.log:3304`

### 5. Row-build instrumentation is now visible in the real run

The final row-build breakdown logged:

- `row_build_count=33`
- `message_build_duration_ms=6`
- `tool_call_build_duration_ms=0`
- `tool_burst_build_duration_ms=0`
- `subagent_build_duration_ms=0`
- `total_row_build_duration_ms=6`
- `worst_row_kind=Some(Message)`
- `worst_row_build_duration_ms=1`
- `max_post_drop_residual_ms=1361`

Relevant log line:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun.log:3319`

### 6. Per-batch breakdown confirms scheduling dominates the delay

Representative batch breakdowns:

- batch 2: `schedule_gap_ms=662`, `measured_row_build_ms=0`, `post_drop_residual_ms=662`
- batch 4: `schedule_gap_ms=1361`, `measured_row_build_ms=0`, `post_drop_residual_ms=1361`
- batch 8: `schedule_gap_ms=1332`, `measured_row_build_ms=1`, `post_drop_residual_ms=1331`
- batch 10: `schedule_gap_ms=1339`, `measured_row_build_ms=1`, `post_drop_residual_ms=1338`
- batch 11: `schedule_gap_ms=661`, `measured_row_build_ms=0`, `post_drop_residual_ms=661`

Relevant log lines:

- `/tmp/sessions-chronicle-issue-140-full-app-rerun.log:3111`
- `/tmp/sessions-chronicle-issue-140-full-app-rerun.log:3156`
- `/tmp/sessions-chronicle-issue-140-full-app-rerun.log:3249`
- `/tmp/sessions-chronicle-issue-140-full-app-rerun.log:3294`
- `/tmp/sessions-chronicle-issue-140-full-app-rerun.log:3317`

## First-Page Breakdown

The first transcript page produced:

| Metric | Value |
| --- | ---: |
| Source transcript rows | 75 |
| Display rows after grouping | 33 |
| Render batches | 11 |
| Total page render duration | 7454 ms |
| First-page load to factory push | 7766 ms |
| Total factory push duration | 1 ms |
| Max schedule gap | 1361 ms |
| Max post-drop residual | 1361 ms |

Row mix:

| Row kind | Count | Measured build time |
| --- | ---: | ---: |
| Message | 20 | 6 ms |
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
| Issue #140 full app run | 7766 ms | 1361 ms | 6 ms | 1361 ms |

## Interpretation

This full-app run reproduces the issue #127 latency almost exactly, but now with row-build breakdown inside the real application path.

The strongest conclusions are:

- database loading is still negligible (`load_duration_ms=1`);
- first-page preparation is still negligible (`build_duration_ms=0`);
- row widget construction is also negligible in aggregate (`total_row_build_duration_ms=6`);
- the dominant delay remains between scheduled render batches on the main loop;
- `max_post_drop_residual_ms=1361` exactly matches `max_schedule_gap_ms=1361`, which means the long gaps are not explained by row construction itself.

The full-app measurement therefore supports the same top-level diagnosis as issue #127, but more strongly than before: the bottleneck is not transcript fetch, transcript preparation, or row construction. It is scheduling or other main-loop contention around the detail-open path.

The isolated component run is still useful as a contrast, but it is no longer necessary to infer the problem indirectly. The real run now shows the same story directly.

## Recommendation

Prioritize #132 as a narrow full-app scheduling and main-loop investigation.

Defer #133. Markdown render caching is not supported by this run because total measured message row construction was only `6 ms`.

Defer #134. Transcript virtualization or windowing would reduce mounted rows, but the first page still mounts only `33` display rows and row construction is not the bottleneck.

Defer #138. Off-main-thread data preparation is not supported by this run because transcript fetch and first-page preparation are already negligible.

Do not close #132 from this run alone, but this report materially narrows it: the next decision-grade investigation should target why the full app experiences repeated 660-1361 ms scheduling gaps after batches are queued, not why transcript rows are expensive to build.
