# Startup Performance Design (Issue #59)

## Problem

App startup is UI-blocking when session directories contain many files.
All sessions are parsed and indexed synchronously in `App::init`, blocking
the GTK main thread until done. There is no mtime/size check, so every file
is re-parsed and re-inserted on every launch.

## Scope

Three axes from the issue comment:

1. **Non-blocking startup** -- show the session list immediately from cached
   SQLite data, index in a background thread.
2. **Incremental skip** -- track file fingerprints (mtime + size) to skip
   unchanged files.
3. **Reduced write amplification** -- largely solved by axis 2; fingerprint
   updates are batched into the existing per-session transaction.

Phase 2 (lightweight list parsing + on-demand full parse) is out of scope.

## Approach

**Thread dedié + Relm4 `spawn_oneshot_command`** (pattern already used in
`transcript_row.rs`). No new dependencies. No async. No thread pool.

---

## 1. Schema: `file_fingerprints` table

New table added in migration v4:

```sql
CREATE TABLE file_fingerprints (
    file_path TEXT PRIMARY KEY,
    mtime INTEGER NOT NULL,
    size INTEGER NOT NULL
);
```

Stores the mtime (seconds since epoch) and size (bytes) of each indexed
source file. Consulted before parsing to decide whether to skip.

---

## 2. Modified startup flow

```text
App::init:
  1. SessionSources::resolve()            -- sync, fast (path resolution)
  2. SessionIndexer::new(&db_path)         -- sync (open SQLite, run migrations)
  3. Init UI components (SessionList, etc.)
     └─ SessionList::fetch_sessions()      -- loads from existing SQLite cache
  4. Show window immediately
  5. Show spinner in headerbar
  6. sender.spawn_oneshot_command(move || {
         let mut indexer = SessionIndexer::new(&db_path)?;  // own connection
         indexer.index_all_with_fingerprints(&sources);
         IndexingComplete  // or IndexingFailed(err)
     })
  7. On IndexingComplete:
     └─ Hide spinner
     └─ SessionList::reload_sessions()
```

Key points:
- UI shows immediately with data from the previous run's SQLite cache.
- Background thread opens its own SQLite connection (no shared `Connection`).
- On first launch (empty DB), list is empty with a spinner; a
  `adw::StatusPage` placeholder shows "Indexing sessions..." until done.

---

## 3. Incremental skip logic

Before parsing each file:

```rust
fn should_reindex(&self, file_path: &Path) -> Result<bool> {
    let meta = fs::metadata(file_path)?;
    let current_mtime = meta.modified()?.duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let current_size = meta.len() as i64;

    match self.get_fingerprint(file_path)? {
        Some((stored_mtime, stored_size))
            if stored_mtime == current_mtime && stored_size == current_size => Ok(false),
        _ => Ok(true),
    }
}
```

After successful parse + insert, `update_fingerprint` is called inside the
same transaction as `insert_parsed_session`.

### Per-provider behavior

| Provider | Fingerprint target | Notes |
|---|---|---|
| Claude Code | Each `.jsonl` file | Check before `index_session_file` |
| Codex | Each `rollout-*.jsonl` file | Same pattern |
| Mistral Vibe | `messages.jsonl` per session dir | Most likely to change |
| OpenCode SQLite | `opencode.db` file itself | If unchanged, skip entire backend |
| OpenCode JSON | Each session `.json` file | Per-file check |

### Orphan cleanup

During pruning (already exists for OpenCode), also delete fingerprints
whose `file_path` no longer exists on disk.

---

## 4. Write amplification

With incremental skip (axis 2), unchanged sessions are never re-parsed, so
the existing DELETE + INSERT path is only hit for actually-modified sessions.

The only additional optimization: `update_fingerprint` runs inside the
existing `insert_parsed_session` transaction to avoid an extra SQLite write
per session.

No incremental diff of messages/tool_calls -- the complexity is not
justified given that the skip check eliminates most rewrites.

---

## 5. UI: spinner and first-launch placeholder

### Spinner

- `gtk::Spinner` in the `adw::HeaderBar`.
- New `indexing: bool` field in `App` model, initialized to `true`.
- Set to `false` on `IndexingComplete`.
- Spinner visibility bound to `model.indexing`.
- Tooltip: "Indexing sessions..."

### First-launch placeholder

- `adw::StatusPage` shown in `SessionList` when the DB is empty and
  indexing is in progress.
- Title: "Indexing sessions..."
- Description: "This may take a moment on first launch."
- Replaced by the normal session list on `IndexingComplete`.

---

## 6. Error handling and edge cases

### Background indexing failure

- Thread sends `IndexingFailed(String)` instead of `IndexingComplete`.
- App hides spinner, shows an `adw::Toast` with the error message.
- Session list retains data from the last successful run (graceful
  degradation).

### Deleted files between launches

- Pruning extended to all 4 providers: after indexing, remove DB sessions
  whose `file_path` no longer exists on disk.
- Corresponding fingerprints are also deleted.

### Manual re-index (`ReindexRequested`)

- Uses the same background mechanism (spinner + thread).
- Ignores fingerprints (forces full re-parse) to serve as a "force refresh."

### Concurrent DB access

- Background thread opens its own `Connection`.
- SQLite WAL mode (`PRAGMA journal_mode=WAL`) enabled at connection open.
- WAL allows concurrent reads (UI thread) during writes (background thread).

---

## Expected outcome

- Fast "time to first list render" -- the UI appears immediately with
  cached data.
- Repeated launches skip unchanged files entirely (mtime + size check).
- Lower startup CPU/IO churn on subsequent launches.
- Stable startup time regardless of session volume growth.
