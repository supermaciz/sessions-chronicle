# Relm4 `view!` Migration for DatePill and SortPill - Design Spec

**Issue:** [#193](https://github.com/supermaciz/sessions-chronicle/issues/193)  
**Date:** 2026-08-11  
**Status:** Approved design, implementation pending  
**Decision:** Use a hybrid migration with a widened tree scope: declarative static trees and pure bindings, imperative GTK effects. The `DatePill` row-builder and endpoint-toggle helpers are inlined into `view!` rather than preserved, because `#[watch]` can only target widgets that are nodes of the macro tree.

## Goal

Convert `DatePill` and `SortPill` from manually constructed widget trees to Relm4 0.11's `#[relm4::component]` and `view!` macros. Let the macro generate `DatePillWidgets` and `SortPillWidgets`, and colocate mechanical model-to-widget bindings with the widgets they update.

This refactor must not change visible text, accessibility properties, focus, selection, row ordering, popover behavior, date-picking behavior, or component outputs. It makes two deliberate, enumerated internal changes, both documented below:

- The `DatePill` row-builder and endpoint-toggle helpers are dissolved into the `view!` tree. The widgets they produce are unchanged; only their construction site moves.
- Programmatic `ToggleGroup` reseeding stops emitting a redundant `CustomEndpointChanged` message (see [Endpoint group signal blocking](#endpoint-group-signal-blocking)).

## Context

The two pills are adjacent header-bar controls with the same stable outer structure: `GtkMenuButton`, button content, `GtkPopover`, and boxed `GtkListBox`. Most other Relm4 components in the repository already use `view!`, while these two manually construct their trees and maintain matching widget structs.

The manual cost is highest in `DatePill`: a large construction block, a 25-field `DatePillWidgets`, mirrored initialization, and separate `sync_*` methods. A reader must move between all three locations to understand a property.

The migration must preserve two less-obvious `GtkCalendar` invariants introduced by PR #192:

- A bubble-phase `GtkGestureClick` turns a click on the already displayed day into a real pick.
- A `GtkEventControllerKey` must be prepended ahead of the calendar's own key controller, then yield Space to GTK once a day cell has focus.

The Relm4 component macro supports generated widget structs, named signals, `#[watch]`, `#[block_signal]`, `#[wrap(..)]`, `additional_fields!`, and manual `post_view()` code. These features permit a hybrid boundary rather than forcing all GTK behavior into the macro. Three of them behave in ways that directly constrain this design; see [Macro Constraints](#macro-constraints).

References:

- [Relm4 component macro reference](https://relm4.org/book/stable/component_macro/reference.html)
- [`relm4_macros::component` 0.11.0](https://docs.rs/relm4-macros/0.11.0/relm4_macros/attr.component.html)
- [Custom Range Picker design](2026-07-25-date-range-custom-picker-design.md)
- [Session List Sorting design](2026-07-21-session-list-sorting-design.md)

## Approaches Considered

### 1. Hybrid migration with a widened tree scope - selected

Convert the static trees, inlining the row and toggle helpers so every widget carrying a model-derived property is a node of the macro tree. Then replace all pure, unconditional model-to-widget synchronization with declarative bindings. Keep dynamic collections and ordered or conditional GTK effects imperative.

This delivers the issue's readability benefit in full while leaving sensitive behavior explicit and testable.

### 2. Structural conversion only

Generate the widget structs and move construction to `view!`, but call the existing `sync_*` logic from `post_view()`. This minimizes initial risk but leaves property behavior separated from widget declarations and only partially addresses the issue.

### 3. Hybrid migration preserving the helpers

Same as 1, but keep `build_preset_row`, `build_row`, and `build_endpoint_toggle` as constructors called from `view!`. Rejected: the six count labels and the two endpoint date labels are created *inside* those helpers and returned only indirectly, so they cannot be `#[name]`d and therefore cannot carry `#[watch]`. Under this variant `sync_counts` and most of `sync_custom_state` survive, and roughly half of the issue's stated benefit is lost.

### 4. Fully declarative conversion

Move controllers, calendar synchronization, dynamic rows, and nearly every signal into `view!`. This minimizes handwritten construction code, but makes controller attachment order and conditional signal blocking harder to audit. The extra abstraction is not justified here.

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

## Macro Constraints

Three `relm4-macros` 0.11.0 behaviors shape the implementation. All three were confirmed with throwaway compilation spikes against this repository's dependency set; treat them as settled, not as guesses.

### Named children of by-value assign functions need `#[wrap(Clone::clone)]`

`adw::ToggleGroup::add` and `gtk::Widget::add_controller` take their argument **by value** (`libadwaita-0.9.1/src/auto/toggle_group.rs:39`). The macro moves a named widget into the generated struct by shorthand at the end of `init`, so a plain named assignment is moved twice:

```rust
#[name = "start_toggle"]
add = adw::Toggle { }   // error[E0382]: use of moved value: `start_toggle`
```

`#[wrap(..)]` wraps the assignment expression, so combining it with the reference form assigns a clone and leaves the local intact:

```rust
#[name = "start_toggle"]
#[wrap(Clone::clone)]
add = &adw::Toggle { /* ... */ },
```

This is what keeps the endpoint toggles and the Escape controller inside `view!` as named, watchable nodes.

### `additional_fields!` does not support `#[cfg]` attributes

The attribute is dropped from the generated struct definition while the field still appears in the initializer, so `#[cfg(test)] announcement_log: ...` fails to compile with `E0425` and `E0560`. Test-only recorders therefore cannot live in the generated widgets struct.

### `additional_fields!` values come from locals bound before `view_output!()`

Each additional field is populated from a local variable of the same name that must already be in scope when `view_output!()` expands. Objects that are attached to macro-generated widgets afterwards must still be *constructed* before the macro.

## DatePill Design

### Declarative tree

`view!` describes the complete stable tree:

- Root `GtkMenuButton`, icon, and filter label.
- Popover and two-page stack.
- Preset list and its seven static rows, each expanded inline down to its title and trailing widget.
- Custom page title, endpoint toggles with their inline caption/date content, calendar, summary, and action buttons.
- Escape shortcut controller, whose capture-phase behavior remains unchanged.

`build_preset_row`, `build_custom_row`, `build_row`, and `build_endpoint_toggle` are removed. Their bodies become `view!` subtrees so that the count labels and endpoint date labels are named nodes. `TitleWidth` disappears with them: `set_hexpand` is written directly on each title label, `true` for the *Custom range...* row and `false` for the six presets.

This costs repetition — seven near-identical row subtrees instead of one helper plus seven calls. That is the price of the declarative bindings below, and it is accepted deliberately: the rows are static, they never gain a variant at runtime, and the repetition is visible in one place rather than split across a helper and its call sites.

The endpoint toggles and the Escape controller use the `#[wrap(Clone::clone)]` form described under [Macro Constraints](#named-children-of-by-value-assign-functions-need-wrapcloneclone).

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

`sync_button`, `sync_counts`, and all of `sync_custom_state` except the calendar date step are removed as a result.

The calendar accessible label is written with a watched `update_property` call. Its `#[cfg(test)]` recorder write has no place inside a `view!` property, so the watched expression calls a `&self` method that records the label and returns it, keeping the single-source-of-truth property.

### Endpoint group signal blocking

The endpoint group's active index becomes a watched property with a named `active-notify` handler and `#[block_signal]`.

This is a deliberate behavior change, not a neutral port. Today `sync_custom_state` calls `set_active` **unblocked**. When `CustomRangeRowSelected` resets `active_endpoint` to `Start` while the group is showing End, the `notify` fires for real and enqueues a `CustomEndpointChanged(Start)` message plus an extra `update_view` pass. The message is idempotent — `update` only assigns `active_endpoint`, which already holds `Start` — so no observable state, output, or rendering differs once it is suppressed.

Blocking it is the correct end state: programmatic reseeding is not a user endpoint change, and leaving it unblocked while the rest of the synchronization becomes declarative would make the redundant round trip harder to see, not easier. User-driven toggle changes still emit `CustomEndpointChanged` exactly as today.

A regression test must assert that entering the custom page from the End endpoint leaves `active_endpoint` at `Start` and produces no additional endpoint message.

### Calendar signal and controllers

`day-selected` is connected in `view!` with a named `@calendar_handler`. The generated handler ID is available from the generated widgets struct.

Calendar date synchronization remains conditional manual code in `post_view()`:

1. Resolve the target date from the active endpoint, falling back to today.
2. Compare it with the date already displayed by `GtkCalendar`.
3. If different and convertible, block `calendar_handler`.
4. Call `set_date`.
5. Unblock `calendar_handler`.

This is intentionally not a watched `set_date` binding. The explicit comparison and block sequence distinguishes programmatic seeding from user picks and protects accessibility announcements.

The click and key controllers remain manually created and connected. They are **constructed before** `view_output!()` — required by `additional_fields!`, which reads locals of the same name — and **attached after** it, once the macro has built the calendar, in the existing order: `GestureClick` first, `EventControllerKey` second. Both closures reach their widget through `gesture.widget()` / `controller.widget()`, so constructing them ahead of the calendar is safe.

GTK prepends controllers and dispatches from the head of the list, so the key controller stays ahead of the calendar's own — the macro also builds the calendar with `gtk::Calendar::new()`, which installs the calendar's controller first, exactly as today. The controller objects are declared through `additional_fields!` so existing ordering and behavior tests can inspect them.

### Model and generated widget ownership

`DatePill` retains widget handles needed directly by update-time effects and deferred callbacks: `listbox`, `popover`, `stack`, `calendar`, and `summary_label`. Those handles are cloned from generated widgets during initialization.

`calendar_handler` moves out of the model because the named macro connection owns it.

The `#[cfg(test)]` announcement and accessible-label recorders **cannot** move to `additional_fields!`, which does not accept `#[cfg]` attributes. They stay `#[cfg(test)]` fields of the `DatePill` model, where they already live today, and the affected tests read them through `ComponentController::model()` instead of `widgets()`. This is a mechanical call-site change at the eight existing assertion sites; no test intent changes.

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

The work proceeds through three independently compiling checkpoints in the same change:

1. Migrate `SortPill`. It is the smaller component, nothing in it is blocked, and it validates the macro plumbing before the risky component. Verify all tests.
2. Migrate the `DatePill` tree: component macro, inlined row and toggle subtrees, `#[wrap(Clone::clone)]` assignments, generated widgets struct, test recorders read through `model()`. Keep the existing `sync_*` bodies driven from `post_view()` so behavior is unchanged. Verify all tests.
3. Move the approved pure properties to `#[watch]`, delete `sync_button` and `sync_counts`, reduce `sync_custom_state` to `sync_calendar_date`, add the endpoint-blocking regression test, and verify again.

Checkpoint 2 is where the calendar invariants from PR #192 are at risk, and it deliberately changes no synchronization logic so that any failure there is a pure construction bug. The checkpoints are for regression isolation; they do not require separate pull requests.

## Scope Boundaries

In scope:

- `src/ui/date_pill.rs`
- `src/ui/sort_pill.rs`
- Dissolving `build_preset_row`, `build_custom_row`, `build_row`, `build_endpoint_toggle`, and `TitleWidth` into the `view!` tree
- Existing tests in those modules: adaptation to generated fields, and moving recorder assertions from `widgets()` to `model()`
- One new regression test for suppressed programmatic endpoint messages

Out of scope:

- Any visual, copy, localization, accessibility, keyboard, focus, or selection change
- New preset or sort options
- Replacing dynamic rows with a Relm4 factory
- Removing all widget handles from component models
- Introducing tracker state to use `#[track]`
- Deduplicating the seven inlined preset row subtrees behind a Relm4 widget template
- Screenshots or Flatpak packaging changes for this behavior-preserving refactor

## Acceptance Criteria

- Manual `DatePillWidgets` and `SortPillWidgets` definitions are removed.
- Both components use `#[relm4::component(pub)]`, `view!`, and `view_output!()`.
- `sync_button` and `sync_counts` are removed, and `sync_custom_state` is reduced to `sync_calendar_date`.
- `build_preset_row`, `build_custom_row`, `build_row`, `build_endpoint_toggle`, and `TitleWidth` are removed.
- All nine listed bindings are `#[watch]` properties in `view!`; none survive as imperative synchronization.
- Remaining imperative synchronization is limited to effects, controller attachment, the guarded calendar date step, and dynamic widget collections.
- The calendar key controller still precedes the calendar's own key controller.
- Programmatic calendar updates do not emit user-pick behavior or announcements.
- Programmatic endpoint-group reseeding emits no `CustomEndpointChanged`, and a test covers it.
- Text, accessibility properties, focus, selection, row ordering, and popover behavior are unchanged.
- No line-count target is imposed; the inlined row subtrees are expected to make `view!` longer than the helpers it replaces. Clarity and explicit invariants take priority.

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
- Preset rows still show their counts, aligned as before, with the chevron pushed to the right edge only on *Custom range...*.
- Switching to End, leaving the custom page, and re-entering it lands back on Start with the calendar seeded from the Start endpoint.

No screenshot update is required because the accepted scope forbids visual changes. No Flatpak build is required because dependencies, resources, and packaging are unchanged.
