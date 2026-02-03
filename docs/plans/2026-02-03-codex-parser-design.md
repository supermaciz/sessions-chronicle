# Codex Parser Design

**Status**: Design validated
**Date**: 2026-02-03

Design document for implementing a Codex CLI session parser in Sessions Chronicle.

## Overview

The Codex parser will read JSONL session files from `~/.codex/sessions/` and extract sessions and messages compatible with the existing data model.

## Storage Structure

```
~/.codex/sessions/
└── YYYY/
    └── MM/
        └── DD/
            └── rollout-<datetime>-<uuid>.jsonl
```

**File naming pattern**: `rollout-YYYY-MM-DDTHH-MM-SS-<uuid>.jsonl`
- The UUID portion is the session ID
- Example: `rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl`

## JSONL Event Structure

Each line is a JSON object with:
- `timestamp`: ISO-8601 string
- `type`: event type (see below)
- `payload`: event-specific data

### Event Types

| Type | Purpose | Frequency |
|------|---------|-----------|
| `session_meta` | Session metadata | 1 per file (first line) |
| `response_item` | Messages, tool calls, reasoning | Many |
| `event_msg` | User input, agent output, tokens | Many |
| `turn_context` | Per-turn context | Per turn |

### session_meta Payload

```json
{
  "id": "019bce9f-0a40-79e2-8351-8818e8487fb6",
  "timestamp": "2026-01-18T01:01:28.123Z",
  "cwd": "/home/user/project",
  "originator": "codex_cli_rs",
  "cli_version": "0.87.0",
  "instructions": "...",
  "source": "cli",
  "model_provider": "openai",
  "git": {
    "commit_hash": "abc123...",
    "branch": "main",
    "repository_url": "git@github.com:user/repo.git"
  }
}
```

### response_item Payload Types

| payload.type | Description | Key Fields |
|--------------|-------------|------------|
| `message` | Chat messages | `role` (user/assistant), `content[]` |
| `function_call` | Tool invocation | `name`, `call_id`, `arguments` |
| `function_call_output` | Tool result | `call_id`, `output` |
| `custom_tool_call` | Custom tools (apply_patch) | `name`, `call_id` |
| `custom_tool_call_output` | Custom tool result | `call_id`, `output` |
| `reasoning` | Model reasoning | `encrypted_content` or `text` |
| `ghost_snapshot` | Git state snapshot | `ghost_commit` |

### event_msg Payload Types

| payload.type | Description | Key Fields |
|--------------|-------------|------------|
| `user_message` | Real user input | `message` |
| `agent_message` | Agent text response | `message` |
| `agent_reasoning` | Visible reasoning | - |
| `token_count` | Token usage | `rate_limits` |

### turn_context Payload

```json
{
  "cwd": "/home/user/project",
  "model": "o4-mini",
  "approval_policy": "on-request",
  "sandbox_policy": "workspace-write",
  "summary": "..."
}
```

## Mapping to Sessions Chronicle Model

### Session Extraction

From `session_meta` event:

| Session Field | Codex Source |
|---------------|--------------|
| `id` | `payload.id` |
| `tool` | `Tool::Codex` |
| `project_path` | `payload.cwd` |
| `start_time` | `payload.timestamp` |
| `file_path` | JSONL file path |
| `last_updated` | Last event timestamp |
| `message_count` | Count of user/assistant messages |

### Message Extraction

**Decision**: Extract messages ONLY from `event_msg` events (not `response_item`).

Rationale: `response_item` with `type: message` often contains injected system context (AGENTS.md, environment_context) rather than actual user prompts. The `event_msg` events contain the real conversational exchanges.

| Event Type | Payload Type | Maps To |
|------------|--------------|---------|
| `event_msg` | `user_message` | Role::User |
| `event_msg` | `agent_message` | Role::Assistant |

Ignored event types:
- `response_item` (system context, tool calls, reasoning)
- `turn_context` (used only for title extraction)
- `token_count` (usage metrics)

| Message Field | Codex Source |
|---------------|--------------|
| `session_id` | From session |
| `index` | Sequential order |
| `role` | Derived from payload type |
| `content` | `payload.message` |
| `timestamp` | Event `timestamp` |

### Title Extraction

Priority order:
1. `turn_context.summary` (if present and non-empty)
2. First `event_msg` where `payload.type == "user_message"` → use `payload.message`
3. Truncate to reasonable length (100 chars)

## Implementation Plan

### 1. Add Tool::Codex Variant

In `src/models/session.rs`, the `Tool` enum already has `Codex` - verify it exists.

### 2. Create Parser Module

New file: `src/parsers/codex.rs`

```rust
pub struct CodexParser;

impl CodexParser {
    pub fn parse(&self, file_path: &Path) -> Result<(Session, Vec<Message>)> {
        // Stream JSONL line by line
        // Extract session_meta for Session
        // Extract user_message/agent_message for Messages
        // Return tuple
    }
}
```

### 3. Implement Streaming Parser

Follow the same pattern as `claude_code.rs`:
- Use `BufReader::lines()` for memory efficiency
- Parse each line as JSON
- Handle malformed lines gracefully (skip with warning)
- Require at least one user message

**Parsing steps**:
1. Read first line → expect `session_meta` → extract ID, cwd, timestamp, cli_version
2. Stream remaining lines, for each:
   - If `event_msg` with `payload.type == "user_message"` → collect as Role::User
   - If `event_msg` with `payload.type == "agent_message"` → collect as Role::Assistant
   - If `turn_context` with `payload.summary` → store as title candidate
   - Skip all other event types
3. Title: use `turn_context.summary` if found, else first `user_message` (truncated)
4. Reject session if no `user_message` found

### 4. Add Indexer Support

In `src/database/indexer.rs`:
- Add `index_codex_sessions()` method
- Walk `~/.codex/sessions/` recursively
- Parse each `rollout-*.jsonl` file
- Insert into database

### 5. Wire Up in App

- Call indexer on startup
- Sessions should appear with Tool::Codex filter

## Edge Cases

### Encrypted Reasoning

`response_item` events with `type: reasoning` may have `encrypted_content` instead of readable text. The parser should:
- Skip these for message extraction
- Never attempt to decrypt
- Log a debug message

### Empty Sessions

Sessions with no `user_message` events should be skipped (consistent with other parsers).

### Large Sessions

The streaming approach handles sessions with thousands of events without memory issues.

### Tool Calls

For Phase 1, we extract only user/assistant messages. Tool calls (`function_call`, `function_call_output`) are skipped. Future phases may include tool call display.

## Testing

### Unit Tests

1. Parse valid session file
2. Handle missing session_meta
3. Handle malformed JSON lines
4. Extract correct message order
5. Handle encrypted reasoning
6. Handle empty sessions

### Test Fixtures

Copy sanitized session samples to `tests/fixtures/codex_sessions/`:
- `valid_session.jsonl` - normal session
- `empty_session.jsonl` - session_meta only
- `malformed.jsonl` - some invalid JSON lines

## Resume Command

For the session detail view, the resume command is:
```
codex resume <session-id>
```

The session ID is the UUID portion of the filename (after the timestamp).

## Sources

- [Codex CLI Reference](https://developers.openai.com/codex/cli/reference/)
- [Non-interactive Mode](https://developers.openai.com/codex/noninteractive/)
- [CodexMonitor](https://github.com/Cocoanetics/CodexMonitor)
- Local session file analysis (versions 0.58.0 - 0.87.0)
