# Design: Pinned as Navigation Target (Issue #109)

**Date:** 2026-04-04  
**Issue:** [#109 — feat: favorite sessions for quick revisit](https://github.com/supermaciz/sessions-chronicle/issues/109)  
**Prior design:** [2026-04-02-favorite-sessions-design.md](2026-04-02-favorite-sessions-design.md)  
**Exploration:** [2026-04-02-favorite-sessions-exploration.md](2026-04-02-favorite-sessions-exploration.md) (Post-Implementation Review section)  
**Branch:** `feat/favorite-sessions` (PR #115 — Proposal F implementation)  
**Type:** Redesign of sidebar navigation model

## Problem

The initial Proposal F implementation treats Pinned as a composable boolean
filter (`pinned_only: bool`) that composes with `ProjectFilter` via AND.
This creates confusing empty states: the sidebar can point to a project with
zero pinned sessions while pinned sessions exist in other projects. The user
sees an empty list despite having pins.

Pinned is functionally a **navigation destination** ("show me my bookmarks"),
not a **filter facet** ("narrow this view"). The current implementation
treats it as the latter.

## Scope

This design modifies the **sidebar navigation model, filtering pipeline,
and pinned-specific empty-state semantics**. Everything else from the
Proposal F implementation is stable:

**Unchanged (already on `feat/favorite-sessions`):**

- Schema v8 (`pinned_at INTEGER NULL` on `sessions`)
- `Session.pinned_at: Option<DateTime<Utc>>`
- `toggle_pin()` atomic DB operation
- Pin icon suffix on session rows (`view-pin-symbolic`, 16px)
- CSS `.pinned-row` left border (`2px solid @accent_color`)
- Context menu "Pin" / "Unpin" action
- `Ctrl+D` shortcut routing (`AppMsg::TogglePinShortcutRequested`)
- Pin toggle button in detail header bar
- Toast notification on pin/unpin
- `count_pinned_sessions()` function

---

## 1. Data Model

### ProjectFilter

Add one variant:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProjectFilter {
    #[default]
    AllSessions,
    Pinned,          // new
    Project(i64),
    Unassigned,
}
```

`Pinned` sits between `AllSessions` and `Project` in the enum to mirror
the sidebar visual order.

### Removals

The `pinned_only: bool` field is removed from the current filter plumbing:

| Location | Field removed |
|---|---|
| `FilterState` (`src/app/types.rs`) | `pinned_only: bool` |
| `SidebarOutput::FiltersChanged` (`src/ui/sidebar.rs`) | `pinned_only: bool` |
| `SidebarMsg::PinnedOnlyToggled` (`src/ui/sidebar.rs`) | entire variant |
| `SessionList` state / `SessionListMsg::SetFilters` (`src/ui/session_list.rs`) | `pinned_only: bool` |
| DB query helpers (`src/database/mod.rs`) | `pinned_only` params on filter-loading/search functions |

### FilterState (after)

```rust
pub(super) struct FilterState {
    pub(super) tools: Vec<AiAssistant>,
    pub(super) project_filter: ProjectFilter,
}
```

### SidebarOutput (after)

```rust
pub enum SidebarOutput {
    FiltersChanged {
        tools: Vec<AiAssistant>,
        project_filter: ProjectFilter,
    },
}
```

`ProjectFilter::Pinned` now carries the "show only pinned" intent that
`pinned_only: bool` used to carry, but as a navigation destination
mutually exclusive with project selection.

---

## 2. Database Queries

### Filter clause

The existing `project_clause` match gains one arm. The separate
`pinned_clause` variable is removed entirely.

```rust
let project_clause = match project_filter {
    ProjectFilter::AllSessions => String::new(),
    ProjectFilter::Pinned => " AND pinned_at IS NOT NULL".to_string(),
    ProjectFilter::Project(_) => " AND project_id = ?".to_string(),
    ProjectFilter::Unassigned => " AND project_id IS NULL".to_string(),
};
```

`ProjectFilter::Pinned` has no bound parameter (like `AllSessions` and
`Unassigned`), so the parameter binding logic is unchanged.

### Functions impacted

| Function | Change |
|---|---|
| `load_sessions_for_filter()` | Remove `pinned_only` param, add `Pinned` arm to match |
| `search_sessions_with_query()` | Same |
| `search_sessions_for_filter()` | Remove `pinned_only` param (no longer forwarded) |
| `count_pinned_sessions()` | Unchanged — used for badge count independently |

### App/session-list plumbing

`SessionListMsg::SetFilters` now carries only:

```rust
SetFilters {
    tools: Vec<AiAssistant>,
    project_filter: ProjectFilter,
}
```

`SessionList` no longer stores `pinned_only: bool`. Empty-state copy that was
previously keyed on `pinned_only` now keys on
`project_filter == ProjectFilter::Pinned`.

This keeps pinned semantics in one place: the navigation target itself.

### Composition with AI Assistants

`ProjectFilter::Pinned` composes with the `tools: &[AiAssistant]` filter
via AND, exactly like all other `ProjectFilter` variants. Selecting Pinned
with only "Claude Code" checked shows pinned Claude Code sessions only.

---

## 3. Sidebar UI

### Removed

- `gtk::Label` with "Pinned" heading text
- `pinned_list` ListBox (`gtk::ListBox` with `.pinned-sidebar-list` CSS)
- `gtk::Separator` below the pinned section
- `gtk::CheckButton` + `adw::ActionRow` inside `pinned_list`
- CSS class `.pinned-sidebar-list` from `style.css`

### Added

One `adw::ActionRow` inserted at **position 1** in the existing
`projects_list` ListBox (after "All Sessions", before project rows):

```rust
let pinned_row = adw::ActionRow::builder()
    .title("Pinned")
    .build();

let pin_icon = gtk::Image::from_icon_name("view-pin-symbolic");
pinned_row.add_prefix(&pin_icon);

let pinned_count_label = gtk::Label::new(Some("0"));
pinned_count_label.add_css_class("project-badge");
pinned_count_label.set_valign(gtk::Align::Center);
pinned_count_label.set_height_request(29);
pinned_row.add_suffix(&pinned_count_label);

pinned_row.set_widget_name("pinned");
```

### Selection handling

The `projects_list` uses `SelectionMode::Single`. The existing
`connect_row_selected` handler matches on `widget_name()` to determine
the `ProjectFilter`. Add one arm:

```rust
"pinned" => ProjectFilter::Pinned,
```

GTK enforces mutual exclusivity — selecting Pinned deselects any project
row, and vice versa. No manual state reconciliation needed.

`project_filter_key()` / `project_filter_from_key()` must also gain the
`Pinned` mapping so row rebuilds and row-selected events stay symmetric.

### Badge count

`count_pinned_sessions()` remains owned by the app-level sidebar data load
path (`load_sidebar_project_data()` / `SidebarProjectData`). The sidebar
continues to receive `pinned_count` via `SidebarMsg::ProjectsLoaded` and
updates only the label widget.

`rebuild_project_rows()` should not query the database directly. The sidebar
stays a pure view over already-computed counts.

### Behavior at count 0

The Pinned row is **always visible**, even when count is 0. The badge
shows "0". The row is selectable — clicking it shows an empty session
list, consistent with how empty projects and "Unassigned" at 0 behave.

### Default selection at launch

"All Sessions" (position 0) remains the default selection at launch.
No GSettings change. Filter state persistence is out of scope.

### Filter retention after sidebar refresh

`retained_project_filter()` must preserve `ProjectFilter::Pinned` unchanged:

```rust
ProjectFilter::Pinned => ProjectFilter::Pinned,
```

Unlike project IDs or `Unassigned`, the Pinned destination does not depend on
project list membership or unassigned visibility, so it should never collapse
back to `AllSessions` during `ProjectsLoaded`.

---

## 4. Sidebar Visual Order

```
AI Assistants
  [x] Claude Code
  [x] OpenCode
  [x] Codex
  [x] Mistral Vibe

Projects
    All Sessions    (42)
  📌 Pinned           (3)     ← new row, position 1
    my-project      (12)
    other-project    (8)
    Unassigned       (2)
```

The Pinned row uses `view-pin-symbolic` as prefix icon and the same
`.project-badge` styled count suffix as project rows. No separator
between Pinned and project rows.

---

## 5. Tests

### Adapted tests

| Test | Change |
|---|---|
| `load_sessions_for_filter_respects_pinned_only()` | Rename. Pass `ProjectFilter::Pinned` instead of `pinned_only: true` |
| `search_sessions_for_filter_respects_pinned_only()` | Same |
| `pinned_sidebar_toggle_emits_filters_changed_with_pinned_only()` | Remove — replaced by row selection test |
| `projects_loaded_updates_pinned_count_badge()` | Adapt — verify badge on `projects_list` row |
| Session-list pinned empty-state tests | Adapt to `ProjectFilter::Pinned` instead of a bool flag |

### Unchanged tests

| Test | Why unchanged |
|---|---|
| `toggle_pin_flips_state_and_returns_new_state()` | Tests `toggle_pin()` DB, no filter involvement |
| `count_pinned_sessions_respects_tool_filter()` | Function unchanged |

### New test

```rust
#[test]
fn pinned_filter_returns_sessions_across_all_projects() {
    // Setup: pin 2 sessions in 2 different projects
    // Action: load_sessions_for_filter with ProjectFilter::Pinned
    // Assert: both sessions returned (no project_id filtering)
}
```

This test validates the core reason for the redesign — Pinned is a
cross-project navigation target, not a per-project facet.

Additional UI/plumbing tests to add:

- `retained_project_filter_preserves_pinned_selection()`
- sidebar row-selection test for `"pinned" -> ProjectFilter::Pinned`
- `project_filter_key_round_trips_pinned()`
- pinned empty-state copy test keyed on `ProjectFilter::Pinned`

---

## 6. Net Impact Summary

| Metric | Change |
|---|---|
| Lines added (est.) | ~20 (new row, match arm, test) |
| Lines removed (est.) | ~50 (pinned section, bool plumbing) |
| New widgets | 0 (reuses existing ListBox) |
| Removed widgets | 3 (Label, ListBox, Separator) |
| New GSettings keys | 0 |
| Schema migration | None (v8 unchanged) |
| Files touched | ~8 (types, sidebar, database, session_list, app handlers, tests) |
