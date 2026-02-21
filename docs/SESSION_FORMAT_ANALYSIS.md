# Session Format Analysis

Analysis of Claude Code, Codex, OpenCode, and Mistral Vibe session file formats for Sessions Chronicle parser design.

---

## Implementation Status

- ✅ Claude Code parser + indexer implemented
- ✅ Session date/sort semantics aligned with agent-sessions (Claude: end time = latest message-like event)
- ✅ OpenCode parser implemented
- ✅ Codex parser implemented
- ✅ Mistral Vibe parser implemented
- ✅ OpenCode subagent session detection implemented (`parentID` sessions are skipped during indexing)
- ✅ Tool-call wire formats documented for Claude, OpenCode, Mistral Vibe, and Codex rollouts
- ✅ LLM model metadata availability mapped (per message vs per turn vs per session)
- ℹ️ Current parser behavior: tool-call/tool-result content is intentionally not indexed yet (Phase 4)

---

## Storage Locations

| Tool | Path | Organization |
|------|------|--------------|
| **Claude Code** | `~/.claude/` | Project-specific directories<br>`~/.claude/projects/-Users-alexm-Repository-<project>/UUID.jsonl` |
| **Codex** | `~/.codex/sessions/` | Date-sharded directories<br>`YYYY/MM/DD/rollout-*.jsonl` |
| **OpenCode** | `~/.local/share/opencode/storage/` | Multi-directory structure:<br>`session/<project>/ses_xxx.json` (metadata)<br>`message/ses_xxx/` (messages)<br>`part/msg_xxx/` (message parts)<br>`session_diff/ses_xxx.json` (file changes) |
| **Mistral Vibe** | `~/.vibe/logs/session/` | One directory per session:<br>`session_YYYYMMDD_HHMMSS_<shortid>/`<br>Contains `meta.json` + `messages.jsonl`.<br>Default can be overridden via `VIBE_HOME` or `session_logging.save_dir` in `config.toml`. |

---

## File Format

**Claude Code & Codex** use **JSONL** (JSON Lines):
- One JSON object per line
- UTF-8 encoded
- Append-only chronological events

**OpenCode** uses **separate JSON files**:
- One JSON file per session (session metadata)
- Separate directories for messages and parts
- Standard JSON format (not line-delimited)

**Mistral Vibe** uses a **directory-based format**:
- `meta.json` contains session-level metadata (standard JSON)
- `messages.jsonl` is JSONL (one message per line)
- Messages are OpenAI-style (`role`, `content`, optional `tool_calls`)

---

## File Naming

| Tool | Pattern | Example |
|------|---------|---------|
| **Claude Code** | `UUID.jsonl` | `a1b2c3d4-e5f6-7890-abcd-ef1234567890.jsonl` |
| **Codex** | `rollout-*.jsonl` | `rollout-20250912-164103.jsonl` |
| **OpenCode** | `ses_*.json` | `ses_66a71b6f4ffeq796jvvOpJQ04m.json` |
| **Mistral Vibe** | `session_YYYYMMDD_HHMMSS_<shortid>/` | `session_20260123_174305_64883c86/` |

---

## Event Structure Comparison

### Common Fields

| Field Category | Claude Code | Codex | OpenCode | Mistral Vibe |
|----------------|-------------|-------|----------|-------------|
| **Event Type** | `type` (`user`, `assistant`, `system`, `summary`, ...) | Rollout envelope `type` (`session_meta`, `event_msg`, `response_item`, `turn_context`, ...); nested `event_msg.payload.type` (`user_message`, `agent_message`, `exec_command_*`, `mcp_tool_call_*`, `collab_*`, ...) | Session metadata only (messages in separate files) | `role` (`system`, `user`, `assistant`, `tool`) in `messages.jsonl`; tool calls on assistant messages via `tool_calls` |
| **Identity** | `uuid`, `parentUuid` (tree structure) | Session id at `session_meta.payload.id`; event-specific IDs like `call_id`, `sender_thread_id`, `receiver_thread_id` | `id`, `parentID` (hierarchical sessions) | No message IDs; tool calls have an `id` and tool responses reference it via `tool_call_id` |
| **Timestamp** | `timestamp` (ISO-8601) | Top-level rollout-line `timestamp` (ISO-8601 string) | `time.created`, `time.updated` (session level) | Session-level only in `meta.json`: `start_time`, `end_time` (ISO-8601). No per-message timestamps |
| **Content** | Nested: `message.content` | Usually in `event_msg.payload` (for example `message`, command output deltas, MCP results), plus optional `response_item.payload.content[]` blocks | Stored in `message/ses_xxx/` directory + `part/msg_xxx/` | `messages.jsonl` lines with `content`; tool output stored as `role: "tool"` messages |
| **Model Metadata** | No stable structured model field observed in sampled events (session/event includes `version` but not a canonical model id) | `session_meta.payload.model_provider` (optional provider, session-level) + `turn_context.payload.model` (model slug, per turn); `event_msg.payload.type == "session_configured"` can also carry `model` + `model_provider_id` | Per-message model fields: `user.model.{providerID,modelID}` and assistant `providerID` + `modelID`; `subtask` parts can optionally include delegated model | No model field in `messages.jsonl` records; session-level `meta.json` can include a full `config` snapshot (`active_model`, `providers`, `models`) when logging is enabled |

