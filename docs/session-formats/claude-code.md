# Claude Code — Session Format Reference

Format reference for Claude Code session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-assistant comparison tables.

---

## Storage & File Naming

| Field   | Value |
|---------|-------|
| **Path** | `~/.claude/projects/<project-dir>/UUID.jsonl` (main session)<br>`~/.claude/projects/<project-dir>/<session-id>/subagents/agent-<id>.jsonl` (subagent transcript; documented upstream and confirmed locally)<br>`~/.claude/projects/<project-dir>/<session-id>/subagents/agent-<id>.meta.json` (subagent metadata sidecar; observed locally in v2.1.148)<br>`~/.claude/projects/<project-dir>/<session-id>/tool-results/<id>.<ext>` (materialized large tool output or attachment payloads; observed in current local sessions) |
| **Pattern** | `UUID.jsonl`, `agent-*.jsonl`, `agent-*.meta.json`, `tool-results/*` |
| **Example** | `a1b2c3d4-e5f6-7890-abcd-ef1234567890.jsonl`<br>`2a19bf71-3687-49ed-8ae9-8bd15e1522f6/subagents/agent-a60d695.jsonl` (legacy naming)<br>`66ae4ab6-e5ea-40f4-8e8f-fb80fd307472/subagents/agent-aimpl-task1-d4584135445167d0.jsonl` (teammate naming, v2.1.216+: `agent-a<name>-<hash16>.jsonl`)<br>`2a19bf71-3687-49ed-8ae9-8bd15e1522f6/subagents/agent-a60d695.meta.json`<br>`82b2d04e-d30e-4370-8e41-f53890baeda1/tool-results/bdw7vxszs.txt` |
| **Format** | JSONL (one JSON object per line, UTF-8, append-only) |

**Not session data:** since roughly v2.1.226 a sibling `~/.claude/projects/<project-dir>/memory/`
directory can exist, holding `MEMORY.md` plus one `*.md` file per stored memory.
It contains no transcript data. Sessions Chronicle's discovery filters on the
`.jsonl` extension (`crates/core/src/database/indexer.rs`), so these files are
ignored; any future discovery change must keep that filter.

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
  "type": "system",                // System events (see `system` subtypes below)
  "type": "file-history-snapshot", // File state tracking
  "type": "progress",              // Streaming/progress events
  "type": "queue-operation",       // Queue orchestration events
  "type": "saved_hook_context",    // Hook context snapshots
  "type": "pr-link",               // PR link events
  "type": "attachment",            // Hook output, skill/agent/MCP listings, command permissions (observed v2.1.148)
  "type": "permission-mode",       // Current permission-mode marker (observed v2.1.148)
  "type": "last-prompt",           // Last-prompt pointer / leaf marker (observed v2.1.148)
  "type": "mode",                  // UI/session mode marker (observed v2.1.216+)
  "type": "ai-title",              // AI-generated session title (observed v2.1.216+)
  "type": "file-history-delta",    // Incremental file backup record (observed v2.1.216+)
  "type": "atis-latch",            // Latch marker carrying an `atis` string (observed v2.1.226+)
  "type": "bridge-session",        // Remote Control / bridge session link (observed v2.1.226+)
  "type": "cost-state",            // Running session cost + per-model usage totals (observed v2.1.226+)
  "type": "agent-name"             // Session name on background sessions (observed v2.1.226+)
}
```

Observed `system.subtype` values (2026-09-06 local scan): `local_command`,
`turn_duration`, `compact_boundary`, `stop_hook_summary`, `away_summary`,
`informational`. The list is not guaranteed exhaustive.

Note (2026-09-06 local scan, v2.1.220–v2.1.263): no `summary` events were found
in any local session touched since 2026-08-01 (2269 events). This confirms and
widens the earlier 2026-07-27 observation (v2.1.216–v2.1.220): `ai-title` is the
current carrier of the generated session title. `summary` may still exist in
older transcripts and possibly on some code paths (unconfirmed — Claude Code is
closed source).

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
        "name": "Bash",
        "input": {
          "command": "ls -la"
        },
        "caller": {
          "type": "direct"
        }
      }
    ]
  }
}
```

