# OpenCode — Session Format Reference

Format reference for OpenCode session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-assistant comparison tables.

> **⚠️ Breaking change (≥ 2026-02-14):** OpenCode migrated to SQLite.
> New sessions are written to SQLite databases in `~/.local/share/opencode/`.
> The default filename is `opencode.db`; non-default channels use `opencode-<channel>.db`.
> The legacy JSON file tree is retained but no longer the write path.

---

## Storage

| Format | Path |
|--------|------|
| **New (≥ 2026-02-14)** | `~/.local/share/opencode/opencode.db` (default/latest/beta channels, SQLite WAL mode); `~/.local/share/opencode/opencode-<channel>.db` for other channels |
| **Legacy (pre-migration)** | `~/.local/share/opencode/storage/` (JSON files, retained post-migration) |

**Legacy file naming:** `ses_*.json` — e.g. `ses_66a71b6f4ffeq796jvvOpJQ04m.json`

---

## New Format — SQLite (`opencode.db` / `opencode-<channel>.db`)

### Schema

```sql
-- Sessions
CREATE TABLE session (
  id TEXT PRIMARY KEY,              -- "ses_" prefix
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  workspace_id TEXT,
  parent_id TEXT,                   -- set for child/subagent sessions
  slug TEXT NOT NULL,
  directory TEXT NOT NULL,
  path TEXT,
  title TEXT NOT NULL,
  version TEXT NOT NULL,
  share_url TEXT,
  summary_additions INTEGER,
  summary_deletions INTEGER,
  summary_files INTEGER,
  summary_diffs TEXT,               -- JSON: FileDiff[]
  revert TEXT,                      -- JSON: {messageID, partID?, snapshot?, diff?}
  permission TEXT,                  -- JSON: PermissionNext.Ruleset
  metadata TEXT,                    -- JSON: Record<string, unknown>
  cost REAL NOT NULL DEFAULT 0,     -- session-level cost
  tokens_input INTEGER NOT NULL DEFAULT 0,
  tokens_output INTEGER NOT NULL DEFAULT 0,
  tokens_reasoning INTEGER NOT NULL DEFAULT 0,
  tokens_cache_read INTEGER NOT NULL DEFAULT 0,
  tokens_cache_write INTEGER NOT NULL DEFAULT 0,
  agent TEXT,                       -- agent name (e.g. "build", "plan", subagent name)
  model TEXT,                       -- JSON: {id, providerID, variant?}
  time_created INTEGER NOT NULL,    -- Unix ms
  time_updated INTEGER NOT NULL,    -- Unix ms
  time_compacting INTEGER,
  time_archived INTEGER
);

-- Messages (data blob holds the full MessageV2.Info minus id/sessionID)
CREATE TABLE message (
  id TEXT PRIMARY KEY,              -- "msg_" prefix
  session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  time_created INTEGER NOT NULL,
  time_updated INTEGER NOT NULL,
  data TEXT NOT NULL                -- JSON blob (role, model, agent, tokens, cost, …)
);

-- Parts (data blob holds the full Part minus id/sessionID/messageID)
CREATE TABLE part (
  id TEXT PRIMARY KEY,              -- "prt_" prefix (new; old JSON files used "part_")
  message_id TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  session_id TEXT NOT NULL,
  time_created INTEGER NOT NULL,
  time_updated INTEGER NOT NULL,
  data TEXT NOT NULL                -- JSON blob (type, text/tool state/subtask/…)
);

-- Projects
CREATE TABLE project (
  id TEXT PRIMARY KEY,              -- git first-commit hash, or "global"
  worktree TEXT NOT NULL,
  vcs TEXT,                         -- "git" or null
  name TEXT,
  icon_url TEXT, icon_color TEXT,
  time_created INTEGER NOT NULL,    -- Unix ms
  time_updated INTEGER NOT NULL,    -- Unix ms
  time_initialized INTEGER,
  sandboxes TEXT NOT NULL,          -- JSON: string[]
  commands TEXT                     -- JSON: {start?: string}
);

-- Companion tables in the same DB: todo, permission, session_share,
--   session_entry, workspace, project, event, event_sequence, account,
--   account_state, control_account
-- (__drizzle_migrations tracks applied schema migrations)

-- Newer tables observed upstream (dev) alongside the V1 message/part tables:
--   session_message   -- id, session_id, type, seq, data (a newer message model:
--                        user/assistant/system/shell/synthetic/compaction/
--                        agent-switched/model-switched)
--   session_input     -- id, session_id, prompt, delivery, admitted_seq, promoted_seq
--   session_context_epoch -- session_id, baseline, snapshot, baseline_seq
-- As of the 2026-09-06 watch pass, message/part remain the transcript write
-- path (message-v2.ts is still active); session_message is a parallel model
-- whose write path is not yet confirmed.
```

