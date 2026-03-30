# Session Summary View — Design

**Issue**: [#91 — Deterministic structured session summary](https://github.com/supermaciz/sessions-chronicle/issues/91)
**Date**: 2026-03-30
**Status**: Validated
**Exploration**: [2026-03-28-session-summary-exploration.md](2026-03-28-session-summary-exploration.md)
**Decision**: Proposal B (Unified Session Header) + 3 navigation anchors from Proposal D

---

## Problem Statement

When a user opens a session, the app drops them into the full transcript with only a
minimal metadata card above it. The most common review questions remain unanswered at
a glance: what happened, how much activity, how it ended, and where to look first.

## Scope

Replace the existing `.card` metadata box with a flush unified session header. Add a
compact navigation anchor row that links to key transcript positions. No LLM-generated
text — everything deterministic from indexed data.

---

## Section 1: Session Identity

Flush `gtk::Box` (vertical) at the top of the scroll area. No `.card` wrapper.

**Content:**

- **Project name** — `gtk::Label.title-2`, left-aligned, wrapping
- **Path** — `gtk::Label.dim-label`, left-aligned, selectable, wrapping
- **Session ID row** — horizontal `gtk::Box`: "Session ID:" dim label + `gtk::Label.monospace` selectable
- **Chip row** — horizontal `gtk::Box` with `.pill` labels:
  - AI assistant icon + name
  - Duration: computed from `last_updated - start_time` (e.g. "2h 14m")
  - Message count: "42 messages"
  - Ending status: semantic color pill (green=clean, amber=abrupt, red=error, dim=unknown)
    with `accessible-label` ("Session ended cleanly" / "Session ended unexpectedly" / "Session ended with error")

**Changes from current:**

- Remove `.card` CSS class (padding, border-radius, background)
- Replace free-form timestamp label ("Started X ago . Updated Y ago") with computed duration chip
- Add ending status pill (currently not displayed)
- Session ID stays visible and selectable (not moved to tooltip)

---

## Section 2: First Prompt

Separated from identity by `GtkSeparator`. Conditional — hidden entirely when `first_prompt`
is `None` or empty. The separator before this section is also hidden when this section is hidden.

**Content:**

- **Section heading** — `gtk::Label.section-heading` "FIRST PROMPT"
- **Prompt text** — `gtk::Label` with `wrap: true`, `max_width_chars: 80`, `lines: 3`,
  `ellipsize: End`

**Data source:** `session.first_prompt` (already available).

---

## Section 3: Activity Breakdown

Separated by `GtkSeparator`. Shows what happened in the session at a glance.

**Content:**

- **Section heading** — "ACTIVITY"
- **Proportional bar** — horizontal `gtk::Box` (8px height, 4px border-radius) with colored
  child boxes whose width ratios reflect edit/command/read counts:
  - Orange (`#e66100`) — edits
  - Green (`#26a269`) — commands
  - Blue (`#3584e4`) — reads
- **Legend row** — horizontal `gtk::Box` with colored dots + count labels:
  "14 edits . 9 commands . 3 reads"
- **Fallback** — when all counts are zero: single label "Conversation only"

**Bar implementation:** Each colored segment is a `gtk::Box` child inside a horizontal parent.
Compute percentage widths and use `set_size_request(width_px, 8)` after measuring the
parent allocation. No custom `GtkDrawingArea` needed.

**Accessibility:** `accessible-label` on the bar container: "Activity: 14 edits, 9 commands,
3 reads" (text summary of the visual).

**Data source:** `session.edit_count`, `session.command_count`, `session.read_count`
(already available).

---

## Section 4: Tokens

Separated by `GtkSeparator`. Conditional — hidden entirely when `token_usage` is `None`.

**Content:**

- **Section heading** — "TOKENS"
- **Horizontal grid** — `gtk::Box (horizontal, homogeneous)` with value/label pairs:
  - Input: value on top (bold, `tabular-nums`), "Input" below (dim, small)
  - Output: same pattern
  - Cache read: conditional, hidden if `None`
  - Reasoning: conditional, hidden if `None`
- Tooltip on section: `token_semantics_help_tooltip()` (already exists in `format.rs`)

**Adaptive behavior:**

- Wide (>600px): all values in one horizontal row
- Narrow (<600px): wrap to 2x2 grid

**Data source:** `session.token_usage` (already available).

---

## Section 5: Navigation Anchors

No section heading. A compact horizontal `gtk::Box` with `spacing: 8` containing 1-3
pill-styled buttons.

**Buttons:**

| Button | Condition | Target |
|--------|-----------|--------|
| "First prompt" | Always shown | Transcript row index 0 |
| "First error: `{tool_name}`" | Only if an error tool call exists | Transcript row at the error's `item_index` |
| "Final message" | Always shown | Last transcript row |

**Scroll behavior:**

- **First prompt**: target is index 0, always loaded. Use existing `scroll_to_item` mechanism.
- **First error**: target is `first_error.item_index`. If within `loaded_count`, scroll directly.
  Otherwise, set `loading_anchor_target` and trigger `LoadMore` in a loop until target is loaded.
- **Final message**: target is `total_transcript_count - 1`. Same load-until-ready pattern as
  first error.

While loading toward an anchor target, the button shows a `gtk::Spinner` replacing its label.

**Edge cases:**

- Session with 0 transcript items: hide all anchors
- Session with <200 items: "Final message" already loaded, no extra loading needed
- Very long session (>1000 items): bulk loading triggered by "Final message"

---

## Widget Tree

```
scroll_child (gtk::Box, vertical, spacing=0, margin=16)
  // Section 1: Session Identity (flush, no card)
  project_label (gtk::Label.title-2)
  path_label (gtk::Label.dim-label, selectable)
  session_id_box (gtk::Box, horizontal)
    "Session ID:" label (dim-label)
    session_id_label (gtk::Label.monospace, selectable)
  chip_row (gtk::Box, horizontal, spacing=8)
    tool_chip (gtk::Box: Image + Label)
    duration_chip (gtk::Label.pill)
    message_count_chip (gtk::Label.pill)
    ending_status_chip (gtk::Label.pill.ending-{clean,interrupted,failed})

  gtk::Separator

  // Section 2: First Prompt (conditional)
  first_prompt_heading (gtk::Label.section-heading "FIRST PROMPT")
  first_prompt_label (gtk::Label, wrap, lines=3, ellipsize=End)

  gtk::Separator  // hidden when first_prompt is empty

  // Section 3: Activity
  activity_heading (gtk::Label.section-heading "ACTIVITY")
  activity_bar (gtk::Box.activity-bar, horizontal)
    edit_segment (gtk::Box.activity-edits)
    command_segment (gtk::Box.activity-commands)
    read_segment (gtk::Box.activity-reads)
  legend_row (gtk::Box, horizontal, spacing=12)
    colored dot + count labels

  gtk::Separator

  // Section 4: Tokens (conditional)
  tokens_heading (gtk::Label.section-heading "TOKENS")
  tokens_grid (gtk::Box, horizontal, homogeneous)
    input_pair (gtk::Box, vertical: value + label)
    output_pair (gtk::Box, vertical: value + label)
    cache_pair (gtk::Box, vertical, conditional)
    reasoning_pair (gtk::Box, vertical, conditional)

  gtk::Separator  // hidden when tokens not available

  // Section 5: Navigation Anchors
  anchor_row (gtk::Box, horizontal, spacing=8)
    first_prompt_btn (gtk::Button.pill "First prompt")
    first_error_btn (gtk::Button.pill "First error: {name}", conditional)
    final_message_btn (gtk::Button.pill "Final message")

  gtk::Separator  // heavier, marks transcript start

  // Section 6: Transcript (unchanged)
  messages_box (FactoryVecDeque<TranscriptRow>)
  load_more_button
```

---

## Database Queries

### New: `load_first_error_tool_call`

```sql
SELECT tc.id, tc.tool_name, ti.item_index
FROM tool_calls tc
JOIN transcript_items ti
  ON ti.session_id = tc.session_id AND ti.tool_call_id = tc.id
WHERE tc.session_id = ?1 AND tc.status = 'Error'
ORDER BY ti.item_index ASC
LIMIT 1
```

Returns `Option<(String, String, usize)>` — `(tool_call_id, tool_name, item_index)`.

### New: `count_transcript_items`

```sql
SELECT COUNT(*) FROM transcript_items WHERE session_id = ?1
```

Returns `usize`. Needed for "Final message" anchor target.

### No schema migration

All data comes from existing tables and columns.

---

## Model Changes

### `SessionDetail` struct additions

```rust
first_error: Option<FirstErrorInfo>,
total_transcript_count: usize,
loading_anchor_target: Option<usize>,  // item_index being scrolled to
```

### New struct

```rust
pub struct FirstErrorInfo {
    pub tool_call_id: String,
    pub tool_name: String,
    pub item_index: usize,
}
```

### New messages

```rust
enum SessionDetailMsg {
    // existing messages unchanged...
    JumpToFirstPrompt,
    JumpToFirstError,
    JumpToFinalMessage,
}
```

### Jump-to logic

All three handlers use the existing `scroll_to_item` mechanism:

1. **JumpToFirstPrompt** — set `scroll_to_item = Some(0)`, always loaded
2. **JumpToFirstError** — if `item_index < loaded_count`, scroll directly;
   otherwise set `loading_anchor_target` and trigger `LoadMore` loop
3. **JumpToFinalMessage** — target is `total_transcript_count - 1`, same
   load-until-ready pattern

The `LoadMore` handler checks `loading_anchor_target` after each page load.
If target is now within `loaded_count`, scroll and clear. If not, trigger another `LoadMore`.

---

## CSS

### New classes

```css
.section-heading {
  font-size: 0.75rem;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  opacity: 0.55;
  margin-top: 8px;
  margin-bottom: 4px;
}

.pill {
  padding: 2px 10px;
  border-radius: 99px;
  font-size: 0.85rem;
  background-color: alpha(@card_bg_color, 0.6);
}

.ending-clean {
  color: #26a269;
}

.activity-bar {
  min-height: 8px;
  border-radius: 4px;
  overflow: hidden;
}

.activity-edits { background-color: #e66100; }
.activity-commands { background-color: #26a269; }
.activity-reads { background-color: #3584e4; }

.token-value {
  font-size: 1.1rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
```

### Modified classes

- `.ending-interrupted` and `.ending-failed` already exist, unchanged
- `.card` usage removed from session detail metadata box

---

## Accessibility

| Element | Treatment |
|---------|-----------|
| Project name | Standard label, focusable via Tab |
| Path, Session ID | Selectable text |
| Chip row pills | `accessible-label` on each: "Duration: 2 hours 14 minutes", etc. |
| Ending status pill | `accessible-label`: "Session ended cleanly/unexpectedly/with error" |
| Section headings | `accessible-role: heading` |
| Activity bar | `accessible-label` on container: "Activity: 14 edits, 9 commands, 3 reads" |
| Token values | `accessible-label` on each pair: "Input tokens: 124,832" |
| Anchor buttons | Standard `GtkButton`, keyboard focusable, Enter/Space activatable |

**Focus order:** path -> session ID -> anchor buttons -> transcript rows -> load more button

No color-only encoding. Icons, text, and labels carry meaning alongside color.

## Adaptive Behavior

| Breakpoint | Behavior |
|------------|----------|
| Wide (>600px) | Chip row horizontal. Token pairs in one row. Legend in one row. |
| Narrow (<600px) | Chip row wraps. Token pairs 2x2. Legend wraps. Anchor buttons stack. |

Activity bar always full-width (proportional, compresses well).

## High Contrast

- Ending status pills use `@warning_color` / `@error_color` (libadwaita adapts)
- Activity bar segments: distinct hues + legend text as backup
- `.section-heading` opacity may need increase to 0.7 in high-contrast — verify during testing

---

## What Gets Deleted

- `.card` CSS class usage on the metadata box in `session_detail.rs`
- Free-form timestamp row ("Started X ago . Updated Y ago")
- Token display inside the metadata card (moves to dedicated section)

## What Stays Unchanged

- Transcript rows (`FactoryVecDeque<TranscriptRow>`)
- Load more button and pagination logic
- Search overlay bar
- Tool call and subagent inspector integration
- All existing `format.rs` helpers

---

## Files to Touch

| File | Change |
|------|--------|
| `src/ui/session_detail.rs` | Restructure view macro, replace card with flush sections, add anchor buttons + jump handlers, add `FirstErrorInfo` and new model fields |
| `src/ui/format.rs` | Add `format_duration()` for session duration chip, `format_ending_label()` for status pill text |
| `src/database/mod.rs` | Add `load_first_error_tool_call()` and `count_transcript_items()` queries |
| `data/resources/style.css` | Add `.section-heading`, `.pill`, `.ending-clean`, `.activity-bar`, `.activity-*`, `.token-value`. Remove `.card` usage from detail view. |

## Implementation Risk

The hardest part is the **jump-to-anchor scroll logic** for targets beyond the loaded page.
The `LoadMore` loop must:
1. Set `loading_anchor_target`
2. Trigger `LoadMore`
3. On completion, check if target is loaded
4. If not, trigger another `LoadMore`
5. Once loaded, scroll and clear

This must not freeze the UI. Each `LoadMore` is already async-friendly since it posts
a message. The loop is message-driven, not blocking.

## Verification Plan

- `--sessions-dir tests/fixtures` covers multiple AI assistants
- Sessions with zero errors: "First error" anchor must be absent
- Sessions with errors: anchor appears with tool call name
- Sessions with zero tool calls: activity shows "Conversation only"
- Short sessions: header height stays under ~300px
- Long first prompt: 3-line ellipsis works
- No token data: tokens section hides cleanly
- No first prompt: section hides, separator hides
- Narrow width: chips wrap, tokens go 2x2, anchors stack
- High contrast theme: semantic colors remain legible
- Paginated jump: "Final message" on a 500+ item session loads all pages then scrolls

---

## References

- [Issue #91](https://github.com/supermaciz/sessions-chronicle/issues/91)
- [Exploration doc](2026-03-28-session-summary-exploration.md)
- [Product Assessment](../PRODUCT_ASSESSMENT_2026-03-21.md) — recommendation 2, Direction 1
- Related: #55, #36, #70, #74, #79, #80, #81
