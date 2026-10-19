# Claude Code — Session Format Reference

Format reference for Claude Code session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-assistant comparison tables.

---

## Storage & File Naming

| Field   | Value |
|---------|-------|
| **Path** | `~/.claude/projects/<project-dir>/UUID.jsonl` (main session)<br>`~/.claude/projects/<project-dir>/<session-id>/subagents/agent-<id>.jsonl` (subagent transcript; documented upstream and confirmed locally)<br>`~/.claude/projects/<project-dir>/<session-id>/subagents/agent-<id>.meta.json` (subagent metadata sidecar; observed locally in v2.1.148)<br>`~/.claude/projects/<project-dir>/<session-id>/tool-results/<id>.<ext>` (materialized large tool output or attachment payloads; observed in current local sessions)<br>`~/.claude/projects/<project-dir>/<session-id>/ccr-tip.json` (non-session sidecar; observed locally in v2.1.300+)<br>`~/.claude/projects/<project-dir>/bridge-pointer.json` (non-session sidecar; observed locally in v2.1.300+) |
| **Pattern** | `UUID.jsonl`, `agent-*.jsonl`, `agent-*.meta.json`, `tool-results/*`, `ccr-tip.json`, `bridge-pointer.json` |
| **Example** | `a1b2c3d4-e5f6-7890-abcd-ef1234567890.jsonl`<br>`2a19bf71-3687-49ed-8ae9-8bd15e1522f6/subagents/agent-a60d695.jsonl` (legacy naming)<br>`66ae4ab6-e5ea-40f4-8e8f-fb80fd307472/subagents/agent-aimpl-task1-d4584135445167d0.jsonl` (teammate naming, v2.1.216+: `agent-a<name>-<hash16>.jsonl`)<br>`2a19bf71-3687-49ed-8ae9-8bd15e1522f6/subagents/agent-a60d695.meta.json`<br>`82b2d04e-d30e-4370-8e41-f53890baeda1/tool-results/bdw7vxszs.txt` |
| **Format** | JSONL (one JSON object per line, UTF-8, append-only) |

**Not session data:** since roughly v2.1.226 a sibling `~/.claude/projects/<project-dir>/memory/`
directory can exist, holding `MEMORY.md` plus one `*.md` file per stored memory.
It contains no transcript data. Sessions Chronicle's discovery filters on the
`.jsonl` extension (`crates/core/src/database/indexer.rs`), so these files are
ignored; any future discovery change must keep that filter.

**Not session data either (2026-10-18 local scan, v2.1.300+):** two small JSON
sidecars appeared next to transcripts. Both are skipped by the `.jsonl` filter.

| File | Observed keys | Notes |
|------|---------------|-------|
| `<project-dir>/bridge-pointer.json` | `sessionId`, `environmentId`, `source`, `pid`, `procStart` | Points a project at a live Remote Control / bridge process; holds a PID, so it is transient process state |
| `<project-dir>/<session-id>/ccr-tip.json` | `eventId`, `updatedAt` | Tip (last event) pointer for a cloud / Remote Control session |

Both were seen on a single project; purpose is inferred from key names only.

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
  "type": "agent-name",            // Session name on background sessions (observed v2.1.226+)
  "type": "api-request",           // Recorded API request: params + references (v2.1.300+)
  "type": "api-request-shape",     // Recorded system prompt + tools, keyed by hash (v2.1.300+)
  "type": "api-request-blob"       // Recorded request message, keyed by hash (v2.1.300+)
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

Added by the 2026-10-18 local scan (v2.1.264–v2.1.302, 2414 attachment events):
`deferred_tools_record` (559 events; keys `entries`, `toolInputCopies`),
`credential_org` (`organizationUuid` — personal data), `silent_turn_reminder`
(`text`), `read_truncation_notice` (`banner`, `toolUseID`), `queued_command`
(`commandMode`, `prompt`, `source_uuid`, `timestamp`), and
`workflow_size_guideline_change` (`size`). `total_tokens_reminder` remains the
most frequent variant (1098 events).

The same scan shows new top-level keys on `attachment` events describing how
the attachment is rendered to the model: `rendered`, `renderedRole`
(`system` or `user`), `renderedBesideToolResult`, and `renderedInHumanTurn`.

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

### API Request Recording Events (v2.1.300+)

Confirmed upstream by the v2.1.300 changelog entry: "Changed local session
transcripts to also record each API request's system prompt, tools and
parameters". Locally, the three event types first appear in v2.1.300 and are
present in every v2.1.301+ session sampled on 2026-10-18.

