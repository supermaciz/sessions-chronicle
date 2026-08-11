# Relm4 `view!` Migration for DatePill and SortPill - Design Spec

**Issue:** [#193](https://github.com/supermaciz/sessions-chronicle/issues/193)  
**Date:** 2026-08-11  
**Status:** Approved design, implementation pending  
**Decision:** Use a conservative hybrid migration: declarative static trees and pure bindings, imperative GTK effects.

## Goal

Convert `DatePill` and `SortPill` from manually constructed widget trees to Relm4 0.11's `#[relm4::component]` and `view!` macros. Let the macro generate `DatePillWidgets` and `SortPillWidgets`, and colocate mechanical model-to-widget bindings with the widgets they update.

This is a strict refactor. It must not change visible text, accessibility, focus, selection, row ordering, popover behavior, date-picking behavior, component messages, or component outputs.

## Context

The two pills are adjacent header-bar controls with the same stable outer structure: `GtkMenuButton`, button content, `GtkPopover`, and boxed `GtkListBox`. Most other Relm4 components in the repository already use `view!`, while these two manually construct their trees and maintain matching widget structs.

The manual cost is highest in `DatePill`: a large construction block, a 25-field `DatePillWidgets`, mirrored initialization, and separate `sync_*` methods. A reader must move between all three locations to understand a property.

The migration must preserve two less-obvious `GtkCalendar` invariants introduced by PR #192:

- A bubble-phase `GtkGestureClick` turns a click on the already displayed day into a real pick.
- A `GtkEventControllerKey` must be prepended ahead of the calendar's own key controller, then yield Space to GTK once a day cell has focus.

The Relm4 component macro supports generated widget structs, named signals, `#[watch]`, `#[block_signal]`, `additional_fields!`, and manual `post_view()` code. These features permit a hybrid boundary rather than forcing all GTK behavior into the macro.

References:

- [Relm4 component macro reference](https://relm4.org/book/stable/component_macro/reference.html)
- [`relm4_macros::component` 0.11.0](https://docs.rs/relm4-macros/0.11.0/relm4_macros/attr.component.html)
- [Custom Range Picker design](2026-07-25-date-range-custom-picker-design.md)
- [Session List Sorting design](2026-07-21-session-list-sorting-design.md)

## Approaches Considered

### 1. Conservative hybrid migration in two checkpoints - selected

First convert the static trees without changing behavior. Then replace only pure, unconditional model-to-widget synchronization with declarative bindings. Keep dynamic collections and ordered or conditional GTK effects imperative.

This delivers the issue's readability benefit while leaving sensitive behavior explicit and testable.

### 2. Structural conversion only

Generate the widget structs and move construction to `view!`, but call the existing `sync_*` logic from `post_view()`. This minimizes initial risk but leaves property behavior separated from widget declarations and only partially addresses the issue.

### 3. Fully declarative conversion

Move controllers, calendar synchronization, dynamic rows, and nearly every signal into `view!`. This minimizes handwritten construction code, but makes controller attachment order and conditional signal blocking harder to audit. The extra abstraction is not justified for a strict refactor.

## Architecture

Both implementations use `#[relm4::component(pub)]`, define a `view!` block, and call `view_output!()` from `init`. The macro-generated widget structs replace the manual `DatePillWidgets` and `SortPillWidgets` definitions.

Widgets needed outside generated bindings receive stable `#[name]` annotations. Every existing widget field used by tests remains available under the same name so the refactor does not require unrelated test rewrites.

The boundary is based on operation type:

Declarative in `view!`:

- Stable widget hierarchy.
- Static properties and CSS classes.
- Pure properties derived from model state.
- Straightforward widget signals.
- Named handlers needed for signal blocking.

Imperative in `init`, `update`, helpers, or `post_view`:

- Popup and popdown effects.
- Deferred focus and row selection.
- Dynamic `SortPill` row reconstruction.
- Accessibility announcements.
- Conditional calendar date synchronization.
- Calendar click and key controller attachment order.

The implementation uses `#[watch]` for bindings that the current `update_view` already recomputes after every message. It does not introduce model tracking solely to use `#[track]`; that would add state and invalidation rules without reducing existing work.

## DatePill Design

### Declarative tree

`view!` describes the complete stable tree:

- Root `GtkMenuButton`, icon, and filter label.
- Popover and two-page stack.
- Preset list and its seven static rows.
- Custom page title, endpoint toggles, calendar, summary, and action buttons.
- Escape shortcut controller, whose capture-phase behavior remains unchanged.

Existing row-builder and endpoint-toggle helpers remain unchanged and are used as constructors by the macro. Only a minimal signature adaptation is allowed if `view!` cannot consume a helper's current return type.

### Declarative bindings

The following current synchronization becomes `#[watch]` properties near the target widgets:

- Root tooltip.
- Pill label text and visibility.
- Six preset count labels.
- Stack visible child name.
- Start and end visual date labels.
- Start and end toggle accessible labels.
- Calendar accessible label.
- Custom-range summary label.
- Apply button sensitivity.

The endpoint group's active index is a watched property with a named `active-notify` handler and `#[block_signal]`. Programmatic synchronization must not enqueue `CustomEndpointChanged`; user changes still do.

### Calendar signal and controllers

`day-selected` is connected in `view!` with a named `@calendar_handler`. The generated handler ID is available from the generated widgets struct.

Calendar date synchronization remains conditional manual code in `post_view()`:

1. Resolve the target date from the active endpoint, falling back to today.
2. Compare it with the date already displayed by `GtkCalendar`.
3. If different and convertible, block `calendar_handler`.
4. Call `set_date`.
5. Unblock `calendar_handler`.

This is intentionally not a watched `set_date` binding. The explicit comparison and block sequence distinguishes programmatic seeding from user picks and protects accessibility announcements.

The click and key controllers remain manually created and connected. They are attached to the generated calendar after `view_output!()` in the existing order: `GestureClick` first, `EventControllerKey` second. GTK prepends controllers, so this leaves the key controller first in dispatch order. The controller objects are declared through `additional_fields!` so existing ordering and behavior tests can inspect them.

### Model and generated widget ownership

`DatePill` retains widget handles needed directly by update-time effects and deferred callbacks: `listbox`, `popover`, `stack`, `calendar`, and `summary_label`. Those handles are cloned from generated widgets during initialization.

`calendar_handler` moves out of the model because the named macro connection owns it. Test-only announcement and accessible-label recorders remain available through `additional_fields!`; changing their test-facing ownership provides no product benefit.

After the migration, `sync_button` and `sync_counts` disappear. `sync_custom_state` is replaced by `sync_calendar_date`, which contains only the conditional calendar operation.

## SortPill Design

### Declarative tree and bindings

`view!` describes the root menu button, icon and label, popover, margins, and named list box. The effective label, label visibility, and tooltip become watched properties.

The row-activation signal remains colocated with the list box. It keeps using the existing `Rc<Cell<bool>>` mirror to distinguish the optional Relevance row from named orders because signal closures cannot borrow the component model.

### Dynamic rows remain imperative

`rebuild_rows()` remains responsible for inserting and removing the Relevance row, appending named-order rows, and installing or removing the separator header function. This collection changes shape at runtime; forcing it into a conditional macro widget or a factory would increase scope and alter ownership for no user-visible benefit.

`SortPill` therefore retains `listbox`, `popover`, and `row_activation_fts_flag`. `sync_button` disappears once its three properties are declared in `view!`.

## Message And Update Flow

The component interfaces are unchanged. `update` remains the only place that mutates business state.

After a message:

1. `update` mutates model state and performs immediate output or popover effects exactly as today.
2. Relm4 applies generated watched properties.
3. `post_view()` performs only the guarded calendar date synchronization for `DatePill`.

Focus callbacks, selection restoration, output delivery, date validation, and `.ok()` handling retain their current behavior. No new error surface or logging policy is introduced.

## Implementation Sequence

The work proceeds through two independently compiling checkpoints in the same change:

1. Add component macros and declarative static trees, generate the widgets structs, preserve existing synchronization behavior, and verify all tests.
2. Move the approved pure properties to `#[watch]`, narrow the remaining manual synchronization, and verify again.

The checkpoints are for regression isolation; they do not require separate pull requests.

## Scope Boundaries

In scope:

- `src/ui/date_pill.rs`
- `src/ui/sort_pill.rs`
- Existing tests in those modules when adaptation to generated fields is unavoidable

Out of scope:

- Any visual, copy, localization, accessibility, keyboard, focus, or selection change
- New preset or sort options
- Replacing dynamic rows with a Relm4 factory
- Removing all widget handles from component models
- Introducing tracker state to use `#[track]`
- Refactoring row-builder helpers unrelated to macro compatibility
- Screenshots or Flatpak packaging changes for this behavior-preserving refactor

## Acceptance Criteria

- Manual `DatePillWidgets` and `SortPillWidgets` definitions are removed.
- Both components use `#[relm4::component(pub)]`, `view!`, and `view_output!()`.
- `sync_button` and `sync_counts` are removed.
- Remaining imperative synchronization is limited to effects and dynamic widget collections.
- The calendar key controller still precedes the calendar's own key controller.
- Programmatic calendar updates do not emit user-pick behavior or announcements.
- Text, accessibility properties, focus, selection, row ordering, and popover behavior are unchanged.
- No line-count target is imposed; clarity and explicit invariants take priority.

## Verification

Automated verification:

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --all --no-fail-fast
```

Manual verification with `--sessions-dir tests/fixtures`:

- Clicking the day already displayed by the calendar enables Apply.
- Tabbing into a fresh calendar and pressing Space selects the displayed day.
- Moving with arrow keys and pressing Space selects the focused day.
- Opening and closing each pill restores the correct row selection and keyboard focus.

No screenshot update is required because the accepted scope forbids visual changes. No Flatpak build is required because dependencies, resources, and packaging are unchanged.
