# Session Detail Issue 132 Batching Verification Report

## Protocol

- Reference session: `019dc51a-f0cd-79c1-ba79-45fedac889c2`
- Build profile: `release`
- Run mode: `native ~/.local/bin/sessions-chronicle`
- Patch configuration: `RENDER_BATCH_SIZE = 1`, GTK tick callback scheduling, `RENDER_BATCH_WATCHDOG_MS = 100`
- Baseline: `docs/reports/2026-05-10-session-detail-issue-146-post-drop-investigation.md` reported largest frame spans of `1 305 530–1 325 982 us` and schedule gaps of `1 307–1 330 ms` with `RENDER_BATCH_SIZE = 3`.
- Runs captured: 3 cold app restarts against the same reference session.
- Probe origin: corrected temporary rerun probe added locally for this measurement pass. Unlike the superseded report revision, it logs exactly one frame-clock phase sample per render batch: the first frame after each post-drop batch.
- Capture command per run: `SC_ISSUE132_PROBE=1 SC_OPEN_SESSION_ID=019dc51a-f0cd-79c1-ba79-45fedac889c2 RUST_LOG=sessions_chronicle=debug ~/.local/bin/sessions-chronicle > target/issue132-rerun-fixed-N.log 2>&1`
- Aggregation: `update_to_layout_us` extracted only from `Session detail issue132 next frame phase measured` lines. Each such line is one batch sample, so the median and p95 are now truly `per batch`, matching the issue #132 design AC.
- Secondary-signal scope: this corrected rerun targets the AC metric (`update_to_layout_us` per batch) and `max_schedule_gap_ms`. The earlier `heartbeat_gap_ms` side signal was not re-collected in this rerun.

## Results

| run | batches | median_update_to_layout_us | p95_update_to_layout_us | max_update_to_layout_us | max_schedule_gap_ms | notes |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 33 | 7 734 | 709 025 | 716 665 | 730 | Corrected rerun using one next-frame sample per batch. |
| 2 | 33 | 7 943 | 723 520 | 726 530 | 741 | Same shape as run 1, slightly higher p95 and max gap. |
| 3 | 33 | 7 738 | 721 154 | 742 687 | 754 | Same median band; largest max gap of the three reruns. |

## Decision

This corrected rerun supersedes the earlier report revision that aggregated every frame inside the probe window. With one `update_to_layout_us` sample per render batch, the measured medians meet the issue #132 acceptance criterion:

- All three medians of `update_to_layout_us` are well below `100 000 us` (7.7 ms / 7.9 ms / 7.7 ms vs. 100 ms target).
- `max_schedule_gap_ms` still improves materially versus the `1 307–1 330 ms` baseline, landing at `730–754 ms` across the corrected reruns.
- The residual stall is still visible in the tail: p95 `update_to_layout_us` remains `709 025–723 520 us`, and max per-batch samples remain `716 665–742 687 us`.

**Outcome:** AC met. Merge PR #151 and close #132.

The remaining `~0.71–0.74 s` p95 / max next-frame layout stall is the next floor. It is still consistent with non-virtualised `gtk::ListBox` doing expensive full layout passes as rows continue to mount. Issue #134 (virtualisation / `TypedListView` migration) remains the natural follow-up and should use this corrected floor, not the earlier all-frames aggregation, as its baseline.

## Validation

- `cargo fmt --all -- --check` passed.
- `cargo check --bin sessions-chronicle` passed.
- `meson install -C builddir` passed.
- Corrected rerun logs captured in `target/issue132-rerun-fixed-{1,2,3}.log`.
