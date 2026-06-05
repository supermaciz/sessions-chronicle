# Session Summary Header Popover Design

**Date**: 2026-06-05  
**Status**: Approved design, implementation plan pending  
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

`ActiveSessionRef` needs no new field: the button uses only `project_name` (which already exists and stops being dead code) and the existing `pinned` state. The ending status does not surface in the header — it stays in the popover's ending-status chip, which `SessionDetail` populates from the full `Session`. The button does not need duration, message count, tokens, path, first prompt, or ending status; those remain in `SessionDetail`.

The button content is a compact horizontal box:

- A neutral summary icon, a standard symbolic such as `document-properties-symbolic`, leading the label so the glyph reads as "session details" rather than as a generic title menu.
- Project label, ellipsized at the end.
- Chevron image, using a standard symbolic icon such as `pan-down-symbolic`.

An earlier draft led with a status-colored dot derived from `ending_status`. That was dropped: the ending status is a triage fact, most useful in the session list when deciding what to open, not while reading the transcript. As the single permanent slot in the detail header it encoded low-frequency information and falsely signaled importance through color, while duplicating the popover chip. A neutral summary glyph carries no such false signal and reinforces what the button is.

The button carries an explicit `tooltip-text` ("Session summary") and an `accessible-label` so the affordance reads as the session summary, not as a project or title menu. A bare `project ▾` button is otherwise ambiguous, and the full summary (duration, tokens, first prompt, activity) is no longer ambient — discoverability rests entirely on this affordance being legible.

On narrow windows, the project name may ellipsize aggressively. The durable affordance is the summary icon plus chevron.

### SessionDetail Summary Content

`src/ui/session_detail.rs` removes `summary_box` from the content column above `transcript_scroller`. The summary subtree is re-hosted in the popover content associated with the header button.

The existing named summary widgets should remain in use where practical:

- `project_label`
- `path_label`
- `session_id_row` / `session_id_label`
- `chip_row` and its AI assistant, duration, message count, and ending-status children
- `first_prompt_section`
- `activity_section`
- `tokens_section`

The existing update helpers continue to populate those widgets:

- `update_session_header`
- `update_chip_row`
- `update_first_prompt`
- `update_activity_section`
- `update_tokens_section`

### Widget Ownership (must be resolved before coding)

This is the highest-risk part of the design and is not optional to settle. The summary widgets (`ending_status_chip`, `tokens_section`, and the rest) are built declaratively in `SessionDetail`'s `view!` macro, and the update helpers write into `widgets.<name>`. The `MenuButton` must live in `App`'s global `adw::HeaderBar`. But `App` only sees `SessionDetail` as a `Controller<SessionDetail>`, exposing `.widget()` (the root) plus message passing — not an arbitrary inner widget such as a popover. The two plausible structures are not equivalent in cost:

- **Option 1 — SessionDetail owns the `Popover`, App's `MenuButton` adopts it.** Requires `App` to retrieve the popover from the controller, which Relm4 does not offer natively (only `.widget()` and messages). Needs a custom accessor on the component, which couples the controller's public surface to one inner widget.
- **Option 2 — App owns the `MenuButton` and an empty `Popover`; SessionDetail receives the popover's content container through a setup message** (for example `SessionDetailMsg::AttachSummaryHost(gtk::Box)`) and builds or reparents the summary into it. This matches the existing App→SessionDetail message pattern (`SessionDetailMsg::CloseInspector`). Its cost is that the summary subtree is no longer purely declarative in `SessionDetail`'s `view!`, or must be reparented at runtime — the fragile part.

**Decision: Option 2.** It aligns with the message-passing boundary already used between `App` and `SessionDetail` and keeps `App` in control of header placement and visibility. The implementation must prove this wiring with a focused spike before building the full summary content, so the reparenting/ownership cost is discovered early rather than mid-implementation. The boundary remains: `App` controls header placement and visibility, while `SessionDetail` controls rich summary rendering.

## Data Flow

When a session is selected, `App` already loads the full `Session` before forwarding it to `SessionDetail`. During that same step, `App` updates `ActiveSessionRef` with its existing fields:

- `id`
- `tool`
- `project_name`
- `pinned`

No new field is added. `ActiveSessionRef` drives only the header button label (`project_name`), pin state, and existing resume/pin behavior. The ending status is not propagated to `App`; it reaches the popover chip through the full `Session` in `SessionDetail`.

The full `Session` still flows to `SessionDetailMsg::SetSession`, and `SessionDetail` uses it to populate the summary popover and transcript. There is no extra load and no duplication of full summary formatting beyond the compact header button.

The popover open/closed state belongs to `gtk::MenuButton` / `gtk::Popover`. No model flag is added unless tests or Relm4 wiring prove one necessary.

## Behavior

The summary becomes consult-on-demand information:

- The transcript receives the full vertical height permanently.
- The user opens the summary by clicking the header button.
- The user closes the popover by clicking away or pressing `Esc` while the popover is active.
- The app-level `F9` dispatcher remains unchanged.
- The app-level `Escape` behavior remains search -> inspector -> navigate back when no popover handles `Esc` first.
- When switching sessions, the popover is closed before the new session data is displayed.
- When returning to the list, the popover is closed and the button disappears.

The popover is height-bounded. If the full summary exceeds available height, the popover content scrolls internally. Transcript scrolling and transcript layout are unaffected.

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
- Add a regression test or helper-level test confirming that `F9` still routes to filters in list mode and inspector in detail mode.

The cross-component popover wiring (the `Widget Ownership` decision) is the hardest part to cover automatically and is not well suited to headless testing. The spike that validates Option 2 stands in for an automated test of that wiring; cover what is testable around it (visibility state, project-name propagation, summary-widget population) rather than asserting the parenting itself.

## Implementation Notes

- Reuse existing ending-status formatting helpers (`ending_css_class` and accessible labels) for the popover's ending-status chip; the header button no longer carries ending status.
- Avoid broad header styling changes; the summary icon is a standard symbolic and should not need custom CSS.
- Prefer the smallest viable Relm4 boundary. Do not duplicate the full summary in `App`.
- Keep the design reversible: the transcript/scroller structure should become simpler, not more coupled to header state.
