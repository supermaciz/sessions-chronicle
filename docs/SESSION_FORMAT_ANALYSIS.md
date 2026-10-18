# Session Format Analysis


Cross-assistant comparison of Claude Code, Codex, OpenCode, Mistral Vibe, and Kimi Code session file formats.

**Per-assistant format details and parser behavior:**

- [Claude Code](session-formats/claude-code.md)
- [Codex](session-formats/codex.md)
- [OpenCode](session-formats/opencode.md) — includes SQLite v2 (`session_v2`/`session_message`), SQLite v1 (≥ 2026-02-14), and legacy JSON
- [Mistral Vibe](session-formats/mistral-vibe.md)
- [Kimi Code](session-formats/kimi-code.md) — current `~/.kimi-code` format; legacy `~/.kimi` is documented but not parsed

**Parser architecture and implementation patterns:** [PARSER_DESIGN.md](PARSER_DESIGN.md)
**Tool call wire formats and normalization:** [TOOL_CALLS_ANALYSIS.md](TOOL_CALLS_ANALYSIS.md)

---

## Implementation Status

- ✅ Claude Code parser + indexer implemented
- ✅ Session date/sort semantics aligned with agent-sessions (Claude: end time = latest message-like event)
- ✅ OpenCode parser implemented with dual-read indexing (SQLite-first + JSON fallback)
- ✅ Codex legacy parser implemented, including archived and compressed rollouts
- ✅ Codex paginated `UserMessage` / `AgentMessage` `item_completed` messages are indexed (2026-10-16); the inherited-history boundary is applied and prompt-less child sessions get a synthetic first prompt
- ⚠️ Codex `history_base` shared-prefix reconstruction, non-message typed items and task-name `wait_agent` / `close_agent` results are not supported
- ✅ Mistral Vibe parser implemented
- ✅ OpenCode parent-child detection implemented (`parentID` sessions are indexed as subagents)
- ⚠️ OpenCode v2 (`session_v2`/`session_message`) is not read yet; indexing targets V1 `session`/`message`/`part` (confirmed 2026-10-14)
- ✅ Codex collab/thread-spawn linkage implemented (child sessions + parent-side subagent rows)
- ✅ Mistral Vibe `task`/`agents/` subagent linkage implemented (child sessions + parent-side subagent rows)
- ✅ Tool-call wire formats documented for Claude, OpenCode, Mistral Vibe, and Codex rollouts
- ✅ LLM model metadata availability mapped (per message vs per turn vs per session)
- ✅ Current parser behavior: tool-call/tool-result content is indexed (Phase 6 delivered)
- ✅ Kimi Code parser + indexer implemented for current `$KIMI_CODE_HOME` sessions (default `~/.kimi-code`); custom locations work when visible in the Flatpak sandbox, while legacy `~/.kimi` sessions are not parsed

---

## Storage Locations

| Tool | Path | Organization |
|------|------|--------------|
| **Claude Code** | `~/.claude/` | Project-specific directories<br>Main session: `~/.claude/projects/-Users-alexm-Repository-<project>/UUID.jsonl`<br>Subagent transcripts appear under `<session-id>/subagents/agent-*.jsonl`, each with an `agent-*.meta.json` metadata sidecar (observed v2.1.148); since v2.1.216+ teammate spawns the filenames are `agent-a<name>-<hash16>.jsonl`<br>Large materialized tool outputs can also appear under `<session-id>/tool-results/`<br>A sibling `<project-dir>/memory/` directory (v2.1.226+) holds `MEMORY.md` + `*.md` memory files — not session data; discovery filters on the `.jsonl` extension |
| **Codex** | `~/.codex/sessions/`<br>`~/.codex/archived_sessions/` | Active sessions: date-sharded `YYYY/MM/DD/rollout-*.jsonl`<br>Archived sessions: flat `rollout-*.jsonl` or cold `rollout-*.jsonl.zst` |
| **OpenCode** | `~/.local/share/opencode/` | **Current (v2, confirmed 2026-10-14)**: SQLite WAL-mode DB, usually `opencode.db` and channel-specific `opencode-<channel>.db` for non-default channels; primary tables `session_v2`, `session_message` (parts embedded in message `content[]`), plus `session_pending`, `session_inbox`, `instruction_state`, `project`, `project_directory`, `worktree`, `todo`, `permission`, `session_share`.<br>**SQLite v1 (≥ 2026-02-14, legacy read model)**: tables `session`, `message`, `part` in the same DB; retained but no longer written under v2.<br>**Legacy (pre-migration)**: Multi-directory JSON under `storage/`: `session/<project>/ses_xxx.json`, `message/ses_xxx/`, `part/msg_xxx/`, `session_diff/ses_xxx.json`. Files are retained post-migration (no auto-cleanup). |
| **Mistral Vibe** | `~/.vibe/logs/session/` | One directory per session:<br>`session_YYYYMMDD_HHMMSS_<shortid>/`<br>Contains `meta.json` + `messages.jsonl`.<br>Subagent traces created by `task` are child session directories under `<parent>/agents/<agent>_YYYYMMDD_HHMMSS_<shortid>/`.<br>Default can be overridden via `VIBE_HOME` or `session_logging.save_dir` in `config.toml`. |
| **Kimi Code** | `$KIMI_CODE_HOME/sessions/` (default `~/.kimi-code/sessions/`) | Grouped by working directory: `sessions/<workDirKey>/<sessionId>/` where `workDirKey` is `wd_<slug>_<sha256-12>` and `sessionId` is `session_<uuid>`.<br>Contains `state.json` + `agents/<agentId>/wire.jsonl` (one journal per agent, `main` + subagents).<br>Top-level `session_index.jsonl` + `workspaces.json` map sessions to workdirs.<br>Custom homes are supported when visible in the Flatpak sandbox. Legacy (pre-migration Python CLI) sessions under `~/.kimi` are not parsed. |

---

## File Format

**Claude Code & Codex** use **JSONL** (JSON Lines):
- One JSON object per line
- UTF-8 encoded
- Append-only chronological events
- Codex can Zstandard-compress cold archived JSONL rollouts as `*.jsonl.zst`

**OpenCode** uses **SQLite (v2 and v1)** or **separate JSON files (legacy)**:
- **Current (v2, confirmed 2026-10-14)**: SQLite WAL-mode database, usually `opencode.db` and `opencode-<channel>.db` for non-default channels. Sessions live in `session_v2`; messages in `session_message`, where assistant parts are embedded in a `content[]` array. Message payloads are stored as JSON in the `data` column.
- **SQLite v1 (≥ 2026-02-14, legacy)**: `session` / `message` / `part` tables in the same database, with parts as separate rows in `part`. Retained but no longer written under v2.
- **Legacy**: One JSON file per session (metadata), separate directories for messages and parts, standard JSON format (not line-delimited). Still present on disk after migration for users who have updated.

**Mistral Vibe** uses a **directory-based format**:
- `meta.json` contains session-level metadata (standard JSON)
- `messages.jsonl` is JSONL (one message per line)
- Messages are OpenAI-style (`role`, `content`, optional `tool_calls`)

**Kimi Code** uses a **directory-based format**:
- `state.json` contains session-level metadata (standard JSON, ISO-8601 timestamps)
- `agents/<agentId>/wire.jsonl` is a JSONL event journal per agent (append-only, used for recovery/replay); first line is a `metadata` envelope with `protocol_version`
- Assistant turns are recorded as loop events (`step.begin` / `content.part` / `tool.call` / `tool.result` / `step.end`) inside `context.append_loop_event` records; wire `time` fields are epoch milliseconds
- Legacy `~/.kimi` wire files use a different envelope (`{"timestamp": <float seconds>, "message": {"type": ..., "payload": ...}}`)

