# OpenCode SQLite Dual-Read Parser Design

**Date:** 2026-02-21
**Status:** Approved
**Scope:** Update the OpenCode parser to read from both the new SQLite database
(`opencode.db`) and the legacy JSON file tree, with deduplication.

---

## Problem

OpenCode migrated its session storage from a JSON file tree to SQLite on
2026-02-14. Sessions Chronicle's parser only reads the legacy JSON files, so
new sessions written to `opencode.db` are invisible. On the author's system
there are 566 sessions in SQLite but only ~536 legacy JSON files — any
post-migration session is missing.

## Approach

**Shared trait with two backends.** Extract a common `OpenCodeBackend` trait
with methods for listing sessions, loading metadata, loading messages, and
loading parts. Two implementations:

- `JsonBackend` — extracted from the existing `opencode.rs` parser
- `SqliteBackend` — new, opens `opencode.db` read-only and queries the
  `session`, `message`, and `part` tables

The `OpenCodeParser` orchestration and `process_part()` logic stay shared
between both backends.

---

## Trait Definition

```rust
trait OpenCodeBackend {
    fn list_sessions(&self) -> Result<Vec<SessionEntry>>;
    fn load_session_metadata(&self, entry: &SessionEntry) -> Result<SessionMetadata>;
    fn load_messages(&self, session_id: &str) -> Result<Vec<MessageMetadata>>;
    fn load_parts(&self, message_id: &str) -> Result<Vec<PartData>>;
}
```

Intermediate types (`SessionMetadata`, `MessageMetadata`, `PartData`) are
unchanged from the current parser. `PartData.raw` holds a `serde_json::Value`
in both backends — for JSON it is the file contents, for SQLite it is the
`data` column blob.

### SessionEntry

```rust
struct SessionEntry {
    id: String,
    source: SessionSource,
}

enum SessionSource {
    JsonFile(PathBuf),
    SqliteRow { db_path: PathBuf },
}
```

---

## SQLite Backend

Opens `opencode.db` with `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX`.

### Session Metadata

```sql
SELECT id, directory, title, parent_id, time_created, time_updated
  FROM session WHERE id = ?
```

Maps directly to `SessionMetadata`. The `directory` field provides
`project_path`.

### Messages

```sql
SELECT id, time_created, data FROM message
  WHERE session_id = ? ORDER BY time_created, id
```

Role is extracted from `data` JSON blob: `data.role`.

### Parts

```sql
SELECT id, time_created, data FROM part
  WHERE message_id = ? ORDER BY time_created, id
```

The entire `data` blob becomes `PartData.raw`. Part type is `data["type"]`.
No explicit `order` column; ordering uses `time_created` + `id`.

### First Prompt

Use session `title` from the database. Fall back to first user text part if
the title is empty.

### file_path

For SQLite-sourced sessions, `file_path` stores the `opencode.db` path. The
`project_path` (from `session.directory`) is always set for SQLite sessions,
so `file_path` is only a fallback identifier.

---

## JSON Backend

Extracted from the current `OpenCodeParser` with no behavioral changes:

- `list_sessions()` — walks `storage_root/session/**/*.json`
- `load_session_metadata()` — reads the session JSON file
- `load_messages()` — reads from `storage_root/message/<session_id>/`
- `load_parts()` — reads from `storage_root/part/<message_id>/`

Shared helpers (`read_json`, `timestamp_from_millis`) become module-level
functions.

---

## Dual-Read Orchestration & Dedup

The indexer calls both backends with SQLite first:

1. **SQLite backend** (if `opencode.db` exists): index all sessions,
   collecting their IDs in a `HashSet`.
2. **JSON backend**: index sessions, skipping any ID already seen from SQLite.

SQLite is authoritative — when both backends have the same session ID, the
SQLite version wins because it has newer/complete data.

### Indexer Signature Change

```rust
pub fn index_opencode_sessions(
    &mut self,
    storage_root: &Path,
    db_path: Option<&Path>,
) -> Result<usize>
```

### SessionSources Change

```rust
pub struct SessionSources {
    pub claude_dir: PathBuf,
    pub opencode_storage_root: PathBuf,
    pub opencode_db_path: Option<PathBuf>,  // NEW
    pub codex_dir: PathBuf,
    pub vibe_dir: PathBuf,
    pub override_mode: bool,
}
```

- Default mode: `opencode_db_path = Some(~/.local/share/opencode/opencode.db)`
- Override mode: `Some(override_root/opencode_storage/opencode.db)` if it
  exists, else `None`

---

## Module Structure

The single `src/parsers/opencode.rs` becomes a module:

```
src/parsers/opencode/
├── mod.rs              # Trait, OpenCodeParser, process_part(), shared types
├── json_backend.rs     # JsonBackend (extracted from current opencode.rs)
├── sqlite_backend.rs   # SqliteBackend (new)
```

---

## Test Fixtures

Add `tests/fixtures/opencode_storage/opencode.db` containing:

1. One session overlapping with a JSON fixture (same ID) — tests dedup
2. One session unique to SQLite — tests new session discovery
3. One subagent session (`parent_id` set) — tests subagent handling

Existing JSON fixtures remain unchanged.

---

## Error Handling

- **Locked DB:** Log warning, fall back to JSON-only.
- **Corrupt data blobs:** Log warning, skip part, continue indexing.
- **Archived sessions:** Include them (valid history; `time_archived` is
  informational only).
- **Missing opencode.db:** Graceful fallback to JSON-only (pre-migration
  installs).

---

## OpenCode Schema Reference

Matches the live database as of 2026-02-21:

```sql
CREATE TABLE session (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    parent_id TEXT,
    slug TEXT NOT NULL,
    directory TEXT NOT NULL,
    title TEXT NOT NULL,
    version TEXT NOT NULL,
    share_url TEXT,
    summary_additions INTEGER,
    summary_deletions INTEGER,
    summary_files INTEGER,
    summary_diffs TEXT,
    revert TEXT,
    permission TEXT,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_compacting INTEGER,
    time_archived INTEGER
);

CREATE TABLE message (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    data TEXT NOT NULL
);

CREATE TABLE part (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    data TEXT NOT NULL
);
```

Part `data` blob types: `text`, `reasoning`, `file`, `tool`, `snapshot`,
`patch`, `agent`, `compaction`, `subtask`, `retry`, `step-start`,
`step-finish`.
