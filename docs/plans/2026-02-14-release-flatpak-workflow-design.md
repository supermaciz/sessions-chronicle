# Plan: GitHub Actions Release Workflow for Stable Flatpak

## Context

The project has a Devel Flatpak manifest and CI that builds it on every push/PR, but no stable Flatpak build and no release automation. We need a GitHub Actions workflow that builds a stable (non-Devel) Flatpak bundle when a release is published on GitHub, and attaches it as a release asset.

## Files to Create

### 1. `build-aux/io.github.supermaciz.sessionschronicle.json` — Stable Flatpak manifest

Derived from the Devel manifest with these differences:

| Field | Devel | Stable |
|-------|-------|--------|
| `id` | `...sessionschronicle.Devel` | `...sessionschronicle` |
| `RUST_LOG` env | `sessions_chronicle=debug` | removed |
| `RUST_BACKTRACE` env | `1` | removed |
| `--filesystem` | `host` | `home:ro` |
| `--talk-name` | `org.freedesktop.Flatpak` | removed |
| `config-opts` | `["-Dprofile=development"]` | `[]` (Meson default = `'default'` = release build) |

Everything else stays the same (runtime gnome-49, SDK extensions, mold linker, build-options, `run-tests: true`). `G_MESSAGES_DEBUG=none` is kept as it suppresses noisy GLib messages in production too.

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
          echo "version=$VERSION" >> "$GITHUB_OUTPUT"

      - name: Build Flatpak
        uses: flatpak/flatpak-github-actions/flatpak-builder@v6
        with:
          bundle: sessions-chronicle-${{ steps.version.outputs.version }}.flatpak
          manifest-path: build-aux/io.github.supermaciz.sessionschronicle.json
          cache-key: flatpak-builder-${{ github.sha }}
          upload-artifact: false

      - name: Upload bundle to release
        uses: softprops/action-gh-release@v2
        with:
          files: sessions-chronicle-${{ steps.version.outputs.version }}.flatpak
```

Key decisions:
- **Trigger:** `release: published` fires for releases and pre-releases (not drafts)
- **Tag format:** Handles both `v0.1.0` and `0.1.0` via `${TAG#v}`
- **Upload:** `softprops/action-gh-release@v2` attaches the bundle to the existing release
- **No artifact:** `upload-artifact: false` avoids redundant storage; the release asset is the canonical location
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
4. After push: create a test release on GitHub and verify the workflow triggers, builds, and attaches the `.flatpak` bundle