---

## File Naming

| Tool | Pattern | Example |
|------|---------|---------|
| **Claude Code** | `UUID.jsonl` (main), `agent-*.jsonl` (subagent; `agent-a<name>-<hash16>.jsonl` since v2.1.216+ teammate spawns), `agent-*.meta.json` (subagent metadata sidecar), `tool-results/*` (materialized tool output) | `a1b2c3d4-e5f6-7890-abcd-ef1234567890.jsonl`<br>`2a19bf71-3687-49ed-8ae9-8bd15e1522f6/subagents/agent-a60d695.jsonl`<br>`66ae4ab6-e5ea-40f4-8e8f-fb80fd307472/subagents/agent-aimpl-task1-d4584135445167d0.jsonl`<br>`2a19bf71-3687-49ed-8ae9-8bd15e1522f6/subagents/agent-a60d695.meta.json`<br>`82b2d04e-d30e-4370-8e41-f53890baeda1/tool-results/bdw7vxszs.txt` |
| **Codex** | `rollout-*.jsonl`, `rollout-*.jsonl.zst`; revert can add `_<rollout_id>` after the thread ID | `rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl` |
| **OpenCode** | **SQLite (v2 and >= 2026-02-14):** `opencode.db`, `opencode-<channel>.db`<br>**Legacy:** `ses_*.json` | `opencode.db` (default channel)<br>`opencode-dev.db` (non-default channel)<br>`ses_66a71b6f4ffeq796jvvOpJQ04m.json` (legacy) |
| **Mistral Vibe** | `session_YYYYMMDD_HHMMSS_<shortid>/` | `session_20260123_174305_64883c86/` |
| **Kimi Code** | `sessions/wd_<slug>_<sha256-12>/session_<uuid>/` containing `state.json` + `agents/main/wire.jsonl` + `agents/<subagentId>/wire.jsonl` | `sessions/wd_sessions-chronicle_a75d38aead93/session_70d49998-f9d1-4546-ab98-3bba4551a6da/`<br>`agents/main/wire.jsonl`<br>`agents/agent-0/wire.jsonl` (subagent) |

---

## Event Structure Comparison

### Common Fields

| Field Category | Claude Code | Codex | OpenCode | Mistral Vibe | Kimi Code |
|----------------|-------------|-------|----------|-------------|-----------|
| **Event Type** | `type` (`user`, `assistant`, `system`, `summary`, `progress`, `queue-operation`, `saved_hook_context`, `pr-link`, `file-history-snapshot`, `file-history-delta`, `attachment`, `permission-mode`, `last-prompt`, `mode`, `ai-title`, plus v2.1.226+ `atis-latch`, `bridge-session`, `cost-state`, `agent-name`, ...); `system` subtypes observed locally are `local_command`, `turn_duration`, `compact_boundary`, `stop_hook_summary`, `away_summary`, `informational`; local sessions v2.1.216–v2.1.263 carry titles in `ai-title` events and show no `summary` | Rollout envelope `type` (`session_meta`, `event_msg`, `response_item`, `turn_context`, ...); nested `event_msg.payload.type` (`item_completed` for paginated history; historical/protocol variants include `user_message`, `agent_message`, `exec_command_*`, `mcp_tool_call_*`, `collab_agent_*`, `collab_waiting_*`, `collab_close_*`, `collab_resume_*`, ...); tool calls can also appear as `response_item` `function_call` / `function_call_output` | Session metadata only (messages in separate files) | `role` (`system`, `user`, `assistant`, `tool`) in `messages.jsonl`; tool calls on assistant messages via `tool_calls` | Wire record `type` (`context.append_message`, `context.append_loop_event`, `turn.prompt`, `llm.request`, `usage.record`, `config.update`, `plan_mode.*`, `swarm_mode.*`, `task.*`, `goal.*`, `cron.*`, `interaction.*`, `turn.ended`, plus post-0.31.1 additions `prompt.*`, `token_counting.*`, `file_history.*`, `tower_mode.*`, ...; `skill.activate` was renamed `skill.activated` and is no longer persisted at main); nested loop-event `event.type` (`step.begin`, `step.end`, `content.part`, `tool.call`, `tool.result`) |
| **Identity** | `uuid`, `parentUuid` (tree structure), plus `promptId` on user turns, `agentId` in subagent logs, and `logicalParentUuid` on some compaction events | Thread ID at `session_meta.payload.id`; current upstream also carries root `session_id` and can reference another rollout via `history_base`; event-specific IDs like `call_id`, `sender_thread_id`, `receiver_thread_id` | `id`, `parentID` (hierarchical sessions) | `message_id` (UUID, optional) on `user`/`assistant` messages; absent on `tool` role. Tool calls have an `id` and tool responses reference it via `tool_call_id` | Session id in directory name (`session_<uuid>`); loop events carry `uuid`/`stepUuid`, `turnId` + `step`; tool correlation via `toolCallId` (short synthetic ids like `Bash_0`); agents keyed in `state.json.agents` with `parentAgentId` |
| **Timestamp** | `timestamp` (ISO-8601) | Top-level rollout-line `timestamp` (ISO-8601 string) | `time.created`, `time.updated` (session level) | Session-level only in `meta.json`: `start_time`, `end_time` (ISO-8601). No per-message timestamps | Per-record `time` (epoch ms) on every wire record; session-level `createdAt`/`updatedAt` (ISO-8601) in `state.json` |
| **Content** | Nested: `message.content`; tool results also appear inline as `tool_result` blocks, can be duplicated in top-level `toolUseResult`, and large outputs may be materialized under `tool-results/` | Legacy messages in `event_msg.payload.message`; paginated messages in `item_completed` TurnItems (not parsed locally), plus optional `response_item.payload.content[]` blocks; skills can also be injected as `response_item` user messages wrapped in `<skill>...</skill>` | Stored in `message/ses_xxx/` directory + `part/msg_xxx/` | `messages.jsonl` lines with `content`; tool output stored as `role: "tool"` messages | User input in `turn.prompt.input` / `context.append_message.message.content`; assistant text in `content.part` loop events (`part.type`: `text`, `think`, `image_url`, ...); tool output in `tool.result.result.output` (string or content parts) |
| **Model Metadata** | Assistant-level `message.model` (slug). In sampled recent logs: present on `assistant`, absent on `user`; `<synthetic>` appears for local synthetic/error assistant messages. Current local v2.1.87 samples still match this. | `session_meta.payload.model_provider` (optional provider, session-level) + `turn_context.payload.model` (model slug, per turn); historical/protocol `session_configured` can carry `model` + `model_provider_id`, but is not persisted by the verified 0.153.4 policy | Per-message model fields: `user.model.{providerID,modelID}` and assistant `providerID` + `modelID`; `subtask` parts can optionally include delegated model. Session table also carries optional session-level `model` (JSON `{id, providerID, variant?}`) and `agent` columns since ~2026 | No model field in `messages.jsonl` records; session-level `meta.json` can include a full `config` snapshot (`active_model`, `providers`, `models`) when logging is enabled | Per-request `llm.request` (`provider`, `model`, `modelAlias`, per step); session-level model switches via `config.update` (`modelAlias`, `thinkingEffort`); no model field on messages or loop events |

### Key Architectural Differences

