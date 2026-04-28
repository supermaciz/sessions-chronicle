# Session Detail Render Measurements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add reproducible, targeted measurements for issue #127 so the next fix is based on evidence rather than the abandoned `perf/session-detail` branch.

**Architecture:** Keep `FactoryVecDeque<TranscriptRow>` and current paging behavior. Add a small internal render metrics accumulator owned by `SessionDetail`, update it from the existing render queue, and expose it only to unit tests through the existing `#[cfg(test)]` module. The first implementation measures batch drain shape and row-kind weight; it does not optimize rendering.

**Tech Stack:** Rust 2024, GTK4/Relm4, existing `#[gtk::test]` coverage in `src/ui/session_detail.rs`, `tracing` logs.

---

## File Structure

- Modify `src/ui/session_detail.rs`: add render-measurement fields, update render queue/batch code, and add tests in the existing test module.
- No new runtime modules. The change is instrumentation-only and should not alter user-visible behavior.
- No fixture files. The tests should use synthetic temp database rows, matching the current `session_detail_loads_transcript_pages_incrementally` pattern.

---

### Task 1: Add a failing metrics-surface test

**Files:**
- Modify: `src/ui/session_detail.rs`
- Test: `src/ui/session_detail.rs`

- [ ] **Step 1: Write the failing test**

Add this test near `session_detail_loads_transcript_pages_incrementally`:

```rust
#[gtk::test]
fn session_detail_records_render_batch_measurements() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    seed_message_transcript(temp_db.path(), "test-session-123", INITIAL_PAGE_SIZE + 5);

    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });

    pump_main_context(|| {
        let parts = controller.state().get();
        parts.model.pending_render_batch.is_none()
            && parts.model.messages.len() == INITIAL_PAGE_SIZE
    });

    let parts = controller.state().get();
    let metrics = parts
        .model
        .last_render_metrics
        .as_ref()
        .expect("first page should record render metrics");
    assert_eq!(metrics.offset, 0);
    assert_eq!(metrics.source_row_count, INITIAL_PAGE_SIZE);
    assert_eq!(metrics.display_item_count, INITIAL_PAGE_SIZE);
    assert_eq!(metrics.batch_count, INITIAL_PAGE_SIZE.div_ceil(RENDER_BATCH_SIZE));
    assert_eq!(metrics.message_count, INITIAL_PAGE_SIZE);
    assert_eq!(metrics.tool_call_count, 0);
    assert_eq!(metrics.tool_burst_count, 0);
    assert_eq!(metrics.subagent_count, 0);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib session_detail::tests::session_detail_records_render_batch_measurements`

Expected: FAIL to compile because `last_render_metrics` does not exist.

- [ ] **Step 3: Add minimal metrics types and storage**

