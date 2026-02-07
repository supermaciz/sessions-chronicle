# Sessions Dir Unified Behavior Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make `--sessions-dir` behave consistently across Claude Code, OpenCode, Codex, and Mistral Vibe, while preventing stale cross-source data with an isolated override database and exposing a manual reset action in Preferences.

**Architecture:** Introduce a single source resolver that maps one optional override root into four effective input paths (one per tool), preferring known fixture subdirectories and falling back to the root itself. Use a dedicated database file when override mode is active so default home indexing and override indexing never contaminate each other. Add a Preferences action that clears and rebuilds the active index using the current runtime source mapping.

**Tech Stack:** Rust 2024, Relm4, libadwaita, clap, rusqlite, gio::Settings.

---

## Preconditions and constraints

- Keep current CLI surface (`--sessions-dir`) and extend semantics, do not add new required flags.
- In override mode, never read tool session paths from `HOME`.
- Keep sidechain/subagent pruning behavior unchanged.
- Keep operation safe: reset action only touches local index DB, never source files.
- Use @superpowers:test-driven-development for each behavior change.

### Task 1: Add unified source resolution module

**Files:**
- Create: `src/session_sources.rs`
- Modify: `src/main.rs`
- Modify: `src/lib.rs`
- Test: `src/session_sources.rs`

**Step 1: Write the failing test**

Add unit tests in `src/session_sources.rs` for:

```rust
#[test]
fn resolve_override_prefers_known_subdirectories() {}

#[test]
fn resolve_override_falls_back_to_root_when_subdirs_missing() {}

#[test]
fn resolve_default_uses_tool_defaults() {}
```

Expected behavior:
- Override root `tests/fixtures` maps to:
  - Claude: `<root>/claude_sessions`
  - OpenCode root: `<root>/opencode_storage`
  - Codex: `<root>/codex_sessions`
  - Vibe: `<root>/vibe_sessions`
- If a subdirectory is missing, use `<root>` for that tool.
- No override: use `Tool::session_dir()`-based defaults.

**Step 2: Run test to verify it fails**

Run: `cargo test session_sources -- --nocapture`

Expected: FAIL (module/tests not implemented yet).

**Step 3: Write minimal implementation**

In `src/session_sources.rs`, add:

```rust
pub struct SessionSources {
    pub claude_dir: std::path::PathBuf,
    pub opencode_storage_root: std::path::PathBuf,
    pub codex_dir: std::path::PathBuf,
    pub vibe_dir: std::path::PathBuf,
    pub override_mode: bool,
}

impl SessionSources {
    pub fn resolve(override_root: Option<&std::path::Path>) -> Self { /* ... */ }
}
```

Resolution rules:
- `override_mode = override_root.is_some()`.
- Override mode:
  - try known subdir; if it exists use it, else fallback to override root.
- Default mode:
  - Claude/Codex/Vibe from `Tool::session_dir()`.
  - OpenCode storage root derived from `Tool::OpenCode.session_dir()` parent.

**Step 4: Run tests to verify pass**

Run: `cargo test session_sources -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/session_sources.rs src/main.rs src/lib.rs
git commit -m "feat: add unified session source resolver"
```

### Task 2: Wire app startup to unified sources, DB isolation, and persist sources

**Files:**
- Modify: `src/app.rs`
- Test: `src/app.rs` (or extracted helper unit tests)

**Step 1: Write the failing test**

Add testable helper(s), e.g.:

```rust
fn select_db_filename(override_mode: bool) -> &'static str {
    if override_mode { "sessions-override.db" } else { "sessions.db" }
}
```

Test:

```rust
#[test]
fn db_filename_changes_in_override_mode() {}
```

**Step 2: Run test to verify it fails**

Run: `cargo test db_filename_changes_in_override_mode -- --nocapture`

Expected: FAIL.

**Step 3: Write minimal implementation**

In `App::init`:
- Resolve `SessionSources` from incoming `sessions_dir`.
- Choose DB path by mode:
  - default: `<user_data_dir>/<APP_ID>/sessions.db`
  - override: `<user_data_dir>/<APP_ID>/sessions-override.db`
