# GNOME Desktop Integration

> **Status (2026-10-23):** The major integrations are implemented. Since v0.10.0, Sessions Chronicle ships a GNOME Shell search provider, session deep links (the `open-session` app action), and D-Bus activation. This document describes the current state and lists the remaining ideas as future work.

## Implemented integrations

### Baseline GNOME integration

- **App ID + build profiles**: Meson computes `application_id` / `profile` and generates `src/config.rs` (`src/meson.build`). The app is identified as `dev.maciz.sessionschronicle` (and `…Devel` in dev builds).
- **Desktop entry**: Installed from `data/dev.maciz.sessionschronicle.desktop.in.in` via `data/meson.build`. It carries `DBusActivatable=true`, so GNOME Shell launches the app over D-Bus, and it makes the app show up in the app grid and overview search (as an app).
- **AppStream (GNOME Software metadata)**: Installed from `data/dev.maciz.sessionschronicle.metainfo.xml.in.in` via `data/meson.build`, with screenshots, homepage URL (`https://sessions-chronicle.maciz.dev`), bugtracker, and full release history.
- **Icons**: Installed by `data/icons/meson.build` so GNOME Shell has the right app icon (including a symbolic icon).
- **GSettings schema**: Installed from `data/dev.maciz.sessionschronicle.gschema.xml.in` via `data/meson.build`. Used for window size/maximized state (`src/app/mod.rs`) and preferences (`src/ui/modals/preferences.rs`).
- **Flatpak manifests**: `build-aux/dev.maciz.sessionschronicle.json` (stable) and `build-aux/dev.maciz.sessionschronicle.Devel.json` (development) build on `org.gnome.Platform`.

### GNOME Shell search provider (Activities Overview results)

Implemented as a dedicated binary in `crates/search-provider/` (`sessions-chronicle-search-provider`):

- Implements `org.gnome.Shell.SearchProvider2` over zbus (`crates/search-provider/src/interface.rs`), querying the same SQLite FTS index as the app in a background worker (`crates/search-provider/src/db_worker.rs`).
- Registration keyfile installed under `share/gnome-shell/search-providers` from `data/dev.maciz.sessionschronicle-search-provider.ini.in`; D-Bus service file from `data/dev.maciz.sessionschronicle.SearchProvider.service.in` (both wired in `data/meson.build`).
- On activation, the provider calls the app's `open-session` action over D-Bus (`crates/search-provider/src/activation.rs`).

Discovery subtleties — why a `~/.local` install is invisible to Shell while still answering D-Bus calls, and which binary answers activation when several are installed — are documented in [DEVELOPMENT_WORKFLOW.md](DEVELOPMENT_WORKFLOW.md#gnome-search-provider-why-a-local-install-is-invisible).

### Session deep links and D-Bus activation

- The private `open-session` GApplication action (registered in `src/main.rs`, handled in `src/app/mod.rs`) opens an indexed top-level session by ID. The same command works whether the app is closed (cold D-Bus activation) or already running.
- Profile-specific bus names keep development and stable installs disjoint: different keyfile names, `DesktopId`, bus names, and object paths, toggled independently in Settings ▸ Search. The app service file is `data/dev.maciz.sessionschronicle.service.in`.
- Testing procedure, including `gapplication action … open-session` with GVariant quoting: [DEVELOPMENT_WORKFLOW.md](DEVELOPMENT_WORKFLOW.md#testing-session-deep-links).
- `open-session` is an internal protocol for components shipped with Sessions Chronicle, not a stable public API. Instances started with `--sessions-dir` are non-unique and do not receive deep-link actions.

## Future work (not implemented)

### Notifications

For "index finished", "new sessions found", etc., use `gio::Notification` via `g_application_send_notification()`.

This integrates with GNOME's notification center and respects user settings.

### Portals + tighter Flatpak permissions

The stable manifest already uses `--filesystem=home:ro`, but the development manifest still uses `--filesystem=host` (`build-aux/dev.maciz.sessionschronicle.Devel.json`), which is very broad.

For a GNOME-friendly sandboxed app:

- Prefer asking the user to pick a sessions directory via GTK's file/directory picker (portal-backed automatically in Flatpak).
- Store that chosen path in GSettings (the schema machinery is already in place).
- Narrow the remaining filesystem permissions further.

Related background:

- Flatpak desktop integration: https://docs.flatpak.org/en/latest/desktop-integration.html

### MIME types / file associations

If "Open With Sessions Chronicle" for e.g. `.jsonl` is wanted:

- Add `MimeType=` to the desktop entry (its `Exec=` currently has no `%U`/`%f`)
- Install a `shared-mime-info` XML definition
- Implement `open()` handling in the app

---

**Last Updated**: 2026-10-23
