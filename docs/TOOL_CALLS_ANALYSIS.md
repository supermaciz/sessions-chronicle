# Tool Calls Analysis (Claude Code, OpenCode, Codex CLI, Mistral Vibe)

Analysis of tool call formats to enable richer Tool Inspector rendering than raw JSON.

## Goal

Document real-world tool call and tool result shapes for the 4 supported agents:

- Claude Code
- OpenCode
- Codex CLI
- Mistral Vibe

## Methodology

Sources used:

- Sampling real sessions from `~/.claude`, `~/.local/share/opencode`, `~/.codex/sessions`, `~/.vibe/logs/session`
- Reviewing current parsers in `src/parsers/*.rs`
- Verifying upstream schemas:
  - Claude Code: [Anthropic API tool use docs](https://docs.anthropic.com/en/docs/build-with-claude/tool-use/overview)
  - Codex protocol/models: `openai/codex` (`codex-rs/protocol/src/protocol.rs`, `codex-rs/protocol/src/models.rs`)
  - OpenCode MessageV2: `anomalyco/opencode` (`packages/opencode/src/session/message-v2.ts`)
  - Mistral Vibe logger/types: `mistralai/mistral-vibe` (`vibe/core/session/session_logger.py`, `vibe/core/types.py`)

Local sample size (home directory):

- Claude Code: 524 JSONL files, 7,950 `tool_use`, 7,948 `tool_result`
- OpenCode: SQLite `opencode.db`, 11,083 `part.type == "tool"`
- Codex CLI: 69 rollout JSONL files, 814 `function_call` + 814 `function_call_output`
- Mistral Vibe: 22 sessions, 200 `tool_calls` + 200 `role == "tool"` messages

## High-Level Comparison (by agent)

| Agent | Tool invocation | Tool result | Correlation | Input | Output |
|---|---|---|---|---|---|
| Claude Code | `assistant.message.content[]` block `type:"tool_use"` | `user.message.content[]` block `type:"tool_result"` | `tool_use.id` → `tool_result.tool_use_id` | JSON object | string or content-block array |
| OpenCode | `part` with `type:"tool"` (state in `state`) | same `part.state` (`completed/error/running`) | internal `callID` + `part` position | JSON object (`state.input`) | string (`state.output`) + optional `attachments[]` |
| Codex CLI (current) | `response_item.payload.type:"function_call"` (or `custom_tool_call`) | `response_item.payload.type:"function_call_output"` (or `custom_tool_call_output`) | `call_id` | JSON string (arguments) or free-form string (custom input) | usually string (sometimes JSON string) |
| Codex CLI (legacy) | `event_msg.payload.type:"mcp_tool_call_begin"` / `"exec_command_begin"` | `event_msg.payload.type:"mcp_tool_call_end"` / `"exec_command_end"` | `call_id` | JSON object (`input`) or synthesized from `command`+`cwd` | string (`output`/`stdout`) |
| Mistral Vibe | assistant message with `tool_calls[]` | following message with `role:"tool"` | `tool_calls[i].id` → `tool_call_id` | JSON string in `function.arguments` | free-form string |

---

## Claude Code

### Main shape

```json
{
  "type": "tool_use",
  "id": "toolu_...",
  "name": "Bash",
  "input": { "command": "git status", "description": "..." },
  "caller": { "type": "direct" }
}
```

```json
{
  "type": "tool_result",
  "tool_use_id": "toolu_...",
  "content": "...",
  "is_error": false
}
```

### Observed variants

- `tool_result.content` is mostly a string, but can be a content-block array
- content-block arrays seen in practice:
  - `{"type":"text","text":"..."}`
  - `{"type":"image","source":{"type":"base64","data":"..."}}` (rare, but real)
- `tool_use` sometimes includes `caller`, sometimes not
- tool names are heterogeneous (`Bash`, `Read`, `Task`, `Skill`, `mcp__...`)
- `tool_result.is_error` — boolean, `true` when the tool execution itself failed
  (distinct from a tool that returns an error message in `content`)

### Error detection

Two error signals exist in Claude `tool_result`:

1. **`is_error: true`** — explicit error flag on the `tool_result` block
   (Currently **not** checked by the parser; status is set to `Completed` whenever a result arrives.)
2. **Error text in `content`** — some tool results contain error messages in their content
   without setting `is_error`; these are not distinguishable from normal output without heuristics.

### Subagent detection

Tool calls with `name == "Agent"` are the current Claude Code subagent marker;
legacy `name == "Task"` remains relevant for older sessions. The parser extracts
`input.description` as title and `input.prompt` as the subagent prompt. The tool result
becomes `result_summary` on the `Subagent` record.

### Side note

- `system/subtype:"local_command"` events also exist (slash commands), with XML-like payloads in `content` (`<command-name>...</command-name>`), but this is a different family than `tool_use/tool_result`.

---

## OpenCode

### Main shape (`part.type == "tool"`)

```json
{
  "type": "tool",
  "callID": "call_...",
  "tool": "bash",
  "state": {
    "status": "completed",
    "input": { "command": "..." },
    "output": "...",
    "title": "...",
    "metadata": { "exit": 0, "truncated": false },
    "time": { "start": 1771033130000, "end": 1771033130037 },
    "attachments": []
  }
}
```

### State machine

```
pending  →  running  →  completed
                     →  error
```

| State | Fields |
|-------|--------|
| `pending` | `input`, `raw` |
| `running` | `input`, optional `title`/`metadata`, `time.start` |
| `completed` | `input`, `output`, `title`, `metadata`, `time.start/end`, optional `attachments` |
| `error` | `input`, `error`, optional `metadata`, `time.start/end` |

### Important variants

- `input` is always a JSON object
- `output` is a string (often very large — can contain full file contents or diffs)
- `error` is a string when present (on `error` status)
- `attachments` can contain files (including image data URLs in base64)
- `metadata.exit` carries the process exit code for shell-type tools
- `time.start`/`time.end` are Unix timestamps in **milliseconds**

### Subagent detection

Current OpenCode delegated subagent work is best detected from the task tool record,
not just from `part.type == "subtask"`:

```json
{
  "type": "tool",
  "tool": "task",
  "state": {
    "status": "completed",
    "input": {
      "description": "Explore parser layout",
      "prompt": "Find all parser files",
      "subagent_type": "explore",
      "task_id": "ses_existing_child"
    },
    "title": "Explore parser layout",
    "metadata": {
      "sessionId": "ses_child123"
    }
  }
}
```

The child session link lives in `state.metadata.sessionId`. `part.type == "subtask"` is a
distinct related part shape with fields such as `description`, `prompt`, `agent`, optional
`model`, and optional `command`; it is not the same record type as `tool == "task"`.

---

## Codex CLI

### 1) Current format observed in recent sessions

Tool activity is now mostly in `response_item` (not `event_msg`):

```json
{
  "type": "response_item",
  "payload": {
    "type": "function_call",
    "name": "exec_command",
    "arguments": "{\"cmd\":\"git status\"}",
    "call_id": "call_..."
  }
}
```

```json
{
  "type": "response_item",
  "payload": {
    "type": "function_call_output",
    "call_id": "call_...",
    "output": "Chunk ID: ...\nProcess exited with code 0\nOutput:\n..."
  }
}
```

Other `response_item` types:

- `custom_tool_call` / `custom_tool_call_output` (example: `apply_patch`)
- `web_search_call` with `action`:
  - `{"type":"search","query":"...","queries":[...]}`
  - `{"type":"open_page","url":"..."}` (url may be absent)
  - `{"type":"find_in_page","url":"...","pattern":"..."}` (url may be absent)
- `local_shell_call` — shell tool with `command`, `workdir`, `timeout_ms`, `sandbox_permissions`
- `image_generation_call` — image generation with `revised_prompt`, `result`

### Rendering-specific details

- `arguments` is a JSON string (should be parsed)
- `output` is often raw text; depending on the tool it can also be a JSON string
  - example `shell`: `{"output":"...","metadata":{"exit_code":0,...}}`
  - example `update_plan`: simple string (`"Plan updated"`)

### 2) Legacy format still present in fixtures/tests

The current parser (`src/parsers/codex.rs`) indexes `event_msg` begin/end pairs:

- `mcp_tool_call_begin` / `mcp_tool_call_end`
- `exec_command_begin` / `exec_command_end`

This format exists in fixtures, but recent real logs are mostly in `response_item.function_call*`.

### Subagent detection

Codex uses collaboration events for subagent spawning:

- `collab_agent_spawn_begin` / `collab_agent_spawn_end` carry `call_id`, `sender_thread_id`,
  and `new_thread_id`
- Additional events: `collab_waiting_*`, `collab_resume_*`, `collab_close_*`
- Currently **not** mapped to subagent records by the parser.

---

## Mistral Vibe

### Main shape

```json
{
  "role": "assistant",
  "tool_calls": [
    {
      "id": "abc123",
      "index": 0,
      "type": "function",
      "function": {
        "name": "bash",
        "arguments": "{\"command\":\"ls -la\"}"
      }
    }
  ]
}
```

```json
{
  "role": "tool",
  "name": "bash",
  "tool_call_id": "abc123",
  "content": "stdout: ...\nstderr: ...\nreturncode: 0"
}
```

### Important variants

- `function.arguments` is a JSON string (should be parsed)
- `role:"tool".content` is free-form text (shape depends on the tool wrapper)
- no explicit native status field ("completed" is inferred when the tool message arrives)
- `name` field on `role:"tool"` messages may or may not be present (not required for correlation)

### Error detection

No explicit error flag in the JSONL log. Errors can only be inferred from:
- content containing `returncode: <non-zero>` (tool-wrapper specific)
- absence of a matching `role:"tool"` message (tool call stays `Pending`)

Note: internally, Mistral Vibe has a `ToolResultEvent` with `error: Option[str]`,
`duration: Option[float]`, and `skipped: bool` fields, but these are **not persisted**
to the `messages.jsonl` log — only the final `LLMMessage` is serialized.

### Subagent detection

No subagent/subtask mechanism observed in current Mistral Vibe format.
All tool calls are top-level.

---

## Parser Field Coverage

The parsers normalize each tool call into a common `ToolCall` struct. Not all fields are
populated by every parser:

| Field | Claude Code | OpenCode | Codex CLI | Mistral Vibe |
|-------|:-----------:|:--------:|:---------:|:------------:|
| `id` | `tool_use.id` | `{ses}-{msg}-{part}` | `call_id` | `{ses}-{raw_id}` |
| `tool_name` | `tool_use.name` | `part.tool` | `payload.tool_name` or `command` | `function.name` |
| `status` | Pending → Completed | Running / Completed / Error / Unknown (`pending` → Unknown) | Running → Completed/Error | Pending → Completed |
| `title` | tool_name | — | tool_name | — |
| `summary` | — | — | — | — |
| `input_json` | `tool_use.input` (object) | `state.input` (object) | `payload.input` (object) | `function.arguments` (string) |
| `output_text` | `tool_result.content` | `state.output` | `payload.output`/`stdout` | `role:tool.content` |
| `error_text` | — | `state.error` | `payload.stderr` (if non-empty) | — |
| `started_at` | event timestamp | — | event timestamp | — |
| `ended_at` | event timestamp | — | event timestamp | — |
| `duration_ms` | end − start | — | `payload.duration_ms` | — |
| `parser_call_id` | — | — | `call_id` | — |

**Legend:** `—` = always NULL

### Notable gaps

- **Claude Code**: `is_error` on `tool_result` is not checked — all completed tool calls show
  status `Completed` even when the tool failed.
- **OpenCode**: `state.time.start/end` (Unix ms) is available in the raw data but **not extracted**
  by the parser into `started_at`/`ended_at`/`duration_ms`.
- **OpenCode**: `state.title` is available but **not extracted** into `ToolCall.title`.
- **OpenCode**: `state.metadata.exit` (exit code) is available but not surfaced.
- **Codex CLI**: Only the legacy `event_msg` begin/end format is parsed; the newer
  `response_item.function_call*` format is **not yet supported**.
- **Codex CLI**: `collab_*` events are not yet mapped to subagent records.
- **Mistral Vibe**: No timing information available in the format itself.
- **All parsers**: `summary` field is never populated.

---

## Subagent Detection — Cross-Tool Summary

| Agent | Subagent trigger | Subagent data fields | Child session link |
|-------|-----------------|---------------------|-------------------|
| Claude Code | `tool_use.name == "Agent"` (legacy `Task` alias) | `input.description` (title), `input.prompt`, `input.subagent_type` | None (inline in parent JSONL) |
| OpenCode | `part.type == "tool" && tool == "task"` | `state.input.description`, `state.input.prompt`, `state.input.subagent_type` | `state.metadata.sessionId` → child `ses_*` |
| Codex CLI | `collab_agent_spawn_begin/end` | `prompt`, `sender_thread_id`, `new_thread_id` | Thread-based (not yet parsed) |
| Mistral Vibe | — | — | — |

---

## UI Implications (styled rendering)

### Recommended normalization

For UI rendering, normalize each entry into a common shape:

```json
{
  "id": "...",
  "tool_name": "...",
  "status": "pending|running|completed|error|unknown",
  "input": {"raw": "...", "parsed": {}},
  "output": {"raw": "...", "parsed": {}},
  "error": "...",
  "attachments": []
}
```

### Suggested parsing rules

1. **Input**
   - If native object (Claude/OpenCode): use directly
   - If string (Codex/Mistral): attempt `JSON.parse`, fallback to raw text

2. **Output**
   - If string: attempt `JSON.parse` only if it looks like JSON (`{` or `[`)
   - Otherwise display raw text
   - If content-block array (Claude): render `text` and `image` blocks

3. **Status**
   - OpenCode: native status field (`completed`, `error`, `running`, `pending`)
   - Claude: check `tool_result.is_error` first; fallback to result presence/absence
   - Codex: infer from output/error content + `exit_code` + call/output pairing
   - Mistral: infer via `tool_call_id` correlation (present = completed, absent = pending)

4. **Attachments**
   - OpenCode: `state.attachments[]` supports media in base64 data URLs
   - Claude: images can appear in `tool_result.content[]` as `{"type":"image",...}`
   - Add size/memory safeguards — attachments can be large

---

## Watchouts

- **Huge outputs** — diffs, file contents, and logs can be hundreds of KB; truncate or virtualize in UI
- **Sensitive data** — API keys, tokens, and private URLs can appear in tool outputs
- **Inter-version drift** — especially Codex legacy (`event_msg`) vs current (`response_item`) format
- **Frequent optional fields** — do not assume `status`, `url`, `metadata`, `time`, etc. always exist
- **Duplicate events** — Claude Code logs are append-only and can contain duplicate entries for the same
  tool call (see [claude-code#1524](https://github.com/anthropics/claude-code/issues/1524))

---

## Conclusion

The 4 agents use different tool-call strategies:

- **Claude Code**: Anthropic API `tool_use`/`tool_result` blocks embedded in messages; rich structure
  but no native timing data beyond event timestamps
- **OpenCode**: Self-contained `tool` parts with a full state machine (`pending → running → completed/error`),
  timing, metadata, and attachments; the richest tool call format
- **Codex CLI**: Evolving format — legacy `event_msg` begin/end pairs (with precise timing) are being
  superseded by `response_item.function_call*` (string-heavy); also has `custom_tool_call` and
  `web_search_call` families
- **Mistral Vibe**: Simple OpenAI-compatible format (string arguments/output); no timing, no error flag,
  no subagents

For a polished UI, the key is a normalization layer that:

- parses JSON strings when relevant,
- preserves robust raw-text fallbacks,
- handles media/attachments safely,
- and explicitly supports version-specific variants (especially Codex).

---

## References

- [Anthropic API — Tool Use](https://docs.anthropic.com/en/docs/build-with-claude/tool-use/overview)
- [Codex protocol definitions (`protocol.rs`)](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs)
- [Codex rollout recorder](https://github.com/openai/codex/blob/main/codex-rs/core/src/rollout/recorder.rs)
- [OpenCode `MessageV2` part schemas](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/message-v2.ts)
- [Mistral Vibe session logger](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/session/session_logger.py)
- [Mistral Vibe types](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/types.py)
