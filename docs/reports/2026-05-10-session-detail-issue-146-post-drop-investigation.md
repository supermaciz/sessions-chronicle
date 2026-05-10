# Session Detail Issue 146 Post-Drop Investigation Report

## Protocol

- Reference session: `019dc51a-f0cd-79c1-ba79-45fedac889c2`
- Build profile: `release`
- Run mode: `native ~/.local/bin/sessions-chronicle`
- Display backend: `wayland`
- GNOME Shell version: `49.6`
- Search before open: none
- Probe targets: `AppMsg::SessionSelected`, `App::handle_session_selected`, `SessionDetailMsg::SetSession`, `NavigationView::push`, `SessionDetail::render_next_transcript_batch`, `App::handle_search_query_changed`, `App::handle_toggle_inspector`, `App::handle_inspector_visibility_changed`, `SidebarMsg::ProjectsLoaded`, `SidebarMsg::ProjectSelected`, `SidebarMsg::AiAssistantToggled`
- Runs captured: one cold run after cache drop attempt, two warm runs, one focused Sysprof capture with `sysprof-cli --gtk --gnome-shell --use-trace-fd`

## Runs

| run | cache_state | reproduced | max_schedule_gap_ms | max_post_drop_residual_ms | default_idle_ms | high_idle_ms | first_frame_ms | second_frame_ms | largest_frame_phase_us | notes |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| cold | cold | yes | 1327 | 1327 | 3246 | 0.0 | 9 | 1330 | 1325982 | `load_session=0 ms`; row build total `1 ms`; repeated `update_to_layout_us` spikes around `640k-1326k us` |
| warm-1 | warm | yes | 1330 | 1330 | 4001 | 664 | 12 | 1335 | 1322549 | `load_session=0 ms`; row build total `1 ms`; repeated `update_to_layout_us` spikes around `1.31M us` and `5k-8k us` |
| warm-2 | warm | yes | 1307 | 1307 | 3966 | 0.0 | 7 | 1310 | 1305530 | `load_session=0 ms`; row build total `3 ms`; repeated `update_to_layout_us` spikes around `1.30M us` and `21k-28k us` |

## Largest Stall Timeline

The cold run produced the clearest largest stall.

1. `AppMsg::SessionSelected`, `App::handle_session_selected`, `load_session`, `SessionDetailMsg::SetSession`, and `NavigationView::push` all happened back-to-back at `22:05:37.148-22:05:37.149`, with `load_session_ms=0`.
2. The first render batch completed its factory push immediately: `push_duration_ms=0`, `high-idle=0 ms`, `default-idle=1 ms`, and `first_frame_ms=7` for batch 1.
3. The first long stall appeared inside frame-clock work, not before it: frame `550` logged `update_to_layout_us=1280408`, `before_paint_to_after_paint_us=1281924`, and the heartbeat logged `heartbeat_gap_ms=1286`.
4. The next batch inherited the same wall-clock loss: batch 4 logged `schedule_gap_ms=1286`, `max_schedule_gap_ms=1286`, and batch 3's second-frame callback landed at `after_drop_to_second_frame_ms=1288`.
5. The page completed with `total_duration_ms=7272`, `max_schedule_gap_ms=1327`, `max_post_drop_residual_ms=1327`, and `total_row_build_duration_ms=1`.

The warm runs repeated the same pattern with nearly identical residual gaps: `1307-1330 ms` schedule gaps, `1310-1335 ms` second-frame delays, and `1305530-1322549 us` largest frame spans while Rust row-build work stayed at `1-3 ms`.

## Owner Decision

`GTK update/layout/paint`

The first matching interpretation rule is the one where the post-drop stall is owned by GTK frame processing because the lost time shows up inside frame phases rather than in Rust batch push, neighboring handlers, or pre-frame scheduling. All three runs reached `load_session=0 ms`, `total_row_build_duration_ms=1-3 ms`, and immediate or near-immediate first high-idle callbacks, but then spent `1305530-1325982 us` in frame-clock spans dominated by `update_to_layout_us`, with matching `1307-1330 ms` heartbeat and schedule gaps. Sysprof matched that measurement window with stacks dominated by GTK update/layout/paint and only secondary Pango presence.

## Suspects Ruled Out

- `neighboring app path`: the only neighboring logs during open were `AppMsg::SessionSelected`, `App::handle_session_selected`, `load_session`, `SessionDetailMsg::SetSession`, and `NavigationView::push`; `load_session` was `0 ms`, and no search or inspector handlers fired in the measured open path.
- `GLib/Relm4 main-loop scheduling`: the earliest high-idle callbacks landed at `0-664 ms`, but the decisive lost time was inside frame-phase measurements, especially `update_to_layout_us`, not before GTK started processing the frame.
- `compositor/swap`: the long spans sat before or during layout, while `paint_to_after_paint_us` stayed tiny and Sysprof did not point to swap/compositor-dominant stacks.
- Rust transcript row construction: `push_duration_ms` stayed `0`, `total_row_build_duration_ms` stayed `1-3 ms`, and the worst row build stayed `1 ms`, far below the `1307-1330 ms` residual stalls.

## What Not To Try Next

- Do not spend the next iteration on database loading, sidebar noise, search handlers, or inspector toggles; the logs already show those are not the owner of the stall window.
- Do not focus on Rust-side row-build micro-optimizations first; the measurement gap is overwhelmingly in GTK frame work after rows have already been pushed.
- Do not treat Pango as the sole owner from this capture; text shaping appears contributory, but the dominant bucket is still broader GTK update/layout/paint.

## Recommended Next Issue

`tune batching against post-drop GTK cost`

The next issue should reduce how much GTK layout/paint work is triggered per post-drop batch, because the current batching pattern creates repeated `~1.3 s` frame-layout stalls even though Rust-side work is negligible.

## Sysprof

`sysprof-cli` was available and produced `target/issue146.syscap`.

The focused capture lined up with the measured stall window and showed stacks dominated by GTK update/layout/paint activity, with some Pango work present but not leading. That matches the frame-clock probes, where the largest spans were in `update_to_layout_us` and not in `paint_to_after_paint_us` or neighboring application handlers.

## Validation

- Environment header recorded in `target/issue146-environment.txt`
- Native release binary installed at `~/.local/bin/sessions-chronicle`
- Log summary extracted into `target/issue146-summary.log`
- Placeholder scan completed with no unresolved authoring markers remaining.
- `cargo fmt --all -- --check` passed after report authoring.
- `cargo test --all --no-fail-fast` passed after report authoring.
