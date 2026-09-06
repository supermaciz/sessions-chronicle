# Codex — Session Format Reference

Format reference for Codex rollout session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-assistant comparison tables.

**Last checked: 2026-09-06.** Persistence policy checked at `rust-v0.153.4`;
protocol, response-item types, recorder, and compression inspected at upstream
commit `ac192cd7937b0d73edc6dffe009940ae53782dd4`. Version introduction dates and
the prevalence of paginated history in real sessions were not established.

---

## Storage & File Naming

| Field   | Value |
|---------|-------|
| **Active path** | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| **Archived path** | `~/.codex/archived_sessions/rollout-*.jsonl[.zst]` |
| **Patterns** | `rollout-*.jsonl`, `rollout-*.jsonl.zst` |
| **Example** | `rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl` |
| **Format** | Line-oriented JSON; active rollouts are append-only JSONL, while cold archived rollouts can be Zstandard-compressed JSONL |

**Date sharding:**

```
~/.codex/sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl
                  └─────────┘          └─────────────────────────────────────────────────────────┘
                  Date sharding        Timestamp + thread id in filename
```

Archived sessions use a flat directory rather than date sharding. Current
upstream Codex can compress cold archived rollouts from `rollout-*.jsonl` to
`rollout-*.jsonl.zst` and transparently read either representation.

Sessions Chronicle discovers `rollout-*.jsonl` and `rollout-*.jsonl.zst` in the
configured session directory. When that directory is named `sessions` or
`codex_sessions`, discovery also includes a sibling `archived_sessions/`, even
if the active directory is absent. Compressed files are decoded as a stream.

The inspected recorder also supports `rollout-<timestamp>-<thread_id>_<rollout_id>.jsonl`
after `thread/revert`. A filename's rollout identity must not be assumed to
equal the stable thread ID in `session_meta.payload.id`.

---

## Event Structure

Codex rollout logs are envelope-based JSONL entries (`RolloutLine`). The following
is a schematic example of the historically supported envelope types, not an
exhaustive current enum:

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
`forked_from_id`, direct `parent_thread_id`, `thread_source`, `agent_nickname`,
`agent_role`, `agent_path`, `base_instructions`, `dynamic_tools`,
`memory_mode`, and `multi_agent_version`.

For spawned child sessions, current rollouts can carry the parent identifier in
both `session_meta.payload.parent_thread_id` and the structured
`source.subagent.thread_spawn.parent_thread_id`. Sessions Chronicle currently
uses the structured `source` value first, then direct `parent_thread_id` as a
child-session linkage fallback.

### History Modes and Shared Prefixes

**Confirmed upstream:** `SessionMeta.history_mode` distinguishes `legacy` and
`paginated`; deserialization defaults to `legacy` when the field is absent.
In the [0.153.4 persistence policy](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/rollout/src/policy.rs),
paginated history persists `event_msg.payload.type == "item_completed"` carrying
typed `TurnItem` values. Legacy `user_message` and `agent_message` events are
persisted only in `legacy` mode. Some `item_completed` events are also retained
in legacy mode, including function-call outputs, plans, and completed subagent
activities; the event alone does not identify the history mode.

**Confirmed local gap:** the parser ignores `item_completed` and response-item
messages. A rollout without a legacy `user_message` is rejected with
`NoUserMessages`, even if it contains user messages as completed items. This is
a code-inspection finding; no paginated reproduction was run during this watch.

**Confirmed metadata contract:** the inspected
[protocol types](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/protocol/src/protocol.rs)
define optional `history_base` as a prefix reference with these fields:

| Field | Meaning in upstream types |
|-------|---------------------------|
| `thread_id` | Referenced rollout ID, despite the historical field name; not necessarily its stable thread ID |
| `end_ordinal_exclusive` | First rollout ordinal excluded from the inherited prefix |
| `end_byte_offset` | Byte offset immediately after the last included JSONL record |

Related optional metadata includes `forked_from_ordinal_exclusive` (the logical
fork boundary, independent of the physical `history_base`) and
`subagent_history_start_ordinal` (the first record belonging to the child's own
projected history; earlier records are inherited model context). Current types
also distinguish root `session_id` from thread `id`.

**Likely impact:** Sessions Chronicle reads each file independently and ignores
these history fields, so an inherited prefix may be missing from its transcript.

