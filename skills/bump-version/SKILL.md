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
- Reject if `new_version` does not match `^\d+\.\d+\.\d+$`.

## Quick Reference

| File | Expected change |
|---|---|
| `Cargo.toml` | Set `[package].version = "<new_version>"` |
| `meson.build` | Set `project(..., version: '<new_version>', ...)` |
| `src/config.rs.in` | Keep `pub const VERSION: &str = @VERSION@;` (never edit `src/config.rs`) |
| `data/dev.maciz.sessionschronicle.metainfo.xml.in.in` | Add newest `<release version="<new_version>" date="YYYY-MM-DD">...</release>` |

## Implementation Workflow

1. Validate `new_version` format.
2. Update `Cargo.toml` version.
3. Update `meson.build` project version.
4. Verify `src/config.rs.in` still uses `@VERSION@` placeholder.
5. Prepare release notes for metainfo:
   - If `release_notes` provided: use it.
   - If missing: auto-generate a draft from recent commits/diff context, then ask user approval before writing.
6. Insert new `<release>` block as first entry under `<releases>`.
7. Verify changes in all four target files.
8. Run final verification command:

```bash
flatpak-builder --user flatpak_app build-aux/dev.maciz.sessionschronicle.Devel.json --force-clean
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
| Forgetting `meson.build` version | Update both `Cargo.toml` and `meson.build` every release |

## Rationalization Table

| Excuse | Reality |
|---|---|
| "`cargo build` is enough" | This project is validated through Meson/Flatpak packaging flow |
| "I can add placeholder notes now" | Release notes must be reviewed or approved before writing |
| "`src/config.rs` is quicker to edit" | It is generated and will be overwritten |
| "Three files are enough" | Release metadata must include metainfo release entry too |

## Red Flags - STOP and Correct

- Skipping user approval for auto-generated release notes
- Editing `src/config.rs`
- Forgetting `meson.build` or metainfo release entry
- Finishing without running the Flatpak build command

If any red flag appears, fix it before declaring the version bump complete.
