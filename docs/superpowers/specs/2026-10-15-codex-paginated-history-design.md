# Codex paginated history — design

**Date:** 2026-10-15  
**Status:** Approved design  
**Scope:** Codex parser (`crates/core/src/parsers/codex.rs`), Codex subagent
linkage (`crates/core/src/database/indexer.rs`), schema migration v18  
**Format reference:** [docs/session-formats/codex.md](../../session-formats/codex.md)

## Problem

Codex now rewrites legacy rollouts as `paginated` through its
`legacy_to_paginated_v1` startup migration. In `paginated` mode the transcript
text is persisted only as `event_msg.payload.type == "item_completed"` carrying
typed turn items; legacy `event_msg.user_message` / `agent_message` events are
no longer written.

The parser reads only the legacy events and rejects any rollout without a
`user_message` (`ParseError::NoUserMessages`). A read-only scan of the local
`~/.codex` on 2026-10-15 found:

| Observation | Count |
|-------------|-------|
| Rollouts | 413 |
| `history_mode: "paginated"` | 413 |
| Legacy `event_msg.user_message` events, all files | 0 |
| Rollouts without any `UserMessage` item | 30 |
| — root sessions with ≤ 5 lines (empty) | 14 |
| — subagent child sessions | 16 |
| Rollouts with non-null `history_base` | 0 |
| Rollouts with `CommandExecution` items | 83, all also containing `function_call` response items |

By code inspection, every local Codex session is therefore rejected on the next
index run.

Two subagent issues compound this for the newer multi-agent surface (16 local
spawns):

1. A child session has no readable task prompt. Its inbound
   `response_item.payload.type == "agent_message"` carries only a clear-text
   header (`Message Type`, `Task name`, `Sender`, empty `Payload:`) plus
   `encrypted_content`. Its only clear `role: user` message is injected context
   (`<recommended_plugins>`). The task prompt is in clear only on the parent
   side, in the `spawn_agent` call's `message` argument.
2. `spawn_agent` output is `{"task_name": "/root/<name>"}` with no `agent_id`.
   `link_codex_subagents_tx` links only through `agent_id`, so the parent row is
   never linked to its child.

## Goals

- Index `paginated` Codex rollouts with their user and assistant messages.
- Keep `legacy` parsing byte-for-byte unchanged in behavior.
- Index subagent child sessions that have no user message, with a readable
  synthetic first prompt.
- Link parent subagent rows to child sessions by `task_name` / `agent_path`,
  independent of indexing order.

## Non-goals

- `history_base` shared-prefix reconstruction (no local occurrence).
- Rendering `CommandExecution`, `FileChange`, `McpToolCall`, `WebSearch`,
  `Extension`, `Plan`, review-mode, or other non-message turn items. Tool calls
  and reasoning keep arriving through `response_item`.
- Non-text `UserMessage` content (`skill`, `mention`, `local_image`).
- `token_usage_record`, `file_ids`, `file_id_details`, `image_order`.
- Attaching `wait_agent` / `close_agent` results whose targets are task names
  instead of agent IDs. Result summaries for task-name subagents may stay empty;
  this is a follow-up.
- Deduplicating ChatGPT desktop imports against their Claude Code originals.
- OpenCode v2 (separate design).

## Design

### 1. History-mode-gated message source

Chosen over reading both sources with heuristic deduplication (fragile) and
over reading `response_item` messages (they include injected `developer`,
`AGENTS.md`, and plugin context).

- Read `history_mode` from the first `session_meta` payload. Absent or any
  value other than `"paginated"` means legacy, matching Codex's deserialization
  default.
- **Legacy:** unchanged. `user_message` / `agent_message` produce messages;
  `item_completed` is ignored for messages.
- **Paginated:** `user_message` / `agent_message` events are ignored;
  `item_completed` is handled:
  - `item.type == "UserMessage"` → `Role::User`. Content is the `text` values of
    `item.content[]` entries whose `type` equals `text` case-insensitively,
    joined with `"\n"`. Other entry types are skipped. If no text remains, no
    message is pushed.
  - `item.type == "AgentMessage"` → `Role::Assistant`, same text extraction
    (the observed type is `Text`), with `current_turn_model` as the model. Both
    `phase` values (`commentary`, `final_answer`) and absent `phase` are kept;
    `phase` is not stored.
  - Any other `item.type` is ignored.
  - Timestamp: the envelope `timestamp`, as for every other event, falling back
    to `Utc::now()` like the existing handlers.
- Messages go through the existing `push_message`, so transcript ordering and
  reasoning attachment work unchanged relative to the `response_item` tool calls
  and reasoning in the same stream.

### 2. Inherited fork context

When `session_meta.payload.subagent_history_start_ordinal` is an integer `N`,
`item_completed` messages on lines whose envelope `ordinal` is below `N` are
skipped: upstream treats those records as inherited model context that produces
no child turn items. Lines without an `ordinal` are kept. Tool calls and
reasoning are not filtered (out of scope; they are not observed below the
boundary locally).

### 3. Synthetic prompt for child sessions

A child session is one whose `parent_session_id` is resolved by the existing
`extract_parent_thread_id` / `parent_thread_id` logic.

At the end of parsing, if `!has_user_message`:

