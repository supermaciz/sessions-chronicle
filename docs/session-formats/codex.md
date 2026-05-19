# Codex — Session Format Reference

Format reference for Codex rollout session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-assistant comparison tables.

---

## Storage & File Naming

| Field   | Value |
|---------|-------|
| **Path** | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| **Pattern** | `rollout-*.jsonl` |
| **Example** | `rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl` |
| **Format** | JSONL (one JSON object per line, UTF-8, append-only) |

**Date sharding:**

```
~/.codex/sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl
                  └─────────┘          └─────────────────────────────────────────────────────────┘
                  Date sharding        Timestamp + thread id in filename
```

---

## Event Structure

Codex rollout logs are envelope-based JSONL entries (`RolloutLine`).

```json
{
  "timestamp": "2026-01-18T01:01:30.000Z",
  "type": "session_meta" | "event_msg" | "response_item" | "turn_context" | "compacted",
  "payload": { "...": "..." }
}
```

### Session Metadata (`session_meta`)

```json
{
  "timestamp": "2026-01-18T01:01:28.000Z",
  "type": "session_meta",
  "payload": {
    "id": "019bce9f-0a40-79e2-8351-8818e8487fb6",
    "timestamp": "2026-01-18T01:01:28.000Z",
    "cwd": "/home/user/project",
    "originator": "codex_cli_rs",
    "cli_version": "0.117.0",
    "source": "cli",
    "model_provider": "openai"
  }
}
```

`source` is now a structured `SessionSource` in upstream types. It can still be a
simple source like `cli`, `vscode`, `exec`, or `unknown`, but it can also be a
structured subagent/custom value.

Representative shapes:

```json
"source": "cli"
```

```json
"source": {
  "sub_agent": {
    "thread_spawn": {
      "parent_thread_id": "thr_parent",
      "depth": 1,
      "agent_path": "/root/agent_a",
      "agent_nickname": "agent_a",
      "agent_role": "reviewer"
    }
  }
}
```

Additional `session_meta` fields now present in upstream types include
`forked_from_id`, `agent_nickname`, `agent_role`, `agent_path`,
`base_instructions`, `dynamic_tools`, and `memory_mode`.

### User / Assistant Events (`event_msg`)

```json
{
  "timestamp": "2026-01-18T01:01:30.000Z",
  "type": "event_msg",
  "payload": {
    "type": "user_message",
    "message": "Summarize the repo"
  }
}
{
  "timestamp": "2026-01-18T01:01:31.000Z",
  "type": "event_msg",
  "payload": {
    "type": "agent_message",
    "message": "Here is the summary"
  }
}
```

### Turn Context (model captured per turn)

```json
{
  "timestamp": "2026-01-18T01:01:29.500Z",
  "type": "turn_context",
  "payload": {
    "cwd": "/home/user/project",
    "approval_policy": "on-request",
    "sandbox_policy": { "type": "workspace-write" },
    "model": "gpt-5.1-codex",
    "summary": "auto"
  }
}
```

### Session Configured Event (can include model + provider)

```json
{
  "timestamp": "2026-01-18T01:01:28.500Z",
  "type": "event_msg",
  "payload": {
    "type": "session_configured",
    "session_id": "019bce9f-0a40-79e2-8351-8818e8487fb6",
    "model": "codex-mini-latest",
    "model_provider_id": "openai"
  }
}
```

### Tool-Related Events

```json
{
  "type": "event_msg",
  "payload": {
    "type": "mcp_tool_call_begin",
    "call_id": "call_123",
    "invocation": {
      "server": "filesystem",
      "tool": "read_file",
      "arguments": { "path": "README.md" }
    }
  }
}
{
  "type": "event_msg",
  "payload": {
    "type": "mcp_tool_call_end",
    "call_id": "call_123",
    "result": { "Ok": { "is_error": false, "content": [] } }
  }
}
```

Other tool-related `event_msg.payload.type` variants: `exec_command_*`, `web_search_*`.

### Skill Invocation Format

Codex CLI skill usage is persisted as an explicit user invocation plus a
separate injected payload. In sampled rollouts, there is no dedicated
Codex-native skill tool-call event analogous to OpenCode's `tool == "skill"`.

**1. Explicit invocation** — `user_message`:

```json
{
  "type": "event_msg",
  "payload": {
    "type": "user_message",
    "message": "$logseq un fichier markdown",
    "text_elements": [
      {
        "byte_range": { "start": 0, "end": 7 },
        "placeholder": "$logseq"
      }
    ]
  }
}
```