### Session Object Fields

```typescript
{
  id: string           // "ses_" prefix
  slug: string
  projectID: string    // git first-commit hash, or "global"
  workspaceID?: string
  directory: string
  parentID?: string    // child/subagent sessions
  title: string
  version: string
  share?: { url: string }
  summary?: { additions, deletions, files, diffs?: FileDiff[] }
  // ⚠️ v1.4.0 SDK break: FileDiff no longer carries `to`/`from` full-file
  // contents; only `patch` (unified diff). Applies to session `summary.diffs`,
  // user-message `summary.diffs`, and edit/patch tool metadata.
  revert?: { messageID, partID?, snapshot?, diff? }
  permission?: PermissionRuleset
  time: { created, updated, compacting?, archived? }  // Unix ms
}
```

Newer fields vs previous docs: `slug`, `share`, `permission`, `revert`, `workspaceID`,
`time.compacting`, `time.archived`, `summary.diffs`.

Additional session-level columns observed upstream (dev, 2026-09-06): `metadata`
(JSON), `cost` (real), `tokens_input` / `tokens_output` / `tokens_reasoning` /
`tokens_cache_read` / `tokens_cache_write` (integer, session-level token
aggregation), `agent` (text), and `model` (JSON `{id, providerID, variant?}`).
These are session-level rollups and do not change the per-message model
metadata described below.

### Message `data` Blob — User

```typescript
{
  role: "user"
  agent: string
  model: { providerID: string; modelID: string; variant?: string }
  format?: { type: "text" } | { type: "json_schema", schema, retryCount? }
  summary?: { title?, body?, diffs: FileDiff[] }
  system?: string
  tools?: Record<string, boolean>
}
// ⚠️ v1.4.0 SDK break: `variant` moved from top-level into `model.variant`.
// Sessions written by OpenCode ≥ 1.4.0 use the nested shape.
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

### Token Usage Notes

- `tokens` is commonly present on assistant messages but is **not guaranteed** (provider/backends can omit it).
- The `part` stream can also include token usage on step boundaries:
  - `part.type == "step-finish"` includes `tokens` (and `cost`)
- If both message-level `tokens` and `step-finish.tokens` exist, avoid double-counting when aggregating.
- `tokens.cache.read` / `tokens.cache.write` are structurally separate, but whether `tokens.input`
  already includes cached tokens is provider-dependent rather than guaranteed by OpenCode itself.

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
| `compaction` | `auto: boolean`, `overflow?` |
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

Official upstream tool states are `pending`, `running`, `completed`, and
`error`. Sessions Chronicle's current OpenCode parser maps `completed`,
`running`, and `error` explicitly; `pending` currently falls through to an
unknown status in the local model.

### Task Tool — Subagent Delegation

OpenCode `task` is a concrete tool name inside a `tool` part, not a generic
todo/task-list marker. The official task tool creates or resumes a child
session and stores the child session ID in tool state metadata.

**Primary marker:**

```json
{
  "type": "tool",
  "tool": "task",
  "callID": "call_xxx",
  "state": {
    "status": "completed",
    "input": {
      "description": "Explore parser layout",
      "prompt": "Find all parser files",
      "subagent_type": "explore",
      "task_id": "ses_existing_child"
    },
    "title": "Explore parser layout",
    "metadata": {
      "sessionId": "ses_child123",
      "model": {
        "providerID": "anthropic",
        "modelID": "claude-sonnet-4-5"
      },
      "truncated": false
    },
    "output": "task_id: ses_child123 (for resuming to continue this task if needed)\n\n<task_result>\n...\n</task_result>",
    "time": { "start": 1776000000000, "end": 1776000010000 }
  }
}
```

**Observed semantics:**

- Detect delegated subagent work with `json_extract(part.data, '$.type') = 'tool'`
  and `json_extract(part.data, '$.tool') = 'task'`
- `$.state.input.description` is the short task title
- `$.state.input.prompt` is the task prompt sent to the subagent
- `$.state.input.subagent_type` identifies the requested OpenCode agent type
- `$.state.input.task_id` is optional and requests resuming an existing child session
- `$.state.metadata.sessionId` is the child session ID produced or resumed by the task tool
- The child session is also present in the `session` table with `parent_id`
  pointing to the parent session
- Local SQLite scan refreshed 2026-04-13: `tool == "task"` appeared 628 times;
  616 completed, 10 errored, 2 running; every child session in that DB was
  referenced by at least one task tool via `state.metadata.sessionId`

Do not collapse this with `part.type == "subtask"`. `subtask` is a distinct
part schema with `prompt`, `description`, `agent`, optional `model`, and optional
`command`, while `tool == "task"` is a tool lifecycle record with `state`,
`callID`, and child-session metadata.

### Skill Invocation Marker

OpenCode skill loading is not just implied by Markdown headings in user text.
It has a native `tool` part marker that should be treated as the primary
detection signal.

**Primary marker:**

```json
{
  "type": "tool",
  "tool": "skill",
  "state": {
    "status": "completed",
    "input": { "name": "brainstorming" },
    "metadata": {
      "name": "brainstorming",
      "dir": "/home/user/.config/opencode/skills/superpowers/brainstorming",
      "truncated": false
    },
    "title": "Loaded skill: brainstorming"
  }
}
```

**Observed semantics:**

- Detect skill loading with `json_extract(part.data, '$.type') = 'tool'` and
  `json_extract(part.data, '$.tool') = 'skill'`
- Extract the skill name from `$.state.metadata.name`, fallback
  `$.state.input.name`
- `$.state.metadata.dir` identifies the loaded skill directory
- `$.state.title` is a readable summary such as `Loaded skill: brainstorming`
- The assistant message carrying this part links back to the source user
  message via `message.data.parentID`
- That parent user message often contains the injected skill markdown as a
  `text` part; treat it as payload for display, not as the proof that a skill
  was invoked
- One assistant reply can load multiple skills, so grouping should not assume
  exactly one skill per user message

### Useful SQLite Queries

Open the database read-only:

```bash
sqlite3 'file:~/.local/share/opencode/opencode.db?mode=ro&immutable=1'
```

For non-default channels, replace `opencode.db` with `opencode-<channel>.db`.

Find sessions containing at least one tool part with `state.status == "error"`:

```sql
SELECT
  s.id,
  s.title,
  s.directory,
  COUNT(*) AS error_parts,
  datetime(MAX(p.time_created) / 1000, 'unixepoch') AS last_error_utc
