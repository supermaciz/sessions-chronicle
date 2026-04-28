# Lightweight Assistant Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce SessionDetail stalls by rendering collapsed assistant message previews with the lightweight label path while keeping full Markdown rendering for expanded content.

**Architecture:** Add an explicit `RenderContentMode` in `src/ui/transcript_row.rs`. Initial/collapsed message previews use `Preview`; expanded and loaded full content use `Full`. `Preview` renders assistant content through the same label/highlight path as user messages, avoiding markdown tables/code-block widgets in transcript previews.

**Tech Stack:** Rust 2024, GTK4/Relm4, existing `transcript_row` and `session_detail` tests, manual ignored perf smoke test.

---

### Task 1: Add Render Mode Test

**Files:**
- Modify: `src/ui/transcript_row.rs`

- [ ] **Step 1: Write the failing test**

Add a unit test that calls a new helper `should_render_markdown(role, mode)` and expects assistant previews not to use Markdown while assistant full content still does.

- [ ] **Step 2: Run the test**

Run: `cargo test should_render_markdown_only_for_assistant_full_content`

Expected: FAIL because the helper does not exist.

- [ ] **Step 3: Implement minimal helper and route render calls**

Add `RenderContentMode::{Preview, Full}` and pass `Preview` for initial/collapse, `Full` for expanded/cached/full-loaded content.

- [ ] **Step 4: Verify focused tests**

Run: `cargo test should_render_markdown_only_for_assistant_full_content transcript_row::tests`

Expected: PASS.

---

### Task 2: Measure Before/After Signal

**Files:**
- Modify: `src/ui/session_detail.rs`

- [ ] **Step 1: Run ignored perf smoke test**

Run: `cargo test session_detail_perf_smoke_measures_synthetic_scenarios -- --ignored --nocapture`

Expected: Markdown scenario drops near simple-message timing because preview no longer builds rich Markdown widgets.

- [ ] **Step 2: Run validation**

Run: `cargo fmt --all -- --check && cargo test session_detail::tests transcript_row::tests`

Expected: PASS.
