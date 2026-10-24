# Model storage for assistant transcript entries

**Status:** Approved design; implementation not yet evidenced in the current codebase.  
**Revision:** 2026-10-25 — rebased on the parser changes merged since the original
draft: OpenCode V2 (#210), Mistral Vibe Unified (#211), and the Kimi Code
subagent/swarm refresh (#213). Cache schema base is now v22.  

## Intent and scope

Preserve the model that produced each indexed assistant entry, including entries
containing only tool calls. The eventual product goal is to display this information
in the transcript. This change covers parsing, persistence, and retrieval only;
display behavior will be designed separately.

The user approved the storage-only design. This document records that design and
clarifies one detail discovered while inspecting the implementation: delegation
calls can be represented as `Subagent` records instead of `ToolCall` records and
must retain their originating model too.

Success means that loading an indexed entry returns the model attributable to its
generation context, when available, without borrowing a model from a later turn,
a tool result, or a child session.

## Current behavior

The relevant code lives in `crates/core/`:

- `models/message.rs`: `Message.model` already stores an optional model.
- `models/reasoning.rs`: `ReasoningAttachment.source_model` already stores the
  model associated with reasoning.
- `models/tool_call.rs`: `ToolCall` has no model field.
- `models/subagent.rs`: `Subagent` has no originating model field.
- `models/context.rs`: `ContextEntry` (system, skill, compaction summary) was
  added by #210. It is not an assistant entry and carries no model.
- `models/transcript_item.rs`: ordered entries reference messages, tool calls,
  subagents, or context entries; they do not own the associated content.
- `parsers/model.rs`: `normalize_model` trims identifiers, rejects malformed or
  empty values and the `<synthetic>` sentinel, and otherwise preserves identifiers.
- `database/mod.rs`: `insert_tool_call` and `insert_subagent` are the only
  insertion paths for both records (all AI assistants go through
  `database/indexer.rs`). `load_tool_call` and `load_subagent` are the
  standalone loaders. Child-session linking in `indexer.rs` and
  `indexer/vibe_unified.rs` later issues `UPDATE subagents` statements that
  touch `child_session_id` only.
- `database/schema.rs`: the current schema version is 22. Since the original
  draft, v19–v20 added OpenCode context storage, source generations, and
  inventories; v21 added `vibe_unified_signatures`; v22 cleared Kimi
  fingerprints.

Freshness is no longer tracked in a single place:

| Sources | Freshness record | Existing invalidation lever |
| --- | --- | --- |
| Claude Code, Codex, legacy Mistral Vibe, Kimi Code, OpenCode JSON | `file_fingerprints` | Delete rows in a migration (v22 precedent, scoped or global) |
| OpenCode SQLite (V1 and V2) | `opencode_source_fingerprints` + `opencode_source_sessions` | `OPENCODE_READER_GENERATION` in `database/indexer/opencode.rs` (currently 6) |
| Mistral Vibe Unified | `vibe_unified_signatures` | `UNIFIED_READER_VERSION` in `parsers/mistral_vibe_unified/signature.rs` (currently 2) |

Claude Code `Task`/`Agent` calls, Codex `spawn_agent` calls, OpenCode
`task`/`subagent` parts, Mistral Vibe Unified subagent effects, and matched Kimi
`Agent`/`AgentSwarm` calls produce only subagent entries. Adding a field solely
to `ToolCall` would leave these assistant entries uncovered.

## Alternatives and decision

### Extend the existing content records — selected

Keep `Message.model` and `ReasoningAttachment.source_model`. Add `ToolCall.model`
and `Subagent.source_model` as optional strings, with corresponding nullable
database columns.

This follows the existing storage boundaries, keeps standalone record loading
useful, and avoids moving existing message and reasoning data. The trade-off is
that a future display layer must select the appropriate field for each entry kind.

### Centralize the model on `TranscriptItem`

Store a common model on every ordered transcript entry. This would simplify a
future uniform transcript query, but would duplicate or relocate existing message
metadata and couple standalone tool-call retrieval to transcript records. It
would also force a meaningless `NULL` on every context item.

The selected approach is the smaller additive change and supports future display
without redesigning transcript storage now.

## Attribution contract

1. The stored value identifies the model that generated the assistant message or
   requested the action. It does not describe the program executing a tool call.
2. Capture attribution from the entry's message, turn, or step context when the
   entry is created. Tool-only responses are covered even when no text message is
   emitted.
3. All calls emitted by the same assistant response use the same attribution.
   Later turns or steps may use different models.
4. Missing or invalid attribution remains `None` in Rust and `NULL` in SQLite.
   Do not infer it from adjacent assistant messages or the final session model,
   except where a parser's existing message attribution already uses a
   documented session-level fallback (legacy Mistral Vibe, Mistral Vibe Unified
   without model-change checkpoints, Kimi `config.update` hint). Tool calls and
   subagents then use exactly the fallback the parser applies to assistant
   messages at the same position, and nothing broader.
5. Preserve the existing normalization rules; do not introduce display names,
   provider guessing, or model aliases beyond existing parser behavior.
6. Results, status changes, streaming updates, and child-session linking
   preserve the originating model. A duplicate representation of an event must
   not replace it with the model current at the time the duplicate is processed.
7. If a subagent is discovered only through a completion or lifecycle event with
   no attributable originating generation context, leave `source_model` unknown.
8. User messages, tool results, and context entries do not gain independent
   model attribution. Existing synthetic-event filtering remains in force.
9. A conversion that turns one record into another (a Kimi tool call matched to
   a subagent, a swarm call expanded into several subagents) carries the
   attribution over unchanged; it never re-resolves it at conversion time.

### Delegations and child sessions

`Subagent.source_model` means the model of the parent assistant that requested
the delegation. A requested child model in call arguments (for example Claude
Code `input.model`, Kimi `args.subagent_type`/profile) is not this value.
Entries within an indexed child session use that child's own source context.
Where a delegation retains both a tool-call record and a subagent record, both
records carry the same originating model.

Delegation shapes introduced since the original draft:

- **Resumed subagents** (Kimi explicit evidence, OpenCode V2 resumed `task`
  calls): several subagent rows share one `child_session_id`. Each row keeps the
  model of its own requesting step; rows are never harmonized with each other.
- **Kimi `AgentSwarm` expansion:** the N subagent rows produced from one swarm
  call all inherit that call's attribution. Calls that stay generic keep it on
  the tool call.
- **OpenCode V2 completions:** a synthetic `metadata.source == "subagent"`
  completion that enriches a `task` row must not change its `source_model`. A
  standalone row created from a completion with no matching task row has no
  originating generation context and keeps `source_model = NULL` (rule 7).
- **Mistral Vibe Unified subagent effects:** attributed with the model current
  at the effect's position in history, exactly like a tool effect.

This change preserves existing transcript shapes. It does not create additional
tool-call entries for lifecycle events or delegation calls currently represented
only as subagents.

## Source-specific rules

| AI assistant | Attribution source | Required behavior |
| --- | --- | --- |
| Claude Code | Normalized `message.model` on the containing assistant event | Pass the model to regular tool calls and `Task`/`Agent` delegation records, including responses without text. Teammate launches (`input.name`) follow the same rule; `input.model` is ignored. |
| OpenCode V1 SQLite and JSON | The parsed message model already available to part processing | Apply it to tool-call and subagent outcomes in both backends. |
| OpenCode V2 | The assistant row's normalized `model.id` (`parsers/opencode/v2_projection.rs`) | Apply it to every `tool` content item and every `task`/`subagent` row of that assistant message. Tool-state updates to an existing message keep the model. Standalone completion rows stay unknown (see above). No session-level model is applied retroactively. |
| Codex | The turn context already used for assistant messages | Capture it for calls and attributable delegation starts (`spawn_agent` response items and `collab_agent_spawn_*` events with a matching call); results, `collab_*` enrichment, and later lifecycle updates must not overwrite it. |
| Mistral Vibe (legacy) | The existing normalized `config.active_model` session metadata | Use the same fallback as assistant messages and reasoning, scoped to the session being parsed. |
| Mistral Vibe Unified | The `current_model` already tracked in `parsers/mistral_vibe_unified/convert.rs` | Pass it to `push_effect` in `effects.rs` for both tool and subagent effects. When history has `model_change` checkpoints, effects before the first checkpoint stay unknown; otherwise the session model fallback applies, as for messages. Discarded or provisional entries remain omitted. |
| Kimi Code | The originating step's resolved model (`llm.request` `modelAlias`/`model`, then the pending/unscoped queues, then the `config.update` hint) | Resolve calls against their step using the rules already used for assistant content. Preserve that attribution across later results, `Agent` matching (including resumes), and `AgentSwarm` expansion. Child journals resolve against their own steps. |

The session-level fallbacks (legacy Mistral Vibe, Unified without checkpoints,
Kimi `config.update`) are less precise than per-response metadata: they do not
prove which model generated every historical response if the model changed
without a recorded event. This change retains those limitations and does not add
new upstream format support to address them.

## Data model and database

- Add `model: Option<String>` to `ToolCall` and `model TEXT` to `tool_calls`.
- Add `source_model: Option<String>` to `Subagent` and `source_model TEXT` to
  `subagents`.
- Missing fields in previously serialized records must deserialize as unknown
  (`#[serde(default)]`).
- Update `insert_tool_call`, `insert_subagent`, `load_tool_call`, and
  `load_subagent` so attribution survives a full parse–store–load round trip.
  Child-linking `UPDATE subagents` statements must keep leaving `source_model`
  untouched.
- Preserve the existing fields on messages and reasoning attachments.
- Do not add a model to `ContextEntry` or `context_entries`.
- Keep the ordering and reference structure of `transcript_items` unchanged.
- Do not add model indexes or include model identifiers in full-text search.
- Do not change UI rendering, labels, grouping, filtering, or analytics.

The future display work may extend `load_transcript_items` /
`load_all_transcript_items` to expose these fields uniformly. Standalone
retrieval must already return them in this change.

## Migration and existing sessions

Add the next schema migration (v23 against the current v22 base). Version
numbers have collided between branches before: if another migration lands
first, allocate the next available version during implementation.

The migration adds both nullable columns, following the existing conventions
(`table_exists`/`column_exists` guards for already-added columns). Backfill then
requires invalidating every freshness mechanism, because all five AI assistants
gain attribution:

- Clear `file_fingerprints` in the migration. A global clear is appropriate here:
  every source tracked in that table is affected (unlike v22, which was Kimi-only).
- Bump `OPENCODE_READER_GENERATION` (6 → 7) so unchanged OpenCode SQLite sources
  are re-read once through the existing inventory/arbitration path. Do not
  delete `opencode_source_fingerprints` or `opencode_source_sessions` rows in the
  migration; the reader generation is the established lever and keeps retry and
  ineligibility state consistent.
- Bump `UNIFIED_READER_VERSION` (2 → 3) so stored Unified signatures stop being
  current. Do not delete `vibe_unified_signatures` rows.

Existing rows initially have `NULL` attribution and remain readable before
reindexing. Available sources populate the new fields through normal indexing.
Sources that cannot be reparsed must not receive fabricated models; existing
source-retention behavior (OpenCode retained owners and retry flags, Vibe
Unified read-failure preservation) remains unchanged.

Migration and reindexing must preserve user metadata such as pinned sessions.
The migration must not delete session contents merely to add these fields.
Reinitializing an already-migrated database must not repeatedly invalidate its
fingerprints, and a second indexing run after the generation bumps must skip
unchanged sources again.

## Error handling and resource use

Continue treating source session data as untrusted. Missing or malformed model
values must not prevent otherwise valid entries from being indexed. Reuse current
normalization and contextual attribution helpers wherever possible. For Mistral
Vibe Unified, a missing model never turns an otherwise supported store into an
`Unsupported`/`Corrupt` failure.

Keep JSONL parsing streamed. Attribution adds only small optional strings to
existing records; it does not require loading complete logs or looking ahead to
future events. The Kimi swarm expansion pass already runs after matching and
needs no extra lookahead.

## Validation

Use focused fixtures and real database round trips to cover:

- Tool-only assistant responses with a known model, including multiple calls in
  one response.
- Different models across consecutive turns or steps, with a late result for an
  earlier call preserving its original model.
- Missing, blank, malformed, and synthetic model values where applicable.
- All five AI assistants, including OpenCode V1 SQLite, JSON, and V2; legacy and
  Unified Mistral Vibe; and supported Codex call representations.
- OpenCode V2: tool and `task`/`subagent` rows inherit `model.id`; a streaming
  update keeps it; task + completion keeps the task's model; a standalone
  completion row stays unknown.
- Mistral Vibe Unified: effects before the first `model_change` are unknown,
  effects after each change take the new model, and a store without checkpoints
  uses the session model.
- Kimi Code: a matched `Agent` call keeps its step model; three resume calls in
  steps with different models keep three distinct values on one child; all
  expanded `AgentSwarm` rows share the call's model; a generic (unmatched) swarm
  keeps it on the tool call; child journals use their own steps.
- Delegation entries retaining the parent model, distinct from a requested child
  model and the model found in child-session messages.
- Lifecycle-only subagent discovery with unknown originating model.
- Child-session linking (including Vibe Unified reciprocal linking and Kimi
  child linking) leaving `source_model` unchanged.
- Insert, update/replacement, and load for known and unknown attribution.
- Upgrade from v22, existing-row readability, one-time invalidation across all
  three freshness mechanisms, fresh database creation, and preservation of
  pinned-session metadata.
- Reindexing an unchanged available source after migration populating
  attribution, for each mechanism (a file-fingerprinted source, an OpenCode
  SQLite database, and a Vibe Unified store).
- Existing transcript ordering, entry counts, context-entry positions, reasoning
  attachment association, Kimi swarm index renumbering, and tool-result
  correlation remaining correct.

Before completion, run formatting, Clippy, and the full test suite using the
repository's headless invocation:

```sh
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --all --no-fail-fast
```

No screenshots or Flatpak build verification are required for this storage-only
change unless implementation also changes UI or packaging scope.

## Acceptance criteria

- Every existing assistant entry kind can persist and retrieve its originating
  model when the source supplies attributable information.
- Tool-only responses and delegation entries are covered, including resumed
  subagents, Kimi swarm expansions, and OpenCode V2 and Vibe Unified delegations.
- Unknown attribution remains explicit, and results never move an earlier call
  to a later model.
- Existing sessions are eligible for automatic backfill through reindexing for
  every freshness mechanism.
- Context entries and UI behavior are unchanged; display remains separate future
  work.

## Decision

Extend the existing content records with nullable model attribution, reuse each
parser's current generation context, and backfill through the normal indexing
flow by invalidating `file_fingerprints` and bumping the OpenCode and Vibe
Unified reader generations. This preserves the information needed for later
display while keeping the current change focused on storage.
