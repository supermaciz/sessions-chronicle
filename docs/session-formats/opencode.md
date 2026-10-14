# OpenCode — Session Format Reference

Format reference for OpenCode session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-assistant comparison tables.

> **⚠️ Breaking change (OpenCode v2, confirmed 2026-10-14):** OpenCode's primary
> session storage moved from the V1 `session` / `message` / `part` tables to
> `session_v2` / `session_message`. In v2, parts are embedded in each message's
> `content` array instead of living in a separate `part` table, and a tool part
> identifies its tool with `name` (not `tool`). V1 tables are retained on
> migrated databases as a legacy read model but are no longer written.
> Earlier breaking change (≥ 2026-02-14): the JSON file tree was replaced by
> SQLite.

---

## Storage

| Format | Path |
|--------|------|
| **Current (OpenCode v2, confirmed 2026-10-14)** | `~/.local/share/opencode/opencode.db` (default/latest/beta channels, SQLite WAL mode); `~/.local/share/opencode/opencode-<channel>.db` for other channels. Primary tables: `session_v2`, `session_message`. |
| **SQLite V1 (≥ 2026-02-14, legacy read model under v2)** | Same database files. Tables: `session`, `message`, `part`. Last written by OpenCode v1; retained after the v2 migration. |
| **Legacy (pre-migration)** | `~/.local/share/opencode/storage/` (JSON files, retained post-migration) |

**Legacy JSON file naming:** `ses_*.json` — e.g. `ses_66a71b6f4ffeq796jvvOpJQ04m.json`

---

## Current Format — SQLite V2 (`session_v2` / `session_message`)

Confirmed on 2026-10-14 from upstream `dev`
(`packages/core/src/session/sql.ts`, `packages/schema/src/session-message.ts`,
`packages/schema/src/session.ts`) and from a local OpenCode v2.0.29 database.

### Schema

```sql
-- Sessions
CREATE TABLE session_v2 (
  id TEXT PRIMARY KEY,              -- "ses_" prefix
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  workspace_id TEXT,
  parent_id TEXT,                   -- set for child/subagent sessions
  fork_session_id TEXT,             -- session this one was forked from
  fork_boundary TEXT,               -- JSON: Session.ForkBoundary
  slug TEXT NOT NULL,
  directory TEXT NOT NULL,
  path TEXT,
  title TEXT,                       -- nullable in v2 (was NOT NULL in v1)
  version TEXT NOT NULL,
  share_url TEXT,
  summary_additions INTEGER,
  summary_deletions INTEGER,
  summary_files INTEGER,
  summary_diffs TEXT,               -- JSON: FileDiff[]
  metadata TEXT,                    -- JSON: Record<string, unknown>
  cost REAL NOT NULL DEFAULT 0,     -- session-level rollup
  tokens_input INTEGER NOT NULL DEFAULT 0,
  tokens_output INTEGER NOT NULL DEFAULT 0,
  tokens_reasoning INTEGER NOT NULL DEFAULT 0,
  tokens_cache_read INTEGER NOT NULL DEFAULT 0,
  tokens_cache_write INTEGER NOT NULL DEFAULT 0,
  revert TEXT,                      -- JSON: Session.Revert | RevertV1
  permission TEXT,                  -- JSON: Permission.Ruleset
  agent TEXT,                       -- agent name (e.g. "build", "plan")
  model TEXT,                       -- JSON: {id, providerID, variant?}
  time_created INTEGER NOT NULL,    -- Unix ms
  time_updated INTEGER NOT NULL,
  time_compacting INTEGER,
  time_archived INTEGER,
  time_suspended INTEGER,           -- execution claim timestamp (historical name)
  resume_attempts INTEGER NOT NULL DEFAULT 0,
  time_idle INTEGER,
  time_viewed INTEGER,
  idle_outcome TEXT                 -- "succeeded" | "failed" | "interrupted"
);

-- Messages (data holds the type-specific SessionMessage payload)
CREATE TABLE session_message (
  id TEXT PRIMARY KEY,              -- "msg_" prefix
  session_id TEXT NOT NULL REFERENCES session_v2(id) ON DELETE CASCADE,
  type TEXT NOT NULL,               -- see "Message types" below
  seq INTEGER NOT NULL,             -- monotonic per session (unique with session_id)
  time_created INTEGER NOT NULL,
  time_updated INTEGER NOT NULL,
  data TEXT NOT NULL                -- JSON payload; assistant content is embedded here
);
```

