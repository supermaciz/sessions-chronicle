# Codex Subagent Support Design

**Date:** 2026-04-18  
**Status:** Implemented [#122](https://github.com/supermaciz/sessions-chronicle/pull/122)

## Problem

Sessions Chronicle currently indexes Codex user/assistant messages, tool calls,
reasoning attachments, and token usage, but it does not map Codex collaboration
events into the existing subagent model.

As a result:

- Codex child rollouts are always indexed as normal top-level sessions
- parent Codex sessions do not produce `Subagent` rows from `collab_*` events
- parent transcripts lose the delegation structure that Claude Code and OpenCode
  already surface
- there is no link from a parent Codex delegation to the indexed child rollout,
  even when both files exist locally

Fresh local evidence shows that Codex now persists both sides of the linkage:

- child rollouts can declare parent provenance in
  `session_meta.payload.source.subagent.thread_spawn.parent_thread_id`
- parent rollouts emit `collab_agent_spawn_end`, `collab_waiting_end`,
  `collab_close_end`, and `collab_agent_interaction_end` events containing child
  thread IDs plus result/status data

That means Codex can fit the current local `Session` + `Subagent` model without
new database tables or a Codex-specific UI path.

## Goal

Index Codex child rollouts as first-class subagent sessions and link them to
parent-session `Subagent` rows extracted from `collab_*` events so the existing
subagent UI can reuse the same navigation model as Claude Code and OpenCode.

## Non-Goals

- Do not invent synthetic child sessions from parent `collab_*` events alone.
- Do not add new database tables or a Codex-only schema.
- Do not redesign the session detail UI.
- Do not change existing Codex message, tool-call, reasoning, or token parsing
  beyond additive subagent support.
- Do not attempt fuzzy prompt-based matching between parent and child sessions.

## Confirmed Evidence

From the current local Codex parser/docs review and real session
`019da0bb-541a-74e2-ae0a-6693c5e4fe04` under `~/.codex/sessions`:

- parent rollouts emit `collab_waiting_end` with `agent_statuses[]` and
  `statuses{thread_id -> status}`
- parent rollouts emit `collab_close_end` with `receiver_thread_id`, nickname,
  role, and final status payload
- parent rollouts emit `collab_agent_interaction_end` with `receiver_thread_id`,
  prompt, nickname/role, and status payload
- child thread IDs are stable Codex rollout session IDs such as
  `019da0bd-3df2-7191-a1a8-e326b55fe052`

From current upstream Codex source:

- the spawn handler emits `collab_agent_spawn_begin` and
  `collab_agent_spawn_end`
- `collab_agent_spawn_end` includes `new_thread_id`, prompt, nickname, role,
  effective model, reasoning effort, and final spawn status
- wait handlers emit `collab_waiting_end` with both `agent_statuses` and a
  `statuses` map
- session metadata supports structured `source.sub_agent.thread_spawn` /
  `source.subagent.thread_spawn` provenance carrying `parent_thread_id`

Local repo evidence:

- `src/models/session.rs` already supports `parent_session_id` and
  `is_subagent`
- `src/models/subagent.rs` and the `subagents` table already support
  `agent_id`, `prompt`, `result_summary`, `child_session_id`, and `parser_ref`
- `src/database/indexer.rs` already contains a post-parse linkage pass for
  Claude subagents, so Codex can follow the same architectural pattern

## Design

### Data model

Reuse the existing local model as-is.

Codex child rollouts should become normal `Session` rows with:

- `tool = Codex`
- `is_subagent = true`
- `parent_session_id = <parent thread id>`
- `id = <Codex child thread/session id>`

Parent Codex rollouts should emit `Subagent` rows that store:

- `id = <stable local subagent row id>`
- `agent_id = <Codex child thread id>` when known
- `title = <nickname or fallback title>`
- `prompt = <delegated prompt>`
- `result_summary = <best terminal result text>`
- `child_session_id = <linked child rollout session id once indexed>`
- `parser_ref = <spawn call_id>`

No new persistence concepts are needed. Codex should reuse the same
`Subagent.child_session_id` linkage field already used by OpenCode and Claude
Code.

### Canonical linkage sources

Use different sources for different responsibilities.

Child session truth:

- `session_meta.payload.source.subagent.thread_spawn.parent_thread_id`
- treat the equivalent upstream spelling `source.sub_agent.thread_spawn` as the
  same structure during parsing

Parent subagent activity truth:

- `collab_agent_spawn_end` creates or upserts the parent-side `Subagent`
- `collab_waiting_end`, `collab_close_end`, and
  `collab_agent_interaction_end` enrich an existing `Subagent`

This splits the responsibility cleanly:

- the child file decides whether it is a subagent session
- the parent file decides which delegations appear inline in the parent
  transcript

### Stable subagent row identity

The stable local `Subagent.id` for Codex should be the spawn `call_id`.

Rationale:

- it is stable within the parent rollout
- it is already present on spawn and related lifecycle events
- it fits the existing parser/indexer storage model without deriving an opaque
  synthetic key

If a later enrichment event lacks the spawn `call_id`, matching should fall back
to `agent_id == receiver_thread_id`, but the persisted row identity should still
be the original spawn `call_id` whenever available.

### Parser behavior

#### Child rollout parse

During Codex `session_meta` handling:

- inspect structured `source`
- if it represents a subagent thread spawn with `parent_thread_id`, mark the
  parsed session as a subagent
- set `session.parent_session_id` to that parent thread id
- keep the session's own upstream `id` as the local session id

All existing Codex parsing for messages, tool calls, transcript items,
reasoning, and tokens remains unchanged.

#### Parent rollout parse

When parsing `event_msg.payload.type` values:

- `collab_agent_spawn_begin`:
  - ignored on purpose; it is the strict prefix of `collab_agent_spawn_end`
    and carries no fields that are not repeated in the `_end` event
  - a parent transcript truncated between `begin` and `end` therefore leaves
    no subagent row, which matches the "conservative, identifier-first"
    policy below
- `collab_agent_spawn_end`:
  - create or upsert a `Subagent`
  - set `id = call_id`
  - set `agent_id = new_thread_id` when present
  - set `title` from `new_agent_nickname` when present, otherwise fallback to
    `Codex subagent`
  - set `prompt` from the event prompt
  - set `parser_ref = call_id`
  - emit a `TranscriptItemKind::Subagent` at the event position
  - rationale: `spawn_end` is chosen over `spawn_begin` because the
    transcript item only becomes meaningful once the spawn has resolved
    (`new_thread_id`, final status, effective model are all `_end`-only).
    Emitting at `_begin` would surface half-formed rows that must be
    rewritten on `_end`, complicating replace-on-reindex semantics without
    visible benefit to the user.
- `collab_waiting_end`:
  - use `agent_statuses[]` first because it carries nickname/role plus status
  - use `statuses{}` as fallback when `agent_statuses[]` is absent or partial
  - update matching subagents with result/status content
- `collab_close_end`:
  - treat as a stronger terminal result than `collab_waiting_end`
  - update title/role metadata if newly available
  - update `result_summary` from the final status payload
- `collab_agent_interaction_end`:
  - treat as lower-priority enrichment
  - use it mainly when spawn/wait/close data is incomplete

The parser should not create synthetic child sessions from parent-only events.

### Result summary precedence

#### `AgentStatus` shape

Every status-bearing `collab_*` event carries an `AgentStatus` payload. The
upstream Rust enum (with `serde(rename_all = "snake_case")`) serializes as:

- `"pending_init"` — unit string
- `"running"` — unit string
- `"interrupted"` — unit string
- `{ "completed": "<final assistant message>" }` or `{ "completed": null }`
- `{ "errored": "<error message>" }`
- `"shutdown"` — unit string
- `"not_found"` — unit string

Terminal variants: `completed`, `errored`, `shutdown`, `not_found`.
Non-terminal variants: `pending_init`, `running`, `interrupted`.

The parser must tolerate both the bare-string unit form and the
externally-tagged object form when deserializing.

#### Extraction rule

When writing `result_summary` for an enrichment event:

- `completed { text }` with a non-empty string → set summary to `text`
- `completed { null }` → set an empty-but-terminal marker (do not keep a stale
  non-terminal summary, but also do not overwrite a previously captured
  completed text from another event on the same subagent)
- `errored { message }` → set summary to `Error: <message>`
- `shutdown` / `not_found` → set a short terminal marker (e.g. `Shutdown`,
  `Not found`) when no better summary exists; do not overwrite existing text
- `pending_init` / `running` / `interrupted` → never overwrite an existing
  summary; only record if no summary is set yet and the field is useful for
  debugging

#### Event priority

When multiple result-bearing events exist for the same delegated child, keep the
latest highest-confidence terminal-looking result.

Priority order:

1. `collab_close_end`
2. `collab_waiting_end` (for the matching `thread_id`)
3. `collab_agent_interaction_end`

Within the same event family, later events win only if their status is terminal
or the existing summary is absent. A later non-terminal event must never
downgrade a previously captured terminal summary.

### Matching rules

Matching should stay strict and identifier-based.

Primary matching key:

- spawn `call_id` (uniquely identifies a delegation within a parent rollout)

Secondary matching key, used only when `call_id` is absent from an
enrichment event:

- `agent_id == receiver_thread_id`

Do not match by:

- prompt text similarity
- transcript ordering alone
- nickname alone

This avoids accidental merges when a parent spawns multiple agents with similar
roles or prompts.

#### Ordering invariant

Within a single parent rollout file, Codex writes events in append-only order
and `collab_agent_spawn_end` always precedes any enrichment event that
references the same `call_id` or `receiver_thread_id`. The parser relies on
this invariant and therefore:

- processes `event_msg` entries in file order
- treats an enrichment event that arrives before a matching spawn row as an
  orphan and drops it (see Error Handling below)

If a future Codex version breaks this invariant, the linker pass can be
extended to defer enrichments until spawn rows are known, but the current
design deliberately does not preempt that complexity.

#### Re-spawn of the same `thread_id`

A parent may in principle issue two `collab_agent_spawn_end` events that
share the same `new_thread_id` but different `call_id` values (for example
after a crash and retry). Because the row identity is the spawn `call_id`,
this produces two distinct `Subagent` rows and is the correct behavior.

Consequently:

- enrichment events are matched by `call_id` first and must not be shared
  across the two rows
- the secondary fallback (`agent_id == receiver_thread_id`) is ambiguous
  when more than one row shares the same `agent_id`; in that case the most
  recently spawned row wins (higher file offset) and a debug-level warning
  is emitted so downstream auditing can spot the edge case
- the indexer linkage pass must still only set `child_session_id` on rows
  whose `agent_id` matches the child session id; when multiple rows match,
  all of them receive the same `child_session_id` because they all describe
  the same live thread

### Indexer linking pass

Add a Codex post-parse linkage pass alongside the existing Claude linkage step.

If the parsed session is a Codex child session:

- read `session.parent_session_id`
- update parent `subagents.child_session_id` where:
  - `session_id == <parent session id>`
  - `agent_id == <child session id>`

This allows either indexing order:

- parent indexed first, child later
- child indexed first, parent later

If the parent row does not exist yet, the child session still indexes correctly
and the link can be filled on a later indexing pass.

#### Re-index of an existing parent

When an already-indexed Codex parent rollout is re-parsed (for example because
the file grew since the last scan), the existing indexer flow calls
`replace_session_contents_tx` before inserting the new parse output. That
helper already deletes rows in `subagents` keyed by `session_id`, which means
re-indexing a parent will:

- drop all previous parent-side `Subagent` rows for that session
- re-insert the rows produced by the new parse
- re-run the Codex linkage pass to repopulate `child_session_id`

Because `Subagent.id` is the spawn `call_id`, the same delegation across two
successive parses keeps the same primary key, so the replace-then-insert
cycle does not fragment external references. This must be covered by a test
(see Testing Plan).

## Error Handling

### Child rollout with structured parent source but no parent file

- index the child rollout as `is_subagent = true`
- keep `parent_session_id`
- do not fabricate a parent `Subagent` row

### Parent spawn without `new_thread_id`

- still create a visible parent-side `Subagent` row when prompt/title evidence is
  present
- leave `agent_id = None`
- leave `child_session_id = None`

### Enrichment event without prior spawn row

- do not create a new `Subagent` by default from `collab_waiting_end` or
  `collab_close_end` alone
- allow enrichment-only updates when a prior spawned subagent can be matched

This keeps the model conservative and avoids duplicate rows from noisy or partial
logs.

### Missing or ambiguous child identifier

- never guess using prompt text
- keep whatever parent-side subagent information is available
- leave linkage fields empty when identifier evidence is insufficient

## UI Behavior

No new UI behavior is required.

- parent Codex sessions should display delegated subagents in the existing
  subagents panel / transcript path
- linked Codex subagents should reuse the existing child-session navigation
  already used for OpenCode and Claude Code
- if `child_session_id` is absent, the UI should continue to hide or disable the
  open-session affordance as it already does

## Testing Plan

### Fixtures

Add focused Codex fixtures covering:

- a parent rollout with `collab_agent_spawn_end`
- a child rollout whose `session_meta.source` points back to that parent,
  with both `sub_agent` and `subagent` spellings represented across fixtures
- parent enrichment events via `collab_waiting_end` and `collab_close_end`,
  including at least one `AgentStatus::Errored { message }` case
- a partial parent-only case (spawn without child rollout)
- a partial child-only case (child rollout without parent file)
- a parent rollout truncated between `collab_agent_spawn_begin` and
  `collab_agent_spawn_end` (expected: no row created)
- a parent rollout that re-spawns the same `thread_id` under two different
  `call_id` values

### Automated tests

Parser tests should verify:

- Codex child rollouts parse as `is_subagent = true` when structured parent
  provenance is present
- both `source.sub_agent.thread_spawn` and `source.subagent.thread_spawn`
  spellings are accepted
- `parent_session_id` is extracted from `session_meta.source`
- `collab_agent_spawn_begin` alone does not create a `Subagent` row
- parent `collab_agent_spawn_end` creates a `Subagent` and transcript item
- `collab_waiting_end` and `collab_close_end` enrich the matching subagent
- result precedence prefers close over waiting over interaction
- a non-terminal status (`running`, `pending_init`, `interrupted`) never
  overwrites a previously captured terminal summary
- `AgentStatus` is parsed from both its bare-string unit form and its
  externally-tagged object form (`"running"` and `{ "completed": "text" }`)
- `errored { message }` produces a summary starting with `Error:`
- existing Codex message/tool/reasoning/token extraction remains unchanged

Indexer tests should verify:

- a parent `Subagent` row links to the child rollout via `child_session_id`
- linking works when the parent is indexed before the child
- linking works when the child is indexed before the parent
- parent-only indexing leaves a usable unlinked subagent row
- child-only indexing leaves a usable subagent session row
- re-indexing the same parent rollout does not duplicate `Subagent` rows and
  preserves the `call_id`-based primary keys through
  `replace_session_contents_tx`
- a parent that re-spawns the same `thread_id` under a different `call_id`
  produces two distinct `Subagent` rows, and both receive the same
  `child_session_id` once the child is indexed

### Manual verification

Run Sessions Chronicle against fixture data and confirm:

- parent Codex sessions show delegated subagents inline
- linked Codex subagents open the indexed child rollout
- child Codex sessions are excluded from top-level session listings the same way
  other subagent sessions already are

## Docs Impact

Update the Codex documentation to match the new parser behavior.

- `docs/session-formats/codex.md`
- `docs/SESSION_FORMAT_ANALYSIS.md`

Specifically:

- remove the statement that `collab_*` events are not yet mapped to subagent
  records
- document the child-session detection rule from structured `session_meta.source`
- document the parent enrichment rule from `collab_*` lifecycle events

## Risks

- Upstream Codex may emit both `sub_agent` and `subagent` source spellings
  depending on version or serializer boundary, so the parser must tolerate both.
- Some collaboration events may be truncated or absent in partial rollouts; the
  parser must treat them as optional enrichment rather than required structure.
- If multiple delegated agents share sparse metadata, loose matching could merge
  rows incorrectly, so matching must remain identifier-first.
- `AgentStatus` is a tagged enum upstream and its set of variants has grown
  across Codex releases (`pending_init`, `running`, `interrupted`,
  `completed(Option<String>)`, `errored(String)`, `shutdown`, `not_found`).
  The parser must use a permissive deserializer that tolerates unknown
  variants and never fails the whole session parse on an unrecognized status.
- Re-indexing relies on `replace_session_contents_tx` deleting parent-side
  subagent rows before reinsertion; any future change to that helper that
  narrows its scope could silently produce duplicate rows, so the re-index
  test is load-bearing.

## Smallest Justified Implementation Scope

1. Detect Codex child sessions from structured `session_meta.source` and set
   `is_subagent` / `parent_session_id`.
2. Parse `collab_agent_spawn_end` into parent `Subagent` rows and transcript
   items.
3. Enrich those rows from `collab_waiting_end`, `collab_close_end`, and
   `collab_agent_interaction_end` with strict matching and precedence rules.
4. Add an indexer linkage pass that fills `subagents.child_session_id` from the
   indexed child session.
5. Add fixtures, tests, and doc updates proving both indexing orders and partial
   data behavior.

This is enough to deliver end-to-end Codex subagent support without broad parser
refactoring or UI redesign.
