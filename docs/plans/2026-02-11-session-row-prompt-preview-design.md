# Session Row Prompt Preview (Proposal A) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace session row project-path-first labeling with first user prompt preview, remove inline resume button from the row, and expose resume via right-click context menu.

**Architecture:** Extend the `Session` domain model and SQLite schema with `first_prompt`, populate it during indexing from the first `Role::User` message, and load it in all session queries. Then refactor `SessionRow` to a clean `AdwActionRow` layout (single-line title, compact metadata subtitle, chevron-only suffix) and attach a right-click `PopoverMenu` action that forwards the existing resume output signal.

**Tech Stack:** Rust 2024, Relm4 0.10 factory components, GTK4 (`GestureClick`, `PopoverMenu`, `gio::Menu`), libadwaita `AdwActionRow`, rusqlite.

---

## Scope and constraints

- Proposal chosen: **A - Clean ActionRow**.
- No behavioral change to left-click activation (open detail still handled by `SessionListMsg::SessionActivated`).
- Resume action remains available and must still emit `SessionRowOutput::ResumeRequested`.
- Existing DB files must migrate safely without dropping data.
- Keep implementation YAGNI: no new search features, no row redesign outside proposal A.

## Key references

- Relm4 Book - factories: `https://raw.githubusercontent.com/Relm4/book/refs/heads/main/src/efficient_ui/factory.md`
- Relm4 Book - component/view macro reference: `https://raw.githubusercontent.com/Relm4/book/refs/heads/main/src/component_macro/reference.md`
- Libadwaita `ActionRow`: `https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/class.ActionRow.html`
- GTK4 `GestureClick` and `PopoverMenu`:
  - `https://docs.gtk.org/gtk4/class.GestureClick.html`
  - `https://docs.gtk.org/gtk4/signal.GestureClick.pressed.html`
  - `https://docs.gtk.org/gtk4/class.PopoverMenu.html`
- GNOME HIG patterns:
  - Boxed lists: `https://developer.gnome.org/hig/patterns/containers/boxed-lists.html`
  - Menus: `https://developer.gnome.org/hig/patterns/controls/menus.html`

---

### Task 1: Add `first_prompt` to model and schema migration

**Files:**
- Modify: `src/models/session.rs`
- Modify: `src/database/schema.rs`
- Test: `src/database/schema.rs` (new `#[cfg(test)]` migration tests)

**Step 1: Write the failing migration test**

Add a test that creates a legacy `sessions` table without `first_prompt`, runs `initialize_database()`, then verifies `first_prompt` exists via `PRAGMA table_info(sessions)`.

```rust
#[test]
fn initialize_database_adds_first_prompt_column_for_legacy_schema() {
    // 1) create legacy schema
    // 2) call initialize_database(&conn)
    // 3) assert table_info contains "first_prompt"
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test initialize_database_adds_first_prompt_column_for_legacy_schema`
Expected: FAIL because migration path does not exist yet.

**Step 3: Implement schema + migration**

- Add `first_prompt TEXT` in `CREATE TABLE IF NOT EXISTS sessions (...)`.
- Add idempotent migration logic:
  - query `PRAGMA table_info(sessions)`;
  - if column missing, run `ALTER TABLE sessions ADD COLUMN first_prompt TEXT`.

**Step 4: Add model field**

Add to `Session`:

```rust
pub first_prompt: Option<String>,
```

**Step 5: Run test to verify it passes**

Run: `cargo test initialize_database_adds_first_prompt_column_for_legacy_schema`
Expected: PASS.

**Step 6: Commit**

```bash
git add src/models/session.rs src/database/schema.rs
git commit -m "feat: add first_prompt field and schema migration"
```

---

### Task 2: Persist and load `first_prompt` in database layer

**Files:**
- Modify: `src/database/indexer.rs`
- Modify: `src/database/mod.rs`
- Test: `tests/load_session.rs`

**Step 1: Write failing integration assertion**

In `tests/load_session.rs`, seed `sessions.first_prompt`, load via `load_session()`, and assert returned `Session.first_prompt` is populated.

```rust
assert_eq!(session.first_prompt.as_deref(), Some("Help me refactor this code"));
```

**Step 2: Run test to verify it fails**

Run: `cargo test load_session_returns_existing_session`
Expected: FAIL due to SQL column mismatch or missing mapping.

**Step 3: Update indexer insert statement**

In `insert_session_and_messages()`:
- extend `INSERT OR REPLACE INTO sessions` column list with `first_prompt`;
- pass `&session.first_prompt` in params.

**Step 4: Update all session SELECT queries and row mapping**

In `src/database/mod.rs`:
- include `first_prompt` in every session query used by:
  - `search_sessions_with_query()`
  - `load_sessions()`
  - `load_session()`
- update `session_from_row()` field offsets accordingly.

**Step 5: Run tests to verify pass**

Run: `cargo test load_session search_sessions`
Expected: PASS.

**Step 6: Commit**

```bash
git add src/database/indexer.rs src/database/mod.rs tests/load_session.rs
git commit -m "feat: persist and load first_prompt in session queries"
```

---

### Task 3: Extract and normalize first user prompt in parsers

**Files:**
- Modify: `src/parsers/mod.rs`
- Modify: `src/parsers/claude_code.rs`
- Modify: `src/parsers/opencode.rs`
- Modify: `src/parsers/codex.rs`
- Modify: `src/parsers/mistral_vibe.rs`
- Test: parser tests inside each parser module

**Step 1: Write failing parser assertions**

Add assertions in existing parser tests:

```rust
assert_eq!(session.first_prompt.as_deref(), Some("Summarize the repo"));
```

and equivalent per fixture for Claude/OpenCode/Mistral.