V2 companion tables in the same database: `session_pending`, `session_inbox`,
`instruction_state`, `instruction_blob`, `instruction_entry`, `project`,
`project_directory`, `worktree`, `permission`, `session_share`, `todo`,
`credential`, `kv`, `event`, `event_sequence`, `account`, `account_state`,
`control_account` (`__drizzle_migrations` / `migration` track applied schema
migrations).

> The `session_input` and `session_context_epoch` tables documented by the
> 2026-09-06 watch pass no longer exist upstream: queued input moved to
> `session_pending` / `session_inbox`, and context epochs to `instruction_state`.

### Session Object Fields

```typescript
{
  id: string           // "ses_" prefix
  parentID?: string    // child/subagent sessions
  fork?: { sessionID: string; boundary: ForkBoundary }
  projectID: string    // git first-commit hash, or "global"
  agent?: string
  model?: { id: string; providerID: string; variant?: string }
  cost: number
  tokens: TokenUsage
  outcome?: "succeeded" | "failed" | "interrupted"   // set once a run reaches terminal
  time: { created, updated, idle?, viewed?, archived? }  // Unix ms
  title?: string       // optional in v2
  location: Location.Ref
  subpath?: string
  metadata?: object
  permissions?: PermissionRuleset   // evaluated after agent rules; last match wins
  revert?: Revert
}
```

> Note: the `Session.Info` type uses `location` / `subpath`, while the persisted
> `session_v2` table stores `directory` / `path` / `workspace_id`.

### Message types

`session_message.type` is a tagged union:

| `type` | Key `data` fields |
|--------|-------------------|
| `user` | `text`, `files`, `agents`, `skills` |
| `assistant` | `agent`, `model`, `content[]`, `snapshot?`, `finish?`, `rawFinish?`, `providerState?`, `cost?`, `tokens?`, `error?`, `retry?` |
| `synthetic` | `text`, `description?`, `metadata?` (used for subagent completion) |
| `system` | `text`, `description?` |
| `skill` | `skill`, `name`, `text` |
| `shell` | `shellID`, `command`, `status`, `exit?`, `output?` |
| `agent-switched` | `agent`, `previous?` |
| `model-switched` | `model`, `previous?` |
| `location-switched` | `location`, `projectID?`, `subpath?`, `previous?` |
| `compaction` | `status`, `reason`, `summary`, `recent`; model/providerState/providerContext when completed, `error` when failed |
| `idle` | `outcome`, `error?` |

Every message also carries `id`, `metadata?`, and `time.created`.

Locally observed types in active use: `user`, `assistant`, `system`,
`synthetic`, `compaction`.

### Assistant `content[]` (the V2 equivalent of parts)

Assistant messages embed their parts inline. `AssistantContent` has only three
variants:

| Content `type` | Key fields |
|----------------|------------|
| `text` | `text`, `state?` |
| `reasoning` | `text`, `state?`, `time?` |
| `tool` | `id` (e.g. `call_…`), `name`, `executed?`, `providerState?`, `providerResultState?`, `state`, `time` |

The tool name is the `name` field — **not** the V1 `tool` field. The V1
`part` table's wider type set (`step-start`, `step-finish`, `snapshot`,
`patch`, `file`, `agent`, `retry`, `compaction`, `subtask`) is not represented as
V2 `content` items; step usage/snapshots are carried on the assistant message
itself (`tokens`, `cost`, `snapshot`).

### Tool Part — Lifecycle State Machine

```
streaming → running → completed
                    → error
```