- **Root session:** return `ParseError::NoUserMessages`, as today.
- **Child session:** prepend a synthetic `Role::User` message instead of
  rejecting:
  - Content: `Delegated task: <name>`, where `<name>` is the last non-empty
    `/`-separated segment of `session_meta.payload.agent_path`; else
    `session_meta.payload.agent_nickname`; else the content is
    `Delegated task`.
  - Timestamp: session `start_time`; model: `None`.
  - Prepending shifts every existing index by one: `Message.index`,
    `TranscriptItem.item_index`, `TranscriptItem.message_index` (when `Some`),
    and `ReasoningAttachment.transcript_item_index`. The synthetic message gets
    message index `0` and transcript item index `0`.
  - The synthetic message becomes `first_prompt` through the existing
    `extract_first_prompt`.
- A child with no other content is still indexed, so the parent row has a link
  target.

This applies to both history modes; legacy children with a `user_message` are
unaffected.

### 4. Task-name linkage

**Parser, parent side.** In `complete_pending_spawn`, read `task_name` from the
parsed `spawn_agent` output. When present and non-empty, store it in
`Subagent.agent_name`. When neither `nickname` nor an existing title applies,
the row title is the last segment of `task_name`. `agent_id` handling is
unchanged; outputs may carry either or both.

**Parser, child side.** Add `agent_path: Option<String>` to `ParsedSession`,
set by the Codex parser from `session_meta.payload.agent_path` and `None` for
every other parser.

**Schema v18.** `apply_v18_migration` adds nullable `sessions.agent_path TEXT`
(guarded by a column-existence check like earlier migrations), clears
`file_fingerprints` so the next incremental run reparses already-indexed
sessions and populates `agent_path` / `agent_name` (the v17 precedent), and sets
`PRAGMA user_version = 18`. `CURRENT_DB_VERSION` becomes `18`. The fresh-schema
`CREATE TABLE sessions` path includes the column. `upsert_session_row_tx` writes
`parsed.agent_path`.

**Indexer.** `link_codex_subagents_tx` keeps the `agent_id` paths and adds a
name path in both directions, mirroring `link_teammate_child_tx` /
`child_sessions_named`:

- **Indexing a child** with `parent_session_id` and `agent_path`: find parent
  rows `subagents WHERE session_id = parent AND agent_name = agent_path`, and
  sibling sessions `WHERE parent_session_id = parent AND agent_path = agent_path`.
  Link only when there is exactly one row and exactly one sibling (the child
  itself). If siblings are ambiguous, set `child_session_id = NULL` on matching
  rows (retracting an earlier link) and log a warning.
- **Indexing a parent:** for each subagent with `agent_name` and no `agent_id`
  link, if `agent_name` is unique among the parent's subagents and exactly one
  session has `parent_session_id = parent AND agent_path = agent_name`, set
  `child_session_id`. Otherwise leave it unlinked and log at `warn`.

The linkage runs after `upsert_session_row_tx`, like the existing calls.

### Error handling

Session files remain untrusted. Missing or malformed `item`, `content`,
`ordinal`, `history_mode`, `agent_path`, or spawn output fields are skipped
without failing the session, matching the parser's current tolerance. Parsing
stays line-streamed through `BufReader`.

## Testing

**Fixtures (synthetic, anonymized, shapes from the local scan):**

1. `tests/fixtures/codex_sessions/2026/10/15/` — a `paginated` root rollout with
   `UserMessage` / `AgentMessage` items (`text` and `Text` entry types, both
   phases), `function_call` + output, `reasoning`, a `CommandExecution` item
   duplicating the function call, and legacy-style `user_message` noise that
   must be ignored.
2. Same directory — an empty `paginated` root rollout (meta, `task_started`,
   `turn_context` only) that must be rejected.
3. `tests/fixtures/codex_subagent_linkage/2026/10/15/` — a parent issuing
   `spawn_agent` with `task_name` and output `{"task_name": ...}`, and its child
   (`source.subagent.thread_spawn`, `agent_path`, inbound encrypted
   `agent_message`, `<recommended_plugins>` user message, no `UserMessage`).
4. Same directory — a fork child with `subagent_history_start_ordinal` and
   `UserMessage` / `AgentMessage` items on both sides of the boundary.

**Unit tests (`codex.rs`):** mode selection (absent / legacy / paginated);
case-insensitive text extraction and skipped non-text entries; ignored
non-message items, with no duplicate tool call; legacy events ignored in
paginated mode; ordinal filtering; synthetic prompt content fallbacks
(`agent_path`, `agent_nickname`, bare) and index shifting including reasoning
attachments; root without user message still rejected; `task_name` stored in
`agent_name`.

**Schema tests (`schema.rs`):** v17 → v18 adds `agent_path`, clears
fingerprints, is idempotent; fresh databases include the column.

**Integration tests (`tests/`):** parent-then-child and child-then-parent
indexing both link; an ambiguous duplicate `agent_path` leaves rows unlinked and
retracts an earlier link; existing Codex fixtures and linkage tests stay green
(legacy regression).

**Manual verification:** run with `--sessions-dir tests/fixtures`, then index
the real `~/.codex`. With the 2026-10-15 data, the expectation is 399 indexed
Codex sessions (413 minus the 14 empty roots) and 16 linked task-name children.

## Documentation

- `docs/session-formats/codex.md`: update **Parser Behavior**, replace the
  "Confirmed local gap" notes with the implemented behavior, and record the
  2026-10-15 local scan (all files paginated; encrypted child task prompt;
  `task_name` spawn output).
- `docs/SESSION_FORMAT_ANALYSIS.md`: replace the ⚠️ line about paginated
  `item_completed` with the implemented scope, keeping `history_base` listed as
  unsupported.
