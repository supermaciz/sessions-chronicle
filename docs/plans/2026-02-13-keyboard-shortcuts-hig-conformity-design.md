# Keyboard Shortcuts — GNOME HIG Conformity

**Date:** 2026-02-13
**Status:** Accepted

## Goal

Bring keyboard shortcuts into conformity with the [GNOME HIG keyboard guidelines](https://developer.gnome.org/hig/guidelines/keyboard.html) and transform the search into a proper toggle SearchBar with type-to-search.

## Current State

Only 2 shortcuts exist:
- `Ctrl+Q` — Quit
- `F9` — Toggle utility pane

The search field is inside a `gtk::SearchBar` controlled by a `ToggleButton`, but lacks `Ctrl+F` accelerator and type-to-search. The `ShortcutsDialog` is hardcoded and incomplete.

## Design

### Shortcuts to add

| Shortcut | Action | Action name | Notes |
|----------|--------|-------------|-------|
| `Ctrl+F` | Toggle search | `win.toggle-search` | Opens/closes SearchBar, gives focus to SearchEntry |
| `Escape` | Close search | (SearchBar built-in) | Closes SearchBar when it has focus |
| `Ctrl+?` | Show keyboard shortcuts | `win.show-help-overlay` | Action exists, add accelerator |
| `Ctrl+,` | Preferences | `win.preferences` | Action exists, add accelerator |

Existing shortcuts (no change):
- `F9` — Toggle utility pane (`win.toggle-pane`)
- `Ctrl+Q` — Quit (`win.quit`)

`F10` for menu is handled automatically by GTK for `MenuButton` widgets.

### SearchBar transformation

The `gtk::SearchBar` already exists in the view. Changes:
1. Call `search_bar.set_key_capture_widget(Some(&main_window))` to enable type-to-search — typing anywhere (when no other input has focus) will open the SearchBar and start filtering.
2. Register `Ctrl+F` as accelerator for `win.toggle-search` action.
3. When SearchBar closes (search_visible becomes false), clear the search query so the list resets.

### ShortcutsDialog update

Update `src/ui/modals/shortcuts.rs` to list all shortcuts organized in groups:
- **General:** Keyboard Shortcuts (`Ctrl+?`), Preferences (`Ctrl+,`), Quit (`Ctrl+Q`)
- **Search:** Toggle search (`Ctrl+F`)
- **View:** Toggle utility pane (`F9`)

### Files impacted

| File | Change |
|------|--------|
| `src/app.rs` | Add `ToggleSearchAction`, accelerators for `Ctrl+F`/`Ctrl+?`/`Ctrl+,`. Add `set_key_capture_widget` on SearchBar. Clear query on search close. |
| `src/ui/modals/shortcuts.rs` | Update with all shortcuts in organized groups |
| `data/resources/ui/shortcuts.ui` | Keep in sync or remove if fully replaced by Rust code |

No new files needed.
