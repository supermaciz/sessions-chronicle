# Startup Performance (Issue #59) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make startup non-blocking by loading cached sessions immediately, then indexing in the background with fingerprint-based incremental skips and safer pruning.

**Architecture:** Add schema v4 (`file_fingerprints`) plus incremental logic inside `SessionIndexer` so unchanged files are skipped and only changed sessions are rewritten. Move startup and manual reindex work into a dedicated Relm4 `Worker` so `App::init` only resolves paths and initializes UI. Surface indexing state to users with a header spinner and `SessionList` first-launch placeholder.

**Tech Stack:** Rust 2024, Relm4 0.10 (`Worker`, `WorkerController`), rusqlite (WAL + busy timeout), GTK4/libadwaita, fixture-driven tests.

---

### Task 1: Add schema v4 for file fingerprints

**Files:**
- Modify: `src/database/schema.rs`
- Test: `src/database/schema.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn fresh_db_initializes_to_v4() {
    let conn = Connection::open_in_memory().unwrap();
    initialize_database(&conn).unwrap();

    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 4);

    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='file_fingerprints'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(table_exists, 1);
}

#[test]
fn v3_to_v4_migration_creates_file_fingerprints_table() {
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
        PRAGMA user_version = 3;
        ",
    )
    .unwrap();

    initialize_database(&conn).unwrap();

    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 4);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --all database::schema::tests`
Expected: FAIL with version assertions still expecting v3 behavior.

**Step 3: Write minimal implementation**

```rust
pub fn initialize_database(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if version < 1 {
        apply_v1_migration(conn)?;
    }
    if version < 2 {
        apply_v2_migration(conn)?;
    }
    if version < 3 {
        apply_v3_migration(conn)?;
    }
    if version < 4 {
        apply_v4_migration(conn)?;
    }

    Ok(())
}

fn apply_v4_migration(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS file_fingerprints (
            file_path TEXT PRIMARY KEY,
            mtime_ns INTEGER NOT NULL,
            size INTEGER NOT NULL
        )",
        [],
    )?;
    conn.execute_batch("PRAGMA user_version = 4")?;
    Ok(())
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --all database::schema::tests`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/database/schema.rs
git commit -m "feat: add v4 fingerprint schema migration"
```

---

### Task 2: Configure SQLite WAL and busy timeout for app DB connections

**Files:**
- Modify: `src/database/indexer.rs`
- Modify: `src/database/mod.rs`
- Test: `src/database/indexer.rs`
- Test: `src/database/mod.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn new_indexer_configures_wal_and_busy_timeout() {
    let temp_db = NamedTempFile::new().unwrap();
    let indexer = SessionIndexer::new(temp_db.path()).unwrap();

    let journal_mode: String = indexer
        .db
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

    let busy_timeout_ms: i64 = indexer
        .db
        .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
        .unwrap();
    assert_eq!(busy_timeout_ms, 5_000);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --all new_indexer_configures_wal_and_busy_timeout`
Expected: FAIL because `journal_mode` and timeout are not configured.

**Step 3: Write minimal implementation**

```rust
const SQLITE_BUSY_TIMEOUT_SECS: u64 = 5;

pub(crate) fn open_read_connection(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path).context("Failed to open database")?;
    conn.busy_timeout(std::time::Duration::from_secs(SQLITE_BUSY_TIMEOUT_SECS))?;
    Ok(conn)
}

impl SessionIndexer {
    pub fn new(db_path: &Path) -> Result<Self> {
        let db = crate::database::open_read_connection(db_path)?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        crate::database::schema::initialize_database(&db)?;
        Ok(Self { db })
    }
}
```

Then replace direct `Connection::open(...)` calls in `src/database/mod.rs` read paths with `open_read_connection(...)`.

**Step 4: Run test to verify it passes**

Run: `cargo test --all new_indexer_configures_wal_and_busy_timeout`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/database/indexer.rs src/database/mod.rs
git commit -m "feat: configure sqlite wal and busy timeout for app db"
```

---

### Task 3: Add fingerprint helpers and skip checks to SessionIndexer

