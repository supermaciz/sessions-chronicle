# SessionDetail Issue 146 Post-Drop Investigation - Design

## Context

GitHub issue [#146](https://github.com/supermaciz/sessions-chronicle/issues/146) asks for a decision-grade investigation of the remaining full-app freeze when opening the large Codex reference session `019dc51a-f0cd-79c1-ba79-45fedac889c2`.

The prior evidence is consistent:

- #127 and #140 showed that database loading, transcript preparation, Rust row construction, and factory push time do not explain the freeze.
- #142 showed that replacing full transcript rows with minimal labels reduced median `max_schedule_gap_ms` from `1346 ms` to `21 ms`, while disabling NavigationView animation and bypassing Markdown/highlighting did not help.
- #143 showed that shell-only lazy row construction can pass at `18 ms`, while the real full-app manual path remained around `1364 ms` median `max_schedule_gap_ms`.
- The strongest signal remains `max_post_drop_residual_ms ~= max_schedule_gap_ms`, which points to work after `drop(guard)` rather than synchronous `FactoryVecDeque` push cost.

This design allows a light re-scope of the issue: the investigation may add temporary probes outside `SessionDetail` when needed to identify the likely owner of the stall. The priority is to name the most likely suspect, not to build reusable telemetry.

## Goal

Identify the most likely owner of the remaining full-app schedule gap well enough to choose the next implementation issue.

The investigation should distinguish these candidate owners:

- GTK layout, realization, allocation, or frame pipeline work.
- Relm4 or GLib main-loop scheduling behavior.
- Parent view, navigation, sidebar, search, or inspector work triggered by opening `SessionDetail`.
- Transcript row content, only if new evidence contradicts #142/#143.
- Another concrete path found by logs or profiler.

## Non-Goals

- Do not implement lazy rows, row/window virtualization, adaptive batching, or a new rendering architecture.
- Do not optimize Markdown rendering, syntax highlighting, database loading, or transcript preparation unless the measurements contradict prior reports.
- Do not redesign the `SessionDetail` UI.
- Do not treat shell-only benchmarks as proof of fixing the full-app responsiveness path.
- Do not keep this instrumentation as permanent product telemetry.
- Do not log transcript content, tool call payloads, command output, or Markdown bodies.

## Recommended Approach

Use a targeted `probe ladder` around the post-factory-push interval.

The probe ladder records the exact time when the factory guard is dropped, then measures how long it takes to reach the next main-loop idle callback and the next frame/tick callback. It also runs a temporary heartbeat during the `SessionDetail` open and adds a small number of neighboring probes around parent view updates. After the logs identify the suspicious interval, take one short Sysprof capture to confirm or reject the leading suspect.

This approach is preferred over broad instrumentation or profiler-first analysis because it directly targets the known signal: the unexplained interval after `drop(guard)`.

## Measurement Architecture

### `SessionDetail` Post-Drop Probes

For each transcript render batch, keep the existing metrics and add an explicit `after_drop_at` timestamp immediately after `drop(guard)`.

From `after_drop_at`, schedule:

- One `glib::idle_add_local_once` callback.
- One `WidgetExt::add_tick_callback` callback on the transcript factory widget or the transcript `ScrolledWindow`, whichever is already rooted and easiest to access without broad widget plumbing.
- A second tick callback only if the first tick fires before the largest observed schedule gap and therefore cannot explain layout/frame cost.

Log fields should include:

- `request_id`
- `session_id` when available
- `offset`
- `render_batch_index`
- `rendered_this_batch`
- `rendered_items`
- `remaining_items`
- `display_item_count`
- display item kind counts
- `push_duration_ms`
- `schedule_gap_ms`
- `max_schedule_gap_ms`
- `after_drop_to_idle_ms`
- `after_drop_to_first_frame_ms`
- `after_drop_to_second_frame_ms` when used

The probes should follow the existing `SessionList` pattern that already schedules post-drop idle and frame measurements after `FactoryVecDeque` updates.

### Neighboring Owner Probes

Add short, temporary logs around app work that may run during the same interval but is not row construction:

- Navigation or detail-view activation triggered by selecting the session.
- Sidebar or project/filter updates that may be emitted around detail open.
- Search state and inspector updates related to the newly selected session.
- Scroll adjustment values for the transcript `ScrolledWindow`, when accessible without invasive widget reads.

These logs should be sparse. Their purpose is correlation, not full tracing.

### Main-Loop Heartbeat

During the reference `SessionDetail` open, run a temporary main-loop heartbeat. Use a lightweight repeating timeout around one frame interval, then log heartbeat gaps at or above `50 ms`; this threshold is high enough to avoid normal frame jitter and low enough to catch stalls relevant to the `<= 50 ms` responsiveness target used by prior work.

The heartbeat timeline should make it possible to line up:

- The first page load.
- Each batch push and `drop(guard)`.
- Large main-loop stalls.
- The next idle and frame callbacks.
- Neighboring owner probes.

The heartbeat should be active only during the measured open window, not for the full app lifetime.

## Protocol

### Instrumentation Pass

Use the reference Codex session `019dc51a-f0cd-79c1-ba79-45fedac889c2`.

Run protocol:

1. Build the app with the temporary probes.
2. Launch the app with `RUST_LOG=info,sessions_chronicle=debug` and redirect logs to a file.
3. Wait for background indexing to complete.
4. Open the reference session exactly once.
5. Do not perform session detail search before opening it.
6. Wait for the detail view to settle, then close the app.
7. Repeat for three runs if the first run reproduces the stall.

Extract these measurements from the logs:

- `max_schedule_gap_ms`
- `max_post_drop_residual_ms`
- `after_drop_to_idle_ms`
- `after_drop_to_first_frame_ms`
- `after_drop_to_second_frame_ms` when present
- heartbeat stall timeline
- batch context for the largest gap
- neighboring owner events near the largest gap
- scroll adjustment values around the largest gap, when available

If the first run does not reproduce the stall, stop the three-run protocol and document the reproduction failure instead of forcing a conclusion.

### Sysprof Pass

Run one focused Sysprof capture after the instrumentation logs identify the suspicious interval.

The profiler pass should answer one question: does the stack during the suspicious interval confirm the leading suspect from the logs?

It should not attempt to profile every historical variant or produce an exhaustive GTK analysis. If Sysprof is unavailable, inconclusive, or too noisy, the report must say so and explain what evidence was still available from logs.

## Interpretation Rules

Use these rules to turn the measurements into a recommendation:

- If `after_drop_to_idle_ms` is within roughly 20% of the largest schedule gap, the main loop is blocked before idle dispatch. The likely owner is GTK layout/realization or synchronous neighboring app work.
- If idle arrives under `50 ms` but `after_drop_to_first_frame_ms` is within roughly 20% of the stall, the likely owner is GTK frame/layout/render pipeline work.
- If heartbeat stalls align with parent, sidebar, search, inspector, or navigation logs, name that app path as the likely owner.
- If Sysprof shows GTK measure/allocation/paint/realization stacks during the suspicious interval and no neighboring app path aligns, name GTK layout/realization as the likely owner.
- If Sysprof shows mostly Relm4/GLib dispatch or scheduling paths without clear GTK layout work, name Relm4/main-loop scheduling as the likely owner.
- Only name transcript row content if first-frame row logs or profiler stacks contradict the #142/#143 evidence.
- If the signals do not distinguish the candidates, report `unknown` and recommend the smallest next measurement step instead of guessing.

## Report Requirements

Write the findings to `docs/reports/`.

The report should include:

- The exact build and run protocol.
- A table of the measured runs.
- A timeline for the largest stall.
- The leading owner and why it was selected.
- Any suspects that were ruled out.
- What should not be tried next based on the evidence.
- The recommended next implementation issue.

The report should explicitly choose one of these likely next paths:

- Tune batching against post-drop GTK cost.
- Simplify transcript row hierarchy.
- Revisit lazy row hydration with a full-app gating strategy.
- Explore row/window virtualization.
- Investigate parent/sidebar/search/inspector work if those probes are implicated.
- Close or re-scope #143 if its lazy-row premise does not address the full-app owner.

## Validation

Before considering the investigation branch complete:

- Run `cargo fmt --all -- --check`.
- Run `cargo test --all --no-fail-fast` if the probes touch code covered by tests or require test updates.
- Execute the manual reference-session protocol.
- Produce the report in `docs/reports/`.

`cargo clippy --all -- -D warnings` is useful before PR, but the key deliverable for this issue is the measured report rather than production instrumentation.

## Decision

Proceed with the targeted probe ladder approach. The design may temporarily instrument `SessionDetail` and nearby app paths, but it should remain narrowly focused on identifying the owner of the post-drop full-app freeze. The profiler pass should be short and confirmatory, not exploratory.
