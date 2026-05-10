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

### Leading Hypothesis

GTK4 layout/realization cost (measure, size-allocate, realize, Pango text shaping) of the rich transcript row widgets, executed after `FactoryVecDeque` inserts but before the next frame is presented.

Rationale:

- #142 reduced median `max_schedule_gap_ms` from `1346 ms` to `21 ms` by replacing rich rows with minimal labels. The Rust factory push cost is unchanged between the two cases, so the saved time is in GTK work that depends on widget complexity.
- The `max_post_drop_residual_ms ~= max_schedule_gap_ms` signal places that work after `drop(guard)`, which is consistent with GTK's deferred measure/allocate/realize phases driven by the frame clock.
- Pango shaping for long Markdown transcripts and code blocks is the most plausible specific cost driver under that umbrella.

### Alternative Owners to Rule In or Out

The probe ladder must produce evidence that either confirms the leading hypothesis or falsifies it in favor of one of:

- Relm4 or GLib main-loop scheduling behavior (pre-idle stall not attributable to GTK phases).
- Parent view, navigation, sidebar, search, or inspector work triggered by opening `SessionDetail`.
- Transcript row content cost only if first-frame timings or profiler stacks contradict the #142/#143 evidence.
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

- One default-idle callback with `glib::idle_add_local_once`. Document explicitly that this measures "idle dispatched at default-idle priority", not "main loop fully unblocked", so a long `after_drop_to_idle_ms` may indicate either a blocked loop or higher-priority sources running ahead of idle.
- One parallel high-idle callback with `glib::idle_add_local_full(glib::Priority::HIGH_IDLE, move || { ...; glib::ControlFlow::Break })` to disambiguate when the default-idle gap is suspicious.
- One `WidgetExt::add_tick_callback` callback on the transcript factory widget or the transcript `ScrolledWindow`, whichever is already rooted and easiest to access without broad widget plumbing.
- A second tick callback whenever a first tick fires, to measure whether the stall straddles one frame (cost concentrated in the first compose/layout/paint cycle) or spans multiple frames (multi-frame deferred work or repeated invalidation). Phrase results as "stall is within first frame" vs "stall spans N frames", not as "first frame cannot explain layout cost".

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

### GTK Frame Clock Phase Timings

Before the Sysprof pass, capture per-phase frame timings via `GdkFrameClock` signals so the logs can distinguish GTK update/layout/paint cost from main-loop scheduling cost without relying on profiler availability.

During the measured window, connect temporary handlers to the rooted widget's frame clock:

- `update`
- `layout`
- `paint`
- `after-paint`

Each handler should record a monotonic wall-clock timestamp plus `frame_clock.frame_counter()`. Log per-frame phase deltas such as:

- `update_to_layout_us`
- `layout_to_paint_us`
- `paint_to_after_paint_us`
- `update_to_after_paint_us`

Correlate each frame's `frame_counter` with the `request_id` and `render_batch_index` of the most recent batch. `GdkFrameTimings` may still be logged as context (`presentation_time_us`, `predicted_presentation_time_us`, `refresh_interval_us`, `is_complete`), but do not interpret `frame_time()` as a duration: it is a frame-clock timestamp, not "GTK work time".

This gives a cheap way to attribute the post-drop residual to one of:

- Long `update_to_after_paint_us`, especially with a large `layout_to_paint_us` or `update_to_layout_us`, which points to GTK update/layout/paint work.
- Tick callbacks delayed but no frame-clock phase span large enough to explain the stall, which points back to main-loop scheduling, higher-priority sources, or neighboring app work.
- Missing or skipped frames (loop blocked, no frame served).

If frame-clock signal data is unavailable on the active backend or the widget is not rooted yet, log that explicitly and fall back on tick-callback wall-clock deltas alone.

### Neighboring Owner Probes

Add short, temporary logs around app work that may run during the same interval but is not row construction. Each probe must name a concrete Relm4 input/output message or component method — vague targets ("sidebar updates") are not actionable and should be removed if a specific signal cannot be cited.

Concrete probes to add (drop any whose target signal does not exist):

- `App` input that handles "session selected" and the navigation push that activates `SessionDetail`. Log the message variant name and timestamp.
- `Sidebar` and project/filter component inputs emitted in response to the same selection (typically a selection-changed broadcast or a project-filter update). Log the message variant and the receiving component name.
- `Search` state update path triggered by a session change, if such a path exists in the current `app.rs` wiring; otherwise omit.
- `Inspector` (or equivalent right-pane) update path triggered by a session change, if wired; otherwise omit.
- Transcript `ScrolledWindow` `vadjustment.value()` and `vadjustment.upper()` sampled on tick callbacks, to detect adjustment-driven invalidations during the stall.