**2. Injected skill payload** — `response_item` user message:

```xml
<skill>
<name>logseq</name>
<path>/home/user/project/skills/logseq/SKILL.md</path>
---
name: logseq
description: ...
---
...
</skill>
```

Observed semantics:

- Explicit invocation appears in `event_msg.payload.type == "user_message"`
  with a leading `$skill-name`
- The loaded skill payload is injected as `response_item.payload.type ==
  "message"` with `role == "user"`
- The injected payload uses a `<skill>` wrapper with `<name>` and `<path>`
  headers, followed by the skill frontmatter/body
- `text_elements[].placeholder` can preserve the exact `$skill-name` token but
  is not consistently populated
- In sampled local sessions, every injected `<skill>` payload was preceded by
  an explicit `$skill-name` user message
- If Codex reports that a named skill is unavailable, the rollout can contain
  the `$skill-name` user message without a following `<skill>` payload

### Collaboration / Subagent Events

Current upstream Codex protocol exposes subagent work through the `collab_*`
`event_msg.payload.type` family. These events are emitted by the multi-agent
callable surface (`spawn_agent`, `send_message`, `followup_task`,
`wait_agent`, `close_agent`, and resume flows).

```json
{
  "type": "event_msg",
  "payload": {
    "type": "collab_agent_spawn_begin",
    "call_id": "spawn_1",
    "started_at_ms": 1770000000000,
    "sender_thread_id": "thr_parent",
    "prompt": "Investigate failing tests",
    "model": "gpt-5.1-codex",
    "reasoning_effort": "medium"
  }
}
{
  "type": "event_msg",
  "payload": {
    "type": "collab_agent_spawn_end",
    "call_id": "spawn_1",
    "completed_at_ms": 1770000001000,
    "sender_thread_id": "thr_parent",
    "new_thread_id": "thr_child",
    "new_agent_nickname": "reviewer-a",
    "new_agent_role": "reviewer",
    "prompt": "Investigate failing tests",
    "model": "gpt-5.1-codex",
    "reasoning_effort": "medium",
    "status": "completed"
  }
}
```

Current collab event types:

- `collab_agent_spawn_begin` / `collab_agent_spawn_end`
- `collab_agent_interaction_begin` / `collab_agent_interaction_end`
- `collab_waiting_begin` / `collab_waiting_end`
- `collab_close_begin` / `collab_close_end`
- `collab_resume_begin` / `collab_resume_end`

Current parser-relevant fields:

| Event | Key fields |
|-------|------------|
| `collab_agent_spawn_end` | `call_id`, `sender_thread_id`, `new_thread_id`, `new_agent_nickname`, `new_agent_role`, `prompt`, `model`, `reasoning_effort`, `status`, `completed_at_ms` |
| `collab_agent_interaction_end` | `call_id`, `sender_thread_id`, `receiver_thread_id`, `receiver_agent_nickname`, `receiver_agent_role`, `prompt`, `status`, `completed_at_ms` |
| `collab_waiting_end` | `call_id`, `sender_thread_id`, `agent_statuses[]`, `statuses{thread_id -> status}`, `completed_at_ms` |
| `collab_close_end` | `call_id`, `sender_thread_id`, `receiver_thread_id`, `receiver_agent_nickname`, `receiver_agent_role`, `status`, `completed_at_ms` |
| `collab_resume_end` | `call_id`, `sender_thread_id`, `receiver_thread_id`, `receiver_agent_nickname`, `receiver_agent_role`, `status`, `completed_at_ms` |

`AgentStatus` currently serializes with these variants:

- `"pending_init"`
- `"running"`
- `"interrupted"`
- `{ "completed": "<final assistant message>" }` or `{ "completed": null }`
- `{ "errored": "<error message>" }`
- `"shutdown"`
- `"not_found"`

Notes:

- Timing fields are useful for ordering and future duration display, but the
  current parser uses the rollout-line `timestamp` for session ordering.
- `model` and `reasoning_effort` describe the effective spawned agent settings.
  They are currently not stored by Sessions Chronicle's subagent model.
- `collab_resume_end` has the same status-bearing shape as close/interaction
  events, but the current parser does not yet use it for subagent summary
  enrichment.
- `wait_agent` can emit `collab_waiting_end` with empty `agent_statuses` and
  empty `statuses`; treat this as no per-agent status update.

### Subagent Tool Calls in Response Items

