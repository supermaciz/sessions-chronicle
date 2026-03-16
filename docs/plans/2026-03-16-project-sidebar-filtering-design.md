# Project Sidebar Filtering: Design

**Parent:** [Project Sidebar Filtering Exploration](2026-03-16-project-sidebar-filtering-exploration.md) — Proposition A
**Issue:** [#66](https://github.com/supermaciz/sessions-chronicle/issues/66)
**Date:** 2026-03-16
**Status:** Validated

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
App (router) ──► SessionListMsg::SetFilters { tools, project_filter }
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

**Flow:**

1. User clicks a project row in the ListBox
2. Sidebar emits `FiltersChanged` with active tools AND project filter
3. App routes to SessionList
4. SessionList executes the SQL query with both filters

Each change (AI assistant toggle OR project selection) emits a complete `FiltersChanged` with both filter axes. No separate messages -- avoids inconsistent intermediate states.

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
    ProjectSelected(ProjectFilter),           // NEW
    ProjectsLoaded(Vec<ProjectInfo>),         // NEW -- loaded from DB
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

Badge counts reflect the active AI assistant filters. The sidebar calls this query on every AI filter change.

```sql
-- Projects with session count filtered by AI assistants
SELECT p.id, p.name, p.path, COUNT(s.id) AS session_count
FROM projects p
INNER JOIN sessions s ON s.project_id = p.id
WHERE s.tool IN (?, ?, ...)          -- active AI filters
  AND s.is_subagent = 0
GROUP BY p.id
HAVING session_count > 0             -- hide projects with no visible sessions
ORDER BY MAX(s.last_updated) DESC    -- most recently active project first

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
│ │ Unassigned     [3] │ │  ← gtk::ListBoxRow, italic, only if count > 0
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
- **"Unassigned":** `gtk::ListBoxRow` with italic label + badge count. Shown only when count > 0.
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

1. **At startup** -- after initial indexing, the App sends `SidebarMsg::ProjectsLoaded(projects)` with project list and counts.
2. **After each indexation** -- when the indexer finishes (new sessions detected), the App reloads projects and resends `ProjectsLoaded`. This updates counts and surfaces new projects.
3. **On each AI assistant toggle** -- the sidebar recalculates dynamic counts internally (lightweight DB query) before emitting `FiltersChanged`.

### Refresh flow

```
App::init()
  -> Indexer finished
  -> App loads projects from DB (with counts filtered by active tools)
  -> SidebarMsg::ProjectsLoaded(Vec<ProjectInfo>)
  -> Sidebar rebuilds the ListBox
```

### Handling selected project after refresh

- If the selected project still exists -> keep the selection
- If the selected project disappeared (count dropped to 0 after AI toggle) -> revert to "All Sessions" and emit `FiltersChanged`
- "Unassigned" disappears if its count drops to 0

### Init change

The sidebar needs `db_path` to execute count queries. Its init changes from `()` to `PathBuf`.

---

## 6. Edge Cases & Error Handling

**No projects in database:**
The Projects section shows only "All Sessions" with the total count. No "Unassigned" row (pointless if everything is unassigned). UX is identical to today.

**All AI assistants unchecked:**
Existing behavior unchanged: session list is empty. All project badges show 0. No project selection change.

**Long project name:**
`adw::ActionRow` truncates the title natively with ellipsis. The subtitle (path) too. The badge remains visible in the suffix.

**Same-name projects (identical basename, different paths):**
The subtitle (path) disambiguates them. Example: two "api" projects with paths `/home/user/work/api` and `/home/user/personal/api`.

**DB error loading projects:**
Log the error, keep the previous list. No crash, no popup.

**Indexing in progress:**
The project list may be incomplete. The post-indexation refresh corrects this. No special handling needed.

---

## 7. Tests & Verification

### Integration tests (cargo test)

- **DB project queries** -- `load_projects(db_path, &tools)` returns projects sorted by `last_updated DESC` with correct counts. Test with existing fixtures.
- **Dynamic counts** -- verify count changes when filtering by AI assistant. Example: a project with 3 Claude sessions + 2 OpenCode sessions -> count = 3 when only Claude is checked.
- **Unassigned filter** -- `load_sessions(db, tools, Unassigned)` returns only sessions with `project_id IS NULL`.
- **Project filter** -- `load_sessions(db, tools, Project(id))` returns only sessions from that project.
- **Cross-filter** -- `Project(id)` + single AI assistant -> correct intersection.
- **Disappeared project** -- a project with 0 sessions after AI filter does not appear in the list.

### Manual verification (flatpak with fixtures)

- `flatpak-builder --run ... sessions-chronicle --sessions-dir tests/fixtures`
- Verify: project selection filters sessions, dynamic badges, "Unassigned" appears/disappears, scroll works with multiple projects, keyboard navigation (Up/Down), re-selecting "All Sessions" resets the filter.

### CI (unchanged)

- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`
