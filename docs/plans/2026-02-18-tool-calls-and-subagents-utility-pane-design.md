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
- Session detail currently renders only message rows.
- A utility-pane stack already exists (`filters` and `session-context`).

## Corrections Applied vs Exploration F

The exploration doc identified the right direction; this design tightens behavior in four places:

1. **No contradiction on pane lifecycle**  
   Pane has explicit hidden/visible states; opening and closing are expected actions.
2. **No click ambiguity with expander rows**  
   Expander row activation controls expansion. A dedicated inspect action opens/updates the pane.
3. **No fixed 73/27 rule**  
   Use `AdwOverlaySplitView` sizing properties (`sidebar_width_fraction`, min/max width) instead of a hard-coded split ratio.
4. **Robust Claude correlation**  
   Prefer `tool_use_id` matching when available; only use ordered fallback as a secondary strategy.

## Options Considered

### A) Reuse app-level utility pane + inline expanders (chosen)

Reuse existing `AdwOverlaySplitView` in `App`, add a third pane mode (`tool-inspector`), and render tool/subagent rows inline in `SessionDetail`.

**Pros:**

- Reuses existing split view, breakpoint, and F9 behavior.
- Avoids nested split containers.
- Keeps one global pane policy across list/detail.

**Cons:**

- Requires new cross-component messages between transcript and app-level pane.

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
- Add `ToolInspector` as a new utility pane mode.
- Render tool/subagent rows inline using expander semantics.
- Open inspector from explicit inspect actions.
- Keep modal dialogs out of normal inspection workflow.

## Interaction Model

## Terminology

- **Utility pane**: HIG concept.
- **Sidebar**: libadwaita API name for the same area.

## State Model

`PaneVisibility`

- `Hidden`
- `Visible`

`PaneLayout` (derived from breakpoint)

- `Split` (wide)
- `Overlay` (narrow, collapsed)

`PaneMode`

- `Filters`
- `SessionContext`
- `ToolInspector`

`InspectorSelection`

- `None`
- `ToolCall(tool_call_id)`
- `Subagent(subagent_id)`
- `SubagentTool(subagent_id, tool_call_id)`

## Interaction Contract

| User action | Expected behavior |
|-------------|-------------------|
| Click expander row body | Toggle inline expand/collapse only |
| Activate inspect suffix on tool row | Select tool call; switch pane mode to `ToolInspector`; open pane if hidden |
| Activate inspect suffix on subagent row | Select subagent overview; switch pane mode to `ToolInspector`; open pane if hidden |
| Activate inner tool action in subagent row | Push detail page in inspector nav stack |
| Press F9 | Toggle pane visibility; keep current mode and selection |
| Press Esc in inspector (overlay mode) | Pop inspector nav page if possible; otherwise close pane |
| Navigate back to session list | Clear inspector selection; switch pane mode to `Filters` |

Notes:

- Expansion and inspection are intentionally separate actions.
- Tool row click does not implicitly resize layout.

## Adaptive Behavior

- Use `AdwBreakpoint` with `sp` units.
- Keep existing app-level collapse behavior and tune threshold as needed (start with current app threshold; validate with Large Text enabled).
- In split mode, configure pane width via split-view properties instead of fixed percentage assumptions.

## Data Model and Persistence

Search remains backed by `messages` FTS5. Tool/subagent rendering uses dedicated normalized tables.

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

- `id TEXT PRIMARY KEY`
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
- `parser_call_id TEXT NULL` (tool-specific correlation id)

Indexes:

- `(session_id, subagent_id)`
- `(session_id, parser_call_id)`

### New table: `subagents`

Suggested columns:

- `id TEXT PRIMARY KEY`
- `session_id TEXT NOT NULL`
- `title TEXT NOT NULL`
- `prompt TEXT NULL`
- `result_summary TEXT NULL`
- `child_session_id TEXT NULL`
- `parser_ref TEXT NULL`

Index: `(session_id)`.

