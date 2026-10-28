---
name: bump-version
description: Use when preparing a Sessions Chronicle release that needs synchronized version metadata updates and Flatpak build verification, especially when release notes are missing and need a reviewed draft.
---

# Bump Version (Sessions Chronicle)

## Overview

Update release metadata consistently for this Meson + Flatpak project.
Core principle: one `new_version`, one approved note, one successful Flatpak build.

## When to Use

- User asks to bump app version (`0.x.y`) for a release.
- Version metadata must stay aligned across `Cargo.toml`, `meson.build`, `src/config.rs.in`, and metainfo releases.
- Release notes are missing and need a generated draft before writing.

## When NOT to Use

- User only wants a local Rust build check (no release metadata changes).

## Required Input

- `new_version` (required): semantic version like `0.3.5`.
- `release_notes` (optional): short paragraph + bullets for metainfo.

Validation:
- Reject if `new_version` is missing.
- Reject if `new_version` is not a `MAJOR.MINOR.PATCH` version with no leading zeroes (except `0`).

## Quick Reference

| File | Expected change |
|---|---|
| `Cargo.toml`, `crates/core/Cargo.toml`, `crates/search-provider/Cargo.toml` | `[package].version = "<new_version>"` (script) |
| `Cargo.lock` | Workspace entries `sessions-chronicle`, `sessions-chronicle-core`, `sessions-chronicle-search-provider` match the manifests (script) |
| `meson.build` | `project(..., version: '<new_version>', ...)` (script) |
| `src/config.rs.in` | Keep `pub const VERSION: &str = @VERSION@;` (never edit `src/config.rs`) |
| `data/dev.maciz.sessionschronicle.metainfo.xml.in.in` | Add newest `<release version="<new_version>" date="YYYY-MM-DD">...</release>` |

## Implementation Workflow

1. Run `python3 skills/bump-version/scripts/bump_versions.py <new_version>` from inside the repository. It validates `new_version`, checks every file before writing, then sets `[package].version` in the three workspace manifests, the `meson.build` project version, and the three workspace entries of `Cargo.lock` (via `cargo update --workspace --offline`, which leaves dependency versions untouched). If it reports an error, nothing is left half-updated; fix the cause and rerun.
2. Verify `src/config.rs.in` still uses the `@VERSION@` placeholder; never edit generated `src/config.rs`.
3. Prepare release notes for metainfo:
   - If `release_notes` provided: use it.
   - If missing: auto-generate a draft from recent commits/diff context, then ask user approval before writing.
4. Insert the new `<release>` block as the first entry under `<releases>`.
5. Verify that all three Cargo manifests, their three `Cargo.lock` package entries, `meson.build`, the config template, and the metainfo release entry are consistent. Run `cargo metadata --locked --no-deps --format-version 1` to confirm lockfile consistency.
6. Run final Flatpak verification:

```bash
flatpak-builder --user flatpak_app build-aux/dev.maciz.sessionschronicle.Devel.json --force-clean
```

If the build fails because `rofiles-fuse` cannot mount in the environment
(for example, `fusermount3` reports permission denied), retry with:

```bash
flatpak-builder --user --disable-rofiles-fuse flatpak_app build-aux/dev.maciz.sessionschronicle.Devel.json --force-clean
```

## Example Release Entry

```xml
<release version="0.3.5" date="2026-03-05">
  <description>
    <p>Improves startup responsiveness and indexing reliability.</p>
    <ul>
      <li>Speed up startup with incremental indexing</li>
      <li>Re-index OpenCode sessions when files are updated</li>
    </ul>
  </description>
</release>
```

## Common Mistakes

| Mistake | Fix |
|---|---|
| Running `cargo build` only | Run the Flatpak build command above |
| Writing generic release notes without approval | Generate draft notes, then ask user to approve/edit before writing |
| Editing `src/config.rs` | Edit/verify `src/config.rs.in` only; `src/config.rs` is generated |
| Editing manifests, `Cargo.lock`, or `meson.build` by hand | Run `bump_versions.py`; it keeps all of them aligned |
| Failing Flatpak build due to unavailable FUSE mounts | Retry with `--disable-rofiles-fuse`; report any subsequent build failure normally |

## Rationalization Table

| Excuse | Reality |
|---|---|
| "`cargo build` is enough" | This project is validated through Meson/Flatpak packaging flow |
| "I can add placeholder notes now" | Release notes must be reviewed or approved before writing |
| "`src/config.rs` is quicker to edit" | It is generated and will be overwritten |
| "Only the root Cargo.toml needs a bump" | The app, core library, and search provider are all workspace packages with tracked lockfile versions |
| "The usual Flatpak build failed before compiling" | An environment-level rofiles-fuse permission failure can be retried with `--disable-rofiles-fuse`; the Flatpak build must still complete successfully |

## Red Flags - STOP and Correct

- Skipping user approval for auto-generated release notes
- Editing `src/config.rs`
- Forgetting `meson.build` or metainfo release entry
- Leaving a workspace crate manifest or its lockfile package entry on an old version
- Finishing without running the Flatpak build command

If any red flag appears, fix it before declaring the version bump complete.
