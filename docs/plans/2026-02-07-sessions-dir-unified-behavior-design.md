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

### Task 2: Wire app startup to unified sources and DB isolation

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
- Index using resolved paths only.
- Keep existing per-tool log lines, but log resolved source path(s).

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

### Task 4: Expose reset action in Preferences

**Files:**
- Modify: `src/ui/modals/preferences.rs`
- Modify: `src/ui/modals/mod.rs` (if type exports change)
- Modify: `src/app.rs`
- Modify: `data/io.github.supermaciz.sessionschronicle.gschema.xml.in` (only if adding preference key)
- Test: `src/ui/modals/preferences.rs` (or integration tests where feasible)

**Step 1: Write the failing test/spec**

Define message contract first:

```rust
pub enum PreferencesOutput {
    ReindexRequested,
}
```

and in app:

```rust
enum AppMsg {
    // ...
    ReindexRequested,
}
```

Add basic test(s) for signal wiring where possible, or a compile-time flow test that the forwarding compiles and triggers path.

**Step 2: Run test to verify it fails**

Run: `cargo test preferences -- --nocapture`

Expected: FAIL or compile error before implementation.

**Step 3: Write minimal implementation**

In Preferences dialog:
- Add an "Advanced" group.
- Add action row/button labeled `Reset session index`.
- On click: show confirmation (`adw::AlertDialog`).
- If confirmed: emit `PreferencesOutput::ReindexRequested`.

In app:
- Launch preferences as a controller connected to outputs (instead of detached fire-and-forget).
- Handle `AppMsg::ReindexRequested`:
  1. Open `SessionIndexer` for active DB.
  2. Call `clear_all_sessions()`.
  3. Re-run indexing with current `SessionSources`.
  4. Ask `SessionList` to reload.
  5. Show success/failure toast.

Keep behavior idempotent and non-blocking enough for UI (short synchronous action acceptable for now).

**Step 4: Run tests to verify pass**

Run: `cargo test preferences -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/ui/modals/preferences.rs src/app.rs src/ui/modals/mod.rs
git commit -m "feat: add preferences action to reset and rebuild index"
```

### Task 5: Refresh session list after reindex

**Files:**
- Modify: `src/ui/session_list.rs`
- Modify: `src/app.rs`
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
- In `AppMsg::ReindexRequested` success path, emit `SessionListMsg::Reload`.

**Step 4: Run tests to verify pass**

Run: `cargo test session_list -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/ui/session_list.rs src/app.rs
git commit -m "feat: reload session list after index rebuild"
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

**Step 6: Commit**

```bash
git add docs/DEVELOPMENT_WORKFLOW.md docs/PROJECT_STATUS.md
git commit -m "docs: describe unified sessions-dir behavior and reset flow"
```

## Rollback strategy

- Safe rollback path: remove resolver wiring and revert to existing per-tool defaults in `App::init`.
- Keep `clear_all_sessions` even if UI action is rolled back (useful maintenance primitive).
- If UI wiring causes regressions, disable only the Preferences button while preserving CLI behavior improvements.

## Risks and mitigations

- **Risk:** OpenCode override root confusion (session subpath expectations).
  - **Mitigation:** Resolver chooses `opencode_storage` when present; fallback root keeps flexibility.
- **Risk:** Users surprised by two DB files.
  - **Mitigation:** Document clearly; expose reset action and log active DB file at startup.
- **Risk:** Long reindex time on large datasets.
  - **Mitigation:** Keep operation manual and explicit via confirmation dialog.

## Definition of done

- `--sessions-dir` influences all tools consistently.
- Override mode never reads tool paths from `HOME`.
- Override/default indexes are isolated (no stale cross-mode contamination).
- Preferences includes a working reset-and-reindex action.
- `cargo fmt --all`, `cargo test`, and `cargo clippy` pass.
