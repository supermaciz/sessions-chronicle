# Keyboard Shortcuts — GNOME HIG Conformity

**Date:** 2026-02-13
**Status:** Accepted (revised)

## Goal

Bring keyboard shortcuts into conformity with the [GNOME HIG keyboard guidelines](https://developer.gnome.org/hig/guidelines/keyboard.html) and implement a proper `gtk::SearchBar` flow with type-to-search.

## Current State

Only 2 shortcuts are currently registered:
- `Ctrl+Q` — Quit
- `F9` — Toggle utility pane

Search UI already uses `gtk::SearchBar`, but currently lacks:
- `Ctrl+F` accelerator
- Type-to-search (`set_key_capture_widget`)
- Explicit model synchronization for SearchBar close events (including `Escape`)

Keyboard shortcut UI is also split between two implementations:
- Rust-based `adw::ShortcutsDialog` (`src/ui/modals/shortcuts.rs`)
- Legacy GTK help overlay resource (`data/resources/ui/shortcuts.ui`)

This creates drift risk and should be reduced to one source of truth.

## Design

### Shortcut map

| Shortcut | Action | Action name | Notes |
|----------|--------|-------------|-------|
| `Ctrl+F` | Show/focus search | `win.toggle-search` | Opens SearchBar and focuses SearchEntry. If already visible, focuses SearchEntry (does not close). |
| `Escape` | Close search | (SearchBar built-in) | Closes SearchBar when search UI has focus. |
| `Ctrl+?` | Show keyboard shortcuts | `win.show-help-overlay` | Add accelerator explicitly. |
| `Ctrl+,` | Preferences | `win.preferences` | Add accelerator explicitly. |

Existing shortcuts (unchanged):
- `F9` — Toggle utility pane (`win.toggle-pane`)
- `Ctrl+Q` — Quit (`win.quit`)

`F10` for menu is handled automatically by GTK for `MenuButton` widgets.

### SearchBar behavior

The `gtk::SearchBar` already exists in the view. Required behavior:

1. Call `search_bar.set_key_capture_widget(Some(&main_window))` to enable type-to-search.
2. Register `Ctrl+F` as accelerator for `win.toggle-search`.
3. Synchronize model state from SearchBar (`search-mode-enabled`) so close/open done by GTK (including `Escape` and type-to-search) is reflected in `search_visible`.
4. When SearchBar closes, clear the search query so list/detail filtering resets.
5. When SearchBar opens, focus the `SearchEntry`.

### ShortcutsDialog update

Update `src/ui/modals/shortcuts.rs` with complete grouped sections:
- **General:** Keyboard Shortcuts (`Ctrl+?`), Preferences (`Ctrl+,`), Quit (`Ctrl+Q`)
- **Search:** Search (`Ctrl+F`)
- **View:** Toggle utility pane (`F9`)

Use action-bound entries (`action-name`) where possible so displayed shortcuts follow registered accelerators.

### Source-of-truth decision

Keep the Rust `adw::ShortcutsDialog` and remove the legacy GTK help overlay resource to avoid duplicated shortcut definitions.

### Files impacted

| File | Change |
|------|--------|
| `src/app.rs` | Add `ToggleSearchAction`, register accelerators for `Ctrl+F`/`Ctrl+?`/`Ctrl+,`, set SearchBar key capture widget, sync SearchBar open/close state with model, clear query on close, focus entry on open. |
| `src/ui/modals/shortcuts.rs` | Update to full grouped shortcuts list, using action-bound items where applicable. |
| `data/resources/resources.gresource.xml` | Remove legacy `gtk/help-overlay.ui` resource alias. |
| `data/resources/ui/shortcuts.ui` | Remove legacy GTK shortcuts overlay file. |

No new files are required.
