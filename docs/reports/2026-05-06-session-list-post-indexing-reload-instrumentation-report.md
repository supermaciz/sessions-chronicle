# SessionList Post-Indexing Reload Instrumentation Report

## Context

- Issue: [#145 — Measure the visible freeze at the end of SessionList background indexing](https://github.com/supermaciz/sessions-chronicle/issues/145)
- Branch under test: `post-indexing-reload`
- Branch HEAD during measurement: `c20334d`
- Date: 2026-05-06
- Goal: identify whether the post-indexing `SessionList` freeze is dominated by synchronous reload work or by work that continues after `drop(guard)`.

## Scope

- Build/run path: locally installed development binary
- Run command used for each run:

```bash
RUST_LOG=info,sessions_chronicle=debug ~/.local/bin/sessions-chronicle > /tmp/sessions-chronicle-issue-145-runN.log 2>&1
```

- Dataset: default local development dataset with all four AI assistant sources enabled
- Observed list size after post-indexing reload: `row_count=888`
- Observed indexing completion range across runs:
  - `indexed=1291-1296`
  - `skipped=650-655`
  - `removed=0`

This report uses the new `sessionlist.post_indexing_reload.measured` event only. No session titles, transcript content, search text, tool call payloads, or raw command output are copied here.

## Protocol

For each run:

1. Launch the application with debug logs redirected to a file.
2. Wait for background indexing to complete.
3. Capture the single `sessionlist.post_indexing_reload.measured` event emitted for the post-indexing completion cycle.
4. Close the application after the measured reload completes.

Log files:

- `/tmp/sessions-chronicle-issue-145-run1.log`
- `/tmp/sessions-chronicle-issue-145-run2.log`
- `/tmp/sessions-chronicle-issue-145-run3.log`
- `/tmp/sessions-chronicle-issue-145-run4.log`
- `/tmp/sessions-chronicle-issue-145-run5.log`

Measurement note:

- Runs 1-3 were launched in parallel and are kept as supporting data only.
- Runs 4-5 were launched sequentially, one process at a time, and are the more trustworthy confirmation set.

## Run Results

| Run | Mode | indexed | skipped | removed | row_count | fetch_ms | clear_ms | push_ms | next_idle_delay_ms | next_frame_delay_ms | total_reload_ms | User-visible freeze |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | parallel | 1292 | 654 | 0 | 888 | 9 | 230 | 3 | 467 | 9 | 1251 | yes |
| 2 | parallel | 1296 | 650 | 0 | 888 | 10 | 255 | 3 | 496 | 10 | 1336 | yes |
| 3 | parallel | 1292 | 654 | 0 | 888 | 15 | 259 | 3 | 485 | 10 | 1349 | yes |
| 4 | sequential | 1291 | 655 | 0 | 888 | 11 | 222 | 3 | 463 | 10 | 1216 | yes |
| 5 | sequential | 1291 | 655 | 0 | 888 | 9 | 308 | 3 | 469 | 9 | 1301 | yes |

## Sequential Confirmation Set

The sequential reruns keep the same overall shape as the initial parallel captures:

- `fetch_ms`: `9-11 ms`
- `clear_ms`: `222-308 ms`
- `push_ms`: `3 ms`
- `next_idle_delay_ms`: `463-469 ms`
- `next_frame_delay_ms`: `9-10 ms`
- `total_reload_ms`: `1216-1301 ms`

Even without overlap from concurrent launches, the reload still shows a clearly user-visible pause dominated by `clear_ms` plus the large post-drop idle delay.

## Aggregate Medians And Worst Case

| Field | Median | Worst run |
| --- | ---: | ---: |
| `fetch_ms` | 10 ms | 15 ms |
| `clear_ms` | 255 ms | 308 ms |
| `push_ms` | 3 ms | 3 ms |
| `next_idle_delay_ms` | 469 ms | 496 ms |
| `next_frame_delay_ms` | 10 ms | 10 ms |
| `total_reload_ms` | 1301 ms | 1349 ms |

## Interpretation

### What is clearly not the bottleneck

- Database fetch is small: `9-15 ms`.
- Row insertion into the factory is also small: `3 ms` in every run.
- The first frame callback arrives quickly once it is scheduled: `9-10 ms`.

These numbers argue strongly against database access or `push_back` itself being the dominant source of the freeze.

### What is expensive

- `guard.clear()` is already expensive on its own: `222-308 ms`.
- The largest delay is still after `drop(guard)`: `next_idle_delay_ms=463-496 ms`.
- End-to-end measured reload time is consistently high: `1216-1349 ms`.

The pattern remains stable in the sequential reruns and matches the earlier parallel captures: synchronous fetch and push are cheap, but the full clear/rebuild cycle is expensive and GTK/main-loop work remains backed up after the guard is dropped.

### User-facing severity

At this dataset scale, the freeze is user-noticeable.

Reasoning:

- `clear_ms` alone is far above a 60 Hz frame budget (`~16 ms`) and above the `~100 ms` threshold where transitions stop feeling instantaneous.
- `next_idle_delay_ms` near half a second means the main loop does not regain control promptly after the synchronous reload path.
- `total_reload_ms` above 1.2 seconds makes the end-of-indexing transition clearly visible even though the user did not explicitly request it.

## Decision

Dominant phase: post-drop idle delay, with a secondary synchronous cost in full factory clear.

Recommended next step: reduce GTK realization/layout pressure with a narrow batched-insertion experiment.

Why this is the narrowest justified follow-up:

- `push_ms` is only `3 ms`, so the problem is not raw per-row `push_back` throughput.
- `clear_ms` is substantial and the current strategy still rebuilds the entire list in one cycle.
- `next_idle_delay_ms` is the largest measured phase, which points at work that GTK or the main loop must process after the list has been cleared and rebuilt.
- Batched insertion is smaller and safer than jumping directly to pagination or windowing, while still targeting the measured pressure point from the report.

## Recommendation

The next implementation issue should test one constrained change first:

1. Keep the current fetch query.
2. Replace the single full repopulation pass with batched row insertion after clear.
3. Re-run this exact measurement protocol.
4. Compare `clear_ms`, `next_idle_delay_ms`, and `total_reload_ms` against this report.

Success criterion for that follow-up:

- materially reduce `next_idle_delay_ms` from the current `469 ms` aggregate median
- materially reduce `total_reload_ms` from the current `1301 ms` aggregate median
- preserve current selection restoration behavior

If batched insertion does not materially reduce the post-drop idle delay, the next investigation should move to more structural list-update strategies such as diff-based updates or pagination/windowing.

## Evidence Summary

- The instrumentation successfully separated synchronous fetch, clear, push, selection, idle, and frame phases.
- The measured cycle was reproduced five times on a realistic local dataset, including two sequential reruns without concurrent launch overlap.
- The freeze is explainable with measured data rather than guesswork.
- The narrowest evidence-backed next step is to reduce post-clear GTK pressure, starting with batched insertion.