Add this near `PendingRenderBatch`:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RenderRowKindCounts {
    message_count: usize,
    tool_call_count: usize,
    tool_burst_count: usize,
    subagent_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderMetrics {
    offset: usize,
    source_row_count: usize,
    display_item_count: usize,
    batch_count: usize,
    message_count: usize,
    tool_call_count: usize,
    tool_burst_count: usize,
    subagent_count: usize,
}
```

Add fields:

```rust
last_render_metrics: Option<RenderMetrics>,
```

on `SessionDetail`, and:

```rust
row_kind_counts: RenderRowKindCounts,
```

on `PendingRenderBatch`.

- [ ] **Step 4: Populate counts when queueing items**

In `queue_transcript_items_for_render`, count `TranscriptItemInit` variants before converting `items` into `VecDeque`:

```rust
let row_kind_counts = Self::count_render_item_kinds(&items);
```

Add helper:

```rust
fn count_render_item_kinds(items: &[TranscriptItemInit]) -> RenderRowKindCounts {
    let mut counts = RenderRowKindCounts::default();
    for item in items {
        match item {
            TranscriptItemInit::Message(_) => counts.message_count += 1,
            TranscriptItemInit::ToolCall(_) => counts.tool_call_count += 1,
            TranscriptItemInit::ToolBurst(_) => counts.tool_burst_count += 1,
            TranscriptItemInit::Subagent(_) => counts.subagent_count += 1,
        }
    }
    counts
}
```

- [ ] **Step 5: Record metrics when a render page finishes**

Before clearing `pending_render_batch`, assign:

```rust
self.last_render_metrics = Some(RenderMetrics {
    offset,
    source_row_count,
    display_item_count: total_items,
    batch_count,
    message_count: row_kind_counts.message_count,
    tool_call_count: row_kind_counts.tool_call_count,
    tool_burst_count: row_kind_counts.tool_burst_count,
    subagent_count: row_kind_counts.subagent_count,
});
```

Also include the four row-kind counts in the existing `tracing::info!` for `Finished rendering transcript page`.

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --lib session_detail::tests::session_detail_records_render_batch_measurements`

Expected: PASS.

---

### Task 2: Add heterogeneous-row measurement coverage

**Files:**
- Modify: `src/ui/session_detail.rs`
- Test: `src/ui/session_detail.rs`

- [ ] **Step 1: Write the failing test**

Add a test that avoids GTK widget insertion and validates the counting helper directly:

```rust
#[test]
fn render_item_kind_counts_capture_heterogeneous_rows() {
    let rows = vec![
        transcript_message_row(0, crate::models::Role::Assistant, "hello"),
        transcript_tool_row(1, "Read"),
        transcript_tool_row(2, "Edit"),
        transcript_subagent_row(3, "Explore"),
    ];

    let items = SessionDetail::build_display_items(
        rows,
        "session-1",
        None,
        Arc::new(PathBuf::from("/tmp/test.db")),
        0,
    );
    let counts = SessionDetail::count_render_item_kinds(&items);

    assert_eq!(counts.message_count, 1);
    assert_eq!(counts.tool_call_count, 0);
    assert_eq!(counts.tool_burst_count, 1);
    assert_eq!(counts.subagent_count, 1);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib session_detail::tests::render_item_kind_counts_capture_heterogeneous_rows`

Expected: FAIL to compile because helper row builders are missing.

- [ ] **Step 3: Add minimal test row builders**

Add private test helpers inside `mod tests`:

```rust
fn transcript_message_row(
    item_index: i64,
    role: crate::models::Role,
    content: &str,
) -> crate::database::TranscriptItemRow {
    crate::database::TranscriptItemRow {
        item_index,
        kind: crate::models::TranscriptItemKind::Message,
        reasoning_preview: crate::models::ReasoningPreview::default(),
        message_index: Some(item_index),
        role: Some(role),
        content_preview: Some(content.to_string()),
        content_len: Some(content.len() as i64),
        timestamp: Some(item_index),
        model: None,
        tool_call_id: None,
        tool_name: None,
        tool_status: None,
        tool_summary: None,
        tool_input_json: None,
        tool_output_text: None,
        duration_ms: None,
        subagent_id: None,
        subagent_title: None,
        subagent_prompt: None,
    }
}

fn transcript_tool_row(item_index: i64, tool_name: &str) -> crate::database::TranscriptItemRow {
    crate::database::TranscriptItemRow {
        item_index,
        kind: crate::models::TranscriptItemKind::ToolCall,
        reasoning_preview: crate::models::ReasoningPreview::default(),
        message_index: None,
        role: None,
        content_preview: None,
        content_len: None,
        timestamp: None,
        model: None,
        tool_call_id: Some(format!("call-{item_index}")),
        tool_name: Some(tool_name.to_string()),
        tool_status: Some(crate::models::ToolCallStatus::Completed),
        tool_summary: Some(format!("{tool_name} summary")),
        tool_input_json: Some("{}".to_string()),
        tool_output_text: None,
        duration_ms: Some(1),
        subagent_id: None,
        subagent_title: None,
        subagent_prompt: None,
    }
}

fn transcript_subagent_row(item_index: i64, title: &str) -> crate::database::TranscriptItemRow {
    crate::database::TranscriptItemRow {
        item_index,
        kind: crate::models::TranscriptItemKind::Subagent,
        reasoning_preview: crate::models::ReasoningPreview::default(),
        message_index: None,
        role: None,
        content_preview: None,
        content_len: None,
        timestamp: None,
        model: None,
        tool_call_id: None,
        tool_name: None,
        tool_status: None,
        tool_summary: None,
        tool_input_json: None,
        tool_output_text: None,
        duration_ms: None,
        subagent_id: Some(format!("subagent-{item_index}")),
        subagent_title: Some(title.to_string()),
        subagent_prompt: Some("investigate".to_string()),
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib session_detail::tests::render_item_kind_counts_capture_heterogeneous_rows`

Expected: PASS.

---

### Task 3: Verify without broad perf claims

**Files:**
- Modify: none unless formatting requires it

- [ ] **Step 1: Format check**

Run: `cargo fmt --all -- --check`

Expected: PASS. If it fails only because files need formatting, run `cargo fmt --all`, then repeat the check.

- [ ] **Step 2: Run focused tests**

Run: `cargo test --lib session_detail::tests::session_detail_records_render_batch_measurements session_detail::tests::render_item_kind_counts_capture_heterogeneous_rows`

Expected: PASS for both tests.

- [ ] **Step 3: Run broader relevant tests**

Run: `cargo test --lib session_detail::tests`

Expected: PASS.

- [ ] **Step 4: Inspect diff**

Run: `git diff -- src/ui/session_detail.rs docs/superpowers/plans/2026-04-28-session-detail-render-measurements.md`

Expected: diff only contains instrumentation, tests, and this plan.

---

## Self-Review

- Spec coverage: The plan covers the approved approach 1 only: targeted measurement surface from `main`/`slow-session`, no optimization and no virtualization.
- Placeholder scan: No TBD/TODO placeholders remain.
- Type consistency: `RenderMetrics`, `RenderRowKindCounts`, `last_render_metrics`, and `count_render_item_kinds` are introduced before use.
