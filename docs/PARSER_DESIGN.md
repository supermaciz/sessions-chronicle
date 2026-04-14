# Parser Design Guide

Architecture and implementation patterns for Sessions Chronicle parsers.
See [SESSION_FORMAT_ANALYSIS.md](SESSION_FORMAT_ANALYSIS.md) for cross-assistant format comparison.

Per-assistant format details:
- [Claude Code](session-formats/claude-code.md)
- [Codex](session-formats/codex.md)
- [OpenCode](session-formats/opencode.md)
- [Mistral Vibe](session-formats/mistral-vibe.md)

---

## Recommended Architecture

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
struct OpenCodeParser;    // Shared parser core for JSON + SQLite backends
struct MistralVibeParser; // Directory-based session parser

impl SessionParser for ClaudeCodeParser { /* ... */ }
impl SessionParser for CodexParser { /* ... */ }
impl SessionParser for OpenCodeParser {
    // Special handling: supports dual-read backends
    // - SQLite backend (`opencode.db`) for current OpenCode sessions
    // - Legacy JSON backend (`storage/`) for compatibility
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
    let home = dirs::home_dir().unwrap_or_default();

    if path.starts_with(home.join(".claude")) {
        Box::new(ClaudeCodeParser)
    } else if path.starts_with(home.join(".codex")) {
        Box::new(CodexParser)
    } else if path.starts_with(home.join(".local/share/opencode")) {
        Box::new(OpenCodeParser)
    } else if path.starts_with(home.join(".vibe/logs/session")) {
        Box::new(MistralVibeParser)
    } else {
        // Try to detect from file structure
        detect_parser(path)
    }
}
```

---

## Title Extraction Strategy

| AI assistant | Logic |
|--------------|-------|
| **Claude Code** | First parsed `user` message content (assistant/system/summary are ignored by parser). |
| **Codex** | First `event_msg.payload.type == "user_message"` event (`payload.message`). |
| **OpenCode** | Prefer metadata `title` when available (SQLite), otherwise first flattened `text` part attached to a `user` message. |
| **Mistral Vibe** | First `messages.jsonl` entry where `role == "user"` and `content` is non-empty. |

---

## Timestamp Parsing

| AI assistant | Approach |
|--------------|----------|
| **Claude Code** | Track earliest/latest across `type in {user, assistant}` using per-event `timestamp` (ISO-8601). |
| **Codex** | `start_time` from first-line `session_meta.payload.timestamp`; `last_updated` from max `event.timestamp` seen in `event_msg` lines. |
| **OpenCode** | Session timestamps from metadata `time.created` + `time.updated` (ms epoch), with per-message `time.created` used for ordering. |
| **Mistral Vibe** | `start_time` from `meta.json.start_time`; `last_updated` from `meta.json.end_time` (fallback to `start_time`). |

---

## Content Extraction

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

---

## Streaming JSONL Files

**Do NOT load entire JSONL files into memory:**

```rust
// WRONG - loads entire file into RAM
let content = fs::read_to_string(file_path)?;
let lines: Vec<&str> = content.lines().collect();
for line in lines { /* parse */ }
```

**Use `BufReader` for line-by-line streaming:**

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

---

## Tool Call Handling

**Claude Code:**

- Raw data can appear in assistant `message.content[]` as `type == "tool_use"`
- Tool execution output is often represented in `system` events (`subtype == "local_command"`)
- Current parser behavior: indexes `tool_use` blocks and correlates `tool_result` blocks by
  `tool_use_id`; `system/local_command` payloads are still not normalized as tool calls

**Codex:**

- Raw data is emitted via `event_msg.payload.type` variants: `exec_command_*`, `mcp_tool_call_*`,
  `web_search_*`, and collab `collab_*`
- Tool call correlation typically uses `call_id`
- Current parser behavior: indexes `exec_command_*` and `mcp_tool_call_*` begin/end pairs as
  tool calls; `collab_*` events are still not mapped to subagent records

**OpenCode:**

- Tool calls are explicit `part.type == "tool"` records with lifecycle state
  (`pending`/`running`/`completed`/`error`)
- Delegated subagent work appears in two shapes: `part.type == "tool"` with
  `tool == "task"` (the full record, with child session ID in
  `state.metadata.sessionId`), and `part.type == "subtask"` (a lighter
  announcement, without child session link in most observed sessions)
- Current parser behavior: maps `tool == "task"` to subagent rows (title from
  `state.input.description`, `child_session_id` from `state.metadata.sessionId`);
  indexes other `tool` parts as tool calls; indexes `subtask` parts as subagent
  records but skips them when the session also contains any `tool == "task"` part
  (dedup); keeps `parentID` sessions as `is_subagent`

**Mistral Vibe:**

- Tool calls appear on assistant messages under `tool_calls[]`
- Tool outputs are separate messages with `role == "tool"` and `tool_call_id` matching the call id
- Arguments are stored as JSON-encoded strings (`tool_calls[*].function.arguments`)
- Current parser behavior: indexes assistant `tool_calls[]` entries and correlates
  `role == "tool"` outputs by `tool_call_id`; uncorrelated outputs are skipped

---

## OpenCode — Multi-File Reading (Legacy Path)

Still needed for pre-migration installs:

```rust
impl OpenCodeParser {
    fn parse_session(&self, session_path: &Path) -> Result<Session> {
        // 1. Read session metadata JSON
        let metadata = self.read_session_metadata(session_path)?;

        // 2. Construct message directory path
        let session_id = &metadata.id;
        let home = dirs::home_dir().unwrap_or_default();
        let msg_dir = home
            .join(".local/share/opencode/storage/message")
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

## OpenCode — SQLite Read Path (Implemented)

For sessions created with OpenCode ≥ 2026-02-14, Sessions Chronicle now uses dual-read indexing:

1. **Detect SQLite database** (`opencode.db`) from resolved sources
2. **Enumerate/index SQLite sessions first** via `SqliteBackend`
3. **Enumerate/index legacy JSON sessions** via `JsonBackend` when present
4. **Deduplicate by session `id`**, keeping SQLite data when both sources overlap

Implementation references:

- `src/parsers/opencode/sqlite_backend.rs`
- `src/parsers/opencode/json_backend.rs`
- `src/database/indexer.rs` (`index_opencode_sessions`)

---

## Session Metadata Extraction from File Path

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
# New (≥ 2026-02-14) — SQLite:
~/.local/share/opencode/opencode.db
   └── session table: id (ses_…), project_id (git hash), directory, title, …

# Legacy — JSON files:
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

---

## Error Handling Recommendations

- Log warnings for malformed JSON/JSONL lines
- Skip problematic entries and continue indexing
- Do not fail entire session on individual bad entries
- Warn on missing required fields; fall back to defaults where safe
- Handle missing message/part directories gracefully (OpenCode orphaned data)