FROM part p
JOIN session s ON s.id = p.session_id
WHERE json_extract(p.data, '$.type') = 'tool'
  AND json_extract(p.data, '$.state.status') = 'error'
GROUP BY s.id, s.title, s.directory
ORDER BY MAX(p.time_created) DESC;
```

Find recent skill invocations:

```sql
SELECT
  p.session_id,
  p.message_id,
  json_extract(p.data, '$.state.metadata.name') AS skill_name,
  json_extract(p.data, '$.state.metadata.dir') AS skill_dir,
  json_extract(p.data, '$.state.title') AS title,
  datetime(p.time_created / 1000, 'unixepoch') AS created_utc
FROM part p
WHERE json_extract(p.data, '$.type') = 'tool'
  AND lower(json_extract(p.data, '$.tool')) = 'skill'
ORDER BY p.time_created DESC
LIMIT 50;
```

Find recent task-tool subagent invocations:

```sql
SELECT
  p.session_id AS parent_session_id,
  json_extract(p.data, '$.state.metadata.sessionId') AS child_session_id,
  json_extract(p.data, '$.state.status') AS status,
  json_extract(p.data, '$.state.input.subagent_type') AS subagent_type,
  json_extract(p.data, '$.state.title') AS title,
  datetime(p.time_created / 1000, 'unixepoch') AS created_utc
FROM part p
WHERE json_extract(p.data, '$.type') = 'tool'
  AND json_extract(p.data, '$.tool') = 'task'