The recording is content-addressed: an `api-request` event references a
deduplicated `api-request-shape` (system prompt + tool definitions) through
`shapeHash`, and request messages are stored once as `api-request-blob` events
keyed by `hash`. All three can carry `agentId` when the request came from a
subagent.

```json
{
  "type": "api-request",
  "sessionId": "UUID",
  "id": "UUID",
  "timestamp": "ISO-8601",
  "version": "2.1.301",
  "querySource": "repl_main_thread",
  "shapeHash": "sha256-hex",
  "params": {
    "model": "claude-sonnet-5-5",
    "betas": ["claude-code-20250219", "..."],
    "max_tokens": 128000,
    "thinking": {"type": "adaptive", "display": "updates"},
    "context_management": {},
    "output_config": {},
    "safeguards": {}
  },
  "keep": 5,
  "tail": ["sha256-hex", "sha256-hex", "sha256-hex"],
  "base": null,
  "cacheControl": [
    {"path": ["system"], "value": {"type": "ephemeral", "ttl": "1h", "scope": "global"}}
  ],
  "requestDigest": "sha256-hex"
}
```

```json
{
  "type": "api-request-shape",
  "sessionId": "UUID",
  "shapeHash": "sha256-hex",
  "timestamp": "ISO-8601",
  "version": "2.1.301",
  "shape": {
    "system": [{"type": "text", "text": "You are Claude Code, ..."}],
    "tools": []
  }
}
```

```json
{
  "type": "api-request-blob",
  "sessionId": "UUID",
  "hash": "sha256-hex",
  "message": {
    "role": "user",
    "content": [{"type": "text", "text": "..."}]
  }
}
```

Observations from the 2026-10-18 local scan:

- `querySource` values: `repl_main_thread`, `agent:custom`.
- `params.thinking` was `{"type": "adaptive", "display": "updates"}` on every
  sampled request; `keep` is an integer and `tail` a short list of blob hashes
  (usually 3). Their exact semantics (how many earlier messages are reused vs.
  newly appended) are inferred, not confirmed.
- `api-request-blob.message.role` values: `system`, `user`, `assistant`. Content
  block types: `text`, `thinking`, `tool_use`, `tool_result`, plus a new
  `tool_addition` block (`{type: "tool_addition", tool: {type:
  "tool_definition", definition: {name, description, input_schema}}}`) that
  records a tool added mid-conversation.
- `assistant` events gained a top-level `requestRef` equal to the `id` of the
  matching `api-request`. The assistant `message.content` is **still inline**
  on those events (504 of 504 sampled), so the recording duplicates content
  rather than moving it.
- **Size impact:** `api-request*` lines account for 35–69 % of the bytes of
  v2.1.300+ transcripts (e.g. 4.54 MB of a 6.61 MB v2.1.301 session), because
  shapes carry the full system prompt and tool schemas.
- `api-request-blob` events with `role: "user"` / `"assistant"` look like
  conversation messages but are not: any code that dispatches on
  `message.role` instead of the top-level `type` would index them twice.

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

### Transcript Completeness Notes (changelog, v2.1.296–v2.1.300)

Changelog entries that affect what a transcript file contains, without
changing its structure:

- v2.1.296: `CLAUDE_CODE_TRANSCRIPT_LOCAL_GC` (opt-in env var) could drop a
  message from the saved transcript; fixed, and a large transcript is no longer
  rewritten. When enabled, a transcript can therefore change other than by
  appending — relevant to fingerprint-based incremental indexing.
- v2.1.299: bytes of @-mentioned PDFs are no longer kept in the session
  transcript.
- v2.1.300: fixed the first transcript entry written after resuming a session
  being lost. Resumed sessions written by earlier versions may miss one entry.

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

