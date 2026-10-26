# Kimi Code — Session Format Reference

Format reference for [Kimi Code CLI](https://github.com/MoonshotAI/kimi-code) session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-assistant comparison tables.

Documented from the official docs (`docs/en/guides/sessions.md`,
`docs/en/configuration/data-locations.md`), the upstream TypeScript sources
(`packages/agent-core-v2`), and local sampling of `~/.kimi-code/` (2026-07-29).
Updated 2026-09-06 from an upstream diff of `@moonshot-ai/kimi-code` 0.31.1 →
main (0.41.0), the generated wire manifest
(`packages/agent-core-v2/docs/wire-manifest.d.ts`), release notes 0.32.0–0.41.0,
and fresh local sampling of `~/.kimi-code/` (CLI 0.31.1).
Updated 2026-10-23 from the official docs site ([data
locations](https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/data-locations.html),
[changelog 0.42.0–2.3.0](https://www.kimi.com/code/docs/en/kimi-code-cli/release-notes/changelog.html),
[agents/sub-agents](https://www.kimi.com/code/docs/en/kimi-code-cli/customization/agents.html))
and fresh local sampling of `~/.kimi-code/` with CLI 2.3.1. A same-day
follow-up inspected the upstream source at main (post-2.3.1 clone): the wire
manifest (65 durable record types), the human-domain event layer
(`packages/agent-core-v2/src/human/`), the subagent fork path, and the TUI
auto-title caller. Entries marked *(2.3.x)* are confirmed from local journals
and, where noted, from that source inspection.

**Format watch 2026-10-25/26 (2.3.2 + main `f70c3652`):** no change under
`packages/agent-core-v2/src/wire/` or in `wire-manifest.d.ts` between 2.3.1,
[2.3.2](https://github.com/MoonshotAI/kimi-code/blob/main/apps/kimi-code/CHANGELOG.md)
and main; `WIRE_PROTOCOL_VERSION` stays `'1.5'`. 2.3.2 is behavioral only:
subagents are launched only on explicit request
([#4386](https://github.com/MoonshotAI/kimi-code/pull/4386)) and `AgentSwarm`
can no longer enter swarm mode by itself
([#4384](https://github.com/MoonshotAI/kimi-code/pull/4384)), so `swarm_mode`
entries with a `tool` trigger are no longer written (record shape unchanged).
Unreleased on main (2026-10-26):
[#4342](https://github.com/MoonshotAI/kimi-code/pull/4342) adds
`/import-sessions`, which converts recent Claude Code and Codex sessions into
ordinary Kimi sessions under `sessions/<workDirKey>/<sessionId>/`
(`packages/migration-legacy/src/imported-session.ts`): a synthesized
`agents/main/wire.jsonl` (metadata `protocol_version: "1.0"`, same turn
records as migrated kimi-cli sessions), a `state.json` with
`titleKind: "generated"`, `agents.main.parentAgentId: null`, and
`custom: {imported_from: "claude-code" | "codex", imported_source_id,
imported_source_path, imported_at}`, plus an `attachments/` directory holding
the effective history and a source copy. Once released, the same conversation
can therefore be indexed both from its original Claude Code / Codex file and
as a Kimi session; `custom.imported_from` is the marker to recognize it.

---

## Storage & File Naming

| Field | Value |
|-------|-------|
| **Data root** | `$KIMI_CODE_HOME` (default: `~/.kimi-code/`) |
| **Sessions root** | `$KIMI_CODE_HOME/sessions/<workDirKey>/<sessionId>/` |
| **workDirKey** | `wd_<slug>_<first-12-chars-of-sha256(workDir)>` |
| **sessionId** | `session_<uuid>` |
| **Example** | `~/.kimi-code/sessions/wd_sessions-chronicle_a75d38aead93/session_70d49998-f9d1-4546-ab98-3bba4551a6da/` |
| **Format** | Directory-based: `state.json` + per-agent `wire.jsonl` (JSONL) |

Setting `KIMI_CODE_HOME` relocates **all** Kimi Code data (config, sessions,
logs, credentials). Sessions are grouped by working directory, one bucket per
workdir, similar to Claude Code's project directories.

### Session index

`$KIMI_CODE_HOME/session_index.jsonl` — one JSON record per line:

```json
{"sessionId":"session_759ccf96-...","sessionDir":"/home/user/.kimi-code/sessions/wd_sessions-chronicle_a75d38aead93/session_759ccf96-...","workDir":"/home/user/Projets/sessions-chronicle"}
```

`$KIMI_CODE_HOME/workspaces.json` maps each `workDirKey` to its working
directory root, name, and `created_at` / `last_opened_at` timestamps
(ISO-8601). It is a faster, more reliable way to resolve the project path than
decoding the bucket name. The file is versioned and nested:

```json
{"version":1,"workspaces":{"wd_sessions-chronicle_a75d38aead93":{"root":"/home/user/Projets/sessions-chronicle","name":"sessions-chronicle","created_at":"...","last_opened_at":"..."}}}
```

---

## Session Directory Structure

```
sessions/<workDirKey>/<sessionId>/
├── state.json               # Session metadata (title, timestamps, agents, fork lineage)
├── agents/
│   ├── main/
│   │   ├── wire.jsonl       # Main agent event journal (see below)
│   │   ├── plans/           # Plan-mode plan files (<id>.md), when plan mode was used
│   │   ├── blobs/           # Inline media offloaded out of wire records (0.32+ era)
│   │   └── file-history/    # Turn-level file history blobs (always on since 0.41.0)
│   └── <subagentId>/        # e.g. agent-0 — one directory per subagent
│       ├── wire.jsonl       # Subagent event journal
│       └── tool-results/    # Spilled large tool outputs (observed locally 2026-09-06)
├── logs/
│   └── kimi-code.log        # Session-level diagnostic log (only when a diagnostic event occurs)
├── tasks/                   # Background task persistence (<task_id>.json + output.log)
├── cron/                    # Scheduled task persistence (reloaded on resume)
├── notify/                  # TUI notification state (own state.json `{enabled}`); observed 2.3.x
└── upcoming-goals.json      # TUI-only queued goals (/goal next), when present
```

Subagent directories are registered in `state.json` under `agents` with
`parentAgentId` — the parent/child relationship is metadata-based, not purely
directory-based. See [Threading](#threading).

Since 0.42.0 the data root also carries `search-index/` — the always-on
"minidb" global search index (custom binary WAL + postings files, **not**
SQLite). It is upstream's own search read model, not session data, and Sessions
Chronicle does not read it. Other non-session root entries observed with 2.3.1
include `server/`, `telemetry/`, `workspace-trust/`, `file-history/`, `cache/`,
`device_id`, and `region`.

---

## `state.json` Format

Session-level metadata. Observed local sample:

```json
{
  "createdAt": "2026-07-29T10:05:33.669Z",
  "updatedAt": "2026-07-29T10:08:56.155Z",
  "title": "On commence l'issue github 167...",
  "isCustomTitle": false,
  "agents": {
    "main": {
      "homedir": "/home/user/.kimi-code/sessions/wd_.../session_.../agents/main",
      "type": "main",
      "parentAgentId": null
    }
  },
  "custom": {},
  "workDir": "/home/user/Projets/sessions-chronicle",
  "lastPrompt": "On commence l'issue github 167..."
}
```

| Field | Description |
|-------|-------------|
| `createdAt` / `updatedAt` | ISO-8601 strings |
| `title` | Session title; prompt-derived, generated, or custom — see [Titling](#titling) |
| `isCustomTitle` | Legacy boolean: whether the title was set manually (`/title`). Superseded by `titleKind`, still dual-written as `titleKind === 'custom'` for back-compat |
| `titleKind` | `replaceable` \| `generated` \| `custom` — successor of `isCustomTitle` at main (`generated` covers the experimental automatic titling from 0.36.1); readers normalize the legacy boolean |
| `lastPrompt` | Most recent user prompt (sanitized, max 4000 chars) |
| `workDir` | Working directory. Dual-written with the schema-canonical `cwd` up to 0.43.x; **2.3.x sessions write only `cwd`** (observed 2026-10-23) — readers must accept either |
| `agents` | Map of agent id → `{homedir, type, parentAgentId?, labels?, ...}` — see [Threading](#threading). 2.3.x omits `parentAgentId` on `main` (previously written as `null`) and sets `labels: {parentAgentId, profileName}` on subagent entries, where `profileName` is the subagent type (`explore`, `coder`, ...) |
| `forkedFrom` | Source session id when created via `/fork` (optional) |
| `archivedAt` | Archive timestamp in epoch ms (optional, added after 0.31.1) |
| `lastTurnReason` | `completed` \| `cancelled` \| `failed` — outcome of the last turn, persisted across restarts (0.34.0) |
| `custom` | Free-form extension map |

Upstream schema (`packages/klient/src/contract/session/metadata.ts`,
`sessionMetaSchema` / `agentMetaSchema`, `SESSION_META_VERSION = 2`)
additionally defines `id`, `version`, `archived`, and `cwd`; agent entries can
also carry `forkedFrom`, `labels`, and `swarmItem`. Locally sampled sessions
already carry `id`, `version`, and `cwd` on disk alongside the dual-written
`workDir` / `isCustomTitle`.

### Titling

A session title is in one of three states, tracked by `titleKind`
(`replaceable` | `generated` | `custom`). `isCustomTitle` is derived on write as
`titleKind === 'custom'` (`packages/agent-core-v2/src/session/sessionMetadata/sessionMetadataService.ts`,
`toPersisted`), so both fields always agree on disk.

**Prompt-derived (`replaceable`)** —
`packages/agent-core-v2/src/session/sessionMetadata/promptMetadata.ts`. On each
submitted prompt `lastPrompt` is updated, and `title` is set only when
`titleKind !== 'custom'` **and** the current title is unset/empty/`"New Session"`
— so later prompts never re-derive the title. Before storage the prompt text is
sanitized (`src/agent/prompt/promptMetadataText.ts`): private keys,
`Authorization: Bearer …`, `api_key`/`token`/`secret`/`password` values, `sk-…`
strings and long base64-ish blobs become `[redacted]`; control characters and
whitespace collapse to single spaces. `lastPrompt` is a `slice(0, 4000)` of the
result, the title a `slice(0, 200)` of the same text. Media parts become
`[image]`/`[audio]`/`[video]`; skill invocations title as `/skill-name args`.

**Generated (`generated`, 0.36.1+ experimental → always on since 0.43.0 web /
2.3.0 CLI)** —
`packages/agent-core-v2/src/session/sessionTitle/sessionTitleService.ts`,
originally gated by the `auto_session_title` flag (env
`KIMI_CODE_EXPERIMENTAL_AUTO_SESSION_TITLE`, default off); the changelog
graduated it — web 0.43.0 made AI titles always on, and CLI 2.3.0 generates a
title automatically after the first turn (confirmed in source: the TUI calls
`generateSessionTitle({source: 'first_turn'})` on every completed turn end —
`apps/kimi-code/src/tui/kimi-tui.ts` `maybeAutoGenerateSessionTitle`, on by
default, `auto_session_title = false` in `tui.toml` to disable; attempts are
capped per session and failures are silent, so `titleKind: "generated"` does
appear in CLI `state.json` when the managed `chat_title` call succeeds while
other sessions stay `replaceable`). It calls the managed
`chat_title` endpoint over Kimi OAuth and applies the reply (truncated to 200
chars) through `setGeneratedTitleIfUncustomized`, which overwrites a
`replaceable` title but never a `custom` one unless `force` is set. The input is
built from one of three sources: `user_prompts` (the first three user prompts,
default), `first_turn` (first user + assistant excerpt), or `digest` (elided
multi-turn excerpt). Nothing in agent-core triggers it — clients call it over
RPC (`sessionTitleService.generateTitle`, exposed by `kap-server`) once the
first turn completes, or on demand from the rename field.

**Custom (`custom`)** — `/title <text>` (alias `/rename`) calls `setTitle()`,
which persists `{title, titleKind: 'custom'}`. Terminal state: no auto-titling
path replaces it. `/fork` accepts an optional title.

**Reading legacy files** — `normalizeSessionTitle` maps `isCustomTitle: true` →
`custom`, an otherwise valid `titleKind` → itself, `isCustomTitle: false` →
`replaceable`, a bare `customTitle` → `custom`, and a title carrying neither
marker → `replaceable`.

---

## `wire.jsonl` Format

JSONL event journal — one JSON object per line, append-only, used for session
recovery and replay. Every record has:

- `type`: record type string (dot-namespaced, e.g. `context.append_loop_event`)
- `time`: epoch milliseconds (optional in the schema, set in practice)

The **first line** is always a metadata envelope:

```json
{"type":"metadata","protocol_version":"1.5","created_at":1785279574895}
```

`protocol_version` is `"1.5"` at main (no 1.6 yet — still `"1.5"` in 2.3.1
local data), but `"1.4"` and `"1.5"` coexist in same-day local data, and
subagent journals may omit `created_at`.
Since 0.32-era releases, every wire record payload also carries `agentId`.

Upstream references: `packages/agent-core-v2/src/wire/record.ts`
(`WireRecord`, `WireMetadataRecord`), the per-domain `*Ops.ts` modules that
register each record type, and the generated
`packages/agent-core-v2/docs/wire-manifest.d.ts` — the authoritative list of
durable record types (65 at main, verified 2026-10-23). The human-domain
records (`agent.message.appended`, `agent.turn.started`, `agent.turn.ended`)
are persisted too but live outside this registry — they belong to the
`src/human/` conversation-history layer.

### Record types

Registered upstream (`packages/agent-core-v2/src/agent/**/*Ops.ts` and the
generated `wire-manifest.d.ts`), all observed locally unless noted. Records
marked *(0.32+)* were added between 0.31.1 and main (0.41.0) and are not yet
observable with a 0.31.1 local install:

| Type | Payload highlights |
|------|--------------------|
| `context.append_message` | `message`: full chat message `{role, content[], toolCalls[], origin{kind}}` |
| `context.append_loop_event` | `event`: one loop event — see [Loop events](#loop-events) |
| `context.apply_compaction`, `context.clear`, `context.undo` | Context maintenance (compaction summaries, rewind) |
| `turn.prompt` | `input`: content parts of the user prompt; `origin.kind` |
| `turn.cancel`, `turn.steer` | Turn interruption / mid-turn steering |
| `turn.ended` | Turn outcome summary: `turnId`, `reason`, `durationMs` (observed locally) |
| `turn.step.interrupted`, `turn.step.retrying` *(0.32+)* | Step interruption / retry markers |
| `prompt.accepted`, `prompt.steered`, `prompt.completed`, `prompt.aborted` *(0.32+)* | Prompt lifecycle |
| `llm.request` | `kind`, `provider`, `model`, `modelAlias`, `thinkingEffort`, `maxTokens`, `messageCount`, `turnStep`, `systemPromptHash`, `toolsHash` — one record per LLM request (per step). 2.3.x adds `thinkingKeep` and `toolSelect` (observed) |
| `llm.tools_snapshot` | Tool schemas sent with a request (`hash`, `tools`) |
| `usage.record` | `model`, `usage` (see [Token usage](#token-usage)), `usageScope` (`"turn"` \| `"session"` — request provenance, both are deltas) |
| `config.update` | `modelAlias`, `thinkingEffort` — active model/thinking changes |
| `tools.set_active_tools`, `tools.reset_active_tools` | Active tool list (`names`) |
| `tools.update_store` | Observed locally; dynamic tool store updates |
| `tools.register_user_tool`, `tools.unregister_user_tool` | User-tool registration |
| `mcp.tools_discovered` | `serverName`, `hash`, `tools`, `enabledNames` |
| `permission.record_approval_result` | `turnId`, `toolCallId`, `toolName`, `action`, `sessionApprovalRule`, `result` |
| `permission.set_mode` | Permission mode changes. (`permission.rules.add` is no longer persisted at main — rule additions are live-only) |
| `plan_mode.enter`, `plan_mode.exit`, `plan_mode.cancel`, `plan.revision` | Plan mode lifecycle |
| `swarm_mode.enter`, `swarm_mode.exit` | AgentSwarm lifecycle |
| `tower_mode.enter`, `tower_mode.exit` *(0.32+)* | Experimental tower mode lifecycle (0.40.0–0.41.0) |
| `task.started`, `task.terminated` | Background task lifecycle |
| `task.waitDelivered` *(0.32+)* | WaitFor-tool delivery notice (0.38.0) |
| `cron.add`, `cron.cursor`, `cron.delete` | Scheduled-task persistence |
| `interaction.request`, `interaction.resolved` | Interactive elicitation request/response (observed locally) |
| `interruptionReminder.recorded` | Interruption reminder marker (observed locally) |
| `profile.bind` | Profile binding; observed in subagent journals |
| `goal.create`, `goal.update`, `goal.clear`, `forked` | Goal mode / session fork (not observed locally yet) |
| `full_compaction.begin`, `full_compaction.complete`, `full_compaction.cancel` | Full-compaction lifecycle (not observed locally yet) |
| `token_counting.measured`, `token_counting.rebased`, `token_counting.truncated`, `token_counting.turn_recorded` *(0.32+)* | Token counting; replaces the removed `context_size.measured` |
| `file_history.checkpoint`, `file_history.tracked` *(0.32+)* | Turn-level file history; always on since 0.41.0 |
| `plugin.session_start` *(0.32+)* | Plugin lifecycle |
| `runtime.set_binding` *(0.32+)* | Runtime binding |
| `changeNotifier.snapshot` *(2.3.x)* | Config/skills/AGENTS.md hash snapshot (`agentsMdHash`, `agentsMdFiles`, `skillsHash`, subagent names); durable, listed in the main manifest (65 record types) |
| `agent.message.appended` *(2.3.x)* | Human-domain record (`src/human/agent/events.ts`, **not** one of the 65 durable state records): `{message}` — a user/assistant/system entry appended to the human-facing conversation history, folded by the `packages/transcript` read model. Coexists with `context.append_message` (model context); frequent in subagent journals (24 vs 2 `context.append_message` in one sample) |
| `agent.turn.started`, `agent.turn.ended` *(2.3.x)* | Human-domain turn lifecycle: `{turnId, queueItemId?}` / `{turnId, outcome: done\|failed\|aborted, errorMessage?}`; `queueItemId` is the `promptId` (`msg_<ulid>`, `src/agent/contextMemory/messageId.ts`) of the queued prompt that started the turn. Coexist with `turn.ended` |

Removed since 0.31.1: `context_size.measured` (→ `token_counting.*`),
`permission.rules.add` (live-only), and `skill.activate` (renamed
`skill.activated`, no longer durable — see
[Skills](#skills--slash-commands)).

### Loop events

Assistant turns are recorded as a stream of loop events inside
`context.append_loop_event` records. The union type is `LoopRecordedEvent` in
`packages/agent-core-v2/src/agent/contextMemory/loopEventFold.ts`:

| Event | Fields |
|-------|--------|
| `step.begin` | `uuid`, `turnId`, `step` — one step = one LLM request/response cycle |
| `content.part` | `stepUuid`, `part` — a content part (`text`, `think`, `image_url`, `audio_url`, `video_url`) |
| `tool.call` | `stepUuid`, `toolCallId`, `name`, `args`, optional `display` (UI rendering hints) |
| `tool.result` | `toolCallId`, `parentUuid`, `result: {output: string \| ContentPart[], isError?, note?}` |
| `step.end` | `uuid`, `usage`, `finishReason`, latency metrics (`llmFirstTokenLatencyMs`, `llmStreamDurationMs`, ...), `messageId` |

Example (abridged, from local sampling):

```json
{"type":"context.append_loop_event","event":{"type":"step.begin","uuid":"2cc63775-...","turnId":"0","step":1},"time":1785279582624}
{"type":"context.append_loop_event","event":{"type":"content.part","stepUuid":"2cc63775-...","part":{"type":"think","think":"..."}},"time":1785279583000}
{"type":"context.append_loop_event","event":{"type":"content.part","stepUuid":"2cc63775-...","part":{"type":"text","text":"Je regarde le dernier commit."}},"time":1785279583100}
{"type":"context.append_loop_event","event":{"type":"tool.call","toolCallId":"Bash_0","name":"Bash","args":{"command":"git log -1 --stat"},"display":{"kind":"command","command":"git log -1 --stat","cwd":"/home/user/Projets/sessions-chronicle"}},"time":1785279583200}
{"type":"context.append_loop_event","event":{"type":"tool.result","parentUuid":"Bash_0","toolCallId":"Bash_0","result":{"output":"commit 124e0d1..."}},"time":1785279584100}
{"type":"context.append_loop_event","event":{"type":"step.end","uuid":"2cc63775-...","usage":{"inputOther":23815,"output":86,"inputCacheRead":11264,"inputCacheCreation":0},"finishReason":"tool_use","messageId":"chatcmpl-..."},"time":1785279584200}
```

Tool call correlation: `tool.call.toolCallId` → `tool.result.toolCallId` (and
`parentUuid` mirrors the call's `uuid`). Note that `toolCallId` values can be
short synthetic ids (`Bash_0`), not UUIDs.

A turn that ends without a `step.end` for its last `step.begin` means the tool
exchange was interrupted (e.g. app quit); upstream replay seals such partial
steps with a "tool execution was interrupted" notice.

### User messages

User prompts appear twice:

- `turn.prompt` records the raw input: `{"input":[{"type":"text","text":"..."}],"origin":{"kind":"user"}}`
- `context.append_message` records the full message as folded into the model
  context: `{"message":{"role":"user","content":[...],"toolCalls":[],"origin":{"kind":"user"}}}`

Upstream defines 12 `origin.kind` values: `user`, `skill_activation`,
`plugin_command`, `injection`, `shell_command`, `compaction_summary`,
`system_trigger`, `task`, `cron_job`, `cron_missed`, `hook_result`, `retry`.
Locally observed so far: `user`, `injection`, `skill_activation`.

### Token usage

Two carriers:

- `usage.record` records: `{"model":"moonshot-ai/kimi-k3","usage":{"inputOther":23815,"output":86,"inputCacheRead":11264,"inputCacheCreation":0},"usageScope":"turn"}`
- `step.end.usage`: same shape, one per step

Cache tokens (`inputCacheRead`, `inputCacheCreation`) are reported separately
from uncached input (`inputOther`) — same convention as Claude Code.

**`usageScope`** (`packages/agent-core-v2/src/session/usage/usageAgentModel.ts`)
labels *what caused* the LLM request, not how the numbers accumulate:

- `"turn"` — the request came from a turn (`source.type === 'turn'`), i.e. the
  normal prompt → step loop.
- `"session"` — the request had no turn source: an `operation` request or one
  emitted with no source at all. At main the only `operation` producer is full
  compaction (`fullCompactionService.ts`, `requestKind: 'full_compaction'`), so
  in practice `"session"` records are the tokens auto-compaction spent on the
  session's behalf.

Both scopes are **deltas, never snapshots**. Sessions Chronicle sums every  
`usage.record` payload exactly once, regardless of `usageScope` (including  
missing or unknown scope values), so session-scoped compaction usage is  
included. If a journal has no `usage.record` records, the parser falls back to  
summing `step.end.usage`; it never adds the two carriers together.

### Model metadata

Per request (per step): `llm.request` carries `provider`, `model`, and
`modelAlias` (e.g. `provider: "kimi"`, `model: "kimi-k3"`,
`modelAlias: "moonshot-ai/kimi-k3"`). Session-level model switches appear as
`config.update` records (`modelAlias`, `thinkingEffort`). Individual messages
and loop events carry no model field.

---

## Threading

Each agent (main or subagent) has its **own** `wire.jsonl` journal under
`agents/<agentId>/`; there is no tree structure inside a single journal beyond
the `turnId`/`step`/`uuid` sequencing of loop events.

### Subagents

A subagent launched via the `Agent` tool:

- appears in the parent's journal as an ordinary `tool.call` / `tool.result`
  pair (`name: "Agent"`). Since 2.x the call args are `description`, `prompt`,
  `subagent_type` — **no `agent_id` in args** (observed 2.3.1); the result's
  text output opens with header lines `agent_id: agent-N`,
  `actual_subagent_type: <profile>`, `status:`, `stop_reason:` before the
  `[summary]` body, so the result text is the linkage carrier;
- gets its own journal at `agents/<subagentId>/wire.jsonl`
  (e.g. `agents/agent-0/wire.jsonl`);
- is registered in `state.json`:

```json
"agents": {
  "main":    {"type": "main", "parentAgentId": null},
  "agent-0": {"type": "sub",  "parentAgentId": "main"}
}
```

The subagent's task prompt is commonly submitted with  
`origin: {"kind": "system_trigger", "name": "subagent"}`  
(`AGENT_RUN_PROMPT_ORIGIN`,  
`packages/agent-core-v2/src/session/subagent/runAgentTurn.ts`). Child journals  
can also contain a `user`-origin task prompt, notably around fork behavior; the  
exact origin used for a fork task remains an open question. Sessions Chronicle  
accepts both origins in child journals, while main sessions still require a  
genuine `user`-origin prompt.

`agentMetaSchema.type` is `main | sub | independent`; `parentAgentId` gives
the parent linkage, and `labels` / `swarmItem` can annotate swarm spawns. In
2.3.x sessions `labels` carries `{parentAgentId, profileName}` on every
subagent entry, duplicating the parent link and naming the subagent profile
(`explore`, `coder`, ...). Unlike Mistral Vibe, the parent tool-call id is not
needed for linkage — the `agents` map names both endpoints.

An experimental `fork: true` arg on the `Agent` tool (flag `subagent_fork` /
`KIMI_CODE_EXPERIMENTAL_SUBAGENT_FORK`, still experimental at main/2.3.1)
spawns the child with a snapshot of the caller's context  
(`session/subagent/subagentService.ts` → `agentLifecycle.fork()`). The child's  
`state.json` agent entry carries `forkedFrom: <callerAgentId>`, and copied  
messages are persisted as `context.append_message` records (with any trailing  
open tool exchange closed first, plus a `fork_context` reminder). The task  
prompt's origin in a forked child is not settled by the available evidence;  
both `user` and `system_trigger`/`subagent` are accepted in child journals.  
Because a genuine task prompt is present, copied parent context is not  
mistaken for the child's own messages.

### AgentSwarm

An `AgentSwarm` tool call carries `description`, `subagent_type`,  
`prompt_template` (which may contain `{{item}}`), and an ordered `items` array.  
Its result output may contain an `<agent_swarm_result>` wrapper with one  
`<subagent agent_id="…" item="…" outcome="…">…</subagent>` block per item.  
The `item` value associates a result block with its swarm item; the body is the  
item's result summary. Child metadata may identify the item as  
`labels.swarmItem`; older journals may instead put `swarmItem` at the top  
level of the agent entry.

Sessions Chronicle expands a resolved call into one subagent item per matched  
swarm item, in `items` order. It substitutes each item into `prompt_template`,  
and strips a leading `<git-context>` block from child task prompts before  
showing the child transcript or title.

### Resumed subagents

Several `Agent` calls can refer to the same child—for example, later calls can  
resume it with `args.resume`. Sessions Chronicle keeps a transcript item for  
each call and links them all to the same child session. This makes repeated  
work on one subagent visible without creating duplicate child sessions.

For ordinary children, a consistent result header alone can identify a resumed  
child. Once a child has been consumed by a resolved `AgentSwarm`, only a later  
`Agent` call with explicit matching `args.resume` can link to it; a header-only  
resume stays a generic tool call.

### Forks

`/fork` creates an independent copy of a session; lineage is recorded in
`state.json.forkedFrom` (and a `forked` wire record). The two sessions evolve
independently afterwards.

---

## Skills / Slash Commands

- Skill activation is no longer persisted as a wire record at main: the former
  `skill.activate` record was renamed `skill.activated` and is live-only. The
  durable marker is `origin.kind == "skill_activation"` on `turn.prompt` /
  `context.append_message` records (11 observed locally), which also carry
  structured fields (`skillName`, `skillPath`, `skillSource`, `skillArgs`,
  `activationId`, `trigger`).
- Other injected content uses `origin.kind` values such as `injection` or
  `system_trigger` (12 kinds upstream — see [User messages](#user-messages)).

---

## Input History (Not a Session Log)

`$KIMI_CODE_HOME/user-history/<md5(workDir)>.jsonl` stores typed prompts for
arrow-key recall in the TUI. It does not contain the assistant/tool transcript.

---

## Legacy Format (`~/.kimi`, pre-migration)

The earlier Python-based Kimi CLI stored sessions under
`~/.kimi/sessions/<md5(workDir)>/<uuid>/` with two JSONL files:

- `context.jsonl` — message history
- `wire.jsonl` — event journal with a different envelope:
  `{"timestamp": <float seconds>, "message": {"type": "TurnBegin" | "TurnEnd" | ..., "payload": {...}}}`
  and `{"type": "metadata", "protocol_version": "1.10"}` as first line

The TypeScript rewrite migrates this data to `~/.kimi-code/` on first run
(`packages/migration-legacy`), leaving a `~/.kimi/.migrated-to-kimi-code`
marker and a `~/.kimi-code/migration-report.json`. Legacy files are retained
on disk. Sessions Chronicle supports the current `$KIMI_CODE_HOME` layout
(default `~/.kimi-code`) when that location is visible in the Flatpak sandbox.
It does not parse the retained legacy `~/.kimi` layout.

---

## Parser Behavior (Sessions Chronicle)

The parser and indexer implement the current TypeScript CLI format:

- Discovery scans `$KIMI_CODE_HOME/sessions/wd_*/session_*/` and requires
  `state.json` plus `agents/main/wire.jsonl`.
- Bundle parsing streams each agent journal with `BufReader`, extracting user
  and assistant messages, reasoning, tool calls/results, model metadata, and
  token usage while tolerating malformed or unknown records locally.
- Incremental indexing uses one composite bundle fingerprint containing
  `state.json` and every agent journal declared by `state.json.agents`; a
  changed child journal therefore reindexes the whole bundle atomically.
- Subagent journals with `parentAgentId` are indexed as synthetic child  
  sessions. Child journals accept `user` prompts and  
  `system_trigger`/`subagent` task prompts; main sessions still require a  
  genuine user prompt. Leading `<git-context>` blocks are removed from child  
  task prompts before they are shown in the transcript or used for the title.  
  Main sessions can resume with `kimi --session <id>`; synthetic children
  cannot be resumed directly.
- `Agent` calls link through child metadata and call/result evidence. Calls  
  resumed with `args.resume` can produce multiple subagent items linked to one  
  child session. `AgentSwarm` calls are expanded into ordered item rows when  
  their result and child metadata can be matched; see [AgentSwarm](#agentswarm).
- Main sessions without a genuine user-origin prompt are not indexed. If an  
  existing bundle's main journal becomes injection-only, its main session,  
  synthetic children, links, transcript content, and fingerprints are pruned.
- Custom `$KIMI_CODE_HOME` locations work when visible in the Flatpak sandbox.
  Legacy sessions under `~/.kimi` are not parsed.
- Unknown or newer wire records (the 0.32–0.41 additions such as `prompt.*`,
  `token_counting.*`, `file_history.*`, `tower_mode.*`) are skipped without
  error, and none of the record types removed upstream (`skill.activate`,
  `context_size.measured`, `permission.rules.add`) were consumed by the parser
  (verified 2026-09-06). `state.json` is read with both `workDir` and `cwd`,
  and `workspaces.json` with its versioned nested shape, so the
  `titleKind`/`cwd` transition is absorbed without parser changes.
- The 2.3.x `changeNotifier.snapshot`, `agent.message.appended`,  
  `agent.turn.started`, and `agent.turn.ended` records are skipped, while  
  `cwd`-only `state.json` files parse through the existing fallback.  
  `Agent` calls without `args.agent_id` can still match their child through  
  result evidence such as the `agent_id:` header. Child `labels.profileName`  
  and both forms of `swarmItem` metadata are read; result headers are removed  
  when extracting the subagent summary. `agent.message.appended` is a  
  human-history projection that coexists with `context.append_message`, not a  
  replacement. Forked child journals accept either task-prompt origin noted in  
  [Threading](#threading); copied context is not surfaced as child messages.

---

## Primary Sources

- [Kimi Code CLI Repository](https://github.com/MoonshotAI/kimi-code)
- [Official docs: Sessions and context](https://github.com/MoonshotAI/kimi-code/blob/main/docs/en/guides/sessions.md)
- [Official docs: Data locations](https://github.com/MoonshotAI/kimi-code/blob/main/docs/en/configuration/data-locations.md)
- [Wire record definitions (`packages/agent-core-v2/src/wire/record.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/wire/record.ts)
- [Human-domain event definitions (`packages/agent-core-v2/src/human/agent/events.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/human/agent/events.ts) — `agent.message.appended` / `agent.turn.started` / `agent.turn.ended`
- [Subagent spawn / fork (`packages/agent-core-v2/src/session/subagent/subagentService.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/session/subagent/subagentService.ts)
- [Generated wire manifest (`packages/agent-core-v2/docs/wire-manifest.d.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/docs/wire-manifest.d.ts) — authoritative durable-record list
- [Loop event model (`packages/agent-core-v2/src/agent/contextMemory/loopEventFold.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/agent/contextMemory/loopEventFold.ts)
- [Session metadata contract (`packages/klient/src/contract/session/metadata.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/klient/src/contract/session/metadata.ts)
- [Session title service (`packages/agent-core-v2/src/session/sessionTitle/sessionTitleService.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/session/sessionTitle/sessionTitleService.ts) — experimental `chat_title` generation
- [Message/content contract (`packages/agent-core-v2/src/kosong/contract/message.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/kosong/contract/message.ts)
- [Legacy migration (`packages/migration-legacy`)](https://github.com/MoonshotAI/kimi-code/tree/main/packages/migration-legacy)
- [Official docs site: Data locations](https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/data-locations.html), [Changelog](https://www.kimi.com/code/docs/en/kimi-code-cli/release-notes/changelog.html), [Agents and Sub-Agents](https://www.kimi.com/code/docs/en/kimi-code-cli/customization/agents.html)
- Local sampling of `~/.kimi-code/` (2026-07-29, 2026-09-06, 2026-10-23 with CLI 2.3.1)
