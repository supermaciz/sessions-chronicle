# Session Format Analysis


Cross-tool comparison of Claude Code, Codex, OpenCode, Mistral Vibe, and Kimi Code session file formats.

**Per-tool format details and parser behavior:**

- [Claude Code](session-formats/claude-code.md)
- [Codex](session-formats/codex.md)
- [OpenCode](session-formats/opencode.md) — includes SQLite (≥ 2026-02-14) and legacy JSON
- [Mistral Vibe](session-formats/mistral-vibe.md)
- [Kimi Code](session-formats/kimi-code.md) — current `~/.kimi-code` format; legacy `~/.kimi` is documented but not parsed

**Parser architecture and implementation patterns:** [PARSER_DESIGN.md](PARSER_DESIGN.md)
**Tool call wire formats and normalization:** [TOOL_CALLS_ANALYSIS.md](TOOL_CALLS_ANALYSIS.md)

---

## Implementation Status

- ✅ Claude Code parser + indexer implemented
- ✅ Session date/sort semantics aligned with agent-sessions (Claude: end time = latest message-like event)
- ✅ OpenCode parser implemented with dual-read indexing (SQLite-first + JSON fallback)
- ✅ Codex parser implemented
- ✅ Mistral Vibe parser implemented
- ✅ OpenCode parent-child detection implemented (`parentID` sessions are indexed as subagents)
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
| **Claude Code** | `~/.claude/` | Project-specific directories<br>Main session: `~/.claude/projects/-Users-alexm-Repository-<project>/UUID.jsonl`<br>Subagent transcripts appear under `<session-id>/subagents/agent-*.jsonl`, each with an `agent-*.meta.json` metadata sidecar (observed v2.1.148); since v2.1.216+ teammate spawns the filenames are `agent-a<name>-<hash16>.jsonl`<br>Large materialized tool outputs can also appear under `<session-id>/tool-results/` |
| **Codex** | `~/.codex/sessions/`<br>`~/.codex/archived_sessions/` | Active sessions: date-sharded `YYYY/MM/DD/rollout-*.jsonl`<br>Archived sessions: flat `rollout-*.jsonl` or cold `rollout-*.jsonl.zst` |
| **OpenCode** | `~/.local/share/opencode/` | **New (≥ 2026-02-14)**: SQLite WAL-mode DB, usually `opencode.db` and channel-specific `opencode-<channel>.db` for non-default channels; tables: `session`, `message`, `part`, `project`, `todo`, `permission`, `session_share`.<br>**Legacy (pre-migration)**: Multi-directory JSON under `storage/`: `session/<project>/ses_xxx.json`, `message/ses_xxx/`, `part/msg_xxx/`, `session_diff/ses_xxx.json`. Files are retained post-migration (no auto-cleanup). |
| **Mistral Vibe** | `~/.vibe/logs/session/` | One directory per session:<br>`session_YYYYMMDD_HHMMSS_<shortid>/`<br>Contains `meta.json` + `messages.jsonl`.<br>Subagent traces created by `task` are child session directories under `<parent>/agents/<agent>_YYYYMMDD_HHMMSS_<shortid>/`.<br>Default can be overridden via `VIBE_HOME` or `session_logging.save_dir` in `config.toml`. |
| **Kimi Code** | `$KIMI_CODE_HOME/sessions/` (default `~/.kimi-code/sessions/`) | Grouped by working directory: `sessions/<workDirKey>/<sessionId>/` where `workDirKey` is `wd_<slug>_<sha256-12>` and `sessionId` is `session_<uuid>`.<br>Contains `state.json` + `agents/<agentId>/wire.jsonl` (one journal per agent, `main` + subagents).<br>Top-level `session_index.jsonl` + `workspaces.json` map sessions to workdirs.<br>Custom homes are supported when visible in the Flatpak sandbox. Legacy (pre-migration Python CLI) sessions under `~/.kimi` are not parsed. |

---

## File Format

**Claude Code & Codex** use **JSONL** (JSON Lines):
- One JSON object per line
- UTF-8 encoded
- Append-only chronological events
- Codex can Zstandard-compress cold archived JSONL rollouts as `*.jsonl.zst`

