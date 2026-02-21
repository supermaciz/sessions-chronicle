# OpenCode — Session Format Reference

Format reference for OpenCode session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-tool comparison tables.

> **⚠️ Breaking change (≥ 2026-02-14):** OpenCode migrated to SQLite.
> New sessions are written to `opencode.db` — the legacy JSON file tree is retained but no longer the write path.

---

## Storage

| Format | Path |
|--------|------|
| **New (≥ 2026-02-14)** | `~/.local/share/opencode/opencode.db` (SQLite WAL mode) |
| **Legacy (pre-migration)** | `~/.local/share/opencode/storage/` (JSON files, retained post-migration) |

**Legacy file naming:** `ses_*.json` — e.g. `ses_66a71b6f4ffeq796jvvOpJQ04m.json`

---

## New Format — SQLite (`opencode.db`)

### Schema

```sql
-- Sessions
CREATE TABLE session (
  id TEXT PRIMARY KEY,              -- "ses_" prefix
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  parent_id TEXT,                   -- set for child/subagent sessions
  slug TEXT NOT NULL,
  directory TEXT NOT NULL,
  title TEXT NOT NULL,
  version TEXT NOT NULL,
  share_url TEXT,
  summary_additions INTEGER,
  summary_deletions INTEGER,
  summary_files INTEGER,
  summary_diffs TEXT,               -- JSON: FileDiff[]
  revert TEXT,                      -- JSON: {messageID, partID?, snapshot?, diff?}
  permission TEXT,                  -- JSON: PermissionNext.Ruleset
  created_at INTEGER,               -- Unix ms
  updated_at INTEGER,               -- Unix ms
  time_compacting INTEGER,
  time_archived INTEGER
);

-- Messages (data blob holds the full MessageV2.Info minus id/sessionID)
CREATE TABLE message (
  id TEXT PRIMARY KEY,              -- "msg_" prefix
  session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  created_at INTEGER,
  updated_at INTEGER,
  data TEXT NOT NULL                -- JSON blob (role, model, agent, tokens, cost, …)
);

-- Parts (data blob holds the full Part minus id/sessionID/messageID)
CREATE TABLE part (
  id TEXT PRIMARY KEY,              -- "prt_" prefix (new; old JSON files used "part_")
  message_id TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  session_id TEXT NOT NULL,
  created_at INTEGER,
  updated_at INTEGER,
  data TEXT NOT NULL                -- JSON blob (type, text/tool state/subtask/…)
);

-- Projects
CREATE TABLE project (
  id TEXT PRIMARY KEY,              -- git first-commit hash, or "global"
  worktree TEXT NOT NULL,
  vcs TEXT,                         -- "git" or null
  name TEXT,
  icon_url TEXT, icon_color TEXT,
  created_at DATETIME, updated_at DATETIME,
  time_initialized INTEGER,
  sandboxes TEXT NOT NULL,          -- JSON: string[]
  commands TEXT                     -- JSON: {start?: string}
);

-- Todos, permissions, session shares also stored in the same DB
```

### Session Object Fields

```typescript
{
  id: string           // "ses_" prefix
  slug: string
  projectID: string    // git first-commit hash, or "global"
  directory: string
  parentID?: string    // child/subagent sessions
  title: string
  version: string
  share?: { url: string }
  summary?: { additions, deletions, files, diffs?: FileDiff[] }
  revert?: { messageID, partID?, snapshot?, diff? }
  permission?: PermissionRuleset
  time: { created, updated, compacting?, archived? }  // Unix ms
}
```

New fields vs previous docs: `slug`, `share`, `permission`, `revert`, `time.compacting`,
`time.archived`, `summary.diffs`.

### Message `data` Blob — User

```typescript
{
  role: "user"
  agent: string
  model: { providerID: string; modelID: string }
  format?: { type: "text" } | { type: "json_schema", schema, retryCount? }
  summary?: { title?, body?, diffs: FileDiff[] }
  system?: string
  tools?: Record<string, boolean>
  variant?: string
}
```

### Message `data` Blob — Assistant

```typescript
{
  role: "assistant"
  parentID: string      // user message ID this responds to
  modelID: string
  providerID: string
  agent: string
  mode: string          // @deprecated
  path: { cwd: string; root: string }
  cost: number
  tokens: { total?, input, output, reasoning, cache: { read, write } }
  summary?: boolean     // true if compaction summary
  error?: ProviderAuthError | UnknownError | MessageOutputLengthError | …
  finish?: string
  structured?: unknown
  variant?: string
}
```

### Part Types (12 total)

`type` field in `data` blob:

| Part type | Key `data` fields |
|-----------|-------------------|
| `text` | `text`, `synthetic?`, `ignored?`, `time?`, `metadata?` |
| `reasoning` | `text`, `time`, `metadata?` |
| `file` | `mime`, `filename?`, `url`, `source?` (FileSource \| SymbolSource \| ResourceSource) |
| `tool` | `callID`, `tool`, `metadata?`, `state` (pending/running/completed/error) |
| `step-start` | `snapshot?` |
| `step-finish` | `reason`, `snapshot?`, `cost`, `tokens` |
| `snapshot` | `snapshot` (git tree hash) |
| `patch` | `hash`, `files: string[]` |
| `agent` | `name`, `source?` |
| `retry` | `attempt`, `error`, `time.created` |
| `compaction` | `auto: boolean` |
| `subtask` | `prompt`, `description`, `agent`, `model?`, `command?` |

New part types vs previous docs: `file`, `agent`, `retry`, `patch` (added).
Part ID prefix changed from `part_` to `prt_`.

