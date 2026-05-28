# Sidebar B Simplification Design

**Date:** 2026-05-28  
**Status:** approved design  
**Source exploration:** `docs/explorations/2026-05-28-sidebar-filters-simplification-exploration.md`  
**Chosen proposal:** B, scoped to the sidebar only

## Goal

Simplify the `SessionList` left sidebar by applying the radical visual direction from proposal B while keeping the implementation small and preserving current filtering behavior.

The sidebar should read as a compact filter panel without a visible global title or visible section titles. The change is intentionally limited to the sidebar filter area; session rows, search, date filtering, and clear-filter behavior remain out of scope.

## Scope

In scope:

- Remove the visible `Filters` label and its separator.
- Remove the visible `AI Assistants` and `Projects` labels.
- Remove the separator between the assistant filter rows and project rows.
- Separate the assistant block and project block with about `32px` of vertical spacing.
- Add each AI assistant's icon to its filter row so the unlabeled assistant block remains recognizable.
- Keep the existing `Pinned` row icon.
- Keep project rows minimal: no new icon for `All Sessions` and no new icons for project rows.
- Keep accessible group identity for the assistant and project blocks even though the visible section labels are removed.

Out of scope:

- Adding a `Clear filters` button.
- Changing search shortcuts or search behavior.
- Changing the date filter pill.
- Changing session row metadata hierarchy.
- Refactoring the sidebar to `AdwPreferencesGroup` or boxed `AdwActionRow` lists.
- Adding project prefix icons beyond the existing `Pinned` icon.

## Visual Structure

The sidebar becomes two unlabeled vertical blocks:

1. Assistant filters at the top.
2. Project filters below, separated from assistants by a larger vertical gap.

Assistant rows use the existing checkbox and status dot behavior, plus a decorative assistant icon near the label. The intended row structure is:

`checkbox` + `assistant icon` + `assistant name` + `status dot`

Project rows keep their current list behavior and visual simplicity. The intended project row structure is:

`project label` + `count`

The `Pinned` row keeps its current prefix icon because it already exists and helps identify the special filter. No icon is added to `All Sessions`, normal projects, or `Unassigned`.

Counters should remain visually subdued and numerically aligned. If the current badge treatment conflicts with the simplified B direction, prefer existing utility classes such as `numeric` and `dim-label` over adding new prominent badge styling.

## Interaction And Behavior

Filtering behavior does not change.

- Toggling an assistant checkbox still updates the active AI assistant filters.
- Assistant status dots keep their current visibility, colors, and tooltips.
- Project filter selection remains a single-selection `gtk::ListBox` interaction.
- `All Sessions`, `Pinned`, project rows, and `Unassigned` keep their existing semantics and ordering.
- The selected project row is still restored after project rows are rebuilt.

This design does not introduce new commands, shortcuts, buttons, or state.

## Accessibility

Removing visible section labels must not remove the semantic identity of the two blocks.

The assistant block should expose an accessible name equivalent to `AI Assistants`. The project block should expose an accessible name equivalent to `Projects`. The exact GTK API can be chosen during implementation, but the result must keep the two groups identifiable to assistive technology.

Assistant icons are visual reinforcement only. They must not replace the row text, and they should be treated as decorative or redundant for assistive technology where GTK exposes that distinction.

Existing row labels and status dot tooltips should be preserved.

## Implementation Notes

Primary affected files:

- `src/ui/sidebar.rs`
- `data/resources/style.css`

Expected code changes:

- Remove the `gtk::Label` widgets for `Filters`, `AI Assistants`, and `Projects` from the sidebar view.
- Remove the visible `gtk::Separator` widgets tied to those labels.
- Adjust container spacing so the assistant and project lists are separated by about `32px`.
- Update `build_assistant_row` to add the assistant icon, likely via `assistant.icon_name()`.
- Keep `make_row` behavior minimal for project rows and preserve the current `Pinned` prefix icon.
- Add accessible labels or descriptions to the assistant and project list containers.
- Prune CSS rules only if the removed widgets or updated counter styling make them obsolete.

The implementation should stay local to the sidebar. Avoid broad UI refactors or libadwaita structure changes in this iteration.

## Testing And Verification

Existing sidebar tests should continue to pass, especially tests covering:

- Assistant status dots start hidden.
- Source status updates apply CSS classes and tooltips.
- Project rows rebuild and preserve selection.

Add targeted tests only if the implementation changes testable structure in a meaningful way, such as accessible labels or row child composition.

Verification before completion:

- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`, or a clearly documented narrower command if full tests are not feasible during the implementation session

## Success Criteria

- The visible sidebar no longer contains `Filters`, `AI Assistants`, or `Projects` headings.
- The two blocks remain visually distinct through spacing alone.
- Assistant rows include assistant icons and remain readable without a section heading.
- Project rows remain minimal and keep the current `Pinned` icon.
- Existing filter behavior is unchanged.
- Assistive technology still has group names for the assistant and project blocks.
