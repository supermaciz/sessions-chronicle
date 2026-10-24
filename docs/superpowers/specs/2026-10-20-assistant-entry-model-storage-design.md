# Model storage for assistant transcript entries

**Status:** Approved design; implementation not yet evidenced in the current codebase.  

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
- `models/transcript_item.rs`: ordered entries reference messages, tool calls, or
  subagents; they do not own the associated content.
- `parsers/model.rs`: `normalize_model` trims identifiers, rejects malformed or
  empty values and the `<synthetic>` sentinel, and otherwise preserves identifiers.
- `database/schema.rs`: the current schema version is 18. Previous parser changes
  invalidate file fingerprints to repopulate existing sessions.

Claude Code `Task`/`Agent` calls and Codex `spawn_agent` calls can produce only a
subagent entry. OpenCode also has subagent outcomes. Adding a field solely to
`ToolCall` would leave these assistant entries uncovered.

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
metadata and couple standalone tool-call retrieval to transcript records.

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
   Do not infer it from adjacent assistant messages or the final session model.
5. Preserve the existing normalization rules; do not introduce display names,
   provider guessing, or model aliases beyond existing parser behavior.
6. Results, status changes, and child-session linking preserve the originating
   model. A duplicate representation of an event must not replace it with the
   model current at the time the duplicate is processed.
7. If a subagent is discovered only through a completion or lifecycle event with
   no attributable originating generation context, leave `source_model` unknown.
8. User messages and tool results do not gain independent model attribution.
   Existing synthetic-event filtering remains in force.

### Delegations and child sessions

`Subagent.source_model` means the model of the parent assistant that requested
the delegation. A requested child model in call arguments is not this value.
Entries within an indexed child session use that child's own source context.
Where a delegation retains both a tool-call record and a subagent record, both
records carry the same originating model.

This change preserves existing transcript shapes. It does not create additional
tool-call entries for lifecycle events or delegation calls currently represented
only as subagents.

## Source-specific rules

| AI assistant | Attribution source | Required behavior |
| --- | --- | --- |
| Claude Code | Normalized `message.model` on the containing assistant event | Pass the model to regular tool calls and `Task`/`Agent` delegation records, including responses without text. |
| OpenCode | The parsed message model already available to part processing | Apply it to tool-call and subagent outcomes in both JSON and SQLite backends. |
| Codex | The turn context already used for assistant messages | Capture it for calls and attributable delegation starts; results or later lifecycle updates must not overwrite it. |
| Mistral Vibe | The existing normalized `config.active_model` session metadata | Use the same fallback as assistant messages and reasoning, scoped to the session being parsed. |
| Kimi Code | The originating step's resolved model | Resolve calls against their step, using the existing request/step attribution rules used for assistant content. Preserve that attribution across later results. |

Mistral Vibe's existing metadata fallback is less precise than per-response
metadata: it does not prove which model generated every historical response if
the model changed during the session. This change retains that limitation and
does not add new upstream format support to address it.

## Data model and database

- Add `model: Option<String>` to `ToolCall` and `model TEXT` to `tool_calls`.
- Add `source_model: Option<String>` to `Subagent` and `source_model TEXT` to
  `subagents`.
- Missing fields in previously serialized records must deserialize as unknown.
- Update database insertion, replacement, and standalone loading for both records
  so attribution survives a full parse–store–load round trip.
- Preserve the existing fields on messages and reasoning attachments.
- Keep the ordering and reference structure of `transcript_items` unchanged.
- Do not add model indexes or include model identifiers in full-text search.
- Do not change UI rendering, labels, grouping, filtering, or analytics.

The future display work may extend transcript preview queries to expose these
fields uniformly. Standalone retrieval must already return them in this change.

## Migration and existing sessions

Add the next schema migration (v19 against the current v18 base). If another
migration lands first, allocate the next available version during implementation.

The migration adds both nullable columns and clears the existing file-fingerprint
cache so the normal indexing flow reparses available sources. Follow the existing
migration conventions, including safe handling of already-added columns.

Existing rows initially have `NULL` attribution and remain readable before
reindexing. Available source files populate the new fields through normal
indexing. Sources that cannot be reparsed must not receive fabricated models;
existing source-retention behavior remains unchanged.

Migration and reindexing must preserve user metadata such as pinned sessions.
The migration must not delete session contents merely to add these fields.
Reinitializing an already-migrated database must not repeatedly invalidate its
fingerprints.

## Error handling and resource use

Continue treating source session data as untrusted. Missing or malformed model
values must not prevent otherwise valid entries from being indexed. Reuse current
normalization and contextual attribution helpers wherever possible.

Keep JSONL parsing streamed. Attribution adds only small optional strings to
existing records; it does not require loading complete logs or looking ahead to
future events.

## Validation

Use focused fixtures and real database round trips to cover:

- Tool-only assistant responses with a known model, including multiple calls in
  one response.
- Different models across consecutive turns or steps, with a late result for an
  earlier call preserving its original model.
- Missing, blank, malformed, and synthetic model values where applicable.
- All five AI assistants, including both OpenCode backends and supported Codex
  call representations.
- Delegation entries retaining the parent model, distinct from a requested child
  model and the model found in child-session messages.
- Lifecycle-only subagent discovery with unknown originating model.
- Insert, update/replacement, and load for known and unknown attribution.
- Upgrade from v18, existing-row readability, one-time fingerprint invalidation,
  fresh database creation, and preservation of pinned-session metadata.
- Reindexing an unchanged available source after migration populating attribution.
- Existing transcript ordering, entry counts, reasoning attachment association,
  and tool-result correlation remaining correct.

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
- Tool-only responses and delegation entries are covered.
- Unknown attribution remains explicit, and results never move an earlier call
  to a later model.
- Existing sessions are eligible for automatic backfill through reindexing.
- UI behavior is unchanged; display remains separate future work.

## Decision

Extend the existing content records with nullable model attribution, reuse each
parser's current generation context, and backfill through the normal indexing
flow. This preserves the information needed for later display while keeping the
current change focused on storage.