| State | Fields |
|-------|--------|
| `streaming` | `input` (raw string) |
| `running` | `input`, `metadata` |
| `completed` | `input`, `content`, optional `metadata` |
| `error` | `input`, `error`, optional `content`, optional `metadata` |

V2 replaced V1's `pending` state with `streaming`, and a completed tool result is
stored in `state.content` (V1 used `state.output`). Locally observed v2 statuses
are `running`, `completed`, and `error`.

Example:

```json
{
  "type": "tool",
  "id": "call_00_h4aJzXEelUD3DiPH9wAJ3388",
  "name": "shell",
  "executed": false,
  "state": {
    "status": "completed",
    "input": { "command": "ls -la" },
    "content": [{ "type": "text", "text": "..." }],
    "metadata": {}
  },
  "time": { "created": 1776000000000, "completed": 1776000010000 }
}
```

### Task Tool — Subagent Delegation

`task` remains a concrete tool name (locally observed ~990 times under v2), but
in V2 it is an `AssistantContent` tool item with `name == "task"` rather than a
V1 `part` row.

**Primary marker (v2):**

```json
{
  "type": "tool",
  "id": "call_xxx",
  "name": "task",
  "state": {
    "status": "completed",
    "input": {
      "description": "Explore parser layout",
      "prompt": "Find all parser files",
      "subagent_type": "explore",
      "task_id": "ses_existing_child"
    },
    "metadata": { "sessionId": "ses_child123" },
    "content": [{ "type": "text", "text": "<task_result>…</task_result>" }]
  }
}
```

**Observed semantics:**

- Detect delegated subagent work inside an assistant message by scanning
  `$.content[]` for `type == "tool"` and `name == "task"`.
- `$.state.input.description` is the short task title.
- `$.state.input.prompt` is the task prompt sent to the subagent.
- `$.state.input.subagent_type` identifies the requested OpenCode agent type.
- `$.state.input.task_id` is optional and requests resuming an existing child session.
- `$.state.metadata.sessionId` is the child session ID produced or resumed by the task tool.
- The child session is also present in `session_v2` with `parent_id` pointing to
  the parent session.
- The completed result text moved from V1 `$.state.output` to `$.state.content`
  (V2 `content` array).

**Secondary marker — subagent completion message:**

Upstream also emits a `synthetic` message when a subagent finishes, carrying a
deterministic child link in its metadata:

```json
{
  "type": "synthetic",
  "text": "<subagent sessionID=\"ses_child123\" state=\"completed\" description=\"Explore parser layout\">\n…\n</subagent>",
  "metadata": {
    "source": "subagent",
    "childID": "ses_child123",
    "agent": "explore",
    "state": "completed"
  }
}
```

This `metadata.source == "subagent"` message (`packages/core/src/session/subagent-completion.ts`)
is the V2 analogue of the V1 `tool == "task"` child-session link.

Do not collapse this with `part.type == "subtask"` from the V1 format. `subtask`
was a distinct V1 part schema with `prompt`, `description`, `agent`, optional
`model`, and optional `command`; it has no direct V2 `content` equivalent.

### Skill Invocation Marker

`skill` remains a concrete tool name (locally observed ~2409 times under v2); in
V2 it is an `AssistantContent` tool item with `name == "skill"`.

**Primary marker (v2):**

```json
{
  "type": "tool",
  "id": "call_yyy",
  "name": "skill",
  "state": {
    "status": "completed",
    "input": { "name": "brainstorming" },
    "metadata": {
      "name": "brainstorming",
      "dir": "/home/user/.config/opencode/skills/superpowers/brainstorming"
    }
  }
}
```

**Observed semantics:**

- Detect skill loading by scanning assistant `$.content[]` for `type == "tool"`
  and `name == "skill"`.
- Extract the skill name from `$.state.metadata.name`, falling back to
  `$.state.input.name`.
- `$.state.metadata.dir` identifies the loaded skill directory.
- The injected skill Markdown also appears as a `skills[]` entry on the `user`
  message (or inline in the skill content); treat it as payload for display, not
  as the proof that a skill was invoked.
- One assistant reply can load multiple skills, so grouping should not assume
  exactly one skill per user message.

