# Codex — Session Format Reference

Format reference for Codex rollout session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-assistant comparison tables.

---

## Storage & File Naming

| Field   | Value |
|---------|-------|
| **Path** | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| **Pattern** | `rollout-*.jsonl` |
| **Example** | `rollout-20250912-164103.jsonl` |
| **Format** | JSONL (one JSON object per line, UTF-8, append-only) |

**Date sharding:**

```
~/.codex/sessions/2025/09/12/rollout-20250912-164103.jsonl
                  └─────────┘          └──────────┘
                  Date sharding        Timestamp in filename
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
    "source": "cli"
  }
}
```

`source` supports subagent variants: `cli`, `vscode`, `subagent_review`, `subagent_compact`,
thread-spawn variants, etc.

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

```json
{
  "type": "event_msg",
  "payload": {
    "type": "collab_agent_spawn_begin",
    "call_id": "spawn_1",
    "sender_thread_id": "thr_parent",
    "prompt": "Investigate failing tests"
  }
}
{
  "type": "event_msg",
  "payload": {
    "type": "collab_agent_spawn_end",
    "call_id": "spawn_1",
    "sender_thread_id": "thr_parent",
    "new_thread_id": "thr_child",
    "status": "completed"
  }
}
```

Additional collab event types: `collab_waiting_*`, `collab_resume_*`, `collab_close_*`.

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
| `session_meta.payload.source` | Source provenance (`cli`, `vscode`, `subagent_*`, ...) |
| `session_meta.payload.model_provider` | Optional session-level provider id |
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
- Currently does not map `collab_*` events into subagent records
- Does not yet extract Codex skill invocations from `$skill-name` / `<skill>`
  pairs

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

- Raw data is emitted via `event_msg.payload.type` variants: `exec_command_*`, `mcp_tool_call_*`,
  `web_search_*`, and collab `collab_*`.
- Tool call correlation typically uses `call_id`.
- Current parser behavior: indexes `exec_command_*` and `mcp_tool_call_*` begin/end pairs as
  tool calls; `collab_*` events are not yet mapped to subagent records.

**Streaming:** Use `BufReader` line-by-line iteration — do not load entire JSONL into memory.

---

## Primary Sources

- [Codex protocol `RolloutItem`, `SessionMeta`, `EventMsg`](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs)
- [Codex turn-context persistence](https://github.com/openai/codex/blob/main/codex-rs/core/src/codex.rs)
- [Codex rollout recorder](https://github.com/openai/codex/blob/main/codex-rs/core/src/rollout/recorder.rs)
- [Codex app-server thread/item event model](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)
- [OpenAI Prompt Caching guide (`cached_tokens` is part of prompt/input usage)](https://developers.openai.com/api/docs/guides/prompt-caching)
