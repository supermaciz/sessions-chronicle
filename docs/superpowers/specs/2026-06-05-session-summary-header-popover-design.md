# Session Summary Header Popover Design

**Date**: 2026-06-05  
**Status**: Implemented [#164](https://github.com/supermaciz/sessions-chronicle/pull/164)  
**Source exploration**: `docs/explorations/2026-06-04-session-summary-collapse-exploration.md`  
**Selected proposal**: E, "The Summary Button"  

## Problem

After the fix that made the transcript `ListView` the direct child of its `ScrolledWindow`, the session summary moved outside the scrollable transcript. It is now fixed above the conversation and permanently consumes roughly 300-400 px of vertical space. The conversation is the primary content of the detail page, so the summary should stop occupying transcript space while remaining available on demand.

The selected design is not a collapse mechanism. It relocates the summary to the existing global header as a compact disclosure button with a popover.

## Goals

- Give the transcript the full height of the detail page.
- Preserve the #160 constraint: the transcript `ListView` remains the direct child of `transcript_scroller`.
- Avoid scroll-driven state: no `vadjustment`, hysteresis, pinning, auto-collapse, or automatic reopen.
- Reuse the existing global `adw::HeaderBar`; do not introduce a nested detail header.
- Leave the center `Sessions | Analytics` switcher and existing end-side header actions unchanged.
- Keep `F9` behavior unchanged: filters in list view, inspector in detail view.
- Preserve all current summary data: project, path, session ID, AI assistant, duration, message count, ending status, first prompt, activity, and tokens.

## Non-Goals

- No scroll-reactive summary behavior.
- No permanent recap bar inside the transcript column.
- No summary integration into the inspector panel.
- No new keyboard shortcut for the summary in the first implementation.
- No extra database query solely for the header button.

## Chosen Approach

`App` owns the header affordance. `SessionDetail` owns the rich summary content.

The existing header gets a detail-scoped `gtk::MenuButton` in the start cluster, after the back, pin, and search controls. The button is visible only while a session detail page is active and a session is loaded. It displays a neutral summary icon, the project name, and a chevron.

The rich summary content moves out of the detail page's vertical flow and into a `gtk::Popover` attached to that button. The detail content column then contains only `transcript_scroller`, giving the conversation the full height.

This design deliberately keeps the summary and the inspector as two distinct surfaces (proposal D folded them together; E does not). The summary is fixed session metadata; the inspector is exploratory, per-message context. They are not merged here, and the popover does not duplicate inspector content. If the two surfaces later prove confusing to have side by side, consolidating them is a separate decision, out of scope for this spec.

## Component Design

### App Header

`src/app/mod.rs` adds a `summary_menu_button` to the existing global `adw::HeaderBar` start cluster, placed after `search_toggle`. This is not a new header and does not replace the header title widget.

Visibility is driven by app state:

- Visible when `detail_visible && are_detail_actions_visible() && active_session.is_some()`.
- Hidden in the session list, in the Analytics workspace, during states without an active session, and whenever detail actions are intentionally hidden.

`ActiveSessionRef` needs no new field: the button uses only `project_name`, which already exists and stops being dead code. The existing `pinned`, `id`, and `tool` fields continue to support the current pin and resume behavior but are unrelated to the summary button. The ending status does not surface in the header — it stays in the popover's ending-status chip, which `SessionDetail` populates from the full `Session`. The button does not need duration, message count, tokens, path, first prompt, or ending status; those remain in `SessionDetail`.

The button content is a compact horizontal box:

- A neutral summary icon, a standard symbolic such as `document-properties-symbolic`, leading the label so the glyph reads as "session details" rather than as a generic title menu.
- Project label, ellipsized at the end.
- Chevron image, using a standard symbolic icon such as `pan-down-symbolic`.

An earlier draft led with a status-colored dot derived from `ending_status`. That was dropped: the ending status is a triage fact, most useful in the session list when deciding what to open, not while reading the transcript. As the single permanent slot in the detail header it encoded low-frequency information and falsely signaled importance through color, while duplicating the popover chip. A neutral summary glyph carries no such false signal and reinforces what the button is.

The button carries an explicit `tooltip-text` ("Session summary") and an `accessible-label` so the affordance reads as the session summary, not as a project or title menu. A bare `project ▾` button is otherwise ambiguous, and the full summary (duration, tokens, first prompt, activity) is no longer ambient — discoverability rests entirely on this affordance being legible.

On narrow windows, the project name may ellipsize aggressively. The durable affordance is the summary icon plus chevron.

### SessionDetail Summary Content

`src/ui/session_detail.rs` removes `summary_box` from the content column above `transcript_scroller`. The summary subtree is extracted into a declarative `SessionSummary` `WidgetTemplate`, which `SessionDetail` creates and hosts inside its own `gtk::Popover`.

The `SessionSummary` template root is a bounded `gtk::ScrolledWindow`; the existing vertical summary box becomes its child. The scroller disables horizontal scrolling, allows vertical scrolling when needed, propagates the child's natural height up to an explicit maximum content height, and sets reasonable minimum/maximum content widths. This lets compact summaries use their natural height while preventing long summaries or paths from growing the popover to the full window height or an excessive width. The summary box retains its existing margins, wrapping, and selectable path/session-ID labels.

The template preserves the existing named summary widgets where practical:

- `project_label`
- `path_label`
- `session_id_row` / `session_id_label`
- `chip_row` and its AI assistant, duration, message count, and ending-status children
- `first_prompt_section`
- `activity_section`
- `tokens_section`

`SessionSummary` exposes one module-private `update(&Session)` method. `SessionDetail::post_view` calls it when a session is active, and it delegates to the existing focused update helpers moved onto `SessionSummary`:

- `update_session_header`
- `update_chip_row`
- `update_first_prompt`
- `update_activity_section`
- `update_tokens_section`

### Widget Ownership

The `MenuButton` must live in `App`'s global `adw::HeaderBar`, while the summary content belongs to `SessionDetail`. Relm4's `ComponentController` exposes both `.widget()` and `.widgets()`, so `App` can clone a deliberately public widget handle from `SessionDetail` without introducing a setup message or runtime reparenting.

**Decision: SessionDetail owns the `Popover`; App's `MenuButton` adopts it.**

`SessionDetail` uses `additional_fields!` to retain the template and expose only the popover:

```rust
additional_fields! {
    pub summary_popover: gtk::Popover,
    summary: SessionSummary,
}
```

During `SessionDetail::init`, it creates the template and sets it as the popover child. In `App::init`, after `view_output!()` has created `summary_menu_button`, `App` retrieves a cloned popover handle through `model.session_detail.widgets().summary_popover` and assigns it to the button with `set_popover`.

This keeps the summary declarative and owned by `SessionDetail`; `App` knows only the popover handle required for header placement. No `AttachSummaryHost` message, custom controller accessor, duplicated summary content, or runtime reparenting is needed. GTK object handles are reference-counted, so cloning the handle does not duplicate the popover or its content.

The boundary remains: `App` controls header placement, button visibility, and closing the popover during app-level navigation or session replacement; `SessionDetail` controls rich summary construction and rendering.

## Data Flow

When a session is selected, `App` already loads the full `Session` before forwarding it to `SessionDetail`. During that same step, `App` updates `ActiveSessionRef` with its existing fields:

- `id`
- `tool`
- `project_name`
- `pinned`

No new field is added. `ActiveSessionRef` drives only the header button label (`project_name`), pin state, and existing resume/pin behavior. The ending status is not propagated to `App`; it reaches the popover chip through the full `Session` in `SessionDetail`.

The full `Session` still flows to `SessionDetailMsg::SetSession`, and `SessionDetail` uses it to populate the summary popover and transcript. There is no extra load and no duplication of full summary formatting beyond the compact header button.

The popover open/closed state belongs to `gtk::MenuButton` / `gtk::Popover`; no mirrored model flag is added. The popover keeps its default modal/autohide behavior so clicking outside it or pressing `Esc` dismisses it before the app-level escape action runs.

`App` centralizes dismissal in a small helper that calls `popdown()` on the exposed `summary_popover`. Call it before every successful `SessionDetailMsg::SetSession`, before `SessionDetailMsg::Clear`, and when transitioning back to session-list mode. This covers direct selection, child-session navigation, return-to-parent navigation, load failures that clear the active session, and navigation back without duplicating popover state in the model.

## Behavior

The summary becomes consult-on-demand information:

- The transcript receives the full vertical height permanently.
- The user opens the summary by clicking the header button.
- The user closes the popover by clicking away or pressing `Esc` while the popover is active.
- The app-level `F9` dispatcher remains unchanged.
- The app-level `Escape` behavior remains search -> inspector -> navigate back when no popover handles `Esc` first.
- When switching sessions, the popover is closed before the new session data is displayed.
- When returning to the list, the popover is closed and the button disappears.

The popover content is height-bounded by the `SessionSummary` root scroller. If the full summary exceeds that bound, it scrolls internally. Transcript scrolling and transcript layout are unaffected.

The popover hosts heavier content than a popover usually carries (a tokens grid, an activity bar, and selectable text such as the path and session ID). A popover that dismisses on click-away is an awkward surface for selecting and copying text. This is accepted for the first implementation, but it is the design's known pressure point: if real use shows users want to copy the path or session ID, the correct pivot is to move the summary into an `adw::Dialog` rather than to grow the popover. The header button and its visibility logic stay the same across that pivot.

## Error Handling And Edge Cases

- No active session: hide the summary button.
- Unknown project: show `Unknown project`, matching current summary behavior.
- Unknown ending status: render the popover's ending-status chip with the existing unknown-status semantic style. The header button is unaffected, since it no longer carries ending status.
- Overlong project name: ellipsize the button label; do not expand the header or push the center switcher.
- Overlong summary content: scroll inside the popover.
- Session switch while the popover is open: close the popover, then display the new session data.
- Navigation back while the popover is open: close the popover, hide the button, and leave no summary data visible.

## Testing Plan

- Preserve the existing structural regression test that asserts `transcript_scroller.child() == messages.view`.
- Add or adapt a test showing that the detail content column no longer contains a fixed `summary_box` above the transcript.
- Add an app/header state test: the summary button is visible only in detail mode with an active session.
- Add a test verifying that the header button label uses the active session project name.
- Keep or adapt `SessionDetail` tests proving that prompt, chips, activity, and tokens are populated in the summary widgets.
- Add a structural test proving that `summary_menu_button.popover()` is the same popover exposed by `SessionDetail`.
- Add a structural test proving that the popover child is the `SessionSummary` root and that the summary is no longer parented in the detail content column.
- Add a test for the centralized dismissal helper, including an open popover followed by session replacement or transition to list mode.
- Add a regression test or helper-level test confirming that `F9` still routes to filters in list mode and inspector in detail mode.

## Implementation Notes

- Reuse existing ending-status formatting helpers (`ending_css_class` and accessible labels) for the popover's ending-status chip; the header button no longer carries ending status.
- Avoid broad header styling changes; the summary icon is a standard symbolic and should not need custom CSS.
- Implement the summary subtree as a `WidgetTemplate`; do not build it imperatively or duplicate it in `App`.
- Expose only `summary_popover` across the component boundary. Keep the `SessionSummary` template and its individual widgets private to `SessionDetail`.
- Keep popover dismissal imperative and centralized in `App`; do not add a mirrored open/closed model flag.
- Keep the popover's default autohide behavior; do not intercept `Esc` inside the summary.
- Keep the design reversible: the transcript/scroller structure should become simpler, not more coupled to header state.
