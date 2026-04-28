# Tool Call Grouping Design (Issue #89)

**Status:** Implemented [#110](https://github.com/supermaciz/sessions-chronicle/pull/110)

## Problem

The session detail view currently renders each tool call as its own transcript row.
Real AI assistant sessions often contain bursts of 5-20 consecutive tool calls,
which creates a tall wall of visually similar rows and pushes the actual
conversation off-screen.

The unit users need to understand is not one tool call at a time, but the burst
of assistant activity between conversational messages.

## Decision Summary

This design implements the decision recorded in
`docs/explorations/2026-04-01-tool-call-grouping-exploration.md`:

- Use a GNOME-native `GtkExpander` burst row.
- Group all consecutive tool calls into one burst when the run length is `>= 2`.
- Leave isolated single tool calls unchanged.
- Show category pills, total duration when available, total tool call count, and
  explicit error text in the collapsed header.
- Auto-expand only when search navigation targets an active match inside a
  collapsed burst.

## Scope

This design covers:

1. Burst detection in the session detail transcript pipeline.
2. A new transcript display unit for grouped tool call bursts.
3. Collapsed and expanded interaction behavior.
4. Search behavior inside grouped tool calls.
5. Accessibility, styling, and verification requirements.

Out of scope:

- Database schema changes.
- Parser changes.
- Narrative summary generation for burst headers.
- Duration-proportional timeline or strip visualizations.
- Any implementation plan.

## Design Constraints

- `SessionDetail` currently loads a flat ordered stream of `TranscriptItemRow`
  values from `load_transcript_items()` and pushes them directly into a
  `FactoryVecDeque<TranscriptRow>`.
- The current transcript search flow tracks match counts per displayed row.
  Today, only message rows contribute those counts.
- The existing tool call row style is compact and visually distinct, with an
  orange left border in `data/resources/style.css`.
- The app already uses Relm4 factories for transcript rendering, so the design
  should preserve efficient incremental UI updates and avoid custom list
  infrastructure.
- The interaction should stay GNOME-native and accessible by keyboard and screen
  reader.

References used while validating this design:

- Relm4 book factory guidance for list rendering and factory-backed UI updates.
- GTK 4 `GtkExpander` documentation, especially `label_widget`, expansion state,
  CSS nodes, and accessibility role behavior.
- GNOME HIG accessibility guidance for accessible names, keyboard navigation,
  high contrast, and large text verification.

## Approach

**Group tool call bursts in `SessionDetail` and render them as a new
`TranscriptRow` kind backed by `GtkExpander`.**

- Keep the database result shape unchanged.
- Transform the flat DB transcript stream into display items immediately before
  populating the transcript factory.
- Introduce a UI-only `ToolBurst` display item for grouped tool call bursts.
- Keep individual tool call rows unchanged when no grouping applies.

This keeps the grouping concern in the presentation layer, where it belongs,
and avoids leaking a UI abstraction into the database or parser layers.

### Pagination boundary handling

The current transcript loads pages of 200 items. A page boundary may split a
burst mid-run. The transformation step must buffer any pending burst at the end
of a page and flush it only when the next page load confirms no more consecutive
tool calls follow, or when the user has not requested the next page yet. In the
latter case, the partial burst renders normally and is re-grouped when the next
page arrives.

---

## 1. Transcript Transformation Layer

`SessionDetail` gains a small transformation step between
`load_transcript_items()` and `guard.push_back(...)`.

The transformation walks the flat transcript rows in order and emits display
items using these rules:

1. Start an empty pending burst buffer.
2. When the next source row is a tool call, append it to the pending burst.
3. When the next source row is not a tool call, flush the pending burst first:
   - length `== 1`: emit a normal `ToolCall` item
   - length `>= 2`: emit one `ToolBurst` item
4. Emit the non-tool-call row normally.
5. Flush any trailing pending burst after the loop ends.

Messages and subagents both terminate a burst. Subagents are not grouped into
tool call bursts.

This design preserves source ordering while allowing the display layer to reduce
vertical noise.

### Index mapping

The current codebase assumes `item_index == factory position` for match
navigation and scrolling. Grouping breaks this invariant because multiple source
rows collapse into one display item.

The transformation step produces a `Vec<DisplayItem>` where each element carries
its own `display_index` (sequential position in the factory). The transformation
also builds a mapping from `display_index` to the source `item_index` range it
covers. `match_counts` in `SessionDetail` must be keyed by `display_index`, not
by the DB `item_index`. `find_item_for_match()` uses `display_index` for scroll
targeting.

---

## 2. Display Model

The transcript display model gains a new UI-only variant:

```rust
enum TranscriptItemInit {
    Message(MessageItemInit),
    ToolCall(ToolCallItemInit),
    ToolBurst(ToolBurstItemInit),
    Subagent(SubagentItemInit),
}
```

`ToolBurstItemInit` is local to the transcript UI and is not persisted in the
database or shared with parsers.

It contains:

- `item_index`: display index in the factory sequence
- `tool_calls: Vec<ToolCallItemInit>`: child tool calls rendered when expanded
- `category_counts`: grouped counts for collapsed header pills
- `error_count`: number of failed or errored tool calls in the burst
- `total_duration_ms: Option<i64>`: summed duration when duration data exists
- `match_count`: total matches across the burst's searchable child fields
- `contains_active_match`: whether the currently selected search hit is inside
  this burst
- `default_expanded`: whether the burst should start opened for the current view

The grouping threshold is fixed at `>= 2` consecutive tool calls.

---

## 3. Rendering Model

`TranscriptRow` stops being a strict one-to-one wrapper around a database row and
becomes the rendering unit for the transcript display.

The new `ToolBurst` row renders as:

```text
gtk::Box.tool-call-group
  GtkExpander
    [label_widget] gtk::Box(H)
      category pills
      total duration (optional)
      dim text: "N tool calls"
      error indicator: "N error(s)" (optional)
    [child] gtk::Box(V)
      tool-call-row x N
```

Note: the disclosure arrow is provided natively by `GtkExpander` (the `expander`
CSS node under `title`). It is not created by the application and should not
appear in the label_widget children.

Important rendering rules:

- Default state is collapsed.
- The `GtkExpander` disclosure remains native.
- The header uses a custom `label_widget` to assemble pills and metadata.
- The expanded body reuses the existing tool call row structure as much as
  possible, including the inspect button and preview line.
- There is no second level of grouping inside a burst.
- An isolated tool call continues to render exactly as a normal `ToolCall` row.

This gives the transcript a higher-level activity summary without hiding the
underlying tool calls from inspection.

### Child widget construction

Burst child tool call widgets are **manually constructed**, not
factory-managed. A `ToolBurst` is a single `FactoryComponent` item whose
`init_widgets` builds N inner tool call widgets programmatically.

To keep rendering consistent between standalone tool calls and burst children,
the tool call widget building logic should be extracted from
`build_tool_call_widgets` into a standalone helper function with a signature
like:

```rust
fn build_tool_call_widget(
    init: &ToolCallItemInit,
    on_inspect: impl Fn(String) + 'static,
) -> gtk::Box
```

This helper is called by the factory path for standalone `ToolCall` rows and by
the burst builder for inner children. The `on_inspect` callback wires the
inspect button signal back to the parent factory item's sender.

Because these children lack `FactoryComponent` lifecycle methods, any dynamic
updates (such as search highlighting within an expanded burst) must be handled
through explicit widget references stored alongside the burst state.

---

## 4. Search And Match Navigation

Search behavior changes from "matches only in message rows" to "matches in any
display item that exposes searchable content".

For tool bursts, searchable content is limited to fields already represented in
the transcript UI:

- tool name
- tool preview
- tool summary

The design does not expand search scope into raw tool input/output payloads that
are only visible in the inspector. This keeps search behavior aligned with the
detail view itself.

### Match counting

- A `ToolBurst` reports one aggregate `match_count` for the entire burst.
- A collapsed burst header displays a match badge when `match_count > 0`.
- `Message` rows continue reporting message-local match counts as they do today.

### Active match behavior

- Bursts do not auto-expand merely because they contain passive matches.
- A burst auto-expands only when Next/Prev navigation targets a match inside it.
- Once auto-expanded for the active search result, it may remain open for the
  life of the current query/view state.

This avoids opening every burst for broad queries while still making search
navigation useful.

### Match badge accessibility

The match badge on collapsed bursts must carry an accessible label, for example
`"3 search matches inside this group"`, so that screen reader users can discover
matches without expanding.

### Indexing and scrolling

The current search navigation logic assumes the transcript row index and the
loaded DB item index are effectively the same. That assumption no longer holds
once multiple source tool call rows collapse into one display item.

This design therefore distinguishes:

- source order: DB transcript item order
- display order: factory item order after grouping

Search navigation targets the display order. The scroll target for a match is
the `display_index` of the display item, not the original DB row index inside
the burst. See the index mapping section under the Approach for the concrete
mapping strategy.

### Scroll-after-expand timing

When search navigation auto-expands a burst via `set_expanded(true)`, the child
widgets may not be allocated in the same frame. GTK 4 layouts are asynchronous.
The scroll-to-child must be deferred until after the next layout pass. Follow
the existing `glib::idle_add_local_once` pattern already used in the session
detail view. If one idle is not sufficient in practice, use a one-shot
`add_tick_callback` on the expanded container and perform the scroll once the
target child is present and has a computed position. Do not plan around
`connect_size_allocate`; that hook is not available in this GTK 4 stack.

---

## 5. Accessibility

Accessibility relies on `GtkExpander` as the primary disclosure control instead
of a custom button-plus-revealer pattern.

Required accessibility behavior:

1. Each burst header exposes a descriptive accessible name, for example:
   `6 tool calls: 3 Read, 1 Edit, 1 Bash, 1 Grep, 1 error`. This must be set
   explicitly via `update_property` with `gtk::accessible::Property::Label` on
   the `GtkExpander`, not derived implicitly from the composite `label_widget`
   child text (which produces awkward concatenation for screen readers):

   ```rust
   expander.update_property(&[
       gtk::accessible::Property::Label(
           "6 tool calls: 3 Read, 1 Edit, 1 Bash, 1 Grep, 1 error"
       ),
   ]);
   ```

2. Error state is never color-only; it always includes visible text such as
   `1 error` or `2 errors`.
3. Category pills are visual affordances, but the same information exists in the
   accessible text and visible header text.
4. Expansion state remains exposed through `GtkExpander`'s built-in accessible
   semantics.

### Keyboard navigation

- Tab reaches the `GtkExpander` as a single tab stop.
- Space or Enter toggles expansion.
- When expanded, Tab navigates into child tool call rows and their inspect
  buttons.
- Shift+Tab returns focus to the expander header.
- Escape does not collapse the expander (this is not standard `GtkExpander`
  behavior).

### Verification expectations

- full keyboard navigation per the flow above
- screen reader announcement of the burst header and expansion state
- high-contrast rendering
- large text rendering

---

## 6. Styling

The grouped burst should feel like a natural evolution of the existing tool call
row styling, not a new visual system.

Styling rules:

- Add a new container style such as `.tool-call-group`.
- Preserve the orange left border to maintain continuity with current tool call
  rows.
- Make the expander header compact by styling the widgets we own inside
  `GtkExpander::set_label_widget`, not undocumented internal `GtkExpander` CSS
  nodes. Add explicit classes to the header container and its subparts so the
  implementation can style them directly, and set header box spacing in code.
  For example:

  ```css
  .tool-call-group-header {
    padding: 4px 8px;
    min-height: 0;
  }

  .tool-call-group-pill {
    margin: 0;
  }
  ```

  This keeps the compact layout under app control and avoids depending on
  fragile or incorrect internal node names such as `expander-widget`.
- Render category pills using `@accent_color` for all categories. The text
  label on each pill (e.g. "3 Read", "1 Edit") is the primary differentiator,
  not color. This avoids misusing `@warning_color` and `@success_color` as
  category identifiers when they carry status semantics in libadwaita.
- Render total tool call count as dim text instead of a dedicated count pill.
- Omit the duration label entirely when no total duration is available.

The burst header should remain compact enough to reduce visual height while
staying legible in large text mode.

---

## 7. Error Handling And Edge Cases

- If one or more tool calls in a burst have an error status, the collapsed
  header shows explicit error text.
- Individual child rows continue to render their own statuses in expanded view.
- Missing duration data is not treated as an error; total duration is simply
  omitted.
- Missing preview or summary data does not invalidate the burst. The header does
  not depend on generated natural-language text.
- A single tool call between messages is never wrapped in a burst.
- A subagent row between tool calls breaks the burst boundary.

---

## 8. Test And Verification Plan

### Unit tests for transcript grouping

Cover the transformation layer independently of GTK widgets:

1. no tool calls
2. one isolated tool call between messages
3. exactly two consecutive tool calls
4. mixed burst: `Read, Read, Grep, Edit, Bash`
5. two bursts separated by a message
6. burst containing one or more errors
7. burst with partial or absent duration data
8. subagent row interrupting a run of tool calls

### Grouping edge case tests

1. page boundary splits a burst mid-run (partial burst at end of page)
2. burst re-grouped correctly when next page arrives

### Search behavior tests

1. burst match count aggregates tool child matches
2. passive matches do not auto-expand the burst
3. active-match navigation expands only the targeted burst
4. scroll targeting uses display indices, not DB item indices, after grouping
5. collapsed burst match badge carries accessible label

### Manual verification

Run the app against fixtures and verify:

1. grouped tool call bursts appear collapsed by default
2. isolated tool calls still render individually
3. inspect actions remain available on child tool calls
4. keyboard-only interaction works for expand/collapse and transcript navigation
5. high contrast and large text keep the burst header readable
6. broad search queries do not expand all bursts at once
7. navigating to the active search result expands the relevant burst

Suggested manual run command:

```bash
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures
```

---

## Expected Outcome

- Consecutive tool call bursts take far less vertical space in the session
  detail view.
- Users can understand assistant activity at a glance from the collapsed header.
- Detailed inspection remains available on demand.
- Search remains useful inside bursts without causing the entire transcript to
  open up.
- The interaction stays GNOME-native and accessible.
