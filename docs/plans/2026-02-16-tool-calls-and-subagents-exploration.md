# Tool Calls & Subagents — UI Exploration

Visual exploration of how to display tool calls (Read, Bash, Edit, etc.) and
subagent sessions in the Sessions Chronicle transcript view.

Reference design: [tool-calls-and-subagents-design.md](2026-01-30-tool-calls-and-subagents-design.md)

---

## Proposals

### A — Badges + Detail Panel

![Mockup A](../mockups/tool-calls-and-subagents/mockup-a-badges-panel.svg)

Tool calls appear as **compact inline badges** (pills) between messages in the
transcript. Clicking a badge opens a **lateral detail panel** on the right
showing the full input/output.

| Aspect | Detail |
|--------|--------|
| Layout | Split: transcript 60% / detail panel 40% |
| Tool calls | Colored pills inline: `📄 Read`, `⚙ Bash`, `✏ Edit` |
| Subagents | Distinct pill: `🔀 Task` with purple accent |
| Interaction | Click badge → panel shows input, output, duration |
| Navigation | Mini-pills at panel bottom to switch between tool calls |

**Pros:** Compact transcript, full detail on demand, badge navigation.  
**Cons:** Requires lateral panel management, split layout reduces transcript width.

**Analysis:** The 60/40 split is problematic on screens < 1400px. On a typical
GNOME laptop (1366×768 or 1920×1080 with scaling), the transcript at 60%
becomes narrow. The panel is only useful when inspecting a tool call — the rest
of the time it wastes space. For subagents, clicking the `🔀 Task` badge opens
the panel with the subagent prompt + result summary, but there is no mechanism
to drill into the subagent's own tool calls without additional panel navigation
(stack push/pop). The panel concept is strong for detail display, but the
permanent split layout has a high UX cost.

---

### B — Expander Rows (GNOME HIG)

![Mockup B](../mockups/tool-calls-and-subagents/mockup-b-expander-rows.svg)

Tool calls are **AdwExpanderRow-style collapsible rows** inline in the
transcript. Follows the GNOME Settings pattern.

| Aspect | Detail |
|--------|--------|
| Layout | Full width, no side panel |
| Tool calls | Full-width expandable rows with chevron ▶/▼ |
| Collapsed | Icon + tool name + summary (e.g. file path, command) |
| Expanded | Monospace content block (terminal output, file content) |
| Subagents | Same pattern, purple accent |

**Pros:** Native GNOME pattern, familiar UX, no panel complexity, full width.  
**Cons:** Expanding a tool pushes messages down, can stretch the transcript.

**Analysis:** Excellent for simple tool calls (Read, Bash, Edit). The pattern
breaks down for subagents: a subagent from Claude Code contains an embedded blob
with its own tool calls — an expander inside an expander becomes confusing.
Similarly, Codex collab threads contain mini-transcripts that don't fit well in
a single expanded row. Long tool outputs (e.g. a `Read` of 200 lines) push
messages far down, disrupting reading flow. Best suited as the primary inline
display, but insufficient alone for subagent hierarchies.

---

### C — Grouped Action Rows (GNOME HIG)

![Mockup C](../mockups/tool-calls-and-subagents/mockup-c-grouped-action-rows.svg)

Consecutive tool calls are **grouped into a single AdwPreferencesGroup card**.
Each tool is an **AdwActionRow** inside the group.

| Aspect | Detail |
|--------|--------|
| Layout | Full width, no side panel |
| Tool calls | Grouped in a card: "3 tool calls" header |
| Each row | Icon + bold name + dim summary + chevron › |
| Interaction | Click row → navigates to detail (or expands) |
| Subagents | Separate group card with purple accent |

**Pros:** Reduces visual noise, groups related calls, clean HIG pattern.  
**Cons:** Loses chronological interleaving with text, click target less obvious.

**Analysis:** Good presentation layer, but incomplete as a standalone solution.
The "or expands" in the interaction model hides an unresolved design choice: if
clicking navigates somewhere, it needs a detail view (falling back to a panel or
overlay). The grouping breaks down when the assistant writes text between tool
calls, making the grouping artificial. For subagents, a separate purple group
card works visually, but the subagent's own tool calls have nowhere to display.
Best used as a **bonus grouping heuristic** on top of another proposal: when N
consecutive tool calls appear with no text between them, group them under a
collapsible "N tool calls" header.

---

### D — Timeline Swimlanes (Creative)

