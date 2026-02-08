# Session Detail Utility Pane Design

**Status:** Validated in collaborative design discussion, refined with API research

**Date:** 2026-02-08

## Goal

Align the app layout with GNOME HIG utility pane patterns by replacing the current `AdwNavigationSplitView` composition with `AdwOverlaySplitView`, while showing different pane content in list vs detail contexts and keeping interaction predictable:

- Session list uses filter controls and starts with the pane open.
- Session detail uses a context pane and starts with the pane closed.
- Returning to list re-opens the filter pane automatically.

## References

- [GNOME HIG: Utility Panes](https://developer.gnome.org/hig/patterns/containers/utility-panes.html)
- [Libadwaita: AdwOverlaySplitView](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.4/class.OverlaySplitView.html)
- [Libadwaita: Adaptive Layouts](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/adaptive-layouts.html)
- [Libadwaita: Migrating to Breakpoints](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/migrating-to-breakpoints.html)

## Terminology

- UX and documentation use **utility pane** (GNOME HIG term).
- Libadwaita API still uses `sidebar` naming (`set_sidebar`, `set_show_sidebar`, etc.).

Both terms refer to the same UI area.

## Current Context

Current layout in `src/app.rs` uses `adw::NavigationSplitView` with one static sidebar (`Sidebar`) and a `NavigationView` content area. This causes the same sidebar content to appear regardless of whether the user is browsing the list or viewing a session detail. The detail view in `src/ui/session_detail.rs` also includes a metadata card and a "Resume in Terminal" button at the top of the main content stream.

The desired behavior is contextual: list-focused controls in list mode, and session-focused actions in detail mode.

### Behavioral difference: `NavigationSplitView` vs `OverlaySplitView`

This is an intentional UX change, not a side effect:

- **Before** (`NavigationSplitView`): on narrow screens, the sidebar becomes a full-width navigation page — the user "navigates into" the sidebar.
- **After** (`OverlaySplitView`): on narrow screens, the sidebar overlays the content as a floating panel.

The overlay behavior is the correct pattern for a utility pane per GNOME HIG: *"ensure that a utility pane will overlap the main view when there isn't available width to show it alongside"*. Users should never need to navigate away from content to reach the pane.

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

### Dependency check

The project already uses `adw = { version = "0.8.1", package = "libadwaita", features = ["v1_8"] }` in `Cargo.toml`. Feature `v1_8` includes `v1_4`, so `adw::OverlaySplitView` is available with no dependency change.

### New UI modes in `App`

Introduce a minimal mode state in `src/app.rs`:

```rust
enum UtilityPaneMode {
    Filters,
    SessionContext,
}
```

And app-owned UI/session state:

- `pane_mode: UtilityPaneMode`
- `pane_open: bool`
- `active_session: Option<ActiveSessionRef>`

```rust
struct ActiveSessionRef {
    id: String,
    tool: Tool,
    project_name: String,
}
```

`active_session` is the canonical source for resume routing from the utility pane.

### Pane content swap mechanism

Use a `gtk::Stack` as the `sidebar` widget of the `OverlaySplitView`. The Stack holds two children:

- `"filters"` — the existing `Sidebar` component widget.
- `"session-context"` — the new detail context pane widget.

On mode transitions, call `stack.set_visible_child_name("filters")` or `stack.set_visible_child_name("session-context")`.

**Why a Stack and not dynamic `set_sidebar()` calls:**

- Avoids destroying and recreating widgets on every navigation.
- Preserves internal state (e.g. filter checkbox selections) across transitions.
- `gtk::Stack` with `set_transition_type(None)` makes the swap instantaneous with no visual glitch.

The `gtk::Stack` instance is stored in the `App` model to allow imperative switching in `update()`.

### Sidebar position

Per GNOME HIG: *"if the pane affects the main view, place it on the left."* The filter pane directly affects the session list, so the utility pane stays on the **left** (start) side. This is the default for `OverlaySplitView` (`sidebar_position = PackType::Start`), so no explicit configuration is needed.

## Components

### 1) List utility pane (existing)

Reuse `src/ui/sidebar.rs` unchanged for filters.

### 2) Detail utility pane (new, v1)

Add a small component `src/ui/detail_context_pane.rs` that provides:

- A short context heading (project name, tool icon).
- Primary action: `Resume in Terminal` button (`suggested-action` CSS class).

The component is not the canonical owner of session state. It stores display-only fields and emits a generic `ResumeClicked` output; `App` owns canonical session identity and resume data (see [Resume routing](#resume-action-in-detail-pane) below).

On `SessionSelected`, `App` resolves metadata for the selected session and sends `SetSession { project_name, tool }` to this pane.

YAGNI for v1:

- No message outline.
- No detailed analytics/stats.
- No expanded metadata matrix.

### 3) Session detail content adjustment

Keep a light metadata header in `src/ui/session_detail.rs` for reading context, but move the primary resume action to the utility pane.

This is intentional duplication with reduced weight in content:

- Main stream: lightweight context header only (remove `resume_button` from the card).
- Utility pane: action-focused controls.

## Header Bar Layout

The current header bar (`src/app.rs:115-137`) has:
- `pack_start`: back button (conditional on `detail_visible`), search toggle button.
- `pack_end`: hamburger menu.

Add a **pane toggle button** at `pack_end`, before the menu button:

```rust
#[name = "pane_toggle"]
gtk::ToggleButton {
    set_icon_name: "sidebar-show-symbolic",
    set_tooltip_text: Some("Toggle utility pane (F9)"),
    set_action_name: Some("win.toggle-pane"),
    #[watch]
    set_active: model.pane_open,
},
```

**Important:** the button uses `set_action_name` instead of `connect_toggled`. The `toggled` signal fires on *any* `active` property change, including programmatic updates from `#[watch]`. Using `connect_toggled` with `#[watch] set_active` would cause an infinite feedback loop: `set_active(false)` → `toggled` → `TogglePane` → `pane_open = true` → `set_active(true)` → ... The GAction only activates on user interaction (click), breaking the cycle.

Bind `OverlaySplitView::show-sidebar` to `model.pane_open` via the `#[watch]` macro in the view definition. The `TogglePane` message updates `pane_open`, and the `#[watch]` on `set_show_sidebar` propagates to the widget.

Per GNOME HIG: *"If utility pane visibility can be toggled, assign the F9 key as a shortcut."* Register a dedicated window action `win.toggle-pane` that dispatches `AppMsg::TogglePane`, then bind `F9` to that action. The toggle button and accelerator both route through the same GAction to avoid divergence.

## Search Bar Positioning

The search bar remains **above** the `OverlaySplitView`, inside the `ToolbarView` content box — the same position as today. This means search applies to the full app view (both list and detail), which matches current behavior where `SearchQueryChanged` updates both `SessionList` and `SessionDetail`.

The widget hierarchy becomes:

```
AdwApplicationWindow
└── AdwToastOverlay
    └── AdwToolbarView
        ├── [top] AdwHeaderBar (back, search toggle, pane toggle, menu)
        └── [content] gtk::Box (vertical)
            ├── gtk::SearchBar
            └── AdwOverlaySplitView
                ├── [sidebar] gtk::Stack
                │   ├── "filters" → Sidebar widget
                │   └── "session-context" → DetailContextPane widget
                └── [content] AdwNavigationView
                    ├── "sessions" → SessionList widget
                    └── "detail" → SessionDetail widget (pushed on select)
```

## Data Flow and State Transitions

### Startup

- Active page: list.
- `pane_mode = Filters`.
- `pane_open = true`.
- Stack visible child: `"filters"`.

### Select session (`SessionSelected`)

- Resolve selected-session metadata in `App` and store `active_session`.
- Load detail (`SessionDetailMsg::SetSession`).
- Send display data to detail pane (`DetailContextPaneMsg::SetSession { project_name, tool }`) from `active_session`.
- Push detail page in `NavigationView`.
- Switch `pane_mode = SessionContext`.
- Set `pane_open = false`.
- Switch stack to `"session-context"`.

### Navigate back (`NavigateBack` / `popped`)

- Pop detail if needed.
- Preserve existing `detail_visible` guard to prevent double-pop from the `connect_popped` signal.
- Clear `active_session = None`.
- Switch `pane_mode = Filters`.
- Set `pane_open = true`.
- Switch stack to `"filters"`.

### Toggle pane button / F9

- `AppMsg::TogglePane` inverts `pane_open` regardless of mode.
- `OverlaySplitView` can also change visibility via gestures and collapse behavior. Wire `notify::show-sidebar` to `AppMsg::PaneVisibilityChanged(bool)` so model state stays synchronized with widget state.
- Does **not** change `pane_mode` or stack visible child.

### Resume action in detail pane

The detail context pane component is kept simple:

1. `DetailContextPane` defines `Output = DetailContextPaneOutput::ResumeClicked`.
2. On button click, it emits `ResumeClicked` (no session data — the pane doesn't own it).
3. `App` receives this via the `forward()` wiring and maps it to `AppMsg::ResumeFromPane`.
4. `AppMsg::ResumeFromPane` reads `self.active_session` and calls the existing `ResumeSession(id, tool)` pipeline.
5. If `active_session` is `None`, the pane keeps its resume button disabled and the message is ignored with a warning log.

This keeps session ownership in one place (`App`) and reuses the existing resume flow end-to-end.

## `OverlaySplitView` Configuration

Key properties to set on the split view:

| Property | Value | Rationale |
|---|---|---|
| `show-sidebar` | bound to `model.pane_open` | Controlled by app state |
| `collapsed` | set via breakpoint | Responsive behavior |
| `sidebar-position` | `Start` (default) | Left pane per HIG |
| `min-sidebar-width` | `180` (default) | Reasonable minimum |
| `max-sidebar-width` | `280` (default) | Reasonable for filters |
| `enable-show-gesture` | `true` | Swipe-to-reveal on touch |
| `enable-hide-gesture` | `true` | Swipe-to-hide on touch |
| `pin-sidebar` | `false` (default) | Keep automatic hide/show on collapse transitions |

### Bidirectional visibility sync

`pane_open` should not be write-only app state. Keep it synchronized both ways:

1. `pane_open` -> `OverlaySplitView::show-sidebar` via `#[watch]`.
2. `OverlaySplitView::show-sidebar` -> `AppMsg::PaneVisibilityChanged(bool)` via `notify::show-sidebar`.

This prevents desynchronization when users open/close the pane with touch gestures or when collapse transitions modify visibility.

The `PaneVisibilityChanged` handler must guard against no-op updates to avoid redundant view cycles:

```rust
AppMsg::PaneVisibilityChanged(visible) => {
    if self.pane_open != visible {
        self.pane_open = visible;
    }
}
```

**Collapse interaction note:** with `pin-sidebar: false`, collapsing the split view auto-hides the sidebar and uncollapsing auto-shows it. These widget-driven changes propagate through `PaneVisibilityChanged`. This means a window resize can override an earlier `pane_open = false` from `SessionSelected` if the window crosses the breakpoint threshold. This is acceptable — the pane follows the layout's spatial constraints — but implementers should be aware that `pane_open` reflects the *effective* visibility, not a latched user intent.

### Breakpoint for responsive collapse

Add an `AdwBreakpoint` to the `AdwApplicationWindow`:

```xml
<condition>max-width: 400sp</condition>
<setter object="overlay_split" property="collapsed">True</setter>
```

In Relm4 code, this is set up imperatively in `init()` after `view_output!()`. The `sp` unit automatically accounts for the GNOME Large Text accessibility setting.

## Error Handling

- Utility pane must never block primary navigation.
- If no active session exists in detail mode, the detail pane shows a neutral placeholder and disables the resume button. Avoid `unwrap()` on optional session data — use `if let` / `map` / defaults.
- Resume failures continue to surface existing toast notifications via `App::show_resume_failure_toast`.
- After calling `stack.set_visible_child_name(name)`, verify `stack.visible_child_name()`. If the returned name does not match the requested `name`, log both values (requested vs actual) and hide the pane rather than panic.

## Testing Strategy

### Unit-level logic checks

Extract pure transition functions from `App::update()` and test them without GTK runtime:

```rust
fn transition_to_detail(pane_mode: &mut UtilityPaneMode, pane_open: &mut bool) {
    *pane_mode = UtilityPaneMode::SessionContext;
    *pane_open = false;
}

fn transition_to_list(pane_mode: &mut UtilityPaneMode, pane_open: &mut bool) {
    *pane_mode = UtilityPaneMode::Filters;
    *pane_open = true;
}
```

Test cases:

- `transition_to_detail` enforces `SessionContext + closed`.
- `transition_to_list` enforces `Filters + open` and clears `active_session`.
- Toggle flips `pane_open` without changing `pane_mode`.
- `PaneVisibilityChanged(bool)` mirrors widget-originated visibility updates to `pane_open`.
- `UtilityPaneMode` maps to correct stack child name (`Filters` → `"filters"`, `SessionContext` → `"session-context"`).

### Integration/manual checks

- Verify filters remain functional in list mode.
- Verify detail mode shows session context pane and resume action.
- Verify returning from detail reopens filters pane.
- Verify narrow window overlay behavior and toggle affordance.
- Verify F9 keyboard shortcut toggles the pane.
- Verify swipe gestures work on touchscreen (show/hide).
- Verify toggle button state remains synchronized after gesture-driven open/close.
- Verify keyboard navigation/focus and back behavior remain coherent.

## Out of Scope

- Rich detail pane metadata and statistics.
- In-conversation navigation map.
- Search-hit navigator in pane.
- Any database or parser schema change.

## Rollout Notes

Keep the implementation incremental and low-risk. Each step has a validation gate:

1. **Replace split view in `App`.**
   Replace `adw::NavigationSplitView` with `adw::OverlaySplitView`. Use a `gtk::Stack` as the sidebar, containing only the existing `Sidebar` widget initially. Add breakpoint for collapse. Add pane toggle button, `win.toggle-pane` action, F9 shortcut, and `notify::show-sidebar` back-sync.
   *Gate:* app builds, filters sidebar works in list mode, pane toggle shows/hides, responsive collapse works on narrow resize, and toggle state stays synchronized after gesture/collapse transitions.

2. **Add detail context pane component.**
   Create `src/ui/detail_context_pane.rs` with heading + resume button. Register it as the second Stack child (`"session-context"`). No wiring yet — it just exists.
   *Gate:* app builds, manually switching stack child shows the new pane.

3. **Wire mode transitions and visibility rules.**
   Add `UtilityPaneMode` enum, `pane_mode`/`pane_open`, and canonical `active_session` state to `App` model. Implement state transitions on `SessionSelected`, `NavigateBack`, `TogglePane`, and `PaneVisibilityChanged`. Extract and test pure transition helpers.
   *Gate:* selecting a session hides pane and switches to context view; going back reopens filters; toggle works in both modes; widget-model visibility stays in sync; unit tests pass.

4. **Move resume action emphasis to the pane.**
   Wire `DetailContextPaneOutput::ResumeClicked` → `AppMsg::ResumeFromPane` → `active_session` lookup → existing resume pipeline. Remove `resume_button` from `session_detail.rs` metadata card.
   *Gate:* resume from pane works end-to-end; toast on failure; no resume button in content area.

Prefer preserving existing message/event contracts unless a simplification is clear and test-covered.