**Threading Model:**
- **Claude Code**: Tree structure via `uuid`/`parentUuid` + `isSidechain` flag; some newer compaction events also include `logicalParentUuid`
- **Codex**: Thread-based rollouts (`session_meta.payload.id` thread id); source provenance now comes from structured `session_meta.payload.source` (`cli`, `vscode`, `exec`, custom, or structured `subagent` variants), with direct `parent_thread_id` as a local linkage fallback and additional child-thread linkage visible in historical collab events (`collab_agent_spawn_*`, `collab_agent_interaction_*`, `collab_waiting_*`, `collab_close_*`, `collab_resume_*`) or local response-item tool calls such as `spawn_agent`
- **OpenCode**: Parent-child sessions via `parent_id` (SQLite) / `parentID` (JSON) as subagent sessions; v2 parent-side links are a `task` tool item (`state.metadata.sessionId`) or a `synthetic` message with `metadata.source == "subagent"`
- **Mistral Vibe**: Linear message list within each `messages.jsonl`; tool calls are embedded in assistant messages and resolved by subsequent `tool` role messages. A `task` tool call creates a separate child transcript under `<parent>/agents/`; since v2.0.0 `meta.json.child_sessions` (`ChildSessionLink[]` with `session_id`, `tool_call_id`, `agent`, `relative_path`) can provide deterministic `tool_call_id` → `child_session_id` pairing, but the parser does not currently read it and falls back to chronological best-effort pairing by `agent_profile.name`
- **Kimi Code**: One JSONL journal per agent (`agents/<agentId>/wire.jsonl`); no tree inside a journal beyond `turnId`/`step`/`uuid` sequencing. Subagents get their own journal and are registered in `state.json.agents` with `parentAgentId` (metadata-based linkage, not directory-based); the parent's `Agent` tool call remains a normal `tool.call`/`tool.result` pair. Session forks record lineage via `state.json.forkedFrom`

**Metadata Storage:**
- **Claude Code**: Rich per-event metadata (`cwd`, `gitBranch`, `version`, `sessionId`, `entrypoint`, `slug`) plus assistant-level model slug at `message.model`; user turns can also include `promptId`, and subagent transcripts include `agentId`; recent logs (v2.1.148) add `attributionSkill` on `assistant` events and `sourceToolAssistantUUID` on some tool-result `user` events; v2.1.211+ adds top-level `effort` (reasoning effort) on `assistant` events, and v2.1.220 samples also show `requestId` and a snake_case `session_id` duplicate; v2.1.226+ adds `sessionKind` (`"bg"` for background sessions), `promptSource` (`typed` / `suggestion_accepted` / `system` / `sdk`) on user turns, the rest of the attribution family (`attributionAgent`, `attributionMcpServer`, `attributionMcpTool`), plus `apiBlockIndex`, `turnCompanion`, and `origin`; v2.1.267–v2.1.300 samples add `requestRef` (link to an `api-request` event), `requestedModel`, `advisorModel`, `perTurnEffort`, `thinkingDisplay`/`thinkingDurationMs`, `wireToolInputs`/`wireIngestContext`, `serverClassifierRequest`, and `attributionPlugin` on `assistant` events, plus `permissionDecision`, `turnOrigin`, `turnPosition`, and `serverClassifierContext` on `user` events
- **Codex**: Session metadata (`session_meta`) can include provider (`model_provider`), origin/runtime (`originator`), CLI version (`cli_version`), and structured source/subagent provenance; turn-level metadata (`turn_context`) includes active model slug (`model`)
- **Codex skills**: Explicit invocation appears as `$skill-name` in
  `event_msg.payload.message`; loaded skill content is injected as a separate
  `response_item` user message wrapped in `<skill>...</skill>`
- **OpenCode**: Session-level metadata (`projectID`, `workspace_id?`, `directory`, `version`, optional `title`), plus session-level rollup columns `metadata`, `cost`, `tokens_*`, `agent`, and `model` (JSON); v2 adds `fork_session_id`/`fork_boundary`, `time_idle`/`time_viewed`/`idle_outcome`, and `time_suspended`/`resume_attempts`
- **Mistral Vibe**: Session-level `meta.json` includes environment, optional git info, token/tool usage stats, tools snapshot, and configuration snapshot data
- **Kimi Code**: Session-level `state.json` (`title`, `titleKind` — dual-written with the legacy `isCustomTitle` boolean — `lastPrompt`, `cwd`/`workDir`, `id`, `version`, `agents` map with `parentAgentId` and `labels`, `forkedFrom`, `archivedAt`, `lastTurnReason`, `custom`); per-request `llm.request` records carry provider/model/thinking config plus prompt/tools hashes; top-level `workspaces.json` + `session_index.jsonl` index workdirs and sessions

**Content Access:**
- **Claude Code**: `event.message.content` (nested in JSONL events); some current tool-result user events also include top-level `toolUseResult`
- **Codex**: legacy `event_msg.payload.message` for user/assistant text; paginated
  messages use `item_completed` TurnItems, whose `UserMessage` / `AgentMessage` text the
  parser reads (other items are ignored). Historical tool/collab
  info in event-specific payload fields; tool calls can also appear as
  `response_item` `function_call` / `function_call_output`; loaded skills can
  appear as injected `response_item` user messages wrapped in
  `<skill>...</skill>`
- **OpenCode**: V1 message content lives in `message`/`part`; v2 content lives in
  `session_message.data` with assistant parts embedded in `content[]`. Skill
  loading has a structural marker via a tool item named `skill` (`part.tool ==
  "skill"` in v1, `name == "skill"` in v2)
- **Mistral Vibe**: `messages.jsonl` holds message entries (one JSON object per
  line); exact `/<skill-name>` invocations can appear as injected `SKILL.md`
  user messages, while free-form skill loading can be inferred from assistant
  `read_file` tool calls to `skills/<name>/SKILL.md`
- **Kimi Code**: `turn.prompt.input` for raw user prompts and
  `context.append_loop_event.event.part` for assistant content; skill
  activation is marked by `origin.kind == "skill_activation"` on
  prompt/message records (the former `skill.activate` wire record was renamed
  `skill.activated` upstream and is no longer persisted at main)

**File Organization:**
- **Claude Code**: Main `UUID.jsonl` session file plus additional `agent-*.jsonl` subagent transcripts under `<session-id>/subagents/`; large materialized tool outputs can also appear under `<session-id>/tool-results/`
- **Codex**: JSONL rollouts, optionally compressed in archives; paginated history can inherit a prefix of another rollout through `session_meta.payload.history_base`. A reverted thread can have a distinct rollout ID while keeping its thread ID.
- **OpenCode**: Multi-file structure (metadata + message directories + parts + diffs) or a single SQLite DB; v2 stores sessions in `session_v2` and messages (with embedded parts) in `session_message`
- **Mistral Vibe**: Directory-based session (`meta.json` + `messages.jsonl`)
- **Kimi Code**: Directory-based session (`state.json` + one `wire.jsonl` journal per agent under `agents/`), plus optional `plans/`, `tasks/`, `cron/`, and session diagnostic log

---

## LLM Model Metadata Availability

Goal: determine whether model information is available per message, per turn, and/or per session.