### Assistant Subagent Launch Block (observed in current local sessions)

Claude Code subagent launches are currently emitted as `tool_use` blocks with
`name == "Agent"`. `Task` was the older tool name and remains an alias in some
settings/agent definitions, but recent local JSONL logs use `Agent` for
subagent launches.

```json
{
  "type": "assistant",
  "message": {
    "role": "assistant",
    "model": "claude-sonnet-4-6",
    "content": [
      {
        "type": "tool_use",
        "id": "toolu_01SEhtw5pj2qvZXjTufUZSyi",
        "name": "Agent",
        "input": {
          "description": "Explore project docs and current state",
          "subagent_type": "Explore",
          "name": "Explore",
          "run_in_background": false,
          "prompt": "..."
        },
        "caller": {
          "type": "direct"
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

### Turn Duration System Event (observed in current main-session logs)

```json
{
  "type": "system",
  "subtype": "turn_duration",
  "durationMs": 186669,
  "messageCount": 34,
  "timestamp": "2026-03-30T22:50:06.144Z"
}
```

### Compact Boundary System Event (observed in recent logs)

```json
{
  "type": "system",
  "subtype": "compact_boundary",
  "content": "Conversation compacted",
  "compactMetadata": {
    "trigger": "auto",
    "preTokens": 167363,
    "preCompactDiscoveredTools": ["AskUserQuestion", "TaskCreate", "TaskList", "TaskUpdate"]
  },
  "logicalParentUuid": "UUID"
}
```

### Attachment Event (observed in v2.1.148 local sessions)

`type: "attachment"` events carry side-band content injected into the
conversation: hook output, deferred-tool/agent/MCP listing deltas, skill
listings, and command permissions. The payload lives under `attachment`, whose
own `type` field selects the variant.

```json
{
  "type": "attachment",
  "attachment": {
    "type": "hook_success",
    "hookName": "SessionStart:startup",
    "hookEvent": "SessionStart",
    "toolUseID": "873dd14b-c4a0-47af-9ba2-82d049f4d42d",
    "content": "",
    "stdout": "..."
  },
  "uuid": "UUID",
  "parentUuid": "UUID",
  "isSidechain": false,
  "sessionId": "UUID",
  "version": "2.1.148",
  "timestamp": "ISO-8601",
  "cwd": "/path/to/project",
  "gitBranch": "main",
  "entrypoint": "cli",
  "userType": "external"
}
```

Observed `attachment.type` values through v2.1.148: `hook_success`,
`hook_additional_context`, `deferred_tools_delta`, `agent_listing_delta`,
`mcp_instructions_delta`, `skill_listing`, `command_permissions`.

Added by the 2026-09-06 local scan (v2.1.220–v2.1.263): `total_tokens_reminder`
(by far the most frequent — 166 of 321 attachment events), `task_reminder`,
`remote_session_change`, `session_context`, `auto_mode`, `prompt_snapshot`,
`compact_file_reference`, `instructions`, `environment`, `date`, `date_change`,
`model`, `file`, `edited_text_file`, `opened_file_in_ide`,
`hook_system_message`. The list is not guaranteed exhaustive.

### Permission Mode Event (observed in v2.1.148 local sessions)

```json
{
  "type": "permission-mode",
  "permissionMode": "default",
  "sessionId": "UUID"
}
```

### Last Prompt Event (observed in v2.1.148 local sessions)

A pointer/leaf marker for the most recent prompt. `lastPrompt` carries the
prompt text when present and is sometimes absent (leaf-only marker).

```json
{
  "type": "last-prompt",
  "lastPrompt": "/watching-ai-format-evolution pour claude code",
  "leafUuid": "UUID",
  "sessionId": "UUID"
}
```

### Mode Event (observed in v2.1.216+ local sessions)

A lightweight marker of the current session mode, similar in spirit to
`permission-mode`:

```json
{
  "type": "mode",
  "mode": "normal",
  "sessionId": "UUID"
}
```

### AI Title Event (observed in v2.1.216+ local sessions)

Carries the AI-generated session title. In recent local sessions this appears
to replace the older `summary` event (see note above). A session can contain
several `ai-title` events as the title is regenerated; the last one wins.

```json
{
  "type": "ai-title",
  "aiTitle": "Explorer UI alternatives pour sélection de plage de dates",
  "sessionId": "UUID"
}
```

### File History Delta Event (observed in v2.1.216+ local sessions)

Incremental companion to `file-history-snapshot`: records one tracked-file
backup, pointing back to the owning snapshot via `snapshotMessageId`.
`backup.backupFileName` can be `null`.

```json
{
  "type": "file-history-delta",
  "messageId": "UUID",
  "snapshotMessageId": "UUID",
  "trackingPath": "AGENTS.md",
  "backup": {
    "backupFileName": "bd059bbd578ebe86@v1",
    "version": 1,
    "backupTime": "ISO-8601",
    "realParentDir": "/path/to/project"
  },
  "timestamp": "ISO-8601"
}
```

### Atis Latch Event (observed in v2.1.226+ local sessions)

Purpose unknown. Every one of the 59 events in the 2026-09-06 local scan carried
an empty `atis` string, and no changelog entry names the field.

```json
{
  "type": "atis-latch",
  "atis": "",
  "sessionId": "UUID"
}
```

### Bridge Session Event (observed in v2.1.226+ local sessions)

Links a local transcript to a server-side "bridge" session. Correlates with the
changelog entry for v2.1.251 ("live streaming of a foreground subagent's tool
calls and results to Remote Control clients") and with the
`attachment.remote_session_change` payload.

```json
{
  "type": "bridge-session",
  "sessionId": "UUID",
  "bridgeSessionId": "cse_01Fw1jZQtU6xnWrBuTBkXxzq",
  "lastSequenceNum": 0,
  "ownerAccountUuid": "UUID",
  "ownerOrganizationUuid": "UUID"
}
```

`bridgeSessionId` uses a `cse_` prefix rather than a UUID. `ownerAccountUuid` and
`ownerOrganizationUuid` identify the account behind the bridge — treat them as
personal data if ever surfaced in the UI.

### Cost State Event (observed in v2.1.226+ local sessions)

Running aggregate of session cost, wall/API duration, edit line counts, and
per-model token usage. Emitted repeatedly within a session; two samples from the
same session differed only in `totalDuration`, suggesting periodic rewrite with
last-one-wins semantics (single-session observation, not generalized).

```json
{
  "type": "cost-state",
  "sessionId": "UUID",
  "totalCostUSD": 0.3153286,
  "totalAPIDuration": 17495,
  "totalAPIDurationWithoutRetries": 17480,
  "totalToolDuration": 298,
  "totalLinesAdded": 0,
  "totalLinesRemoved": 0,
  "totalDuration": 4462611,
  "startTime": 1787921917747,
  "modelUsage": {
    "claude-sonnet-5": {
      "inputTokens": 604,
      "outputTokens": 459,
      "cacheReadInputTokens": 247603,
      "cacheCreationInputTokens": 64384,
      "webSearchRequests": 0,
      "costUSD": 0.3128546
    }
  },
  "hasUnknownModelCost": false
}
```

Note the camelCase token field names here (`inputTokens`, `cacheReadInputTokens`)
versus the snake_case names used in `message.usage` (`input_tokens`,
`cache_read_input_tokens`). `startTime` is epoch milliseconds; the durations are
milliseconds. This is a per-session alternative to summing `message.usage`, and
avoids the append-only duplicate-request problem described under Token Usage.

### Agent Name Event (observed in v2.1.226+ local sessions)

Carries a session name. The single local sample appeared on a session with
`sessionKind: "bg"` (background session) that **also** contained an `ai-title`
event, so `agent-name` does not currently displace `ai-title` as the title
carrier. Whether it ever appears without `ai-title` is unconfirmed.

```json
{
  "type": "agent-name",
  "agentName": "Review Mistral AI job posting",
  "sessionId": "UUID"
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
    "messageId": "UUID",
    "trackedFileBackups": {},
    "timestamp": "ISO-8601"
  },
  "isSnapshotUpdate": false
}
```

Recent local sessions (v2.1.148) also include a top-level `isSnapshotUpdate`
boolean, and the nested `snapshot` object repeats `messageId`.

### Tool Results

- Tool results are still represented inline in user `message.content[]` via `type: "tool_result"` blocks.
- Recent local sessions also show a top-level `toolUseResult` object on some user events, duplicating stdout/stderr in structured form.
- Large tool outputs can be materialized to sibling files under `tool-results/` and then referenced by later `Read` tool calls.

### Meta Flag

- `isMeta: true` → Skip for title extraction (system-generated)
- `isMeta: false` → User-generated content

### Threading

Tree structure via `uuid`/`parentUuid` + `isSidechain` flag.
Sidechain/subagent context appears through `isSidechain` and parent links (`parentUuid`).
Some recent `system/compact_boundary` events also include `logicalParentUuid`.

### Nested Subagent Transcript Linkage

Claude Code stores subagent transcripts as nested files under:

`<session-id>/subagents/agent-<agentId>.jsonl`

The nested transcript reuses the parent `sessionId`. Sessions Chronicle therefore
derives a local child session ID from the parent session ID plus `agentId`, then
stores that derived ID in `Subagent.child_session_id` so the existing inspector
navigation can open the indexed child transcript.

**Legacy form (documented through ~v2.1.148):** the bridge from the parent
transcript to the nested file was the `agentId` token emitted inside the
`Agent`/`Task` `tool_result` text, e.g.:

```
Async agent launched successfully.
agentId: a41c0fb07beb52ed6
```

The parser captures that value onto the parent `Subagent.agent_id`, and the
indexer matches it against nested `agent-<agentId>.jsonl` files to populate
`child_session_id`.

#### Teammate Form (observed in v2.1.216–v2.1.220 local sessions)

Recent sessions (after subagents started running in the background by default,
changelog v2.1.198) use a "teammate" spawn shape that breaks the legacy bridge:

- The `tool_result` text uses snake_case `agent_id:` (never `agentId:`) and the
  value is `<name>@session-<short-parent-id>`, e.g.
  `agent_id: impl-task1@session-66ae4ab6`.
- The user event's structured top-level `toolUseResult` object carries the same
  data: `{agent_id, agent_type, color, is_splitpane, model, name,
  plan_mode_required, prompt, status: "teammate_spawned", team_name,
  teammate_id, tmux_pane_id, tmux_session_name, tmux_window_name}`.
- Nested transcript filenames are now `agent-a<name>-<hash16>.jsonl`, e.g.
  `agent-aimpl-task1-d4584135445167d0.jsonl`. The 16-hex suffix does not match
  any value observed in the parent transcript or the sidecar, so filename
  matching must go through the `<name>` segment.
- A 2026-07-27 scan of recent local sessions containing subagent transcripts
  found zero legacy `agentId:` tokens. Whether a non-teammate (foreground,
  unnamed) launch still emits the legacy token is unconfirmed.

The parser bridges this by storing the teammate `name` — the only value shared
by the parent transcript and the nested filename — on `Subagent.agent_name`.
The 16-hex suffix appears in no parent-side field and no sidecar field, so the
join is by name with an ambiguity guard rather than by id.

#### Subagent Metadata Sidecar

Each nested subagent transcript has a sibling `.meta.json` sidecar. The shape
changed between v2.1.148 and v2.1.216.

**v2.1.148 (legacy):**

```json
{
  "agentType": "Product Manager",
  "description": "Recommend next product step",
  "name": "pm-advisor",
  "toolUseId": "toolu_0125QgdpDHhnquARkmbV3VNc"
}
```

`toolUseId` linked the sidecar directly to the parent `tool_use` block.

**v2.1.216+ (teammate form):** `toolUseId` is gone. Observed shape:

```json
{
  "agentType": "impl-task1",
  "description": "Implement Task 1: localized labels",
  "name": "impl-task1",
  "spawnDepth": 0,
  "model": "sonnet",
  "taskKind": "in_process_teammate",
  "teamName": "session-66ae4ab6",
  "color": "blue",
  "planModeRequired": false,
  "permissionMode": "auto"
}
```

With `toolUseId` removed, `name` is the only observed reliable link back to the
parent `tool_use` block (`input.name`) and to the `toolUseResult.agent_id`
prefix (`<name>@session-...`). The parser does not consume this sidecar:
`name` is already available from the parent transcript's `input.name` and
`toolUseResult.name`, so reading it would add filesystem I/O and a new failure
mode without adding information.

---

## Metadata Available in Events

Rich per-event metadata:

| Field | Description |
|-------|-------------|
| `sessionId` | Unique session identifier |
| `cwd` | Working directory |
| `gitBranch` | Git branch name |
| `version` | Claude Code version |
| `promptId` | Prompt/turn identifier on user events |
| `message.model` | Assistant model slug (`claude-opus-4-6`, `claude-sonnet-4-5-20250929`, ...) |
| `userType` | `"external"` or other |
| `entrypoint` | Session entrypoint (`"cli"` observed locally) |
| `slug` | Human-friendly session slug present on many current events |
| `uuid` / `parentUuid` | Tree structure links |
| `logicalParentUuid` | Additional lineage link observed on compacted system events |
| `isSidechain` | Subagent/sidechain indicator |
| `agentId` | Present in subagent transcript events |
| `attributionSkill` | Skill slug attributed to an `assistant` event (observed v2.1.148) |
| `sourceToolAssistantUUID` | On some `user` events, links a tool-result event back to the originating `assistant` event (observed v2.1.148) |
| `effort` | Reasoning effort level on `assistant` events (`"medium"` observed); added upstream in v2.1.211 per changelog |
| `requestId` | API request identifier on `assistant` events (observed v2.1.220) |
| `session_id` | snake_case duplicate of `sessionId` on the same `assistant` events (observed v2.1.220) |
| `sessionKind` | Session flavor; only value observed is `"bg"` (background session), on `user`/`assistant`/`system`/`attachment` events (observed v2.1.226+). Absent on ordinary interactive sessions |
| `promptSource` | Origin of a `user` prompt (observed v2.1.226+): `typed`, `suggestion_accepted`, `system`, `sdk` |
| `attributionAgent` | Subagent display name attributed to an `assistant` event, e.g. `"UI Designer"` (observed v2.1.226+); sibling of `attributionSkill` |
| `attributionMcpServer` / `attributionMcpTool` | MCP server and tool attributed to an `assistant` event (observed v2.1.226+) |
| `apiBlockIndex` | Index of the content block within an API response, on `assistant` events (observed v2.1.226+) |
| `turnCompanion` | Companion marker on a small number of events (observed v2.1.226+; purpose unconfirmed) |
| `origin` | Event origin marker (observed v2.1.226+; value set not yet enumerated) |
| `lastSequenceNum` | Bridge stream sequence position on `bridge-session` events (observed v2.1.226+) |

The attribution family (`attributionSkill`, `attributionAgent`,
`attributionMcpServer`, `attributionMcpTool`) marks *why* an assistant event
happened. In the 2026-09-06 local scan these were mutually exclusive per event
except for one pair where `attributionSkill` and `attributionMcpTool` co-occurred,
so do not assume at most one attribution field per event.

**Model metadata:** Recent Claude Code logs include a stable structured field at
`message.model` on `assistant` events.

- Local sample (2026-02-24): `message.model` present on all sampled `assistant` events,
  absent on sampled `user` events.
- Local sample refresh (2026-03-31): `message.model` still present on sampled `assistant`
  events in v2.1.87 main-session and subagent logs.
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
- Anthropic-style cache accounting keeps `cache_read_input_tokens` and `cache_creation_input_tokens`
  separate from `input_tokens`, so `input_tokens` represents the uncached input portion.
- Recent local sessions also include richer nested usage metadata such as `cache_creation`,
  `server_tool_use`, `service_tier`, `inference_geo`, `iterations`, and `speed`.
- The log is append-only and can contain multiple assistant events for the same underlying request; if you
  aggregate tokens, deduplicate by a stable identifier such as `requestId` + `message.id` and keep the last
  (or max) `usage` record per request.

---

## Parser Behavior (Sessions Chronicle)

Current implementation: `crates/core/src/parsers/claude_code.rs`

- Indexes `type == user|assistant` text content
- Extracts `tool_use` blocks as tool calls; current implementation maps both
  `Agent` and legacy `Task` tool uses to subagent records
- Correlates `tool_result` blocks by `tool_use_id`
- Does not currently normalize `system/local_command` events into tool calls
- Does not currently persist/index `message.model` in Sessions Chronicle database schema
- Dispatches on `type in {user, assistant, ai-title}` with a catch-all `_ => {}`
  arm; every other event type — including `attachment`, `permission-mode`,
  `last-prompt`, `mode`, `file-history-delta`, and the newer `atis-latch`,
  `bridge-session`, `cost-state`, and `agent-name` events — falls through and is
  skipped without error (no parser breakage from these additions)
- Recent real Claude Code sessions (local scan refreshed 2026-04-13, versions
  observed through v2.1.100) show subagent launches as `name == "Agent"` with
  `input.description`, `input.subagent_type`, and `input.prompt`; optional
  fields include `input.name`, `input.run_in_background`, `input.team_name`,
  and `input.mode`.
- **Teammate linkage (implemented 2026-07-28):** the parser stores the
  `Agent`/`Task` `input.name` (falling back to `toolUseResult.name`) on
  `Subagent.agent_name`, and the indexer pairs it with nested
  `agent-a<name>-<hash16>.jsonl` transcripts by name, in either indexing
  order. A name that is ambiguous on either side is left unlinked rather than
  linked to the wrong transcript. The legacy `agentId:` path is unchanged.
- **Synthetic-only sessions (implemented 2026-07-31):** an assistant event is
  synthetic when it carries `isApiErrorMessage: true` (e.g. spend limit, 429) or
  `message.model == "<synthetic>"` — both mean the CLI generated the text
  locally and no model response happened. A session whose assistant events are
  *all* synthetic is rejected with `NoRealAssistantMessages` and pruned from the
  index; a session with no assistant event at all (freshly started) stays
  indexable, and synthetic messages remain visible in the transcript of sessions
  that also contain real responses.

**Title extraction:** The last non-blank `ai-title` event wins; otherwise the
first parsed `user` message content is used (assistant/system/summary are
ignored). `agent-name` is **not** consumed — the one local sample carrying it
also carried `ai-title`, so no fallback is justified yet.

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
- Current local samples use PascalCase tool names such as `Read`, `Edit`, `Bash`, and `Agent`.
- `Agent` is the current Claude Code subagent launch tool name. Treat exact
  `Task` as a legacy subagent launch alias, not as equivalent to task-list tools
  such as `TaskCreate`, `TaskUpdate`, or `TaskList`.
- Tool execution output is commonly observable in `system` events (`subtype: "local_command"`)
  with `command`, `stdout`, `stderr` fields.
- Tool result payloads may also be duplicated in a top-level `toolUseResult` object on the user event.
- Current parser behavior: indexes `tool_use` blocks and correlates `tool_result` blocks by
  `tool_use_id`; `system/local_command` payloads are not yet normalized as tool calls.

**Streaming:** Use `BufReader` line-by-line iteration — do not load entire JSONL into memory.

---

## References

- [Anthropic prompt caching docs (`cache_read_input_tokens`, `cache_creation_input_tokens`, `input_tokens`)](https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching)
- [Claude Code subagents docs (subagent transcript path)](https://docs.anthropic.com/en/docs/claude-code/sub-agents)
- [Claude Code changelog (compaction/transcript behavior changes)](https://docs.anthropic.com/en/docs/claude-code/changelog)
- [Claude Code model configuration (support article)](https://support.claude.com/en/articles/11940350-claude-code-model-configuration)
- [Claude Sonnet 4.6 page (`claude-sonnet-4-6`)](https://www.anthropic.com/claude/sonnet)
- [Claude Opus 4.6 page (`claude-opus-4-6`)](https://www.anthropic.com/claude/opus)
- [Claude Code issue: duplicate entries in session logs](https://github.com/anthropics/claude-code/issues/1524)
