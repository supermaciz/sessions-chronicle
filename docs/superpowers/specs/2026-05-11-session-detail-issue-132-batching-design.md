# Session detail batching tune — issue #132 design

Date: 2026-05-11  
**Status:** Implemented [#151](https://github.com/supermaciz/sessions-chronicle/pull/151)  
Related: issue #132, escape hatch issue #134, investigation report `docs/reports/2026-05-10-session-detail-issue-146-post-drop-investigation.md`.

## Goal

Reduce the GTK update/layout/paint stall observed when opening `SessionDetail` (median `update_to_layout_us` per batch currently 1 305 530–1 325 982 µs on the frame following the guard drop) by:

1. Pushing fewer transcript rows per render batch.
2. Spacing successive batches on the GTK frame clock instead of a fixed timeout, so GTK has a better chance to process frame work between pushes.

Target: median `update_to_layout_us` per batch below ~100 ms on a real session, measured in release mode.

## Non-goals

- Transcript virtualization or migration to `gtk::ListView` / `TypedListView` (tracked in #134, used as escape hatch).
- Row-type weighting (deferred; can become a follow-up if measurements show markdown rows dominate).
- Transcript row redesign, parser changes, database changes.
- Refactoring the batching engine into a standalone module.
- Headless tests of frame-clock scheduling (no frame clock in test env).

## Design

### Code changes

All changes live in `src/ui/session_detail.rs`.

**Constants** (currently `src/ui/session_detail.rs:33-34`):

- `RENDER_BATCH_SIZE: usize = 1` (was 3). One-line comment explaining the link to transcript container Layout cost: every mounted row participates in each Layout pass, so per-batch row count scales the per-frame Layout cost.
- `RENDER_BATCH_DELAY_MS` is removed. Replaced by `RENDER_BATCH_WATCHDOG_MS: u64 = 100`, used by the fallback path described below.

**`schedule_transcript_render_batch`** (currently at `src/ui/session_detail.rs:1728`):

Today it uses `glib::timeout_add_local_once(16ms, …)`. After:

- Pose `Widget::add_tick_callback` on the transcript factory widget, currently the `gtk::Box` returned by `model.messages.widget()`. The callback fires on the next frame clock tick, sends `SessionDetailMsg::RenderNextTranscriptBatch { request_id }`, and returns `glib::ControlFlow::Break`.
- This is frame-clock throttling, not a strict post-layout barrier: GTK tick callbacks run before a frame and do not by themselves prove that the previous batch's Layout/Paint phase fully completed. The verification report decides whether this looser scheduling contract is sufficient.
- In parallel, arm a `glib::timeout_add_local_once(RENDER_BATCH_WATCHDOG_MS, …)` watchdog. If the tick callback has not fired before the watchdog (e.g. the widget is not realised, the window is minimised, or the frame clock is otherwise inactive), the watchdog sends the same message.
- Both paths share an `Rc<Cell<bool>>` "fired" flag. The first one to call `replace(true)` sends the message; the second sees `true` and no-ops. This guarantees at most one `RenderNextTranscriptBatch` per `schedule_transcript_render_batch` call.
- Best-effort cleanup: when the watchdog fires first, it calls `tick_id.remove()` to detach the unused frame callback. If cleanup cannot be expressed cleanly during implementation, accept the no-op (the tick callback returns `Break` on its next invocation anyway).

Sketch:

```rust
fn schedule_transcript_render_batch(&self, sender: &ComponentSender<Self>, request_id: u64) {
    let input_sender = sender.input_sender().clone();
    let fired = Rc::new(Cell::new(false));

    let fired_tick = fired.clone();
    let input_tick = input_sender.clone();
    let tick_id = self.transcript_render_widget.add_tick_callback(move |_, _| {
        if !fired_tick.replace(true) {
            let _ = input_tick.send(SessionDetailMsg::RenderNextTranscriptBatch { request_id });
        }
        glib::ControlFlow::Break
    });

    let fired_wd = fired.clone();
    glib::timeout_add_local_once(Duration::from_millis(RENDER_BATCH_WATCHDOG_MS), move || {
        if !fired_wd.replace(true) {
            tick_id.remove();
            let _ = input_sender.send(SessionDetailMsg::RenderNextTranscriptBatch { request_id });
        }
    });
}
```

**Transcript widget access from the model.** `schedule_transcript_render_batch` runs on `&self` of the component model and currently does not have a widget reference. Store a `gtk::Widget` clone (GObject ref-counted) in the model at init, e.g. `transcript_render_widget: gtk::Widget`, populated from the Relm4 factory widget via `model.messages.widget().clone().upcast::<gtk::Widget>()`.

**Unchanged.** `queue_transcript_items_for_render`, `render_next_transcript_batch`, `request_id` handling, and the metric collection (`max_schedule_gap`, `update_to_layout_us` probe) remain identical. The existing instrumentation already measures exactly what the AC asks about, so we do not need new probes.

### Tests

Inventory the tests under `src/ui/session_detail.rs` that assert on batch counts derived from `RENDER_BATCH_SIZE = 3`. Known starting points:

- `session_detail_records_render_batch_measurements`
- Tests that simulate multiple `RenderNextTranscriptBatch` ticks and check `batch_count`, `pushed_row_count`, `rendered_items`.

Approach:

1. Run `cargo test --all --no-fail-fast` after the const change and collect failures.
2. For each failure, prefer rewriting the assertion to derive from item count (`expected_batches = items.len()`) rather than hard-coding `1`. This keeps the test stable if `RENDER_BATCH_SIZE` is later tuned upward.
3. No new tests are required. Frame-clock scheduling is not exercised in headless tests; AC verification happens on a real release build.

### Verification protocol

1. Release build: `meson install -C builddir`.
2. Run `~/.local/bin/sessions-chronicle` against the user's real home session directory (no `--sessions-dir`).
3. Open the reference session normally used by the user; record its id in the verification report.
4. Collect probe logs for the open: `update_to_layout_us` per batch, `heartbeat_gap_ms`, `schedule_gap_ms`, `max_schedule_gap_ms`.
5. Repeat 3 runs. Close and restart the app between runs to start from a cold cache.

Per-run report:

- Median and p95 of `update_to_layout_us` across batches.
- `max_schedule_gap_ms` for the full open.
- Count of `heartbeat_gap_ms` samples above 200 ms.

Archived in a new report: `docs/reports/2026-05-XX-session-detail-issue-132-batching-verification.md`. Contents: baseline cited from `2026-05-10-session-detail-issue-146-post-drop-investigation.md`, post-patch configuration (size=1, tick + 100 ms watchdog), per-run results, aggregated medians, decision.

### Escape hatch

- If the median `update_to_layout_us` is at or below ~100 ms on all 3 runs: AC met, merge, close #132.
- If the median is above ~100 ms but clearly better than the 1 307–1 330 ms baseline: keep the patch (smaller is still better), document the result, update #134 with the data so virtualization work picks up from a measured floor.
- If batch size 1 is not measurably better than batch size 3 (or worse): revert the const change and the scheduling change, document the negative result, escalate to #134 immediately.

The decision is made by reading the verification report numbers; no automated threshold in CI.

## Definition of Done

- `cargo fmt --all -- --check` passes.
- `cargo clippy --all -- -D warnings` passes.
- `cargo test --all --no-fail-fast` passes.
- Verification report committed under `docs/reports/`.
- Decision (merge or escape hatch) recorded in the report and reflected in the issue trackers (#132 closed or #134 updated).

## Open implementation notes

- Verify `TickCallbackId::remove()` availability in the gtk4-rs version pinned by the project. If cleanup becomes awkward because of ownership, drop the cleanup call — the tick closure self-terminates with `Break`.
- Confirm during implementation that the widget exposed to the model is the transcript factory widget that holds the rendered rows, currently `model.messages.widget()` / `messages_box`, not a parent scroller. The tick clock attaches to whichever widget; for correctness any realised widget works, but using the rows' direct container keeps the contract obvious.
