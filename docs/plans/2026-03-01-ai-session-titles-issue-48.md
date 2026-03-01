# AI Session Titles (Issue #48) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a dedicated nullable `Session.title` field that preserves native source titles and can optionally generate missing titles safely during indexing.

**Architecture:** Keep parser output and DB schema as the source of truth, then run a bounded title-generation pass inside the existing `IndexingWorker` after indexing. Preserve existing generated titles across reindex runs, while allowing native parser titles to override generated ones. Read title-generation settings when dispatching indexing work so preference changes apply immediately without restart.

**Tech Stack:** Rust 2024, Relm4 worker thread model, rusqlite + SQLite migrations, gio::Settings, std::process + Flatpak host execution, optional `wait-timeout` crate.

---

## V2 decision updates

1. Keep `IndexingWorker::Init = PathBuf` and pass `TitleGenerationConfig` in worker input messages, not init payload.
2. Replace `INSERT OR REPLACE` session writes with `INSERT ... ON CONFLICT(id) DO UPDATE` and preserve prior `sessions.title` when parser emits no native title.
3. Keep `IndexingOutcome { stats, indexed_session_ids }`, but add backlog fill so `MAX_TITLE_GENERATIONS_PER_RUN = 25` does not permanently skip old untitled sessions.
4. In `auto` provider mode, use ordered fallback `OpenCode -> Claude` per generation attempt when the first provider fails at runtime.
5. In `auto` provider mode, use default models `opencode/gpt-5-nano` then Claude Haiku when no provider-specific override is set.
6. Keep all failures non-fatal and keep worker completion behavior unchanged (`Completed` unless indexing fails).

## Scope

In scope:
- Add `Session.title` to model + SQLite schema v5
- Parse native titles for OpenCode and Claude Code summary events
- Add global settings (`enabled`, `provider`, `model`)
- Auto-detect host CLIs in native and Flatpak
- Generate titles in background worker after indexing
- UI title precedence `title -> first_prompt -> project fallback`

Out of scope:
- Per-tool/per-workspace policies
- Manual title regeneration action
- New providers (Codex, Vibe, Ollama)
- Async runtime introduction

## Task-by-task implementation (TDD, small commits)

### Task 1: Add `Session.title` and schema v5 migration

**Files:**
- Modify: `src/models/session.rs`
- Modify: `src/database/schema.rs`
- Test: `src/database/schema.rs`

**Step 1: Write the failing tests**

Add tests in `src/database/schema.rs`:

```rust
#[test]
fn v4_to_v5_migration_adds_title_column() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "
        CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            tool TEXT NOT NULL,
            project_path TEXT,
            start_time INTEGER NOT NULL,
            message_count INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            last_updated INTEGER NOT NULL,
            first_prompt TEXT,
            parent_session_id TEXT,
            is_subagent INTEGER NOT NULL DEFAULT 0,
            input_tokens INTEGER,
            output_tokens INTEGER,
            cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,
            reasoning_tokens INTEGER
        );
        PRAGMA user_version = 4;
        ",
    )
    .unwrap();

    initialize_database(&conn).unwrap();

    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 5);

    let mut stmt = conn.prepare("PRAGMA table_info(sessions)").unwrap();
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(columns.contains(&"title".to_string()));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test v4_to_v5_migration_adds_title_column -- --exact`  
Expected: FAIL because user_version is still 4 and `title` column does not exist.

**Step 3: Write minimal implementation**

Add model field in `src/models/session.rs`:

```rust
#[serde(default)]
pub title: Option<String>,
```

Add migration in `src/database/schema.rs`:

```rust
fn apply_v5_migration(conn: &Connection) -> Result<()> {
    match conn.execute("ALTER TABLE sessions ADD COLUMN title TEXT", []) {
        Ok(_) => {}
        Err(e) if e.to_string().contains("duplicate column name") => {}
        Err(e) => return Err(e.into()),
    }
    conn.execute_batch("PRAGMA user_version = 5")?;
    Ok(())
}
```

And call it from `initialize_database` when `version < 5`.

