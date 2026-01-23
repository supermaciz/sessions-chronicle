# Session Format Analysis

Analysis of Claude Code, Codex, OpenCode, and Mistral Vibe session file formats for Sessions Chronicle parser design.

---

## Implementation Status

- ✅ Claude Code parser + indexer implemented
- ✅ Session date/sort semantics aligned with agent-sessions (Claude: end time = latest message-like event)
- ⬜ OpenCode parser pending (filters show empty for OpenCode)
- ⬜ Codex parser pending (filters show empty for Codex)
- ⬜ Mistral Vibe parser pending (not yet detected/indexed)

---

## Storage Locations

| Tool | Path | Organization |
|------|------|--------------|
| **Claude Code** | `~/.claude/` | Project-specific directories<br>`~/.claude/projects/-Users-alexm-Repository-<project>/UUID.jsonl` |
| **Codex** | `~/.codex/sessions/` | Date-sharded directories<br>`YYYY/MM/DD/rollout-*.jsonl` |
| **OpenCode** | `~/.local/share/opencode/storage/` | Multi-directory structure:<br>`session/<project>/ses_xxx.json` (metadata)<br>`message/ses_xxx/` (messages)<br>`part/msg_xxx/` (message parts)<br>`session_diff/ses_xxx.json` (file changes) |
| **Mistral Vibe** | `~/.vibe/logs/session/` | One JSON file per session:<br>`session_YYYYMMDD_HHMMSS_<shortid>.json`<br>Default can be overridden via `VIBE_HOME` or `session_logging.save_dir` in `config.toml`. |

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

**Mistral Vibe** uses **single JSON files**:
- One JSON file per session
- UTF-8 encoded, pretty-printed JSON (not line-delimited)
- Session file is rewritten/updated over time (not append-only)
- Conversation stored as a list of messages (OpenAI-style)

---

## File Naming

| Tool | Pattern | Example |
|------|---------|---------|
| **Claude Code** | `UUID.jsonl` | `a1b2c3d4-e5f6-7890-abcd-ef1234567890.jsonl` |
| **Codex** | `rollout-*.jsonl` | `rollout-20250912-164103.jsonl` |
| **OpenCode** | `ses_*.json` | `ses_66a71b6f4ffeq796jvvOpJQ04m.json` |
| **Mistral Vibe** | `session_*.json` | `session_20260123_174305_64883c86.json` |

---

## Event Structure Comparison

### Common Fields

| Field Category | Claude Code | Codex | OpenCode | Mistral Vibe |
|----------------|-------------|-------|----------|-------------|
| **Event Type** | `type` (`user`, `system`, `summary`) | `type` (preferred) or `role` (fallback) | Session metadata only (messages in separate files) | `role` (`system`, `user`, `assistant`, `tool`); tool calls on assistant messages via `tool_calls` |
| **Identity** | `uuid`, `parentUuid` (tree structure) | `id`/`message_id`, `parent_id` (threaded) | `id`, `parentID` (hierarchical sessions) | No message IDs; tool calls have an `id` and tool responses reference it via `tool_call_id` |
| **Timestamp** | `timestamp` (ISO-8601) | Multiple possible keys: `timestamp`, `time`, `ts`, `created`, etc. | `time.created`, `time.updated` (session level) | Session-level only: `metadata.start_time`, `metadata.end_time` (ISO-8601). No per-message timestamps |
| **Content** | Nested: `message.content` | Top-level: `content`, `text`, or `message` | Stored in `message/ses_xxx/` directory | `messages[*].content` for text; tool output stored as `role: "tool"` messages |

### Key Architectural Differences

**Threading Model:**
- **Claude Code**: Tree structure via `uuid`/`parentUuid` + `isSidechain` flag
- **Codex**: Linear threading via `message_id`/`parent_id`
- **OpenCode**: Parent-child sessions via `parentID` (subagent sessions)
- **Mistral Vibe**: Linear message list; tool calls are embedded in assistant messages and resolved by subsequent `tool` role messages