**Files:**
- Modify: `src/database/indexer.rs`
- Test: `src/database/indexer.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn should_reindex_uses_mtime_and_size_fingerprint() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let session_file = dir.path().join("session.jsonl");

    std::fs::write(&session_file, "{\"type\":\"message\"}\n").unwrap();

    assert!(indexer.should_reindex(&session_file).unwrap());
    indexer.upsert_fingerprint_for_file(&session_file).unwrap();
    assert!(!indexer.should_reindex(&session_file).unwrap());

    // Rewrite same bytes to force mtime update.
    std::fs::write(&session_file, "{\"type\":\"message\"}\n").unwrap();
    assert!(indexer.should_reindex(&session_file).unwrap());
}

#[test]
fn prune_orphan_fingerprints_removes_missing_paths() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let session_file = dir.path().join("session.jsonl");

    std::fs::write(&session_file, "{}\n").unwrap();
    indexer.upsert_fingerprint_for_file(&session_file).unwrap();
    std::fs::remove_file(&session_file).unwrap();

    let removed = indexer.prune_orphan_fingerprints().unwrap();
    assert_eq!(removed, 1);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --all should_reindex_uses_mtime_and_size_fingerprint`
Expected: FAIL (methods do not exist yet).

**Step 3: Write minimal implementation**

```rust
fn get_fingerprint(&self, file_path: &Path) -> Result<Option<(i64, i64)>> { /* SELECT ... */ }

fn current_fingerprint(file_path: &Path) -> Result<(i64, i64)> { /* metadata + mtime_ns + size */ }

fn should_reindex(&self, file_path: &Path) -> Result<bool> {
    let (mtime_ns, size) = Self::current_fingerprint(file_path)?;
    match self.get_fingerprint(file_path)? {
        Some((stored_mtime_ns, stored_size)) if stored_mtime_ns == mtime_ns && stored_size == size => Ok(false),
        _ => Ok(true),
    }
}

fn upsert_fingerprint_tx(tx: &rusqlite::Transaction<'_>, file_path: &Path) -> Result<()> { /* INSERT OR REPLACE */ }

fn upsert_fingerprint_for_file(&mut self, file_path: &Path) -> Result<()> { /* transaction wrapper */ }

fn prune_orphan_fingerprints(&mut self) -> Result<usize> { /* delete fingerprints where path no longer exists */ }
```

**Step 4: Run test to verify it passes**

Run: `cargo test --all should_reindex_uses_mtime_and_size_fingerprint`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/database/indexer.rs
git commit -m "feat: add fingerprint helpers for incremental indexing"
```

---

### Task 4: Add incremental skip mode for Claude and Codex parsers

**Files:**
- Modify: `src/database/indexer.rs`
- Test: `src/database/indexer.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn claude_incremental_skips_unchanged_files() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let sessions_dir = PathBuf::from("tests/fixtures/claude_sessions");

    let first = indexer.index_claude_sessions_incremental(&sessions_dir).unwrap();
    assert!(first.indexed > 0);

    let second = indexer.index_claude_sessions_incremental(&sessions_dir).unwrap();
    assert_eq!(second.indexed, 0);
    assert!(second.skipped > 0);
}

#[test]
fn codex_incremental_skips_unchanged_rollouts() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let sessions_dir = PathBuf::from("tests/fixtures/codex_sessions");

    let first = indexer.index_codex_sessions_incremental(&sessions_dir).unwrap();
    assert!(first.indexed > 0);

    let second = indexer.index_codex_sessions_incremental(&sessions_dir).unwrap();
    assert_eq!(second.indexed, 0);
    assert!(second.skipped > 0);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --all claude_incremental_skips_unchanged_files`
Expected: FAIL because incremental APIs are missing.

**Step 3: Write minimal implementation**

```rust
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IndexingStats {
    pub indexed: usize,
    pub skipped: usize,
}

pub fn index_claude_sessions_incremental(&mut self, sessions_dir: &Path) -> Result<IndexingStats> {
    self.index_claude_sessions_internal(sessions_dir, true)
}