**v2.1.300+ (single local sample, 2026-10-18):** the one sidecar touched since
2026-09-06 carried `agentType`, `description`, `name`, `spawnDepth` plus new
`requestShape`, `requestNonInteractive`, and `stoppedByUser`, and lacked the
teammate-form `model` / `taskKind` / `teamName` / `color` /
`planModeRequired` / `permissionMode` keys. One sample does not establish a new
contract — it may simply be a non-teammate launch.

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
| `requestRef` | On `assistant` events, the `id` of the matching `api-request` event (observed v2.1.300+) |
| `requestedModel` | Model the request asked for, on `assistant` events (observed v2.1.295+); can differ from `message.model` |
| `advisorModel` | Advisor model slug paired with the session, on `assistant` events (observed v2.1.287+; see `/advisor` in the changelog) |
| `perTurnEffort` | Per-turn reasoning effort on `assistant` events (`"medium"` or `null`; observed v2.1.267+); sibling of `effort` |
| `thinkingDisplay` / `thinkingDurationMs` | Thinking display mode (`"updates"` only observed) and thinking time on `assistant` events (observed v2.1.296+) |
| `wireToolInputs` / `wireIngestContext` | Maps keyed by `tool_use_id`: the tool input as sent on the wire and its ingest context (e.g. `cwd`), on `assistant` events (observed v2.1.267+) |
| `serverClassifierRequest` / `serverClassifierContext` | Auto-mode classifier request id (`assistant`) and its context, including git state and live cwd (`user`) (observed v2.1.278+) |
| `attributionPlugin` | Plugin attributed to an `assistant` event, e.g. `"superpowers"` (observed v2.1.299+); joins the attribution family |
| `permissionDecision` | On tool-result `user` events: `{decision, source, reasonType}`. Observed `decision`: `accept`; `source`: `config`, `user_temporary`, `user_permanent`; `reasonType`: `classifier`, `mode`, `subcommandResults`, `rule`, `other` (observed v2.1.294+) |
| `toolDenialKind` | Denial reason on tool-result `user` events (`"automode-unavailable"` observed) |
| `turnOrigin` / `origin` | Turn origin on `user` events: `turnOrigin` `human` or `task_notification`; `origin.kind` `human` or `task-notification` (observed v2.1.278+) |
| `turnPosition` | `{promptIndex, turnIndex}` on `user` events (observed v2.1.287+) |
| `sourceToolUseID` / `queueSkipAttachments` | Rare `user` event fields (observed v2.1.300+; purpose unconfirmed) |

The `observed vX+` versions on rows added by the 2026-10-18 scan are the lowest
version in which the field appeared among sessions modified since 2026-09-06
(oldest: v2.1.220). They bound when a field was first seen locally, not when it
was introduced upstream.

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
- Local sample refresh (2026-10-18, v2.1.264–v2.1.302): only Claude 5 family slugs
  remain: `claude-haiku-5-5`, `claude-sonnet-5-5`, `claude-opus-5-5`,
  `claude-sonnet-5`, `claude-opus-5`, plus `<synthetic>`. Slugs are undated
  (no `-YYYYMMDD` suffix).

### Supported Claude Code Model Slugs