**OpenCode** uses **SQLite (new)** or **separate JSON files (legacy)**:
- **New (≥ 2026-02-14)**: SQLite WAL-mode database, usually `opencode.db` and `opencode-<channel>.db` for non-default channels. Message and part content are stored as JSON blobs in the `data` column.
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
| **Codex** | `rollout-*.jsonl` | `rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl` |
| **OpenCode** | **New (>= 2026-02-14):** `opencode.db`, `opencode-<channel>.db`<br>**Legacy:** `ses_*.json` | `opencode.db` (default channel)<br>`opencode-dev.db` (non-default channel)<br>`ses_66a71b6f4ffeq796jvvOpJQ04m.json` (legacy) |
| **Mistral Vibe** | `session_YYYYMMDD_HHMMSS_<shortid>/` | `session_20260123_174305_64883c86/` |
| **Kimi Code** | `sessions/wd_<slug>_<sha256-12>/session_<uuid>/` containing `state.json` + `agents/main/wire.jsonl` + `agents/<subagentId>/wire.jsonl` | `sessions/wd_sessions-chronicle_a75d38aead93/session_70d49998-f9d1-4546-ab98-3bba4551a6da/`<br>`agents/main/wire.jsonl`<br>`agents/agent-0/wire.jsonl` (subagent) |

---

## Event Structure Comparison

### Common Fields

| Field Category | Claude Code | Codex | OpenCode | Mistral Vibe | Kimi Code |
|----------------|-------------|-------|----------|-------------|-----------|
| **Event Type** | `type` (`user`, `assistant`, `system`, `summary`, `progress`, `queue-operation`, `saved_hook_context`, `pr-link`, `file-history-snapshot`, `file-history-delta`, `attachment`, `permission-mode`, `last-prompt`, `mode`, `ai-title`, ...); current `system` subtypes observed locally include `local_command`, `turn_duration`, and `compact_boundary`; recent local sessions (v2.1.216+) carry titles in `ai-title` events and no longer show `summary` | Rollout envelope `type` (`session_meta`, `event_msg`, `response_item`, `turn_context`, ...); nested `event_msg.payload.type` (`user_message`, `agent_message`, `exec_command_*`, `mcp_tool_call_*`, `collab_agent_*`, `collab_waiting_*`, `collab_close_*`, `collab_resume_*`, ...); tool calls can also appear as `response_item` `function_call` / `function_call_output` | Session metadata only (messages in separate files) | `role` (`system`, `user`, `assistant`, `tool`) in `messages.jsonl`; tool calls on assistant messages via `tool_calls` | Wire record `type` (`context.append_message`, `context.append_loop_event`, `turn.prompt`, `llm.request`, `usage.record`, `config.update`, `skill.activate`, `plan_mode.*`, `swarm_mode.*`, `task.*`, `goal.*`, ...); nested loop-event `event.type` (`step.begin`, `step.end`, `content.part`, `tool.call`, `tool.result`) |
| **Identity** | `uuid`, `parentUuid` (tree structure), plus `promptId` on user turns, `agentId` in subagent logs, and `logicalParentUuid` on some compaction events | Session id at `session_meta.payload.id`; event-specific IDs like `call_id`, `sender_thread_id`, `receiver_thread_id` | `id`, `parentID` (hierarchical sessions) | `message_id` (UUID, optional) on `user`/`assistant` messages; absent on `tool` role. Tool calls have an `id` and tool responses reference it via `tool_call_id` | Session id in directory name (`session_<uuid>`); loop events carry `uuid`/`stepUuid`, `turnId` + `step`; tool correlation via `toolCallId` (short synthetic ids like `Bash_0`); agents keyed in `state.json.agents` with `parentAgentId` |
| **Timestamp** | `timestamp` (ISO-8601) | Top-level rollout-line `timestamp` (ISO-8601 string) | `time.created`, `time.updated` (session level) | Session-level only in `meta.json`: `start_time`, `end_time` (ISO-8601). No per-message timestamps | Per-record `time` (epoch ms) on every wire record; session-level `createdAt`/`updatedAt` (ISO-8601) in `state.json` |
| **Content** | Nested: `message.content`; tool results also appear inline as `tool_result` blocks, can be duplicated in top-level `toolUseResult`, and large outputs may be materialized under `tool-results/` | Usually in `event_msg.payload` (for example `message`, command output deltas, MCP results), plus optional `response_item.payload.content[]` blocks; skills can also be injected as `response_item` user messages wrapped in `<skill>...</skill>` | Stored in `message/ses_xxx/` directory + `part/msg_xxx/` | `messages.jsonl` lines with `content`; tool output stored as `role: "tool"` messages | User input in `turn.prompt.input` / `context.append_message.message.content`; assistant text in `content.part` loop events (`part.type`: `text`, `think`, `image_url`, ...); tool output in `tool.result.result.output` (string or content parts) |
| **Model Metadata** | Assistant-level `message.model` (slug). In sampled recent logs: present on `assistant`, absent on `user`; `<synthetic>` appears for local synthetic/error assistant messages. Current local v2.1.87 samples still match this. | `session_meta.payload.model_provider` (optional provider, session-level) + `turn_context.payload.model` (model slug, per turn); `event_msg.payload.type == "session_configured"` can also carry `model` + `model_provider_id` | Per-message model fields: `user.model.{providerID,modelID}` and assistant `providerID` + `modelID`; `subtask` parts can optionally include delegated model | No model field in `messages.jsonl` records; session-level `meta.json` can include a full `config` snapshot (`active_model`, `providers`, `models`) when logging is enabled | Per-request `llm.request` (`provider`, `model`, `modelAlias`, per step); session-level model switches via `config.update` (`modelAlias`, `thinkingEffort`); no model field on messages or loop events |

