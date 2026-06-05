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

The existing header gets a detail-scoped `gtk::MenuButton` in the start cluster, after the back, pin, and search controls. The button is visible only while a session detail page is active and a session is loaded. It displays a status-colored dot, the project name, and a chevron.

The rich summary content moves out of the detail page's vertical flow and into a `gtk::Popover` attached to that button. The detail content column then contains only `transcript_scroller`, giving the conversation the full height.

## Component Design

### App Header

`src/app/mod.rs` adds a `summary_menu_button` to the existing global `adw::HeaderBar` start cluster, placed after `search_toggle`. This is not a new header and does not replace the header title widget.

Visibility is driven by app state:

- Visible when `detail_visible && are_detail_actions_visible() && active_session.is_some()`.
- Hidden in the session list, in the Analytics workspace, during states without an active session, and whenever detail actions are intentionally hidden.

`ActiveSessionRef` gains the active session's `ending_status`. `project_name` already exists and becomes used by the summary button. The button does not need duration, message count, tokens, path, or first prompt; those remain in `SessionDetail`.

The button content is a compact horizontal box:

- Status dot, styled from `ending_status` with the same semantic colors as the ending-status chip.
- Project label, ellipsized at the end.
- Chevron image, using a standard symbolic icon such as `pan-down-symbolic`.

On narrow windows, the project name may ellipsize aggressively. The durable affordance is the status dot plus chevron.

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

If Relm4 widget ownership makes direct cross-component parenting too fragile, `App` owns only the `MenuButton` shell and `SessionDetail` receives a popover container through a focused setup method. The boundary remains unchanged: `App` controls header placement and visibility, while `SessionDetail` controls rich summary rendering.

## Data Flow

When a session is selected, `App` already loads the full `Session` before forwarding it to `SessionDetail`. During that same step, `App` updates `ActiveSessionRef` with:

- `id`
- `tool`
- `project_name`
- `pinned`
- `ending_status`

`ActiveSessionRef` drives only the header button label, the status dot style, pin state, and existing resume/pin behavior.

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

## Error Handling And Edge Cases

- No active session: hide the summary button.
- Unknown project: show `Unknown project`, matching current summary behavior.
- Unknown ending status: use the existing unknown-status semantic style.
- Overlong project name: ellipsize the button label; do not expand the header or push the center switcher.
- Overlong summary content: scroll inside the popover.
- Session switch while the popover is open: close the popover, then display the new session data.
- Navigation back while the popover is open: close the popover, hide the button, and leave no summary data visible.

## Testing Plan

- Preserve the existing structural regression test that asserts `transcript_scroller.child() == messages.view`.
- Add or adapt a test showing that the detail content column no longer contains a fixed `summary_box` above the transcript.
- Add an app/header state test: the summary button is visible only in detail mode with an active session.
- Add a test verifying that the header button uses the active session project name and ending status.
- Keep or adapt `SessionDetail` tests proving that prompt, chips, activity, and tokens are populated in the summary widgets.
- Add a regression test or helper-level test confirming that `F9` still routes to filters in list mode and inspector in detail mode.

## Implementation Notes

- Reuse existing ending-status formatting helpers where possible, especially `ending_css_class` and accessible labels.
- Add only narrowly targeted CSS if the status dot needs it; avoid broad header styling changes.
- Prefer the smallest viable Relm4 boundary. Do not duplicate the full summary in `App`.
- Keep the design reversible: the transcript/scroller structure should become simpler, not more coupled to header state.