### Key Architectural Differences

**Threading Model:**
- **Claude Code**: Tree structure via `uuid`/`parentUuid` + `isSidechain` flag
- **Codex**: Thread-based rollouts (`session_meta.payload.id` thread id); optional subagent provenance via `session_meta.payload.source == "subagent_*"` and collab events (`collab_agent_spawn_*`, `collab_resume_*`, ...)
- **OpenCode**: Parent-child sessions via `parentID` (subagent sessions)
- **Mistral Vibe**: Linear message list in `messages.jsonl`; tool calls are embedded in assistant messages and resolved by subsequent `tool` role messages

**Metadata Storage:**
- **Claude Code**: Rich per-event metadata (`cwd`, `gitBranch`, `version`, `sessionId`)
- **Codex**: Session metadata (`session_meta`) can include provider (`model_provider`), and turn-level metadata (`turn_context`) includes active model slug (`model`)
- **OpenCode**: Session-level metadata (`projectID`, `directory`, `version`, `title`)
- **Mistral Vibe**: Session-level `meta.json` includes environment, optional git info, token/tool usage stats, tools snapshot, and configuration snapshot data

**Content Access:**
- **Claude Code**: `event.message.content` (nested in JSONL events)
- **Codex**: `event_msg.payload.message` for user/assistant text; tool/collab info in event-specific payload fields
- **OpenCode**: Separate file system (messages not in session metadata file)
- **Mistral Vibe**: `messages.jsonl` holds message entries (one JSON object per line)

**File Organization:**
- **Claude Code**: Single JSONL file per session
- **Codex**: Single JSONL file per session
- **OpenCode**: Multi-file structure (metadata + message directories + parts + diffs)
- **Mistral Vibe**: Directory-based session (`meta.json` + `messages.jsonl`), plus a separate input history file `~/.vibe/vibehistory` (not a full session log)

---

## Event Types

### Claude Code

```json
{
  "type": "summary",          // Session title
  "type": "user",             // User messages
  "type": "assistant",        // Assistant messages
  "type": "system",           // System events (subtype: local_command)
  "type": "file-history-snapshot"  // File state tracking
}
```

**User Message Example:**
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

**Assistant Tool-Use Content Block (in `message.content[]`):**
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

**Tool Execution System Event (commonly observed):**
```json
{
  "type": "system",
  "subtype": "local_command",
  "command": ["ls", "-la"],
  "stdout": "...",
  "timestamp": "2025-01-10T10:30:10.000Z"
}
```

### Codex

Codex rollout logs are envelope-based JSONL entries (`RolloutLine`).

```json
{
  "timestamp": "2026-01-18T01:01:30.000Z",
  "type": "session_meta" | "event_msg" | "response_item" | "turn_context" | "compacted",
  "payload": { "...": "..." }
}
```

**Session Metadata Example:**
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

**User / Assistant Events (via `event_msg`):**
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

**Turn Context (model captured per turn):**
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

**Session Configured Event (can include model + provider):**
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

**Tool-Related Events (selected examples):**
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

**Collaboration/Subagent Events (Codex app-server / collab mode):**
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

### OpenCode

**Session Metadata File** (`session/<project>/ses_xxx.json`):
```json
{
  "id": "ses_66a71b6f4ffeq796jvvOpJQ04m",
  "version": "1.0.0",
  "projectID": "abc123def456",
  "directory": "/home/user/project",
  "title": "Fix authentication bug",
  "time": {
    "created": 1704067200000,
    "updated": 1704153600000
  },
  "parentID": "ses_parent123"  // Optional: indicates subagent session
}
```

