# Session Format Analysis

Cross-tool comparison of Claude Code, Codex, OpenCode, and Mistral Vibe session file formats.

**Per-tool format details and parser behavior:**

- [Claude Code](session-formats/claude-code.md)
- [Codex](session-formats/codex.md)
- [OpenCode](session-formats/opencode.md) — includes SQLite (≥ 2026-02-14) and legacy JSON
- [Mistral Vibe](session-formats/mistral-vibe.md)

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
- ✅ Tool-call wire formats documented for Claude, OpenCode, Mistral Vibe, and Codex rollouts
- ✅ LLM model metadata availability mapped (per message vs per turn vs per session)
- ✅ Current parser behavior: tool-call/tool-result content is indexed (Phase 6 delivered)

---

## Storage Locations

| Tool | Path | Organization |
|------|------|--------------|
| **Claude Code** | `~/.claude/` | Project-specific directories<br>Main session: `~/.claude/projects/-Users-alexm-Repository-<project>/UUID.jsonl`<br>Subagent transcripts appear under `<session-id>/subagents/agent-*.jsonl`<br>Large materialized tool outputs can also appear under `<session-id>/tool-results/` |
| **Codex** | `~/.codex/sessions/` | Date-sharded directories<br>`YYYY/MM/DD/rollout-*.jsonl` |
| **OpenCode** | `~/.local/share/opencode/` | **New (≥ 2026-02-14)**: SQLite WAL-mode DB, usually `opencode.db` and channel-specific `opencode-<channel>.db` for non-default channels; tables: `session`, `message`, `part`, `project`, `todo`, `permission`, `session_share`.<br>**Legacy (pre-migration)**: Multi-directory JSON under `storage/`: `session/<project>/ses_xxx.json`, `message/ses_xxx/`, `part/msg_xxx/`, `session_diff/ses_xxx.json`. Files are retained post-migration (no auto-cleanup). |
| **Mistral Vibe** | `~/.vibe/logs/session/` | One directory per session:<br>`session_YYYYMMDD_HHMMSS_<shortid>/`<br>Contains `meta.json` + `messages.jsonl`.<br>Default can be overridden via `VIBE_HOME` or `session_logging.save_dir` in `config.toml`. |

---

## File Format

**Claude Code & Codex** use **JSONL** (JSON Lines):
- One JSON object per line
- UTF-8 encoded
- Append-only chronological events

**OpenCode** uses **SQLite (new)** or **separate JSON files (legacy)**:
- **New (≥ 2026-02-14)**: SQLite WAL-mode database, usually `opencode.db` and `opencode-<channel>.db` for non-default channels. Message and part content are stored as JSON blobs in the `data` column.
- **Legacy**: One JSON file per session (metadata), separate directories for messages and parts, standard JSON format (not line-delimited). Still present on disk after migration for users who have updated.

**Mistral Vibe** uses a **directory-based format**:
- `meta.json` contains session-level metadata (standard JSON)
- `messages.jsonl` is JSONL (one message per line)
- Messages are OpenAI-style (`role`, `content`, optional `tool_calls`)

---

## File Naming

| Tool | Pattern | Example |
|------|---------|---------|
| **Claude Code** | `UUID.jsonl` (main), `agent-*.jsonl` (subagent), `tool-results/*` (materialized tool output) | `a1b2c3d4-e5f6-7890-abcd-ef1234567890.jsonl`<br>`2a19bf71-3687-49ed-8ae9-8bd15e1522f6/subagents/agent-a60d695.jsonl`<br>`82b2d04e-d30e-4370-8e41-f53890baeda1/tool-results/bdw7vxszs.txt` |
| **Codex** | `rollout-*.jsonl` | `rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl` |
| **OpenCode** | **New (>= 2026-02-14):** `opencode.db`, `opencode-<channel>.db`<br>**Legacy:** `ses_*.json` | `opencode.db` (default channel)<br>`opencode-dev.db` (non-default channel)<br>`ses_66a71b6f4ffeq796jvvOpJQ04m.json` (legacy) |
| **Mistral Vibe** | `session_YYYYMMDD_HHMMSS_<shortid>/` | `session_20260123_174305_64883c86/` |

---

## Event Structure Comparison

### Common Fields

