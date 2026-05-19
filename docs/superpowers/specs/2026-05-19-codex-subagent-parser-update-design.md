# Codex Subagent Parser Update — Design

Date: 2026-05-19

## Problem

The Codex parser (`src/parsers/codex.rs`) has two confirmed gaps against the
current upstream Codex protocol (verified against `codex-rs/protocol/src/protocol.rs`,
rust-v0.131.0):

1. **`collab_resume_end` is ignored.** The parser handles `collab_agent_spawn_end`,
   `collab_waiting_end`, `collab_close_end`, and `collab_agent_interaction_end`,
   but `collab_resume_end` falls through the `_ => {}` arm of `handle_event_msg`.
   It carries a `receiver_thread_id` and an `AgentStatus` — the same status-bearing
   shape as `collab_close_end` — so a resumed subagent's terminal status is lost.

2. **Response-item `spawn_agent` / `wait_agent` are not mapped to subagents.**
   Codex `0.130.0+` rollouts can persist subagent work as `response_item`
   `function_call` / `function_call_output` pairs instead of `event_msg`
   `collab_*` events. The parser indexes these as generic tool calls, so the
   parent session shows no `Subagent` rows for that work.

Both gaps are enrichment opportunities, not breakage: nothing fails to index.

Fixtures and guard tests already exist (commit `95b404f`):

- `tests/fixtures/codex_subagent_linkage/2026/05/18/` — real, anonymized
  response-item parent + child pair.
- `tests/fixtures/codex_sessions/2026/05/19/rollout-2026-05-19T09-00-00-collab-resume.jsonl`
  — synthetic `collab_resume_end` fixture.
- `tests/codex_subagent_event_coverage.rs` — two tests currently asserting the
  *pre-fix* behavior.

## Scope

In scope:

- `collab_resume_end` enrichment of existing parent-side subagents.
- Response-item `spawn_agent` → parent-side `Subagent` rows.
- Response-item `wait_agent` → terminal enrichment of those rows.
- `spawn_agent` / `wait_agent` `function_call` pairs stop being indexed as
  generic tool calls (they become `Subagent` rows only, consistent with the
  `collab_*` form).

Out of scope:

- `close_agent` and `send_message` / `followup_task` response-item calls remain
  generic tool calls. They are not in the documented gap, not in the fixtures,
  and `wait_agent` already supplies the terminal `completed` summary. Can be
  extended later.
- Collab timing fields (`started_at_ms` / `completed_at_ms`), spawned-agent
  `model`, and `reasoning_effort` remain unstored.

## Change A — `collab_resume_end`

- Add `SubagentEventPriority::Resume = 4` (highest). Rationale: `collab_resume_end`
  carries the agent's status *after* resume — chronologically the freshest event,
  so its terminal status should win over `Close`. Edge case `spawn → resume → close`
  is unusual; the priority model is already a lifecycle heuristic and this keeps
  the common `close → resume` case correct.
- New match arm in `handle_event_msg`:
  `Some("collab_resume_end")` →
  `update_subagent_from_status(call_id, receiver_thread_id, receiver_agent_nickname, None, status, SubagentEventPriority::Resume)`.
  Identical shape to the existing `collab_close_end` arm.
- No row creation. Resume only enriches an existing subagent resolved by
  `receiver_thread_id` → `agent_id`. With no matching spawn, the existing
  orphan-drop path in `update_subagent_from_status` applies.

## Change B — response-item `spawn_agent` / `wait_agent`

A response-item subagent spawns over two events: the `spawn_agent` `function_call`
(has `call_id` + `arguments`, no `agent_id`) and the `function_call_output`
(has `agent_id` + `nickname`). The `Subagent` row is created at the output,
when all data is available.

### Parser state additions

- `pending_spawns: HashMap<String, PendingSpawn>` keyed by spawn `call_id`.
- `pending_waits: HashSet<String>` of wait `call_id`s.
- `struct PendingSpawn { agent_type: Option<String>, message: Option<String> }`.

### Refactor

Extract the row-creation core of `record_subagent_spawn` into a typed helper:

```
fn push_subagent_row(&mut self, id: String, agent_id: Option<String>,
                     title: String, prompt: Option<String>)
```

It pushes the `Subagent`, registers `subagent_idx_by_call_id`,
`subagent_priority_by_id`, and `subagent_indexes_by_agent_id`, emits the
`Subagent` `TranscriptItem`, and calls `flush_pending_reasoning_to_item`.
The existing `record_subagent_spawn(&Value)` becomes a thin extractor over the
collab payload that calls `push_subagent_row`.

### `handle_response_item` — `function_call` arm

Before `push_tool_call`, match on `name`:

- `spawn_agent`: parse `arguments` (a JSON string) → store
  `PendingSpawn { agent_type, message }` keyed by `call_id`. No tool call.
- `wait_agent`: insert `call_id` into `pending_waits`. No tool call.
- otherwise: existing `push_tool_call`.

A `spawn_agent` / `wait_agent` `function_call` with a missing/empty `call_id`
is skipped with a warning (current behavior), no pending entry.

### `handle_response_item` — `function_call_output` arm

Before `complete_tool_call`, test the `call_id`:

- in `pending_spawns`: parse `output` (a JSON string) with `serde_json::from_str`
  → `agent_id`, `nickname`. Call
  `push_subagent_row(id = call_id, agent_id, title = nickname ?? agent_type ?? "Codex subagent", prompt = message)`.
  Consume the pending entry.
- in `pending_waits`: parse `output` → `status` object map
  `{ agent_id → AgentStatus }`. For each entry,
  `update_subagent_from_status(None, Some(agent_id), None, None, &status, SubagentEventPriority::Waiting)`.
  Consume the pending entry.
- otherwise: existing `complete_tool_call`.

### Robustness

- `output` is a JSON string containing JSON — `serde_json::from_str` required.
- Spawn output that fails to parse or has no `agent_id`: no row created, pending
  entry consumed, `tracing::debug!` logged. The session still indexes.
- `wait_agent` output with an empty `status` map: no enrichment (matches the
  empty `collab_waiting_end` behavior).
- `wait_agent` referencing an `agent_id` with no matching spawn: orphan-drop via
  the existing `update_subagent_from_status` path.
- An orphan `function_call_output` with no matching pending entry and no tool
  call: falls through to `complete_tool_call`, which no-ops.

### Linkage

`push_subagent_row` sets `agent_id` from `output.agent_id`, which equals the
child rollout's `session_meta.payload.id`. Parent↔child linkage is then resolved
at index time exactly as for the `collab_*` form (`agent_id` → child session).

## Testing

- Rework `tests/codex_subagent_event_coverage.rs` to assert the post-fix behavior:
  - Response-item test: parent has exactly 1 `Subagent` row titled `Nord`,
    `agent_id` and `child_session_id` equal the child session, `result_summary`
    equals the `wait_agent` `completed` text; `spawn_agent` / `wait_agent` are
    no longer present as generic tool calls.
  - Resume test: the spawn subagent's `result_summary` equals the
    `collab_resume_end` `completed` text (`"Parser changes look correct."`).
- `tests/codex_subagent_linkage.rs` (collab form) must pass unchanged.
- `cargo fmt --all -- --check`, `cargo clippy --all -- -D warnings`,
  `cargo test --all --no-fail-fast`.

## Docs

Flip the "does not yet" statements now made false:

- `docs/session-formats/codex.md`: the parser implementation bullets for
  `collab_resume_end` enrichment and response-item `spawn_agent` / `wait_agent`
  mapping.
- `docs/SESSION_FORMAT_ANALYSIS.md`: the Codex subagents notes and the related
  open questions.
