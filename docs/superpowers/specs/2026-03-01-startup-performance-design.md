# Startup Performance Design (Issue #59)

**Status:** Implemented [#60](https://github.com/supermaciz/sessions-chronicle/pull/60)

## Problem

Startup is currently UI-blocking on large session datasets.
`App::init` does path resolution, opens the DB, parses all providers, and
writes everything before the first usable frame. On repeated launches, the app
still re-parses unchanged files and rewrites session data.

## Scope

This design addresses three axes:

1. **Non-blocking startup**: render session list from cached SQLite data
   immediately, then index in background.
2. **Incremental skip**: use file fingerprints (`mtime` + `size`) to skip
   unchanged inputs.
3. **Reduced write amplification**: only rewrite changed sessions; update
   fingerprints in the same transaction as session writes.

Out of scope:
- Phase 2 (lightweight list parse + on-demand full parse).
- Cross-process distributed indexing.

## Design constraints

- The app root component is a Relm4 `SimpleComponent` (`src/app.rs`).
  `spawn_oneshot_command` with typed command payloads is not available in the
  same way as for explicit `Component` command outputs.
- SQLite WAL supports concurrent readers + one writer, but it can still return
  `SQLITE_BUSY`; we need `busy_timeout` and a single-writer indexing flow.

## Approach

**Dedicated Relm4 Worker for indexing + fingerprint-based incremental indexing**

- Keep `App` as `SimpleComponent`.
- Add an `IndexingWorker` (`Worker` trait) to run indexing off the UI thread.
- Worker owns a separate SQLite connection and emits completion/failure back to
  `App`.
- App remains responsive and shows cached sessions immediately.
- Single-flight guard ensures only one indexing job runs at a time.

---

## 1. Schema: `file_fingerprints` table (migration v4)

```sql
CREATE TABLE file_fingerprints (
    file_path TEXT PRIMARY KEY,
    mtime_ns INTEGER NOT NULL,
    size INTEGER NOT NULL
);
```

Notes:
- Use nanosecond precision (`mtime_ns`) to avoid false negatives when files are
  updated multiple times within one second.
- Fingerprints are upserted only after successful parse + DB insert.

---

## 2. Startup flow (non-blocking)

```text
App::init:
  1. SessionSources::resolve()                      (fast path resolution)
  2. SessionIndexer::new(&db_path)                  (open DB + migrations)
  3. Initialize SessionList/SessionDetail/sidebar
     └─ SessionList loads from existing SQLite cache
  4. Present window immediately
  5. Set model.indexing = true and show header spinner
  6. Send IndexingWorkerInput::StartIncremental(sources)
  7. On IndexingWorkerOutput::Completed:
     ├─ model.indexing = false
     └─ SessionList reloads from DB
  8. On IndexingWorkerOutput::Failed(err):
     ├─ model.indexing = false
     └─ Show toast, keep cached list
```

Behavior:
- First launch with empty DB shows an indexing placeholder.
- Subsequent launches show cached sessions immediately.

---

## 3. Worker API and single-flight behavior

```rust
enum IndexingWorkerInput {
    StartIncremental(SessionSources),
    StartFullReindex(SessionSources),
}

enum IndexingWorkerOutput {
    Completed { indexed: usize, skipped: usize },
    Failed(String),
}
```

Single-flight rules:
- If a job is already running, new requests are ignored (or queued later if we
  decide to support queueing).
- `ReindexRequested` while running shows a short toast: "Indexing already in
  progress."

---

## 4. Incremental fingerprint logic

Before parsing each candidate source:

```rust
fn should_reindex(&self, file_path: &Path) -> Result<bool> {
    let meta = fs::metadata(file_path)?;
    let modified = meta.modified()?.duration_since(UNIX_EPOCH)?;
    let current_mtime_ns = i64::try_from(modified.as_nanos())?;
    let current_size = i64::try_from(meta.len())?;

    match self.get_fingerprint(file_path)? {
        Some((stored_mtime_ns, stored_size))
            if stored_mtime_ns == current_mtime_ns && stored_size == current_size => Ok(false),
        _ => Ok(true),
    }
}
```

After successful parse + write:
- Upsert fingerprint inside the same transaction as
  `insert_parsed_session`.

### Per-provider fingerprint targets

| Provider | Fingerprint target | Parse target | Prune strategy |
|---|---|---|---|
| Claude Code | each `.jsonl` file | same file | file-path prune |
| Codex | each `rollout-*.jsonl` file | same file | file-path prune |
| Mistral Vibe | `messages.jsonl` inside session dir | session dir parse | dir-path prune (`Session.file_path` remains session dir) |
| OpenCode SQLite | `opencode.db` | SQLite rows | ID prune only when SQLite backend is enumerated |
| OpenCode JSON | each session `.json` | same file | file-path prune |

OpenCode-specific rule:
- If `opencode.db` fingerprint is unchanged in incremental mode, skip SQLite
  parse and skip SQLite ID-prune for that run.

---

## 5. Pruning and fingerprint cleanup

Pruning happens after indexing:

1. **File-path prune (all file-backed sources)**
   Remove sessions whose `file_path` target no longer exists.

2. **OpenCode SQLite ID prune (conditional)**
   Keep existing `prune_stale_opencode_sessions(&indexed_ids)` semantics, but
   only when SQLite backend was actually enumerated.

3. **Fingerprint orphan cleanup**
   Remove `file_fingerprints` rows whose `file_path` no longer exists.

This avoids accidental OpenCode deletions when SQLite parsing is skipped due to
unchanged `opencode.db`.

---

## 6. DB concurrency and reliability

For app DB connections:

- `PRAGMA journal_mode=WAL` on writer/indexer connection.
- `busy_timeout(5s)` on both writer and read connections to reduce transient
  `SQLITE_BUSY` during concurrent read/write.
- Keep one writer path (worker); UI thread remains read-only for list/detail
  loads.

No custom checkpoint tuning in this phase.

---

## 7. UI behavior

### Header spinner

- Add `indexing: bool` to `App` model.
- Spinner in `adw::HeaderBar`, visible while indexing is running.
- Tooltip: "Indexing sessions..."

### First-launch placeholder

- `SessionList` receives indexing state updates.
- If list is empty and indexing is running, show:
  - Title: "Indexing sessions..."
  - Description: "This may take a moment on first launch."
- Replace with list when data arrives.

### Failure UX

- On worker failure, hide spinner and show toast.
- Keep existing cached data visible (graceful degradation).

---

## 8. Manual reindex behavior (`ReindexRequested`)

- Trigger `IndexingWorkerInput::StartFullReindex`.
- Full reindex clears:
  - `sessions`
  - `messages`
  - `transcript_items`
  - `tool_calls`
  - `subagents`
  - `file_fingerprints`
- Then parses all providers without skip checks.

---

## 9. Error handling and edge cases

- Parse failures remain non-fatal per file/session; indexing continues.
- If a file disappears between enumeration and parse, log + skip.
- If DB init fails, keep current behavior (error log + empty list/cached list
  as available).
- If indexing fails mid-run, keep previously indexed rows; no destructive
  rollback of prior successful sessions.

---

## 10. Test and verification plan

1. **Schema tests**
   - migration v4 creates `file_fingerprints`.

2. **Indexer unit/integration tests**
   - unchanged file is skipped.
   - changed file (mtime/size) is reindexed.
   - full reindex clears fingerprints.
   - OpenCode incremental skip does not trigger accidental stale-ID prune.

3. **UI behavior checks**
   - startup renders cached list before background indexing completes.
   - spinner appears during indexing and hides on completion/failure.

4. **Manual verification command**
   - `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures`

---

## Expected outcome

- Fast time-to-first-render from cached SQLite data.
- Repeated launches avoid reparsing unchanged sources.
- Lower startup CPU/IO and fewer redundant writes.
- Stable responsiveness as session volume grows.