Local Codex `0.130.0` rollouts can persist subagent operations as ordinary
`response_item` tool calls instead of `event_msg.payload.type == "collab_*"`.
This shape was observed in a real local parent rollout with
`originator == "codex-tui"` and `cli_version == "0.130.0"`.

Spawn call:

```json
{
  "type": "response_item",
  "payload": {
    "type": "function_call",
    "call_id": "call_spawn",
    "name": "spawn_agent",
    "arguments": {
      "agent_type": "product-manager",
      "message": "...",
      "reasoning_effort": "medium"
    }
  }
}
```

Spawn output:

```json
{
  "type": "response_item",
  "payload": {
    "type": "function_call_output",
    "call_id": "call_spawn",
    "output": "{\"agent_id\":\"019e382d-e986-7b62-9f97-b015c5cc70f5\",\"nickname\":\"Nord\"}"
  }
}
```

Wait output can also carry a per-agent status map:

```json
{
  "type": "response_item",
  "payload": {
    "type": "function_call_output",
    "call_id": "call_wait",
    "output": "{\"status\":{\"019e382d-e986-7b62-9f97-b015c5cc70f5\":{\"completed\":\"...\"}},\"timed_out\":false}"
  }
}
```

After `wait_agent` resolves, the same rollout also persists a `response_item`
`message` with `role == "user"` whose `input_text` wraps a
`<subagent_notification>` payload. It duplicates the per-agent status under
`agent_path` and `status`:

```json
{
  "type": "response_item",
  "payload": {
    "type": "message",
    "role": "user",
    "content": [
      {
        "type": "input_text",
        "text": "<subagent_notification>\n{\"agent_path\":\"019e382d-e986-7b62-9f97-b015c5cc70f5\",\"status\":{\"completed\":\"...\"}}\n</subagent_notification>"
      }
    ]
  }
}
```

This marker is informational only: the status it carries is the same as the
`wait_agent` `function_call_output`. The current parser ignores `response_item`
`message` items, so it is not double-counted.

The linked child rollout still uses structured session provenance:

```json
{
  "type": "session_meta",
  "payload": {
    "id": "019e382d-e986-7b62-9f97-b015c5cc70f5",
    "source": {
      "subagent": {
        "thread_spawn": {
          "parent_thread_id": "019e3829-1153-77d3-acc5-8d683325f21d",
          "depth": 1,
          "agent_nickname": "Nord",
          "agent_role": "product-manager"
        }
      }
    },
    "thread_source": "subagent",
    "agent_nickname": "Nord",
    "agent_role": "product-manager"
  }
}
```

Parser implication:

- `spawn_agent` `function_call_output` identifies a parent-side subagent via
  `output.agent_id` and optional `output.nickname`.
- `wait_agent` `function_call_output` carries terminal summaries in
  `output.status.{agent_id}` and enriches the matching parent-side `Subagent` rows.
- Sessions Chronicle maps response-item `spawn_agent` and `wait_agent` pairs
  into parent-side `Subagent` rows instead of generic tool calls.
- The trailing `<subagent_notification>` `response_item` `message` is ignored
  by the parser and carries no information beyond the `wait_agent` output.

### Encrypted Reasoning

```json
{
  "type": "response_item",
  "payload": {
    "type": "reasoning",
    "encrypted_content": "AAECAwQFBgcICQoL..."
  }
}
```

- Never decrypt locally
- Persist unchanged
- Forward to API to maintain context

### Multimodal Content

Two patterns:

1. **Inline Base64**: `data:image/png;base64,iVBORw0...`
2. **References**: HTTP(S) URLs or file identifiers

---

## Metadata Available in Events

| Field | Description |
|-------|-------------|
| `session_meta.payload.id` | Session/thread identifier |
| `session_meta.payload.cwd` | Working directory |
| `session_meta.payload.source` | Source provenance (`cli`, `vscode`, `exec`, structured subagent/custom variants, ...) |
| `session_meta.payload.model_provider` | Optional session-level provider id |
| `session_meta.payload.originator` | Recorder/runtime identifier |
| `session_meta.payload.cli_version` | CLI version that created the rollout |
| `turn_context.payload.model` | Active model slug for that turn |
| `event_msg` `session_configured` | Can provide `model` + `model_provider_id` |
| Skill invocation | Explicit `$skill-name` `user_message` plus injected `<skill>` payload |

**Model metadata:**

| Level | Field | Notes |
|-------|-------|-------|
| Per message | ❌ Not on `user_message`/`agent_message` payloads | |
| Per turn | ✅ `turn_context.payload.model` | Model slug, captured before sampling |
| Per session | ⚠️ `session_meta.payload.model_provider` | Optional, provider-only (no guaranteed model slug) |

