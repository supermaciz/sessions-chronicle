# Heatmap Width Limit Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Limit the analytics heatmap to a rolling 6-month window ending at the last activity day and show the visible date range in the Activity section.

**Architecture:** Keep the truncation logic in `src/database/analytics.rs` where heatmap normalization already happens, extend `HeatmapData` with explicit display-range metadata, and keep the widget/UI layers focused on presentation. Update tests first so the new semantics are locked in before code changes.

**Tech Stack:** Rust 2024, chrono, rusqlite, GTK4/libadwaita, Relm4, cargo test

---

### Task 1: Add failing analytics tests for the bounded heatmap window

**Files:**
- Modify: `tests/analytics_queries.rs`
- Test: `tests/analytics_queries.rs`

**Step 1: Write the failing test**

Add a test that inserts activity spanning more than 6 months, loads analytics, and asserts that:
- the first visible heatmap day is within the last 6 months window after week alignment
- the last visible heatmap day matches the aligned week containing the last activity day
- older activity outside the bounded window is not represented in `heatmap.weeks`

Add a second test that inserts activity spanning less than 6 months and asserts the earliest real activity day remains included.

**Step 2: Run test to verify it fails**

Run: `cargo test --test analytics_queries heatmap_ -- --nocapture`
Expected: FAIL because `build_heatmap()` still includes the full historical range and `HeatmapData` has no explicit display-range metadata.

**Step 3: Write minimal implementation placeholders if needed**

If compilation blocks the new assertions, add the minimal temporary model fields in `src/models/analytics.rs` first, keeping the analytics logic unchanged so the behavior assertions still fail.

**Step 4: Run test to verify behavior still fails for the right reason**

Run: `cargo test --test analytics_queries heatmap_ -- --nocapture`
Expected: FAIL on range assertions, not on unrelated compile issues.

**Step 5: Commit**

```bash
git add tests/analytics_queries.rs src/models/analytics.rs
git commit -m "test: define bounded heatmap expectations"
```

### Task 2: Extend the analytics model with visible range metadata

**Files:**
- Modify: `src/models/analytics.rs`
- Test: `tests/analytics_queries.rs`

**Step 1: Write the failing test**

Add assertions that `analytics.heatmap.display_start_day` and `analytics.heatmap.display_end_day` are populated for non-empty heatmaps and correspond to the intended visible activity range.

**Step 2: Run test to verify it fails**

Run: `cargo test --test analytics_queries heatmap_ -- --nocapture`
Expected: FAIL because `HeatmapData` does not expose these fields yet.

**Step 3: Write minimal implementation**

Update `src/models/analytics.rs`:
- add `display_start_day: Option<String>`
- add `display_end_day: Option<String>`
- keep `Default`, `Clone`, `PartialEq`, and `Eq` derives intact

**Step 4: Run test to verify it compiles and still fails only on unimplemented population**

Run: `cargo test --test analytics_queries heatmap_ -- --nocapture`
Expected: FAIL because the fields are still empty until `build_heatmap()` populates them.

**Step 5: Commit**

```bash
git add src/models/analytics.rs tests/analytics_queries.rs
git commit -m "refactor: add heatmap display range metadata"
```

### Task 3: Implement the 6-month bounded heatmap window

**Files:**
- Modify: `src/database/analytics.rs`
- Test: `tests/analytics_queries.rs`

**Step 1: Write the failing test**

Refine the tests so they assert the exact expected visible range for a representative dataset spanning more than 6 months, including Monday/Sunday alignment.

**Step 2: Run test to verify it fails**

Run: `cargo test --test analytics_queries heatmap_ -- --nocapture`
Expected: FAIL because `build_heatmap()` still starts from the first historical activity day.

**Step 3: Write minimal implementation**

Update `src/database/analytics.rs` to:
- compute `window_end` from the last activity day
- subtract 6 calendar months using chrono date arithmetic
- clamp the visible start to the first real activity day when history is shorter
- align the clamped range to Monday/Sunday
- zero-fill only the bounded aligned range
- populate `HeatmapData.display_start_day` and `HeatmapData.display_end_day`

Keep `max_sessions_in_a_day` derived from the bounded normalized days actually displayed.

