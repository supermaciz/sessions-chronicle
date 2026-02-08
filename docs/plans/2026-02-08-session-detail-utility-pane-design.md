# Session Detail Utility Pane Design

**Status:** Validated in collaborative design discussion

**Date:** 2026-02-08

## Goal

Align the app layout with GNOME HIG utility pane patterns by replacing the current `AdwNavigationSplitView` composition with `AdwOverlaySplitView`, while showing different pane content in list vs detail contexts and keeping interaction predictable:

- Session list uses filter controls and starts with the pane open.
- Session detail uses a context pane and starts with the pane closed.
- Returning to list re-opens the filter pane automatically.

## Terminology

- UX and documentation use **utility pane** (GNOME HIG term).
- Libadwaita API still uses `sidebar` naming (`set_sidebar`, `set_show_sidebar`, etc.).

Both terms refer to the same UI area.

## Current Context

Current layout in `src/app.rs` uses `adw::NavigationSplitView` with one static sidebar (`Sidebar`) and a `NavigationView` content area. This causes the same sidebar content to appear regardless of whether the user is browsing the list or viewing a session detail. The detail view in `src/ui/session_detail.rs` also includes a metadata card and a "Resume in Terminal" button at the top of the main content stream.

The desired behavior is contextual: list-focused controls in list mode, and session-focused actions in detail mode.

## Options Considered

### A) Single `AdwOverlaySplitView` with dynamic utility pane content (chosen)

Use one overlay split view at app level and swap utility pane content according to active page.

**Pros:**
- Best match for utility pane behavior.
- Centralized state and transitions.
- Smallest functional delta while preserving existing navigation internals.

**Cons:**
- Requires an app-level pane mode state.
- Introduces one new context component.

### B) Two split views (one for list, one for detail)

Each page owns its own split layout.

**Pros:**
- Strong separation of concerns.

**Cons:**
- Heavier structure and more lifecycle complexity.
- Higher risk of inconsistent behavior between pages.

### C) Keep `AdwNavigationSplitView` and only tweak visibility

Keep current composition and patch behavior around it.

**Pros:**
- Minimal refactor.

**Cons:**
- Does not fully align with desired utility pane overlay behavior.
- Harder to reason about contextual pane content long-term.

## Architecture Decision

Adopt option A.

`src/app.rs` becomes the single orchestrator for utility pane behavior:

- Root content uses `adw::OverlaySplitView`.
- `content` remains the existing `adw::NavigationView` stack (sessions page + detail page).
- `sidebar` (API term) hosts utility pane content that changes with app context.

### New UI modes in `App`

Introduce a minimal mode state:

```rust
enum UtilityPaneMode {
    Filters,
    SessionContext,
}
```

And a visibility flag (or directly mirror split view property):

- `pane_mode: UtilityPaneMode`
- `pane_open: bool`

## Components

### 1) List utility pane (existing)

Reuse `src/ui/sidebar.rs` unchanged for filters.

### 2) Detail utility pane (new, v1)

Add a small component (for example `src/ui/detail_context_pane.rs`) that provides:

- A short context heading.
- Primary action: `Resume in Terminal`.

YAGNI for v1:

- No message outline.
- No detailed analytics/stats.
- No expanded metadata matrix.

### 3) Session detail content adjustment

Keep a light metadata header in `src/ui/session_detail.rs` for reading context, but move the primary resume action to the utility pane.

This is intentional duplication with reduced weight in content:

- Main stream: lightweight context header only.
- Utility pane: action-focused controls.

## Data Flow and State Transitions

### Startup

- Active page: list.
- `pane_mode = Filters`.
- `pane_open = true`.

### Select session (`SessionSelected`)

- Load detail (`SessionDetailMsg::SetSession`).
- Push detail page in `NavigationView`.
- Switch `pane_mode = SessionContext`.
- Set `pane_open = false`.

### Navigate back (`NavigateBack` / `popped`)

- Pop detail if needed.
- Switch `pane_mode = Filters`.
- Set `pane_open = true`.

### Toggle pane button

- Inverts `pane_open` regardless of mode.

### Resume action in detail pane

Reuse existing resume pipeline:

- Detail pane emits app-level resume intent.
- `App` forwards to `SessionDetailMsg::ResumeClicked` (or equivalent reused path).
- Existing `SessionDetailOutput::ResumeRequested(id, tool)` flow remains the source of truth.

## Responsive Behavior

- On large widths, utility pane appears side-by-side with content.
- On narrow widths, use `OverlaySplitView` collapsed behavior so the utility pane overlays content.
- Keep desktop default policy validated in discussion:
  - list: open,
  - detail: closed,
  - back to list: reopen.

Use breakpoints to set `collapsed` for compact widths.

## Error Handling

- Utility pane must never block primary navigation.
- If no active session exists in detail mode, show neutral state and disable resume action.
- Resume failures continue to surface existing toast notifications.
- Any pane rendering issue should degrade to hidden pane, not crash.

## Testing Strategy

### Unit-level logic checks

Add tests for pure transition helpers (if extracted):

- list -> detail enforces `SessionContext + closed`.
- detail -> list enforces `Filters + open`.
- toggle flips visibility without changing mode.

### Integration/manual checks

- Verify filters remain functional in list mode.
- Verify detail mode shows session context pane and resume action.
- Verify returning from detail reopens filters pane.
- Verify narrow window overlay behavior and toggle affordance.
- Verify keyboard navigation/focus and back behavior remain coherent.

## Out of Scope

- Rich detail pane metadata and statistics.
- In-conversation navigation map.
- Search-hit navigator in pane.
- Any database or parser schema change.

## Rollout Notes

- Keep the implementation incremental and low-risk:
  1. Introduce split view replacement in `App`.
  2. Add detail context pane component.
  3. Wire mode transitions and visibility rules.
  4. Move resume action emphasis to the pane.
- Prefer preserving existing message/event contracts unless a simplification is clear and test-covered.
