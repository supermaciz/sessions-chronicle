# Model Display Option 1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Display the raw assistant model slug inline in each message header as `ASSISTANT · <model> · HH:MM:SS`, with no custom widget and no model transformation.

**Architecture:** Extend the existing transcript preview pipeline end-to-end (`messages.model` -> `TranscriptItemRow` -> `MessagePreview` -> `TranscriptRow`). Keep rendering logic simple and deterministic: only assistant messages with non-empty model values show the extra label and separators. Preserve existing role colors, message layout, and truncation behavior.

**Tech Stack:** Rust 2024, rusqlite (SQLite/FTS5), Relm4 + GTK4/libadwaita CSS classes (`caption`, `dim-label`, `monospace`), cargo test/clippy/fmt.

---

### Task 1: Lock behavior with failing data-pipeline test

**Files:**
- Create: `tests/transcript_items_model.rs`
- Test: `tests/transcript_items_model.rs`

**Step 1: Write the failing test**

Create a new integration test that seeds:
- one assistant message with model `claude-sonnet-4-5-20250514`
- one user message with `NULL` model
- transcript item rows for both

Assert from `load_transcript_items(...)`:
- assistant row includes `model == Some("claude-sonnet-4-5-20250514")`
- user row includes `model == None`

```rust
#[test]
fn load_transcript_items_exposes_model_for_assistant_only() {
    // setup temp db + schema
    // insert session, messages, transcript_items
    // call load_transcript_items
    // assert assistant row model Some(...), user row model None
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test transcript_items_model load_transcript_items_exposes_model_for_assistant_only -- --exact`
Expected: FAIL with compile error or assertion failure because `TranscriptItemRow` does not expose `model` yet.

**Step 3: Commit test scaffold**

```bash
git add tests/transcript_items_model.rs
git commit -m "test: add transcript item model pipeline expectation"
```

---

### Task 2: Extend DB transcript row shape with `model`

**Files:**
- Modify: `src/database/mod.rs`
- Test: `tests/transcript_items_model.rs`

**Step 1: Implement minimal DB changes**

In `src/database/mod.rs`:
- add `pub model: Option<String>` to `TranscriptItemRow`
- update `load_transcript_items` SELECT to include `m.model`
- map the selected column into `TranscriptItemRow.model`
- keep current column ordering coherent and adjust indices safely

```rust
pub struct TranscriptItemRow {
    // ...existing fields...
    pub timestamp: Option<i64>,
    pub model: Option<String>,
    // ...tool/subagent fields...
}
```

```sql
SELECT ti.item_index, ti.kind, ti.message_index, ti.tool_call_id, ti.subagent_id,
       m.role, substr(m.content, 1, ?2) AS content_preview,
       length(m.content) AS content_len, m.timestamp, m.model,
       tc.tool_name, tc.status, tc.summary, tc.duration_ms,
       sa.title AS subagent_title, sa.prompt AS subagent_prompt
```

**Step 2: Run targeted tests**

Run: `cargo test --test transcript_items_model -- --nocapture`
Expected: PASS for the new test.

**Step 3: Run regression tests around DB reads**

Run: `cargo test --test message_preview --test load_session --test search_sessions`
Expected: PASS, proving no regression in existing query behavior.

**Step 4: Commit DB pipeline change**

```bash
git add src/database/mod.rs tests/transcript_items_model.rs
git commit -m "feat: expose message model in transcript item rows"
```

---

### Task 3: Propagate model through `MessagePreview` and row init

**Files:**
- Modify: `src/models/message_preview.rs`
- Modify: `src/database/mod.rs`
- Modify: `src/ui/transcript_row.rs`
- Test: `tests/message_preview.rs`

**Step 1: Add failing test for preview model field**

In `tests/message_preview.rs`, add a test inserting an assistant message with a model and asserting `load_message_previews_for_session(...)` returns it on the preview.

```rust
#[test]
fn load_message_previews_keeps_model_slug() {
    // insert assistant message with model
    // assert previews[0].model.as_deref() == Some("o3-mini")
}
```

**Step 2: Run the failing test**