## Parser Extraction Rules

### Claude Code

- Extract tool calls from assistant content blocks where `type == "tool_use"`.
- Extract tool results from `type == "tool_result"` blocks.
- Correlation strategy:
  1. match by `tool_result.tool_use_id` when present,
  2. fallback to first unmatched call in order.
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

- Extend `UtilityPaneMode` with `ToolInspector`.
- Add `ToolInspectorPane` controller to the existing `pane_stack`.
- Keep F9 shortcut and toggle button behavior unchanged.
- Route transcript selection events into pane mode changes and inspector updates.

### Session detail transcript (`src/ui/session_detail.rs`)

- Replace message-only rendering with transcript item rendering.
- Introduce row types:
  - message row (existing style)
  - tool-call expander row
  - subagent expander row with inner action rows
- Emit outputs when inspect actions are activated.

### New pane component (`src/ui/tool_inspector_pane.rs`)

- Root content in `AdwNavigationView`:
  - page 1: selected tool or subagent overview
  - page 2+: subagent inner tool details
- Display:
  - tool identity, status, duration
  - input/output/error blocks (monospace)
  - sibling call navigation list
- Expose outputs for navigation intents (e.g., open child session).

### Styling (`data/resources/style.css`)

Add classes for:

- selected tool/subagent row state,
- tool status chips,
- inspector code blocks,
- compact metadata labels.

## Data Flow

1. Indexers parse raw sessions and persist `sessions`, `messages` (FTS), and artifact tables.
2. Session detail loads ordered `transcript_items` previews.
3. Inline expander interactions stay local to transcript rows.
4. Inspect action sends selection to `App`.
5. `App` switches pane mode to `ToolInspector`, ensures pane visibility, and forwards selection to inspector pane.
6. Inspector pane can request navigation to sibling calls or child sessions.

## Performance and Safety

- Keep parser behavior resilient to malformed entries (warn and continue).
- Store full output but render preview first; lazy-load heavy text into inspector blocks.
- Continue streaming JSONL reads with `BufReader`.
- Avoid hardcoded filesystem paths; use existing source resolvers.

## Implementation Phases

### Phase 1 - Schema and models

- Add session columns and new artifact tables.
- Add model structs for transcript items, tool calls, and subagents.
- Update load/query APIs.

### Phase 2 - Parser and indexer enrichment

- Implement extraction for all four parsers.
- Persist transcript ordering and correlated artifacts.
- Add parser fixtures for tool and subagent coverage.

### Phase 3 - Inline transcript rows

- Implement tool/subagent expander rows in `SessionDetail`.
- Wire inspect outputs to app-level message flow.

### Phase 4 - Inspector pane and drill-down

- Build `ToolInspectorPane` with navigation stack.
- Add subagent inner-tool drill-down and optional child-session open action.

### Phase 5 - polish and grouping heuristic

- Optional grouping for consecutive tool-call runs.
- Keyboard/a11y polish.
- Empty/error states and truncation UX tuning.

## Testing Strategy

### Automated

- Unit tests per parser for tool extraction and correlation edge cases.
- DB tests for schema migration and ordered transcript retrieval.
- UI component tests for row expansion and inspector routing where practical.

### Manual

- `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures`
- Verify:
  - F9 toggle behavior in list and detail,
  - split vs overlay adaptation,
  - inspect action semantics,
  - subagent drill-down navigation.

### CI parity before PR

- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`

## Acceptance Criteria

- Tool-call rows render inline for all supported parsers when data exists.
- Inspector pane is non-modal and updates from explicit inspect actions.
- Subagent rows support drill-down to inner tool details.
- OpenCode child sessions are linkable without polluting default session list.
- No regressions to existing search, list navigation, or session resume behavior.

## Out of Scope (v1)

- Timeline swimlane visualization of true parallel execution.
- Arbitrary-depth subagent recursion beyond one practical nested drill-down level.
- Reworking search ranking to include tool output content.