Before the instrumentation pass starts, list the exact Relm4 message variants and method names that will be probed in the report's protocol section. If only one or two of the candidates above resolve to real code paths, the others are dropped — this is an explicit pruning step, not a TODO.

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

Build and runtime conditions to record for every run (these change Pango/font cache, GPU shader cache, SQLite page cache, and therefore the stall):

- Build profile: `release` (mandatory for measurement; debug builds are not representative).
- Run mode: native `meson install -C builddir && ~/.local/bin/sessions-chronicle` for the primary measurement path. Flatpak (`flatpak-builder --run …`) only as a parity check, since sandbox/font-cache state differs.
- Cold vs warm: a "cold" run is the first launch after `pkill sessions-chronicle && sync && echo 3 | sudo tee /proc/sys/vm/drop_caches` (or equivalent); a "warm" run is the next launch within the same desktop session, with caches populated.
- Display backend: record whether the session is Wayland or X11 (`echo $XDG_SESSION_TYPE`).
- Compositor: record GNOME Shell version (`gnome-shell --version`).

Run protocol:

1. Build the app in release with the temporary probes.
2. Launch the app with `RUST_LOG=info,sessions_chronicle=debug` and redirect logs to a file. Record build profile, run mode, cold/warm state, and display backend in the log header.
3. Wait for background indexing to complete.
4. Open the reference session exactly once.
5. Do not perform session detail search before opening it.
6. Wait for the detail view to settle, then close the app.
7. Run at least one cold run and two warm runs. If the first cold run does not reproduce the stall, do warm runs anyway and report the cold-vs-warm asymmetry rather than forcing more cold runs.

Extract these measurements from the logs:

- `max_schedule_gap_ms`
- `max_post_drop_residual_ms`
- `after_drop_to_idle_ms` at default-idle priority
- `after_drop_to_idle_ms` at high-idle priority (when used)
- `after_drop_to_first_frame_ms`
- `after_drop_to_second_frame_ms`
- frame-clock phase deltas (`update_to_layout_us`, `layout_to_paint_us`, `paint_to_after_paint_us`, `update_to_after_paint_us`) for frames within the stall window
- optional `GdkFrameTimings` context (`presentation_time_us`, `predicted_presentation_time_us`, `refresh_interval_us`, `is_complete`) when available
- heartbeat stall timeline
- batch context for the largest gap
- neighboring owner events near the largest gap
- scroll adjustment values around the largest gap, when available

If neither the cold nor the warm runs reproduce the stall, stop and document the reproduction failure instead of forcing a conclusion.

### Sysprof Pass

Run one focused Sysprof capture after the instrumentation logs identify the suspicious interval.

The profiler pass should answer one question: does the stack during the suspicious interval confirm the leading suspect from the logs?

Capture conditions:

- Run the native release binary (`~/.local/bin/sessions-chronicle`) outside the Flatpak sandbox. Flatpak/sandboxed runs frequently produce truncated GTK/Pango stacks because debug symbols for runtime libraries are not exported into the sandbox, which makes the capture useless for distinguishing measure/allocate/realize from text shaping.
- Ensure debug symbols are available for `gtk4`, `pango`, and `cairo` (install distro `-debuginfo` packages if needed).
- Use the GNOME Sysprof default sampling rate; the suspicious interval is on the order of `1 s`, so default-rate sampling is sufficient.

It should not attempt to profile every historical variant or produce an exhaustive GTK analysis. If Sysprof is unavailable, inconclusive, or too noisy (truncated stacks, missing symbols), the report must say so and explain what evidence was still available from logs and frame timings.

## Interpretation Rules

Use these rules to turn the measurements into a recommendation. Apply them in order; the first matching rule wins.

- If both default-idle and high-idle `after_drop_to_idle_ms` are within roughly 20% of the largest schedule gap, the main loop is blocked before any idle dispatch. The likely owner is GTK layout/realization, Pango shaping, or synchronous neighboring app work.
- If high-idle arrives quickly but default-idle is delayed by close to the full stall, higher-priority sources (frame clock, redraw) are saturating the loop ahead of default idle. The likely owner is GTK frame/layout/render pipeline work; this is consistent with the leading hypothesis.
- If frame-clock phase deltas, especially `update_to_after_paint_us`, account for most of the stall, name GTK update/layout/paint as the likely owner. If `GdkFrameTimings` presentation data is complete and presentation timing dominates while GTK phase spans are small, name compositor/swap as the likely owner (out of scope for Sessions Chronicle code).
- If heartbeat stalls align with concrete neighboring-owner probes (named Relm4 message variants), name that app path as the likely owner.
- If Sysprof shows Pango/`gtk_widget_measure`/`gtk_widget_size_allocate`/`gtk_widget_realize` stacks during the suspicious interval and no neighboring app path aligns, name GTK layout/realization (with Pango shaping called out specifically when present) as the likely owner. This confirms the leading hypothesis.
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