**Key Fields:**
- `id`: Unique session identifier (format: `ses_<identifier>`)
- `version`: OpenCode version
- `projectID`: Git root commit hash (used for project identification)
- `directory`: Working directory path
- `title`: Session title/description
- `time.created`: Creation timestamp (Unix epoch milliseconds)
- `time.updated`: Last update timestamp (Unix epoch milliseconds)
- `parentID`: Optional - present only for subagent sessions (spawned via task tools)

Model fields are message-scoped (not session-scoped):
- User messages: `model.providerID` + `model.modelID`
- Assistant messages: top-level `providerID` + `modelID`

**Storage Structure:**
```
~/.local/share/opencode/storage/
├── session/<projectID>/ses_xxx.json     # Session metadata
├── message/ses_xxx/                      # Message metadata files
├── part/msg_xxx/                         # Message parts (text/tool/subtask/etc.)
└── session_diff/ses_xxx.json            # File change tracking
```

**Message File** (`message/<sessionID>/msg_xxx.json`):
```json
{
  "id": "msg_user_001",
  "sessionID": "ses_abc",
  "role": "user",
  "agent": "assistant",
  "model": {
    "providerID": "anthropic",
    "modelID": "claude-sonnet-4-5"
  },
  "time": {
    "created": 1704067210000
  }
}
```

```json
{
  "id": "msg_asst_001",
  "sessionID": "ses_abc",
  "role": "assistant",
  "parentID": "msg_user_001",
  "providerID": "anthropic",
  "modelID": "claude-sonnet-4-5",
  "time": {
    "created": 1704067210500,
    "completed": 1704067211200
  }
}
```

**Tool Part** (`part/<messageID>/part_xxx.json`):
```json
{
  "id": "part_002",
  "sessionID": "ses_abc",
  "messageID": "msg_001",
  "type": "tool",
  "callID": "call_01",
  "tool": "bash",
  "state": {
    "status": "completed",
    "input": { "command": "ls -la" },
    "title": "List files",
    "output": "...",
    "time": { "start": 1704067210000, "end": 1704067211200 }
  }
}
```

**Subtask Part** (records delegated work in parent session):
```json
{
  "id": "part_subtask",
  "sessionID": "ses_parent",
  "messageID": "msg_user",
  "type": "subtask",
  "prompt": "Find all parser files",
  "description": "Explore parser layout",
  "agent": "explore",
  "model": { "providerID": "anthropic", "modelID": "claude-sonnet" },
  "command": "@explore find parser files"
}
```

**Project Identification:**
- Uses git root commit hash as `projectID`
- Command: `git rev-list --max-parents=0 --all`
- Sessions grouped by project under `session/<projectID>/`

**Subagent Sessions:**
- Child sessions spawned through task tools or agent mentions
- Identified by presence of `parentID` field
- Form hierarchical parent-child relationships
- Can accumulate without cleanup (known limitation)

### Mistral Vibe

**Session Directory** (`~/.vibe/logs/session/session_*/`):

- `meta.json` contains session-wide timestamps, environment info, token/tool usage stats, and config snapshots
- `messages.jsonl` is an OpenAI-style chat transcript (`role`, `content`, optional `tool_calls`)
- `messages.jsonl` entries do not include a normalized model identifier field
- Model selection is recoverable from `meta.json` config snapshot when present (`config.active_model`, plus `config.providers` / `config.models`)
- No stable per-message IDs in `messages.jsonl`; tool call correlation is via `tool_calls[*].id` -> `tool_call_id`