**Unknown / not yet inspected:** upstream prefix resolution and reconstruction,
chained references, missing-prefix handling, compressed-prefix offset semantics,
and interaction between revert and child-history projection. The field contract
is sufficient to identify a gap, but is not an implementation specification.

### User / Assistant Events (`event_msg`)

The examples below describe legacy message events. See history modes above for
the paginated representation.

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

Historical/protocol shape supported by the parser. The inspected 0.153.4
persistence policy does not persist `session_configured`; use `turn_context`
and `session_meta` for durable model/provider metadata.

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

These are historical/protocol event shapes. In the inspected 0.153.4 policy,
`exec_command_begin`, `exec_command_end`, `mcp_tool_call_begin`, and
`web_search_begin` are transient; `mcp_tool_call_end` and `web_search_end` are
retained only in legacy mode. Protocol variants are not a persistence guarantee.

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

**Persistence caveat (confirmed at 0.153.4):** the `collab_*` events listed below
are transient and are not written by the inspected persistence policy. These
examples remain relevant to older files and parser compatibility tests.
The policy instead retains `sub_agent_activity` events for non-completed
activities in legacy mode and completed subagent activities through
`item_completed`. Sessions Chronicle does not handle either activity carrier.

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
  events. The current parser uses it for subagent summary enrichment, with
  priority over earlier waiting and close updates.
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

- `spawn_agent` `function_call` creates an unlinked parent-side subagent from
  its arguments. The matching `function_call_output` enriches it via
  `output.agent_id` and optional `output.nickname`. When the output is missing,
  unparseable, or omits `agent_id` (a rejected spawn), the parser keeps the
  unlinked `Subagent` row rather than dropping the spawn from the transcript.
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

Current implementation: [`crates/core/src/parsers/codex.rs`](../../crates/core/src/parsers/codex.rs)

- Indexes `event_msg.payload.type == user_message|agent_message`
- Indexes tool lifecycle pairs for `mcp_tool_call_begin|end` and `exec_command_begin|end`
- Indexes Codex child rollouts as subagent sessions when `session_meta.payload.source.sub_agent.thread_spawn.parent_thread_id` or `source.subagent.thread_spawn.parent_thread_id` is present
- Uses direct `session_meta.payload.parent_thread_id` as a child-session linkage fallback
- Discovers plain and compressed rollouts, including sibling archives under the directory-name rules above
- Does not handle `item_completed`, `sub_agent_activity`, or shared-prefix reconstruction via `history_base`
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
- `last_updated`: maximum valid top-level event timestamp across all subsequent envelope types, initialized from the session start time

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

- Historical/protocol data uses `event_msg.payload.type` variants:
  `exec_command_*`, `mcp_tool_call_*`, `web_search_*`, and collab `collab_*`;
  local rollouts can also emit tool calls as `response_item` `function_call`
  / `function_call_output`. The current persistence policy above determines
  which protocol events actually reach disk.
- Tool call correlation typically uses `call_id`.
- Current parser behavior: indexes `exec_command_*` and `mcp_tool_call_*`
  begin/end pairs plus `response_item` function calls as tool calls; maps Codex
  `collab_*` lifecycle events into parent `Subagent` rows plus child-session
  linkage when the child rollout is present.

**Streaming:** Use `BufReader` line-by-line iteration for plain JSONL and a
streaming Zstandard decoder for `*.jsonl.zst` — do not load entire rollout logs
into memory.

---

## Primary Sources

- [Codex persistence policy, verified at 0.153.4](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/rollout/src/policy.rs)
- [Codex protocol `SessionMeta`, `HistoryPosition`, `EventMsg`, inspected commit](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/protocol/src/protocol.rs)
- [Codex response-item types, inspected commit](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/protocol/src/models.rs)
- [Codex recorder, including revert filename support, inspected commit](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/rollout/src/recorder.rs)
- [Codex turn-context persistence](https://github.com/openai/codex/blob/main/codex-rs/core/src/codex.rs)
- [Codex rollout recorder](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/recorder.rs)
- [Codex rollout compression and compressed-file discovery](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/compression.rs)
- [Codex app-server thread/item event model](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)
- [OpenAI Prompt Caching guide (`cached_tokens` is part of prompt/input usage)](https://developers.openai.com/api/docs/guides/prompt-caching)