**Step 4: Run test to verify it passes**

Run: `cargo test v4_to_v5_migration_adds_title_column -- --exact`  
Expected: PASS.

**Step 5: Commit**

```bash
git add src/models/session.rs src/database/schema.rs
git commit -m "feat: add session title model field and v5 schema migration"
```

### Task 2: Update DB reads/writes and preserve titles on reindex

**Files:**
- Modify: `src/database/mod.rs`
- Modify: `src/database/indexer.rs`
- Test: `src/database/indexer.rs`

**Step 1: Write failing tests**

Add tests in `src/database/indexer.rs`:

```rust
#[test]
fn reindex_preserves_existing_title_when_parser_has_none() {
    // Seed existing session with generated title
    // Reinsert parsed session with same id and session.title = None
    // Assert title remains unchanged
}

#[test]
fn native_parser_title_overrides_existing_generated_title() {
    // Seed existing session with generated title
    // Reinsert parsed session with same id and session.title = Some("Native")
    // Assert title becomes "Native"
}
```

**Step 2: Run tests to verify they fail**

Run:
- `cargo test reindex_preserves_existing_title_when_parser_has_none -- --exact`
- `cargo test native_parser_title_overrides_existing_generated_title -- --exact`

Expected: FAIL because current `INSERT OR REPLACE` loses old `title`.

**Step 3: Write minimal implementation**

In `src/database/indexer.rs`, replace `INSERT OR REPLACE` with conflict-safe update:

```sql
INSERT INTO sessions (
  id, tool, project_path, start_time, message_count, file_path, last_updated,
  first_prompt, title, parent_session_id, is_subagent,
  input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
ON CONFLICT(id) DO UPDATE SET
  tool = excluded.tool,
  project_path = excluded.project_path,
  start_time = excluded.start_time,
  message_count = excluded.message_count,
  file_path = excluded.file_path,
  last_updated = excluded.last_updated,
  first_prompt = excluded.first_prompt,
  title = COALESCE(excluded.title, sessions.title),
  parent_session_id = excluded.parent_session_id,
  is_subagent = excluded.is_subagent,
  input_tokens = excluded.input_tokens,
  output_tokens = excluded.output_tokens,
  cache_read_tokens = excluded.cache_read_tokens,
  cache_write_tokens = excluded.cache_write_tokens,
  reasoning_tokens = excluded.reasoning_tokens
```

Update all `SELECT ... FROM sessions` projections in `src/database/mod.rs` to include `title`, and map it in `session_from_row`.

**Step 4: Run tests to verify they pass**

Run:
- `cargo test reindex_preserves_existing_title_when_parser_has_none -- --exact`
- `cargo test native_parser_title_overrides_existing_generated_title -- --exact`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/database/mod.rs src/database/indexer.rs
git commit -m "fix: preserve existing session titles across reindex updates"
```

### Task 3: Parse native titles from OpenCode and Claude

**Files:**
- Modify: `src/parsers/opencode/mod.rs`
- Modify: `src/parsers/claude_code.rs`
- Test: `src/parsers/opencode/mod.rs`
- Test: `src/parsers/claude_code.rs`
- Test: `src/database/indexer.rs` (update existing OpenCode expectation)

**Step 1: Write failing tests**

Add/adjust tests:

```rust
#[test]
fn opencode_metadata_title_maps_to_session_title_not_first_prompt() {
    // metadata.title exists
    // first user message is "First"
    // assert session.title == Some(metadata title)
    // assert session.first_prompt == Some("First")
}

#[test]
fn claude_summary_event_sets_session_title_latest_non_empty() {
    // include two summary events, second non-empty should win
    // assert session.title is latest non-empty summary
}
```

Update `opencode_dual_read_prefers_sqlite_over_json` in `src/database/indexer.rs` to assert `title` rather than `first_prompt` for SQLite metadata title.

**Step 2: Run tests to verify they fail**

Run:
- `cargo test opencode_metadata_title_maps_to_session_title_not_first_prompt -- --exact`
- `cargo test claude_summary_event_sets_session_title_latest_non_empty -- --exact`
- `cargo test opencode_dual_read_prefers_sqlite_over_json -- --exact`

Expected: FAIL with current parser behavior.

**Step 3: Write minimal implementation**

In `src/parsers/opencode/mod.rs`:

```rust
let title = metadata.title.clone().filter(|t| !t.trim().is_empty());
let first_prompt = crate::parsers::extract_first_prompt(&flattened);