**Tool Call + Result (simplified):**
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
{
  "role": "tool",
  "name": "bash",
  "tool_call_id": "abc123",
  "content": "stdout: ...\n\nstderr: ...\nreturncode: 0"
}
```

---

## Subagents & Tool Calls (Focused Update)

### Raw Format Findings

**Claude Code**
- Tool invocations are represented in assistant `message.content[]` blocks as `{"type":"tool_use", "id", "name", "input"}`.
- Tool execution output is commonly observable in `system` events (`subtype: "local_command"`) with command/result fields (`command`, `stdout`, `stderr`).
- Sidechain/subagent context appears through `isSidechain` and parent links (`parentUuid`).

**Codex**
- Modern rollouts are envelope JSONL entries: `session_meta`, `event_msg`, `response_item`, `turn_context`, `compacted`.
- Tool-related activity is emitted as `event_msg.payload.type` variants, especially `exec_command_*`, `mcp_tool_call_*`, and web-search events.
- Collaboration/subagent activity is explicit in `collab_*` events (`collab_agent_spawn_begin/end`, `collab_resume_*`, `collab_waiting_*`) and in `session_meta.source` (`subagent_*`).

**OpenCode**
- Parent/child sessions are explicit at session level with `parentID`.
- Tool calls are first-class message parts (`type: "tool"`) with lifecycle state machine:
  - `pending` (`input`, `raw`)
  - `running` (`input`, optional `title`/`metadata`, `time.start`)
  - `completed` (`input`, `output`, `title`, `metadata`, `time.start/end`, optional `attachments`)
  - `error` (`input`, `error`, optional `metadata`, `time.start/end`)
- Delegation intent is captured in `subtask` parts (`prompt`, `description`, `agent`, optional `model`, optional `command`), and task execution creates child sessions with `parentID`.

**Mistral Vibe**
- Tool calls are OpenAI-style `assistant.tool_calls[]` entries (`id`, `function.name`, `function.arguments`).
- Tool outputs are separate `role: "tool"` messages, linked by `tool_call_id`.
- No dedicated subagent session model observed in current `meta.json` + `messages.jsonl` format.

### LLM Model Metadata Availability (Focused Update)

Goal: determine whether model information is available per message, per turn, and/or per session.

| Tool | Per Message | Per Turn | Per Session | Notes |
|------|-------------|----------|-------------|-------|
| **Claude Code** | ❌ Not observed as a stable structured field in sampled `user`/`assistant` records | ❌ No explicit turn-context object in the known JSONL schema | ⚠️ Partial: session/events include `version`, but no canonical `model` key | Model switches may appear as free-text command/system content (for example `/model`) rather than a normalized field. |
| **Codex** | ⚠️ Not on `user_message` / `agent_message` payloads | ✅ `turn_context.payload.model` (`TurnContextItem.model`) | ✅/⚠️ `session_meta.payload.model_provider` is optional and provider-only (no guaranteed model slug) | `event_msg.payload.type == "session_configured"` can provide `model` + `model_provider_id`; reroutes can be observed via `model_reroute` events. |
| **OpenCode** | ✅ User message has `model.{providerID,modelID}` and assistant message has `providerID` + `modelID` | N/A (message-centric schema) | ❌ Session metadata has no model field | `subtask` parts can optionally pin delegated model (`model.providerID`, `model.modelID`). |
| **Mistral Vibe** | ❌ `messages.jsonl` (`LLMMessage`) has no model key | ❌ No separate turn-context model object in logs | ✅ `meta.json` metadata dump can contain `config` snapshot with `active_model`, plus `providers`/`models` arrays | Requires session logging metadata output; minimal/older logs may omit full config snapshot. |

**Primary evidence used for this update:**
- Codex protocol and recorder code: `codex-rs/protocol/src/protocol.rs` (`SessionMeta`, `TurnContextItem`, `SessionConfiguredEvent`) and `codex-rs/core/src/codex.rs` (`to_turn_context_item` persisted before sampling).
- OpenCode schemas: `packages/opencode/src/session/message-v2.ts` and generated SDK types (`packages/sdk/js/src/v2/gen/types.gen.ts`).
- Mistral Vibe logger/types: `vibe/core/session/session_logger.py` and `vibe/core/types.py`.
- Claude Code format observations from fixtures and existing external format analysis (`agent-sessions` docs).

### Current Sessions Chronicle Parsing Behavior

**Important:** raw formats above are richer than what is currently indexed.

- **Claude parser (`src/parsers/claude_code.rs`)**: indexes `type == user|assistant`; ignores `tool_use` blocks and `system` tool-output events.
- **Codex parser (`src/parsers/codex.rs`)**: indexes only `event_msg.payload.type == user_message|agent_message`; ignores tool/collab event variants.
- **OpenCode parser (`src/parsers/opencode.rs`)**:
  - skips sessions with `parentID` (subagents)
  - converts only `part.type == text`
  - skips `tool`, `reasoning`, `step-start`, `step-finish`, `snapshot`, `compaction`, `subtask`
- **Mistral Vibe parser (`src/parsers/mistral_vibe.rs`)**: indexes `role == user|assistant` text; skips `role == tool` and assistant records that only contain `tool_calls` with empty text.
- **All parsers currently**: do not persist model metadata into indexed session/message tables (no normalized `model`/`provider` columns yet).

This is intentional and matches current project scope. Tool/subagent transcript indexing remains a separate follow-up.

## Special Features

### Claude Code

**Summary Events:**
```json
{
  "type": "summary",
  "summary": "Session title text",
  "leafUuid": "UUID"
}
```

**File History Snapshots:**
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

**Meta Flag:**
- `isMeta: true` → Skip for title extraction (system-generated)
- `isMeta: false` → User-generated content

### Codex

**Rollout Envelope:**
- Each JSONL line is a `RolloutLine` (`timestamp` + tagged `type` + `payload`)
- Core variants: `session_meta`, `event_msg`, `response_item`, `turn_context`, `compacted`

**Subagent/Collab Provenance:**
- `session_meta.source` supports subagent variants (`subagent_review`, `subagent_compact`, thread-spawn variants)
- `event_msg` can include collab lifecycle events (`collab_agent_spawn_*`, `collab_waiting_*`, `collab_resume_*`, `collab_close_*`)

**Encrypted Reasoning:**
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

**Multimodal Content:**

Two patterns:
1. **Inline Base64**: `data:image/png;base64,iVBORw0...`
2. **References**: HTTP(S) URLs or file identifiers

### OpenCode

**Multi-Directory Storage:**
- Session metadata separate from message content
- Allows independent access to sessions vs full conversation history
- File change tracking in dedicated `session_diff/` directory

**Git-Based Project Organization:**
- Project identification via git root commit hash
- Automatic grouping of sessions by repository
- No manual project configuration needed

**Orphaned Data Risk:**
- Deleting session metadata file leaves orphaned messages/parts/diffs
- No built-in cleanup mechanism
- Manual deletion requires removing multiple related directories

**Subtask Metadata:**
- Delegated work is represented in `subtask` parts on parent-session user messages (`prompt`, `description`, `agent`, optional `model`, optional `command`)
- Task execution then creates child sessions whose `parentID` points to the parent session

### Mistral Vibe

**Rich Session Metadata:**
- `meta.json.stats` includes token usage and tool call counters
- `meta.json.tools_available` captures the set of tools available to the agent for the session
- `meta.json.config` captures a snapshot of resolved configuration (including active model + provider/model catalogs)
- `meta.json.agent_profile` captures selected profile/override metadata

**Input History (Not a Session Log):**
- `~/.vibe/vibehistory` stores a JSONL list of user inputs for prompt recall; it does not contain the full assistant/tool transcript

---

## Parser Design Implications

### Title Extraction Strategy

| Tool | Logic |
|------|-------|
| **Claude Code** | First parsed `user` message content (assistant/system/summary are ignored by parser). |
| **Codex** | First `event_msg.payload.type == "user_message"` event (`payload.message`). |
| **OpenCode** | First flattened `text` part attached to a `user` message (session metadata `title` is currently not indexed). |
| **Mistral Vibe** | First `messages.jsonl` entry where `role == "user"` and `content` is non-empty. |

### Timestamp Parsing

| Tool | Approach |
|------|----------|
| **Claude Code** | Track earliest/latest across `type in {user, assistant}` using per-event `timestamp` (ISO-8601). |
| **Codex** | `start_time` from first-line `session_meta.payload.timestamp`; `last_updated` from max `event.timestamp` seen in `event_msg` lines. |
| **OpenCode** | Session timestamps from metadata `time.created` + `time.updated` (ms epoch), with per-message `time.created` used for ordering. |
| **Mistral Vibe** | `start_time` from `meta.json.start_time`; `last_updated` from `meta.json.end_time` (fallback to `start_time`). |

### Content Extraction

```rust
// Claude Code
fn extract_content_claude(event: &Value) -> Option<String> {
    // supports both plain string and block arrays
    // array blocks currently include "text" and "thinking"
    ClaudeCodeParser::extract_content(event.get("message")?.get("content")?)
}

