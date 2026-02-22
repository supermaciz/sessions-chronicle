# Claude Code — Session Format Reference

Format reference for Claude Code session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-tool comparison tables.

---

## Storage & File Naming

| Field   | Value |
|---------|-------|
| **Path** | `~/.claude/projects/<project-dir>/UUID.jsonl` |
| **Pattern** | `UUID.jsonl` |
| **Example** | `a1b2c3d4-e5f6-7890-abcd-ef1234567890.jsonl` |
| **Format** | JSONL (one JSON object per line, UTF-8, append-only) |

**Path encoding:**

```
~/.claude/projects/-Users-alexm-Repository-myproject/UUID.jsonl
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
  "type": "file-history-snapshot"  // File state tracking
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
| `userType` | `"external"` or other |
| `uuid` / `parentUuid` | Tree structure links |
| `isSidechain` | Subagent/sidechain indicator |

**Model metadata:** No stable structured model field observed in sampled events.
Session/events include `version` but not a canonical model id.
Model switches may appear as free-text command/system content (e.g. `/model`)
rather than a normalized field.

---

## Parser Behavior (Sessions Chronicle)

Current implementation: `src/parsers/claude_code.rs`

- Indexes `type == user|assistant` text content
- Extracts `tool_use` blocks as tool calls (and maps `Task` tool uses to subagent records)
- Correlates `tool_result` blocks by `tool_use_id`
- Does not currently normalize `system/local_command` events into tool calls

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
