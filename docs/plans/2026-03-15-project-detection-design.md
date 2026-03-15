# Project Detection: Design

**Issue:** [#67](https://github.com/supermaciz/sessions-chronicle/issues/67)  
**Date:** 2026-03-15  
**Status:** Accepted  
**Prerequisite for:** [Option A — Project Sidebar](2026-03-14-project-views-exploration.md)

## Problem

Sessions Chronicle indexes AI coding sessions from four assistants. Each parser
already extracts a raw `cwd` (current working directory) and stores it in
`sessions.project_path`. However there is no canonical project identity:

- Sessions from git worktrees and the main repo are treated as separate projects.
- Sessions from subdirectories of the same repo are not grouped.
- There is no `projects` table — project data is derived ad-hoc from raw paths.
- The upcoming Option A sidebar needs a stable project identity to group and
  filter sessions.

## Scope

**In scope:**
- Git root resolution (walk up to `.git`)
- Git worktree resolution (`.git` file → main repo root)
- New `projects` table with integer PK (v6 migration)
- Project upsert during indexing for all 4 assistants
- Graceful fallback when paths don't exist on disk
- Unit and integration tests

**Out of scope:**
- UI changes (sidebar, filtering, display name disambiguation)
- Per-project analytics or metadata beyond name/path

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
**Input:** raw `cwd` path (String from parser)
**Output:** resolved project path (String), or raw `cwd` as fallback

```text
resolve_project_path(cwd: &str) -> String:
    1. Let path = Path::new(cwd)
    2. If path doesn't exist on disk → return cwd as-is
    3. Canonicalize path (resolve symlinks and ..)
    4. Walk up from canonicalized path looking for .git:
       For each ancestor (including path itself):
         a. Check if ancestor/.git exists
         b. If DIRECTORY → normal git repo → return ancestor path
         c. If FILE → worktree:
            - Read content: "gitdir: /path/to/main/.git/worktrees/<name>"
            - Parse the gitdir path
            - Strip /worktrees/<name> to get main repo .git dir
            - Strip /.git to get main repo root
            - Return main repo root
    5. No .git found → return canonicalized cwd (non-git project)
```

**Key decisions:**
- `std::fs::canonicalize` normalizes symlinks before storing.
- The worktree `.git` file format (`gitdir: <path>\n`) is stable across git
  versions.
- Every step has a graceful fallback — if file read fails or path doesn't
  parse, return `cwd` as-is.
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

### Column Semantics

Both columns are kept on `sessions`:

| Column | Meaning | Example |
|--------|---------|---------|
| `project_path` | Raw `cwd` from parser | `/home/user/myapp/src/backend` |
| `project_id` | FK to resolved project root | → `projects.id` where `path = "/home/user/myapp"` |

- `project_path` preserves what the user sees in their terminal (useful as
  session subtitle).
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

- `index_session_file` (and equivalent methods for each assistant) gains a
  step between parsing and DB insert: resolve path, upsert project, set
  `project_id`.
- All 4 assistant paths flow through the same `store_parsed_session` method,
  so the integration point is centralized.
- No parser changes — parsers keep returning raw `cwd` in `project_path`.

### Edge Cases

| Scenario | Behavior |
|----------|----------|
| `project_path` is `None` | `project_id` stays `NULL`, no upsert |
| Path doesn't exist on disk | Project created with raw cwd as `path` |
| Two worktrees of same repo | Both resolve to same `projects.path` |
| Fixture data (`--sessions-dir`) | Falls back to raw cwd (paths don't exist) |
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
- `src/models/session.rs` — add `project_id` field

**Unchanged:**
- All parsers
- UI
- `session_sources.rs`

## Testing

### Unit Tests (`project_resolver`)

| Test | Setup | Expected |
|------|-------|----------|
| Normal git repo | tempdir with `.git/` directory | Resolves to that dir |
| Subdirectory of repo | cwd = `repo/src/lib` | Resolves to `repo/` |
| Git worktree | `.git` file with `gitdir:` content | Resolves to main repo root |
| Non-git directory | No `.git` up the tree | Returns raw cwd |
| Path doesn't exist | Nonexistent path string | Returns raw cwd |
| Symlink normalization | Symlinked path | Resolves to canonical location |

### Integration Tests (indexer + projects)

| Test | Expected |
|------|----------|
| Two sessions, same project | Same `project_id` |
| Worktree + main repo sessions | Same `project_id` |
| Session with no cwd | `project_id` is NULL |
| Re-index idempotency | No duplicate projects |

Unit tests use `tempfile::tempdir()` with synthetic `.git` dirs/files.
Integration tests use existing fixtures from `tests/fixtures/`.

## Display Name Disambiguation

Not part of this design. When the UI loads projects for the sidebar, it will
detect basename collisions at query time and append a disambiguator dynamically
(e.g. `api (myapp)` vs `api (otherapp)`). The `projects` table stores only
`name` (basename). This keeps the table simple and avoids re-computation when
the project set changes.