let session = Session {
    // ...
    first_prompt,
    title,
    // ...
};
```

In `src/parsers/claude_code.rs`, track summary events during line iteration:

```rust
let mut latest_summary: Option<String> = None;

match event_type {
    Some("summary") => {
        if let Some(s) = event.get("summary").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                latest_summary = Some(s.to_string());
            }
        }
    }
    // existing arms...
}
```

Then write `title: latest_summary` when building `Session`.

**Step 4: Run tests to verify they pass**

Run:
- `cargo test opencode_metadata_title_maps_to_session_title_not_first_prompt -- --exact`
- `cargo test claude_summary_event_sets_session_title_latest_non_empty -- --exact`
- `cargo test opencode_dual_read_prefers_sqlite_over_json -- --exact`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/parsers/opencode/mod.rs src/parsers/claude_code.rs src/database/indexer.rs
git commit -m "feat: preserve native session titles from OpenCode metadata and Claude summaries"
```

### Task 4: Introduce `IndexingOutcome` and collect indexed session ids

**Files:**
- Modify: `src/database/indexer.rs`
- Modify: `src/database/mod.rs`
- Test: `src/database/indexer.rs`

**Step 1: Write failing test**

Add test:

```rust
#[test]
fn index_all_incremental_returns_indexed_session_ids() {
    // run on fixtures
    // assert outcome.stats.indexed > 0
    // assert !outcome.indexed_session_ids.is_empty()
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test index_all_incremental_returns_indexed_session_ids -- --exact`  
Expected: FAIL because API currently returns `IndexingStats` only.

**Step 3: Write minimal implementation**

Add type in `src/database/indexer.rs`:

```rust
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexingOutcome {
    pub stats: IndexingStats,
    pub indexed_session_ids: Vec<String>,
}
```

Update `index_all_incremental` and `index_all_full_reindex` to return `IndexingOutcome`, and collect IDs on every successful session insert.

Re-export from `src/database/mod.rs`:

```rust
pub use indexer::{IndexingOutcome, IndexingStats, SessionIndexer};
```

**Step 4: Run test to verify it passes**

Run: `cargo test index_all_incremental_returns_indexed_session_ids -- --exact`  
Expected: PASS.

**Step 5: Commit**

```bash
git add src/database/indexer.rs src/database/mod.rs
git commit -m "feat: return indexed session ids from indexing runs"
```

### Task 5: Build title generation utility module

**Files:**
- Create: `src/utils/title_generator.rs`
- Modify: `src/utils/mod.rs`
- Modify: `Cargo.toml` (if using `wait-timeout`)
- Test: `src/utils/title_generator.rs`

**Step 1: Write failing tests**

Add tests for:
- provider parsing and auto resolution order (`OpenCode` before `Claude`)
- auto-mode default model mapping (`opencode/gpt-5-nano` then Haiku)
- Flatpak host wrapping behavior
- command argument mapping with/without model override
- sanitization and max 50 char truncation
- timeout path returns `None`

Example test scaffold:

```rust
#[test]
fn sanitize_generated_title_takes_first_non_empty_line_and_truncates() {
    let raw = "\n  \"Very long title ...\"\nignored";
    let title = sanitize_generated_title(raw).unwrap();
    assert!(title.chars().count() <= 50);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test sanitize_generated_title_takes_first_non_empty_line_and_truncates -- --exact`  
Expected: FAIL because module does not exist.

**Step 3: Write minimal implementation**

Create `src/utils/title_generator.rs` with:

```rust
pub const MAX_TITLE_CHARS: usize = 50;
pub const TITLE_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleGenerationConfig {
    pub enabled: bool,
    pub provider: TitleProvider,
    pub model_override: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleProvider {
    Auto,
    Claude,
    OpenCode,
}
```

Implement:
- `detect_available_provider()` using `which` or `flatpak-spawn --host which`
- `resolve_provider_chain(config)` so `Auto` returns `[OpenCode, Claude]` filtered by availability
- `default_model_for(provider)` so auto mode defaults to `opencode/gpt-5-nano` for OpenCode and Haiku for Claude
- `generate_title(context, config)` with runtime fallback for auto mode
- sanitize pipeline exactly: first non-empty line, trim outer quotes, collapse spaces, safe truncation, reject empty

Command contracts:
- OpenCode: `opencode run <prompt> --format default --model opencode/gpt-5-nano` in auto mode by default, otherwise `[--model <m>]` when explicitly configured
- Claude: `claude -p <prompt> --output-format text --permission-mode plan --tools "" --model <haiku-or-override>` as fallback in auto mode

Timeout behavior:
- hard timeout 30s
- on timeout: kill process, wait, return `None`
- on non-zero exit or empty stdout: return `None`

If `wait-timeout` is used, add to `Cargo.toml`:

```toml
wait-timeout = "0.2"
```

**Step 4: Run tests to verify they pass**

Run: `cargo test title_generator -- --nocapture`  
Expected: PASS for all new module tests.

**Step 5: Commit**

```bash
git add src/utils/title_generator.rs src/utils/mod.rs Cargo.toml
git commit -m "feat: add provider detection and CLI-based title generation utility"
```

### Task 6: Add DB helpers for candidate loading and title updates

**Files:**
- Modify: `src/database/indexer.rs`
- Test: `src/database/indexer.rs`

**Step 1: Write failing tests**

Add tests:

```rust
#[test]
fn update_session_title_updates_only_target_session() {
    // seed two sessions
    // update one id
    // assert only target changed
}

#[test]
fn load_title_backlog_candidates_excludes_subagents_and_existing_titles() {
    // seed mixed rows
    // assert only eligible rows returned in last_updated desc order
}
```

**Step 2: Run tests to verify they fail**

Run:
- `cargo test update_session_title_updates_only_target_session -- --exact`
- `cargo test load_title_backlog_candidates_excludes_subagents_and_existing_titles -- --exact`

Expected: FAIL because helpers do not exist.

**Step 3: Write minimal implementation**

Add in `src/database/indexer.rs`:

```rust
pub fn update_session_title(&self, session_id: &str, title: &str) -> Result<()> { /* ... */ }

pub fn load_title_candidate(&self, session_id: &str) -> Result<Option<TitleCandidate>> { /* ... */ }

pub fn load_title_backlog_candidates(&self, limit: usize) -> Result<Vec<TitleCandidate>> { /* ... */ }
```

Backlog SQL constraints:

```sql
WHERE is_subagent = 0
  AND title IS NULL
  AND first_prompt IS NOT NULL
  AND trim(first_prompt) <> ''
ORDER BY last_updated DESC
LIMIT ?
```

**Step 4: Run tests to verify they pass**

