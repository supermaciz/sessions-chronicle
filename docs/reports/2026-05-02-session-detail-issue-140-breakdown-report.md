# Session Detail Issue 140 Breakdown Report

## Scope

- Date: 2026-05-02
- Target session: `019dc51a-f0cd-79c1-ba79-45fedac889c2`
- AI assistant: Codex
- Reference baseline: `docs/reports/2026-05-01-session-detail-issue-127-clean-run-log-report.md`
- Measurement command:

```bash
SESSION_DETAIL_ISSUE_140_SOURCE_FILE=/home/mcizo/.codex/sessions/2026/04/25/rollout-2026-04-25T16-46-10-019dc51a-f0cd-79c1-ba79-45fedac889c2.jsonl \
  RUST_LOG=info,sessions_chronicle=debug \
  cargo test session_detail_issue_140_reference_session_breakdown -- --ignored --nocapture
```

## Scenario

The issue-140 measurement copied the local Codex session file into a temporary Codex session root, indexed it into a temporary database, and opened it through the `SessionDetail` component test harness.

This keeps the input session directly comparable with issue #127 while removing full-app noise from session list rendering, background indexing, and manual click timing.

## First-Page Breakdown

The first transcript page produced:

| Metric | Value |
| --- | ---: |
| Source transcript rows | 75 |
| Display rows after grouping | 33 |
| Render batches | 11 |
| Total page render duration | 263 ms |
| First-page load to factory push | 519 ms |
| Total factory push duration | 2 ms |
| Max schedule gap | 69 ms |
| Max post-drop residual | 68 ms |

Row mix:

| Row kind | Count | Measured build time |
| --- | ---: | ---: |
| Message | 20 | 57 ms |
| ToolCall | 1 | 0 ms |
| ToolBurst | 12 | 0 ms |
| Subagent | 0 | 0 ms |

Worst row:

- Kind: `Message`
- Duration: 49 ms

## Interpretation

In the isolated `SessionDetail` run, row construction does not explain the previous clean-run latency.

The most important contrast with issue #127 is:

| Run | First-page completion | Max schedule gap | Total measured row build |
| --- | ---: | ---: | ---: |
| Issue #127 full app clean run | ~7.9 s | 1398 ms | not measured |
| Issue #140 isolated component run | 519 ms | 69 ms | 57 ms |

The new row-build instrumentation shows that:

- `Message` rows are the only non-zero measured row-build cost in this reference first page.
- Collapsed `ToolBurst` rows are not expensive at mount time in this run.
- The first-page cost is not dominated by factory `push_back`.
- The large issue-127 delay is not reproduced when `SessionDetail` is measured in isolation.

That points away from database loading, transcript preparation, and ordinary row widget construction as the primary explanation. The remaining hypothesis is full-app main-loop contention or scheduling noise around the detail open path.

## Recommendation

Prioritize #132 only as a narrow scheduling/main-loop investigation, not as a blind adaptive-batch implementation. The data says schedule gaps are the suspicious dimension in the full app, but the isolated component does not reproduce the 7.9 s delay.

Defer #133. Markdown render caching is not supported by this first-page breakdown because total message row construction was 36 ms.

Defer #134. Transcript virtualization/windowing would reduce mounted rows, but this reference first page mounted only 33 display rows and row construction was not the bottleneck.

Defer #138. Off-main-thread data preparation is not supported by either issue #127 or issue #140: database load and first-page preparation are already negligible.

Do not close #132, #133, #134, or #138 from this run alone. The next decision-grade measurement should run the same instrumentation in the full app clean-run scenario to confirm whether the previous 1398 ms schedule gaps now appear as `post_drop_residual_ms`.