ORDER BY p.time_created DESC
LIMIT 50;
```

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

This is a distinct part type from `tool == "task"`. Use the task tool marker
when you need the child session link (`state.metadata.sessionId`).

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

Model metadata is primarily message-scoped, unchanged between SQLite and JSON formats:

| Message role | Fields |
|--------------|--------|
| User | `model.providerID` + `model.modelID` |
| Assistant | top-level `providerID` + `modelID` |
| Subtask parts | optional `model.providerID`, `model.modelID` for delegated model |

Since ~2026 the `session` table also carries optional session-level `model`
(JSON `{id, providerID, variant?}`) and `agent` (text) columns. These are
session-level rollups (e.g. the model/agent used to create the session) and
do not replace the per-message fields, which remain the source of truth for
individual turns.

---

## Subagent Sessions

- Child sessions spawned through task tools or agent mentions
- Identified by presence of `parentID` field (both formats)
- In SQLite task-tool records, `part.data.state.metadata.sessionId` points to
  the child session created or resumed by `tool == "task"`
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

- New primary storage is a SQLite WAL-mode database in `~/.local/share/opencode/`
- Default/latest/beta channels use `opencode.db`; other channels use `opencode-<channel>.db`
- On first run after upgrade, OpenCode runs a one-time `JsonMigration` that reads all JSON files
  from `storage/` and imports them into SQLite
- The `storage/` directory is **not deleted** after migration; legacy files remain on disk
- Part ID prefix changed from `part_` to `prt_` in the SQLite era
- Deduplicate by session `id` when both paths return data

---

## Parser Behavior (Sessions Chronicle)

Current implementation:

- Parser core: `crates/core/src/parsers/opencode/mod.rs`
- JSON backend: `crates/core/src/parsers/opencode/json_backend.rs`
- SQLite backend: `crates/core/src/parsers/opencode/sqlite_backend.rs`
- Indexer orchestration: `crates/core/src/database/indexer.rs` (`index_opencode_sessions`)

Current indexing strategy is **SQLite-first dual-read with JSON fallback**:

1. If an OpenCode SQLite database is available, list/parse sessions from SQLite first
2. Also enumerate legacy JSON storage when present
3. Deduplicate by session `id` (SQLite wins on overlap)
4. Support JSON-only and SQLite-only installs

- Indexes sessions with `parentID` as subagent sessions (`is_subagent = 1`)
- Converts `part.type == text` into transcript messages
- Extracts `part.type == tool` into indexed tool-call records, with the exception
  that `tool == "task"` is mapped to subagent records (title from
  `state.input.description`, prompt from `state.input.prompt`, `child_session_id`
  from `state.metadata.sessionId`, result from `state.output`)
- Official upstream also emits `state.status == "pending"`; the current local
  parser does not map that status explicitly and falls back to `Unknown`
- Extracts `part.type == subtask` into subagent records; when a session also
  contains any `tool == "task"` part, `subtask` records are skipped to avoid
  duplication (legacy sessions without `tool == "task"` continue to use `subtask`)
- Non-message parts like `reasoning`, `step-start`, `step-finish`, `snapshot`, `compaction`, `file`, `agent`, `retry`, and `patch` are currently not rendered as transcript messages
- Current official schema also includes optional `workspaceID` on sessions and optional `overflow` on `compaction` parts

**Title extraction:** Prefer session metadata `title` (SQLite `session.title`) when present;
fallback to first flattened `text` part attached to a `user` message.

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

**SQLite read path:** Implemented via `SqliteBackend` (`rusqlite` read-only connection).
SQLite rows deserialize `message.data` and `part.data` JSON blobs and feed the same parser core
used by the JSON backend.

---

## Primary Sources

> **Note:** OpenCode's session/parser sources were reorganized into the
> `packages/core` (`@opencode-ai/core`) and `packages/schema` packages; the
> `packages/opencode/src/session/` paths below were valid before that move.
> The URLs listed here reflect the current (`dev`) locations.

- [OpenCode GitHub Repository](https://github.com/anomalyco/opencode)
- [OpenCode `MessageV2` part schemas](https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/message-v2.ts)
- [OpenCode SQLite Drizzle schema (`session`/`message`/`part`/`session_message`/`session_input`/`session_context_epoch`)](https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/sql.ts)
- [OpenCode V1 part/session types (`Part`, `ToolState`, `Info`, …)](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/v1/session.ts)
- [OpenCode newer message model (`session_message` types)](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/session-message.ts)
- [OpenCode task tool (creates child sessions with `parentID`)](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/task.ts)
- [OpenCode Sessions Issue #3026](https://github.com/anomalyco/opencode/issues/3026)
- [OpenCode Sessions Issue #5734](https://github.com/anomalyco/opencode/issues/5734)