- **Store `SessionSources` in the `App` struct** so it remains accessible for later reindexing (Task 5 needs it for `AppMsg::ReindexRequested`).
- Index using resolved paths only.
- Keep existing per-tool log lines, but log resolved source path(s).

Updated `App` struct (add field):

```rust
pub(super) struct App {
    // ... existing fields ...
    sources: SessionSources,
}
```

**Step 4: Run tests to verify pass**

Run: `cargo test db_filename_changes_in_override_mode -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat: isolate override indexing db and use unified sources"
```

### Task 3: Add full index reset primitive

**Files:**
- Modify: `src/database/indexer.rs`
- Test: `src/database/indexer.rs`

**Step 1: Write the failing test**

Add test:

```rust
#[test]
fn clear_all_sessions_removes_sessions_and_messages() {}
```

Seed at least one session and one message, call `clear_all_sessions()`, assert both counts are zero.

**Step 2: Run test to verify it fails**

Run: `cargo test clear_all_sessions_removes_sessions_and_messages -- --nocapture`

Expected: FAIL.

**Step 3: Write minimal implementation**

Add to `SessionIndexer`:

```rust
/// Clear all indexed sessions and messages.
///
/// Note: `messages` is an FTS5 virtual table. Standard `DELETE FROM` works
/// correctly on FTS5 tables and participates in transactions normally.
pub fn clear_all_sessions(&mut self) -> Result<()> {
    let tx = self.db.transaction()?;
    tx.execute("DELETE FROM messages", [])?;
    tx.execute("DELETE FROM sessions", [])?;
    tx.commit()?;
    Ok(())
}
```

**Step 4: Run tests to verify pass**

Run: `cargo test clear_all_sessions_removes_sessions_and_messages -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/database/indexer.rs
git commit -m "feat: add full index reset operation"
```

### Task 4: Add Reload message to session list

> Moved before the Preferences task (was Task 5) because it is simple, independent, and can be tested in isolation before wiring the full reindex flow.

**Files:**
- Modify: `src/ui/session_list.rs`
- Test: `src/ui/session_list.rs`

**Step 1: Write the failing test**

Add message:

```rust
pub enum SessionListMsg {
    // ...
    Reload,
}
```

Add a test ensuring `Reload` executes same path as filter/search-triggered refresh and does not panic with empty DB.

**Step 2: Run test to verify it fails**

Run: `cargo test session_list -- --nocapture`

Expected: FAIL.

**Step 3: Write minimal implementation**

- Handle `SessionListMsg::Reload` by calling `reload_sessions()`.
- No changes to `app.rs` yet -- the wiring from `AppMsg::ReindexRequested` to `SessionListMsg::Reload` happens in Task 5.

**Step 4: Run tests to verify pass**

Run: `cargo test session_list -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/ui/session_list.rs
git commit -m "feat: add Reload message to session list"
```

### Task 5: Expose reset action in Preferences and wire reindex flow

> This task involves a **significant refactoring** of the PreferencesDialog: migrating from the current fire-and-forget pattern to a controller-with-outputs pattern. The sub-steps below detail this migration explicitly.

**Files:**
- Modify: `src/ui/modals/preferences.rs`
- Modify: `src/ui/modals/mod.rs` (if type exports change)
- Modify: `src/app.rs`
- Modify: `data/io.github.supermaciz.sessionschronicle.gschema.xml.in` (only if adding preference key)

#### Current state (what needs to change)

The PreferencesDialog is currently fire-and-forget:

```rust
// app.rs — inside post_view / action setup
let preferences_action = {
    RelmAction::<PreferencesAction>::new_stateless(move |_| {
        PreferencesDialog::builder().launch(()).detach(); // output channel dropped
    })
};
```

And the dialog presents itself inside its own `init()`:

```rust
// preferences.rs
fn init(_: Self::Init, root: Self::Root, _sender: ComponentSender<Self>) -> ComponentParts<Self> {
    // ... build widgets ...
    root.present(Some(&main_application().windows()[0]));
    ComponentParts { model, widgets }
}
```

This pattern cannot support outputs. Four changes are required:

1. **Define `PreferencesInput` and `PreferencesOutput` enums** on the dialog.
2. **Create the dialog once in `App::init()`** using `.forward(sender.input_sender(), ...)` and store the `Controller<PreferencesDialog>` in `App`.
3. **Replace the stateless action** with an `AppMsg::ShowPreferences` that calls `present()` on the stored controller's widget.
4. **Move `present()` out of `PreferencesDialog::init()`** — the parent controls visibility.

#### Step 1: Define message contracts

In `preferences.rs`:

```rust
pub enum PreferencesInput {
    /// User clicked the "Reset session index" button.
    ResetClicked,
    /// Confirmation dialog responded.
    ResetConfirmed,
}

pub enum PreferencesOutput {
    ReindexRequested,
}
```

In `app.rs`:

```rust
enum AppMsg {
    // ... existing variants ...
    ShowPreferences,
    ReindexRequested,
}
```

#### Step 2: Refactor PreferencesDialog to controller-with-outputs

In `preferences.rs`:
- Change `type Input = PreferencesInput;` and `type Output = PreferencesOutput;`.
- Remove the `root.present(...)` call from `init()`.
- Add an "Advanced" preferences group with an action row/button labeled **"Reset session index"**.
- On button click: send `PreferencesInput::ResetClicked`.
- In `update()`, handle `ResetClicked` by showing an `adw::AlertDialog` confirmation.

**`adw::AlertDialog` API note:** Unlike `gtk::MessageDialog`, libadwaita's `AlertDialog` uses **string-based response IDs**, not `gtk::ResponseType`:

```rust
// In update(), handling ResetClicked:
let dialog = adw::AlertDialog::builder()
    .heading("Reset session index?")
    .body("This will clear and rebuild the entire session index from source files.")
    .build();
dialog.add_response("cancel", "Cancel");
dialog.add_response("confirm", "Reset");
dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
dialog.set_default_response(Some("cancel"));
dialog.set_close_response("cancel");

let sender_clone = sender.clone();
dialog.connect_response(None, move |_, response| {
    if response == "confirm" {
        sender_clone.input(PreferencesInput::ResetConfirmed);
    }
});
dialog.present(Some(&root_widget));
```

Handle `ResetConfirmed` by emitting: `sender.output(PreferencesOutput::ReindexRequested).unwrap();`

#### Step 3: Refactor App to use stored controller

In `App::init()`:

```rust
// Create preferences dialog once, with forwarded outputs
let preferences_dialog = PreferencesDialog::builder()
    .launch(())
    .forward(sender.input_sender(), |msg| match msg {
        PreferencesOutput::ReindexRequested => AppMsg::ReindexRequested,
    });
```

Store in `App` struct:

```rust
pub(super) struct App {
    // ... existing fields ...
    sources: SessionSources,  // from Task 2
    preferences_dialog: Controller<PreferencesDialog>,
}
```

Replace the stateless action:

```rust
// Old: PreferencesDialog::builder().launch(()).detach();
// New: send a message to App, which presents the stored dialog
let preferences_action = {
    let sender = sender.clone();
    RelmAction::<PreferencesAction>::new_stateless(move |_| {
        sender.input(AppMsg::ShowPreferences);
    })
};
```

Handle `AppMsg::ShowPreferences` in `update()`:

```rust
AppMsg::ShowPreferences => {
    let dialog_widget = self.preferences_dialog.widget();
    dialog_widget.present(Some(&root_widget));
}
```

Handle `AppMsg::ReindexRequested` in `update()`:

1. Open `SessionIndexer` for active DB (`self.db_path`).
2. Call `clear_all_sessions()`.
3. Re-run indexing using `self.sources` (stored in Task 2).
4. Send `SessionListMsg::Reload` to the session list controller (wired in Task 4).
5. Show success/failure toast.

#### Step 4: Test strategy

Automated testing of Relm4 UI wiring requires `gtk::init()` and an event loop, which is impractical for unit tests. The test strategy for this task is:

- **Unit-testable:** The `select_db_filename` helper (Task 2) and `clear_all_sessions` primitive (Task 3) are already covered.
- **Compile-time verification:** The type system enforces that `PreferencesOutput::ReindexRequested` maps to `AppMsg::ReindexRequested` via the `.forward()` closure. A type mismatch will not compile.
- **Manual verification:** The full flow (button click -> confirmation -> reindex -> list reload -> toast) is verified in Task 6 step 5.

Run: `cargo test` (ensure no regressions).

Expected: PASS.

#### Step 5: Known technical debt — synchronous reindex blocks UI

The reindex operation (clear + re-index all tools) runs synchronously on the GTK main thread. This **will freeze the UI** during the operation. For small to medium datasets this is acceptable (sub-second). For large datasets it could be noticeable.

Accepted debt. Future improvement path: use a Relm4 `Worker` (dedicated background thread) or `CommandOutput` for async indexing, with a spinner in the dialog during execution.

#### Step 6: Commit

```bash
git add src/ui/modals/preferences.rs src/app.rs src/ui/modals/mod.rs
git commit -m "feat: add preferences action to reset and rebuild index"
```

### Task 6: Documentation and final verification

**Files:**
- Modify: `docs/DEVELOPMENT_WORKFLOW.md`
- Modify: `docs/PROJECT_STATUS.md`

**Step 1: Update docs**

Document new behavior:
- `--sessions-dir` applies to all tools.
- Known subdir mapping and fallback-to-root behavior.
- Override mode uses isolated DB (`sessions-override.db`).
- Preferences includes manual `Reset session index` action.

**Step 2: Run formatting and checks**

Run: `cargo fmt --all`

Expected: no diff after formatting.

**Step 3: Run full test suite**

Run: `cargo test`

Expected: PASS.

**Step 4: Run lint checks**

Run: `cargo clippy`

Expected: PASS or no new warnings introduced by this change set.

**Step 5: Manual verification**

Run app without override:

```bash
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle
```

Run app with fixture root override:

```bash
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures
```

Run app with Claude-only-like root:

```bash
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures/claude_sessions
```

Expected manual results:
- Override run does not index from home paths.
- Switching between modes does not mix stale sessions.
- Preferences reset clears and rebuilds active DB index.
- UI does not freeze noticeably during reindex on fixture data.

**Step 6: Commit**

```bash
git add docs/DEVELOPMENT_WORKFLOW.md docs/PROJECT_STATUS.md
git commit -m "docs: describe unified sessions-dir behavior and reset flow"
```

## Rollback strategy

- Safe rollback path: remove resolver wiring and revert to existing per-tool defaults in `App::init`.
- Keep `clear_all_sessions` even if UI action is rolled back (useful maintenance primitive).
- If UI wiring causes regressions, disable only the Preferences button while preserving CLI behavior improvements.
- PreferencesDialog can be reverted to fire-and-forget independently of the source resolver changes.

## Risks and mitigations

- **Risk:** OpenCode override root confusion (session subpath expectations).
  - **Mitigation:** Resolver chooses `opencode_storage` when present; fallback root keeps flexibility.
- **Risk:** Users surprised by two DB files.
  - **Mitigation:** Document clearly; expose reset action and log active DB file at startup.
- **Risk:** Long reindex time on large datasets freezes UI.
  - **Mitigation:** Keep operation manual and explicit via confirmation dialog. Synchronous blocking is accepted technical debt; future path is Relm4 `Worker` or `CommandOutput` for async indexing.
- **Risk:** `adw::AlertDialog` uses string response IDs (not `gtk::ResponseType`).
  - **Mitigation:** Implementation uses `dialog.add_response("confirm", ...)` / `dialog.connect_response(None, ...)` pattern per libadwaita API.

## Definition of done

- `--sessions-dir` influences all tools consistently.
- Override mode never reads tool paths from `HOME`.
- Override/default indexes are isolated (no stale cross-mode contamination).
- `SessionSources` is stored in `App` and available for reindexing.
- Preferences includes a working reset-and-reindex action.
- PreferencesDialog uses controller-with-outputs pattern (not fire-and-forget).
- `cargo fmt --all`, `cargo test`, and `cargo clippy` pass.