fn index_claude_sessions_internal(&mut self, sessions_dir: &Path, incremental: bool) -> Result<IndexingStats> {
    let parser = ClaudeCodeParser;
    let mut stats = IndexingStats::default();

    for entry in walkdir::WalkDir::new(sessions_dir).max_depth(5).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if entry.file_type().is_file()
            && path.extension().is_some_and(|ext| ext == "jsonl")
            && !Self::is_sidechain_file(path, sessions_dir)
        {
            if incremental && !self.should_reindex(path)? {
                stats.skipped += 1;
                continue;
            }

            let parsed = parser.parse(path)?;
            self.insert_parsed_session_with_fingerprint(&parsed, path, path)?;
            stats.indexed += 1;
        }
    }

    Ok(stats)
}
```

Apply the same pattern for Codex (`rollout-*.jsonl`). Keep existing `index_claude_sessions` and `index_codex_sessions` as full-index wrappers to preserve current callers/tests.

**Step 4: Run test to verify it passes**

Run: `cargo test --all claude_incremental_skips_unchanged_files`
Run: `cargo test --all codex_incremental_skips_unchanged_rollouts`
Expected: PASS for both tests.

**Step 5: Commit**

```bash
git add src/database/indexer.rs
git commit -m "feat: add incremental skip mode for claude and codex"
```

---

### Task 5: Add incremental mode for Mistral Vibe and shared prune cleanup

**Files:**
- Modify: `src/database/indexer.rs`
- Test: `src/database/indexer.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn vibe_incremental_uses_messages_jsonl_fingerprint() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let sessions_dir = PathBuf::from("tests/fixtures/vibe_sessions");

    let first = indexer.index_vibe_sessions_incremental(&sessions_dir).unwrap();
    assert!(first.indexed > 0);

    let second = indexer.index_vibe_sessions_incremental(&sessions_dir).unwrap();
    assert_eq!(second.indexed, 0);
    assert!(second.skipped > 0);
}

#[test]
fn clear_all_sessions_also_clears_fingerprints() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let sessions_dir = PathBuf::from("tests/fixtures/claude_sessions");

    indexer.index_claude_sessions_incremental(&sessions_dir).unwrap();
    indexer.clear_all_sessions().unwrap();

    let fingerprint_count: i64 = indexer
        .db
        .query_row("SELECT COUNT(*) FROM file_fingerprints", [], |row| row.get(0))
        .unwrap();
    assert_eq!(fingerprint_count, 0);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --all vibe_incremental_uses_messages_jsonl_fingerprint`
Expected: FAIL due missing incremental Vibe path.

**Step 3: Write minimal implementation**

```rust
pub fn index_vibe_sessions_incremental(&mut self, sessions_dir: &Path) -> Result<IndexingStats> {
    let parser = MistralVibeParser;
    let mut stats = IndexingStats::default();

    for entry in std::fs::read_dir(sessions_dir)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }

        let fingerprint_target = path.join("messages.jsonl");
        if !fingerprint_target.exists() || !path.join("meta.json").exists() {
            continue;
        }

        if !self.should_reindex(&fingerprint_target)? {
            stats.skipped += 1;
            continue;
        }

        let parsed = parser.parse(&path)?;
        self.insert_parsed_session_with_fingerprint(&parsed, &path, &fingerprint_target)?;
        stats.indexed += 1;
    }

    Ok(stats)
}

pub fn clear_all_sessions(&mut self) -> Result<()> {
    let tx = self.db.transaction()?;
    tx.execute("DELETE FROM transcript_items", [])?;
    tx.execute("DELETE FROM tool_calls", [])?;
    tx.execute("DELETE FROM subagents", [])?;
    tx.execute("DELETE FROM messages", [])?;
    tx.execute("DELETE FROM sessions", [])?;
    tx.execute("DELETE FROM file_fingerprints", [])?;
    tx.commit()?;
    Ok(())
}

