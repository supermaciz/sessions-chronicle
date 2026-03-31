# Claude Code — Session Format Reference

Format reference for Claude Code session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-assistant comparison tables.

---

## Storage & File Naming

| Field   | Value |
|---------|-------|
| **Path** | `~/.claude/projects/<project-dir>/UUID.jsonl` (main session)<br>`~/.claude/projects/<project-dir>/<session-id>/subagents/agent-<id>.jsonl` (subagent transcript; documented upstream and confirmed locally)<br>`~/.claude/projects/<project-dir>/<session-id>/tool-results/<id>.<ext>` (materialized large tool output or attachment payloads; observed in current local sessions) |
| **Pattern** | `UUID.jsonl`, `agent-*.jsonl`, `tool-results/*` |
| **Example** | `a1b2c3d4-e5f6-7890-abcd-ef1234567890.jsonl`<br>`2a19bf71-3687-49ed-8ae9-8bd15e1522f6/subagents/agent-a60d695.jsonl`<br>`82b2d04e-d30e-4370-8e41-f53890baeda1/tool-results/bdw7vxszs.txt` |
| **Format** | JSONL (one JSON object per line, UTF-8, append-only) |

**Path encoding:**

```
~/.claude/projects/-Users-alexm-Repository-myproject/UUID.jsonl
~/.claude/projects/-Users-alexm-Repository-myproject/<session-id>/subagents/agent-a29fd7d.jsonl
                    └──────────────────────────────┘
                           Project path encoding
```

---

## Event Types

```json
{
  "type": "summary",               // Session title
  "type": "user",                  // User messages
  "type": "assistant",             // Assistant messages
  "type": "system",                // System events (for example: local_command, turn_duration, compact_boundary)
  "type": "file-history-snapshot", // File state tracking
  "type": "progress",              // Streaming/progress events
  "type": "queue-operation",       // Queue orchestration events
  "type": "saved_hook_context",    // Hook context snapshots
  "type": "pr-link"                // PR link events
}
```

### User Message Example

```json
{
  "type": "user",
  "message": {
    "role": "user",
    "content": "Help me fix this bug"
  },
  "timestamp": "2025-10-02T20:15:32.885Z",
  "cwd": "/path/to/project",
  "sessionId": "UUID",
  "version": "2.0.5",
  "gitBranch": "main",
  "uuid": "UUID",
  "parentUuid": null,
  "isSidechain": false,
  "userType": "external",
  "isMeta": true
}
```

### Assistant Tool-Use Content Block (in `message.content[]`)

```json
{
  "type": "assistant",
  "message": {
    "role": "assistant",
    "model": "claude-sonnet-4-6",
    "content": [
      {
        "type": "tool_use",
        "id": "toolu_01D7FLrfh4GYq7yT1ULFeyMV",
        "name": "Bash",
        "input": {
          "command": "ls -la"
        },
        "caller": {
          "type": "direct"
        }
      }
    ]
  }
}
```

### Assistant Subagent Launch Block (observed in current local sessions)

```json
{
  "type": "assistant",
  "message": {
    "role": "assistant",
    "model": "claude-sonnet-4-6",
    "content": [
      {
        "type": "tool_use",
        "id": "toolu_01SEhtw5pj2qvZXjTufUZSyi",
        "name": "Agent",
        "input": {
          "description": "Explore project docs and current state",
          "subagent_type": "Explore",
          "prompt": "..."
        },
        "caller": {
          "type": "direct"
        }
      }
    ]
  }
}
```

### Tool Execution System Event (commonly observed)

```json
{
  "type": "system",
  "subtype": "local_command",
  "command": ["ls", "-la"],
  "stdout": "...",
  "timestamp": "2025-01-10T10:30:10.000Z"
}
```

### Turn Duration System Event (observed in current main-session logs)

```json
{
  "type": "system",
  "subtype": "turn_duration",
  "durationMs": 186669,
  "messageCount": 34,
  "timestamp": "2026-03-30T22:50:06.144Z"
}
```

### Compact Boundary System Event (observed in recent logs)

```json
{
  "type": "system",
  "subtype": "compact_boundary",
  "content": "Conversation compacted",
  "compactMetadata": {
    "trigger": "auto",
    "preTokens": 167363,
    "preCompactDiscoveredTools": ["AskUserQuestion", "TaskCreate", "TaskList", "TaskUpdate"]
  },
  "logicalParentUuid": "UUID"
}
```

---

## Special Features

### Summary Events

```json
{
  "type": "summary",
  "summary": "Session title text",
  "leafUuid": "UUID"
}
```

### File History Snapshots

```json
{
  "type": "file-history-snapshot",
  "messageId": "UUID",
  "snapshot": {
    "trackedFileBackups": {},
    "timestamp": "ISO-8601"
  }
}
```

### Tool Results

- Tool results are still represented inline in user `message.content[]` via `type: "tool_result"` blocks.
- Recent local sessions also show a top-level `toolUseResult` object on some user events, duplicating stdout/stderr in structured form.
- Large tool outputs can be materialized to sibling files under `tool-results/` and then referenced by later `Read` tool calls.

### Meta Flag

- `isMeta: true` → Skip for title extraction (system-generated)
- `isMeta: false` → User-generated content

### Threading

Tree structure via `uuid`/`parentUuid` + `isSidechain` flag.
Sidechain/subagent context appears through `isSidechain` and parent links (`parentUuid`).
Some recent `system/compact_boundary` events also include `logicalParentUuid`.

---

## Metadata Available in Events

Rich per-event metadata:

| Field | Description |
|-------|-------------|
| `sessionId` | Unique session identifier |
| `cwd` | Working directory |
| `gitBranch` | Git branch name |
| `version` | Claude Code version |
| `promptId` | Prompt/turn identifier on user events |
| `message.model` | Assistant model slug (`claude-opus-4-6`, `claude-sonnet-4-5-20250929`, ...) |
| `userType` | `"external"` or other |
| `entrypoint` | Session entrypoint (`"cli"` observed locally) |
| `slug` | Human-friendly session slug present on many current events |
| `uuid` / `parentUuid` | Tree structure links |
| `logicalParentUuid` | Additional lineage link observed on compacted system events |
| `isSidechain` | Subagent/sidechain indicator |
| `agentId` | Present in subagent transcript events |

**Model metadata:** Recent Claude Code logs include a stable structured field at
`message.model` on `assistant` events.

- Local sample (2026-02-24): `message.model` present on all sampled `assistant` events,
  absent on sampled `user` events.
- Local sample refresh (2026-03-31): `message.model` still present on sampled `assistant`
  events in v2.1.87 main-session and subagent logs.
- Observed values in local sessions:
  - `claude-opus-4-6`
  - `claude-sonnet-4-6`
  - `claude-opus-4-5-20251101`
  - `claude-sonnet-4-5-20250929`
  - `claude-haiku-4-5-20251001`
  - `<synthetic>` (sentinel used for locally generated assistant error/limit messages)

### Supported Claude Code Model Slugs

Official Claude Code model configuration currently documents:

| Product name | Slug |
|-------------|------|
| Sonnet 4.6 | `claude-sonnet-4-6` |
| Opus 4.6 | `claude-opus-4-6` |
| Opus 4.5 | `claude-opus-4-5-20251101` |
| Haiku 4.5 | `claude-haiku-4-5-20251001` |
| Sonnet 4.5 | `claude-sonnet-4-5-20250929` |

---

## Token Usage

Recent Claude Code session logs can include **per-assistant-message token usage** under
`message.usage` (not guaranteed in all historical logs/fixtures).

Observed shape (subset):

```json
{
  "type": "assistant",
  "message": {
    "usage": {
      "input_tokens": 123,
      "output_tokens": 456,
      "cache_read_input_tokens": 789,
      "cache_creation_input_tokens": 0
    }
  }
}
```

Notes:

- `usage` is typically present on `type == "assistant"` events and absent on `type == "user"` events.
- Anthropic-style cache accounting keeps `cache_read_input_tokens` and `cache_creation_input_tokens`
  separate from `input_tokens`, so `input_tokens` represents the uncached input portion.
- Recent local sessions also include richer nested usage metadata such as `cache_creation`,
  `server_tool_use`, `service_tier`, `inference_geo`, `iterations`, and `speed`.
- The log is append-only and can contain multiple assistant events for the same underlying request; if you
  aggregate tokens, deduplicate by a stable identifier such as `requestId` + `message.id` and keep the last
  (or max) `usage` record per request.

---

## Parser Behavior (Sessions Chronicle)

Current implementation: `src/parsers/claude_code.rs`

- Indexes `type == user|assistant` text content
- Extracts `tool_use` blocks as tool calls (current implementation maps `Task` tool uses to subagent records)
- Correlates `tool_result` blocks by `tool_use_id`
- Does not currently normalize `system/local_command` events into tool calls
- Does not currently persist/index `message.model` in Sessions Chronicle database schema
- Recent real Claude Code sessions (v2.1.87) show subagent launches as `name == "Agent"`
  with `input.subagent_type`, so the local parser assumption is now stale relative to current logs.

**Title extraction:** First parsed `user` message content (assistant/system/summary are ignored).

**Timestamp parsing:** Track earliest/latest across `type in {user, assistant}` using
per-event `timestamp` (ISO-8601).

**Content extraction:**

```rust
fn extract_content_claude(event: &Value) -> Option<String> {
    // supports both plain string and block arrays
    // array blocks currently include "text" and "thinking"
    ClaudeCodeParser::extract_content(event.get("message")?.get("content")?)
}
```

**Tool call handling:**

- Tool invocations appear in assistant `message.content[]` as `{type: "tool_use", id, name, input}`.
- Current local samples use PascalCase tool names such as `Read`, `Edit`, `Bash`, and `Agent`.
- Tool execution output is commonly observable in `system` events (`subtype: "local_command"`)
  with `command`, `stdout`, `stderr` fields.
- Tool result payloads may also be duplicated in a top-level `toolUseResult` object on the user event.
- Current parser behavior: indexes `tool_use` blocks and correlates `tool_result` blocks by
  `tool_use_id`; `system/local_command` payloads are not yet normalized as tool calls.

**Streaming:** Use `BufReader` line-by-line iteration — do not load entire JSONL into memory.

---

## References

- [Anthropic prompt caching docs (`cache_read_input_tokens`, `cache_creation_input_tokens`, `input_tokens`)](https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching)
- [Claude Code subagents docs (subagent transcript path)](https://docs.anthropic.com/en/docs/claude-code/sub-agents)
- [Claude Code changelog (compaction/transcript behavior changes)](https://docs.anthropic.com/en/docs/claude-code/changelog)
- [Claude Code model configuration (support article)](https://support.claude.com/en/articles/11940350-claude-code-model-configuration)
- [Claude Sonnet 4.6 page (`claude-sonnet-4-6`)](https://www.anthropic.com/claude/sonnet)
- [Claude Opus 4.6 page (`claude-opus-4-6`)](https://www.anthropic.com/claude/opus)
- [Claude Code issue: duplicate entries in session logs](https://github.com/anthropics/claude-code/issues/1524)