### Key Architectural Differences

**Threading Model:**
- **Claude Code**: Tree structure via `uuid`/`parentUuid` + `isSidechain` flag; some newer compaction events also include `logicalParentUuid`
- **Codex**: Thread-based rollouts (`session_meta.payload.id` thread id); source provenance now comes from structured `session_meta.payload.source` (`cli`, `vscode`, `exec`, custom, or structured `subAgent` variants), with additional child-thread linkage visible in collab events (`collab_agent_spawn_*`, `collab_agent_interaction_*`, `collab_waiting_*`, `collab_close_*`, `collab_resume_*`) or local response-item tool calls such as `spawn_agent`
- **OpenCode**: Parent-child sessions via `parentID` (subagent sessions)
- **Mistral Vibe**: Linear message list within each `messages.jsonl`; tool calls are embedded in assistant messages and resolved by subsequent `tool` role messages. A `task` tool call creates a separate child transcript under `<parent>/agents/`; the child does not persist the parent tool-call id, so repeated same-profile calls require chronological best-effort pairing
- **Kimi Code**: One JSONL journal per agent (`agents/<agentId>/wire.jsonl`); no tree inside a journal beyond `turnId`/`step`/`uuid` sequencing. Subagents get their own journal and are registered in `state.json.agents` with `parentAgentId` (metadata-based linkage, not directory-based); the parent's `Agent` tool call remains a normal `tool.call`/`tool.result` pair. Session forks record lineage via `state.json.forkedFrom`

**Metadata Storage:**
- **Claude Code**: Rich per-event metadata (`cwd`, `gitBranch`, `version`, `sessionId`, `entrypoint`, `slug`) plus assistant-level model slug at `message.model`; user turns can also include `promptId`, and subagent transcripts include `agentId`; recent logs (v2.1.148) add `attributionSkill` on `assistant` events and `sourceToolAssistantUUID` on some tool-result `user` events; v2.1.211+ adds top-level `effort` (reasoning effort) on `assistant` events, and v2.1.220 samples also show `requestId` and a snake_case `session_id` duplicate
- **Codex**: Session metadata (`session_meta`) can include provider (`model_provider`), origin/runtime (`originator`), CLI version (`cli_version`), and structured source/subagent provenance; turn-level metadata (`turn_context`) includes active model slug (`model`)
- **Codex skills**: Explicit invocation appears as `$skill-name` in
  `event_msg.payload.message`; loaded skill content is injected as a separate
  `response_item` user message wrapped in `<skill>...</skill>`
- **OpenCode**: Session-level metadata (`projectID`, `workspaceID?`, `directory`, `version`, `title`)
- **Mistral Vibe**: Session-level `meta.json` includes environment, optional git info, token/tool usage stats, tools snapshot, and configuration snapshot data
- **Kimi Code**: Session-level `state.json` (`title`, `isCustomTitle`, `lastPrompt`, `workDir`, `agents` map, `forkedFrom`, `custom`); per-request `llm.request` records carry provider/model/thinking config plus prompt/tools hashes; top-level `workspaces.json` + `session_index.jsonl` index workdirs and sessions

