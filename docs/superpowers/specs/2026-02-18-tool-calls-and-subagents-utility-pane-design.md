# Tool Calls and Subagents Utility Pane Design

**Status:** Proposed  
**Date:** 2026-02-18  
**Based on:** [Tool Calls and Subagents - UI Exploration](2026-02-16-tool-calls-and-subagents-exploration.md) proposal F  
**Supersedes (for phase 6):** [Tool Calls and Subagents Display](2026-01-30-tool-calls-and-subagents-design.md)

## Goal

Implement phase 6 with a GNOME HIG-aligned interaction model:

- Inline transcript rows for tool calls and subagents (expander pattern).
- Full input/output inspection in a utility pane (not a modal dialog).
- Deterministic drill-down for nested subagent calls.
- Consistent behavior across Claude Code, Codex, OpenCode, and Mistral Vibe.

This design keeps proposal F's core direction and resolves ambiguity identified during review.

## References

- [UI exploration proposal F](2026-02-16-tool-calls-and-subagents-exploration.md)
- [Session format analysis (Claude/Codex/OpenCode/Mistral)](../SESSION_FORMAT_ANALYSIS.md)
- [Current app utility pane design](2026-02-08-session-detail-utility-pane-design.md)
- [GNOME HIG: Utility Panes](https://developer.gnome.org/hig/patterns/containers/utility-panes.html)
- [GNOME HIG: Dialogs](https://developer.gnome.org/hig/patterns/feedback/dialogs.html)
- [Libadwaita: Adaptive Layouts](https://raw.githubusercontent.com/GNOME/libadwaita/main/doc/adaptive-layouts.md)
- [Libadwaita: AdwOverlaySplitView](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.OverlaySplitView.html)
- [Libadwaita: AdwNavigationView](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.NavigationView.html)
- [Libadwaita: AdwBreakpoint](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.Breakpoint.html)
- [Libadwaita: AdwExpanderRow](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.ExpanderRow.html)
- [Relm4 Book: Factory](https://raw.githubusercontent.com/Relm4/book/refs/heads/main/src/efficient_ui/factory.md)

## Current Context

Today, phase 6 data is not indexed for display:

- Claude parser keeps text/thinking blocks only, ignores `tool_use` / `tool_result`.
- Codex parser keeps `user_message` / `agent_message` only, ignores tool and collab events.
- OpenCode parser skips `tool` and `subtask` parts and rejects child sessions (`parentID`).
- Mistral Vibe parser ignores `role: "tool"` rows and assistant rows with empty text + `tool_calls`.

UI context:

- The app already uses `AdwOverlaySplitView` at app level (`src/app.rs`) with F9 toggle.
- Session detail currently renders only message rows via `FactoryVecDeque<MessageRow>`.
- A utility-pane `gtk::Stack` already exists with `Filters` and `SessionContext` modes.
- `DetailContextPane` currently shows project name, tool icon, and "Resume in Terminal" button.

## Corrections Applied vs Exploration F

The exploration doc identified the right direction; this design tightens behavior in five places:

1. **No contradiction on pane lifecycle**
   Pane has explicit hidden/visible states; opening and closing are expected actions.
2. **No click ambiguity with expander rows**
   Expander row activation controls expansion. A dedicated inspect action opens/updates the pane.
3. **No fixed 73/27 rule**
   Use `AdwOverlaySplitView` sizing properties (`sidebar_width_fraction`, min/max width) instead of a hard-coded split ratio.
4. **Robust Claude correlation**
   Prefer `tool_use_id` matching when available; only use ordered fallback as a secondary strategy. Verify against fixture data.
5. **Inspector-only pane in detail view**
   The utility pane in detail view is exclusively a tool/subagent inspector. "Resume in Terminal" moves to the header bar. `DetailContextPane` and `SessionContext` mode are removed.

## Options Considered

### A) Reuse app-level utility pane with dynamic position (chosen)

Reuse existing `AdwOverlaySplitView` in `App`. Replace `SessionContext` mode with
`ToolInspector` mode. Change sidebar position dynamically: `start` (left) for
filters in list view, `end` (right) for inspector in detail view.

**Pros:**

- Reuses existing split view, breakpoint, and F9 behavior.
- Avoids nested split containers.
- Inspector on the right follows GNOME HIG (inspectors at `end`).
- Filters on the left follows GNOME HIG (navigation/filters at `start`).
- Simpler pane model: only 2 modes instead of 3.

**Cons:**

- Requires `set_sidebar_position()` call during view transitions. Visual smoothness needs validation.
- Cross-component messages between transcript and app-level pane.

### B) Add another split view inside `SessionDetail`

Create a dedicated split layout inside detail page.

**Pros:**

- Encapsulated detail-specific UI logic.

**Cons:**

- Two competing utility-pane systems in one app.
- Higher state complexity and more keyboard/gesture edge cases.

### C) Modal detail dialog (old direction)

Open full details in `AdwDialog`.

**Pros:**

- Straightforward to implement.

**Cons:**

- Conflicts with HIG guidance for non-blocking contextual inspection.
- Breaks inspection flow (close, scroll, reopen).

## Architecture Decision

Adopt option A.

- Continue using the existing app-level `AdwOverlaySplitView`.
- Replace `SessionContext` mode with `ToolInspector` mode (2 modes total).
- Remove `DetailContextPane` component entirely.
- Move "Resume in Terminal" to a `GtkButton` in the header bar, visible only in detail view.
- Change sidebar position dynamically via `set_sidebar_position()` during list↔detail transitions.
- Render tool/subagent rows inline using expander semantics.
- Open inspector from explicit inspect actions.
- Keep modal dialogs out of normal inspection workflow.

## Terminology

- **Utility pane**: HIG concept for the secondary panel.
- **Sidebar**: libadwaita API name for the same area (`AdwOverlaySplitView` sidebar).

## State Model

`PaneVisibility`

- `Hidden`
- `Visible`

`PaneLayout` (derived from breakpoint)

- `Split` (wide)
- `Overlay` (narrow, collapsed)

`PaneMode`

- `Filters` — list view, sidebar position: `Start` (left)
- `ToolInspector` — detail view, sidebar position: `End` (right)

`InspectorSelection`

- `None`
- `ToolCall(tool_call_id)`
- `Subagent(subagent_id)`
- `SubagentTool(subagent_id, tool_call_id)`

IDs in `InspectorSelection` are resolved in the context of the active `session_id`.

## Sidebar Position Strategy

The sidebar position changes dynamically with view transitions:

| Transition | Sidebar position | Rationale |
|------------|-----------------|-----------|
| Enter detail view (`transition_to_detail`) | `End` (right) | Inspector inspects selected content — HIG `end` position |
| Return to list view (`transition_to_list`) | `Start` (left) | Filters affect main content — HIG `start` position |

Implementation:
- `transition_to_detail()` / `transition_to_list()` update view state only (no widget side effects).
- Apply `widgets.overlay_split.set_sidebar_position(...)` in `update_with_view`
  based on the resolved target view (`End` in detail, `Start` in list).

Note: validate that the position change is visually smooth. If it causes a jarring flash,
briefly set `show_sidebar: false` before changing position, then restore visibility.

## Interaction Contract

| User action | Expected behavior |
|-------------|-------------------|
| Click expander row body | Toggle inline expand/collapse only |
| Click inspect button (`view-reveal-symbolic`) on tool row | Select tool call; switch pane to `ToolInspector`; open pane if hidden |
| Click inspect button on subagent row | Select subagent overview; switch pane to `ToolInspector`; open pane if hidden |
| Click `go-next-symbolic` on inner tool row (inside expanded subagent) | Push detail page in inspector nav stack |
| Click "Open full session" on a subagent with `child_session_id` | Open child session in detail view (same app detail page), and enable one-hop return to parent session |
| Click "Back to parent session" (shown after child-session jump) | Return to the originating parent session and clear child-session return context |
| Click "Resume in Terminal" button in header bar | Resume session in terminal (existing pipeline) |
| Press F9 | Toggle pane visibility; keep current mode and selection |
| Press Esc in detail view | Pop inspector nav page if possible; otherwise close inspector pane if open; otherwise navigate back to list |
| Navigate back to session list | Clear inspector selection; switch pane to `Filters`; change sidebar position to `Start` |

### Esc ownership and priority

- Set main app `AdwNavigationView` (`App` level) to `pop_on_escape = false` to avoid conflicts with inspector-level escape handling.
- Keep inspector `AdwNavigationView` `pop_on_escape = true` so drill-down pages feel native.
  Level 1 (pop inspector page) is handled automatically by libadwaita when the stack depth > 1.
- For levels 2 and 3, register a `RelmAction<EscapeAction>` using the existing action pattern:
  `app.set_accelerators_for_action::<EscapeAction>(&["Escape"])`.
  The action fires only when the inspector nav does not consume Esc (i.e. stack depth == 1).
  In the action handler, send `AppMsg::Escape` and resolve priority in `update()`:
  1. if utility pane is visible, close pane;
  2. else trigger normal detail back navigation to list.
  (Defensive check: if stack depth > 1 is observed, no-op because level 1 should have
  already been consumed natively by inspector navigation.)

### Inspect Action Widget

- **Tool call rows**: `GtkButton` with `view-reveal-symbolic` icon placed as suffix widget on the expander row. Clicking the button opens/updates the inspector pane. Clicking the row body toggles expand/collapse.
- **Subagent inner tool rows**: `AdwActionRow` with `go-next-symbolic` as suffix icon. Clicking the row pushes a detail page in the inspector navigation stack.

Notes:

- Expansion and inspection are intentionally separate actions.
- Tool row click does not implicitly resize layout.

## Adaptive Behavior

- Use `AdwBreakpoint` with `sp` units.
- Keep existing app-level collapse behavior and tune threshold as needed (start with current app threshold; validate with Large Text enabled).
- In split mode, configure pane width via split-view properties instead of fixed percentage assumptions.

## Data Model and Persistence

Search remains backed by `messages` FTS5. Tool/subagent rendering uses dedicated normalized tables.

### Schema policy for current dev phase

- Phase 6 does **not** require in-place DB migrations.
- On incompatible schema changes, delete the app DB file and rebuild it by reindexing session sources.
- Keep schema creation idempotent (`CREATE TABLE IF NOT EXISTS`, additive indexes) so a clean rebuild path remains deterministic.
- Before PRs, verify a full rebuild from fixtures and real session sources succeeds.

### Sessions table additions

Add:

- `parent_session_id TEXT NULL`
- `is_subagent INTEGER NOT NULL DEFAULT 0`

Session list queries must filter `is_subagent = 0` by default.

### New table: `transcript_items`

Ordered stream for detail rendering.

Suggested columns:

- `session_id TEXT NOT NULL`
- `item_index INTEGER NOT NULL`
- `kind TEXT NOT NULL` (`message`, `tool_call`, `subagent`)
- `message_index INTEGER NULL`
- `tool_call_id TEXT NULL`
- `subagent_id TEXT NULL`

Primary key: `(session_id, item_index)`.

### New table: `tool_calls`

Suggested columns:

- `id TEXT NOT NULL` (session-scoped tool-call identifier)
- `session_id TEXT NOT NULL`
- `subagent_id TEXT NULL`
- `tool_name TEXT NOT NULL`
- `status TEXT NOT NULL` (`pending`, `running`, `completed`, `error`, `unknown`)
- `title TEXT NULL`
- `summary TEXT NULL`
- `input_json TEXT NULL`
- `output_text TEXT NULL`
- `error_text TEXT NULL`
- `started_at INTEGER NULL`
- `ended_at INTEGER NULL`
- `duration_ms INTEGER NULL`
- `parser_call_id TEXT NULL` (tool-specific correlation id; used by Codex for `call_id` begin/end pairing and by Mistral Vibe for `tool_calls[].id`; `NULL` for Claude and OpenCode)

Primary key: `(session_id, id)`.

`subagent_id` is `NULL` for top-level tool calls; set to the parent subagent's `id` for tool calls owned by a subagent.

Indexes:

- `(session_id, subagent_id)`
- `(session_id, parser_call_id)`

### New table: `subagents`

Suggested columns:

- `id TEXT NOT NULL` (session-scoped subagent identifier)
- `session_id TEXT NOT NULL`
- `title TEXT NOT NULL`
- `prompt TEXT NULL`
- `result_summary TEXT NULL`
- `child_session_id TEXT NULL`
- `parser_ref TEXT NULL`

Primary key: `(session_id, id)`.

Indexes:

- `(session_id)`
- `(session_id, child_session_id)`

### Transcript loading query

Load transcript items with previews in a single query:

```sql
SELECT ti.item_index, ti.kind, ti.message_index, ti.tool_call_id, ti.subagent_id,
       m.role, substr(m.content, 1, ?2) AS content_preview,
       length(m.content) AS content_len, m.timestamp,
       tc.tool_name, tc.status, tc.summary, tc.duration_ms,
       sa.title AS subagent_title, sa.prompt AS subagent_prompt
FROM transcript_items ti
LEFT JOIN messages m ON ti.session_id = m.session_id
                    AND ti.message_index = CAST(m.message_index AS INTEGER)
LEFT JOIN tool_calls tc ON ti.session_id = tc.session_id
                       AND ti.tool_call_id = tc.id
LEFT JOIN subagents sa ON ti.session_id = sa.session_id
                      AND ti.subagent_id = sa.id
WHERE ti.session_id = ?1
ORDER BY ti.item_index
LIMIT ?3 OFFSET ?4
```

Pagination applies to `transcript_items` count (not just messages). A session with 200 messages + 150 tool calls = 350 items; page size stays at 200 items per page.

### Search behavior in transcript (v1)

- Existing FTS and highlight/navigation remain **message-based**.
- Tool-call and subagent rows do not participate in match counts for v1.
- Pagination is transcript-item based, while match navigation targets message rows only.
- Search ranking/indexing for tool output remains out of scope for this phase.

## Parser Extraction Rules

### Claude Code

- Extract tool calls from assistant content blocks where `type == "tool_use"`.
- Extract tool results from `type == "tool_result"` blocks.
- Correlation strategy:
  1. match by `tool_result.tool_use_id` when present,
  2. fallback to first unmatched call in order.
- Verify against Claude Code fixtures that `tool_use_id` is consistently present in the wire format.
- Treat `Task` tool usage as subagent entries.

### Codex

- Extract from `event_msg.payload.type`:
  - `mcp_tool_call_begin/end`
  - `exec_command_begin/end`
  - relevant web-search begin/end pairs
- Build subagent entries from `collab_agent_spawn_*` and related thread metadata.
- Correlate begin/end by shared `call_id`.

### OpenCode

- Parse `part.type == "tool"` with lifecycle state fields in `state`.
- Parse `part.type == "subtask"` as subagent entries.
- Stop dropping child sessions entirely; store them as `sessions.is_subagent = 1` and link via `parent_session_id`.
- Keep child sessions hidden from normal list filtering, but available for "Open full session" actions.

### Mistral Vibe

- Extract tool call metadata from assistant `tool_calls[]`.
- Correlate with `role == "tool"` messages by `tool_call_id`.
- No dedicated subagent entities for now.

## UI Components

### App-level pane orchestration (`src/app.rs`)

- Replace `UtilityPaneMode` variants: remove `SessionContext`, keep `Filters`, add `ToolInspector`.
- Remove `DetailContextPane` controller and all references to `DetailContextPaneMsg` / `DetailContextPaneOutput`.
- Remove `AppMsg::ResumeFromPane`.
- Add "Resume in Terminal" `GtkButton` to `AdwHeaderBar`, visible only when `detail_visible == true`. Wire to existing `AppMsg::ResumeSession` using `active_session`.
- Add `ToolInspectorPane` controller to the existing `pane_stack`.
  The stack child name for `ToolInspector` mode is `"tool-inspector"`; update `UtilityPaneMode::stack_child_name()` to return it.
- Keep `transition_to_detail()` / `transition_to_list()` pure (state-only) and apply
  `widgets.overlay_split.set_sidebar_position(...)` inside `update_with_view`
  (`End` for detail, `Start` for list).
- `transition_to_detail()` sets `pane_open = false`; the pane opens only when the user triggers an inspect action.
- Keep F9 shortcut and toggle button behavior unchanged.
- Set main app `AdwNavigationView` `pop_on_escape = false`; route Esc using the priority contract defined above.
- Route transcript selection events into pane mode changes and inspector updates.
- Add one-hop child-session return context in `App` for subagent "Open full session" actions (`child -> parent` only).

### Session detail transcript (`src/ui/session_detail.rs`)

- Replace `FactoryVecDeque<MessageRow>` with `FactoryVecDeque<TranscriptRow>`.
- `TranscriptRow` dispatches internally based on a `TranscriptItemInit` enum:

```rust
enum TranscriptItemInit {
    Message(MessageRowInit),
    ToolCall(ToolCallRowInit),
    Subagent(SubagentRowInit),
}
```

- Introduce row types:
  - message row (existing style, refactored from `MessageRow`)
  - tool-call expander row (icon + name + summary + duration pill + inspect button suffix)
  - subagent expander row (icon + name + summary + tool count pill + inspect button suffix; expanded content shows prompt, inner tool ActionRows, result summary)
- Emit outputs when inspect actions are activated:
  - `TranscriptRowOutput::InspectToolCall(tool_call_id)`
  - `TranscriptRowOutput::InspectSubagent(subagent_id)`
  - `TranscriptRowOutput::InspectSubagentTool(subagent_id, tool_call_id)`

### New pane component (`src/ui/tool_inspector_pane.rs`)

- Root content in `AdwNavigationView`:
  - page 1: selected tool or subagent overview
  - page 2+: subagent inner tool details
- Display:
  - tool identity, status, duration
  - input/output/error blocks (monospace)
  - sibling call navigation list
- Expose outputs for navigation intents:
  - `ToolInspectorPaneOutput::OpenChildSession(child_session_id)`
  - `ToolInspectorPaneOutput::ReturnToParentSession`
  - `ToolInspectorPaneOutput::NavigateToSiblingTool(tool_call_id)`
- Show "Open full session" only when the inspected subagent has a non-null `child_session_id`.
  Hide the button in child-session context (when a one-hop parent return context is already active)
  to prevent recursive child-session navigation at the UI layer.
- Show an explicit empty/placeholder state when no inspector selection exists
  (centered label: "Select a tool call or subagent to inspect").

### Styling (`data/resources/style.css`)

Add classes for:

- selected tool/subagent row state,
- tool status chips,
- inspector code blocks,
- compact metadata labels.

### Removed component (`src/ui/detail_context_pane.rs`)

`DetailContextPane` is removed. Its "Resume in Terminal" functionality moves to a header bar button in `src/app.rs`. The `ActiveSessionRef` struct in `App` already holds the session identity needed for resume routing.

## Data Flow

1. Indexers parse raw sessions and persist `sessions`, `messages` (FTS), and artifact tables.
2. Session detail loads ordered `transcript_items` previews via the LEFT JOIN query.
3. Inline expander interactions stay local to transcript rows.
4. Inspect action emits `TranscriptRowOutput` → forwarded to `AppMsg`.
5. `App` switches pane mode to `ToolInspector`, changes sidebar position to `End`, ensures pane visibility, and forwards selection to inspector pane.
6. Inspector pane can request navigation to sibling calls or child sessions.
7. When opening a child session from the inspector, `App` stores one-hop parent return context and replaces the active detail session with the child session.
8. "Resume in Terminal" is handled by header bar button → `AppMsg::ResumeSession` → existing terminal pipeline.

## Performance and Safety

- Keep parser behavior resilient to malformed entries (warn and continue).
- Store full output but render preview first; lazy-load heavy text into inspector blocks.
- Continue streaming JSONL reads with `BufReader`.
- Avoid hardcoded filesystem paths; use existing source resolvers.
- Paginate transcript items at 200 items per page (same threshold as current message pagination).

## Implementation Phases

### Phase 1 - Schema, models, and fixtures

- Add session columns and new artifact tables in schema initialization code.
- Adopt dev-phase schema policy: on incompatible schema updates, delete DB and reindex (no in-place migration path required).
- Add model structs for transcript items, tool calls, and subagents.
- Update load/query APIs with the LEFT JOIN transcript query.
- Create test fixtures with tool calls and subagents for all 4 parser formats, so phases 2-3 can develop against real data shapes.

### Phase 2 - Parser and indexer enrichment

- Implement extraction for all four parsers.
- Persist transcript ordering and correlated artifacts.
- Verify Claude Code `tool_use_id` correlation against fixture data.
- Unit tests for extraction and correlation edge cases.

### Phase 3 - Pane restructuring and inline transcript rows

- Remove `DetailContextPane` and `SessionContext` mode.
- Add "Resume in Terminal" button to header bar.
- Add `ToolInspector` mode and dynamic sidebar position.
- Replace `FactoryVecDeque<MessageRow>` with `FactoryVecDeque<TranscriptRow>`.
- Implement tool/subagent expander rows in `SessionDetail`.
- Wire inspect outputs to app-level message flow.

### Phase 4 - Inspector pane and drill-down

- Build `ToolInspectorPane` with navigation stack.
- Add subagent inner-tool drill-down and child-session open/return actions.

### Phase 5 - Polish and grouping heuristic

- Optional grouping for consecutive tool-call runs.
- Keyboard/a11y polish.
- Empty/error states and truncation UX tuning.
- Validate sidebar position transition smoothness.

## Testing Strategy

### Automated

- Unit tests per parser for tool extraction and correlation edge cases.
- DB tests for schema initialization/rebuild and ordered transcript retrieval.
- UI component tests for row expansion and inspector routing where practical.

### Manual

- `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures`
- Verify:
  - F9 toggle behavior in list and detail,
  - sidebar position changes on view transitions,
  - "Resume in Terminal" button in header bar,
  - split vs overlay adaptation,
  - inspect action semantics (separate from expand),
  - Esc priority behavior (inspector pop → pane close → detail back),
  - subagent drill-down navigation,
  - child-session open + one-hop return to parent session.

### CI parity before PR

- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`

## Acceptance Criteria

- Tool-call rows render inline for all supported parsers when data exists.
- Inspector pane is non-modal and updates from explicit inspect actions.
- Inspector pane appears on the right (end) in detail view, filters on the left (start) in list view.
- "Resume in Terminal" works from header bar button (no regression from removing `DetailContextPane`).
- Subagent rows support drill-down to inner tool details.
- OpenCode child sessions are linkable without polluting default session list.
- Child-session navigation from inspector supports deterministic one-hop return to parent session.
- Search highlight/navigation continues to work for message rows without regression while transcript pagination is item-based.
- No regressions to existing search, list navigation, or session resume behavior.

## Out of Scope (v1)

- Timeline swimlane visualization of true parallel execution.
- Arbitrary-depth subagent recursion beyond one practical nested drill-down level.
- Reworking search ranking to include tool output content.
