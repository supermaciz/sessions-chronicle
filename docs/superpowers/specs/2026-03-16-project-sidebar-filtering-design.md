# Project Sidebar Filtering: Design

**Parent:** [Project Sidebar Filtering Exploration](2026-03-16-project-sidebar-filtering-exploration.md) — Proposition A  
**Issue:** [#66](https://github.com/supermaciz/sessions-chronicle/issues/66)  
**Date:** 2026-03-16  
**Status:** Implemented [#81](https://github.com/supermaciz/sessions-chronicle/pull/81)

## Problem Statement

The sidebar has a "No projects yet" placeholder where project filtering should live.
The `projects` table and `sessions.project_id` foreign key are already populated by the indexer (PR #80).
This design adds a single-select project ListBox to the sidebar, enabling users to filter the session list by project while cross-filtering with the existing AI assistant checkboxes.

## Scope

- Single-select project ListBox in the sidebar (Proposition A from exploration)
- Cross-filtering: project filter AND AI assistant filter applied together
- Dynamic badge counts reflecting active AI assistant filters
- "All Sessions" default row + "Unassigned" row for sessions without a project
- No info bar (deferred)
- No filter persistence across restarts (always starts on "All Sessions")

---

## 1. Architecture & Data Flow

```
Sidebar
  ├ AI Assistants (CheckButtons, unchanged)
  └ Projects (gtk::ListBox, single-select)
      ├ All Sessions          [42]
      ├ sessions-chronicle    [15]
      ├ my-api                [ 8]
      └ Unassigned            [ 3]
          │
          │ SidebarOutput::FiltersChanged { tools, project_filter }
          ▼
 App (owner of FilterState)
  ├ update filter state
  ├ refresh sidebar project counts
  └ SessionListMsg::SetFilters { tools, project_filter }
          │
          ▼
 SessionList
  ├ fetch_sessions(db, tools, project_filter)
  └ Session rows
```

**Project filter state** -- an enum with 3 variants:

- `AllSessions` -- no project filter
- `Project(i64)` -- filter on a specific project_id
- `Unassigned` -- filter on `project_id IS NULL`

**Ownership:**

- `App` owns the canonical `FilterState { tools, project_filter }`
- `Sidebar` is a UI component only: it renders rows and emits user interactions
- Database queries for project counts stay in the app/database layer, not in the sidebar

**Flow:**

1. User clicks a project row in the ListBox
2. Sidebar emits `FiltersChanged` with active tools AND project filter
3. App updates its canonical filter state
4. App refreshes sidebar project counts from the DB using the active AI assistant filters
5. App routes the full filter state to SessionList
6. SessionList executes the SQL query with both filters

Each change (AI assistant toggle OR project selection) emits a complete `FiltersChanged` with both filter axes. No separate messages -- avoids inconsistent intermediate states.

The important constraint is that one user action produces one coordinated app-level refresh: App recalculates sidebar data and session-list data from the same filter state, instead of letting the sidebar query the DB independently.

---

## 2. Message Types & Relm4 Communication

### New type for project filter

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectFilter {
    AllSessions,
    Project(i64),    // project_id
    Unassigned,      // project_id IS NULL
}
```

### Sidebar modifications

```rust
pub enum SidebarMsg {
    AiAssistantToggled(AiAssistant, bool),
    ProjectSelected(ProjectFilter),
    ProjectsLoaded {
        projects: Vec<ProjectInfo>,
        all_sessions_count: usize,
        unassigned_count: usize,
        show_unassigned: bool,
        selected_filter: ProjectFilter,
    },
}

pub enum SidebarOutput {
    FiltersChanged {
        tools: Vec<AiAssistant>,
        project_filter: ProjectFilter,
    },
}
```

### New struct to populate the ListBox

```rust
pub struct ProjectInfo {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub session_count: usize,  // dynamic, reflects AI filters
}
```

### App modifications

```rust
pub struct FilterState {
    pub tools: Vec<AiAssistant>,
    pub project_filter: ProjectFilter,
}

pub enum AppMsg {
    FiltersChanged {
        tools: Vec<AiAssistant>,
        project_filter: ProjectFilter,
    },
    // ... existing unchanged
}
```

### SessionList modifications

```rust
pub enum SessionListMsg {
    SetFilters {
        tools: Vec<AiAssistant>,
        project_filter: ProjectFilter,
    },
    // replaces SetTools
    // ... existing unchanged
}
```

---

## 3. Database Queries

### Load projects with dynamic counts

Badge counts reflect the active AI assistant filters. App runs the query and sends the resulting rows to the sidebar. Project rows remain visible even when their current count is `0`.

```sql
-- Projects with session count filtered by AI assistants
SELECT p.id, p.name, p.path, COUNT(s.id) AS session_count
FROM projects p
LEFT JOIN sessions s
  ON s.project_id = p.id
 AND s.tool IN (?, ?, ...)           -- active AI filters
 AND s.is_subagent = 0
GROUP BY p.id
ORDER BY MAX(s.last_updated) DESC, p.name COLLATE NOCASE ASC

-- Count for "Unassigned"
SELECT COUNT(*) FROM sessions
WHERE project_id IS NULL
  AND tool IN (?, ?, ...)
  AND is_subagent = 0

-- Count for "All Sessions"
SELECT COUNT(*) FROM sessions
WHERE tool IN (?, ?, ...)
  AND is_subagent = 0
```

### Load sessions with project filter

```sql
-- AllSessions (current behavior, unchanged)
SELECT ... FROM sessions
WHERE tool IN (?, ...) AND is_subagent = 0
ORDER BY last_updated DESC

-- Project(id)
SELECT ... FROM sessions
WHERE project_id = ?
  AND tool IN (?, ...) AND is_subagent = 0
ORDER BY last_updated DESC

-- Unassigned
SELECT ... FROM sessions
WHERE project_id IS NULL
  AND tool IN (?, ...) AND is_subagent = 0
ORDER BY last_updated DESC
```

### Search sessions

Same logic -- add `project_id = ?` or `project_id IS NULL` clauses to the existing FTS queries.

### Optimization

When all AI assistants are checked, omit the `tool IN (...)` clause (as today).
When `AllSessions`, omit the `project_id` clause.

The project query still uses a `LEFT JOIN` in the all-tools case so projects with zero visible sessions remain in the list.

---

## 4. Sidebar UI -- ListBox & Rows

### Layout

```
┌────────────────────────┐
│ Filters          (title)│
│────────────────────────│
│ AI Assistants  (heading)│
│ ☑ Claude Code          │
│ ☑ OpenCode             │
│ ☑ Codex                │
│ ☑ Mistral Vibe         │
│────────────────────────│
│ Projects       (heading)│
│ ┌────────────────────┐ │
│ │ All Sessions  [42] │ │  ← gtk::ListBoxRow, bold, selected by default
│ │▌sessions-chr… [15] │ │  ← adw::ActionRow, accent when selected
│ │ my-api         [8] │ │  ← adw::ActionRow
│ │ Unassigned     [3] │ │  ← gtk::ListBoxRow, italic, shown if unassigned exists in DB
│ └────────────────────┘ │
└────────────────────────┘
```

### Widgets

- **Section Projects:** `gtk::ListBox` with `SelectionMode::Single`
- **"All Sessions":** `gtk::ListBoxRow` with bold `gtk::Label` + badge count. Selected by default at startup.
- **Project rows:** `adw::ActionRow` with:
  - Title: `project.name`
  - Subtitle: `project.path` (truncated with ellipsis natively by GTK)
  - Suffix: badge count in a `gtk::Label` with CSS class `project-badge`
- **"Unassigned":** `gtk::ListBoxRow` with italic label + badge count. Shown when at least one unassigned session exists in the database, even if the current AI assistant filter makes its visible count `0`.
- **ScrolledWindow:** The ListBox sits in a `gtk::ScrolledWindow` with `vexpand: true` to fill remaining space.

### Selection behavior

- Click on a row -> `row-selected` signal -> `SidebarMsg::ProjectSelected(filter)`
- Selected row gets accent background natively via `ListBox` single-select
- Clicking "All Sessions" removes the project filter

### CSS additions

```css
.project-badge {
    border-radius: 9px;
    padding: 0 8px;
    min-height: 18px;
    font-size: 0.8em;
    background: alpha(@accent_bg_color, 0.15);
    color: @accent_fg_color;
}

.unassigned-label {
    font-style: italic;
}
```

---

## 5. Project Loading & Refresh

### When to load the project list

1. **At startup** -- after initial indexing, App sends `SidebarMsg::ProjectsLoaded { ... }` with project rows, counts, and the selected filter.
2. **After each indexation** -- when the indexer finishes (new sessions detected), the App reloads projects and resends `ProjectsLoaded`. This updates counts and surfaces new projects.
3. **On each AI assistant toggle** -- App recalculates dynamic counts and resends `ProjectsLoaded`.
4. **On each project selection** -- no sidebar DB work; App only updates the selected row mirror if needed.

### Refresh flow

```
App::init()
  -> Indexer finished
  -> App loads projects from DB (with counts filtered by active tools)
  -> SidebarMsg::ProjectsLoaded { ... }
  -> Sidebar rebuilds the ListBox
```

For an AI assistant toggle, App follows the same pattern from a single canonical filter state:

```
SidebarOutput::FiltersChanged { tools, project_filter }
  -> App updates FilterState
  -> App loads project counts for tools
  -> App emits SidebarMsg::ProjectsLoaded { ... selected_filter: project_filter }
  -> App emits SessionListMsg::SetFilters { tools, project_filter }
```

### Handling selected project after refresh

- If the selected project still exists -> keep the selection
- If the selected project count drops to `0` after an AI toggle -> keep the selection; the session list becomes empty until the user changes filters
- Project rows do not disappear just because their current visible count is `0`
- `Unassigned` remains visible as long as unassigned sessions exist in the database

### Init change

The sidebar does not need `db_path`. Its init remains lightweight, and App remains responsible for DB-backed refreshes.

---

## 6. Edge Cases & Error Handling

**No projects in database:**
The Projects section shows "All Sessions" with the total count. If unassigned sessions exist, show `Unassigned` too; otherwise there are no project-specific rows.

**All AI assistants unchecked:**
Existing behavior unchanged: session list is empty. All project badges show 0. No project selection change.

**Long project name:**
`adw::ActionRow` truncates the title natively with ellipsis. The subtitle (path) too. The badge remains visible in the suffix.

**Same-name projects (identical basename, different paths):**
The subtitle (path) disambiguates them. Example: two "api" projects with paths `/home/user/work/api` and `/home/user/personal/api`.

**DB error loading projects:**
Log the error, keep the previous list. No crash, no popup.

**Selected project with 0 visible sessions:**
Keep the row selected and show an empty session list state. Do not auto-reset to `All Sessions`; the empty result accurately reflects the active filter combination.

**Indexing in progress:**
The project list may be incomplete. The post-indexation refresh corrects this. No special handling needed.

---

## 7. Tests & Verification

### Integration tests (cargo test)

- **DB project queries** -- `load_projects(db_path, &tools)` returns projects sorted by `last_updated DESC` with correct counts. Test with existing fixtures.
- **Dynamic counts** -- verify count changes when filtering by AI assistant. Example: a project with 3 Claude sessions + 2 OpenCode sessions -> count = 3 when only Claude is checked.
- **Zero-count visibility** -- verify a project remains in the sidebar with badge `0` when filtered out by the active AI assistant filter.
- **Unassigned filter** -- `load_sessions(db, tools, Unassigned)` returns only sessions with `project_id IS NULL`.
- **Project filter** -- `load_sessions(db, tools, Project(id))` returns only sessions from that project.
- **Cross-filter** -- `Project(id)` + single AI assistant -> correct intersection.
- **Selection preservation** -- verify the selected project stays selected when its count drops to `0` after an AI assistant toggle.

### Manual verification (flatpak with fixtures)

- `flatpak-builder --run ... sessions-chronicle --sessions-dir tests/fixtures`
- Verify: project selection filters sessions, dynamic badges update, zero-count project rows remain visible, `Unassigned` appears when present in DB, scroll works with multiple projects, keyboard navigation (Up/Down), re-selecting `All Sessions` resets the filter.

### CI (unchanged)

- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`
