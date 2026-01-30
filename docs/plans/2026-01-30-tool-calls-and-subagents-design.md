# Tool Calls & Subagents Display

## Context

Sessions Chronicle currently ignores tool_use/tool_result blocks and rejects
subagent sessions. This design adds full support for displaying tool calls
(with inputs/outputs) and subagent hierarchies.

## Design Decisions

- **Message model: exploded** — A single assistant turn containing text +
  N tool calls is split into N+1 `Message` rows (1 text + N ToolCall),
  ordered by index. Simpler to query and display.
- **Tool results hidden from transcript** — `ToolResult` messages don't appear
  inline; their content is shown in a detail panel on click.
- **Badges + panel UI** — Tool calls appear as compact inline badges in the
  transcript. Clicking a badge opens a lateral detail panel.
- **Subagent = Task tool** — In Claude Code, subagents are `tool_use` blocks
  with `name == "Task"`. In OpenCode, they are separate sessions linked by
  `parent_id`.

---

## Phase A: Data Model & Parsers

### Message model changes

Add fields to `Message`:

- `tool_name: Option<String>` — tool name (e.g. "Bash", "Read", "Edit")
- `tool_input: Option<String>` — serialized input (JSON string)
- `parent_message_index: Option<usize>` — links a ToolResult to its ToolCall

### Session model changes

- `parent_session_id: Option<String>` — links subagent session to parent

### SQLite schema

Add nullable columns to `messages` and `sessions` tables via migration.

### Claude Code parser (`claude_code.rs`)

- `extract_content`: handle `"tool_use"` blocks → emit `Message` with
  `role: ToolCall`, `tool_name`, `tool_input`
- Handle `"tool_result"` blocks → emit `Message` with `role: ToolResult`,
  link via `parent_message_index`
- Identify subagents: `tool_name == "Task"` (no separate session file)

### OpenCode parser (`opencode.rs`)

- Stop rejecting sessions with `parent_id` (`ParseError::SubagentSession`)
- Populate `parent_session_id` from the metadata `parentID` field
- Parse `tool-invocation` and `tool-result` message parts

### Tests

- Unit tests for Claude Code tool_use/tool_result extraction
- Unit tests for OpenCode subagent session parsing
- Integration test: session with mixed text + tool calls produces correct
  message sequence

---

## Phase B: UI — Inline Badges

### ToolBadge widget

- Compact box: icon + tool name (e.g. `⚙ Bash`, `📄 Read`, `✏ Edit`)
- Subagent variant: `🔀 Subagent` for Task tool calls
- Clickable → opens detail panel (Phase C)

### Transcript integration

- In `SessionDetail`, insert `ToolBadge` widgets at the correct index
  between text messages
- `ToolResult` messages are not rendered in the transcript

---

## Phase C: UI — Detail Panel

### ToolDetailPanel widget

- Lateral panel (right side) opened on badge click
- Displays: tool name, formatted input, output/result
- Adapted rendering per tool type:
  - **Bash**: monospace for command + output
  - **Read/Edit/Write**: file path + content
  - **Task (subagent)**: prompt sent + result summary, with a
    "View session" link for OpenCode (navigates to child session)
- Close button to dismiss

---

## Phase D: UI — Subagent Tree

### Header button

- Button in `SessionDetail` header: "Subagents (N)", visible only when
  the session has subagent tool calls or child sessions

### Tree widget

- Simple tree view: parent → children (one level deep typically)
- Each node shows: subagent description (from Task tool input)
- Click navigates to:
  - **OpenCode**: the child session
  - **Claude Code**: scrolls to the corresponding badge in the transcript

---

## Implementation Order

Phases are sequential: A → B → C → D. Each phase is independently
shippable. Phase D (subagent tree) depends on A but is optional and can
be deferred.