### Useful SQLite Queries

Open the database read-only:

```bash
sqlite3 'file:~/.local/share/opencode/opencode.db?mode=ro'
```

For non-default channels, replace `opencode.db` with `opencode-<channel>.db`.

Find sessions containing at least one tool with `state.status == "error"`:

```sql
SELECT
  s.id,
  s.title,
  s.directory,
  COUNT(*) AS error_tools,
  datetime(MAX(m.time_created) / 1000, 'unixepoch') AS last_error_utc
FROM session_message m
JOIN session_v2 s ON s.id = m.session_id,
     json_each(m.data, '$.content') c
WHERE m.type = 'assistant'
  AND json_extract(c.value, '$.type') = 'tool'
  AND json_extract(c.value, '$.state.status') = 'error'
GROUP BY s.id, s.title, s.directory
ORDER BY MAX(m.time_created) DESC;
```

Find recent skill invocations:

```sql
SELECT
  m.session_id,
  m.id AS message_id,
  json_extract(c.value, '$.name') AS tool_name,
  json_extract(c.value, '$.state.metadata.name') AS skill_name,
  datetime(m.time_created / 1000, 'unixepoch') AS created_utc
FROM session_message m, json_each(m.data, '$.content') c
WHERE m.type = 'assistant'
  AND json_extract(c.value, '$.type') = 'tool'
  AND lower(json_extract(c.value, '$.name')) = 'skill'
ORDER BY m.time_created DESC
LIMIT 50;
```

Find recent task-tool subagent invocations:

```sql
SELECT
  m.session_id AS parent_session_id,
  json_extract(c.value, '$.state.metadata.sessionId') AS child_session_id,
  json_extract(c.value, '$.state.status') AS status,
  json_extract(c.value, '$.state.input.subagent_type') AS subagent_type,
  json_extract(c.value, '$.state.input.description') AS description,
  datetime(m.time_created / 1000, 'unixepoch') AS created_utc
FROM session_message m, json_each(m.data, '$.content') c
WHERE m.type = 'assistant'
  AND json_extract(c.value, '$.type') = 'tool'
  AND json_extract(c.value, '$.name') = 'task'
ORDER BY m.time_created DESC
LIMIT 50;
```

Find subagent completion markers:

```sql
SELECT
  m.session_id AS parent_session_id,
  json_extract(m.data, '$.metadata.childID') AS child_session_id,
  json_extract(m.data, '$.metadata.agent') AS agent,
  json_extract(m.data, '$.metadata.state') AS state,
  datetime(m.time_created / 1000, 'unixepoch') AS created_utc
FROM session_message m
WHERE m.type = 'synthetic'
  AND json_extract(m.data, '$.metadata.source') = 'subagent'
ORDER BY m.time_created DESC
LIMIT 50;
```

---

## Legacy Format — SQLite V1 (`session` / `message` / `part`)

This was the primary SQLite write path from ≥ 2026-02-14 until the v2 upgrade.
Under OpenCode v2 these tables are retained as a read model but are no longer
written. They are still relevant for pre-v2 sessions and for compatibility.

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
  cost REAL NOT NULL DEFAULT 0,
  tokens_input INTEGER NOT NULL DEFAULT 0,
  tokens_output INTEGER NOT NULL DEFAULT 0,
  tokens_reasoning INTEGER NOT NULL DEFAULT 0,
  tokens_cache_read INTEGER NOT NULL DEFAULT 0,
  tokens_cache_write INTEGER NOT NULL DEFAULT 0,
  agent TEXT,
  model TEXT,                       -- JSON: {id, providerID, variant?}
  time_created INTEGER NOT NULL,    -- Unix ms
  time_updated INTEGER NOT NULL,
  time_compacting INTEGER,
  time_archived INTEGER
);

-- Messages (data blob holds the full MessageV2.Info minus id/sessionID)
CREATE TABLE message (
  id TEXT PRIMARY KEY,              -- "msg_" prefix
  session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  time_created INTEGER NOT NULL,
  time_updated INTEGER NOT NULL,
  data TEXT NOT NULL
);

