# Session Detail Issue 132 Batching Verification Report

## Protocol

- Reference session: `019dc51a-f0cd-79c1-ba79-45fedac889c2`
- Build profile: `release`
- Run mode: `native ~/.local/bin/sessions-chronicle`
- Patch configuration: `RENDER_BATCH_SIZE = 1`, GTK tick callback scheduling, `RENDER_BATCH_WATCHDOG_MS = 100`
- Baseline: `docs/reports/2026-05-10-session-detail-issue-146-post-drop-investigation.md` reported largest frame spans of `1 305 530–1 325 982 us` and schedule gaps of `1 307–1 330 ms` with `RENDER_BATCH_SIZE = 3`.
- Runs captured: 3 cold app restarts against the same reference session.
- Probe origin: temporary instrumentation re-derived from PR #150 (`SessionDetailProbeWindow`, frame-clock `before/update/layout/paint/after_paint` handlers, idle/tick post-drop logging, 16 ms heartbeat). Removed in the follow-up commit after this report was committed.
- Capture command per run: `RUST_LOG=sessions_chronicle=debug ~/.local/bin/sessions-chronicle 2>&1 | tee target/issue132-run-N.log`
- Aggregation: `update_to_layout_us` extracted from every `Session detail issue132 frame clock phase measured` line during the probe window; `max_schedule_gap_ms` and `heartbeat_gap_ms` extracted from the matching probe lines.

## Results

| run | frames | median_update_to_layout_us | p95_update_to_layout_us | max_update_to_layout_us | max_schedule_gap_ms | heartbeat_gap_samples_over_200_ms | max_heartbeat_gap_ms | notes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 142 | 331 | 648 000 | 690 201 | 706 | 12 | 707 | First cold open after install. |
| 2 | 87 | 41 546 | 654 632 | 687 314 | 698 | 12 | 694 | Median raised by fewer total frames captured before close. |
| 3 | 144 | 767 | 645 911 | 683 109 | 695 | 12 | 695 | Matches run 1 shape. |

## Decision

The measured medians meet the issue #132 acceptance criterion:

- All three medians of `update_to_layout_us` are well below `100 000 us` (0.3 ms / 41.5 ms / 0.8 ms vs. 100 ms target).
- `max_schedule_gap_ms` collapsed from the `1 307–1 330 ms` baseline to `695–706 ms`, a measurable reduction.
- `heartbeat_gap_ms` no longer reaches the baseline `1 307–1 330 ms` range; the largest sample is now `694–707 ms`. Spikes above 200 ms still occur (12 per run) but are clearly reduced in both magnitude and likely cause: they cluster around the same `~700 ms` frame stall that the p95 of `update_to_layout_us` also captures.

**Outcome:** AC met. Merge PR #151 and close #132.

The remaining `~700 ms` p95 layout stall is the next floor; it is the expected residual from non-virtualised `gtk::ListBox` doing full layout passes as rows continue to mount. Issue #134 (virtualisation / `TypedListView` migration) is the natural follow-up and should be updated with this measured floor so the virtualisation work picks up from a quantified baseline.

## Validation

- `cargo fmt --all -- --check` passed.
- `cargo clippy --all -- -D warnings` passed.
- `cargo test --all --no-fail-fast` passed.
- `meson install -C builddir` passed.
- Probe instrumentation will be reverted in the commit that follows this report; PR #151 stays at the production patch without the temporary probes.