**Content Access:**
- **Claude Code**: `event.message.content` (nested in JSONL events); some current tool-result user events also include top-level `toolUseResult`
- **Codex**: `event_msg.payload.message` for user/assistant text; tool/collab
  info in event-specific payload fields; tool calls can also appear as
  `response_item` `function_call` / `function_call_output`; loaded skills can
  appear as injected `response_item` user messages wrapped in
  `<skill>...</skill>`
- **OpenCode**: Message content lives in `message`/`part`; skill loading has a
  structural marker via `part.type == "tool"` and `part.tool == "skill"`
- **Mistral Vibe**: `messages.jsonl` holds message entries (one JSON object per
  line); exact `/<skill-name>` invocations can appear as injected `SKILL.md`
  user messages, while free-form skill loading can be inferred from assistant
  `read_file` tool calls to `skills/<name>/SKILL.md`
- **Kimi Code**: `turn.prompt.input` for raw user prompts and
  `context.append_loop_event.event.part` for assistant content; skill
  activation has a structural marker via the `skill.activate` wire record and
  `origin.kind == "skill_activation"` on prompt/message records

**File Organization:**
- **Claude Code**: Main `UUID.jsonl` session file plus additional `agent-*.jsonl` subagent transcripts under `<session-id>/subagents/`; large materialized tool outputs can also appear under `<session-id>/tool-results/`
- **Codex**: Single JSONL file per session
- **OpenCode**: Multi-file structure (metadata + message directories + parts + diffs) or single SQLite DB
- **Mistral Vibe**: Directory-based session (`meta.json` + `messages.jsonl`)
- **Kimi Code**: Directory-based session (`state.json` + one `wire.jsonl` journal per agent under `agents/`), plus optional `plans/`, `tasks/`, `cron/`, and session diagnostic log

---

## LLM Model Metadata Availability

Goal: determine whether model information is available per message, per turn, and/or per session.

| Tool | Per Message | Per Turn | Per Session | Notes |
|------|-------------|----------|-------------|-------|
| **Claude Code** | ✅ On assistant events as `message.model` (slug). Not present on sampled `user` events. | ❌ No explicit turn-context object in the known JSONL schema | ⚠️ Partial: session/events include `version`; model is currently event/message-level, not a dedicated session object | Observed slugs include `claude-opus-4-6`, `claude-sonnet-4-6`, `claude-opus-4-5-20251101`, `claude-sonnet-4-5-20250929`, `claude-haiku-4-5-20251001`; `<synthetic>` appears on generated fallback/error messages. Current local v2.1.87 sampling still matches this pattern. |
| **Codex** | ⚠️ Not on `user_message` / `agent_message` payloads | ✅ `turn_context.payload.model` (`TurnContextItem.model`) | ✅/⚠️ `session_meta.payload.model_provider` is optional and provider-only (no guaranteed model slug) | `event_msg.payload.type == "session_configured"` can provide `model` + `model_provider_id`; reroutes can be observed via `model_reroute` events. |
| **OpenCode** | ✅ User message has `model.{providerID,modelID}` and assistant message has `providerID` + `modelID` | N/A (message-centric schema) | ❌ Session metadata has no model field | `subtask` parts can optionally pin delegated model (`model.providerID`, `model.modelID`). Session metadata now also has optional `workspaceID`, but still no session-level model field. |
| **Mistral Vibe** | ❌ `messages.jsonl` (`LLMMessage`) has no model key | ❌ No separate turn-context model object in logs | ✅ `meta.json` metadata dump can contain `config` snapshot with `active_model`, plus `providers`/`models` arrays | Requires session logging metadata output; minimal/older logs may omit full config snapshot. |
| **Kimi Code** | ❌ No model field on messages or loop events | ✅ Per step: `llm.request` records carry `provider`, `model`, `modelAlias` | ✅/⚠️ `config.update` records track `modelAlias`/`thinkingEffort` changes over the session | Observed values: `provider: "kimi"`, `model: "kimi-k3"`, `modelAlias: "moonshot-ai/kimi-k3"`. `llm.request` also records `thinkingEffort`, `maxTokens`, `systemPromptHash`, `toolsHash`. |

