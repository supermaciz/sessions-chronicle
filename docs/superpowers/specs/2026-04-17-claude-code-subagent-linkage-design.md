# Claude Code Subagent Linkage Design

**Status:** Implemented [#121](https://github.com/supermaciz/sessions-chronicle/pull/121)

## Problem

Sessions Chronicle currently detects Claude Code subagent launches in the parent
 transcript, but it does not index the corresponding subagent transcript files
 under `subagents/agent-*.jsonl`. Those files are currently treated as sidechain
 data and pruned during indexing.

As a result:

- Claude Code subagents appear in the parent transcript as `Subagent` rows
- those rows always have `child_session_id = None`
- the existing inspector navigation used for OpenCode cannot open the Claude
  subagent transcript

Fresh real-session evidence shows that Claude Code stores subagent transcripts as
 separate nested JSONL files linked by parent session ID and `agentId`, not by a
 separate upstream child session ID.

## Goal

Index Claude Code subagent transcripts as first-class local sessions and link
 them to parent `Subagent` rows so the existing inspector navigation can reuse
 the same button behavior as OpenCode.

## Non-Goals

- Do not redesign the tool inspector UI beyond reusing the existing button path.
- Do not add a Claude-specific alternate button or special-case UI copy.
- Do not attempt fuzzy matching based on prompt text or transcript ordering.
- Do not change OpenCode linkage semantics.

## Confirmed Evidence

From a fresh real Claude Code session in `/home/mcizo/.claude/projects`:

- the parent transcript is stored as `<session-id>.jsonl`
- subagent transcripts are stored under
  `<session-id>/subagents/agent-<agentId>.jsonl`
- the subagent transcript carries the same parent `sessionId`
- the subagent transcript includes `isSidechain: true` and `agentId`
- the parent transcript contains an `Agent` tool use
- the parent transcript's async launch result includes the launched `agentId`

This means Claude Code is linkable, but not the same way as OpenCode. The local
 model must derive a child session identity from the indexed subagent transcript
 rather than reading an explicit child session ID from the parent transcript.

## Design

### Data model

Reuse the existing `Subagent.child_session_id` field as the UI-facing linkage
 field across assistants.

- For OpenCode, keep existing behavior: store the explicit child session ID.
- For Claude Code, store the local indexed subagent session ID in the same field.

Each Claude subagent transcript should be indexed as a real `Session` row with:

- `assistant = ClaudeCode`
- `is_subagent = true`
- `parent_session_id = <parent Claude session id>`
- `file_path = <.../subagents/agent-*.jsonl>`
- `id = <deterministic derived local subagent session id>`

The parent `Subagent` row should link to that indexed child session by setting:

- `child_session_id = <derived local subagent session id>`

This keeps a single navigation model in the UI while allowing Claude Code and
 OpenCode to differ internally.

### Child session identity

Claude subagent session IDs should be local, deterministic, and opaque.

Conceptually:

`claude-subagent::<parent_session_id>::<agent_id>`

Requirements:

- deterministic across reindex runs
- unique within the database
- derived from stable evidence only
- not dependent on transcript ordering

The exact string format is an implementation detail, but it must be stable and
 easily testable.

### Indexing flow

Claude Code indexing should treat nested subagent transcript files as a second
 discovered input type instead of pruning them.

#### Session discovery

- continue discovering normal Claude parent transcripts as today
- additionally discover nested Claude subagent transcripts under
  `<session-id>/subagents/agent-*.jsonl`

#### Parent transcript parse

- continue parsing `Agent` and legacy `Task` tool calls into `Subagent` rows
- capture the launched `agentId` from the async launch `tool_result` when
  available

#### Subagent transcript parse

- parse the sidechain transcript as a normal Claude session transcript
- mark the parsed session as `is_subagent = true`
- set `parent_session_id` from the enclosing parent session directory and shared
  parent session ID
- derive the local child session ID from parent session ID plus `agentId`

#### Linking pass

- match parent `Subagent` rows to indexed Claude subagent sessions by:
  - parent session ID
  - `agentId`
- set `Subagent.child_session_id` to the derived local child session ID

#### Reindex behavior

Because the child session ID is deterministic, repeated indexing should update
 the same child session row rather than creating duplicates.

### Linkage rules

Primary linkage key:

- parent session ID
- `agentId`

Supporting evidence only:

- nested file path
- `promptId`
- transcript ordering

The path structure helps discover and validate subagent transcripts, but it
 should not replace `agentId` as the primary match key.

### Error handling

#### Parent launch without child transcript

If the parent transcript contains a Claude subagent launch but no matching
 subagent transcript file exists yet:

- still store the parent `Subagent`
- leave `child_session_id = None`
- keep the existing inspector button hidden until a later reindex finds the
  child transcript

#### Child transcript without matching parent launch row

If a Claude subagent transcript exists but no matching parent launch record is
 found:

- still index it as a session
- set `is_subagent = true`
- keep `parent_session_id` when derivable
- do not fabricate a parent `Subagent` row

#### Missing or unusable `agentId`

If `agentId` cannot be extracted reliably:

- do not guess from prompt text
- keep the session indexed if possible
- leave it unlinked

#### Derived ID collisions

If multiple subagent transcripts collide on the same derived child session ID:

- treat that as a parser/indexing bug
- log it clearly
- do not silently merge unrelated transcripts

## UI Behavior

The existing subagent inspector button path should be reused.

- OpenCode continues to open an explicitly linked child session
- Claude Code opens the indexed local child session derived from the subagent
  transcript

No assistant-specific alternate button is needed. The same button can remain
 hidden whenever `child_session_id` is absent.

## Testing Plan

### Fixtures

Add a Claude fixture that includes:

- one parent transcript
- at least one real `subagents/agent-*.jsonl` transcript
- matching `agentId` evidence in the parent launch result and child transcript

### Automated tests

Tests should verify:

- the parent Claude session is indexed normally
- the child Claude subagent transcript is indexed as a separate local session
- the child session has `is_subagent = true`
- the child session has `parent_session_id = <parent>`
- the parent `Subagent.child_session_id` points to the indexed child session
- reindexing reuses the same child session row instead of duplicating it
- inspector navigation can reuse the existing OpenCode button path without
  assistant-specific branching

### Manual verification

Run the app against fixture data and confirm:

- Claude subagent rows appear in the parent transcript
- linked Claude subagents expose the same open-session button behavior as
  OpenCode
- opening the linked Claude subagent session shows the indexed child transcript

## Risks

- Claude parent and child linkage depends on reliable `agentId` extraction from
  both sides of the evidence chain.
- The current indexer intentionally prunes sidechains, so this change must be
  scoped carefully to Claude subagent transcripts rather than broadly relaxing
  pruning behavior.
- If existing schema or upsert logic assumes session IDs map directly to
  upstream IDs, derived local child IDs may require targeted adjustments.

## Smallest Justified Implementation Scope

1. Stop pruning Claude `subagents/agent-*.jsonl` transcripts.
2. Parse and index them as subagent sessions.
3. Capture `agentId` in parent and child parsing.
4. Link parent `Subagent` rows to derived child sessions via
   `child_session_id`.
5. Reuse the existing inspector button path.

This is sufficient to prove the design end-to-end without introducing new UI
 concepts.