| Field Category | Claude Code | Codex | OpenCode | Mistral Vibe |
|----------------|-------------|-------|----------|-------------|
| **Event Type** | `type` (`user`, `assistant`, `system`, `summary`, `progress`, `queue-operation`, `saved_hook_context`, `pr-link`, `file-history-snapshot`, ...); current `system` subtypes observed locally include `local_command`, `turn_duration`, and `compact_boundary` | Rollout envelope `type` (`session_meta`, `event_msg`, `response_item`, `turn_context`, ...); nested `event_msg.payload.type` (`user_message`, `agent_message`, `exec_command_*`, `mcp_tool_call_*`, `collab_*`, ...) | Session metadata only (messages in separate files) | `role` (`system`, `user`, `assistant`, `tool`) in `messages.jsonl`; tool calls on assistant messages via `tool_calls` |
| **Identity** | `uuid`, `parentUuid` (tree structure), plus `promptId` on user turns, `agentId` in subagent logs, and `logicalParentUuid` on some compaction events | Session id at `session_meta.payload.id`; event-specific IDs like `call_id`, `sender_thread_id`, `receiver_thread_id` | `id`, `parentID` (hierarchical sessions) | `message_id` (UUID, optional) on `user`/`assistant` messages; absent on `tool` role. Tool calls have an `id` and tool responses reference it via `tool_call_id` |
| **Timestamp** | `timestamp` (ISO-8601) | Top-level rollout-line `timestamp` (ISO-8601 string) | `time.created`, `time.updated` (session level) | Session-level only in `meta.json`: `start_time`, `end_time` (ISO-8601). No per-message timestamps |
| **Content** | Nested: `message.content`; tool results also appear inline as `tool_result` blocks, can be duplicated in top-level `toolUseResult`, and large outputs may be materialized under `tool-results/` | Usually in `event_msg.payload` (for example `message`, command output deltas, MCP results), plus optional `response_item.payload.content[]` blocks; skills can also be injected as `response_item` user messages wrapped in `<skill>...</skill>` | Stored in `message/ses_xxx/` directory + `part/msg_xxx/` | `messages.jsonl` lines with `content`; tool output stored as `role: "tool"` messages |
| **Model Metadata** | Assistant-level `message.model` (slug). In sampled recent logs: present on `assistant`, absent on `user`; `<synthetic>` appears for local synthetic/error assistant messages. Current local v2.1.87 samples still match this. | `session_meta.payload.model_provider` (optional provider, session-level) + `turn_context.payload.model` (model slug, per turn); `event_msg.payload.type == "session_configured"` can also carry `model` + `model_provider_id` | Per-message model fields: `user.model.{providerID,modelID}` and assistant `providerID` + `modelID`; `subtask` parts can optionally include delegated model | No model field in `messages.jsonl` records; session-level `meta.json` can include a full `config` snapshot (`active_model`, `providers`, `models`) when logging is enabled |

### Key Architectural Differences

**Threading Model:**
- **Claude Code**: Tree structure via `uuid`/`parentUuid` + `isSidechain` flag; some newer compaction events also include `logicalParentUuid`
- **Codex**: Thread-based rollouts (`session_meta.payload.id` thread id); source provenance now comes from structured `session_meta.payload.source` (`cli`, `vscode`, `exec`, custom, or structured `subAgent` variants), with additional child-thread linkage visible in collab events (`collab_agent_spawn_*`, `collab_resume_*`, ...)
- **OpenCode**: Parent-child sessions via `parentID` (subagent sessions)
- **Mistral Vibe**: Linear message list in `messages.jsonl`; tool calls are embedded in assistant messages and resolved by subsequent `tool` role messages

**Metadata Storage:**
- **Claude Code**: Rich per-event metadata (`cwd`, `gitBranch`, `version`, `sessionId`, `entrypoint`, `slug`) plus assistant-level model slug at `message.model`; user turns can also include `promptId`, and subagent transcripts include `agentId`
- **Codex**: Session metadata (`session_meta`) can include provider (`model_provider`), origin/runtime (`originator`), CLI version (`cli_version`), and structured source/subagent provenance; turn-level metadata (`turn_context`) includes active model slug (`model`)
- **Codex skills**: Explicit invocation appears as `$skill-name` in
  `event_msg.payload.message`; loaded skill content is injected as a separate
  `response_item` user message wrapped in `<skill>...</skill>`
- **OpenCode**: Session-level metadata (`projectID`, `workspaceID?`, `directory`, `version`, `title`)
- **Mistral Vibe**: Session-level `meta.json` includes environment, optional git info, token/tool usage stats, tools snapshot, and configuration snapshot data

**Content Access:**
- **Claude Code**: `event.message.content` (nested in JSONL events); some current tool-result user events also include top-level `toolUseResult`
- **Codex**: `event_msg.payload.message` for user/assistant text; tool/collab
  info in event-specific payload fields; loaded skills can appear as injected
  `response_item` user messages wrapped in `<skill>...</skill>`
- **OpenCode**: Message content lives in `message`/`part`; skill loading has a
  structural marker via `part.type == "tool"` and `part.tool == "skill"`
- **Mistral Vibe**: `messages.jsonl` holds message entries (one JSON object per
  line); exact `/<skill-name>` invocations can appear as injected `SKILL.md`
  user messages, while free-form skill loading can be inferred from assistant
  `read_file` tool calls to `skills/<name>/SKILL.md`