// Codex
fn extract_content_codex_event_msg(event: &Value) -> Option<(Role, String)> {
    let payload = event.get("payload")?;
    match payload.get("type")?.as_str()? {
        "user_message" => Some((Role::User, payload.get("message")?.as_str()?.to_string())),
        "agent_message" => Some((Role::Assistant, payload.get("message")?.as_str()?.to_string())),
        _ => None,
    }
}

// OpenCode
fn extract_opencode_text_part(part: &Value) -> Option<String> {
    if part.get("type")?.as_str()? != "text" {
        return None;
    }
    part.get("text")?.as_str().map(|s| s.to_string())
}

// Mistral Vibe
fn extract_vibe_content(event: &Value) -> Option<String> {
    event.get("content")?
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
}
```

### Important: Use Streaming for JSONL Files

**Do NOT load entire JSONL files into memory:**

```rust
// WRONG - loads entire file into RAM
let content = fs::read_to_string(file_path)?;
let lines: Vec<&str> = content.lines().collect();
for line in lines { /* parse */ }
```

**Use BufReader for line-by-line streaming:**

```rust
// CORRECT - streams line by line
let file = File::open(file_path)?;
let reader = BufReader::new(file);
for line in reader.lines() {
    let line = line?;
    if !line.trim().is_empty() {
        let event: Value = serde_json::from_str(&line)?;
        // process event
    }
}
```

This is critical for sessions with thousands of messages.

### Tool Call Handling

**Claude Code:**
- Raw data can appear in assistant `message.content[]` as `type == "tool_use"`
- Tool execution output is often represented in `system` events (`subtype == "local_command"`)
- Current parser behavior: ignores both patterns (indexes only `user`/`assistant` text)

**Codex:**
- Raw data is emitted via `event_msg.payload.type` variants such as `exec_command_*`, `mcp_tool_call_*`, `web_search_*`, and collab `collab_*`
- Tool call correlation typically uses `call_id`
- Current parser behavior: ignores these events and indexes only `user_message` / `agent_message`

**OpenCode:**
- Tool calls are explicit `part.type == "tool"` records with lifecycle state (`pending`/`running`/`completed`/`error`)
- Delegation markers are explicit `part.type == "subtask"` records
- Current parser behavior: skips `tool` and `subtask` parts, and skips child sessions with `parentID`

**Mistral Vibe:**
- Tool calls appear on assistant messages under `tool_calls[]`
- Tool outputs are separate messages with `role == "tool"` and `tool_call_id` matching the call id (`name` may be present depending on producer)
- Arguments are stored as JSON-encoded strings (`tool_calls[*].function.arguments`)
- Current parser behavior: ignores `role == "tool"` records and assistant-only tool-call stubs without text

---

## Session Metadata Extraction

### From File Path

**Claude Code:**
```
~/.claude/projects/-Users-alexm-Repository-myproject/UUID.jsonl
                    └──────────────────────────────┘
                           Project path encoding
