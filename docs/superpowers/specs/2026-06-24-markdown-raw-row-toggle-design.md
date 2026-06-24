# Markdown Raw Row Toggle Design

**Date**: 2026-06-24  
**Status**: Approved for implementation planning  
**Related exploration**: `docs/explorations/2026-06-23-markdown-rendering-config-exploration.md`  
**Primary code area**: `src/ui/session_detail/transcript/`

## Goal

Assistant messages currently render markdown as a tree of GTK widgets. That improves readability, but makes whole-message selection impossible because each rendered block is its own selection island. Add a per-row `Select raw text` toggle for assistant messages so a user can collapse one assistant answer into a single selectable raw text label, copy it, and switch back without changing global preferences or rebuilding the transcript.

This design implements the adopted Mii Beta direction from the exploration: assistant-only rendering stays the rule, scope settings are not added, and the raw mode is ephemeral row state rather than GSettings.

## UX

Only assistant message rows get the control. User, tool result, tool call, tool burst, and subagent rows remain unchanged.

The control is a flat `GtkToggleButton` with the existing `code-symbolic` icon. It sits at the trailing end of the assistant message header, after the existing role/model/timestamp/reasoning metadata. The button is icon-only to keep the row compact.

The button has:

- tooltip: `Select raw text`
- accessible label: `Select raw text`
- active tooltip: `Show rendered markdown`
- active accessible label: `Show rendered markdown`
- pending tooltip: `Loading full message...`
- pending accessible label: `Loading full message`

Visibility follows a low-noise pattern:

- hidden by default when inactive;
- visible when the row is hovered or focus is within the row;
- always visible while raw mode is active;
- always visible while full-content loading is pending.

The visual states are:

- **Off**: assistant content renders through markdown as today.
- **Pending**: the user requested raw mode on a truncated assistant message and the full content is loading. The button is active, disabled, and shows loading feedback, preferably a compact spinner if that fits the existing GTK patterns. The row keeps its current rendered preview until full content arrives.
- **On**: the row content is a single selectable monospace `GtkLabel` containing the complete raw assistant message.

## Data Model

Add two ephemeral per-row bindings to `TranscriptItemData`:

- `raw: BoolBinding`
- `raw_pending_full_content: BoolBinding`

Both initialize to `false` in `TranscriptItemData::from_init`. The state is not persisted and is not stored in the database. It resets naturally when the transcript is rebuilt or the app restarts.

This mirrors the existing row-local state model used by `expanded` and `content_revision`, and avoids storing UI-only state in global `SessionDetail` fields. `raw` is coupled to `expanded` (see Data Flow): turning raw on also sets `expanded`, so raw always reflects the complete message.

## Data Flow

**Invariant: raw implies expanded.** Raw mode only ever shows a *complete* message — there is no "raw collapsed" state. Turning raw on therefore also sets `expanded = true`, which reuses the existing expand path to fetch full content when the message is truncated. This is intentional: a raw label built from a truncated preview would look copy-ready while being incomplete, so that state is forbidden by construction rather than guarded against. The coupling is one-directional — `raw = true ⇒ expanded = true`, but `expanded = true` does **not** imply raw.

When the toggle is clicked on an assistant message:

- If turning raw off, set `raw = false`, clear `raw_pending_full_content`, and pulse `content_revision`. **`expanded` is left unchanged** (it stays `true`): the full content is already loaded and on screen, so re-collapsing it under the user would hide content they are reading. Raw off simply re-renders the same complete content as markdown.
- If turning raw on and the message is not truncated, set `raw = true`, set `expanded = true`, and pulse `content_revision`.
- If turning raw on and full content is already loaded, set `raw = true`, set `expanded = true`, and pulse `content_revision`.
- If turning raw on and the message is truncated with no loaded full content, set `raw = true`, set `expanded = true`, set `raw_pending_full_content = true`, emit `SessionDetailMsg::RequestMessageFullContent { item_index }`, and keep the rendered preview visible while pending.

The fetch itself is the existing expand-triggered load path; no second loader is introduced. `raw_pending_full_content` is retained only to distinguish "flip this row to raw when content arrives" from an ordinary (non-raw) expand that happens to be loading.

When full content arrives, `SessionDetail::set_typed_message_full_content` stores it as today, clears `raw_pending_full_content`, and pulses `content_revision`. The next render sees `raw = true` and displays the complete raw text.

When full-content loading fails, the existing rollback path should also clear `raw_pending_full_content` and `raw`, then pulse `content_revision`. The row returns to the rendered markdown preview. This avoids showing a raw preview that looks copy-ready but is incomplete.

