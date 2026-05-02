# SessionDetail Issue #142 Investigation - Design

## Context

GitHub issue #142 asks why opening a large session in the full app exhibits multi-hundred-millisecond to ~1.4 s scheduling gaps between transcript render batches, even though prior reports have ruled out the obvious suspects.

Issues #127 and #140 established that, for the reference Codex session `019dc51a-f0cd-79c1-ba79-45fedac889c2`:

- first transcript page load: `2 ms`
- first-page preparation: `0 ms`
- total measured row construction: `4 ms`
- total factory push duration: `1 ms`
- first-page load to factory push completion: `8211 ms`
- max schedule gap: `1440 ms`
- max post-drop residual: `1439 ms`

The strong signal is `max_post_drop_residual_ms ≈ max_schedule_gap_ms`. The cost is therefore not in `guard.push_back()` itself; it is whatever GTK runs on the main loop after `drop(guard)` and before the next `glib::timeout_add_local_once(RENDER_BATCH_DELAY_MS)` callback is dispatched: widget realization, layout/measure, animation ticks, and any concurrent redraw work.

The adaptive batching design (`docs/superpowers/specs/2026-05-02-adaptive-session-detail-render-batches-design.md`) explicitly notes that `push_duration_ms` is a lower bound on real per-pass cost and that gating on `schedule_gap_ms` would be a deliberate next iteration. This investigation produces the evidence needed for that decision.

## Goal

Identify what consumes the GTK main loop between consecutive `RenderNextTranscriptBatch` invocations during a SessionDetail open in the full app, and produce a quantitative report that unblocks the decision on #132 (revive / rewrite / close).

## Non-Goals