**Step 4: Run test to verify it passes**

Run: `cargo test --test analytics_queries heatmap_ -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/database/analytics.rs tests/analytics_queries.rs src/models/analytics.rs
git commit -m "feat: limit heatmap to recent activity window"
```

### Task 4: Update the heatmap summary to match bounded-range semantics

**Files:**
- Modify: `src/ui/analytics_heatmap.rs`
- Test: `src/ui/analytics_heatmap.rs`

**Step 1: Write the failing test**

Add or update unit tests around `summarize_heatmap()` so they assert the summary text reflects the bounded visible range represented by `display_start_day` and `display_end_day` when present.

**Step 2: Run test to verify it fails**

Run: `cargo test ui::analytics_heatmap::tests::summarize_heatmap -- --nocapture`
Expected: FAIL because the summary currently infers range only from rendered cells.

**Step 3: Write minimal implementation**

Update `src/ui/analytics_heatmap.rs` so `summarize_heatmap()` uses the explicit display-range metadata where appropriate and continues to produce concise accessible text.

**Step 4: Run test to verify it passes**

Run: `cargo test ui::analytics_heatmap::tests::summarize_heatmap -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/ui/analytics_heatmap.rs
git commit -m "fix: align heatmap summary with bounded range"
```

### Task 5: Show the visible date range in the Activity section

**Files:**
- Modify: `src/ui/analytics_view.rs`
- Test: `src/ui/analytics_heatmap.rs`

**Step 1: Write the failing test**

If there is an existing view-model or formatting helper, add a focused unit test for a new date-range formatter that turns the explicit heatmap bounds into a label such as `Oct 2025 - Mar 2026`.

If no formatter exists yet, introduce one in a testable location first and write tests for:
- same-month formatting if both bounds fall in one month
- cross-month formatting
- empty-range fallback behavior

**Step 2: Run test to verify it fails**

Run: `cargo test heatmap_range -- --nocapture`
Expected: FAIL because no formatter or label wiring exists yet.

**Step 3: Write minimal implementation**

Update `src/ui/analytics_view.rs` to:
- add a small label near the `Activity` section
- populate it from `data.heatmap.display_start_day` and `data.heatmap.display_end_day`
- keep the label hidden or empty when no range is available

Prefer a small helper for formatting so the logic remains unit-testable.

**Step 4: Run test to verify it passes**

Run: `cargo test heatmap_range -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/ui/analytics_view.rs src/ui/analytics_heatmap.rs
git commit -m "feat: show heatmap visible date range"
```

### Task 6: Update integration expectations for bounded heatmap data

**Files:**
- Modify: `tests/analytics_integration.rs`
- Test: `tests/analytics_integration.rs`

**Step 1: Write the failing test**

Update the fixture integration assertions so they no longer require `heatmap_total == overview.total_sessions` for arbitrary histories. Replace that with assertions that fit the bounded model:
- non-empty `heatmap.weeks`
- positive `max_sessions_in_a_day`
- populated display-range metadata
- `heatmap_total <= overview.total_sessions`

**Step 2: Run test to verify it fails**

Run: `cargo test --test analytics_integration -- --nocapture`
Expected: FAIL until the assertions match the new semantics or until the bounded window is implemented.

**Step 3: Write minimal implementation**

Adjust `tests/analytics_integration.rs` to the new semantics without loosening unrelated analytics guarantees.

**Step 4: Run test to verify it passes**

Run: `cargo test --test analytics_integration -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add tests/analytics_integration.rs
git commit -m "test: update bounded heatmap integration checks"
```

### Task 7: Run full verification

**Files:**
- Modify: none
- Test: workspace verification

**Step 1: Run formatting check**

Run: `cargo fmt --all -- --check`
Expected: PASS

**Step 2: Run clippy**

Run: `cargo clippy --all -- -D warnings`
Expected: PASS

**Step 3: Run full test suite**

Run: `cargo test --all --no-fail-fast`
Expected: PASS

**Step 4: Commit verification-safe follow-ups if needed**

If verification required any final non-behavioral cleanup, commit it with an appropriate message before handing off.

**Step 5: Handoff**

Record the exact commands run and their outcomes for the PR or final summary.
