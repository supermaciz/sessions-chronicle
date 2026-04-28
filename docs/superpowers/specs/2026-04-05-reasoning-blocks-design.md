# Reasoning/Thinking Blocks Display — Design

**Issue:** [#45](https://github.com/supermaciz/sessions-chronicle/issues/45)  
**Exploration:** [2026-04-05-reasoning-blocks-exploration.md](2026-04-05-reasoning-blocks-exploration.md)  
**Date:** 2026-04-05  
**Status:** Implemented [#117](https://github.com/supermaciz/sessions-chronicle/pull/117)  
**Decision:** Proposal B — Inspector Pill, revised around transcript attachments

---

## Overview

Display reasoning/thinking content from AI assistant sessions in Sessions Chronicle.
A small pill badge signals that reasoning exists; clicking the pill opens the existing
inspector pane with the reasoning content.

Reasoning is treated as **forensic evidence** to inspect on demand, not as part of the
main conversation flow.  
The transcript layout remains focused on messages, tool calls, and subagents.

This revision changes one important assumption from the exploration notes:
reasoning is **not** attached only to assistant messages. Real session formats often emit
reasoning as standalone records before the next visible transcript item, especially before
tool calls. The data model therefore attaches reasoning to the **next rendered transcript item**.

---

## 1. Reasoning Data Model

Each extracted reasoning payload is stored explicitly, without sentinel text values in the
payload fields:

```rust
pub struct ReasoningAttachment {
    pub session_id: String,
    pub transcript_item_index: i64,  // < 0 reserved for orphan attachments
    pub visible_text: Option<String>,
    pub summary_text: Option<String>,
    pub encrypted_content: Option<String>,
    pub source_model: Option<String>,
    pub source_timestamp: Option<DateTime<Utc>>,
}
```

### Field meanings

- `visible_text`: full inspectable reasoning text
- `summary_text`: short inspectable reasoning summary when the source exposes only a summary
- `encrypted_content`: opaque encrypted payload, never rendered directly in v1
- `source_model`: model slug associated with the reasoning-producing assistant turn
- `source_timestamp`: timestamp of the reasoning-producing assistant turn

### Normalization rules

- Trim whitespace-only strings to `NULL`
- Ignore empty reasoning blocks unless they also include `encrypted_content`
- Keep both `visible_text` and `summary_text` when both exist
- Do not use DB-internal sentinels such as `"[encrypted]"`

### UI state derivation

```text
has_reasoning =
  visible_text IS NOT NULL
  OR summary_text IS NOT NULL
  OR encrypted_content IS NOT NULL

has_visible_reasoning =
  visible_text IS NOT NULL
  OR summary_text IS NOT NULL

encrypted_only =
  encrypted_content IS NOT NULL
  AND visible_text IS NULL
  AND summary_text IS NULL
```

---

## 2. Attachment Model

Reasoning is attached to the **next visible transcript item** produced by the same assistant turn.

That target may be:

- a message row
- a tool call row
- a subagent row

It is **not** stored only against `messages`, because real formats frequently produce:

- reasoning-only assistant events followed by tool calls
- standalone reasoning records followed by a function call
- part-level reasoning before the next visible part

### Attachment algorithm

Each parser keeps a `PendingReasoning` accumulator while processing one assistant turn or one
assistant source message.

When the parser emits the next visible `TranscriptItem`, it:

1. writes the normal transcript item
2. flushes the accumulated reasoning onto that `item_index`
3. clears the accumulator

### Scope guardrails

- Reasoning is never carried across a user turn
- Reasoning is never attached to a previous transcript item
- If an assistant turn ends with reasoning but produces no visible transcript item, the
  reasoning is stored as an **orphan attachment** with a unique negative
  `transcript_item_index` for that session, allocated from a reserved orphan range such as
  `-1`, `-2`, `-3`, in encounter order. Allocation is deterministic: each parser/indexing
  pass starts an orphan counter at `-1` and decrements it for every orphan attachment
  encountered in that session. This preserves the data for diagnostics and future use
  without primary-key collisions, and a `warn!` log is emitted noting the orphaned
  reasoning. Orphan attachments are excluded from transcript row joins (the query already
  filters on `ti.item_index = ra.transcript_item_index`) so they are invisible in the UI
  but queryable for debugging.

Normal transcript items always use non-negative `item_index` values. Negative
`transcript_item_index` values are reserved for orphan reasoning only.

This keeps attachment deterministic and avoids ambiguous cross-turn pairing while
preventing silent data loss.

---

## 3. Parser Extraction

Each parser extracts a `PendingReasoning` payload:

```rust
struct PendingReasoning {
    visible_text: Option<String>,
    summary_text: Option<String>,
    encrypted_content: Option<String>,
    source_model: Option<String>,
    source_timestamp: Option<DateTime<Utc>>,
}
```

Multiple blocks for the same assistant turn are joined with `\n---\n`.

| Assistant | Source field(s) | Extraction | Attachment target |
|-----------|------------------|------------|-------------------|
| **Claude Code** | `content[].type == "thinking"` | Split thinking blocks from text blocks; ignore empty thinking strings | Next transcript item emitted from the same assistant event |
| **Mistral Vibe** | `reasoning_content` on assistant messages, if present | Read field conditionally when present | Assistant message item |
| **OpenCode** | `part.type == "reasoning"`, plus `metadata.openai.reasoningEncryptedContent` when present | Accumulate reasoning parts instead of skipping them; ignore empty text unless encrypted payload exists | Next transcript item emitted from the same source message |
| **Codex** | `response_item.type == "reasoning"` with `summary[]`, optional `content`, and optional `encrypted_content` | Capture summary text and encrypted payload explicitly | Next transcript item emitted after that reasoning item |

### Claude Code

Current behavior in `extract_text_from_array` mixes `text` and `thinking` into one message body.

New behavior:

- `text` blocks stay in visible assistant content
- `thinking` blocks populate `PendingReasoning.visible_text`
- empty `thinking` blocks are ignored
- if the event also emits tool calls and no visible text, reasoning attaches to the first tool-call transcript item produced from that event

### Mistral Vibe

`reasoning_content` is supported conditionally.

Important status note:

- the field was **not observed** in the current local sessions under `~/.vibe/logs/session`
- implementation should support it when present
- current Vibe-shaped sessions must remain a graceful no-op when it is absent

Testing strategy: since no real session with `reasoning_content` has been observed, the parser
unit test constructs a minimal JSON payload inline with the field present, without requiring a
dedicated fixture file. This keeps the test self-contained and avoids maintaining a synthetic
fixture that could drift from the real format.

### OpenCode

OpenCode is processed part-by-part, so reasoning extraction is not just a `Nothing -> Message`
switch. The parser must accumulate reasoning across parts and flush it to the next visible part-derived
transcript item.

Important details:

- `part.type == "reasoning"` may contain visible text in `text`
- some reasoning parts have empty visible text but do carry encrypted metadata
- `part.type == "reasoning"` must no longer be discarded blindly

### Codex

Codex reasoning items are standalone records and often precede tool calls rather than assistant
messages.

Important details:

- capture `summary[].type == "summary_text"` into `summary_text`
- capture `content` if it ever becomes non-null
- capture `encrypted_content` explicitly
- if summary is available, the pill is clickable even when raw reasoning content is encrypted
- if only encrypted payload exists, show a non-interactive encrypted pill

---

## 4. Schema Migration (v9)

Store reasoning outside the FTS5 `messages` table.

### Index stability

`transcript_item_index` is a positional key assigned by the parser during indexing. It is
deterministic for a given parser version and session file: the same input always produces the
same sequence of `item_index` values. However, a parser change could renumber items. To prevent
stale reasoning attachments from pointing to the wrong transcript item:

- **Re-index always co-deletes**: every code path that deletes `transcript_items` for a session
  must also delete the corresponding `reasoning_attachments` rows. The indexer already follows
  this pattern for `tool_calls`, `subagents`, and `messages` — `reasoning_attachments` is added
  to the same delete cascade. Because orphan attachments use a session-global negative index
  range, any file-level reindex that touches a session must delete and rebuild that session's
  orphan reasoning rows before reinserting them; per-file orphan cleanup is not sufficient.
- **Full re-index on migration**: the v9 migration clears `file_fingerprints`, which forces a
  complete re-parse and re-insert of both tables from scratch.

This means reasoning attachments are never orphaned by a parser evolution, because both tables
are rebuilt together. The same rule applies to orphan attachments: their reserved negative
indices are regenerated from scratch during the same parse, using deterministic encounter
order within the session.

```sql
CREATE TABLE IF NOT EXISTS reasoning_attachments (
    session_id TEXT NOT NULL,
    transcript_item_index INTEGER NOT NULL,
    visible_text TEXT,
    summary_text TEXT,
    encrypted_content TEXT,
    source_model TEXT,
    source_timestamp INTEGER,
    PRIMARY KEY (session_id, transcript_item_index)
);

CREATE INDEX IF NOT EXISTS idx_reasoning_attachments_session
    ON reasoning_attachments(session_id);

-- Force re-index so existing sessions get reasoning extracted and attached
DELETE FROM file_fingerprints;
```

Schema version bumped from 8 to 9.

### Why not alter `messages`?

`messages` is an FTS5 virtual table.  
The earlier `ALTER TABLE messages ADD COLUMN reasoning_content TEXT` approach is invalid for FTS5
and must not be used.

This revised design avoids mutating the FTS table entirely:

- transcript search still indexes only visible message content
- reasoning storage lives in a normal side table
- no FTS rebuild is needed for this feature

---

## 5. Query Model and Lazy Loading

Transcript rendering is driven by `transcript_items`, so reasoning preview flags must be joined
at that level rather than at `messages` level.

### Preview shape

```rust
pub struct ReasoningPreview {
    pub has_reasoning: bool,
    pub has_visible_reasoning: bool,
    pub encrypted_only: bool,
}
```

This preview is carried on transcript-row init data for:

- message rows
- tool call rows
- subagent rows

Grouped tool-burst rows do not own a separate reasoning record. Their header state is derived
from the already-loaded child tool-call rows being grouped:

- `visible_reasoning_child_count`: number of grouped children where `has_visible_reasoning == true`
- `encrypted_only_child_count`: number of grouped children where `encrypted_only == true`
- each grouped child keeps its own raw database `transcript_item_index`

No extra DB query is required to build burst-header reasoning indicators.

### Transcript query join

```sql
SELECT ...,
       (ra.visible_text IS NOT NULL
        OR ra.summary_text IS NOT NULL
        OR ra.encrypted_content IS NOT NULL) AS has_reasoning,
       (ra.visible_text IS NOT NULL
        OR ra.summary_text IS NOT NULL) AS has_visible_reasoning,
       (ra.encrypted_content IS NOT NULL
        AND ra.visible_text IS NULL
        AND ra.summary_text IS NULL) AS encrypted_only
FROM transcript_items ti
LEFT JOIN reasoning_attachments ra
       ON ti.session_id = ra.session_id
      AND ti.item_index = ra.transcript_item_index
...
```

### Lazy-loading function

```rust
pub fn load_reasoning_attachment(
    db_path: &Path,
    session_id: &str,
    transcript_item_index: i64,
) -> Result<Option<ReasoningAttachment>>
```

Returns `None` when no reasoning is attached to that transcript item.
UI callers only pass non-negative transcript-item indices. Negative indices are
diagnostic-only orphan attachments and are never routed through the inspector.

---

## 6. UI: Reasoning Pill in Transcript Rows

### Visible pill

Rendered when `has_visible_reasoning == true`.

```text
gtk::Button .reasoning-pill .flat .pill
├── gtk::Image
└── gtk::Label "Thinking"
```

Clicking emits:

```rust
TranscriptRowOutput::InspectReasoning {
    session_id,
    transcript_item_index,
}
```

### Encrypted-only pill

Rendered when `encrypted_only == true`.

```text
gtk::Box .reasoning-pill-encrypted
├── gtk::Image
└── gtk::Label "Thinking (encrypted)"
```

Non-interactive.  
Dimmed appearance.

### Placement rules

- **Message rows**: after model/timestamp in the header
- **Tool call rows**: near the existing inspect action
- **Subagent rows**: near the existing inspect action
- **Tool burst rows**:
  - when one or more grouped child tool calls have visible reasoning, show a non-interactive
    burst-header pill labelled with a count of affected children, for example `1 thinking`
    or `2 thinking`
  - if no grouped child has visible reasoning but one or more children are encrypted-only,
    show a dimmed non-interactive burst-header pill labelled `1 encrypted` or
    `2 encrypted`
  - if a mixed burst contains both visible-reasoning children and encrypted-only children,
    the collapsed header shows only the visible `N thinking` count in v1; encrypted-only
    children remain discoverable after expansion via their own dimmed child pills
  - the burst-header pill is informational only and never opens the inspector
  - reasoning inspection happens only on the expanded child tool-call rows
  - each expanded child that has reasoning keeps its own pill and routes to
    `InspectReasoning { session_id, transcript_item_index }`
  - the visible-thinking count reflects the number of child tool-call rows with visible
    reasoning, not the number of reasoning blocks

A tool burst never owns reasoning itself. It only aggregates the presence of reasoning
attached to its child transcript items. This ensures grouped tool-call UI does not hide the
presence of reasoning entirely while keeping inspection bound to a real child transcript row.

### CSS

Reuse existing `.pill` styling as the base.  
Add only the feature-specific accent and dimmed variants.

---

## 7. UI: Inspector Pane — Reasoning View

### New selection variant

```rust
enum InspectorSelection {
    None,
    ToolCall { session_id, tool_call_id },
    Subagent { session_id, subagent_id },
    Reasoning { session_id, transcript_item_index },
}
```

### New messages

```rust
ToolInspectorPaneMsg::SelectReasoning {
    session_id: String,
    transcript_item_index: i64,
}

ToolInspectorPaneCmd::Reasoning {
    request_id: u64,
    result: Result<Option<ReasoningAttachment>, String>,
}
```

### New content page

The reasoning inspector page renders:

- metadata row using `source_model` and `source_timestamp` when available
- optional **Summary** section
- optional **Reasoning** section

If both `summary_text` and `visible_text` exist, show summary first and full reasoning second.

Encrypted payload is not displayed directly in v1.

### Loading flow

1. `SelectReasoning` received
2. show loading page
3. `spawn_oneshot_command` loads `ReasoningAttachment`
4. switch to reasoning page
5. replace any currently displayed tool call or subagent content

### Closing

Escape closes the inspector pane using existing behavior.

---

## 8. Message Routing

Routing mirrors the existing inspect flow, but targets the raw database
`transcript_item_index`:

```text
TranscriptRow
  → TranscriptRowOutput::InspectReasoning { session_id, transcript_item_index }
    → TranscriptDisplay relays
      → SessionDetail receives
        → ToolInspectorPaneMsg::SelectReasoning { ... }
```

No standalone reasoning row component is introduced.

Grouped tool-burst UI must preserve each child row's original database
`transcript_item_index` separately from any top-level `display_index`, so expanding a burst
and clicking a child pill always loads the correct reasoning attachment.

---

## 9. Testing & Verification

- **Parser unit tests**
  - Claude Code: thinking/text split, empty thinking filtered, reasoning-only event attaches to first tool call
  - Codex: summary extraction, encrypted payload extraction, reasoning item attaches to next visible transcript item
  - OpenCode: part-level accumulation, empty reasoning ignored unless encrypted metadata exists
  - Mistral Vibe: conditional extraction when `reasoning_content` is present (tested via synthetic fixture); graceful no-op verified on current real Vibe-shaped sessions
- **DB tests**
  - v9 creates `reasoning_attachments`
  - v9 clears `file_fingerprints`
  - transcript preview query derives reasoning flags correctly
  - lazy-load returns the full attachment payload
  - per-session, per-file, and clear-all cleanup paths co-delete `reasoning_attachments` rows
  - multiple orphan attachments in one session receive distinct negative indices and do not
    collide on the primary key
  - orphan negative-index allocation is deterministic for a fixed session input and parser
    version
  - file-level reindex for a session clears and rebuilds that session's orphan reasoning rows
    so the reserved negative index range remains collision-free
- **UI tests / manual verification**
  - message row with visible reasoning pill
  - tool call row with visible reasoning pill
  - grouped tool burst header shows a non-interactive count pill when one or more children
    have reasoning
  - grouped tool burst header uses `N thinking` for visible child reasoning and a dimmed
    `N encrypted` pill when only encrypted-only children are present
  - expanding a grouped tool burst keeps clickable child pills on only the affected tool calls
  - encrypted-only pill is dimmed and non-clickable
  - inspector renders summary-only and full-text cases correctly
  - clicking a child pill inside a grouped tool burst opens the matching reasoning attachment
  - no pill appears for empty dropped reasoning
- **Fixtures**
  - Claude Code fixture with thinking-only event before tool call
  - Codex fixture with summary + encrypted reasoning
  - OpenCode fixture with visible reasoning text
  - OpenCode fixture with encrypted-only reasoning part
  - Mistral Vibe: no dedicated fixture; parser test uses inline JSON payload with `reasoning_content`

---

## 10. Files Affected

| File | Change |
|------|--------|
| `src/parsers/claude_code.rs` | Split visible text from thinking; attach reasoning to next emitted transcript item |
| `src/parsers/mistral_vibe.rs` | Conditionally extract `reasoning_content` when present |
| `src/parsers/opencode/mod.rs` | Accumulate `reasoning` parts and attach to next visible part-derived transcript item |
| `src/parsers/codex.rs` | Extract reasoning summaries and encrypted payload; attach to next transcript item |
| `src/database/schema.rs` | Add `reasoning_attachments` table and v9 migration |
| `src/database/indexer.rs` | Persist reasoning attachments during re-index; add `DELETE FROM reasoning_attachments` to all session-cleanup paths (per-session, per-file, and clear-all) |
| `src/database/mod.rs` | Join reasoning preview flags into transcript queries; add `load_reasoning_attachment` |
| `src/models/` | Add explicit reasoning attachment/preview types |
| `src/ui/transcript_row.rs` | Add pills on message/tool/subagent rows, a non-interactive count pill for tool-burst headers, and preserve raw child transcript indices for grouped tool rows |
| `src/ui/transcript_display.rs` | Relay `InspectReasoning` and aggregate grouped reasoning counts |
| `src/ui/session_detail.rs` | Route `InspectReasoning` to the inspector pane while keeping database transcript indices distinct from display indices |
| `src/ui/tool_inspector_pane.rs` | Add reasoning selection, loading, and rendering |
| `data/resources/style.css` | Add reasoning pill styles |
| `tests/fixtures/` | Add reasoning-bearing fixtures across supported parsers |

---

## 11. Scope Boundaries (v1)

**In scope:**

- explicit reasoning extraction for supported formats
- transcript-item-level reasoning attachment
- visible and encrypted-only reasoning pills
- inspector-pane reasoning view
- grouped tool-row reasoning indicators
- migration with re-index via fingerprint clear

**Out of scope (future):**

- FTS/search over reasoning content
- inline reasoning transcript rows
- raw display of encrypted payload
- token count in the pill
- global "show all reasoning" mode
- exporting reasoning separately from the session transcript
