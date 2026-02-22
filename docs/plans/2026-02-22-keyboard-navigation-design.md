# Keyboard Navigation Design

**Date:** 2026-02-22
**Status:** Approved

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
and call `set_search_mode(false)` on the SearchBar widget via a new message or
direct widget access.

### Feature: Keyboard navigation in session list

**Change `ListBox` selection mode** from `SelectionMode::None` to
`SelectionMode::Single`.

This gives us natively from GTK4:

- Arrow Up/Down: move selection between rows
- Enter: activate selected row (triggers `row-activated`, already wired)
- Home/End: jump to first/last row
- Page Up/Page Down: scroll by page

**Focus management:**

- On app launch: give focus to the first row in the session list
- On return from detail view (Escape/back): restore focus to the previously
  selected row
- On SearchBar close: return focus to the session list

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

## Files to modify

- `src/app.rs` — Escape handler, focus management
- `src/ui/session_list.rs` — SelectionMode change
- `src/ui/modals/shortcuts.rs` — Navigation section

## What does NOT change

- Type-to-search behavior (key capture on main window)
- `row-activated` signal and `SessionListOutput::SessionSelected` flow
- Click-to-open behavior
- Right-click context menu