-- Parts (data blob holds the full Part minus id/sessionID/messageID)
CREATE TABLE part (
  id TEXT PRIMARY KEY,              -- "prt_" prefix
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
  time_updated INTEGER NOT NULL,
  time_initialized INTEGER,
  sandboxes TEXT NOT NULL,          -- JSON: string[]
  commands TEXT                     -- JSON: {start?: string}
);
```

### Message `data` Blob — User (V1)

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
```

### Message `data` Blob — Assistant (V1)

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

### Part Types (V1, 12 total)

`type` field in `part.data`:

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

V1 tool lifecycle: `pending → running → completed | error`, with the tool name in
`data.tool` and the result in `state.output`.

### Token Usage Notes (V1)

- `tokens` is commonly present on assistant messages but is **not guaranteed**
  (provider/backends can omit it).
- The `part` stream can also include token usage on step boundaries
  (`part.type == "step-finish"` includes `tokens` and `cost`).
- If both message-level `tokens` and `step-finish.tokens` exist, avoid
  double-counting when aggregating.
- `tokens.cache.read` / `tokens.cache.write` are structurally separate, but
  whether `tokens.input` already includes cached tokens is provider-dependent.

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

### Subtask Part (records delegated work in the parent session)

This is a distinct V1 part type from `tool == "task"`. Use the task tool marker
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

Model metadata is primarily message-scoped in both V1 and V2:

| Message role | Fields |
|--------------|--------|
| User (V1) | `model.providerID` + `model.modelID` |
| Assistant (V1) | top-level `providerID` + `modelID` |
| Assistant (V2) | `model: { id, providerID, variant? }` |
| Subtask parts (V1) | optional `model.providerID`, `model.modelID` for delegated model |

Both `session` (V1) and `session_v2` also carry optional session-level `model`
(JSON `{id, providerID, variant?}`) and `agent` (text) columns. These are
session-level rollups (the model/agent used to create the session) and do not
replace the per-message fields, which remain the source of truth for individual
turns.

---

## Subagent Sessions

- Child sessions spawned through task tools or agent mentions.
- Identified by the presence of `parent_id` (SQLite) / `parentID` (JSON).
- V2: the parent-side link is either `AssistantContent` tool `name == "task"`
  with `state.metadata.sessionId`, or a `synthetic` message with
  `metadata.source == "subagent"` and `metadata.childID`.
- V1: `part.data.state.metadata.sessionId` points to the child session created or
  resumed by `tool == "task"`.
- Form hierarchical parent-child relationships.
- Can accumulate without cleanup (known limitation).

---

## Project Identification

- Uses git root commit hash as `projectID`.
- Command: `git rev-list --max-parents=0 --all`.
- Sessions grouped by project:
  - JSON: under `session/<projectID>/`
  - SQLite: `project` table with `id` (hash), `worktree`, `vcs`, `name`, etc.;
    `project_directory` and `worktree` map projects to directories.

---

## Storage Migration Timeline

- **≥ 2026-02-14** — JSON file tree replaced by SQLite WAL-mode database in
  `~/.local/share/opencode/`. Default/latest/beta channels use `opencode.db`;
  other channels use `opencode-<channel>.db`.
- On first run after upgrade, OpenCode runs a one-time `JsonMigration` that
  imports JSON files into SQLite. The `storage/` directory is **not deleted**;
  legacy files remain on disk.
- Part ID prefix changed from `part_` to `prt_` in the SQLite era.
- **OpenCode v2 (confirmed 2026-10-14)** — the primary write path moved to
  `session_v2` / `session_message`. V1 `session` / `message` / `part` tables are
  retained on migrated databases but stop being written after the v2 upgrade.
  Deduplicate by session `id` when both paths return data.

### V1 → V2 structural changes

- **Table rename:** `session` → `session_v2`, and `message` + `part` →
  `session_message` (parts are embedded in `content[]`).
- **New session columns:** `fork_session_id`, `fork_boundary`,
  `time_suspended`, `resume_attempts`, `time_idle`, `time_viewed`,
  `idle_outcome`; `title` became nullable.