**Primary evidence:**
- Codex: `codex-rs/protocol/src/protocol.rs` (`SessionMeta`, `TurnContextItem`, `SessionConfiguredEvent`) and `codex-rs/core/src/codex.rs`.
- OpenCode: `packages/opencode/src/session/message-v2.ts` and `packages/sdk/js/src/v2/gen/types.gen.ts`.
- Mistral Vibe: `vibe/core/session/session_logger.py` and `vibe/core/types.py`.
- Claude Code: direct `~/.claude/projects/**/*.jsonl` sampling (2026-02-24 and 2026-03-31), fixture comparison, and Anthropic model documentation.
- Kimi Code: upstream `packages/agent-core-v2` sources (`wire/record.ts`, `agent/llmRequester/llmRequestOps.ts`, `agent/profile/profileOps.ts`) and direct `~/.kimi-code/sessions/**/wire.jsonl` sampling (2026-07-29).

---

## Key Findings Summary

- **Claude Code**: JSONL format, tree-structured events, project-based organization; current local logs add `turn_duration` and `compact_boundary` system events, confirm subagent transcripts under `<session-id>/subagents/agent-*.jsonl`, and show large materialized tool output under `<session-id>/tool-results/`; model slug is available on assistant events (`message.model`) in recent logs; **token usage is commonly available per assistant message** (`message.usage`, optional and version-dependent), with cache fields reported separately from uncached input
- **Claude Code new event types/fields (v2.1.148)**: Local sampling adds `attachment` events (hook output, deferred-tool/agent/MCP listing deltas, skill listings, command permissions), `permission-mode` events, and `last-prompt` events, plus an `agent-*.meta.json` subagent metadata sidecar (`agentType`, `description`, `name`, `toolUseId`). The parser dispatches only on `type in {user, assistant}`, so these are skipped without error — docs/fixtures updated, no parser change justified yet
- **Claude Code teammate subagents (v2.1.216–v2.1.220, local scan 2026-07-27)**: `Agent` launches now spawn background "teammates" — `tool_result` text uses snake_case `agent_id: <name>@session-<shortid>` (legacy `agentId:` token no longer observed), structured `toolUseResult` carries `agent_id`/`status: "teammate_spawned"`/`team_name`, nested transcripts are named `agent-a<name>-<hash16>.jsonl`, and the `.meta.json` sidecar dropped `toolUseId` (new fields: `spawnDepth`, `model`, `taskKind`, `teamName`, `color`, `planModeRequired`, `permissionMode`). **The implemented `agentId`-token linkage finds nothing in these sessions, so parent→child subagent navigation is broken for new-format sessions.** New event types also appeared (`mode`, `ai-title`, `file-history-delta`); `ai-title` seems to replace `summary` as the title carrier. See `session-formats/claude-code.md` for details
- **Claude Code subagent/tool naming**: Current local v2.1.87 sessions show subagent launches as `tool_use` with `name == "Agent"` and `input.subagent_type`; older local fixtures and parser assumptions still reference `Task`
- **Codex**: JSONL rollout envelope (`session_meta`/`event_msg`/`turn_context`/...); active rollouts are date-sharded under `sessions/`, while archived rollouts are flat under `archived_sessions/` and can be Zstandard-compressed as `*.jsonl.zst`; model provider can exist at session level, and model slug is captured at turn level (`turn_context.model`); **token usage is emitted as `event_msg` `token_count` events** (running totals + last-call deltas), where cached input is a subset of `input_tokens`. Sessions Chronicle currently indexes only active plain `rollout-*.jsonl` files.
- **Codex subagents**: Current upstream Codex protocol defines subagent lifecycle data through `collab_*` events. Sessions Chronicle indexes `collab_*` spawn/waiting/interaction/close/resume data as parent-side subagents and links child rollouts through structured `session_meta.payload.source.subagent.thread_spawn.parent_thread_id` or `source.sub_agent.thread_spawn.parent_thread_id`. Current upstream `session_meta` also carries a direct `parent_thread_id`, which Sessions Chronicle does not yet use as a linkage fallback. Local Codex `0.130.0` response-item `spawn_agent` / `wait_agent` pairs are also mapped into parent-side `Subagent` rows and terminal summaries rather than generic tool calls.
- **Codex skills**: Sampled local rollouts show explicit `$skill-name`
  `user_message` events followed by injected `<skill>` payloads in
  `response_item` user messages. The injected payload includes `<name>` and
  `<path>` headers plus the skill body; unavailable skills can appear as a
  `$skill-name` invocation without a following `<skill>` payload.