![Mockup D](../mockups/tool-calls-and-subagents/mockup-d-timeline-swimlanes.svg)

The conversation is a **vertical timeline with swimlanes**. Messages flow in
the center, tool calls branch to the right as parallel execution lanes.

| Aspect | Detail |
|--------|--------|
| Layout | 3 columns: time / conversation / tool lanes |
| Tool calls | Branch right with bezier curves, shown in lane boxes |
| Parallelism | Concurrent tools at same Y position |
| Metadata | Duration pills, result summaries on each tool box |
| Subagents | Distinct lane with purple accent, nested sub-tasks |

**Pros:** Shows execution flow and parallelism, rich metadata, unique.  
**Cons:** Complex layout, harder to implement in GTK4, wide screen needed.

**Analysis:** The only proposal that honestly represents parallelism — Claude
Code often launches 3-4 Read calls simultaneously, Codex spawns concurrent
threads. However, GTK4 has no layout engine for this. It would require a custom
`GtkDrawingArea` or absolute positioning in a `GtkFixed`, completely outside
libadwaita. Bezier curves, synchronized Y positioning, resizable swimlanes —
this is a standalone UI project. On a normal screen, 3 columns don't fit.
Accessibility (screen readers, keyboard navigation) would be a significant
challenge. Intellectually the most interesting proposal, but practically
unrealistic for a first pass. Could be revisited as a v2 alternative view.

---

### E — Nested Thought Process (Creative)

![Mockup E](../mockups/tool-calls-and-subagents/mockup-e-nested-thought-bubbles.svg)

The assistant message is a **single tall card** containing everything: text
interleaved with **nested tool cards** at increasing indentation levels.

| Aspect | Detail |
|--------|--------|
| Layout | Full width, single column |
| Tool calls | Cards nested inside the assistant message (indent 24px) |
| Subagents | Double-nested cards (indent 48px), deeper background shade |
| Nesting | Background shading: #fff → #f6f6f6 → #efefef |
| Content | Collapsed (2-line preview + "Show output") or expanded |

**Pros:** Reads like the AI's thought process, natural flow, shows hierarchy.  
**Cons:** Tall messages, deep nesting can become visually heavy.

**Analysis:** The best conceptual representation of the hierarchy. Subagents
from Claude Code (embedded blob) map naturally to a deeper indentation level
with their own tool cards inside. However, messages become very tall — an
assistant that makes 5 tool calls produces a card 500px+ high. Deep nesting (3
levels: message → subagent → tool call → output) reduces usable width. For
OpenCode with separate child sessions, the nesting is artificial — you'd need
to load the child session's data and inject it into the parent's flow. The
collapsed/expanded state management adds significant UI complexity.

---

### F — Expanders + Utility Pane (Recommended Hybrid)

This proposal combines the strengths of **B (Expander Rows)** for inline
preview and **A (Detail Panel)** for full detail, using an **AdwOverlaySplitView
utility pane** instead of a permanent split or a modal dialog.

The key insight: the transcript is a "list" and the pane is the "detail". Click
a tool call in the transcript, the pane updates. No open/close cycle — the pane
stays open and follows selection. This is the GNOME Builder inspector pattern.

#### F1 — Transcript (pane hidden)

![Mockup F1](../mockups/tool-calls-and-subagents/mockup-f1-expanders-transcript.svg)

When the utility pane is hidden, the transcript uses full width with
**AdwExpanderRow-style rows** for all tool calls. Each row shows icon + tool
name + summary + duration pill, and can be expanded inline for a quick preview.
Subagent rows expand to show a prompt summary and a **list of inner tool calls**
as ActionRows. Clicking `›` on an inner tool call opens the utility pane.

#### F2 — Wide screen: side-by-side split

![Mockup F2](../mockups/tool-calls-and-subagents/mockup-f2-utility-pane-split.svg)

On wide screens (above `AdwBreakpoint` threshold), the utility pane appears
**side-by-side** with the transcript (~73% / ~27%). The transcript remains
scrollable and interactive. Clicking any tool call row in the transcript updates
the pane content — no modal, no navigation away. The selected tool call is
highlighted in the transcript with a border accent.

#### F3 — Narrow screen: overlay mode

![Mockup F3](../mockups/tool-calls-and-subagents/mockup-f3-utility-pane-overlay.svg)

On narrow screens (below breakpoint), `AdwOverlaySplitView` automatically
collapses: the pane **floats over** the transcript as a sliding panel from the
right edge. The transcript is dimmed but visible behind. Swipe-right or the
close button dismisses the pane. This is the same widget in both modes — only
the `collapsed` property changes via `AdwBreakpoint`.

