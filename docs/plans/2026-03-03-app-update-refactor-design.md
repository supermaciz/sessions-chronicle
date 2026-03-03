# App Update Modular Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Split `App::update` into focused handler modules while preserving all runtime behavior.

**Architecture:** Convert `src/app.rs` to a module tree rooted at `src/app/mod.rs`, then extract shared enums/helpers and message handlers into `src/app/types.rs`, `src/app/helpers.rs`, and `src/app/handlers/*.rs`. Keep `mod.rs` as a thin Relm4 integration layer with dispatch-only `update` match arms.

**Tech Stack:** Rust 2024, Relm4, GTK4/libadwaita, cargo fmt, clippy, cargo test.

---

### Task 1: Create `src/app/` module scaffold

**Files:**
- Create: `src/app/mod.rs`
- Create: `src/app/handlers/mod.rs`
- Modify: `src/main.rs` (module path compatibility check only)
- Delete: `src/app.rs`

**Step 1: Move app root into module form**

Move `src/app.rs` to `src/app/mod.rs` with no logic changes.

**Step 2: Add handlers module placeholder**

Create `src/app/handlers/mod.rs` with a placeholder comment-free module declaration surface.

**Step 3: Verify compile after move**

Run: `cargo test app::tests::utility_pane_mode_maps_to_correct_stack_child_name -- --exact`

Expected: PASS; module path works with `mod app;` from `src/main.rs`.

**Step 4: Commit**

Run:
`git add src/app/mod.rs src/app/handlers/mod.rs src/app.rs`
`git commit -m "refactor(app): convert app.rs to app module scaffold"`

---

### Task 2: Extract app-local types and pure helpers

**Files:**
- Create: `src/app/types.rs`
- Create: `src/app/helpers.rs`
- Modify: `src/app/mod.rs`

**Step 1: Write failing check (compile usage before exports)**

Run: `cargo test app::tests::active_search_query_treats_blank_input_as_none -- --exact`

Expected: PASS before extraction (baseline guard).

**Step 2: Extract types into `types.rs`**

Move:
- `UtilityPaneMode`
- `ActiveSessionRef`
- `ReindexAction`
- `EscapeResolution`

**Step 3: Extract pure helpers into `helpers.rs`**

Move:
- `active_search_query`
- `search_query_update_messages`
- `parent_session_load_failure_messages`
- `resolve_escape_action`
- `transition_to_detail`
- `transition_to_list`
- `detail_pop_sync_decision`
- `decide_reindex_action`

**Step 4: Verify helper tests**

Run:
- `cargo test app::tests::search_query_update_messages_include_detail_update -- --exact`
- `cargo test app::tests::escape_priority_chain_search_then_inspector_then_back -- --exact`

Expected: PASS; behavior unchanged.

**Step 5: Commit**

Run:
`git add src/app/mod.rs src/app/types.rs src/app/helpers.rs`
`git commit -m "refactor(app): extract app types and pure helpers"`

---

### Task 3: Extract navigation/search/escape handlers

**Files:**
- Create: `src/app/handlers/navigation.rs`
- Modify: `src/app/handlers/mod.rs`
- Modify: `src/app/mod.rs`

**Step 1: Write failing check boundary (current behavior lock)**

Run:
- `cargo test app::tests::unsuppressed_pop_signal_syncs_when_detail_visible -- --exact`
- `cargo test app::tests::escape_priority_chain_search_then_inspector_then_back -- --exact`

Expected: PASS before extraction (guardrails).

**Step 2: Move navigation/search handlers**

Extract methods from `impl App`:
- `handle_search_mode_changed`
- `handle_toggle_pane`
- `handle_pane_visibility_changed`
- `handle_search_query_changed`
- `handle_request_navigate_back`
- `handle_navigate_back`
- `handle_escape`

**Step 3: Keep update as dispatcher**

`fn update` should only route message payloads to `handle_*` methods, with no embedded large logic blocks.

**Step 4: Verify navigation tests**

Run:
- `cargo test app::tests::escape_priority_chain_search_then_inspector_then_back -- --exact`
- `cargo test app::tests::unsuppressed_pop_signal_is_ignored_when_detail_hidden -- --exact`

Expected: PASS.

**Step 5: Commit**

Run:
`git add src/app/mod.rs src/app/handlers/mod.rs src/app/handlers/navigation.rs`
`git commit -m "refactor(app): extract navigation and escape handlers"`

---

### Task 4: Extract session selection and inspector handlers

**Files:**
- Create: `src/app/handlers/sessions.rs`
- Modify: `src/app/handlers/mod.rs`
- Modify: `src/app/mod.rs`

**Step 1: Add common session loading helper**

Add a helper method in sessions module for the shared `load_session` + `active_session` + detail emission path.

**Step 2: Extract session/inspector handlers**

Extract methods:
- `handle_session_selected`
- `handle_open_child_session`
- `handle_return_to_parent_session`
- `handle_inspect_tool_call`
- `handle_inspect_subagent`

**Step 3: Verify behavior tests**

Run:
- `cargo test app::tests::parent_session_load_failure_clears_detail_and_inspector -- --exact`

Expected: PASS.

**Step 4: Commit**

Run:
`git add src/app/mod.rs src/app/handlers/mod.rs src/app/handlers/sessions.rs`
`git commit -m "refactor(app): extract session and inspector handlers"`

---

### Task 5: Extract resume and indexing handlers

**Files:**
- Create: `src/app/handlers/resume.rs`
- Create: `src/app/handlers/indexing.rs`
- Modify: `src/app/handlers/mod.rs`
- Modify: `src/app/mod.rs`

**Step 1: Extract resume flow**

Extract methods:
- `handle_resume_session`
- `handle_resume_active_session`

Keep all existing user feedback semantics (error dialog/toast titles) intact.

**Step 2: Extract indexing flow**

Extract methods:
- `handle_reindex_requested`
- `handle_indexing_completed`
- `handle_indexing_failed`

**Step 3: Verify indexing + startup UI test**

Run:
- `cargo test app::tests::reindex_request_starts_full_reindex_when_idle -- --exact`
- `cargo test app::tests::startup_shows_indexing_spinner_during_incremental_indexing -- --exact`

Expected: PASS.

**Step 4: Commit**

Run:
`git add src/app/mod.rs src/app/handlers/mod.rs src/app/handlers/resume.rs src/app/handlers/indexing.rs`
`git commit -m "refactor(app): extract resume and indexing handlers"`

---

### Task 6: Final verification and cleanup

**Files:**
- Modify: `src/app/mod.rs`
- Modify: `src/app/handlers/*.rs`

**Step 1: Ensure `App::update` remains dispatch-only**

Confirm only message routing remains in `update`.

**Step 2: Run formatter and lints**

Run:
- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`

Expected: PASS.

**Step 3: Run full tests**

Run:
- `cargo test --all --no-fail-fast`

Expected: PASS.

**Step 4: Commit final cleanup**

Run:
`git add src/app/mod.rs src/app/handlers/*.rs src/app/helpers.rs src/app/types.rs`
`git commit -m "refactor(app): simplify App update dispatch"`

---

## Expected Outcome

- `App::update` becomes short and readable.
- Message behavior is preserved for search/navigation/sessions/resume/indexing.
- Future `AppMsg` additions can be implemented in focused modules instead of a monolithic function.
