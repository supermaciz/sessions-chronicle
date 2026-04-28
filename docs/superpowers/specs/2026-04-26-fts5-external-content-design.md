# FTS5 External Content for Messages

Date: 2026-04-26
Status: Implemented [#126](https://github.com/supermaciz/sessions-chronicle/pull/126)

## Problem

The `messages` table is an FTS5 virtual table inherited from schema v0. All
columns except `content` are `UNINDEXED`, so non-search reads
(`load_message_full_content`, `load_message_previews_for_session`,
`load_transcript_items`) cannot use indexed lookup by
`(session_id, message_index)`. This made transcript pagination on large
sessions multi-second-slow.

PR #126 introduced a `message_cache` mirror table (v13) maintained by
application-level dual-writes from `indexer.rs`, plus a runtime router
(`direct_message_table_for_session`) that picks `messages` or `message_cache`
per session based on which one has data.

The mirror approach has working downsides:

- Two writes per message in `index_session_tx`; a future writer that forgets
  the mirror corrupts transcript reads silently.
- Content is duplicated on disk (~2x message bytes).
- A runtime `EXISTS` lookup per read for the routing decision.
- `CAST(message_index AS INTEGER)` is required everywhere because the FTS5
  fallback path stores `message_index` as TEXT (commit `0838506`).
- The schema cannot be reasoned about without running the indexer to know
  which table holds a given session.

## Goal

Replace the dual-table layout with a single source of truth backed by an
FTS5 external-content index. After this change:

- `messages` is a b-tree table with proper indexes and INTEGER columns.
- `messages_fts` is a virtual FTS5 index whose content is fully derived from
  `messages` via triggers. It stores only the inverted index, not the
  content.
- No application-level dual-write, no runtime table routing, no CAST.

This is the layout used by AgentsView (`internal/db/db.go`,
`internal/db/schema.sql`), the closest peer project listed in
`docs/SIMILAR_PROJECTS.md`.

## Scope and timing

This work lands inside PR #126 directly. The v13 migration in PR #126 is
rewritten so the dual-table mirror is never shipped: there is no v14, no
intermediate state. The commits introducing `message_cache`
(`4c6e748`, `0838506`, `114407d`) are reworked or dropped before merge.

Justification: the project has no production users yet. Replacing the v13
content cleanly is cheaper than carrying transitional dead code and a future
v14 cleanup.

## Target schema

```sql
-- Source of truth (b-tree)
CREATE TABLE messages (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id    TEXT NOT NULL,
    message_index INTEGER NOT NULL,
    role          TEXT NOT NULL,
    content       TEXT NOT NULL,
    timestamp     INTEGER NOT NULL,
    model         TEXT,
    UNIQUE(session_id, message_index)
);

-- FTS5 external-content index over messages
CREATE VIRTUAL TABLE messages_fts USING fts5(
    content,
    content='messages',
    content_rowid='id'
);

-- Sync triggers
CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;

CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content)
        VALUES('delete', old.id, old.content);
END;

CREATE TRIGGER messages_au AFTER UPDATE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content)
        VALUES('delete', old.id, old.content);
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;
```

Notes:

- `id INTEGER PRIMARY KEY AUTOINCREMENT` is a defensive choice so rowids are
  never reused after deletes. FTS5 external content only requires a stable
  rowid for each live row, so `INTEGER PRIMARY KEY` would be sufficient if the
  sync triggers always remain correct; avoiding reuse makes stale-index bugs
  less likely to point at unrelated new content.
- `UNIQUE(session_id, message_index)` creates the lookup index used by the
  read path; no additional `CREATE INDEX` is needed.
- No `tokenize=` clause: the v0 virtual table did not specify one either, so
  the SQLite default `unicode61` is preserved. Changing tokenizer is a
  separate decision.

## Migration

The v13 migration is rewritten end-to-end. There is no v14.

```rust
/// Migrate from v12 to v13.
///
/// Replace the FTS5-virtual `messages` table inherited from v0 with a
/// b-tree source-of-truth `messages` table backed by an FTS5
/// external-content `messages_fts` index. The transcript read path no
/// longer needs runtime table routing or CAST(message_index AS INTEGER).
///
/// Existing message data is intentionally not preserved: clearing
/// `file_fingerprints` causes the indexer to repopulate from JSONL on the
/// next run. JSONL files are the authoritative source.
fn apply_v13_migration(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "BEGIN IMMEDIATE;
         DROP TRIGGER IF EXISTS messages_ai;
         DROP TRIGGER IF EXISTS messages_ad;
         DROP TRIGGER IF EXISTS messages_au;
         DROP TABLE IF EXISTS messages_fts;
         DROP TABLE IF EXISTS messages;

         CREATE TABLE messages (
             id            INTEGER PRIMARY KEY AUTOINCREMENT,
             session_id    TEXT NOT NULL,
             message_index INTEGER NOT NULL,
             role          TEXT NOT NULL,
             content       TEXT NOT NULL,
             timestamp     INTEGER NOT NULL,
             model         TEXT,
             UNIQUE(session_id, message_index)
         );

         CREATE VIRTUAL TABLE messages_fts USING fts5(
             content,
             content='messages',
             content_rowid='id'
         );

         CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
             INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
         END;
         CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
             INSERT INTO messages_fts(messages_fts, rowid, content)
                 VALUES('delete', old.id, old.content);
         END;
         CREATE TRIGGER messages_au AFTER UPDATE ON messages BEGIN
             INSERT INTO messages_fts(messages_fts, rowid, content)
                 VALUES('delete', old.id, old.content);
             INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
         END;

         DELETE FROM file_fingerprints;
         PRAGMA user_version = 13;
         COMMIT;",
    )?;

    Ok(())
}
```

The schema header comment for migrations becomes:

```text
//   12 – add session-list ordering indexes for faster startup/filter reloads
//   13 – replace FTS5-virtual `messages` with a b-tree source table backed
//        by an FTS5 external-content `messages_fts` index; clear
//        file_fingerprints to force reindexing from JSONL
```

## Application changes

### `src/database/mod.rs`

Remove `direct_message_table_for_session` (introduced earlier in PR #126).

`load_message_full_content`:

```rust
let mut stmt = db.prepare(
    "SELECT content FROM messages
     WHERE session_id = ?1 AND message_index = ?2",
)?;
```

No CAST. Static SQL string, no `format!`.

`load_message_previews_for_session`:

```rust
let mut stmt = db.prepare(
    "SELECT session_id, message_index, role,
            substr(content, 1, ?2) AS content,
            length(content) AS content_len,
            timestamp, model
     FROM messages
     WHERE session_id = ?1
     ORDER BY message_index ASC
     LIMIT ?3 OFFSET ?4",
)?;
```

`load_transcript_items` LEFT JOIN clause:

```sql
LEFT JOIN messages m ON ti.session_id = m.session_id
                    AND ti.message_index = m.message_index
```

The two FTS search sites at `mod.rs:211-213` and `mod.rs:232-234` move from
`messages MATCH ?` to `messages_fts MATCH ?`, joining back to `messages` to
read `session_id`:

```sql
FROM messages_fts
JOIN messages m ON m.id = messages_fts.rowid
JOIN sessions s ON s.id = m.session_id
WHERE messages_fts MATCH ?
  AND s.is_subagent = 0
  ...
ORDER BY bm25(messages_fts) ASC, s.last_updated DESC
```

`bm25(messages)` becomes `bm25(messages_fts)`.

### `src/database/indexer.rs`

`index_session_tx`: a single `INSERT INTO messages (session_id,
message_index, role, content, timestamp, model)`. The `id` column is
auto-assigned. The second `INSERT OR REPLACE INTO message_cache` is removed.
The `messages_ai` trigger keeps `messages_fts` in sync.

`delete_session_contents_tx`: a single `DELETE FROM messages WHERE
session_id = ?1`. The `messages_ad` trigger keeps `messages_fts` in sync.
The `DELETE FROM message_cache` line is removed.

Other delete sites in `indexer.rs` keep their `DELETE FROM messages ...` SQL
unchanged but remove their paired `DELETE FROM message_cache ...` statements.
Those `messages` deletes now drive triggers. Bulk-delete trigger overhead is a
known issue (AgentsView documents it); not addressed here, see "Out of scope".

### `src/ui/session_detail.rs`

The test fixture INSERT at line 1660 changes column types: `message_index`
and `timestamp` are passed as INTEGER, the `id` column is auto-assigned.

### Tests in `src/database/schema.rs`

Remove:

- `v12_to_v13_migration_backfills_message_cache`

Add:

- `v13_migration_creates_messages_and_fts_index`: fresh DB initialises with
  `messages` (b-tree, `id INTEGER PRIMARY KEY`, `UNIQUE(session_id,
  message_index)`), `messages_fts` (virtual FTS5 with `content='messages'`),
  and the three triggers.
- `v11_to_v13_clears_fingerprints_and_drops_old_messages`: seed a v11 DB
  with rows in the FTS5 `messages` and a row in `file_fingerprints`, run
  migration, assert the new `messages` is empty and `file_fingerprints` is
  empty.
- `messages_fts_stays_in_sync_via_triggers`: INSERT a row into `messages`,
  assert `messages_fts MATCH 'word'` returns it; DELETE it, assert MATCH no
  longer returns; INSERT then UPDATE its content, assert MATCH on the new
  content matches and on the old content does not.

Adjust `fresh_db_initializes_to_latest` to assert `table_exists("messages")`
and `table_exists("messages_fts")`, and to assert `message_cache` does not
exist.

## Verification plan

CI parity:

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
cargo test --all --no-fail-fast
```

Manual:

- Start with `--sessions-dir tests/fixtures` after deleting any existing DB;
  confirm fixtures fully reindex and transcripts render.
- Open a 200+ message session; confirm the transcript still renders without
  the "not responding" popup (the perf gain comes from the b-tree index on
  `(session_id, message_index)` via `UNIQUE`, identical to v13 mirror
  performance).
- Run a full-text search across sessions; confirm results match the v0
  baseline qualitatively (same tokenizer, same `bm25` ordering).
- Switch sessions mid-render (regression check from PR #126).
- Flatpak build verification:
  `flatpak-builder --user flatpak_app build-aux/dev.maciz.sessionschronicle.Devel.json --force-clean`.

## Risks and mitigations

- **First-run reindex cost.** Clearing `file_fingerprints` forces the
  indexer to re-parse all JSONL files at next launch. Acceptable for the
  current user base (no production users). The existing indexing UI surfaces
  progress.
- **`sqlite_sequence` table appears.** A side effect of `AUTOINCREMENT`.
  Inert and expected; no action needed.
- **Bulk-delete trigger overhead.** AgentsView reports per-row trigger
  dispatch dominates large `DELETE FROM messages WHERE session_id = ?` on
  sessions of thousands of rows with multi-MB content. Sessions Chronicle
  does not currently profile this as a bottleneck; the optimisation
  (drop/rebuild `messages_ad` trigger around the bulk delete, replace with a
  single `INSERT INTO messages_fts(messages_fts, rowid, content) SELECT
  'delete', id, content FROM messages WHERE session_id = ?`) is left for a
  follow-up PR if profiling confirms it.

## Out of scope

- **Tokenizer change** (Porter stemming, language-aware tokenizers): a
  product decision separate from this schema migration.
- **Bulk-delete trigger bypass**: see risks; only if profiling shows it.
- **Long-term virtualization of the transcript renderer** (`gtk::ListView` +
  `gio::ListStore`): mentioned in PR #126 description; independent of this
  schema change.
