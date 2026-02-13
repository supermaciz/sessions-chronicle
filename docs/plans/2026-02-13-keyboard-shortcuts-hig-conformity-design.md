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
| `Ctrl+F` | Show/focus search | `win.show-search` | Opens SearchBar and focuses SearchEntry. If already visible, focuses SearchEntry (does not close). |
| `Escape` | Close search | (SearchBar built-in) | Closes SearchBar only when SearchBar/SearchEntry has focus (not when a dialog is open or detail view is focused). Handled natively by `gtk::SearchBar`. |
| `Ctrl+?` | Show keyboard shortcuts | `win.show-help-overlay` | Add accelerator explicitly. GTK accelerator string: `<Control>question`. |
| `Ctrl+,` | Preferences | `win.preferences` | Add accelerator explicitly. |

Existing shortcuts (unchanged):
- `F9` — Toggle utility pane (`win.toggle-pane`)
- `Ctrl+Q` — Quit (`win.quit`)

`F10` for menu is handled automatically by GTK for `MenuButton` widgets.

### SearchBar behavior

The `gtk::SearchBar` already exists in the view. Required behavior:

1. Call `search_bar.set_key_capture_widget(Some(&main_window))` to enable type-to-search.
2. Register `Ctrl+F` as accelerator for `win.show-search` (replaces the old `win.toggle-search` action). The `show-search` action calls `search_bar.set_search_mode(true)` unconditionally — it never closes.
3. Synchronize model state from SearchBar (`search-mode-enabled`) so close/open done by GTK (including `Escape` and type-to-search) is reflected in `search_visible`. Use `connect_search_mode_enabled_notify` on the SearchBar and gate the model update: only send a message when `search_bar.is_search_mode() != model.search_visible` to prevent infinite update loops.
4. When SearchBar closes, clear the search query so list/detail filtering resets.
5. When SearchBar opens, focus the `SearchEntry`.

#### Header bar ToggleButton coordination

The existing search `ToggleButton` in the header bar (currently wired to `AppMsg::ToggleSearch`) must stay in sync with the SearchBar's state. Bind the ToggleButton's `active` property bidirectionally to the SearchBar's `search-mode-enabled` property. This way:
- Clicking the ToggleButton still toggles the SearchBar open/closed.
- `Ctrl+F` (show-only) and `Escape` (close) update the SearchBar state, which automatically reflects in the ToggleButton.
- The `AppMsg::ToggleSearch` message can be removed — the ToggleButton no longer needs a manual signal handler since the bidirectional binding handles everything.

### ShortcutsDialog update

Update `src/ui/modals/shortcuts.rs` with complete grouped sections:
- **General:** Keyboard Shortcuts (`Ctrl+?`), Preferences (`Ctrl+,`), Quit (`Ctrl+Q`)
- **Search:** Search (`Ctrl+F`)
- **View:** Toggle utility pane (`F9`)

Use action-bound entries (`action-name`) where possible so displayed shortcuts follow registered accelerators. Note: verify that `adw::ShortcutsItem` supports the `action-name` property — the current code uses `ShortcutsItem::new(title, accelerator)` which sets the accelerator string directly. If `action-name` is not available, continue using explicit accelerator strings but keep them consistent with the registered accelerators.

### Source-of-truth decision

Keep the Rust `adw::ShortcutsDialog` and remove the legacy GTK help overlay resource to avoid duplicated shortcut definitions.

### Files impacted

| File | Change |
|------|--------|
| `src/app.rs` | Replace `win.toggle-search` with `win.show-search` action, register accelerators for `Ctrl+F`/`Ctrl+?`/`Ctrl+,`, set SearchBar key capture widget, bind ToggleButton to SearchBar's `search-mode-enabled` bidirectionally, add `connect_search_mode_enabled_notify` with guard to sync model, clear query on close, focus entry on open. Remove `AppMsg::ToggleSearch` if ToggleButton binding replaces it. |
| `src/ui/modals/shortcuts.rs` | Update to full grouped shortcuts list, using action-bound items where applicable. |
| `data/resources/resources.gresource.xml` | Remove legacy `gtk/help-overlay.ui` resource alias. |
| `data/resources/ui/shortcuts.ui` | Remove legacy GTK shortcuts overlay file. |

No new files are required.

## Testing Checklist

Manual validation scenarios (run via Flatpak):

- [ ] **Ctrl+F** opens SearchBar and focuses SearchEntry.
- [ ] **Ctrl+F** when SearchBar is already open re-focuses SearchEntry (does not close it).
- [ ] **Type-to-search:** typing characters with no widget focused opens SearchBar and starts filtering.
- [ ] **Escape** closes SearchBar when SearchEntry has focus; does nothing when a dialog is open.
- [ ] **ToggleButton** opens/closes SearchBar; button state stays in sync with SearchBar state after Ctrl+F and Escape.
- [ ] Closing SearchBar (via Escape or ToggleButton) clears the search query and resets filtering.
- [ ] **Ctrl+?** opens the keyboard shortcuts dialog.
- [ ] **Ctrl+,** opens the preferences window.
- [ ] **Ctrl+Q** quits the application.
- [ ] **F9** toggles the utility pane.
- [ ] **F10** opens the primary menu (if MenuButton present).
- [ ] Shortcuts dialog lists all shortcuts with correct labels and grouping.