| Tool | Per Message | Per Turn | Per Session | Notes |
|------|-------------|----------|-------------|-------|
| **Claude Code** | ✅ On assistant events as `message.model` (slug). Not present on sampled `user` events. | ❌ No explicit turn-context object in the known JSONL schema | ⚠️ Partial: session/events include `version`; model is currently event/message-level, not a dedicated session object | Observed slugs include `claude-opus-4-6`, `claude-sonnet-4-6`, `claude-opus-4-5-20251101`, `claude-sonnet-4-5-20250929`, `claude-haiku-4-5-20251001`; `<synthetic>` appears on generated fallback/error messages. Current local v2.1.87 sampling still matches this pattern. The 2026-10-18 scan (v2.1.264–v2.1.302) shows only Claude 5 family slugs (`claude-haiku-5-5`, `claude-sonnet-5-5`, `claude-opus-5-5`, `claude-sonnet-5`, `claude-opus-5`) plus `<synthetic>`. |
| **Codex** | ⚠️ Not on `user_message` / `agent_message` payloads | ✅ `turn_context.payload.model` (`TurnContextItem.model`) | ✅/⚠️ `session_meta.payload.model_provider` is optional and provider-only (no guaranteed model slug) | `session_configured` and `model_reroute` are protocol events, but neither is persisted by the verified 0.153.4 policy. |
| **OpenCode** | ✅ User message has `model.{providerID,modelID}` (v1) / `model` on the `user` message (v2); assistant message has `providerID` + `modelID` (v1) / `model: {id, providerID, variant?}` (v2) | N/A (message-centric schema) | ⚠️ Partial: `session`/`session_v2` have optional `model` (JSON `{id, providerID, variant?}`) and `agent` columns (session-level rollups) | V1 `subtask` parts can optionally pin a delegated model (`model.providerID`, `model.modelID`). Per-message model remains the source of truth; session-level `model`/`agent` are rollups, not per-turn data. |
| **Mistral Vibe** | ❌ `messages.jsonl` (`LLMMessage`) has no model key | ❌ No separate turn-context model object in logs | ✅ `meta.json` metadata dump can contain `config` snapshot with `active_model`, plus `providers`/`models` arrays | Requires session logging metadata output; minimal/older logs may omit full config snapshot. |
| **Kimi Code** | ❌ No model field on messages or loop events | ✅ Per step: `llm.request` records carry `provider`, `model`, `modelAlias` | ✅/⚠️ `config.update` records track `modelAlias`/`thinkingEffort` changes over the session | Observed values: `provider: "kimi"`, `model: "kimi-k3"`, `modelAlias: "moonshot-ai/kimi-k3"`. `llm.request` also records `thinkingEffort`, `maxTokens`, `systemPromptHash`, `toolsHash`. |

**Primary evidence:**
- Codex: `codex-rs/protocol/src/protocol.rs` (`SessionMeta`, `TurnContextItem`, `SessionConfiguredEvent`) and `codex-rs/core/src/codex.rs`.
- OpenCode: `packages/core/src/session/sql.ts`, `packages/schema/src/session-message.ts`, `packages/schema/src/session.ts`, and the legacy `packages/schema/src/v1/session.ts`.
- Mistral Vibe: `vibe/core/session/session_logger.py` and `vibe/core/types.py`.
- Claude Code: direct `~/.claude/projects/**/*.jsonl` sampling (2026-02-24, 2026-03-31, 2026-07-27, 2026-09-06 covering v2.1.220–v2.1.263, and 2026-10-18 covering v2.1.264–v2.1.302), fixture comparison, the official changelog, and Anthropic model documentation. Claude Code is closed source, so no upstream types or migrations are available as primary evidence.
- Kimi Code: upstream `packages/agent-core-v2` sources (`wire/record.ts`, `agent/llmRequester/llmRequestOps.ts`, `agent/profile/profileOps.ts`), the generated wire manifest (`packages/agent-core-v2/docs/wire-manifest.d.ts`, diffed between tags 0.31.1 and main/0.41.0 on 2026-09-06), and direct `~/.kimi-code/sessions/**/wire.jsonl` sampling (2026-07-29, 2026-09-06).

---

## Key Findings Summary