- **New message columns:** `type`, `seq`, plus `time_created` / `time_updated`.
- **New message `type` union:** `agent-switched`, `model-switched`,
  `location-switched`, `user`, `synthetic`, `system`, `skill`, `shell`,
  `assistant`, `compaction`, `idle`.
- **Content shrinks to 3 types:** `text`, `reasoning`, `tool` (was 12 part types).
- **Tool name field:** `tool` → `name`; tool call id is the content item `id`.
- **Tool state:** `pending` → `streaming`; completed output `state.output` →
  `state.content`.
- **Subagent linkage:** V1 task-tool metadata plus a V2 `synthetic`
  `metadata.source == "subagent"` completion message.
- **Input/context tables:** `session_input` / `session_context_epoch` →
  `session_pending` / `session_inbox` / `instruction_state`.

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

> **⚠️ V2 gap (confirmed 2026-10-14):** the SQLite backend still reads only the
> V1 tables — `SELECT id FROM session` for discovery and `message` / `part` for
> the transcript. It does not read `session_v2` / `session_message`, so sessions
> written after the v2 upgrade are not discovered, and part/tool extraction
> assumes V1 rows (`.tool`, `.state.output`, `pending`). This is tracked as a
> known limitation, not a documented contract.

Current behavior:

- Indexes sessions with `parentID` as subagent sessions (`is_subagent = 1`)
- Converts `part.type == text` into transcript messages
- Extracts `part.type == tool` into indexed tool-call records, with the exception
  that `tool == "task"` is mapped to subagent records (title from
  `state.input.description`, prompt from `state.input.prompt`, `child_session_id`
  from `state.metadata.sessionId`, result from `state.output`)
- Maps tool state `pending`, `running`, `completed`, and `failed`/`error`;
  unknown statuses fall back to `Unknown` (V2 `streaming` currently falls here)
- Extracts `part.type == subtask` into subagent records; when a session also
  contains any `tool == "task"` part, `subtask` records are skipped to avoid
  duplication (legacy sessions without `tool == "task"` continue to use `subtask`)
- Non-message parts like `reasoning`, `step-start`, `step-finish`, `snapshot`,
  `compaction`, `file`, `agent`, `retry`, and `patch` are currently not rendered
  as transcript messages

**Title extraction:** Prefer session metadata `title` (SQLite `session.title`
when present) when present; fallback to first flattened `text` part attached to a
`user` message.

**Timestamp parsing:** Session timestamps from metadata `time.created` +
`time.updated` (ms epoch), with per-message `time.created` used for ordering.

**SQLite read path:** Implemented via `SqliteBackend` (`rusqlite` read-only
connection). SQLite rows deserialize `message.data` and `part.data` JSON blobs
and feed the same parser core used by the JSON backend.

---

## Primary Sources

> **Note:** OpenCode's session/parser sources live in the `packages/core`
> (`@opencode-ai/core`) and `packages/schema` packages. The
> `packages/opencode/src/session/` paths were valid before that move. URLs below
> reflect current (`dev`) locations.

- [OpenCode GitHub Repository](https://github.com/anomalyco/opencode)
- [OpenCode SQLite Drizzle schema (`session_v2`, `session_message`, `session_pending`, `session_inbox`, `instruction_*`)](https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/sql.ts)
- [OpenCode V2 message/part types (`SessionMessage`, `AssistantContent`, `ToolState`)](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/session-message.ts)
- [OpenCode V2 session type (`Session.Info`)](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/session.ts)
- [OpenCode V2 subagent completion marker](https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/subagent-completion.ts)
- [OpenCode V1 part/session types (`Part`, `ToolState`, `Info`, …)](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/v1/session.ts)
- [OpenCode task tool (creates child sessions with `parentID`)](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/task.ts)
- [OpenCode Sessions Issue #3026](https://github.com/anomalyco/opencode/issues/3026)
- [OpenCode Sessions Issue #5734](https://github.com/anomalyco/opencode/issues/5734)