---

## Token Usage

Codex rollouts can include `event_msg` entries with `payload.type == "token_count"`.
These provide **token usage totals for the session** and **the last model call**.

Example (abridged):

```json
{
  "type": "event_msg",
  "payload": {
    "type": "token_count",
    "info": {
      "total_token_usage": {
        "input_tokens": 14329,
        "cached_input_tokens": 10496,
        "output_tokens": 540,
        "reasoning_output_tokens": 477,
        "total_tokens": 14869
      },
      "last_token_usage": {
        "input_tokens": 15946,
        "cached_input_tokens": 14720,
        "output_tokens": 65,
        "reasoning_output_tokens": 13,
        "total_tokens": 16011
      },
      "model_context_window": 258400
    }
  }
}
```

Notes:

- `info.total_token_usage` is a running total for the current session/rollout file.
- `info.last_token_usage` is the usage for the most recent model call.
- `cached_input_tokens` is the cached subset of `input_tokens`, not an extra bucket to add on top.
- `reasoning_output_tokens` is exposed as a separate field in the payload and is normalized separately by
  Sessions Chronicle.
- Some `token_count` events can have `info: null` (treat as “unknown”, not zero).

See also: [Codex issue discussion around `token_count` logging](https://github.com/openai/codex/issues/5276).

---

## Parser Behavior (Sessions Chronicle)

Current implementation: `src/parsers/codex.rs`

- Indexes `event_msg.payload.type == user_message|agent_message`
- Indexes tool lifecycle pairs for `mcp_tool_call_begin|end` and `exec_command_begin|end`
- Indexes Codex child rollouts as subagent sessions when `session_meta.payload.source.sub_agent.thread_spawn.parent_thread_id` or `source.subagent.thread_spawn.parent_thread_id` is present
- Indexes `collab_agent_spawn_end` as parent-side `Subagent` rows and transcript items
- Enriches parent-side subagents from `collab_waiting_end`, `collab_close_end`, `collab_resume_end`, and `collab_agent_interaction_end`
- Ignores collab timing fields, spawned-agent `model`, and spawned-agent `reasoning_effort`
- Maps response-item `spawn_agent` / `wait_agent` `function_call` pairs into
  parent-side `Subagent` rows and terminal summaries instead of generic tool calls
- Leaves response-item `close_agent`, `send_message`, and `followup_task` calls
  as generic tool calls
- Does not yet extract Codex skill invocations from `$skill-name` / `<skill>` pairs

**Title extraction:** First `event_msg.payload.type == "user_message"` event (`payload.message`).

**Timestamp parsing:**

- `start_time`: from first-line `session_meta.payload.timestamp`
- `last_updated`: max `event.timestamp` seen in `event_msg` lines

**Content extraction:**

```rust
fn extract_content_codex_event_msg(event: &Value) -> Option<(Role, String)> {
    let payload = event.get("payload")?;
    match payload.get("type")?.as_str()? {
        "user_message" => Some((Role::User, payload.get("message")?.as_str()?.to_string())),
        "agent_message" => Some((Role::Assistant, payload.get("message")?.as_str()?.to_string())),
        _ => None,
    }
}
```

**Tool call handling:**

- Raw data is emitted via `event_msg.payload.type` variants:
  `exec_command_*`, `mcp_tool_call_*`, `web_search_*`, and collab `collab_*`;
  local rollouts can also emit tool calls as `response_item` `function_call`
  / `function_call_output`.
- Tool call correlation typically uses `call_id`.
- Current parser behavior: indexes `exec_command_*` and `mcp_tool_call_*`
  begin/end pairs plus `response_item` function calls as tool calls; maps Codex
  `collab_*` lifecycle events into parent `Subagent` rows plus child-session
  linkage when the child rollout is present.

**Streaming:** Use `BufReader` line-by-line iteration — do not load entire JSONL into memory.

---

## Primary Sources

- [Codex protocol `RolloutItem`, `SessionMeta`, `EventMsg`](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs)
- [Codex turn-context persistence](https://github.com/openai/codex/blob/main/codex-rs/core/src/codex.rs)
- [Codex rollout recorder](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/recorder.rs)
- [Codex app-server thread/item event model](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)
- [OpenAI Prompt Caching guide (`cached_tokens` is part of prompt/input usage)](https://developers.openai.com/api/docs/guides/prompt-caching)