- **Claude Code**: JSONL format, tree-structured events, project-based organization; current local logs add `turn_duration` and `compact_boundary` system events, confirm subagent transcripts under `<session-id>/subagents/agent-*.jsonl`, and show large materialized tool output under `<session-id>/tool-results/`; model slug is available on assistant events (`message.model`) in recent logs; **token usage is commonly available per assistant message** (`message.usage`, optional and version-dependent), with cache fields reported separately from uncached input
- **Claude Code new event types/fields (v2.1.148)**: Local sampling adds `attachment` events (hook output, deferred-tool/agent/MCP listing deltas, skill listings, command permissions), `permission-mode` events, and `last-prompt` events, plus an `agent-*.meta.json` subagent metadata sidecar (`agentType`, `description`, `name`, `toolUseId`). The parser dispatches only on `type in {user, assistant}`, so these are skipped without error — docs/fixtures updated, no parser change justified yet
- **Claude Code teammate subagents (v2.1.216–v2.1.220, local scan 2026-07-27)**: `Agent` launches now spawn background "teammates" — `tool_result` text uses snake_case `agent_id: <name>@session-<shortid>` (legacy `agentId:` token no longer observed), structured `toolUseResult` carries `agent_id`/`status: "teammate_spawned"`/`team_name`, nested transcripts are named `agent-a<name>-<hash16>.jsonl`, and the `.meta.json` sidecar dropped `toolUseId` (new fields: `spawnDepth`, `model`, `taskKind`, `teamName`, `color`, `planModeRequired`, `permissionMode`). **The implemented `agentId`-token linkage finds nothing in these sessions, so parent→child subagent navigation is broken for new-format sessions.** New event types also appeared (`mode`, `ai-title`, `file-history-delta`); `ai-title` seems to replace `summary` as the title carrier. See `session-formats/claude-code.md` for details
- **Claude Code new event types/fields (v2.1.226–v2.1.263, local scan 2026-09-06)**: Four more top-level event types appeared — `atis-latch` (`atis` empty in all 59 samples, purpose unknown), `bridge-session` (`bridgeSessionId` `cse_*`, `lastSequenceNum`, `ownerAccountUuid`, `ownerOrganizationUuid`; matches the changelog's v2.1.251 Remote Control streaming entry), `cost-state` (running `totalCostUSD` + per-model `modelUsage` with camelCase token fields), and `agent-name` (session name, seen on a `sessionKind: "bg"` session that also carried `ai-title`). New `attachment.type` variants include `total_tokens_reminder` (166 of 321 attachment events), `task_reminder`, `remote_session_change`, `session_context`, `auto_mode`, `prompt_snapshot`, and `compact_file_reference`; new `system` subtypes are `stop_hook_summary`, `away_summary`, and `informational`. A non-session `<project-dir>/memory/` directory also appeared. **The parser matches only `user`/`assistant`/`ai-title` with a catch-all arm, and discovery filters on `.jsonl`, so nothing here breaks** — docs updated, no parser change justified. `cost-state` and the bridge/background-session surface are open product questions, not drift
- **Claude Code API request recording (v2.1.300+, local scan 2026-10-18)**: Confirmed by the v2.1.300 changelog ("local session transcripts ... also record each API request's system prompt, tools and parameters"). Three content-addressed event types appeared — `api-request` (`params`, `querySource`, `shapeHash`, `tail` blob hashes), `api-request-shape` (system prompt + tool definitions, keyed by `shapeHash`), and `api-request-blob` (`message{role, content[]}` keyed by `hash`, including a new `tool_addition` block) — and `assistant` events gained `requestRef`. Assistant content stays inline, but these lines are 35–69 % of transcript bytes. New `attachment.type` variants include `deferred_tools_record`, `credential_org`, `silent_turn_reminder`, `read_truncation_notice`, `queued_command`, and `workflow_size_guideline_change`; non-session sidecars `bridge-pointer.json` and `<session-id>/ccr-tip.json` also appeared. **Dispatch on top-level `type` with a catch-all arm and the `.jsonl` discovery filter mean nothing breaks** — docs updated, no parser change justified; the extra full-line deserialization cost is unmeasured
- **Claude Code subagent/tool naming**: Current local v2.1.87 sessions show subagent launches as `tool_use` with `name == "Agent"` and `input.subagent_type`; older local fixtures and parser assumptions still reference `Task`
- **Codex**: JSONL rollout envelope (`session_meta`/`event_msg`/`turn_context`/...); active rollouts are date-sharded under `sessions/`, while archived rollouts are flat under `archived_sessions/` and can be Zstandard-compressed as `*.jsonl.zst`; model provider can exist at session level, and model slug is captured at turn level (`turn_context.model`); **token usage is emitted as `event_msg` `token_count` events** (running totals + last-call deltas), where cached input is a subset of `input_tokens`. Sessions Chronicle indexes plain and compressed rollouts and includes sibling archives when the configured directory is named `sessions` or `codex_sessions`.
- **Codex subagents**: The Codex protocol defines `collab_*` lifecycle events, but the verified 0.153.4 persistence policy treats them as transient; historical files can still contain them. Sessions Chronicle indexes `collab_*` spawn/waiting/interaction/close/resume data as parent-side subagents and links child rollouts through structured `session_meta.payload.source.subagent.thread_spawn.parent_thread_id` or `source.sub_agent.thread_spawn.parent_thread_id`. Sessions Chronicle also uses direct `session_meta.payload.parent_thread_id` as a fallback. Local Codex `0.130.0` response-item `spawn_agent` / `wait_agent` pairs are also mapped into parent-side `Subagent` rows and terminal summaries rather than generic tool calls.
- **Codex paginated history (confirmed, 2026-09-06)**: The [0.153.4 persistence policy](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/rollout/src/policy.rs) stores paginated messages as `item_completed`; legacy `user_message` / `agent_message` events are retained only in legacy mode. As of 2026-10-16 the parser reads `UserMessage` / `AgentMessage` completed items in paginated mode (earlier it rejected such files with `NoUserMessages`). Completed subagent activities also use `item_completed`, including in legacy history; non-completed activities use `sub_agent_activity` in legacy mode. Neither activity carrier is parsed locally.
- **Codex shared history (confirmed source contract, 2026-09-06)**: [The lineage resolver at `ac192cd`](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/thread-store/src/local/rollout_lineage.rs) follows `history_base.{thread_id,end_ordinal_exclusive,end_byte_offset}` recursively by rollout ID, using active or archived files. Ancestors contribute frozen prefixes; byte offsets address decompressed JSONL. Missing sources, cycles, non-paginated ancestors, and invalid bounds are errors. Revert preserves thread identity while selecting a new rollout through SQLite; the physical base is distinct from the logical fork parent. `subagent_history_start_ordinal` suppresses inherited context in the child's visible projection, including copied context without a base. These rules are confirmed from source and inspected upstream tests, not an executed reproduction. Local parsing ignores `history_base`, so a child that inherits a shared prefix through it still shows an incomplete transcript (a separate limitation from paginated messages); `subagent_history_start_ordinal` is applied to drop copied child context. See [resolution, compression, revert, and evidence limits](session-formats/codex.md#resolving-a-shared-history).
- **Codex local evidence (observed, 2026-09-06)**: A read-only scan of 348 rollouts under `~/.codex/sessions/` and `~/.codex/archived_sessions/` found 9 paginated files (versions `0.147.0` and `0.153.4`) with `item_completed` and no legacy user/assistant message events. None of 352 metadata records had a non-null `history_base` or `forked_from_ordinal_exclusive`; two had `subagent_history_start_ordinal` without a base. No compressed files were present. Thus local samples confirm pagination and copied-child boundaries, but do not validate shared-prefix reconstruction.
- **Codex skills**: Sampled local rollouts show explicit `$skill-name`
  `user_message` events followed by injected `<skill>` payloads in
  `response_item` user messages. The injected payload includes `<name>` and
  `<path>` headers plus the skill body; unavailable skills can appear as a
  `$skill-name` invocation without a following `<skill>` payload.
- **OpenCode**: **Breaking change ≥ 2026-02-14** — migrated to SQLite (`opencode.db` by default, `opencode-<channel>.db` on non-default channels). Sessions Chronicle indexes SQLite sessions first and falls back to legacy JSON storage, deduplicating by session `id` when both sources contain the same session. Legacy JSON file tree remains relevant for pre-migration/compatibility reads. Model metadata remains message-level; **token usage is commonly available per assistant message** (optional and provider-dependent) and can also appear on step boundaries in V1 (`part.type == "step-finish"` includes `tokens`). Source code moved from `packages/opencode/src/session/` to `packages/core/src/session/` and `@opencode-ai/schema`.
- **OpenCode v2 (confirmed, 2026-10-14)**: The primary store moved to `session_v2` + `session_message` (upstream `dev` `packages/core/src/session/sql.ts`; V1 types retained only under `packages/schema/src/v1/`). `session_message` carries a `type`/`seq` message model (`agent-switched`, `model-switched`, `location-switched`, `user`, `synthetic`, `system`, `skill`, `shell`, `assistant`, `compaction`, `idle`) with assistant parts embedded in `content[]` (`text`, `reasoning`, `tool`) instead of a separate `part` table. The tool part uses `name` (not V1 `tool`) and the tool state is `streaming`/`running`/`completed`/`error` (`pending` is gone; completed output moved from `state.output` to `state.content`). `session_v2` adds `fork_session_id`, `fork_boundary`, `time_suspended`, `resume_attempts`, `time_idle`, `time_viewed`, `idle_outcome` and makes `title` nullable. `session_input`/`session_context_epoch` were replaced by `session_pending`/`session_inbox`/`instruction_*`, and subagents are linked via a `synthetic` message with `metadata.source == "subagent"`. A local v2.0.29 database confirms V1 `session`/`message`/`part` froze on 2026-10-11 while `session_v2`/`session_message` stay current. **The parser only reads V1 tables, so this is a confirmed parser gap.**
- **OpenCode skills**: V1 uses `part.type == "tool"` with `part.tool == "skill"`; V2 uses an assistant `content[]` tool item with `name == "skill"`. Skill identity is available in `state.metadata.name` and `state.input.name`; the injected Markdown is display payload, not the primary detection signal.
- **Mistral Vibe**: Directory-based session format with `meta.json` + JSONL
  `messages.jsonl`; model info is session-level via `meta.json.config` snapshot
  when present, not message-level; **token usage is available when
  `meta.json.stats` is present** (session totals + last-turn metrics, including
  cache-token counters `session_cached_tokens`/`last_turn_cached_tokens` since
  v2.23.2); assistant messages from reasoning-capable models can include
  `reasoning_content` and `reasoning_payloads` fields. Subagents launched
  through `task` are persisted as separate child sessions under
  `<parent>/agents/`; `meta.json.child_sessions` (`ChildSessionLink[]`) can
  provide deterministic `tool_call_id` → `child_session_id` pairing but is not
  currently read by the parser. Sessions Chronicle indexes child sessions and
  surfaces the parent `task` call as a subagent row
- **Mistral Vibe skills**: Two patterns were observed locally. Exact
  `/<skill-name>` invocation is expanded client-side into a `role == "user"`
  message containing the full `SKILL.md` body. Free-form or slash-with-args
  prompts can instead lead to ordinary assistant `tool_calls` to `read_file` on
  `skills/<skill-name>/SKILL.md` (and related files such as `PATHS.md`). No
  dedicated native skill event was found.
- **Kimi Code**: Directory-based session format (`state.json` + per-agent
  `wire.jsonl` journals under `agents/`); sessions grouped by working directory
  (`wd_<slug>_<sha256-12>` buckets) with a top-level `session_index.jsonl` and
  `workspaces.json`; assistant turns are recorded as structured loop events
  (`step.begin` / `content.part` / `tool.call` / `tool.result` / `step.end`)
  rather than flat messages; model metadata is per request (`llm.request`) and
  per session change (`config.update`); **token usage is available per turn**
  (`usage.record`) and per step (`step.end.usage`), with cache tokens
  (`inputCacheRead`, `inputCacheCreation`) reported separately from uncached
  input (`inputOther`) — the same convention as Claude Code. Subagents are
  linked through `state.json.agents[*].parentAgentId` with their own journal.
  The parser discovers current bundles under `$KIMI_CODE_HOME` (default
  `~/.kimi-code`) when visible in the Flatpak sandbox. Legacy `~/.kimi`
  sessions use a different wire envelope and are not parsed. The 0.32–0.41
  upstream releases added 17 wire record types (`prompt.*`, `token_counting.*`,
  `file_history.*`, `tower_mode.*`, ...) and renamed/removed three
  (`skill.activate` → non-durable `skill.activated`, `context_size.measured` →
  `token_counting.*`, `permission.rules.add` → live-only); all of it is
  additive from the parser's perspective, which tolerates unknown records and
  consumes none of the removed types (2026-09-06 watch pass).
- **Kimi Code skills**: Skill activation is marked by
  `origin.kind == "skill_activation"` on `turn.prompt` /
  `context.append_message` records, which also carry structured fields
  (`skillName`, `skillPath`, `skillSource`, `skillArgs`, `activationId`,
  `trigger`). The former `skill.activate` wire record was renamed
  `skill.activated` upstream and is no longer persisted at main (2026-09-06
  watch pass). Other injected content uses `origin.kind` values such as
  `injection` or `system_trigger` (12 kinds exist upstream in total).

---

## Open Questions

1. **Tool/Event Indexing Scope (post-Phase 6)**:
   - Which additional event families should be indexed beyond the current tool-call/tool-result/subagent coverage?
   - Should we keep full structured JSON (`input`, `output`, `metadata`, `attachments`) only, or add normalized text projections for search?

2. **OpenCode Parent-Child Session Display**:
   - Should subagent sessions be shown nested under parents?
   - Or displayed as separate sessions with parent reference?
   - How deep can nesting go?

3. **Codex Retry / Duplicate Thread Display**:
   - When multiple Codex parent subagent rows share the same child session, should the UI surface them as separate attempts or collapse them behind one child-session link?

4. **Codex Subagent Metadata Depth**:
   - Should collab timing fields (`started_at_ms`, `completed_at_ms`) be stored for future subagent duration display?
   - Should spawned-agent `model` and `reasoning_effort` be added to the normalized subagent model?

5. **OpenCode Session Diffs**:
   - Should `session_diff/ses_xxx.json` be ingested for richer "changes made" previews?
   - How should diff metadata be surfaced without overwhelming session list/search?

7. **Image/Attachment Handling in Tool Results**:
   - How should we present tool-result attachments (data URLs, image/pdf, references) safely?
   - Should remote references require explicit user opt-in before fetch?

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

## Token Usage Availability (All Supported Parsers)

Sessions Chronicle supports parsing Claude Code, Codex, OpenCode, Mistral Vibe, and current-format Kimi Code sessions.
Each tool can persist token usage metrics, but **the granularity and presence are tool- and version-dependent**.

| Tool | Where tokens appear | Granularity | Notes |
|------|---------------------|------------|-------|
| **Claude Code** | `assistant` events: `message.usage`; since v2.1.226+ also `cost-state` events (`modelUsage`) | Per assistant message / request, plus a running per-session total | Often includes `input_tokens`, `output_tokens`, plus `cache_read_input_tokens` / `cache_creation_input_tokens`. Anthropic reports cache separately, so `input_tokens` is the uncached portion only. Not present on all historical logs/fixtures. `cost-state` carries the same quantities per model under camelCase names (`inputTokens`, `cacheReadInputTokens`, ...) plus `costUSD`, and sidesteps the append-only duplicate-request problem — but it is emitted repeatedly, so take the last one rather than summing. |
| **Codex** | `event_msg` events: `payload.type == "token_count"` | Running session totals + per-call deltas | `info.total_token_usage` is a running total; `info.last_token_usage` is the last model call. `cached_input_tokens` is the cached subset of `input_tokens`, while `reasoning_output_tokens` is exposed as a separate field in the payload. Some events may have `info: null`. |
| **OpenCode** | V1: assistant message metadata (`message.tokens`) and/or `part.type == "step-finish"`; v2: assistant message `tokens` | Per assistant message and/or per step | Presence depends on provider/backends and OpenCode version; avoid double-counting if both are present. `tokens.cache.read` / `tokens.cache.write` are separate fields, but whether `tokens.input` already includes cache is provider-dependent. |
| **Mistral Vibe** | `meta.json.stats` (`AgentStats`) | Session totals + last turn | `stats` may be `null` in minimal/older logs or when logging is configured without stats. `messages.jsonl` does not include per-message tokens. Since v2.23.2, `stats` includes cache-token counters (`session_cached_tokens`, `last_turn_cached_tokens`, `cached_input_price_per_million`); older logs have prompt/completion aggregates only, and the parser does not currently read the cache fields. No separate reasoning-token counter. Reasoning content itself is available per-message via `reasoning_content` on assistant messages, but not as a separate token counter. `stats` also carries tool-call counters (`tool_calls_agreed/rejected/hook_denied/failed/succeeded`), performance metrics (`steps`, `tokens_per_second`, `last_turn_duration`), and pricing (`input_price_per_million`, `output_price_per_million`, `cached_input_price_per_million`, `session_cost`). |
| **Kimi Code** | `usage.record` wire records (`usageScope: "turn" \| "session"`) and `step.end.usage` | Per turn + per step; `usageScope: "session"` records carry non-turn work (full compaction) | Same shape in both carriers: `inputOther` (uncached input), `output`, `inputCacheRead`, `inputCacheCreation`. Cache is explicit and separate, like Claude Code. `usage.record` also carries the `model`; both scopes are deltas, so a true session total sums every record (Sessions Chronicle sums only `"turn"`). `step.end` adds latency metrics and `finishReason`. |

### Cross-provider token semantics

- `TokenUsage` is a useful normalized shape, but the fields are not perfectly equivalent across assistants.
- `Claude Code`: cache is explicit and separate from uncached `input_tokens`.
- `Codex`: `cached_input_tokens` is included in `input_tokens` as a subset; do not add it on top.
- `OpenCode`: token shape is close to Sessions Chronicle's model, but cache/input overlap depends on the underlying provider.
- `Mistral Vibe`: prompt/completion aggregates are always exposed; cache-token counters (`session_cached_tokens`, `last_turn_cached_tokens`) are available since v2.23.2 but not yet read by the parser.
- `Kimi Code`: cache is explicit and separate (`inputCacheRead`, `inputCacheCreation` vs uncached `inputOther`), same convention as Claude Code; usage is per turn (`usage.record`) and per step (`step.end.usage`), so avoid double-counting if both are summed.

---

## Next Steps for Design

1. **Tool call indexing enhancements (post-Phase 6)**:
   - Expand extraction coverage for less-common tool/subtask/collab variants per parser
   - Keep existing user/assistant + current tool/subagent indexing as baseline behavior

2. **Subagent graph model**:
   - Extend the unified parent-child relation beyond the current OpenCode + Codex + Claude linkage primitives
   - Revisit whether the UI should expose Codex duplicate-thread / retry history explicitly when multiple parent subagent rows point at the same child session

3. **UI surfacing experiment**:
   - Add optional expandable "Tool Activity" and "Subtasks/Subagents" sections in session details
   - Evaluate nested vs flat display using OpenCode fixtures with parent/child sessions

4. **Diff ingestion spike (OpenCode)**:
   - Parse `session_diff/ses_xxx.json` and test lightweight summaries (file count, additions/deletions)

5. **Test parser with edge cases**:
   - Codex paginated `item_completed` messages: covered by synthetic fixtures under `tests/fixtures/codex_sessions/2026/10/15/` and `tests/fixtures/codex_subagent_linkage/2026/10/15/` (2026-10-16)
   - Codex shared history: add source-derived fixtures for frozen/chained prefixes, compressed ancestors, reverts, missing sources, cycles, and copied child context; decide current-rollout selection, incomplete-history reporting, and ancestor index invalidation
   - Empty sessions
   - Malformed JSON/JSONL
   - Missing required fields
   - Very large files (JSONL streaming)
   - OpenCode orphaned data (missing message/part directories)
   - Deep parent-child hierarchies (OpenCode)

---

## Reference Documentation

### Community Format References
- [Claude Code Session Format](https://github.com/jazzyalex/agent-sessions/blob/main/docs/claude-code-session-format.md)
- [Codex Session Storage Format](https://github.com/jazzyalex/agent-sessions/blob/main/docs/session-storage-format.md)
- [Codex Schema Reference](https://github.com/jazzyalex/agent-sessions/blob/main/docs/schemas/session_event.schema.json)

### Codex (Primary Sources)
- [Codex persistence policy, verified at 0.153.4](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/rollout/src/policy.rs)
- [Codex protocol `SessionMeta`, `HistoryPosition`, `EventMsg`, inspected commit](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/protocol/src/protocol.rs)
- [Codex recorder and revert filenames, inspected commit](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/rollout/src/recorder.rs)
- [Codex turn-context persistence (`RolloutItem::TurnContext` before sampling)](https://github.com/openai/codex/blob/main/codex-rs/core/src/codex.rs)
- [Codex rollout recorder writes `session_meta.model_provider`](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/recorder.rs)
- [Codex app-server thread/item event model](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)
- [Codex TypeScript SDK note on session persistence](https://github.com/openai/codex/blob/main/sdk/typescript/README.md)
- [OpenAI Prompt Caching guide (`cached_tokens` within prompt/input usage)](https://developers.openai.com/api/docs/guides/prompt-caching)

### OpenCode
- [Agent Sessions GitHub Repository](https://github.com/jazzyalex/agent-sessions)
- [OpenCode GitHub Repository](https://github.com/anomalyco/opencode)
- [OpenCode Sessions Issue #3026](https://github.com/anomalyco/opencode/issues/3026)
- [OpenCode Sessions Issue #5734](https://github.com/anomalyco/opencode/issues/5734)
- [OpenCode V2 message/part types (`SessionMessage`, `AssistantContent`, `ToolState`)](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/session-message.ts)
- [OpenCode V2 session type (`Session.Info`)](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/session.ts)
- [OpenCode V2 subagent completion marker](https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/subagent-completion.ts)
- [OpenCode SQLite Drizzle schema (`session_v2`/`session_message`/`session_pending`/`session_inbox`/`instruction_*`)](https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/sql.ts)
- [OpenCode V1 part/session types (`Part`, `ToolState`, `Info`, …)](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/v1/session.ts)
- [OpenCode task tool](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/task.ts)

### Claude References
- [Claude API tool-use block structure](https://platform.claude.com/docs/en/api/typescript/messages/create)
- [Claude Code subagents docs](https://docs.anthropic.com/en/docs/claude-code/sub-agents)
- [Claude Code changelog](https://docs.anthropic.com/en/docs/claude-code/changelog)
- [Anthropic prompt caching docs (`cache_read_input_tokens`, `cache_creation_input_tokens`, `input_tokens`)](https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching)
- [Claude Code model configuration (supported slugs)](https://support.claude.com/en/articles/11940350-claude-code-model-configuration)
- [Claude Sonnet 4.6 model page](https://www.anthropic.com/claude/sonnet)
- [Claude Opus 4.6 model page](https://www.anthropic.com/claude/opus)

### Mistral Vibe
- [Mistral Vibe Repository](https://github.com/mistralai/mistral-vibe)
- [Mistral Vibe Configuration Docs](https://docs.mistral.ai/mistral-vibe/introduction/configuration)
- [Mistral Vibe session logger](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/session/session_logger.py)
- [Mistral Vibe `task` tool](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/tools/builtins/task.py)
- [Mistral Vibe `TaskArgs`/`TaskResult` types](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/subagents.py)
- [Mistral Vibe builtin agent profiles](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/agents/models.py)
- [Mistral Vibe message/session models](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/types.py)
- [Mistral Vibe system prompt skill section](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/system_prompt.py)
- [Mistral Vibe CLI skill slash-command handler](https://github.com/mistralai/mistral-vibe/blob/main/vibe/cli/textual_ui/app.py)
- [Mistral Vibe CHANGELOG](https://github.com/mistralai/mistral-vibe/blob/main/CHANGELOG.md)

### Kimi Code
- [Kimi Code CLI Repository](https://github.com/MoonshotAI/kimi-code)
- [Official docs: Sessions and context](https://github.com/MoonshotAI/kimi-code/blob/main/docs/en/guides/sessions.md)
- [Official docs: Data locations](https://github.com/MoonshotAI/kimi-code/blob/main/docs/en/configuration/data-locations.md)
- [Wire record definitions (`packages/agent-core-v2/src/wire/record.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/wire/record.ts)
- [Loop event model (`packages/agent-core-v2/src/agent/contextMemory/loopEventFold.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/agent/contextMemory/loopEventFold.ts)
- [Session metadata contract (`packages/klient/src/contract/session/metadata.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/klient/src/contract/session/metadata.ts)
- [Legacy migration (`packages/migration-legacy`)](https://github.com/MoonshotAI/kimi-code/tree/main/packages/migration-legacy)

---

**Last Updated**: 2026-10-14  
**Status (2026-09-06 Kimi Code watch pass)**: Upstream diffed between tags 0.31.1 (locally installed CLI) and main (0.41.0) using the generated wire manifest (`packages/agent-core-v2/docs/wire-manifest.d.ts`), plus release notes 0.32.0–0.41.0 and a fresh local sample of `~/.kimi-code/sessions/`. Confirmed: 17 new durable wire record types (`prompt.*`, `token_counting.*`, `file_history.*`, `tower_mode.*`, `turn.step.interrupted/retrying`, `task.waitDelivered`, `plugin.session_start`, `runtime.set_binding`), three removals (`skill.activate` → non-durable `skill.activated`, `context_size.measured` → `token_counting.*`, `permission.rules.add` → live-only), `agentId` on all wire payloads, `usage.record.usageScope`, `state.json` moving to `titleKind` (dual-written with `isCustomTitle`) plus `archivedAt` / `lastTurnReason` / `id` / `version` / `cwd` / agent `labels`, a 12-value `origin.kind` union, versioned nested `workspaces.json`, coexisting `protocol_version` 1.4/1.5, and subagent `tool-results/` spill directories. The parser tolerates unknown records, consumes none of the removed types, and already reads both `workDir`/`cwd` and the nested `workspaces.json`, so no parser change is justified; fixtures for the newer record types are still outstanding.  
**Status (2026-09-06 Codex watch pass)**: Persistence policy verified at 0.153.4; source and reconstruction tests inspected at `ac192cd7937b0d73edc6dffe009940ae53782dd4`. Documented paginated-message support gaps, shared-prefix resolution, logical byte offsets, revert identity, child projection, and upstream error handling. Corrected existing archive/compression and direct-parent support. Local read-only sampling covered 348 rollouts, including 9 paginated files but no `history_base` examples. Upstream tests were inspected, not executed; introduction versions remain unknown.

**Status (2026-09-06 Claude Code watch pass)**: Docs refreshed from a local scan of every `~/.claude/projects/**/*.jsonl` touched since 2026-08-01 (2269 events, versions v2.1.220 through v2.1.263), cross-checked against the official changelog. New event types `atis-latch`, `bridge-session`, `cost-state`, and `agent-name`; new fields `sessionKind`, `promptSource`, `attributionAgent`, `attributionMcpServer`/`attributionMcpTool`, `apiBlockIndex`, `turnCompanion`, `origin`, `lastSequenceNum`; sixteen new `attachment.type` variants and three new `system` subtypes; a non-session `<project-dir>/memory/` directory. No `summary` events across the whole range, widening the 2026-07-27 finding. The parser's catch-all match arm and the indexer's `.jsonl` extension filter absorb all of it, so no parser change is justified; fixtures for the new event types are still outstanding.  
**Status (2026-09-06 Mistral Vibe watch pass)**: Upstream source inspected at commit `4530b9ce` (DeepWiki index 2026-08-13, CHANGELOG verified through v2.24.1, 2026-08-11). Confirmed: `meta.json.child_sessions` (`ChildSessionLink[]` with `session_id`, `tool_call_id`, `agent`, `relative_path`) provides deterministic subagent linkage since v2.0.0 but is not read by the parser; `AgentStats` gained cache-token counters (`session_cached_tokens`, `last_turn_cached_tokens`, `cached_input_price_per_million`) in v2.23.2 and `tool_calls_hook_denied` with hooks graduation; `LLMMessage` reasoning fields are `reasoning_content` + `reasoning_payloads` + `reasoning_message_id` (the previously documented `reasoning_signature`/`reasoning_state` do not exist in the upstream type); new `LLMMessage` fields `tool_result` (`PersistedToolResult`), `context_boundary` (`"compaction"`), `user_display_content`, `input_text`, `resources`, `manual_shell`; `meta.json.last_message_fingerprint` for incremental append detection. The `task` tool's `TaskArgs.agent` default is still `"explore"` (the v2.24.1 "Default agent renamed to `ask`" changelog entry refers to the main-session default, not the subagent default). No parser change justified — the parser reads fields with `.get()` so unknown fields are naturally skipped; docs and cross-assistant tables updated. Enrichment opportunities (`child_sessions` deterministic linking, `session_cached_tokens` → `cache_read_tokens`, `tool_result.duration` → `duration_ms`) are held until a confirming fixture is captured.

**Status (2026-10-14 OpenCode v2 watch pass)**: Upstream `dev` branch inspected (`packages/core/src/session/sql.ts`, `packages/schema/src/session-message.ts`, `packages/schema/src/session.ts`, `packages/core/src/session/subagent-completion.ts`, `packages/schema/src/v1/session.ts`), cross-checked against a local OpenCode v2.0.29 database. Confirmed: the primary store is now `session_v2` + `session_message` (V1 `session`/`message`/`part` retained but frozen), parts embedded in assistant `content[]` (`text`/`reasoning`/`tool`), tool field renamed `tool` → `name`, tool state `pending` → `streaming` with output moving to `state.content`, `session_v2` gained fork/execution/idle columns and a nullable `title`, `session_input`/`session_context_epoch` replaced by `session_pending`/`session_inbox`/`instruction_*`, and subagents linked via a `synthetic` `metadata.source == "subagent"` message. The parser still reads only V1 tables, so this is a confirmed parser gap; docs updated and a V2 fixture/regression test is the next step.
**Status (2026-09-06 OpenCode watch pass)**: Upstream `dev` branch inspected (`packages/core/src/session/sql.ts`, `message-v2.ts`, `packages/schema/src/v1/session.ts` and `session-message.ts`). Confirmed: the `session` table gained `metadata`, `cost`, `tokens_input`/`tokens_output`/`tokens_reasoning`/`tokens_cache_read`/`tokens_cache_write`, `agent`, and `model` columns; three new tables appeared (`session_message` with a `type` + `seq` message model, `session_input`, `session_context_epoch`); `part.session_id` is now `NOT NULL`; source code moved from `packages/opencode/src/session/` to `packages/core/src/session/` and `@opencode-ai/schema`. `message`/`part` remain the confirmed transcript write path and the 12 part types plus `pending`/`running`/`completed`/`error` tool states are unchanged, so no parser change is justified; docs updated and source URLs corrected.

**Previous status**: Kimi Code parser and indexer implemented for current `$KIMI_CODE_HOME` sessions (default `~/.kimi-code`) when visible in the Flatpak sandbox. Discovery scans `sessions/wd_*/session_*/`; bundle parsing covers messages, tool calls, token/model metadata, and namespaced synthetic child transcripts linked through `parentAgentId`. Incremental indexing fingerprints `state.json` and every declared agent journal as one composite bundle, and bundles without genuine user-origin messages are not retained. Legacy `~/.kimi` sessions are not parsed. Claude Code docs refreshed from real-session sampling (v2.1.148 logs): new `attachment`, `permission-mode`, and `last-prompt` event types, new `attributionSkill` / `sourceToolAssistantUUID` fields, `isSnapshotUpdate` on file-history snapshots, and the `agent-*.meta.json` subagent metadata sidecar; parser skips the new event types without error, so no parser change is justified yet. Earlier refresh (2026-03-31) covered v2.1.87-era subagent naming (`Agent`), `turn_duration`/`compact_boundary` system events, and `tool-results/` side files. Mistral Vibe docs updated for v2.7.0: new `meta.json` fields (`username`, `title`, `total_messages`, `system_prompt`), new optional `LLMMessage` fields (`message_id`, `reasoning_content`), system message placement corrected. Mistral Vibe docs refreshed again from upstream source through v2.14.1 (2026-06-09 watch pass): new `SessionMetadata` fields (`parent_session_id`, `title_source`, `loops`, `experiments`), new `LLMMessage` fields (`images`, `injected`, `reasoning_message_id`), expanded `AgentStats` (tool-call counters, performance metrics, per-million pricing), the new `read`/`edit` tool-call format, and directory-based `task` subagent traces under `<parent>/agents/`. The parser's subagent behavior matches upstream; same-profile parallel call-to-child pairing remains best-effort because child metadata contains no parent tool-call id.