- Designing or implementing the fix. The fix will be brainstormed in a separate session once the cause is confirmed.
- Adaptive batching (#132).
- Markdown render caching (#133).
- Transcript virtualization or windowing (#134).
- Off-main-thread transcript data preparation (#138).
- Database optimization.

## Approach

Three phases, executed in order. Phase A is the entry point; phase B quantifies inside the zone identified by phase A; phase C is a backup if phase A does not isolate a single suspect.

### Phase A — UI bisection

Disable suspects one at a time on a throwaway branch and measure `max_schedule_gap_ms` and `first_page_load_to_factory_push_ms` against a baseline run.

### Phase B — Targeted instrumentation

Once a zone is identified by phase A, add `tracing::debug!` spans inside that zone to quantify the contributors precisely. These spans are kept on `main`, following the same pattern as the instrumentation landed for #127 and #140.

### Phase C — Profiler (backup)

If none of the phase A variants meets the verdict threshold, switch to `sysprof-cli` to characterize a diffuse main-loop cost.

## Measurement Protocol

### Build and run

Local development build, not Flatpak:

```bash
meson install -C builddir
RUST_LOG=info,sessions_chronicle=debug \
  ~/.local/bin/sessions-chronicle > /tmp/sc-issue142-<variant>.log 2>&1
```

### Scenario

Frozen across all runs:

1. Cold launch the application.
2. Wait for `Background indexing complete` in the log.
3. Click the reference session `019dc51a-f0cd-79c1-ba79-45fedac889c2` exactly once.
4. Do not perform any search before the click.

### Metrics of record

Extracted from the `First transcript page factory push complete` log line:

- `max_schedule_gap_ms` — primary verdict signal.
- `first_page_load_to_factory_push_ms` — secondary signal.
- `max_post_drop_residual_ms` — sanity-check signal that the gap is still post-drop and not absorbed into push duration by the variant.

### Sampling and verdict

- **Sampling**: 3 runs per variant. Use the median for verdict comparison.
- **Verdict threshold**: a variant is the suspected cause when its median `max_schedule_gap_ms` is at least 70 % lower than the baseline median (i.e. ≤ ~430 ms for a baseline near 1440 ms).
- **Borderline cases**: if a variant lands between 50 % and 70 % reduction, decide jointly whether to declare it the cause, continue bisecting, or escalate to phase C. Do not auto-promote borderline reductions.

## Phase A — Bisection Variants

Run the variants in the fixed order below. Stop as soon as one variant meets the verdict threshold and proceed to phase B targeted on that zone.

### Baseline

No code changes. 3 runs. Establishes the median `max_schedule_gap_ms` and `first_page_load_to_factory_push_ms` against which every variant is compared.

### A1 — NavigationView push animation

**Hypothesis**: the libadwaita `NavigationView::push` slide animation runs concurrently with the first batches, generating one frame every ~16 ms on content that is still being populated. Layout/snapshot of a growing transcript repeats on each animation tick, which can plausibly delay the next `timeout_add_local_once` dispatch.

**Neutralization**: disable transitions on the navigation view for the duration of the run. Either:

- call the appropriate "no animation" setter on `nav_view` before `nav_view.push(&self.detail_page)` in `handle_session_selected`, or
- delay emitting `SetSession` until after the navigation transition has had time to complete, so no first-page batch can be queued during the slide.

Either approach is acceptable for the variant; pick whichever is less invasive in the current code.

### A2 — Transcript row realization

**Hypothesis**: the dominant cost is widget realization (markdown rendering, syntax highlighting, `GtkTextView` measure/allocate) that happens on idle after `drop(guard)` and before the next batch callback runs. This is the strongest candidate given `post_drop_residual ≈ schedule_gap`.

**Neutralization**: temporarily replace the `transcript_row` widget tree with a minimal variant: a single `GtkLabel` displaying the raw item text, with no markdown rendering, no syntax highlighting, no tool renderer, no expandable sections. Keep `TranscriptRow` factory wiring intact so batch counts and request invalidation still match production.

### A3 — Markdown and highlight specifically

Run only if A2 meets the threshold and we want to discriminate between `TextView` cost and Markdown/highlight cost.

**Hypothesis**: some meaningful share of the A2 cost is in `markdown::*` rendering and `highlight::*` calls, not in `GtkTextView` realization, layout, measure, or snapshot work. This is a discriminator after A2, not the primary suspect: existing #140 row-build instrumentation already includes markdown/highlight execution and measured only a few milliseconds of synchronous row construction.

**Neutralization**: keep the production `transcript_row` widget tree, but short-circuit `markdown::render_*` and `highlight::*` to return raw text. This isolates the markdown/highlight contribution from the rest of the row.

### Stop conditions

- A1, A2, or A3 meets the verdict threshold → declare cause, exit phase A, move to phase B.
- None of A1, A2, A3 meets the threshold → cause is likely diffuse. Move to phase C.

## Phase B — Targeted Instrumentation

After phase A names a zone, add `tracing::debug!` spans inside that zone to quantify the contributors precisely. Concrete examples by zone:

- **A1 coupable**: spans around `NavigationView` push and per-frame measure/allocate during the transition.
- **A2 coupable**: spans inside `TranscriptRow::init` (or equivalent) covering widget construction, plus measure and allocate timings per realized row.
- **A3 coupable**: spans around `markdown::render_*` and `highlight::*` recording elapsed time and input size (not content).

### Logging discipline

- Spans live at `tracing::debug!` level so they are off by default in release builds.
- No transcript content, tool call payload, command output, or Markdown body is logged. Sizes and counts only.
- Field names follow the same conventions as #127 and #140 instrumentation so the existing reports remain comparable.

### Persistence

The phase B spans are committed to `main` in a small dedicated commit at the end of the investigation. The patches from phase A are **not** committed to `main`.

## Phase C — Profiler Backup

Triggered only if phase A produces no qualifying variant:

```bash
sysprof-cli --gtk -o /tmp/sc-issue142.syscap -- ~/.local/bin/sessions-chronicle
```

Reading focus: what holds the main loop between successive `RenderNextTranscriptBatch` dispatches? Likely candidates if the investigation reaches this stage are diffuse GObject allocation, global idle handlers, or cascading `queue_resize` chains.

If phase C identifies a cause, the report still concludes with a single named cause and recommendation for #132. If it does not, the report documents the negative result so future work knows what has already been excluded.

## Code Hygiene

- Phase A variants are produced on a throwaway branch named `investigate/issue-142` using a stash/revert workflow between variants; no commits from these variants reach `main`.
- Phase B instrumentation lands on `main` as a small focused commit, separate from any future fix.
- The reference report and any follow-up reports live under `docs/reports/`.

## Deliverables

- **Report**: `docs/reports/2026-05-02-session-detail-issue-142-investigation-report.md`. Includes: protocol used, baseline median values, table of `(variant, run 1, run 2, run 3, median, % vs baseline)` for `max_schedule_gap_ms` and `first_page_load_to_factory_push_ms`, verdict, and a recommendation for #132 (revive, rewrite, or close).
- **Phase B commit (if reached)**: a small commit on `main` adding `tracing::debug!` spans in the identified zone, following #127/#140 logging conventions.
- **No fix.** The fix is out of scope of this spec and will be brainstormed in a follow-up session that takes this report as input.

## Acceptance Criteria

- A single named cause is identified, or the report explicitly states the cause is diffuse and documents the phase C output supporting that conclusion.
- For every variant attempted, the report records baseline + 3 runs + median + percentage delta versus baseline, for both `max_schedule_gap_ms` and `first_page_load_to_factory_push_ms`.
- The report ends with an actionable recommendation on #132 (revive, rewrite, or close).
- No fix is delivered in this scope. Phase A patches are not merged to `main`.
- If phase B instrumentation is kept, no transcript content or sensitive payload is logged.
- Existing SessionDetail rendering, pagination, grouped tool calls, and in-session search still work after any phase B commit.

## Follow-Up

Once the report is published, open a separate brainstorm to design the fix using the report as input. The shape of that fix depends on which suspect was confirmed:

- A1 confirmed → likely candidates are gating animation during open, or pushing the detail page before queueing batches.
- A2 confirmed → candidates include lazy realization of expensive row content, downgrading offscreen rows, or schedule-gap-driven adaptive batching (revives #132 with the right control signal).
- A3 confirmed → candidates include Markdown render caching (#133) or rendering Markdown lazily on visibility.
- Phase C cause → fix design will depend on what phase C reveals.

The follow-up brainstorm will not pre-commit to any of these directions; they are listed here only to make explicit that the fix space is contingent on the report.