```

**Codex:**
```
~/.codex/sessions/2025/09/12/rollout-20250912-164103.jsonl
                  └─────────┘          └──────────┘
                  Date sharding        Timestamp in filename
```

**OpenCode:**
```
~/.local/share/opencode/storage/session/abc123def456/ses_xxx.json
                                        └─────────┘  └──────┘
                                        Project ID   Session ID
                                        (git root commit hash)
```

**Mistral Vibe:**
```
~/.vibe/logs/session/session_20260123_174305_64883c86/
                    └──────────────┬──────────────┘
                       timestamp + session id prefix
```

### From Events

**Claude Code** (rich metadata per event):
- `sessionId`: Unique session identifier
- `cwd`: Working directory
- `gitBranch`: Git branch name
- `version`: Claude Code version
- `userType`: "external" or other
- No canonical structured `model` field observed in sampled session events

**Codex** (envelope + event payload model):
- `session_meta.payload.id`: session/thread identifier
- `session_meta.payload.cwd`: working directory
- `session_meta.payload.source`: source provenance (`cli`, `vscode`, `subagent_*`, ...)
- `session_meta.payload.model_provider`: optional session-level provider id
- `turn_context.payload.model`: active model slug for that turn
- `event_msg.payload` can include `session_configured` (`model`, `model_provider_id`)
- Additional event metadata in `event_msg.payload` (tool call IDs, collab thread IDs, etc.)

**OpenCode** (session-level metadata):
- `id`: Session identifier (commonly `ses_*`, but parser should not hardcode prefix)
- `projectID`: Git root commit hash
- `directory`: Working directory path
- `version`: OpenCode version
- `title`: User-provided session title
- `parentID`: Parent session ID (if subagent)
- Model metadata is message-level (`user.model.{providerID,modelID}` and assistant `providerID`/`modelID`)

**Mistral Vibe** (session-level metadata in `meta.json`):
- `session_id`: UUID
- `start_time`, `end_time`: ISO-8601 strings
- `environment.working_directory`: working directory
- Optional git info: `git_commit`, `git_branch`
- Optional model config snapshot in `meta.json.config` (for example `active_model`, `providers`, `models`)
- `stats`: token usage, tool call counters, and other session metrics

---

## Recommended Parser Architecture

### Trait-Based Design

```rust
trait SessionParser {
    fn parse_file(&self, path: &Path) -> Result<Session>;
    fn extract_metadata(&self, path: &Path) -> Result<SessionMetadata>;
    fn parse_event(&self, line: &str) -> Result<Event>;  // For JSONL-based parsers
    fn extract_title(&self, events: &[Event]) -> Option<String>;
}