Checked 2026-10-19 against the official
[Claude Code model configuration](https://code.claude.com/docs/en/model-config)
page (the older support-article link now superseded it). The official page maps
aliases to display names per provider rather than listing a slug table. On the
Anthropic API the aliases resolve to: `opus` = Opus 5.5 (v2.1.280+), `sonnet` =
Sonnet 5.5 (v2.1.284+), `haiku` = Haiku 5.5 (v2.1.293+). It also resolves
`fable` to Fable 5.1 (v2.1.257+).

| Product name | Slug | Status |
|--------------|------|--------|
| Opus 5.5 | `claude-opus-5-5` | Verified: named in the official page ("use the full model name, for example `claude-opus-5-5`") and observed locally |
| Haiku 5.5 | `claude-haiku-5-5` | Verified: named in the official page (`/model claude-haiku-5-5`) and observed locally |
| Opus 5 | `claude-opus-5` | Verified as an ID used in official `availableModels` examples; observed locally |
| Sonnet 5 | `claude-sonnet-5` | Verified as an ID used in official `fallbackModel` examples; observed locally |
| Sonnet 5.5 | `claude-sonnet-5-5` | **Observed locally, not verified upstream.** The official page names "Sonnet 5.5" but never prints this slug; it follows the `claude-opus-5-5` / `claude-haiku-5-5` pattern, which is an inference, not proof |

Notes and disagreements:

- The official page also documents slugs that were **not** observed in local
  transcripts: `claude-fable-5-1`, `claude-fable-5`, `claude-opus-4-8`, and the
  older `claude-opus-4-6`, `claude-sonnet-4-6`, `claude-sonnet-4-5`,
  `claude-sonnet-4-5-20250929`, `claude-haiku-4-5`. The local set is what
  sessions on one machine happened to use, not the full supported set.
- The official page uses undated IDs for current models; the local 5.x slugs
  are undated too. Older 4.x slugs in historical transcripts may be dated
  (`claude-opus-4-5-20251101`, `claude-haiku-4-5-20251001`).
- Third-party providers resolve aliases differently (for example Bedrock
  resolves `sonnet` to Sonnet 4.5); the slugs recorded in transcripts reflect
  the provider actually used, so the table above is the Anthropic API view.
- `<synthetic>` is a sentinel written by the CLI for locally generated
  assistant error/limit messages, not a model.
- The 2026-10-18 local set remains `claude-haiku-5-5`, `claude-sonnet-5-5`,
  `claude-opus-5-5`, `claude-sonnet-5`, `claude-opus-5` plus `<synthetic>`.

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
  skipped without error (no parser breakage from these additions). The same
  holds for the v2.1.300+ `api-request`, `api-request-shape`, and
  `api-request-blob` events; their `message.role: user|assistant` blobs are not
  mistaken for messages because dispatch keys on the top-level `type`
- **Skipped-line pre-filter (2026-10-19):** since v2.1.300 the skipped
  `api-request*` lines are 35-69 % of transcript bytes, so the parser no longer
  builds a full `serde_json::Value` for lines it ignores. A line that starts
  with `{` (object guard, so positional arrays such as `["permission-mode", ...]`
  cannot decode as a header) is first deserialized into a three-field borrowed
  `EventHeader` (`type`, `sessionId`, `cwd`). If its `type` is not `user`,
  `assistant`, or `ai-title`, only the session id and cwd are captured
  (first-wins, as before) and the line is skipped. Capture happens only after
  the header deserialized successfully. Everything else takes the unchanged
  full-`Value` path: the three retained types, lines whose header fails to
  deserialize (escaped, non-string, or duplicate fields, non-objects), and
  missing, null, or non-string `type`.
- **Accepted divergence of the pre-filter:** serde_json's skipping path for
  the header's ignored fields is more lenient than a full `Value` parse: it
  does not reject out-of-range numbers such as `1e999` (overflow), and it is
  not bound by `Value`'s nesting limit in the same way (depth). A *skipped*
  line that a full parse would reject is therefore no longer counted as a
  parse error and can supply the first session id / cwd when none was captured
  yet. Making it strict would require fully parsing skipped lines, defeating
  the optimization. Observable effect: the parse-error flag only changes the
  error returned for a session with no messages at all (a session whose only
  failures are such lines now reports "Session contains no messages" instead
  of "Session contained parse errors and no messages"), and the captured
  id / cwd come from a line that is syntactically JSON but rejected by a full parse. Retained
  (`user` / `assistant` / `ai-title`) lines are unaffected, since their capture
  happens after the full parse. This leniency is pinned by tests
  (`recent_header_*`).
- Measured effect: see [Parse-time measurements](#parse-time-measurements).
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

**Title extraction:** The last non-blank `ai-title` event wins and takes
precedence over everything below. Otherwise the first prompt is chosen
human-first: user messages carrying any of three structured non-human markers
are excluded from the first-prompt search:

- `isMeta == true` (system-generated context, e.g. an injected skill body)
- `origin.kind == "task-notification"`
- `turnOrigin == "task_notification"`

Matching is exact: `isMeta: false`, a string `"true"`, other `origin.kind` /
`turnOrigin` values, and null values do not mark a message, and a human prompt
that merely quotes `<task-notification>` is not excluded (text is never
inspected for these markers). Among the remaining messages the usual wrapper
filtering still applies (`<system-reminder>` and `<command-output>` wrapped
messages are skipped, blank results ignored). If no eligible human prompt
survives, the search falls back to all user messages including the flagged
ones, so a session never loses its title only because its sole prompt was
flagged. The flagged messages are still indexed, counted, and searchable; only
first-prompt selection changes, and message counts, indices, and transcript
items are unchanged. `agent-name` is **not** consumed — the one local sample
carrying it also carried `ai-title`, so no fallback is justified yet.

Slash-command invocations (`<command-name>`, `<command-message>`,
`<command-args>`) are normalized for the first prompt into the command and its
arguments when the message is a single well-formed invocation; anything else is
left as-is. This is unchanged.

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

## Parse-time measurements

Measured 2026-10-19 for the skipped-line pre-filter described under Parser
Behavior. **Decision: retained** (gate: at least 20 % faster on the high-skip
transcript, no more than 5 % slower on low-skip transcripts).

**Method.** Opt-in harness `crates/core/tests/claude_parse_timing.rs` (ignored
in the ordinary suite), release build with `lto = true`, rustc 1.96.0. Fixed
private copies of three real transcripts, identical for both variants. One
discarded warm-up, then 8 batches alternating baseline and candidate, 50 timed
parses per batch, process pinned to one CPU core. Baseline is the parser as of
`0c937c1`; candidate is `fec9eec`. Times are per-parse milliseconds. An earlier
3 batches x 20 runs attempt was inconclusive because batch medians swung about
2x within a single variant (machine noise), so it was discarded and
re-measured with the setup above.

**Workloads.** No transcript from v2.1.301+ with a low skipped-byte share (nor
a near-zero-skip one) existed locally, so the low-skip rows are **proxies**,
not true low-skip workloads: the lowest v2.1.301+ share found was 62.05 %, and
a pre-v2.1.301 transcript (20.86 %) stands in for the case where the filter
saves little.

| Workload | Version | Bytes | Lines | Skipped-type bytes |
|----------|---------|------:|------:|-------------------:|
| High-skip | v2.1.301 | 955,846 | 227 | 87.27 % |
| Low-skip proxy (new) | v2.1.302 | 2,373,281 | 482 | 62.05 % |
| Low-skip proxy (old) | v2.1.287 | 720,808 | 166 | 20.86 % |

**Results** (per-batch medians, ms, in run order; aggregate is the median of
the eight batch medians):

| Workload | Variant | Batch medians | Batch-median range | Overall min-max | Aggregate | Change |
|----------|---------|---------------|-------------------:|----------------:|----------:|-------:|
| High-skip | baseline | 1.832, 2.027, 2.043, 2.005, 2.017, 2.016, 2.021, 2.008 | 1.832-2.043 | 1.405-2.297 | 2.016 | |
| High-skip | candidate | 0.854, 0.859, 0.867, 0.871, 0.871, 0.881, 0.875, 0.874 | 0.854-0.881 | 0.844-1.147 | 0.871 | -56.8 % |
| Low-skip proxy (new) | baseline | 5.005, 5.046, 5.024, 5.137, 5.024, 4.982, 4.991, 5.008 | 4.982-5.137 | 3.528-7.226 | 5.016 | |
| Low-skip proxy (new) | candidate | 2.657, 2.590, 2.598, 2.634, 2.586, 2.582, 2.586, 2.607 | 2.582-2.657 | 2.551-3.250 | 2.594 | -48.3 % |
| Low-skip proxy (old) | baseline | 0.814, 0.828, 0.824, 0.827, 0.822, 0.822, 0.858, 0.816 | 0.814-0.858 | 0.795-1.048 | 0.823 | |
| Low-skip proxy (old) | candidate | 0.793, 0.796, 0.578, 0.816, 0.805, 0.802, 0.810, 0.891 | 0.578-0.891 | 0.551-1.187 | 0.804 | -2.4 % (computed from unrounded values) |

**Gate decision.** High-skip improved 56.8 % (threshold 20 %); batch-median
ranges are disjoint, so the difference far exceeds the spread. The newer
low-skip proxy also improved 48.3 % with disjoint ranges. The old low-skip
proxy shows no slowdown (-2.4 %, computed from unrounded values, i.e. faster), but its ranges overlap because of
outlier batches, so treat it as "no measurable regression", not as a speed-up.
The pre-filter was therefore retained as shipped in `fec9eec`. Caveats: three
transcripts from one machine, parse time only (not end-to-end indexing), and
proxy low-skip workloads.

---

## References

- [Anthropic prompt caching docs (`cache_read_input_tokens`, `cache_creation_input_tokens`, `input_tokens`)](https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching)
- [Claude Code subagents docs (subagent transcript path)](https://docs.anthropic.com/en/docs/claude-code/sub-agents)
- [Claude Code changelog (compaction/transcript behavior changes)](https://docs.anthropic.com/en/docs/claude-code/changelog)
- [Claude Code CHANGELOG.md (v2.1.300 API request recording entry)](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md)
- [Claude Code model configuration (official; aliases, version history, full model names)](https://code.claude.com/docs/en/model-config)
- [Models overview (API model IDs)](https://platform.claude.com/docs/en/about-claude/models/overview)
- [Claude Code issue: duplicate entries in session logs](https://github.com/anthropics/claude-code/issues/1524)