### Tool Part — Lifecycle State Machine

```
pending  → running → completed
                   → error
```

State fields:

| State | Fields |
|-------|--------|
| `pending` | `input`, `raw` |
| `running` | `input`, optional `title`/`metadata`, `time.start` |
| `completed` | `input`, `output`, `title`, `metadata`, `time.start/end`, optional `attachments` |
| `error` | `input`, `error`, optional `metadata`, `time.start/end` |

---

## Legacy Format — JSON Files (pre-2026-02-14)

**Storage structure:**

```
~/.local/share/opencode/storage/
├── session/<projectID>/ses_xxx.json     # Session metadata
├── message/ses_xxx/                      # Message metadata files
├── part/msg_xxx/                         # Message parts (text/tool/subtask/etc.)
└── session_diff/ses_xxx.json            # File change tracking
```

**Path encoding:**

```
~/.local/share/opencode/storage/session/abc123def456/ses_xxx.json
                                        └─────────┘  └──────┘
                                        Project ID   Session ID
                                        (git root commit hash)
```

### Session Metadata File (`session/<projectID>/ses_xxx.json`)

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

Key fields:

- `id`: Unique session identifier (format: `ses_<identifier>`)
- `version`: OpenCode version
- `projectID`: Git root commit hash (used for project identification)
- `directory`: Working directory path
- `title`: Session title/description
- `time.created` / `time.updated`: Unix epoch milliseconds
- `parentID`: Optional — present only for subagent sessions (spawned via task tools)

### Message File (`message/<sessionID>/msg_xxx.json`)

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

### Tool Part (`part/<messageID>/part_xxx.json`)

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

### Subtask Part (records delegated work in parent session)

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

---

## Model Metadata

Model metadata is message-scoped (not session-scoped), unchanged between SQLite and JSON formats:

| Message role | Fields |
|--------------|--------|
| User | `model.providerID` + `model.modelID` |
| Assistant | top-level `providerID` + `modelID` |
| Subtask parts | optional `model.providerID`, `model.modelID` for delegated model |

---

## Subagent Sessions

- Child sessions spawned through task tools or agent mentions
- Identified by presence of `parentID` field (both formats)
- Form hierarchical parent-child relationships
- Can accumulate without cleanup (known limitation)

---

## Project Identification

- Uses git root commit hash as `projectID`
- Command: `git rev-list --max-parents=0 --all`
- Sessions grouped by project:
  - JSON: under `session/<projectID>/`
  - SQLite: `project` table with `id` (hash), `worktree`, `vcs`, `name`, etc.

---

## Storage Migration (≥ 2026-02-14)

- New primary storage is `opencode.db` (SQLite WAL mode)
- On first run after upgrade, OpenCode runs a one-time `JsonMigration` that reads all JSON files
  from `storage/` and imports them into SQLite
- The `storage/` directory is **not deleted** after migration; legacy files remain on disk
- Part ID prefix changed from `part_` to `prt_` in the SQLite era
- Deduplicate by session `id` when both paths return data

---

## Parser Behavior (Sessions Chronicle)

Current implementation: `src/parsers/opencode.rs`

**⚠️ Only reads the legacy JSON file tree** (`storage/session/`, `storage/message/`, `storage/part/`).
New sessions written to `opencode.db` (≥ 2026-02-14) are **not indexed**.

- Indexes sessions with `parentID` as subagent sessions (`is_subagent = 1`)
- Converts `part.type == text` into transcript messages
- Extracts `part.type == tool` into indexed tool-call records and `part.type == subtask` into subagent records
- Non-message parts like `reasoning`, `step-start`, `step-finish`, `snapshot`, `compaction`, `file`, `agent`, `retry`, and `patch` are currently not rendered as transcript messages

**Title extraction:** First flattened `text` part attached to a `user` message
(session metadata `title` is currently not indexed).

**Timestamp parsing:** Session timestamps from metadata `time.created` + `time.updated` (ms epoch),
with per-message `time.created` used for ordering.

**Content extraction:**

```rust
fn extract_opencode_text_part(part: &Value) -> Option<String> {
    if part.get("type")?.as_str()? != "text" {
        return None;
    }
    part.get("text")?.as_str().map(|s| s.to_string())
}
```

**SQLite read path (not yet implemented):**

To support the new format, a dual-read strategy is needed:

1. **Detect SQLite database**: check for `opencode.db` at `~/.local/share/opencode/opencode.db`
2. **If present**: open with `rusqlite` and query `session`, `message`, `part` tables;
   deserialize `data` JSON blobs
3. **If absent (older install)**: fall back to the existing multi-file JSON reader
4. **Dedup on migration overlap**: deduplicate by session `id`

`rusqlite` is already a project dependency.

---

## Primary Sources

- [OpenCode GitHub Repository](https://github.com/sst/opencode)
- [OpenCode `MessageV2` part schemas](https://github.com/sst/opencode/blob/dev/packages/opencode/src/session/message-v2.ts)
- [OpenCode task tool (creates child sessions with `parentID`)](https://github.com/sst/opencode/blob/dev/packages/opencode/src/tool/task.ts)
- [OpenCode generated v2 SDK types](https://github.com/sst/opencode/blob/dev/packages/sdk/js/src/v2/gen/types.gen.ts)
- [OpenCode session schema](https://github.com/sst/opencode/blob/dev/packages/opencode/src/session/index.ts)
- [OpenCode Sessions Issue #3026](https://github.com/sst/opencode/issues/3026)
- [OpenCode Sessions Issue #5734](https://github.com/sst/opencode/issues/5734)
