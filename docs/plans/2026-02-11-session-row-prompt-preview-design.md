# Session Row Prompt Preview (Proposal A) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Status:** Implemented

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

## Technical notes (from review)

### Relm4 factory `init_widgets`

The `view!` macro in `FactoryComponent` works the same as in regular components (using `self` to refer to the model). However, imperative widget construction (gesture controllers, popovers, action groups) **cannot** be expressed in the `view!` macro and **must** go inside `init_widgets`, which the current `SessionRow` already overrides.

### Column index safety

The `session_from_row()` mapper uses hardcoded column indices (`row.get(0)` through `row.get(6)`). The new `first_prompt` column must be **appended as the last column** (after `last_updated`) in the CREATE TABLE and all SELECT statements to preserve existing indices. The new field maps to `row.get(7)?`.

### PopoverMenu lifecycle in GTK4

A `PopoverMenu` created imperatively must not be dropped. Use `popover.set_parent(&root)` in `init_widgets` to transfer ownership to the widget tree. The gesture closure must capture a cloned reference to the popover for calling `popup()`.

### FactorySender output in imperative code

Inside `init_widgets`, the `view!` macro sugar (`sender.output(...)`) is not available for plain closures. Use:

```rust
let output_sender = sender.output_sender().clone();
action.connect_activate(move |_, _| {
    let _ = output_sender.send(SessionRowOutput::ResumeRequested(id.clone(), tool));
});
```

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

- **Append** `first_prompt TEXT` as the **last column** in `CREATE TABLE IF NOT EXISTS sessions (...)`, after `last_updated`. This preserves all existing column indices (0–6) in `session_from_row()`.
- Add idempotent migration logic:
  - query `PRAGMA table_info(sessions)`;
  - if column missing, run `ALTER TABLE sessions ADD COLUMN first_prompt TEXT`.
- Existing rows get `NULL` for `first_prompt`, which maps to `Option::None`.

**Step 4: Add model field**

Add to `Session` (in `src/models/session.rs`):

```rust
#[serde(default)]
pub first_prompt: Option<String>,
```

Note: `#[serde(default)]` ensures backward-compatible deserialization if `Session` is ever read from JSON without the field.

**Step 5: Update all existing `Session` struct literals in tests**

Adding `first_prompt` to `Session` will break any test that constructs a `Session` inline. Update:
- `src/ui/session_list.rs:274` — add `first_prompt: None,` to the `Session` literal in `session_list_emits_selection_on_row_activation`.
- Any other direct `Session` construction sites discovered during compilation.

**Step 6: Run test to verify it passes**

Run: `cargo test initialize_database_adds_first_prompt_column_for_legacy_schema`
Expected: PASS.

**Step 7: Commit**

```bash
git add src/models/session.rs src/database/schema.rs src/ui/session_list.rs
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

In `insert_session_and_messages()` (`src/database/indexer.rs:241-254`):
- extend `INSERT OR REPLACE INTO sessions` column list with `first_prompt` as the 8th column;
- pass `&session.first_prompt` as `?8` in params.

**Step 4: Update all 5 session SELECT queries and row mapping**

In `src/database/mod.rs`, add `s.first_prompt` (or `first_prompt`) to **all five** SQL strings:

1. `search_sessions_with_query()` — all-tools variant (line ~113): add `s.first_prompt` after `s.last_updated`
2. `search_sessions_with_query()` — filtered variant (line ~127): add `s.first_prompt` after `s.last_updated`
3. `load_sessions()` — all-tools variant (line ~176): add `first_prompt` after `last_updated`
4. `load_sessions()` — filtered variant (line ~187): add `first_prompt` after `last_updated`
5. `load_session()` (line ~220): add `first_prompt` after `last_updated`

Update `session_from_row()` to read the new column at **index 7**:

```rust
first_prompt: row.get(7)?,
```

This preserves all existing indices (0–6) unchanged.

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

**Step 5: Update Codex parser TODO comment**

In `src/parsers/codex.rs:159-162`, remove or update the TODO comment about title extraction since `first_prompt` now fulfills that intent:

```rust
// first_prompt is populated via extract_first_prompt() in parsers/mod.rs
```

**Step 6: Run parser tests again**

Run: `cargo test parsers`
Expected: PASS.

**Step 7: Commit**

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
- subtitle format is `project-name · N messages · relative-time` (or the project style separator chosen by implementation).

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

**Step 1: Extract resume action handler into a testable helper**

Create a standalone function that can be unit-tested without GTK:

```rust
fn emit_resume(sender: &relm4::Sender<SessionRowOutput>, id: &str, tool: Tool) {
    let _ = sender.send(SessionRowOutput::ResumeRequested(id.to_string(), tool));
}
```

Add a unit test for this helper.

**Step 2: Run test to verify fail**

Run: `cargo test session_row`
Expected: FAIL until helper exists.

**Step 3: Implement popover menu and action in `init_widgets`**

All imperative widget wiring **must go inside `init_widgets`**, since the `view!` macro cannot express gesture controllers, popovers, or action groups.

Implementation shape inside `init_widgets`:

```rust
// 1. Create menu model
let menu = gio::Menu::new();
menu.append(Some("Resume in Terminal"), Some("row.resume"));

// 2. Create action group with "row" prefix
let action_group = gio::SimpleActionGroup::new();
let resume_action = gio::SimpleAction::new("resume", None);

// 3. Capture output sender (NOT sender.output() — that's view! macro sugar)
let output_sender = sender.output_sender().clone();
let session_id = self.session.id.clone();
let tool = self.session.tool;
resume_action.connect_activate(move |_, _| {
    emit_resume(&output_sender, &session_id, tool);
});
action_group.add_action(&resume_action);
root.insert_action_group("row", Some(&action_group));

// 4. Create popover, parent it to root for lifecycle management
let popover = gtk::PopoverMenu::from_model(Some(&menu));
popover.set_parent(&root);

// 5. Attach right-click gesture
let gesture = gtk::GestureClick::new();
gesture.set_button(3); // right-click
let popover_ref = popover.clone();
gesture.connect_pressed(move |_, _, x, y| {
    popover_ref.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover_ref.popup();
});
root.add_controller(gesture);
```

Note on action prefix: the `gio::Menu` item uses `"row.resume"` which matches the action group registered as `"row"` on the root widget.

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