**Step 2: Run tests to verify they fail**

Run:
- `cargo test claude_code::tests::parse_returns_session_and_messages`
- `cargo test opencode::tests::message_reconstruction_orders_correctly`
- `cargo test codex::tests::parse_valid_session_extracts_messages`
- `cargo test mistral_vibe::tests::parse_valid_session_extracts_messages_and_tool_calls`

Expected: FAIL because `Session` constructor does not set `first_prompt`.

**Step 3: Implement shared helper in `src/parsers/mod.rs`**

Create helper(s):

```rust
pub(crate) fn extract_first_prompt(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .find(|msg| msg.role == Role::User)
        .map(|msg| normalize_prompt(&msg.content))
        .filter(|s| !s.is_empty())
}
```

Include:
- whitespace normalization (`split_whitespace().join(" ")` behavior),
- truncation to 200 chars (character-safe, not byte truncation).

**Step 4: Wire helper into all parser `Session` constructors**

For each parser, set:

```rust
first_prompt: crate::parsers::extract_first_prompt(&messages),
```

after message vector is finalized.

**Step 5: Run parser tests again**

Run: `cargo test parsers`
Expected: PASS.

**Step 6: Commit**

```bash
git add src/parsers/mod.rs src/parsers/claude_code.rs src/parsers/opencode.rs src/parsers/codex.rs src/parsers/mistral_vibe.rs
git commit -m "feat: extract and store first user prompt during parsing"
```

---

### Task 4: Refactor `SessionRow` layout to Proposal A

**Files:**
- Modify: `src/ui/session_row.rs`
- Test: `src/ui/session_row.rs` (new unit tests)

**Step 1: Write failing row formatting tests**

Add unit tests for:
- title uses `first_prompt` when present,
- title fallback uses project name then `Unknown project`,
- subtitle format is `project-name - N messages - relative-time` (or the project style separator chosen by implementation).

**Step 2: Run tests to verify they fail**

Run: `cargo test session_row`
Expected: FAIL for missing `first_prompt` title behavior.

**Step 3: Update row view to clean ActionRow**

In `view!`:
- set title from new helper `session_title()` using `first_prompt` first;
- set `set_title_lines: 1`;
- remove resume button suffix;
- remove right-aligned time suffix label;
- keep chevron suffix only;
- keep tool icon prefix (16px symbolic).

**Step 4: Update subtitle content builder**

Change subtitle from full path to compact metadata:
- derive project name from `project_path` basename;
- include `message_count`;
- include formatted relative time.

**Step 5: Run tests to verify pass**

Run: `cargo test session_row session_list`
Expected: PASS.

**Step 6: Commit**

```bash
git add src/ui/session_row.rs
git commit -m "feat: redesign session row to prompt-first clean actionrow"
```

---

### Task 5: Add right-click Resume context menu on session rows

**Files:**
- Modify: `src/ui/session_row.rs`
- Optional style tweak: `data/resources/style.css` (only if needed for popover spacing)

**Step 1: Write failing interaction test (or minimal structural test)**

If GTK test ergonomics allow, add a widget-level test that ensures right-click path is wired (action group exists and emits `ResumeRequested`). If not practical, add a focused unit test for the action callback helper function and document manual QA fallback.

**Step 2: Run test to verify fail**

Run: `cargo test session_row`
Expected: FAIL until action/menu wiring exists.

**Step 3: Implement popover menu and action**

Implementation shape:
- create `gio::Menu` with item label `Resume in Terminal` bound to a row-local action name;
- attach `gio::SimpleActionGroup` to row root;
- install action callback to emit `SessionRowOutput::ResumeRequested(session_id.clone(), tool)`;
- create `gtk::PopoverMenu::from_model(...)` parented to row root;
- add `gtk::GestureClick` with `set_button(3)`;
- on `pressed`, set pointing rectangle from `(x, y)` and call `popover.popup()`.

**Step 4: Ensure no left-click regression**

Keep existing row activation behavior untouched.

**Step 5: Run relevant tests**

Run: `cargo test session_row session_list`
Expected: PASS.

**Step 6: Commit**

```bash
git add src/ui/session_row.rs
git commit -m "feat: move resume action to row context menu"
```

---

### Task 6: Cross-cutting verification and cleanup

**Files:**
- Verify only: no new files expected besides tests/docs touched above.

**Step 1: Format**

Run: `cargo fmt --all`
Expected: no formatting errors.

**Step 2: Full test suite**

Run: `cargo test`
Expected: PASS.

**Step 3: Lint**

Run: `cargo clippy`
Expected: PASS or only accepted warnings.

**Step 4: Manual UX checklist**

Run app and verify:
- rows show prompt-first title with ellipsis,
- subtitle has compact metadata,
- no inline resume button in rows,
- right-click opens menu and resume action works,
- sessions from legacy DB still load.

**Step 5: Final commit**

```bash
git add -A
git commit -m "feat: implement proposal A prompt-first session rows"
```

---

## Non-goals

- Do not implement Proposal B/C/C-lite/D/E.
- Do not add `first_response`.
- Do not change search ranking or detail pane layout in this feature.

## Rollback plan

- If migration causes startup DB issues: keep `first_prompt` nullable and guard all reads with `Option`; hotfix by reverting migration branch and re-running with backup DB.
- If context menu introduces interaction bugs: temporarily disable right-click gesture and keep row layout changes; resume remains available from detail pane.

## Definition of done

- `Session` has `first_prompt` end-to-end (parser -> indexer -> DB -> load -> UI).
- Session list rows follow Proposal A visual structure.
- Resume is available through row context menu.
- `cargo fmt --all`, `cargo test`, and `cargo clippy` succeed.