**File Organization:**
- **Claude Code**: Main `UUID.jsonl` session file plus additional `agent-*.jsonl` subagent transcripts under `<session-id>/subagents/`; large materialized tool outputs can also appear under `<session-id>/tool-results/`
- **Codex**: Single JSONL file per session
- **OpenCode**: Multi-file structure (metadata + message directories + parts + diffs) or single SQLite DB
- **Mistral Vibe**: Directory-based session (`meta.json` + `messages.jsonl`)

---

## LLM Model Metadata Availability

Goal: determine whether model information is available per message, per turn, and/or per session.

| Tool | Per Message | Per Turn | Per Session | Notes |
|------|-------------|----------|-------------|-------|
| **Claude Code** | ✅ On assistant events as `message.model` (slug). Not present on sampled `user` events. | ❌ No explicit turn-context object in the known JSONL schema | ⚠️ Partial: session/events include `version`; model is currently event/message-level, not a dedicated session object | Observed slugs include `claude-opus-4-6`, `claude-sonnet-4-6`, `claude-opus-4-5-20251101`, `claude-sonnet-4-5-20250929`, `claude-haiku-4-5-20251001`; `<synthetic>` appears on generated fallback/error messages. Current local v2.1.87 sampling still matches this pattern. |
| **Codex** | ⚠️ Not on `user_message` / `agent_message` payloads | ✅ `turn_context.payload.model` (`TurnContextItem.model`) | ✅/⚠️ `session_meta.payload.model_provider` is optional and provider-only (no guaranteed model slug) | `event_msg.payload.type == "session_configured"` can provide `model` + `model_provider_id`; reroutes can be observed via `model_reroute` events. |
| **OpenCode** | ✅ User message has `model.{providerID,modelID}` and assistant message has `providerID` + `modelID` | N/A (message-centric schema) | ❌ Session metadata has no model field | `subtask` parts can optionally pin delegated model (`model.providerID`, `model.modelID`). Session metadata now also has optional `workspaceID`, but still no session-level model field. |
| **Mistral Vibe** | ❌ `messages.jsonl` (`LLMMessage`) has no model key | ❌ No separate turn-context model object in logs | ✅ `meta.json` metadata dump can contain `config` snapshot with `active_model`, plus `providers`/`models` arrays | Requires session logging metadata output; minimal/older logs may omit full config snapshot. |

**Primary evidence:**
- Codex: `codex-rs/protocol/src/protocol.rs` (`SessionMeta`, `TurnContextItem`, `SessionConfiguredEvent`) and `codex-rs/core/src/codex.rs`.
- OpenCode: `packages/opencode/src/session/message-v2.ts` and `packages/sdk/js/src/v2/gen/types.gen.ts`.
- Mistral Vibe: `vibe/core/session/session_logger.py` and `vibe/core/types.py`.
- Claude Code: direct `~/.claude/projects/**/*.jsonl` sampling (2026-02-24 and 2026-03-31), fixture comparison, and Anthropic model documentation.

---

## Key Findings Summary

- **Claude Code**: JSONL format, tree-structured events, project-based organization; current local logs add `turn_duration` and `compact_boundary` system events, confirm subagent transcripts under `<session-id>/subagents/agent-*.jsonl`, and show large materialized tool output under `<session-id>/tool-results/`; model slug is available on assistant events (`message.model`) in recent logs; **token usage is commonly available per assistant message** (`message.usage`, optional and version-dependent), with cache fields reported separately from uncached input
- **Claude Code subagent/tool naming**: Current local v2.1.87 sessions show subagent launches as `tool_use` with `name == "Agent"` and `input.subagent_type`; older local fixtures and parser assumptions still reference `Task`
- **Codex**: JSONL rollout envelope (`session_meta`/`event_msg`/`turn_context`/...); model provider can exist at session level, and model slug is captured at turn level (`turn_context.model`); **token usage is emitted as `event_msg` `token_count` events** (running totals + last-call deltas), where cached input is a subset of `input_tokens`
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
  `reasoning_signature` fields (currently not indexed by the parser)
- **Mistral Vibe skills**: Two patterns were observed locally. Exact
  `/<skill-name>` invocation is expanded client-side into a `role == "user"`
  message containing the full `SKILL.md` body. Free-form or slash-with-args
  prompts can instead lead to ordinary assistant `tool_calls` to `read_file` on
  `skills/<skill-name>/SKILL.md` (and related files such as `PATHS.md`). No
  dedicated native skill event was found.

---

## Open Questions

1. **Tool/Event Indexing Scope (post-Phase 6)**:
   - Which additional event families should be indexed beyond the current tool-call/tool-result/subagent coverage?
   - Should we keep full structured JSON (`input`, `output`, `metadata`, `attachments`) only, or add normalized text projections for search?

