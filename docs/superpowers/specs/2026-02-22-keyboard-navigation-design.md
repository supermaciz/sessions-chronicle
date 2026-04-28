# Keyboard Navigation Design

**Date:** 2026-02-22
**Status:** Implemented [#38](https://github.com/supermaciz/sessions-chronicle/pull/38)

## Problem

Two issues to address:

1. **Bug:** Escape does not close the SearchBar. The app-level `EscapeAction`
   accelerator intercepts Escape before `gtk::SearchBar` can handle it natively.
   The `AppMsg::Escape` handler only checks for inspector pane and detail view,
   not search mode.

2. **Feature gap:** No keyboard navigation in the session list. The `ListBox`
   uses `SelectionMode::None`, so arrow keys have no visual effect and there is
   no way to browse sessions without a mouse.

## Design

### Fix: Escape closes SearchBar

Add SearchBar dismissal as the highest priority in the `AppMsg::Escape` handler.

**Revised Escape priority chain:**

1. Close SearchBar (if `search_visible` is true)
2. Close inspector pane (if open in detail view)
3. Navigate back to session list (if in detail view)
4. No-op (if none of the above apply)

Implementation: in `AppMsg::Escape` handler, check `self.search_visible` first
and disable search mode before any pane/navigation logic.

When search mode is disabled, focus restoration must be context-aware:

- If `detail_visible == true`: keep focus in detail view (do not force list focus)
- If `detail_visible == false`: restore focus to session list selection

### Feature: Keyboard navigation in session list

**Change `ListBox` selection mode** from `SelectionMode::None` to
`SelectionMode::Single`.

This gives us natively from GTK4:

- Arrow Up/Down: move selection between rows
- Enter: activate selected row (triggers `row-activated`, already wired)
- Home/End: jump to first/last row
- Page Up/Page Down: scroll by page

**Focus management:**

All focus scenarios share a single fallback rule: if no row is currently
selected and the list is non-empty, select the first row. This logic lives
entirely in `EnsureSelection` and is never duplicated at call sites.

Call sites that send `EnsureSelection` (then optionally `FocusSelection`):

- **App launch / first non-empty load:** `EnsureSelection` to guarantee an
  initial selection.
- **Return from detail view (Escape/back):** `EnsureSelection` + `FocusSelection`
  to restore keyboard focus to the current (or first) row.
- **SearchBar close in list view:** `EnsureSelection` + `FocusSelection` (same
  sequence as above).
- **List reload / filter / search change:** `EnsureSelection` to keep a valid
  selection after the list contents change.

This requires explicit `SessionList` messages so `App` does not reach into GTK
row widgets directly.

Recommended additions to `SessionListMsg`:

- `EnsureSelection` — if no row is selected and the list is non-empty, select
  the first row. This is the **single source of truth** for the fallback rule.
- `FocusSelection` — grab keyboard focus on the currently selected row (assumes
  a valid selection already exists; callers send `EnsureSelection` first).

**Type-to-search coexistence:** Arrow keys do not trigger the SearchBar's
key capture, so there is no conflict. Letter keys still open search mode as
before.

### Update shortcuts dialog

Add a "Navigation" section to the `ShortcutsDialog` in `shortcuts.rs`:

| Shortcut | Description |
|----------|-------------|
| Up/Down | Select previous/next session |
| Enter | Open selected session |
| Escape | Close search / Close inspector / Go back |

## Implementation notes

- Keep `Type-to-search` unchanged (`SearchBar::set_key_capture_widget(main_window)`).
- Keep `row-activated` flow unchanged for opening sessions.
- Prefer handling selection/focus inside `SessionList` to avoid brittle widget
  references in `App`.
- Trigger `EnsureSelection` after list reloads and after initial population.
- Trigger `FocusSelection` when leaving detail view and when search closes in list view.

## Files to modify

- `src/app.rs` — Escape handler, focus management
- `src/ui/session_list.rs` — SelectionMode change + selection/focus messages
- `src/ui/modals/shortcuts.rs` — Navigation section
- `tests/` — add/update integration tests for Escape priority and list focus behavior

## What does NOT change

- Type-to-search behavior (key capture on main window)
- `row-activated` signal and `SessionListOutput::SessionSelected` flow
- Click-to-open behavior
- Right-click context menu