**Metadata Storage:**
- **Claude Code**: Rich per-event metadata (`cwd`, `gitBranch`, `version`, `sessionId`)
- **Codex**: Minimal per-event metadata, model info stored separately
- **OpenCode**: Session-level metadata (`projectID`, `directory`, `version`, `title`)
- **Mistral Vibe**: Session-level `metadata` includes environment, optional git info, token/tool usage stats, tools snapshot, and agent config snapshot

**Content Access:**
- **Claude Code**: `event.message.content` (nested in JSONL events)
- **Codex**: `event.content` or `event.text` (top-level in JSONL events)
- **OpenCode**: Separate file system (messages not in session metadata file)
- **Mistral Vibe**: `messages` array inside the JSON session file

**File Organization:**
- **Claude Code**: Single JSONL file per session
- **Codex**: Single JSONL file per session
- **OpenCode**: Multi-file structure (metadata + message directories + parts + diffs)
- **Mistral Vibe**: One JSON file per session (non-append-only), plus a separate input history file `~/.vibe/vibehistory` (not a full session log)

---

## Event Types

### Claude Code

```json
{
  "type": "summary",          // Session title
  "type": "user",             // User messages
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

### Codex

```json
{
  "type": "user",             // User messages
  "type": "assistant",        // Assistant responses
  "type": "tool_call",        // Tool invocations
  "type": "tool_result",      // Tool outputs
  "type": "error",            // Error events
  "type": "meta",             // Metadata events
  "type": "reasoning"         // Thinking/reasoning (may be encrypted)
}
```

**User Message Example:**
```json
{
  "type": "user",
  "timestamp": "2025-09-12T16:41:03Z",
  "content": "Find all TODOs in the repo"
}
```

**Tool Call + Result:**
```json
{
  "type": "tool_call",
  "function": {"name": "grep"},
  "arguments": {"pattern": "TODO"}
}
{
  "type": "tool_result",
  "stdout": "README.md:12: TODO: add tests\n"
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

**Storage Structure:**
```
~/.local/share/opencode/storage/
├── session/<projectID>/ses_xxx.json     # Session metadata
├── message/ses_xxx/                      # Session messages (separate directory)
├── part/msg_xxx/                         # Message parts/components
└── session_diff/ses_xxx.json            # File change tracking
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

**Session Log File** (`~/.vibe/logs/session/session_*.json`):

- Top-level object with two keys: `metadata` and `messages`
- `metadata` contains session-wide timestamps, environment info, token/tool usage stats, and config snapshots
- `messages` is an OpenAI-style chat transcript (`role`, `content`, optional `tool_calls`)

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

**Streaming Support:**
- Fields: `delta`, `chunk`, `delta_index`
- Parser must coalesce chunks by `message_id`

**Encrypted Reasoning:**
```json
{
  "type": "reasoning",
  "encrypted_content": "AAECAwQFBgcICQoL..."
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

### Mistral Vibe

**Rich Session Metadata:**
- `metadata.stats` includes token usage and tool call counters
- `metadata.tools_available` captures the set of tools available to the agent for the session
- `metadata.agent_config` captures a snapshot of the resolved configuration (providers, models, tool permissions)

**Input History (Not a Session Log):**
- `~/.vibe/vibehistory` stores a JSONL list of user inputs for prompt recall; it does not contain the full assistant/tool transcript

---

## Parser Design Implications

### Title Extraction Strategy

| Tool | Logic |
|------|-------|
| **Claude Code** | Look for `type == "summary"` and extract `summary` field<br>Fallback: First `type == "user"` where `isMeta == false` → use `message.content` |
| **Codex** | First `type == "user"` event → use `content` or `text` field |
| **OpenCode** | Direct access: session metadata contains `title` field at top level |
| **Mistral Vibe** | No explicit title field. Use first `messages[*]` where `role == "user"` and `content` is non-empty (optionally truncate for display). |

### Timestamp Parsing

| Tool | Approach |
|------|----------|
| **Claude Code** | Single field: `timestamp` (ISO-8601 string) |
| **Codex** | Check multiple fields in priority order:<br>`timestamp` → `time` → `ts` → `created` → `created_at` → ... |
| **OpenCode** | Nested object: `time.created` and `time.updated` (Unix epoch milliseconds) |
| **Mistral Vibe** | Prefer `metadata.end_time` (ISO-8601), fallback to `metadata.start_time`, then file mtime if missing. |

### Content Extraction

```rust
// Claude Code
fn extract_content_claude(event: &Value) -> Option<String> {
    event.get("message")?.get("content")?.as_str()
}

// Codex
fn extract_content_codex(event: &Value) -> Option<String> {
    event.get("content")
        .or_else(|| event.get("text"))
        .or_else(|| event.get("message"))
        .and_then(|v| v.as_str())
}

// OpenCode
fn extract_title_opencode(session: &Value) -> Option<String> {
    session.get("title")?.as_str().map(|s| s.to_string())
}

// Mistral Vibe
fn extract_title_vibe(session_log: &Value) -> Option<String> {
    session_log
        .get("messages")?
        .as_array()?
        .iter()
        .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("user"))
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

// Note: OpenCode messages are in separate directory structure
fn get_messages_path_opencode(session_id: &str) -> PathBuf {
    PathBuf::from("~/.local/share/opencode/storage/message")
        .join(session_id)
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
- Not explicitly documented (may be in `system` events with `subtype`)
- Need to inspect actual files

**Codex:**
- Clear structure: `tool_call` → `tool_result`
- Tool name: `tool`, `name`, or `function.name`
- Arguments: `arguments` or `input`
- Output: `stdout`, `stderr`, `result`, or `output`

**OpenCode:**
- Tool calls likely stored in message parts (`part/msg_xxx/`)
- Structure not documented - needs investigation
- May follow similar pattern to Codex (structured messages)

**Mistral Vibe:**
- Tool calls appear on assistant messages under `tool_calls[]`
- Tool outputs are separate messages with `role == "tool"`, `name == <tool>`, and `tool_call_id` matching the call id
- Arguments are stored as JSON-encoded strings (`tool_calls[*].function.arguments`)

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
~/.vibe/logs/session/session_20260123_174305_64883c86.json
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

**Codex** (minimal per-event):
- Extract from first event
- Infer from directory structure
- Tool/model info may vary by event

**OpenCode** (session-level metadata):
- `id`: Session identifier (`ses_<id>`)
- `projectID`: Git root commit hash
- `directory`: Working directory path
- `version`: OpenCode version
- `title`: User-provided session title
- `parentID`: Parent session ID (if subagent)

**Mistral Vibe** (session-level metadata in `metadata` object):
- `session_id`: UUID
- `start_time`, `end_time`: ISO-8601 strings
- `environment.working_directory`: working directory
- Optional git info: `git_commit`, `git_branch`
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
struct MistralVibeParser; // JSON session log parser

impl SessionParser for ClaudeCodeParser { /* ... */ }
impl SessionParser for CodexParser { /* ... */ }
impl SessionParser for OpenCodeParser {
    // Special handling: reads session metadata from JSON file
    // Messages loaded from separate directory structure
    // Must handle parent-child session relationships
}
impl SessionParser for MistralVibeParser {
    // Reads a single JSON file containing `metadata` + `messages`
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
        let session_id = metadata.id.strip_prefix("ses_")?;
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

1. **Claude Code Tool Calls**: Not explicitly shown in docs
   - Do they appear as `system` events?
   - What does `subtype: "local_command"` contain?
   - Need to inspect actual session files

2. **OpenCode Message Format**: Session metadata is documented, but message structure is not
   - What format are files in `message/ses_xxx/` directory?
   - What are "message parts" in `part/msg_xxx/`?
   - How are tool calls represented?
   - Are messages also JSONL, JSON, or another format?

3. **OpenCode Session Diffs**: File change tracking mentioned but not detailed
   - What's the structure of `session_diff/ses_xxx.json`?
   - How are file changes tracked (full content vs diffs)?
   - Is this similar to Claude Code's `file-history-snapshot`?

4. **Streaming Chunks**: Codex supports delta/chunk fields
   - Should parser coalesce automatically?
   - Or store chunks separately for playback?

5. **Image Handling**:
   - Claude Code: "Multimodal content appears as arrays"
   - Codex: Base64 or URLs
   - OpenCode: Unknown
   - Should images be extracted/cached?
   - Privacy implications for remote URLs?

6. **Session Resumption**:
   - What's the command format for each tool?
   - `claude-code resume <session-id>`?
   - `codex resume <session-id>`?
   - `opencode resume <session-id>` or different command?
   - Need to verify for each tool

7. **OpenCode Parent-Child Session Display**:
   - Should subagent sessions be shown nested under parents?
   - Or displayed as separate sessions with parent reference?
   - How deep can nesting go?

8. **Error Handling for Malformed Data**:
   - How should parser handle malformed JSON/JSONL lines?
   - Skip and continue, or fail entire session?
   - What about missing required fields?
   - Recommendation: Log warnings, skip problematic entries, continue indexing

9. **Memory Management for Large Sessions**:
   - What's the practical limit for session size?
   - Should large messages be truncated for display?
   - How to handle sessions with 10,000+ messages?
   - Consider pagination or virtual scrolling in UI

---

## Next Steps for Design

1. **Inspect actual session files**:
   - Get real Claude Code session from `~/.claude/`
   - Get real Codex session from `~/.codex/`
   - Get real OpenCode sessions from `~/.local/share/opencode/storage/`
      - Inspect session metadata JSON
      - Examine message directory structure
      - Look at message parts format
      - Check session_diff format
   - Get real Mistral Vibe sessions from `~/.vibe/logs/session/`
      - Confirm `metadata` fields and `messages` structure

2. **Verify tool call format**:
   - How does Claude Code represent tool calls/results?
   - Confirm Codex format matches documentation
   - Investigate OpenCode message/part structure
   - Confirm Mistral Vibe pairing: `assistant.tool_calls[]` <-> `tool.tool_call_id`

3. **Define unified data model**:
   - Design `Session`, `Event`, `Message` structs
   - Handle different threading models/representations:
      - Tree structure (Claude Code: `uuid`/`parentUuid`)
      - Linear threading (Codex: `message_id`/`parent_id`)
      - Hierarchical sessions (OpenCode: session-level `parentID`)
      - Linear message list (Mistral Vibe: message roles, no per-message ids/timestamps)
   - Support multimodal content (text, images, code)
   - Handle JSONL events vs multi-file storage

4. **Database schema design**:
   - How to represent four different threading models/representations in unified schema?
   - Should OpenCode subagent sessions be separate rows or nested?
   - FTS5 indexing strategy for all four formats
   - Metadata normalization (different field names, different types)
   - How to handle OpenCode's multi-file structure (index messages separately?)

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
- [Codex Schema Reference](https://github.com/jazzyalex/agent-sessions/blob/main/docs/schemas/session_event.schema.json) (mentioned but not fetched)

### OpenCode Information Sources
- [Agent Sessions GitHub Repository](https://github.com/jazzyalex/agent-sessions) - Multi-tool session browser
- [OpenCode GitHub Repository](https://github.com/opencode-ai/opencode) - Official OpenCode repository
- [OpenCode Sessions Issue #3026](https://github.com/sst/opencode/issues/3026) - Storage structure details
- [OpenCode Sessions Issue #5734](https://github.com/sst/opencode/issues/5734) - Subagent session structure

### Mistral Vibe Information Sources
- [Mistral Vibe Configuration Docs](https://docs.mistral.ai/mistral-vibe/introduction/configuration) - `VIBE_HOME`, `config.toml` behavior
- [Mistral Vibe Repository](https://github.com/mistralai/mistral-vibe) - session logging implementation

### Key Findings Summary

- **Claude Code**: JSONL format, tree-structured events, project-based organization
- **Codex**: JSONL format, threaded messages, date-sharded storage
- **OpenCode**: Multi-file JSON format, session metadata + separate message directories, git-based project identification
- **Mistral Vibe**: Single JSON session log file containing `metadata` + OpenAI-style `messages` (including tool call/result pairing)

---

**Last Updated**: 2026-01-23
**Status**: Claude/Codex/OpenCode documented; Mistral Vibe session log format documented (session logs only; input history noted)