Run:
- `cargo test update_session_title_updates_only_target_session -- --exact`
- `cargo test load_title_backlog_candidates_excludes_subagents_and_existing_titles -- --exact`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/database/indexer.rs
git commit -m "feat: add session title update and candidate query helpers"
```

### Task 7: Wire worker pipeline with cap + backlog fill

**Files:**
- Modify: `src/indexing_worker.rs`
- Modify: `src/database/indexer.rs`
- Test: `src/indexing_worker.rs` (or pure helper tests)

**Step 1: Write failing tests**

Add pure helper tests for selection logic:

```rust
#[test]
fn candidate_selection_prioritizes_indexed_ids_then_backlog_until_cap() {
    // given indexed ids and backlog
    // ensure order + cap behavior
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test candidate_selection_prioritizes_indexed_ids_then_backlog_until_cap -- --exact`  
Expected: FAIL because helper logic does not exist.

**Step 3: Write minimal implementation**

In `src/indexing_worker.rs`:

- Keep `type Init = PathBuf`.
- Change input payload:

```rust
pub struct IndexingRequest {
    pub sources: SessionSources,
    pub title_generation: TitleGenerationConfig,
}
```

- Use `IndexingOutcome` from indexer.
- Generation pipeline:
  1. if disabled, return completion immediately
  2. attempt eligible IDs from `outcome.indexed_session_ids`
  3. if attempts < 25, fill from backlog query
  4. ignore generation failures and continue

**Step 4: Run tests to verify they pass**

Run: `cargo test indexing_worker -- --nocapture`  
Expected: PASS.

**Step 5: Commit**

```bash
git add src/indexing_worker.rs src/database/indexer.rs
git commit -m "feat: run bounded title generation after indexing with backlog fill"
```

### Task 8: Read settings per dispatch in app (no restart required)

**Files:**
- Modify: `src/app.rs`
- Test: `src/app.rs` (pure mapping tests)

**Step 1: Write failing tests**

Add tests for provider parsing defaults:

```rust
#[test]
fn parse_title_provider_defaults_to_auto_on_invalid_value() {
    assert_eq!(parse_title_provider("invalid"), TitleProvider::Auto);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test parse_title_provider_defaults_to_auto_on_invalid_value -- --exact`  
Expected: FAIL because helper does not exist.

**Step 3: Write minimal implementation**

In `src/app.rs`:
- add helper that reads:
  - `ai-title-generation-enabled`
  - `ai-title-generation-provider`
  - `ai-title-generation-model`
- trim empty model to `None`
- keep auto behavior priority as `OpenCode -> Claude` with defaults `opencode/gpt-5-nano -> Haiku`
- emit indexing worker messages with fresh config each time:
  - startup incremental
  - full reindex from preferences

**Step 4: Run test to verify it passes**

Run: `cargo test parse_title_provider_defaults_to_auto_on_invalid_value -- --exact`  
Expected: PASS.

**Step 5: Commit**

```bash
git add src/app.rs
git commit -m "fix: read title generation settings at indexing dispatch time"
```

### Task 9: Add GSettings keys and Preferences UI controls

**Files:**
- Modify: `data/io.github.supermaciz.sessionschronicle.gschema.xml.in`
- Modify: `src/ui/modals/preferences.rs`
- Test: `src/ui/modals/preferences.rs` (pure helpers)

**Step 1: Write failing tests**

Add pure tests in preferences module:

```rust
#[test]
fn provider_index_mapping_round_trips() {
    // Auto <-> 0, OpenCode <-> 1, Claude <-> 2
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test provider_index_mapping_round_trips -- --exact`  
Expected: FAIL because mapping helpers are missing.

**Step 3: Write minimal implementation**

Add GSettings keys:

```xml
<key name="ai-title-generation-enabled" type="b">
  <default>false</default>
</key>
<key name="ai-title-generation-provider" type="s">
  <default>"auto"</default>
</key>
<key name="ai-title-generation-model" type="s">
  <default>""</default>
</key>
```

Add Preferences group "AI Session Titles" with:
- `adw::SwitchRow` bound to enabled key
- `adw::ComboRow` for `Auto`, `OpenCode`, `Claude`
- `adw::EntryRow` for optional model override
- auto subtitle from runtime detection:
  - `Auto (OpenCode detected)`
  - `Auto (Claude detected)`
  - `Auto (No CLI detected)`

**Step 4: Run test to verify it passes**

Run: `cargo test provider_index_mapping_round_trips -- --exact`  
Expected: PASS.

**Step 5: Commit**

```bash
git add data/io.github.supermaciz.sessionschronicle.gschema.xml.in src/ui/modals/preferences.rs
git commit -m "feat: add preferences controls for AI session title generation"
```

### Task 10: Update session row precedence and subtitle consistency

**Files:**
- Modify: `src/ui/session_row.rs`
- Test: `src/ui/session_row.rs`

**Step 1: Write failing tests**

Add tests:

```rust
#[test]
fn session_title_uses_title_before_first_prompt() {
    // title + first_prompt both present
    // assert title wins
}

#[test]
fn session_subtitle_uses_project_name_when_title_present_without_prompt() {
    // title present, first_prompt none
    // assert subtitle location is project name
}
```

**Step 2: Run tests to verify they fail**

Run:
- `cargo test session_title_uses_title_before_first_prompt -- --exact`
- `cargo test session_subtitle_uses_project_name_when_title_present_without_prompt -- --exact`

Expected: FAIL with current first_prompt-only logic.

**Step 3: Write minimal implementation**

In `src/ui/session_row.rs`:
- Title precedence:

```rust
session.title
    .as_deref()
    .map(str::trim)
    .filter(|v| !v.is_empty())
    .map(str::to_string)
    .or_else(|| {
        session
            .first_prompt
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    })
    .unwrap_or_else(|| Self::project_name(session).unwrap_or_else(|| "Unknown project".to_string()))
```

- Subtitle location uses project name when either `title` or `first_prompt` is present.

**Step 4: Run tests to verify they pass**

Run: `cargo test session_row -- --nocapture`  
Expected: PASS.

**Step 5: Commit**

```bash
git add src/ui/session_row.rs
git commit -m "feat: display session title with title-first precedence"
```

### Task 11: Cross-cutting test updates and docs

**Files:**
- Modify: `tests/load_session.rs`
- Modify: `README.md`
- Modify: `docs/DEVELOPMENT_WORKFLOW.md` (if title behavior documented there)

**Step 1: Write failing integration test**

Add test in `tests/load_session.rs`:

```rust
#[test]
fn load_session_returns_title_when_present() {
    // insert session row with title
    // assert loaded session.title == Some(...)
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test load_session_returns_title_when_present -- --exact`  
Expected: FAIL until `SELECT`/mapping includes title everywhere.

**Step 3: Write minimal implementation**

Update inserts/projections in `tests/load_session.rs` to include `title` column where needed, and document in README:
- precedence `title -> first_prompt -> project`
- generation disabled by default
- provider auto detection behavior and priority `OpenCode (opencode/gpt-5-nano) -> Claude (Haiku)`

**Step 4: Run test to verify it passes**

Run: `cargo test load_session_returns_title_when_present -- --exact`  
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/load_session.rs README.md docs/DEVELOPMENT_WORKFLOW.md
git commit -m "test: cover session title loading and document title precedence"
```

## Final verification gate (@superpowers:verification-before-completion)

Run in order:

1. `cargo fmt --all -- --check`  
   Expected: no formatting diffs.
2. `cargo clippy --all -- -D warnings`  
   Expected: zero warnings.
3. `cargo test --all --no-fail-fast`  
   Expected: all tests pass.
4. Manual Flatpak sanity:
   - `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures`
   - Verify preferences controls, auto detection subtitle, default disabled behavior.

## Risks and mitigations (V2)

| Risk | Mitigation |
|------|------------|
| Config changes not applied until restart | Read title-generation settings each indexing dispatch in `app.rs`, do not store config in worker init |
| Reindex wipes generated titles | Use `ON CONFLICT(id) DO UPDATE` with `title = COALESCE(excluded.title, sessions.title)` |
| 25-cap leaves stale untitled sessions forever | Process indexed IDs first, then backlog candidates until cap |
| Auto mode picks unavailable authenticated provider | In `Auto`, attempt providers in priority order `OpenCode -> Claude` and fallback on runtime failure |
| Flatpak cannot see host CLIs | Use `flatpak-spawn --host` for both detection and execution |
| Potential `wait-timeout` SIGCHLD caveat | Keep usage isolated; if instability appears, switch to `try_wait` polling loop with explicit deadline |

## Assumptions

- Generation only happens during indexing worker runs.
- Subagent sessions are never title-generated.
- All generation failures are non-fatal and logged.
- Existing sessions are backfilled gradually via backlog fill, not via one-shot migration.
