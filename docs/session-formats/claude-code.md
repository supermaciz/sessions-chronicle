# Claude Code — Session Format Reference

Format reference for Claude Code session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-tool comparison tables.

---

## Storage & File Naming

| Field   | Value |
|---------|-------|
| **Path** | `~/.claude/projects/<project-dir>/UUID.jsonl` (main session)<br>`~/.claude/projects/<project-dir>/<session-id>/subagents/agent-<id>.jsonl` (subagent transcript; commonly observed in newer logs) |
| **Pattern** | `UUID.jsonl`, `agent-*.jsonl` |
| **Example** | `a1b2c3d4-e5f6-7890-abcd-ef1234567890.jsonl`<br>`2a19bf71-3687-49ed-8ae9-8bd15e1522f6/subagents/agent-a60d695.jsonl` |
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
  "type": "system",                // System events (subtype: local_command)
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
        "name": "bash",
        "input": {
          "command": "ls -la"
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

### Meta Flag

- `isMeta: true` → Skip for title extraction (system-generated)
- `isMeta: false` → User-generated content

### Threading

Tree structure via `uuid`/`parentUuid` + `isSidechain` flag.
Sidechain/subagent context appears through `isSidechain` and parent links (`parentUuid`).

---

## Metadata Available in Events

Rich per-event metadata:

| Field | Description |
|-------|-------------|
| `sessionId` | Unique session identifier |
| `cwd` | Working directory |
| `gitBranch` | Git branch name |
| `version` | Claude Code version |
| `message.model` | Assistant model slug (`claude-opus-4-6`, `claude-sonnet-4-5-20250929`, ...) |
| `userType` | `"external"` or other |
| `uuid` / `parentUuid` | Tree structure links |
| `isSidechain` | Subagent/sidechain indicator |

**Model metadata:** Recent Claude Code logs include a stable structured field at
`message.model` on `assistant` events.

- Local sample (2026-02-24): `message.model` present on all sampled `assistant` events,
  absent on sampled `user` events.
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
- The log is append-only and can contain multiple assistant events for the same underlying request; if you
  aggregate tokens, deduplicate by a stable identifier such as `requestId` + `message.id` and keep the last
  (or max) `usage` record per request.

---

## Parser Behavior (Sessions Chronicle)

Current implementation: `src/parsers/claude_code.rs`

- Indexes `type == user|assistant` text content
- Extracts `tool_use` blocks as tool calls (and maps `Task` tool uses to subagent records)
- Correlates `tool_result` blocks by `tool_use_id`
- Does not currently normalize `system/local_command` events into tool calls
- Does not currently persist/index `message.model` in Sessions Chronicle database schema

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
- Tool execution output is commonly observable in `system` events (`subtype: "local_command"`)
  with `command`, `stdout`, `stderr` fields.
- Current parser behavior: indexes `tool_use` blocks and correlates `tool_result` blocks by
  `tool_use_id`; `system/local_command` payloads are not yet normalized as tool calls.

**Streaming:** Use `BufReader` line-by-line iteration — do not load entire JSONL into memory.

---

## References

- [Claude Code model configuration (support article)](https://support.claude.com/en/articles/11940350-claude-code-model-configuration)
- [Claude Sonnet 4.6 page (`claude-sonnet-4-6`)](https://www.anthropic.com/claude/sonnet)
- [Claude Opus 4.6 page (`claude-opus-4-6`)](https://www.anthropic.com/claude/opus)
- [Claude Code issue: duplicate entries in session logs](https://github.com/anthropics/claude-code/issues/1524)