#### Design Summary

| Aspect | Detail |
|--------|--------|
| Layout | `AdwOverlaySplitView`: side-by-side on wide, overlay on narrow |
| Transcript | Full width when pane hidden; ~73% when pane visible (wide) |
| Tool calls | AdwExpanderRow inline: collapsed summary, expand for quick preview |
| Subagents | Expanded row shows prompt + inner tool call list (AdwActionRow) |
| Utility pane | Non-modal, follows selection, shows full input/output/status |
| Pane navigation | `AdwNavigationView` stack inside pane for subagent drill-down |
| Toggle | Header bar button toggles pane; also opens on tool call click |
| Adaptive | `AdwBreakpoint` auto-collapses below threshold (e.g. `max-width: 800sp`) |
| Gestures | Edge swipe to open/close pane on touch (`enable-show-gesture`) |
| Grouping | Optional: N consecutive tool calls with no text → "N tool calls" group |

#### Interaction Flow

```
Wide screen (side-by-side):

┌─────────────────────────────────────────┬──────────────────┐
│ Transcript (~73%)                       │ Utility Pane     │
│                                         │                  │
│ [User message]                          │ 📄 Read  ✓ 120ms│
│ [Assistant message]                     │                  │
│ [▶ Read — config.rs      120ms] ◄───────│ INPUT            │
│ [▶ Bash — cargo test      3.2s]         │ file_path: ...   │
│ [Assistant message]                     │                  │
│ [▶ Edit — config.rs:42    85ms]         │ OUTPUT           │
│ [▼ 🔀 Task — Review   3 tools]         │ ┌──────────────┐ │
│   ├ 📄 Read config.rs        ›          │ │ 1 use serde  │ │
│   ├ 📄 Read config_test.rs   ›          │ │ 2 use std    │ │
│   └ RESULT: Looks good...              │ │ ...          │ │
│ [Assistant message]                     │ └──────────────┘ │
│                                         │                  │
│ Click any tool row → pane updates       │ SESSION TOOLS    │
│                                         │ [📄 Read ██████] │
│                                         │ [⚙ Bash       ›] │
│                                         │ [✏ Edit       ›] │
│                                         │ [🔀 Task      ›] │
└─────────────────────────────────────────┴──────────────────┘

Narrow screen (overlay):

┌──────────────────────────┐
│ Transcript (100%)        │
│                     ┌────┤
│ (dimmed)            │Pane│  ← slides in from right
│                     │    │  ← swipe right to dismiss
│                     │    │
│                     └────┤
└──────────────────────────┘

Subagent drill-down (inside pane):

┌──────────────────┐      ┌──────────────────┐
│ 🔀 Task overview │ ───► │ ‹ Task: Review   │
│                  │      │                  │
│ PROMPT: Review.. │      │ 🔀 Task › 📄 Read│
│ TOOL CALLS       │      │                  │
│ [📄 Read    ›]   │ click│ 📄 Read  ✓ 120ms │
│ [📄 Read    ›]   │      │ INPUT: ...       │
│ [🔍 Grep    ›]   │      │ OUTPUT: ...      │
│ RESULT: Looks... │      │                  │
└──────────────────┘      └──────────────────┘
         page 1 (push)           page 2 (push)
```

#### Why Utility Pane over Dialog Overlay

| Aspect | AdwDialog (previous design) | AdwOverlaySplitView (current) |
|--------|---------------------------|-------------------------------|
| Modality | Modal — blocks transcript interaction | Non-modal — transcript stays interactive |
| Workflow | Close → scroll → click another tool → reopen | Click tool → pane updates in place |
| Wide screen | Dialog floats centered, space wasted | Side-by-side, all space used |
| Narrow screen | Same dialog, covers most of the view | Collapses to overlay automatically |
| Persistence | Ephemeral, closing loses context | Stays open, toggle on/off |
| Gestures | None | Edge swipe to open/close (touch) |
| GNOME pattern | Dialogs are for confirmations/forms | Inspector/utility pane (GNOME Builder) |

#### Per-Parser Behavior