## Rendering

Extend `render_content` in `row_rendering.rs` with a `raw: bool` parameter.

The rendering branch becomes:

```rust
if role == Role::Assistant && !raw {
    markdown::render_markdown(content, highlight_query)
} else {
    // Single selectable GtkLabel.
}
```

For raw assistant content, the plain-label branch must:

- use `gtk::Label::new(Some(content))` or equivalent plain text assignment, not markup;
- set `selectable = true`;
- wrap with `gtk::pango::WrapMode::WordChar`;
- align left as existing plain rows do;
- add monospace styling.

For non-raw user/tool-result content, the current highlight behavior stays unchanged. For raw assistant content, `highlight_query` is intentionally ignored so the raw label remains faithful and simple to select. Global search counters may still be based on the underlying content; a raw row simply does not show in-row highlight markup.

## Implementation Boundaries

Expected file changes:

- `src/ui/session_detail/transcript/item_data.rs`: add and initialize `raw` and `raw_pending_full_content` bindings.
- `src/ui/session_detail/transcript/typed_row.rs`: add the raw toggle to `MessagePageWidgets`, bind visibility/state, handle clicks, and pass raw state into `render_message_body`.
- `src/ui/session_detail/transcript/row_rendering.rs`: add the `raw` parameter and raw plain-label styling.
- `src/ui/session_detail.rs`: clear raw pending state on full-content success or failure before bumping `content_revision`.
- `data/resources/style.css`: add minimal CSS for the compact raw toggle visibility rules and active/pending visibility.

Out of scope:

- GSettings keys.
- Preferences UI.
- Markdown rendering for user messages.
- Transcript-wide raw/source mode.
- Parser, database, or search-index schema changes.

## Alternatives Considered

**A direct "Copy message" button instead of a selection toggle.** A one-click action that loads full content and writes the raw markdown to the clipboard would serve the "copy the whole answer" case with no render-path change, no second render state, and no pending visual treatment beyond awaiting content. It was rejected: the goal is explicitly to let the user *choose the selection*, including copying only part of an answer. A copy-all button cannot do partial copy, whereas a selectable raw label covers both whole- and partial-message copy. The selection toggle is therefore the more general affordance for the stated intent. (A copy action could still be added later as a complement, but it is out of scope here.)

## Accessibility And Keyboard Behavior

The toggle must be keyboard reachable when focus is within the row. Hiding the inactive button must not make it inaccessible to keyboard users; focus-within should reveal it before or as it receives focus.

The button uses a clear accessible label because the visible UI is icon-only. The raw label improves keyboard copy compared to the rendered markdown tree because it is one selectable text widget.

The implementation should keep existing row focus and scroll behavior intact. In particular, it should avoid replacing the `GtkListView` item or causing list-model churn, following the existing `content_revision` pattern used for expansion.

## Testing

Automated tests should cover the stable behavior where feasible:

- `render_content` renders assistant markdown by default.
- `render_content` renders assistant raw content as one selectable monospace label.
- User and tool-result content remain plain-label content with existing highlight behavior.
- `TranscriptItemData::from_init` initializes `raw` and `raw_pending_full_content` to `false`.

Behavioral tests should be added where the existing test harness can support them without overbuilding infrastructure:

- Activating raw on a truncated assistant row emits `RequestMessageFullContent` and enters pending state.
- Full-content success clears pending and renders the complete raw content.
- Full-content failure clears pending/raw and returns to markdown preview.

Manual verification with `--sessions-dir tests/fixtures` should check:

- an assistant answer with paragraphs, code fence, and table cannot be selected as one block in rendered mode;
- toggling raw displays one selectable raw message;
- copy selection includes the complete raw markdown after full-content loading;
- toggling off returns to rendered markdown;
- scroll position does not jump;
- inactive buttons appear on hover/focus and active buttons remain visible;
- search-active raw rows do not show in-row highlights, while non-raw rows still do.

## Risks

The main implementation risk is interaction with `GtkListView` row recycling. The state must live on `TranscriptItemData`, and unbind/rebind must disconnect handlers and reset widget state just like existing message controls.

The second risk is pending full-content loading. The row should not display raw preview as if it were complete; pending mode keeps the rendered preview visible until the full body arrives.

The third risk is discoverability. The icon-only control is intentionally low-noise, so tooltip and focus behavior are important. If later feedback shows users miss it, the escalation path is a labelled action or context-menu entry, not a global setting.
