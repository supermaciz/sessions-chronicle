# Plan: GitHub Actions Release Workflow for Stable Flatpak

## Context

The project has a Devel Flatpak manifest and CI that builds it on every push/PR, but no stable Flatpak build and no release automation. We need a GitHub Actions workflow that builds a stable (non-Devel) Flatpak bundle when a release is published on GitHub, and attaches it as a release asset.

## Cleanup

- Remove `build-aux/com.belmoussaoui.GtkRustTemplate.json` — residual from the GTK Rust template, no longer relevant.

## Files to Create

### 1. `build-aux/io.github.supermaciz.sessionschronicle.json` — Stable Flatpak manifest

Derived from the Devel manifest with these differences:

| Field | Devel | Stable |
|-------|-------|--------|
| `id` | `...sessionschronicle.Devel` | `...sessionschronicle` |
| `RUST_LOG` env | `sessions_chronicle=debug` | removed |
| `RUST_BACKTRACE` env | `1` | removed |
| `G_MESSAGES_DEBUG` env | `none` | removed (no-op in release: GLib debug messages are off by default) |
| `--filesystem` | `host` | `home:ro` |
| `--talk-name` | `org.freedesktop.Flatpak` | kept (required for `flatpak-spawn --host`) |
| `config-opts` | `["-Dprofile=development"]` | omitted (Meson default = `'default'` = release build) |
| `run-tests` | `true` | `true` (kept as safety net — build fails if tests fail) |

**Filesystem rationale:** `home:ro` gives read-only access to `~/` so the app can discover Claude session files. The app writes its SQLite database under `~/.var/app/<app-id>/data/`, which the Flatpak sandbox already grants write access to without any `--filesystem` permission.

Everything else stays the same (runtime gnome-49, SDK extensions, mold linker, build-options).

### 2. `.github/workflows/release.yml` — Release workflow

```yaml
on:
  release:
    types: [published]

name: Release

jobs:
  flatpak:
    name: Flatpak Bundle
    runs-on: ubuntu-latest
    container:
      image: ghcr.io/flathub-infra/flatpak-github-actions:gnome-49
      options: --privileged
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v6

      - name: Set version from tag
        id: version
        run: |
          TAG="${{ github.event.release.tag_name }}"
          VERSION="${TAG#v}"
          SAFE_VERSION="$(printf '%s' "$VERSION" | sed 's#[^0-9A-Za-z._-]#-#g')"
          [ -n "$SAFE_VERSION" ] || SAFE_VERSION="${{ github.event.release.id }}"
          echo "version=$SAFE_VERSION" >> "$GITHUB_OUTPUT"

      - name: Build Flatpak
        uses: flatpak/flatpak-github-actions/flatpak-builder@v6
        with:
          bundle: sessions-chronicle-${{ steps.version.outputs.version }}.flatpak
          manifest-path: build-aux/io.github.supermaciz.sessionschronicle.json
          cache-key: flatpak-builder-${{ github.sha }}
          upload-artifact: false

      - name: Generate SHA256 checksum
        run: |
          sha256sum sessions-chronicle-${{ steps.version.outputs.version }}.flatpak \
            > sessions-chronicle-${{ steps.version.outputs.version }}.flatpak.sha256

      - name: Upload bundle to release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            sessions-chronicle-${{ steps.version.outputs.version }}.flatpak
            sessions-chronicle-${{ steps.version.outputs.version }}.flatpak.sha256
```

Key decisions:
- **Trigger:** `release: published` fires for releases and pre-releases (not drafts)
- **Tag format:** Strips optional `v` prefix, sanitizes for filename safety, and falls back to release ID if needed
- **Upload:** `softprops/action-gh-release@v2` attaches the bundle to the existing release
- **Checksum:** SHA256 checksum attached alongside the bundle for integrity verification
- **No artifact:** `upload-artifact: false` avoids redundant storage; the release asset is the canonical location
- **Tests:** release workflow only builds/uploads; tests remain in CI on push/PR
- **Permissions:** `contents: write` needed by softprops to attach assets

## Critical Files (reference)

- `build-aux/io.github.supermaciz.sessionschronicle.Devel.json` — template for stable manifest
- `.github/workflows/ci.yml` — reference for action versions and container image
- `meson_options.txt` — confirms default profile is `'default'`
- `meson.build` + `src/meson.build` — profile-conditional logic (app ID, release mode)

## Verification

1. Validate stable manifest JSON: `python3 -m json.tool build-aux/io.github.supermaciz.sessionschronicle.json`
2. Local Flatpak build: `flatpak-builder --user flatpak_app build-aux/io.github.supermaciz.sessionschronicle.json --force-clean`
3. Verify release mode: check Meson output for `--release` flag in cargo build
4. After push: create a test release on GitHub and verify the workflow triggers, builds, and attaches the `.flatpak` bundle and `.sha256` checksum
