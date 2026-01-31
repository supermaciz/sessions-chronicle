# OpenCode Parser Design

**Date:** 2026-01-26
**Status:** ✅ Implementation complete (automated validation passed)

## Overview

Add OpenCode session support to Sessions Chronicle through a two-phase implementation:

1. **Phase A:** Refactor Claude parser to combined `parse()` method
2. **Phase B:** Add OpenCode parser with same API pattern

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Parser API | Combined `parse()` → `(Session, Vec<Message>)` | Single file/directory read, cleaner indexer code |
| Claude refactor | Yes, do first | Establish pattern before adding OpenCode |
| Error handling | Best-effort with warnings | Partial data better than no data |
| Message detail | Text + tool summaries | Consistent with Claude, keeps transcripts readable |
| Subagent handling | Skip and prune | Keep session list clean with top-level sessions only |
| Empty sessions | Skip (require user messages) | Consistent with Claude behavior |
| Resume | Tool-aware command building | `opencode --session <id>` for OpenCode sessions |

## Out of Scope

- Subagent/child session display
- Diff/patch rendering
- OpenCode-specific UI features

## Phase A: Claude Parser Refactor

### Current API

```rust
impl ClaudeCodeParser {
    pub fn parse_metadata(&self, file_path: &Path) -> Result<Session>
    pub fn parse_messages(&self, file_path: &Path) -> Result<Vec<Message>>
}
```

### New API

```rust
impl ClaudeCodeParser {
    pub fn parse(&self, file_path: &Path) -> Result<(Session, Vec<Message>)>
}
```

### Implementation

Merge the two methods into one that:

1. Opens the file once with `BufReader`
2. Iterates through lines, collecting both metadata and messages in a single pass
3. Returns early with error if no user messages found
4. Returns `(Session, Vec<Message>)` tuple

### Indexer Update

```rust
// Before
let session = parser.parse_metadata(file_path)?;
let messages = parser.parse_messages(file_path)?;

// After
let (session, messages) = parser.parse(file_path)?;
```

## Phase B: OpenCode Parser

### Storage Layout

```
~/.local/share/opencode/storage/
├── session/<projectID>/<sessionID>.json    # Session metadata
├── message/<sessionID>/<messageID>.json    # Message info (role, timestamps)
└── part/<messageID>/<partID>.json          # Message content (text, tools, etc.)
```

### Parser Structure

```rust
pub struct OpenCodeParser {
    storage_root: PathBuf,
}

impl OpenCodeParser {
    pub fn new(storage_root: &Path) -> Self
    pub fn parse(&self, session_path: &Path) -> Result<(Session, Vec<Message>)>

    // Internal helpers
    fn parse_session_metadata(&self, path: &Path) -> Result<OpenCodeSessionMeta>
    fn load_messages(&self, session_id: &str) -> Result<Vec<Message>>
    fn load_parts(&self, message_id: &str) -> Vec<OpenCodePart>  // best-effort
}
```

### Session Metadata Fields

From `session/<projectID>/<sessionID>.json`:

- `id` - session ID
- `directory` - project path
- `time.created`, `time.updated` - timestamps (epoch milliseconds)
- `parentID` - if present, skip this session (subagent)

### Message Reconstruction

| Part type | Maps to | Content extraction |
|-----------|---------|-------------------|
| `text` | Same role as parent message | `content.text` field |
| `tool-invocation` | `Role::ToolCall` | `[Tool: {toolName}]` + input summary |
| `tool-result` | `Role::ToolResult` | `content.result` or `[Tool result cleared]` |
| `reasoning` | Skip | Not displayed |
| Other types | Skip with warning | Log and continue |

### Message Ordering

1. Load all messages for session, sort by `time.created`
2. For each message, load parts sorted by order/ID
3. Flatten: one OpenCode message may become multiple `Message` records
4. Assign sequential `index` values after flattening

### User Message Detection

A session has user input if any message has `role: "user"` with at least one non-ignored `text` part.

## Indexer Integration

### New Method

```rust
impl SessionIndexer {
    pub fn index_opencode_sessions(&mut self, storage_root: &Path) -> Result<usize>
}
```

### Discovery Logic

1. Walk `storage_root/session/` to find all `<projectID>/<sessionID>.json` files
2. For each session file:
   - Parse metadata
   - If `parentID` exists → skip and prune from DB
   - If no user messages → skip and prune
   - Otherwise → parse messages and upsert to DB

### Database Operations

```sql
-- Upsert session
INSERT OR REPLACE INTO sessions (id, tool, project_path, ...)
VALUES (?1, 'opencode', ?2, ...)

-- Refresh messages
DELETE FROM messages WHERE session_id = ?1
INSERT INTO messages ...
```

### App Startup

```rust
indexer.index_claude_sessions(&claude_dir)?;
indexer.index_opencode_sessions(&opencode_dir)?;
```

If OpenCode storage directory doesn't exist, treat as zero sessions (not an error).

## Resume Handling

### Commands by Tool

| Tool | Command |
|------|---------|
| Claude Code | `claude -r <session_id>` |
| OpenCode | `opencode --session <session_id>` |

### Implementation

```rust
pub fn build_resume_command(tool: Tool, session_id: &str) -> Vec<String> {
    match tool {
        Tool::ClaudeCode => vec!["claude".into(), "-r".into(), session_id.into()],
        Tool::OpenCode => vec!["opencode".into(), "--session".into(), session_id.into()],
        Tool::Codex => vec!["codex".into(), session_id.into()],
    }
}
```

### Files to Modify

- `src/utils/terminal.rs` - add `build_resume_command()`
- `src/ui/session_detail.rs` - pass `Tool` when triggering resume

## Testing Strategy

### Fixture Structure

```
tests/fixtures/opencode_storage/
├── session/
│   ├── project-a/
│   │   ├── session-001.json        # Normal session
│   │   └── session-002.json        # Subagent (has parentID)
│   └── global/
│       └── session-003.json        # Global session
├── message/
│   ├── session-001/
│   │   ├── msg-001.json            # User message
│   │   └── msg-002.json            # Assistant with tool
│   └── session-003/
│       └── msg-001.json
└── part/
    ├── msg-001/
    │   └── part-001.json           # Text part
    └── msg-002/
        ├── part-001.json           # Text part
        └── part-002.json           # Tool invocation
```

### Unit Tests

- `parse_session_metadata_extracts_fields`
- `parse_skips_subagent_sessions`
- `parse_skips_sessions_without_user_messages`
- `load_parts_handles_missing_files`
- `message_reconstruction_orders_correctly`

### Integration Test

- Index fixtures into temp DB, verify session count
- Search FTS for known text, verify hits from OpenCode sessions

## Implementation Order

1. Refactor Claude parser to combined method
2. Update indexer to use new Claude API
3. Add OpenCode parser with same pattern
4. Add `index_opencode_sessions()` to indexer
5. Wire into app startup
6. Update resume to be tool-aware
7. Add fixtures and tests

## Validation Checklist

- [x] `cargo fmt --all` - Clean
- [x] `cargo clippy` - 0 warnings
- [x] `cargo test` - 34 unit + 9 integration tests pass, 0 failures
- [x] Manual: OpenCode sessions appear under OpenCode filter
- [x] Manual: Session detail shows user/assistant text and tool entries
- [x] Manual: Search returns hits from OpenCode sessions
- [x] Manual: Resume uses `opencode --session <id>` for OpenCode sessions