- **OpenCode**: **Breaking change ≥ 2026-02-14** — migrated to SQLite (`opencode.db` by default, `opencode-<channel>.db` on non-default channels). Sessions Chronicle now indexes SQLite sessions first and falls back to legacy JSON storage, deduplicating by session `id` when both sources contain the same session. Legacy JSON file tree remains relevant for pre-migration/compatibility reads. Data schema has continued to evolve: session rows now include optional `workspace_id` / `workspaceID`; newer part types include `file`, `agent`, `retry`, `patch`; `compaction` parts can include optional `overflow`; part ID prefix in SQLite era is `prt_`. Model metadata remains message-level; **token usage is commonly available per assistant message** (`message.tokens`, optional and provider-dependent) and can also appear on step boundaries (`part.type == "step-finish"` includes `tokens`).
- **OpenCode skills**: Skill invocations have a reliable structural marker in
  `part.type == "tool"` with `part.tool == "skill"`. Skill identity is
  available in `state.metadata.name` and `state.input.name`; the injected
  Markdown in the parent user message is display payload, not the primary
  detection signal.
- **Mistral Vibe**: Directory-based session format with `meta.json` + JSONL
  `messages.jsonl`; model info is session-level via `meta.json.config` snapshot
  when present, not message-level; **token usage is available when
  `meta.json.stats` is present** (session totals + last-turn metrics); assistant
  messages from reasoning-capable models can include `reasoning_content` and
  `reasoning_signature` fields. Subagents launched through `task` are persisted
  as separate child sessions under `<parent>/agents/`; Sessions Chronicle
  indexes them and surfaces the parent `task` call as a subagent row
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
  sessions use a different wire envelope and are not parsed.
- **Kimi Code skills**: Skill activation has a structural marker — a
  `skill.activate` wire record — and skill-driven prompts carry
  `origin.kind == "skill_activation"` on `turn.prompt` /
  `context.append_message` records. Other injected content uses
  `origin.kind` values `injection` or `system_trigger`.

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
| **Claude Code** | `assistant` events: `message.usage` | Per assistant message / request | Often includes `input_tokens`, `output_tokens`, plus `cache_read_input_tokens` / `cache_creation_input_tokens`. Anthropic reports cache separately, so `input_tokens` is the uncached portion only. Not present on all historical logs/fixtures. |
| **Codex** | `event_msg` events: `payload.type == "token_count"` | Running session totals + per-call deltas | `info.total_token_usage` is a running total; `info.last_token_usage` is the last model call. `cached_input_tokens` is the cached subset of `input_tokens`, while `reasoning_output_tokens` is exposed as a separate field in the payload. Some events may have `info: null`. |
| **OpenCode** | Assistant message metadata (`message.tokens`) and/or `part.type == "step-finish"` | Per assistant message and/or per step | Presence depends on provider/backends and OpenCode version; avoid double-counting if both are present. `tokens.cache.read` / `tokens.cache.write` are separate fields, but whether `tokens.input` already includes cache is provider-dependent. |
| **Mistral Vibe** | `meta.json.stats` (`AgentStats`) | Session totals + last turn | `stats` may be `null` in minimal/older logs or when logging is configured without stats. `messages.jsonl` does not include per-message tokens; no separate cache/reasoning token counters in `stats`. Reasoning content itself is available per-message via `reasoning_content` on assistant messages, but not as a separate token counter. `stats` also carries tool-call counters (`tool_calls_agreed/rejected/failed/succeeded`), performance metrics (`steps`, `tokens_per_second`, `last_turn_duration`), and pricing (`input_price_per_million`, `output_price_per_million`, `session_cost`). |
| **Kimi Code** | `usage.record` wire records (`usageScope: "turn"`) and `step.end.usage` | Per turn + per step | Same shape in both carriers: `inputOther` (uncached input), `output`, `inputCacheRead`, `inputCacheCreation`. Cache is explicit and separate, like Claude Code. `usage.record` also carries the `model`. `step.end` adds latency metrics and `finishReason`. |

### Cross-provider token semantics

