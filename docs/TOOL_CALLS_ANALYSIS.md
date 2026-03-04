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
  - Codex protocol/models: `openai/codex` (`codex-rs/protocol/src/protocol.rs`, `codex-rs/protocol/src/models.rs`)
  - OpenCode MessageV2: `sst/opencode` (`packages/opencode/src/session/message-v2.ts`)
  - Mistral Vibe logger/types: `mistralai/mistral-vibe` (`vibe/core/session/session_logger.py`, `vibe/core/types.py`)

Local sample size (home directory):

- Claude Code: 524 JSONL files, 7,950 `tool_use`, 7,948 `tool_result`
- OpenCode: SQLite `opencode.db`, 11,083 `part.type == "tool"`
- Codex CLI: 69 rollout JSONL files, 814 `function_call` + 814 `function_call_output`
- Mistral Vibe: 22 sessions, 200 `tool_calls` + 200 `role == "tool"` messages

## High-Level Comparison (by agent)

| Agent | Tool invocation | Tool result | Correlation | Input | Output |
|---|---|---|---|---|---|
| Claude Code | `assistant.message.content[]` block `type:"tool_use"` | `user.message.content[]` block `type:"tool_result"` | `tool_use.id` -> `tool_result.tool_use_id` | JSON object | string or content-block array |
| OpenCode | `part` with `type:"tool"` (state in `state`) | same `part.state` (`completed/error/running`) | internal `callID` + `part` position | JSON object (`state.input`) | string (`state.output`) + optional `attachments[]` |
| Codex CLI (current) | `response_item.payload.type:"function_call"` (or `custom_tool_call`) | `response_item.payload.type:"function_call_output"` (or `custom_tool_call_output`) | `call_id` | JSON string (arguments) or free-form string (custom input) | usually string (sometimes JSON string) |
| Mistral Vibe | assistant message with `tool_calls[]` | following message with `role:"tool"` | `tool_calls[i].id` -> `tool_call_id` | JSON string in `function.arguments` | free-form string |

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

### States and fields

- Observed `status` values: `completed`, `error`, `running`
- Completed: `input`, `output`, `title`, `metadata`, `time`, `attachments?`
- Error: `input`, `error`, `time`, `metadata?`
- Running: `input`, `time`, `metadata?`, sometimes `title`

### Important variants

- `input` is always a JSON object
- `output` is a string (often very large)
- `attachments` can contain files (including image data URLs in base64)
- `part.type == "subtask"` is a separate family (delegation/subagent), useful for a dedicated subagent panel

---

## Codex CLI

## 1) Current format observed in recent sessions

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

Other active types:

- `custom_tool_call` / `custom_tool_call_output` (example: `apply_patch`)
- `web_search_call` with `action`:
  - `{"type":"search","query":"...","queries":[...]}`
  - `{"type":"open_page","url":"..."}` (url may be absent)
  - `{"type":"find_in_page","url":"...","pattern":"..."}` (url may be absent)

### Rendering-specific details

- `arguments` is a JSON string (should be parsed)
- `output` is often raw text; depending on the tool it can also be a JSON string
  - example `shell`: `{"output":"...","metadata":{"exit_code":0,...}}`
  - example `update_plan`: simple string (`"Plan updated"`)

## 2) Legacy format still present in fixtures/tests

The current parser (`src/parsers/codex.rs`) still indexes `event_msg` begin/end pairs:

- `mcp_tool_call_begin` / `mcp_tool_call_end`
- `exec_command_begin` / `exec_command_end`

This format exists in fixtures, but recent real logs are mostly in `response_item.function_call*`.

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

---

## UI Implications (styled rendering)

## Recommended normalization

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
   - OpenCode: native status
   - Claude: `tool_result.is_error` or result presence/absence
   - Codex: infer from output/error content + call/output pairing
   - Mistral: infer via `tool_call_id` correlation

4. **Attachments**
   - OpenCode: `state.attachments[]` supports media in base64 data URLs
   - Claude: images can appear in `tool_result.content[]`
   - Add size/memory safeguards

---

## Watchouts

- Potentially huge outputs (diffs, files, logs)
- Sensitive data can appear in tool outputs (API keys, tokens, private URLs)
- Inter-version drift (especially Codex legacy vs current format)
- Frequent optional fields (do not assume `status`, `url`, `metadata`, etc. always exist)

---

## Conclusion

The 4 agents use different tool-call strategies:

- **Claude/OpenCode**: relatively rich structures, close to stable event models
- **Mistral Vibe**: simple OpenAI-like format (string arguments/output)
- **Codex CLI**: recent format centered on `response_item.function_call*` (string-heavy), with additional `custom_tool_call` and `web_search_call` families

For a polished UI, the key is a normalization layer that:

- parses JSON strings when relevant,
- preserves robust raw-text fallbacks,
- handles media/attachments safely,
- and explicitly supports version-specific variants (especially Codex).
