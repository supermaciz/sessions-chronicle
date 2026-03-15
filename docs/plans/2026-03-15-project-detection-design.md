# Project Detection: Design

**Issue:** [#67](https://github.com/supermaciz/sessions-chronicle/issues/67)  
**Date:** 2026-03-15  
**Status:** Accepted  
**Prerequisite for:** [Option A — Project Sidebar](2026-03-14-project-views-exploration.md)

## Problem

Sessions Chronicle indexes AI coding sessions from four assistants. Each parser
already extracts a raw `cwd` (current working directory) and stores it in
`sessions.project_path`. However, that path is not a stable project identity:

- Sessions from git worktrees and the main repo are treated as separate projects.
- Sessions from subdirectories of the same repo are not grouped.
- Symlinked or normalized paths can refer to the same logical project but appear
  as different raw paths.
- There is no `projects` table — project data is derived ad-hoc from raw paths.
- The upcoming Option A sidebar needs a stable project identity to group and
  filter sessions.

## Scope

**In scope:**
- Git root resolution (walk up to `.git`)
- Git worktree resolution (`.git` file → common git dir → main repo root)
- Preserve raw `project_path` for resume behavior and session subtitles
- New `projects` table with integer PK (v6 migration)
- Project upsert during indexing for all 4 assistants
- Graceful fallback when paths or ancestors don't exist on disk
- Unit and integration tests

**Out of scope:**
- UI changes (sidebar, filtering, display name disambiguation)
- Per-project analytics or metadata beyond name/path
- Migrating existing analytics queries from raw `project_path` to canonical
  `project_id`

## Architecture

### Approach

A new standalone module `src/project_resolver.rs` owns all resolution logic.
The indexer calls it after parsing each session, upserts into the `projects`
table, and sets `sessions.project_id`.

Alternatives considered:
- **Resolution inside each parser** — rejected because it duplicates logic
  across 4 parsers and muddies parser responsibility.
- **Post-indexing batch pass** — rejected because it adds two-phase pipeline
  complexity and risks stale project entries.

## Resolution Algorithm

**Module:** `src/project_resolver.rs`
**Input:** raw `cwd` path (`String` from parser)
**Output:** resolved canonical project path (`String`), or raw `cwd` as fallback

```text
resolve_project_path(cwd: &str) -> String:
    1. Let raw_path = Path::new(cwd)
    2. Walk upward from raw_path until finding the first ancestor that exists
       on disk
       - If no ancestor exists, return cwd as-is
    3. Canonicalize that existing ancestor
       - If canonicalization fails, return cwd as-is
    4. Walk upward from the canonicalized path looking for .git
    5. For each ancestor:
         a. Check if ancestor/.git exists
         b. If DIRECTORY:
            - Normal git repo
            - Return ancestor path
         c. If FILE:
            - Read content: "gitdir: <path>"
            - Parse <path> as a filesystem path
            - If relative, resolve it relative to the directory containing the
              .git file
            - If the resolved path is .../.git/worktrees/<name>, strip
              /worktrees/<name> to get the common git dir
            - If the common git dir ends with /.git, return its parent as the
              main repo root
            - If read/parse/resolution fails, return the canonicalized existing
              ancestor
    6. No .git found:
       - Return the canonicalized existing ancestor
```

**Key decisions:**
- `std::fs::canonicalize` normalizes symlinks before storing canonical project
  identity.
- If the raw path has no existing ancestor on disk, return `cwd` as-is.
- Once an existing ancestor has been canonicalized successfully, later failures
  fall back to that canonicalized path, not the raw `cwd`.
- Git gitfiles are parsed as paths, including relative `gitdir:` values.
- No new crate dependencies; `std::fs` is sufficient.

## Database Schema

### v6 Migration

**New table:**

```sql
CREATE TABLE projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL
);

CREATE INDEX idx_projects_name ON projects(name);
```

**Sessions table change:**

```sql
ALTER TABLE sessions ADD COLUMN project_id INTEGER REFERENCES projects(id);
CREATE INDEX idx_sessions_project_id ON sessions(project_id);
```

**Migration steps:**
1. Create the `projects` table and index.
2. Add `project_id` column to `sessions`.
3. Create `idx_sessions_project_id`.
4. Clear `file_fingerprints` to force full re-index (same pattern as v5).
5. `PRAGMA user_version = 6`.
6. Re-index sessions so `projects` and `sessions.project_id` are populated.

### Foreign Key Semantics

`project_id` is declared as a foreign key to `projects(id)`.

If SQLite foreign key enforcement is enabled for each connection with
`PRAGMA foreign_keys = ON`, the relationship is enforced at runtime.  
If the application does not enable that pragma yet, `project_id` still acts as a
logical foreign key and application code must preserve referential integrity.

### Column Semantics

Both columns are kept on `sessions`:

| Column | Meaning | Example |
|--------|---------|---------|
| `project_path` | Raw `cwd` from parser | `/home/user/myapp/src/backend` |
| `project_id` | FK to resolved project root | → `projects.id` where `path = "/home/user/myapp"` |

- `project_path` preserves what the user sees in their terminal (useful as
  session subtitle).
- `project_path` remains the best-effort working directory for resume/open in
  terminal flows.
- `project_id` is the canonical identity for grouping and filtering.
- A session with no `cwd` gets `project_id = NULL`.

## Indexer Integration

### Flow

```text
parse session → get raw cwd (project_path)
       ↓
resolve_project_path(cwd) → resolved root
       ↓
upsert into projects table → get project_id
       ↓
insert session with project_id
```

### Upsert SQL

```sql
INSERT INTO projects (path, name) VALUES (?1, ?2)
    ON CONFLICT(path) DO UPDATE SET name = excluded.name;
SELECT id FROM projects WHERE path = ?1;
```

### Changes

All assistants already converge into the same parsed-session storage path, so
project resolution is performed once in the shared insert path rather than in
individual parsers. This keeps parsers responsible only for extracting raw
metadata and avoids assistant-specific divergence in project identity rules.

- The shared parsed-session insert path gains a project-resolution step before
  inserting into `sessions`.
- Parser-specific index methods remain unchanged apart from continuing to call
  the shared insert path.
- No parser changes — parsers keep returning raw `cwd` in `project_path`.

### Edge Cases

| Scenario | Behavior |
|----------|----------|
| `project_path` is `None` | `project_id` stays `NULL`, no upsert |
| Path doesn't exist on disk | Walk up to nearest existing ancestor; if none exists, use raw cwd |
| Path itself is missing but a parent exists | Resolve from the nearest existing ancestor |
| `.git` file contains a relative `gitdir:` | Resolve relative to the gitfile directory |
| Two worktrees of same repo | Both resolve to same `projects.path` |
| Fixture data (`--sessions-dir`) | Usually falls back to raw cwd because fixture paths don't exist locally |
| Re-index same session | Idempotent — project exists, gets same `id` |

## Model Change

`Session` struct in `src/models/session.rs` gains:

```rust
pub project_id: Option<i64>,
```

## Module Structure

**New files:**
- `src/project_resolver.rs`

**Modified files:**
- `src/main.rs` — `mod project_resolver;`
- `src/database/schema.rs` — v6 migration
- `src/database/indexer.rs` — resolve + upsert + set project_id
- `src/database/mod.rs` — load/store queries updated to read `project_id`
- `src/models/session.rs` — add `project_id` field

**Unchanged:**
- All parsers
- UI
- `session_sources.rs`
- `src/database/analytics.rs` — unchanged in this phase; continues using raw
  `project_path` until project-aware analytics is designed

## Testing

### Unit Tests (`project_resolver`)

| Test | Setup | Expected |
|------|-------|----------|
| Normal git repo | tempdir with `.git/` directory | Resolves to that dir |
| Subdirectory of repo | cwd = `repo/src/lib` | Resolves to `repo/` |
| Git worktree | `.git` file with `gitdir:` content | Resolves to main repo root |
| Relative `gitdir:` | `.git` file points to relative path | Resolves correctly |
| Non-git directory | No `.git` up the tree | Returns canonicalized existing path |
| Path doesn't exist | Nonexistent path string | Returns raw cwd if no ancestor exists |
| Missing leaf, existing parent | cwd leaf missing, parent exists | Resolves from nearest existing ancestor |
| Symlink normalization | Symlinked path | Resolves to canonical location |

### Integration Tests

Integration tests split into two groups:

1. **Database/indexer tests using existing fixtures**
   - Session with no cwd → `project_id` is `NULL`
   - Re-index idempotency → no duplicate projects
   - Fixture paths that do not exist locally fall back predictably

2. **Resolver/indexer tests using temporary real git repositories**
   - Two sessions in same repo → same `project_id`
   - Main repo + linked worktree sessions → same `project_id`
   - Subdirectory sessions resolve to repo root

Unit tests use `tempfile::tempdir()` with synthetic `.git` dirs/files.  
Worktree integration tests use a real temporary git repo plus `git worktree`
commands, because fixture-only paths cannot validate canonical worktree
resolution.

## Display Name Disambiguation

Not part of this design. When the UI loads projects for the sidebar, it will
detect basename collisions at query time and append a disambiguator dynamically
(e.g. `api (myapp)` vs `api (otherapp)`). The `projects` table stores only
`name` (basename). This keeps the table simple and avoids re-computation when
the project set changes.

## Deferred Follow-Up

This design intentionally does not yet migrate analytics or project list queries
to the new canonical project identity. Those changes should happen in the
project-aware UI and analytics work once `projects` and `project_id` exist.