- `TokenUsage` is a useful normalized shape, but the fields are not perfectly equivalent across assistants.
- `Claude Code`: cache is explicit and separate from uncached `input_tokens`.
- `Codex`: `cached_input_tokens` is included in `input_tokens` as a subset; do not add it on top.
- `OpenCode`: token shape is close to Sessions Chronicle's model, but cache/input overlap depends on the underlying provider.
- `Mistral Vibe`: only prompt/completion aggregates are exposed in current logs.
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
- [Codex Schema Reference](https://github.com/jazzyalex/agent-sessions/blob/main/docs/schemas/session_event.schema.json)

### Codex (Primary Sources)
- [Codex protocol `RolloutItem`, `SessionMeta`, `EventMsg`](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs)
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
- [OpenCode `MessageV2` part schemas](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/message-v2.ts)
- [OpenCode task tool](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/task.ts)
- [OpenCode generated v2 SDK types](https://github.com/anomalyco/opencode/blob/dev/packages/sdk/js/src/v2/gen/types.gen.ts)
- [OpenCode session schema](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/index.ts)

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
- [Mistral Vibe message/session models](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/types.py)
- [Mistral Vibe system prompt skill section](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/system_prompt.py)
- [Mistral Vibe CLI skill slash-command handler](https://github.com/mistralai/mistral-vibe/blob/main/vibe/cli/textual_ui/app.py)

### Kimi Code
- [Kimi Code CLI Repository](https://github.com/MoonshotAI/kimi-code)
- [Official docs: Sessions and context](https://github.com/MoonshotAI/kimi-code/blob/main/docs/en/guides/sessions.md)
- [Official docs: Data locations](https://github.com/MoonshotAI/kimi-code/blob/main/docs/en/configuration/data-locations.md)
- [Wire record definitions (`packages/agent-core-v2/src/wire/record.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/wire/record.ts)
- [Loop event model (`packages/agent-core-v2/src/agent/contextMemory/loopEventFold.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/agent/contextMemory/loopEventFold.ts)
- [Session metadata contract (`packages/klient/src/contract/session/metadata.ts`)](https://github.com/MoonshotAI/kimi-code/blob/main/packages/klient/src/contract/session/metadata.ts)
- [Legacy migration (`packages/migration-legacy`)](https://github.com/MoonshotAI/kimi-code/tree/main/packages/migration-legacy)

---

**Last Updated**: 2026-07-29
**Status**: Kimi Code parser and indexer implemented for current `$KIMI_CODE_HOME` sessions (default `~/.kimi-code`) when visible in the Flatpak sandbox. Discovery scans `sessions/wd_*/session_*/`; bundle parsing covers messages, tool calls, token/model metadata, and namespaced synthetic child transcripts linked through `parentAgentId`. Incremental indexing fingerprints `state.json` and every declared agent journal as one composite bundle, and bundles without genuine user-origin messages are not retained. Legacy `~/.kimi` sessions are not parsed. Claude Code docs refreshed from real-session sampling (v2.1.148 logs): new `attachment`, `permission-mode`, and `last-prompt` event types, new `attributionSkill` / `sourceToolAssistantUUID` fields, `isSnapshotUpdate` on file-history snapshots, and the `agent-*.meta.json` subagent metadata sidecar; parser skips the new event types without error, so no parser change is justified yet. Earlier refresh (2026-03-31) covered v2.1.87-era subagent naming (`Agent`), `turn_duration`/`compact_boundary` system events, and `tool-results/` side files. Mistral Vibe docs updated for v2.7.0: new `meta.json` fields (`username`, `title`, `total_messages`, `system_prompt`), new optional `LLMMessage` fields (`message_id`, `reasoning_content`, `reasoning_signature`), system message placement corrected. Mistral Vibe docs refreshed again from upstream source through v2.14.1 (2026-06-09 watch pass): new `SessionMetadata` fields (`parent_session_id`, `title_source`, `loops`, `experiments`), new `LLMMessage` fields (`images`, `injected`, `reasoning_state`, `reasoning_message_id`), expanded `AgentStats` (tool-call counters, performance metrics, per-million pricing), the new `read`/`edit` tool-call format, and directory-based `task` subagent traces under `<parent>/agents/`. The parser's subagent behavior matches upstream; same-profile parallel call-to-child pairing remains best-effort because child metadata contains no parent tool-call id.
