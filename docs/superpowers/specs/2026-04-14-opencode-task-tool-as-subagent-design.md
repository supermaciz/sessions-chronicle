# OpenCode `tool == "task"` as Subagent — Design

Date: 2026-04-14
Status: Implemented [#120](https://github.com/supermaciz/sessions-chronicle/pull/120)

## Problem

The OpenCode parser currently indexes `part.type == "tool" && tool == "task"` as a generic tool call. In reality, this is the concrete subagent-delegation tool: it carries the child session ID in `state.metadata.sessionId`, along with the prompt, the requested subagent type, and the full output. As a result:

- Task-tool invocations show up in the tool inspector as opaque generic calls instead of in the dedicated subagents panel with drill-down and child-session navigation.
- The older `part.type == "subtask"` marker is indexed as a subagent, which causes duplication for sessions that emit both shapes for the same delegation.

Claude Code is already correct: `src/parsers/claude_code.rs:517` maps both `Task` (legacy) and `Agent` (current) tool-use names to subagent records. No changes are needed there.

## Data observed (local OpenCode DB, 2026-04-14)

Source: `~/.local/share/opencode/opencode.db`.

- `tool == "task"` parts: 629 (latest 2026-04-13).
- `part.type == "subtask"` parts: 73 (latest 2026-04-08).
- All 59 sessions containing `subtask` also contain `tool == "task"` parts.
- Descriptions overlap between the two shapes for the same delegation; the `tool == "task"` record is the complete one (has status, child session ID, full output).

Conclusion: the two shapes coexist in the same sessions and describe the same delegations. `tool == "task"` is the source of truth; `subtask` is a lighter announcement that duplicates it.

## Scope

- Recognise `part.type == "tool" && tool == "task"` as a subagent record in the OpenCode parser.
- Exclude those parts from the generic tool-call list so they do not appear twice.
- Dedup against `subtask` using a per-session "has task tool" flag.
- No UI changes: the existing subagents panel handles `child_session_id` drill-down already.

Out of scope:

- Matching `subtask` ↔ `tool == "task"` by description or timestamp (data shows it is not needed).
- Any change to Claude Code, Codex, or Mistral Vibe parsers.
- Changes to SQL schema or indexing pipeline.

## Parser changes — `src/parsers/opencode/mod.rs`

Before the current parts loop, precompute:

```rust
let has_task_tool_in_session = parts.iter().any(|p|
    p.get("type").and_then(|v| v.as_str()) == Some("tool")
    && p.get("tool").and_then(|v| v.as_str()) == Some("task")
);
```

Then, inside the parts loop:

- `part.type == "tool" && part.tool == "task"` → push a `Subagent`:
  - `title` = `state.input.description`
  - `prompt` = `state.input.prompt`
  - `subagent_type` = `state.input.subagent_type`
  - `child_session_id` = `state.metadata.sessionId`
  - `result_summary` = `state.output` (truncated consistently with other subagent results)
  - Do **not** also push a `ToolCall` for this part.
- `part.type == "subtask"`:
  - If `has_task_tool_in_session` → skip (dedup).
  - Else → existing legacy behaviour.
- All other `tool` parts: unchanged.

Edge cases:

- Missing `state.input.description` → fall back to empty title, consistent with existing subagent handling.
- Missing `state.metadata.sessionId` → store `child_session_id = None`; subagent still recorded.
- `status == "error"` or `"running"` → still indexed; `result_summary` uses whatever is present (`state.error` message or empty).

## Tests — `src/parsers/opencode/mod.rs`

Three new tests under the existing `tests` module:

1. `task_tool_produces_subagent_entry` — session with a single `tool == "task"` part. Assert: one `Subagent` with expected title, prompt, `subagent_type`, `child_session_id`, and no `ToolCall` entry for that part.
2. `task_and_subtask_coexist_dedup` — session with both a `tool == "task"` and a `subtask` sharing the same description. Assert: exactly one `Subagent` (from the task tool); the `subtask` is skipped.
3. `subtask_alone_still_works` — session with only a `subtask` part. Assert: legacy behaviour preserved (one `Subagent` from the subtask).

## UI / inspector

No code changes. The existing subagents panel already:

- Lists subagents separately from tool calls.
- Handles `child_session_id` drill-down to open the child session.

Task-tool delegations will appear naturally in that panel once the parser emits them as subagents.

## Docs

Update to reflect the new parser behaviour:

- `docs/session-formats/opencode.md` — remove the "current parser does not yet special-case `tool == task`" note; document the dedup rule against `subtask`.
- `docs/PARSER_DESIGN.md` — same alignment.

## Verification

- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`
- Manual run against a real OpenCode session directory via `--sessions-dir`, confirm task-tool delegations appear in the subagents panel with working child-session drill-down.
