# GNOME Desktop Integration

This repository already has most of the “GNOME desktop integration” plumbing wired up via Meson + Flatpak; what’s left is mostly:

1. Filling in real metadata
2. Adding optional GNOME-specific services (search provider, notifications, portals, etc.)

## What you already have (baseline GNOME integration)

- **App ID + build profiles**: Meson computes `application_id` / `profile` and generates `src/config.rs` (`src/meson.build`). The app is identified as `io.github.supermaciz.sessionschronicle` (and `…Devel` in dev builds).
- **Desktop entry**: Installed from `data/io.github.supermaciz.sessionschronicle.desktop.in.in` via `data/meson.build`. This makes the app show up in GNOME Shell’s app grid, overview search (as an app), etc.
- **AppStream (GNOME Software metadata)**: Installed from `data/io.github.supermaciz.sessionschronicle.metainfo.xml.in.in` via `data/meson.build`. GNOME Software uses this for name/summary/screenshots/releases.
- **Icons**: Installed by `data/icons/meson.build` so GNOME Shell has the right app icon (including a symbolic icon).
- **GSettings schema**: Installed from `data/io.github.supermaciz.sessionschronicle.gschema.xml.in` via `data/meson.build`. The app already uses it for window size/maximized state (`src/app.rs`).
- **Flatpak manifest**: `build-aux/io.github.supermaciz.sessionschronicle.Devel.json` sets you up on `org.gnome.Platform` and ensures the app behaves like a GNOME app when installed.

## The “must do” to look/feel properly integrated

- **Replace template metadata with real content**:
  - Update the desktop file template `data/io.github.supermaciz.sessionschronicle.desktop.in.in` (it still says “Write a GTK + Rust application”).
  - Update the AppStream template `data/io.github.supermaciz.sessionschronicle.metainfo.xml.in.in` (it still references old template project URLs and a template screenshot URL).
- **Make `Exec=` match how users actually run it**:
  - If you want GNOME to be able to “open” things (files/URIs), you’ll typically add `%U` (or `%f/%F`) in `Exec=` and implement `open()` / command-line handling in the app.

## High-value optional GNOME integrations (pick what matches your goals)

### GNOME Shell search provider (Activities Overview results)

This is a great fit for Sessions Chronicle because you already have SQLite FTS.

A search provider is a D-Bus service implementing `org.gnome.Shell.SearchProvider2`, plus a registration file installed under `share/gnome-shell/search-providers`.

- GNOME tutorial: https://developer.gnome.org/documentation/tutorials/search-provider.html
- Typical design:
  - Reuse your existing search/query code; return top hits as result IDs
  - On activation, open the app directly on that session

### Notifications

For “index finished”, “new sessions found”, etc., use `gio::Notification` via `g_application_send_notification()`.

This integrates with GNOME’s notification center and respects user settings.

### Portals + tighter Flatpak permissions

Your current Flatpak dev manifest uses `--filesystem=host` (`build-aux/io.github.supermaciz.sessionschronicle.Devel.json`), which works but is very broad.

For a GNOME-friendly sandboxed app:

- Prefer asking the user to pick a sessions directory via GTK’s file/directory picker (portal-backed automatically in Flatpak).
- Store that chosen path in GSettings (the schema machinery is already in place).
- Then drop `--filesystem=host` in favor of narrower permissions.

Related background:

- Flatpak desktop integration: https://docs.flatpak.org/en/latest/desktop-integration.html

### D-Bus activatable app (single-instance + better launching)

Adding `DBusActivatable=true` in the desktop file and ensuring your app supports “service mode” improves integration with:

- Shell search providers
- File opening
- Single-instance behavior

### MIME types / file associations

If you want “Open With Sessions Chronicle” for e.g. `.jsonl`:

- Add `MimeType=` to the desktop entry
- Install a `shared-mime-info` XML definition
- Implement `open()` handling in the app

---

If you decide which integration you care about most (GNOME Software polish vs Shell search provider vs Flatpak sandboxing), the next step is to translate that into concrete changes in `data/` (desktop/appstream/schema), `build-aux/` (Flatpak permissions), and possibly `src/` (D-Bus, open handling, notifications).