| Parser | Tool calls | Subagents |
|--------|-----------|-----------|
| **Claude Code** | Expander rows from `tool_use` blocks | Subagent row expands to show prompt + inner tools extracted from `tool_result` blob. Click inner tool → pane navigates (stack push) |
| **Codex** | Expander rows from `mcp_tool_call_*` / `exec_command_*` events | Subagent row from `collab_agent_spawn_*` events. Inner tools filtered by child `thread_id`. Click → pane navigates |
| **OpenCode** | Expander rows from `tool` parts (with full lifecycle state) | Subagent row from `subtask` parts. Inner tools loaded from child session (`parentID`). "Open full session" link also available |
| **Mistral Vibe** | Expander rows from `tool_calls[]` + correlated `role: "tool"` results | No subagent support. Tool calls stay at one level |

#### Tool Call ↔ Result Correlation

| Parser | Mechanism |
|--------|-----------|
| **Claude Code** | `tool_use.id` → next `tool_result` (API sequencing convention). Parser reconstructs pairs |
| **Codex** | `call_id` shared between `begin` and `end` events. Direct ID lookup |
| **OpenCode** | Single `tool` part contains `state.input` + `state.output`. Self-contained |
| **Mistral Vibe** | `tool_calls[].id` → `tool_call_id` on `role: "tool"` message. ID lookup |

#### GTK4/libadwaita Components

| UI Element | Widget |
|-----------|--------|
| Split layout (transcript + pane) | `AdwOverlaySplitView` (sidebar = pane, content = transcript) |
| Adaptive collapse | `AdwBreakpoint` sets `collapsed: true` below threshold |
| Tool call rows (collapsed/expanded) | `AdwExpanderRow` or custom `GtkListBoxRow` with expand logic |
| Inner tool call list (inside subagent) | `AdwActionRow` inside `AdwPreferencesGroup` |
| Stack navigation in pane | `AdwNavigationView` with push/pop pages |
| Breadcrumb trail | `GtkBox` with `GtkLabel` + separator |
| Code output block | `GtkSourceView` or `GtkTextView` with monospace |
| Pane toggle | `GtkToggleButton` in header bar |

**Pros:**
- Full-width transcript when pane is hidden (no permanent split cost)
- Non-modal: transcript remains interactive while pane is open
- Click-to-update: selecting a tool call in transcript updates pane (no open/close cycle)
- Adaptive: side-by-side on wide screens, overlay on narrow screens (single widget)
- Touch support: edge swipe gestures via `AdwOverlaySplitView`
- Stack navigation in pane handles subagent nesting naturally
- GNOME HIG compliant (AdwOverlaySplitView, AdwNavigationView, AdwBreakpoint)
- Works for all 4 parsers with parser-specific correlation strategies
- Optional grouping of consecutive tool calls (Proposal C bonus)

**Cons:**
- Two interaction modes to learn (expand inline vs. inspect in pane)
- Pane reduces transcript width on wide screens when open (~73% vs 100%)
- More implementation work than pure B (expanders) due to split view + navigation

---

## Comparison Matrix

| Criterion | A: Badges | B: Expander | C: Grouped | D: Timeline | E: Nested | **F: Hybrid** |
|-----------|-----------|-------------|------------|-------------|-----------|---------------|
| GNOME HIG compliance | Medium | High | High | Low | Medium | **High** |
| Implementation complexity | Medium | Low | Low | High | Medium | **Medium** |
| Transcript readability | High | Medium | High | Medium | Medium | **High** |
| Tool detail visibility | High (panel) | High (inline) | Low (nav) | Medium | High (inline) | **High (both)** |
| Parallelism display | No | No | No | Yes | No | No |
| Subagent hierarchy | Badge only | Row only | Group | Swimlane | Nesting | **Expand + pane stack** |
| Screen width needed | Wide (split) | Normal | Normal | Wide (3-col) | Normal | **Adaptive** |
| Multi-parser support | Medium | Medium | Medium | Low | Medium | **High** |

---

## Recommendation

**Proposal F (Expanders + Utility Pane)** is the recommended approach. It
provides the best balance of HIG compliance, transcript readability, and
subagent support across all four parsers.

The inline expander rows handle 90% of tool call inspection needs (quick
preview), while the utility pane handles the 10% that requires full detail or
subagent drill-down — without blocking interaction with the transcript.

The `AdwOverlaySplitView` widget gives adaptive behavior for free: side-by-side
on wide screens, overlay on narrow screens, with a single `AdwBreakpoint`
condition. This resolves Proposal A's permanent split problem and the dialog
overlay's modality problem in one widget.

The optional grouping heuristic from Proposal C can be layered on top: when N
consecutive tool calls appear with no interleaved text, collapse them under a
"N tool calls" header. This reduces visual noise without losing chronological
accuracy.