Run: `cargo test --test message_preview load_message_previews_keeps_model_slug -- --exact`
Expected: FAIL because `MessagePreview` has no `model` field yet.

**Step 3: Implement minimal propagation**

- Add `pub model: Option<String>` to `MessagePreview`.
- Update all `MessagePreview { ... }` constructors:
  - `load_message_previews_for_session` query/select mapping
  - `transcript_item_init_from_row` mapping from `row.model`
  - unknown-kind fallback initializes `model: None`

```rust
pub struct MessagePreview {
    pub session_id: String,
    pub message_index: usize,
    pub role: Role,
    pub content_preview: String,
    pub content_len: usize,
    pub timestamp: DateTime<Utc>,
    pub model: Option<String>,
}
```

**Step 4: Re-run tests**

Run: `cargo test --test message_preview -- --nocapture`
Expected: PASS (including existing preview tests).

**Step 5: Commit preview propagation**

```bash
git add src/models/message_preview.rs src/database/mod.rs src/ui/transcript_row.rs tests/message_preview.rs
git commit -m "feat: carry message model through preview structs"
```

---

### Task 4: Render Option 1 inline caption in transcript header

**Files:**
- Modify: `src/ui/transcript_row.rs`
- Modify: `data/resources/style.css` (no changes expected; touch only if spacing tweak is necessary)
- Test: `src/ui/transcript_row.rs` (unit tests for helper function)

**Step 1: Add failing unit tests for visibility rule**

Extract a pure helper in `src/ui/transcript_row.rs`:
- returns model display text only when `role == Role::Assistant` and model is non-empty after trim

Add tests:
- assistant + model => visible
- assistant + empty/whitespace => hidden
- user/toolresult + model => hidden

```rust
fn model_label_text(role: Role, model: Option<&str>) -> Option<String> {
    // pure logic
}
```

**Step 2: Run unit tests to verify fail**

Run: `cargo test model_label_text -- --nocapture`
Expected: FAIL before helper implementation.

**Step 3: Implement UI header update**

In `build_message_widgets`:
- keep role label as-is
- if helper returns text, append:
  - separator label `"·"` with `caption dim-label`
  - model label with `caption dim-label monospace`
  - separator label `"·"` with `caption dim-label`
- append timestamp label last (unchanged format `%H:%M:%S`)

Pseudo-structure:

```text
ASSISTANT · claude-sonnet-4-5-20250514 · 14:32:05
```

**Step 4: Re-run focused tests**

Run: `cargo test model_label_text -- --nocapture`
Expected: PASS.

**Step 5: Commit UI rendering change**

```bash
git add src/ui/transcript_row.rs
git commit -m "feat: show assistant model slug in transcript header"
```

---

### Task 5: Full verification and manual UX check

**Files:**
- Verify only (no mandatory file edits)

**Step 1: Run CI-parity checks**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
cargo test --all --no-fail-fast
```

Expected: all commands PASS.

**Step 2: Manual fixture validation**

Run app with fixtures:

```bash
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures
```

Manual checks:
- assistant rows show `ASSISTANT · <raw-slug> · <time>`
- user/tool rows do not show model
- no layout break on narrow window (timestamp remains visible when possible; no crash)

**Step 3: Capture screenshot evidence (if preparing PR)**

Save updated UI screenshot under `docs/screenshots/` if this work will be submitted as a PR.

**Step 4: Final commit (optional squash policy dependent)**

```bash
git add -A
git commit -m "test: verify model display option 1 end-to-end"
```

---

## Risks and Mitigations

- Long model slugs may reduce header space on small widths.
  - Mitigation: keep this release strict to option 1; defer truncation/tooltip hybrid to a follow-up if needed.
- Column index drift in SQL row mapping can cause subtle bugs.
  - Mitigation: explicit integration test in `tests/transcript_items_model.rs` and full test suite run.
- UI-only regressions can be missed by unit tests.
  - Mitigation: fixture-based manual validation in Flatpak runtime.

## Out of Scope

- Model name shortening/normalization for display.
- Provider color coding or badges.
- Tooltip/popover-based model details.
- Model-based filters/search UI.
