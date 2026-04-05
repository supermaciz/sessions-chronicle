# Reasoning/Thinking Blocks Display — Design

**Issue:** [#45](https://github.com/supermaciz/sessions-chronicle/issues/45)  
**Exploration:** [2026-04-05-reasoning-blocks-exploration.md](2026-04-05-reasoning-blocks-exploration.md)  
**Date:** 2026-04-05  
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

Each extracted reasoning payload is stored explicitly, without sentinels:

```rust
pub struct ReasoningAttachment {
    pub session_id: String,
    pub transcript_item_index: usize,
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
  reasoning is dropped and recorded in diagnostics/logging

This keeps attachment deterministic and avoids ambiguous cross-turn pairing.

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
    transcript_item_index: usize,
) -> Result<Option<ReasoningAttachment>>
```

Returns `None` when no reasoning is attached to that transcript item.

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
  - aggregate child reasoning count in the burst header when any grouped child has reasoning
  - retain per-child pills when the group is expanded

This ensures grouped tool-call UI does not hide the presence of reasoning entirely.

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
    transcript_item_index: usize,
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

Routing mirrors the existing inspect flow, but targets `transcript_item_index`:

```text
TranscriptRow
  → TranscriptRowOutput::InspectReasoning { session_id, transcript_item_index }
    → TranscriptDisplay relays
      → SessionDetail receives
        → ToolInspectorPaneMsg::SelectReasoning { ... }
```

No standalone reasoning row component is introduced.

---

## 9. Testing & Verification

- **Parser unit tests**
  - Claude Code: thinking/text split, empty thinking filtered, reasoning-only event attaches to first tool call
  - Codex: summary extraction, encrypted payload extraction, reasoning item attaches to next visible transcript item
  - OpenCode: part-level accumulation, empty reasoning ignored unless encrypted metadata exists
  - Mistral Vibe: conditional extraction when `reasoning_content` is present; graceful no-op on current Vibe-shaped sessions
- **DB tests**
  - v9 creates `reasoning_attachments`
  - v9 clears `file_fingerprints`
  - transcript preview query derives reasoning flags correctly
  - lazy-load returns the full attachment payload
- **UI tests / manual verification**
  - message row with visible reasoning pill
  - tool call row with visible reasoning pill
  - grouped tool burst shows aggregate reasoning indicator
  - encrypted-only pill is dimmed and non-clickable
  - inspector renders summary-only and full-text cases correctly
  - no pill appears for empty dropped reasoning
- **Fixtures**
  - Claude Code fixture with thinking-only event before tool call
  - Codex fixture with summary + encrypted reasoning
  - OpenCode fixture with visible reasoning text
  - OpenCode fixture with encrypted-only reasoning part
  - Mistral Vibe fixture with `reasoning_content` if available; otherwise current no-reasoning fixture remains valid

---

## 10. Files Affected

| File | Change |
|------|--------|
| `src/parsers/claude_code.rs` | Split visible text from thinking; attach reasoning to next emitted transcript item |
| `src/parsers/mistral_vibe.rs` | Conditionally extract `reasoning_content` when present |
| `src/parsers/opencode/mod.rs` | Accumulate `reasoning` parts and attach to next visible part-derived transcript item |
| `src/parsers/codex.rs` | Extract reasoning summaries and encrypted payload; attach to next transcript item |
| `src/database/schema.rs` | Add `reasoning_attachments` table and v9 migration |
| `src/database/indexer.rs` | Persist reasoning attachments during re-index |
| `src/database/mod.rs` | Join reasoning preview flags into transcript queries; add `load_reasoning_attachment` |
| `src/models/` | Add explicit reasoning attachment/preview types |
| `src/ui/transcript_row.rs` | Add pills on message/tool/subagent rows and aggregate handling for grouped tool rows |
| `src/ui/transcript_display.rs` | Relay `InspectReasoning` and aggregate grouped reasoning counts |
| `src/ui/session_detail.rs` | Route `InspectReasoning` to the inspector pane |
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