fn prune_missing_file_backed_sessions(&mut self) -> Result<()> {
    // Select distinct sessions.file_path and delete rows where path no longer exists.
}
```

Call `prune_missing_file_backed_sessions()` and `prune_orphan_fingerprints()` at the end of each indexing run.

**Step 4: Run test to verify it passes**

Run: `cargo test --all vibe_incremental_uses_messages_jsonl_fingerprint`
Run: `cargo test --all clear_all_sessions_also_clears_fingerprints`
Expected: PASS for both tests.

**Step 5: Commit**

```bash
git add src/database/indexer.rs
git commit -m "feat: add vibe incremental indexing and fingerprint cleanup"
```

---

### Task 6: Implement OpenCode incremental skip and conditional ID prune

**Files:**
- Modify: `src/database/indexer.rs`
- Test: `src/database/indexer.rs`
- Modify: `src/session_sources.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn opencode_incremental_skip_keeps_sqlite_only_sessions() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let storage_root = PathBuf::from("tests/fixtures/opencode_storage");
    let db_path = storage_root.join("opencode.db");

    let first = indexer
        .index_opencode_sessions_incremental(&storage_root, Some(&db_path))
        .unwrap();
    assert!(first.indexed > 0);

    let second = indexer
        .index_opencode_sessions_incremental(&storage_root, Some(&db_path))
        .unwrap();
    assert!(second.skipped > 0);

    let sqlite_only_exists: bool = indexer
        .db
        .query_row(
            "SELECT COUNT(*) > 0 FROM sessions WHERE id = 'session-sqlite-only'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(sqlite_only_exists);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --all opencode_incremental_skip_keeps_sqlite_only_sessions`
Expected: FAIL because unchanged `opencode.db` is not skipped and/or stale prune behavior is incorrect.

**Step 3: Write minimal implementation**

```rust
pub fn index_opencode_sessions_incremental(
    &mut self,
    storage_root: &Path,
    db_path: Option<&Path>,
) -> Result<IndexingStats> {
    let parser = OpenCodeParser::new(storage_root);
    let mut stats = IndexingStats::default();
    let mut indexed_ids: HashSet<String> = HashSet::new();
    let mut sqlite_enumerated = false;

    if let Some(db_path) = db_path {
        if self.should_reindex(db_path)? {
            sqlite_enumerated = true;
            // enumerate sqlite backend, parse entries, insert sessions, add ids
            // upsert fingerprint on successful writes
        } else {
            stats.skipped += 1;
        }
    }

    // enumerate JSON backend
    // if unchanged JSON file: skipped += 1 and keep id in indexed_ids

    if sqlite_enumerated {
        self.prune_stale_opencode_sessions(&indexed_ids)?;
    }

    Ok(stats)
}
```

Also derive clone support for worker job payloads:

```rust
#[derive(Debug, Clone)]
pub struct SessionSources { ... }
```

**Step 4: Run test to verify it passes**

Run: `cargo test --all opencode_incremental_skip_keeps_sqlite_only_sessions`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/database/indexer.rs src/session_sources.rs
git commit -m "fix: skip opencode stale prune when sqlite backend is unchanged"
```

---

### Task 7: Move startup/reindex indexing to a dedicated Relm4 worker

**Files:**
- Create: `src/indexing_worker.rs`
- Modify: `src/main.rs`
- Modify: `src/app.rs`
- Test: `src/app.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn reindex_request_is_ignored_when_indexing_already_running() {
    assert_eq!(
        decide_reindex_action(true),
        ReindexAction::AlreadyRunning
    );
}

#[test]
fn reindex_request_starts_full_reindex_when_idle() {
    assert_eq!(
        decide_reindex_action(false),
        ReindexAction::StartFull
    );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --all reindex_request_is_ignored_when_indexing_already_running`
Expected: FAIL because helper and async flow are not implemented.

**Step 3: Write minimal implementation**

```rust
// src/indexing_worker.rs
pub struct IndexingWorker {
    db_path: PathBuf,
}

#[derive(Debug)]
pub enum IndexingWorkerInput {
    StartIncremental(SessionSources),
    StartFullReindex(SessionSources),
}

#[derive(Debug)]
pub enum IndexingWorkerOutput {
    Completed { indexed: usize, skipped: usize },
    Failed(String),
}

impl Worker for IndexingWorker {
    type Init = PathBuf;
    type Input = IndexingWorkerInput;
    type Output = IndexingWorkerOutput;

    fn init(init: Self::Init, _sender: ComponentSender<Self>) -> Self {
        Self { db_path: init }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        let result = (|| -> anyhow::Result<crate::database::indexer::IndexingStats> {
            let mut indexer = SessionIndexer::new(&self.db_path)?;
            match msg {
                IndexingWorkerInput::StartIncremental(sources) => {
                    indexer.index_all_incremental(&sources)
                }
                IndexingWorkerInput::StartFullReindex(sources) => {
                    indexer.index_all_full_reindex(&sources)
                }
            }
        })();

        match result {
            Ok(stats) => {
                sender
                    .output(IndexingWorkerOutput::Completed {
                        indexed: stats.indexed,
                        skipped: stats.skipped,
                    })
                    .ok();
            }
            Err(err) => {
                sender.output(IndexingWorkerOutput::Failed(err.to_string())).ok();
            }
        }
    }
}
```

```rust
// src/app.rs (key points)
indexing_worker: WorkerController<IndexingWorker>,
indexing: bool,

let indexing_worker = IndexingWorker::builder()
    .detach_worker(db_path.clone())
    .forward(sender.input_sender(), |output| match output {
        IndexingWorkerOutput::Completed { indexed, skipped } => {
            AppMsg::IndexingCompleted { indexed, skipped }
        }
        IndexingWorkerOutput::Failed(err) => AppMsg::IndexingFailed(err),
    });

// startup
self.indexing = true;
self.session_list.emit(SessionListMsg::SetIndexing(true));
self.indexing_worker
    .emit(IndexingWorkerInput::StartIncremental(self.sources.clone()));
```

Add `mod indexing_worker;` to `src/main.rs`.

Replace synchronous startup and reindex indexing in `App::init`/`AppMsg::ReindexRequested` with worker dispatch + single-flight guard.

**Step 4: Run test to verify it passes**

Run: `cargo test --all app::tests`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/indexing_worker.rs src/main.rs src/app.rs
git commit -m "feat: run indexing in a background relm4 worker"
```

---

### Task 8: Add indexing spinner and first-launch placeholder behavior

**Files:**
- Modify: `src/app.rs`
- Modify: `src/ui/session_list.rs`
- Test: `src/ui/session_list.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn empty_state_prefers_indexing_placeholder_when_loading_and_empty() {
    let state = compute_empty_state(
        true,  // sessions_empty
        "",    // search_query
        true,  // all_tools_selected
        true,  // indexing
    );

    assert_eq!(state.title, "Indexing sessions...");
    assert_eq!(state.description, "This may take a moment on first launch.");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --all empty_state_prefers_indexing_placeholder_when_loading_and_empty`
Expected: FAIL because indexing state is not modeled in `SessionList`.

**Step 3: Write minimal implementation**

```rust
// src/ui/session_list.rs
pub enum SessionListMsg {
    // ...existing
    SetIndexing(bool),
}

pub struct SessionList {
    // ...existing
    indexing: bool,
}

fn compute_empty_state(
    sessions_empty: bool,
    search_query: &str,
    all_tools_selected: bool,
    indexing: bool,
) -> EmptyStateCopy {
    if sessions_empty && indexing {
        return EmptyStateCopy {
            title: "Indexing sessions...",
            description: "This may take a moment on first launch.",
        };
    }

    if !search_query.trim().is_empty() {
        return EmptyStateCopy {
            title: "No sessions match search",
            description: "Try a different query or adjust filters",
        };
    }

    if all_tools_selected {
        EmptyStateCopy {
            title: "No Sessions Yet",
            description: "Your AI coding sessions will appear here",
        }
    } else {
        EmptyStateCopy {
            title: "No sessions match filters",
            description: "Try adjusting the tool filters in the sidebar",
        }
    }
}
```

```rust
// src/app.rs header bar
gtk::Spinner {
    set_tooltip_text: Some("Indexing sessions..."),
    #[watch]
    set_visible: model.indexing,
    #[watch]
    set_spinning: model.indexing,
}
```

Send `SessionListMsg::SetIndexing(true/false)` when worker jobs start/finish/fail.

**Step 4: Run test to verify it passes**

Run:
- `cargo test --all empty_state_prefers_indexing_placeholder_when_loading_and_empty`
- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`

Expected: PASS for all commands.

Manual app verification command:

`flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures`

Expected manual behavior:
- Cached sessions appear immediately on repeated launches.
- Header spinner is visible only while indexing.
- Empty DB + active indexing shows "Indexing sessions..." placeholder.
- Reindex while indexing shows toast: "Indexing already in progress."

**Step 5: Commit**

```bash
git add src/app.rs src/ui/session_list.rs
git commit -m "feat: show indexing state in header and empty session list"
```

---

## Final Validation Checklist

- Run `cargo fmt --all -- --check`
- Run `cargo clippy --all -- -D warnings`
- Run `cargo test --all --no-fail-fast`
- Run Flatpak fixture launch command and validate startup behavior manually
- Capture screenshots for spinner/placeholder UI changes before PR

## Suggested PR Description Bullets

- Move startup and manual reindex work off the UI thread using a Relm4 worker.
- Add schema v4 fingerprints and incremental skip logic to avoid reparsing unchanged session sources.
- Prevent accidental OpenCode stale-ID pruning when SQLite backend is skipped.
- Add visible indexing status (header spinner + first-launch placeholder) while keeping cached sessions available.