2. **OpenCode Parent-Child Session Display**:
   - Should subagent sessions be shown nested under parents?
   - Or displayed as separate sessions with parent reference?
   - How deep can nesting go?

3. **Codex Collaboration Mapping**:
   - Should Codex `collab_*` events map to the same "subagent" concept as OpenCode `parentID` sessions?
   - Do we show child thread IDs as navigable links in the UI?

4. **OpenCode Session Diffs**:
   - Should `session_diff/ses_xxx.json` be ingested for richer "changes made" previews?
   - How should diff metadata be surfaced without overwhelming session list/search?

5. **Image/Attachment Handling in Tool Results**:
   - How should we present tool-result attachments (data URLs, image/pdf, references) safely?
   - Should remote references require explicit user opt-in before fetch?

6. **Error Handling for Malformed Data**:
   - How should parser handle malformed JSON/JSONL lines?
   - Skip and continue, or fail entire session?
   - What about missing required fields?
   - Recommendation: Log warnings, skip problematic entries, continue indexing

7. **Memory Management for Large Sessions**:
   - What's the practical limit for session size?
   - Should large messages be truncated for display?
   - How to handle sessions with 10,000+ messages?
   - Consider pagination or virtual scrolling in UI

---

## Token Usage Availability (All Supported Parsers)

Sessions Chronicle supports parsing Claude Code, Codex, OpenCode, and Mistral Vibe sessions.
Each tool can persist token usage metrics, but **the granularity and presence are tool- and version-dependent**.

| Tool | Where tokens appear | Granularity | Notes |
|------|---------------------|------------|-------|
| **Claude Code** | `assistant` events: `message.usage` | Per assistant message / request | Often includes `input_tokens`, `output_tokens`, plus `cache_read_input_tokens` / `cache_creation_input_tokens`. Anthropic reports cache separately, so `input_tokens` is the uncached portion only. Not present on all historical logs/fixtures. |
| **Codex** | `event_msg` events: `payload.type == "token_count"` | Running session totals + per-call deltas | `info.total_token_usage` is a running total; `info.last_token_usage` is the last model call. `cached_input_tokens` is the cached subset of `input_tokens`, while `reasoning_output_tokens` is exposed as a separate field in the payload. Some events may have `info: null`. |
| **OpenCode** | Assistant message metadata (`message.tokens`) and/or `part.type == "step-finish"` | Per assistant message and/or per step | Presence depends on provider/backends and OpenCode version; avoid double-counting if both are present. `tokens.cache.read` / `tokens.cache.write` are separate fields, but whether `tokens.input` already includes cache is provider-dependent. |
| **Mistral Vibe** | `meta.json.stats` | Session totals + last turn | `stats` may be `null` in minimal/older logs or when logging is configured without stats. `messages.jsonl` does not include per-message tokens; no separate cache/reasoning token counters in `stats`. Reasoning content itself is available per-message via `reasoning_content` on assistant messages, but not as a separate token counter. |

### Cross-provider token semantics

- `TokenUsage` is a useful normalized shape, but the fields are not perfectly equivalent across assistants.
- `Claude Code`: cache is explicit and separate from uncached `input_tokens`.
- `Codex`: `cached_input_tokens` is included in `input_tokens` as a subset; do not add it on top.
- `OpenCode`: token shape is close to Sessions Chronicle's model, but cache/input overlap depends on the underlying provider.
- `Mistral Vibe`: only prompt/completion aggregates are exposed in current logs.

---

## Next Steps for Design

1. **Tool call indexing enhancements (post-Phase 6)**:
   - Expand extraction coverage for less-common tool/subtask/collab variants per parser
   - Keep existing user/assistant + current tool/subagent indexing as baseline behavior

2. **Subagent graph model**:
   - Prototype a unified parent-child relation that can represent:
     - OpenCode session-level `parentID`
     - Codex collab/thread-spawn links
     - Claude sidechain indicators

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
- [Mistral Vibe message/session models](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/types.py)
- [Mistral Vibe system prompt skill section](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/system_prompt.py)
- [Mistral Vibe CLI skill slash-command handler](https://github.com/mistralai/mistral-vibe/blob/main/vibe/cli/textual_ui/app.py)

---

**Last Updated**: 2026-03-31
**Status**: Claude Code docs refreshed from recent real-session sampling (v2.1.87-era logs), including current subagent naming (`Agent`), `turn_duration`/`compact_boundary` system events, and `tool-results/` side files; remaining parser/fixture drift is documented. Mistral Vibe docs updated for v2.7.0: new `meta.json` fields (`username`, `title`, `total_messages`, `system_prompt`), new optional `LLMMessage` fields (`message_id`, `reasoning_content`, `reasoning_signature`), system message placement corrected.