struct ClaudeCodeParser;  // JSONL parser
struct CodexParser;       // JSONL parser
struct OpenCodeParser;    // JSON + multi-file parser
struct MistralVibeParser; // Directory-based session parser

impl SessionParser for ClaudeCodeParser { /* ... */ }
impl SessionParser for CodexParser { /* ... */ }
impl SessionParser for OpenCodeParser {
    // Special handling: reads session metadata from JSON file
    // Messages loaded from separate directory structure
    // Must handle parent-child session relationships
}
impl SessionParser for MistralVibeParser {
    // Reads `meta.json` + streams `messages.jsonl`
    // Title and timestamps are stored at session-level (no per-message timestamps)
}
```

### Parser Factory

```rust
fn get_parser(path: &Path) -> Box<dyn SessionParser> {
    if path.starts_with("~/.claude/") {
        Box::new(ClaudeCodeParser)
    } else if path.starts_with("~/.codex/") {
        Box::new(CodexParser)
    } else if path.starts_with("~/.local/share/opencode/") {
        Box::new(OpenCodeParser)
    } else if path.starts_with("~/.vibe/logs/session/") {
        Box::new(MistralVibeParser)
    } else {
        // Try to detect from file structure
        detect_parser(path)
    }
}
```

### OpenCode-Specific Parser Challenges

**Multi-File Reading:**
```rust
impl OpenCodeParser {
    fn parse_session(&self, session_path: &Path) -> Result<Session> {
        // 1. Read session metadata JSON
        let metadata = self.read_session_metadata(session_path)?;

        // 2. Construct message directory path
        let session_id = &metadata.id;
        let msg_dir = Path::new("~/.local/share/opencode/storage/message")
            .join(session_id);

        // 3. Read all messages from directory
        let messages = self.read_messages(&msg_dir)?;

        // 4. Read message parts
        let parts = self.read_message_parts(&messages)?;

        // 5. Read session diffs
        let diffs = self.read_session_diffs(session_id)?;

        Ok(Session {
            metadata,
            messages,
            parts,
            diffs,
        })
    }
}
```

---

## Open Questions

1. **Tool/Event Indexing Scope**:
   - Should tool calls/results become first-class indexed records (new role/type), or remain transcript-only metadata?
   - If indexed, should we preserve full structured JSON (`input`, `output`, `metadata`, `attachments`) or normalize to text?

2. **OpenCode Parent-Child Session Display**:
   - Should subagent sessions be shown nested under parents?
   - Or displayed as separate sessions with parent reference?
   - How deep can nesting go?

3. **Codex Collaboration Mapping**:
   - Should Codex `collab_*` events map to the same "subagent" concept as OpenCode `parentID` sessions?
   - Do we show child thread IDs as navigable links in the UI?

4. **OpenCode Session Diffs**:
   - Should `session_diff/ses_xxx.json` be ingested for richer "changes made" previews?
   - How should diff metadata be surfaced without overwhelming session list/search?

5. **Image/Attachment Handling in Tool Results**:
   - How should we present tool-result attachments (data URLs, image/pdf, references) safely?
   - Should remote references require explicit user opt-in before fetch?

6. **Error Handling for Malformed Data**:
   - How should parser handle malformed JSON/JSONL lines?
   - Skip and continue, or fail entire session?
   - What about missing required fields?
   - Recommendation: Log warnings, skip problematic entries, continue indexing

7. **Memory Management for Large Sessions**:
   - What's the practical limit for session size?
   - Should large messages be truncated for display?
   - How to handle sessions with 10,000+ messages?
   - Consider pagination or virtual scrolling in UI

---

## Next Steps for Design

1. **Tool call indexing prototype (Phase 4 candidate)**:
   - Add optional extraction mode in each parser for tool/subtask/collab events
   - Keep existing user/assistant indexing unchanged as baseline behavior

2. **Subagent graph model**:
   - Prototype a unified parent-child relation that can represent:
     - OpenCode session-level `parentID`
     - Codex collab/thread-spawn links
     - Claude sidechain indicators

3. **UI surfacing experiment**:
   - Add optional expandable "Tool Activity" and "Subtasks/Subagents" sections in session details
   - Evaluate nested vs flat display using OpenCode fixtures with parent/child sessions

4. **Diff ingestion spike (OpenCode)**:
   - Parse `session_diff/ses_xxx.json` and test lightweight summaries (file count, additions/deletions)

5. **Test parser with edge cases**:
   - Empty sessions
   - Malformed JSON/JSONL
   - Missing required fields
   - Very large files (JSONL streaming)
   - OpenCode orphaned data (missing message/part directories)
   - Deep parent-child hierarchies (OpenCode)

---

## Reference Documentation

### Official Format Documentation
- [Claude Code Session Format](https://github.com/jazzyalex/agent-sessions/blob/main/docs/claude-code-session-format.md)
- [Codex Session Storage Format](https://github.com/jazzyalex/agent-sessions/blob/main/docs/session-storage-format.md)
- [Codex Schema Reference](https://github.com/jazzyalex/agent-sessions/blob/main/docs/schemas/session_event.schema.json)

### Codex (Primary Sources)
- [Codex protocol `RolloutItem`, `SessionMeta`, `EventMsg`](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs)
- [Codex turn-context persistence (`RolloutItem::TurnContext` before sampling)](https://github.com/openai/codex/blob/main/codex-rs/core/src/codex.rs)
- [Codex rollout recorder writes `session_meta.model_provider`](https://github.com/openai/codex/blob/main/codex-rs/core/src/rollout/recorder.rs)
- [Codex app-server thread/item event model](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)
- [Codex TypeScript SDK note on session persistence (`~/.codex/sessions`)](https://github.com/openai/codex/blob/main/sdk/typescript/README.md)

### OpenCode Information Sources
- [Agent Sessions GitHub Repository](https://github.com/jazzyalex/agent-sessions) - Multi-tool session browser
- [OpenCode GitHub Repository](https://github.com/sst/opencode) - Official OpenCode repository
- [OpenCode Sessions Issue #3026](https://github.com/sst/opencode/issues/3026) - Storage structure details
- [OpenCode Sessions Issue #5734](https://github.com/sst/opencode/issues/5734) - Subagent session structure
- [OpenCode `MessageV2` part schemas (`tool`, `subtask`, etc.)](https://github.com/sst/opencode/blob/dev/packages/opencode/src/session/message-v2.ts)
- [OpenCode task tool creates child sessions with `parentID`](https://github.com/sst/opencode/blob/dev/packages/opencode/src/tool/task.ts)
- [OpenCode generated v2 SDK types (`Session`, `Part`, `ToolPart`)](https://github.com/sst/opencode/blob/dev/packages/sdk/js/src/v2/gen/types.gen.ts)
- [OpenCode session schema (session-level fields, no model)](https://github.com/sst/opencode/blob/dev/packages/opencode/src/session/index.ts)

### Claude Tool-Use References
- [Claude API tool-use block structure (`tool_use`/`tool_result`)](https://platform.claude.com/docs/en/api/typescript/messages/create)

### Mistral Vibe Information Sources
- [Mistral Vibe Configuration Docs](https://docs.mistral.ai/mistral-vibe/introduction/configuration) - `VIBE_HOME`, `config.toml` behavior
- [Mistral Vibe Repository](https://github.com/mistralai/mistral-vibe) - session logging implementation
- [Mistral Vibe session logger (`meta.json` + `messages.jsonl` + `config` dump)](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/session/session_logger.py)
- [Mistral Vibe message/session models (`SessionMetadata`, `LLMMessage`)](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/types.py)

### Key Findings Summary

- **Claude Code**: JSONL format, tree-structured events, project-based organization; no stable structured per-message/per-session model field observed in sampled logs
- **Codex**: JSONL rollout envelope (`session_meta`/`event_msg`/`turn_context`/...); model provider can exist at session level, and model slug is captured at turn level (`turn_context.model`)
- **OpenCode**: Multi-file JSON format with explicit `tool` and `subtask` parts; model metadata is message-level (`user.model.*`, assistant `providerID`/`modelID`), not session-level
- **Mistral Vibe**: Directory-based session format with `meta.json` + JSONL `messages.jsonl`; model info is session-level via `meta.json.config` snapshot when present, not message-level

---

**Last Updated**: 2026-02-21
**Status**: Subagent + tool-call + model-metadata analysis refreshed; parser behavior and remaining scope gaps documented
