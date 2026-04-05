# Reasoning/Thinking Blocks Display — Design

**Issue:** [#45](https://github.com/supermaciz/sessions-chronicle/issues/45)  
**Exploration:** [2026-04-05-reasoning-blocks-exploration.md](2026-04-05-reasoning-blocks-exploration.md)  
**Date:** 2026-04-05  
**Decision:** Proposal B — Inspector Pill

---

## Overview

Display reasoning/thinking content from AI assistant sessions in Sessions Chronicle.
A small pill badge on assistant message headers signals that reasoning exists; clicking
the pill opens the existing inspector pane with the full reasoning text.

Reasoning is treated as **forensic evidence** (inspect on demand), not conversation flow.
The transcript layout is never disrupted.

---

## 1. Parser Extraction

Each parser separates reasoning content from response text and produces an `Option<String>`.

| Assistant | Source field | Extraction | Multi-block handling |
|-----------|------------|-----------|---------------------|
| **Claude Code** | `content[].type == "thinking"` | Split `extract_text_from_array`: thinking blocks go to reasoning, text blocks stay in content | Joined with `\n---\n` |
| **Mistral Vibe** | `reasoning_content` on assistant messages | Read field directly | Single field, no joining needed |
| **OpenCode** | `part.type == "reasoning"` | Capture instead of skipping (`PartOutcome::Nothing` → text) | Multiple parts joined with `\n---\n` |
| **Codex** | `response_item.type == "reasoning"` + `encrypted_content` | Store sentinel `"[encrypted]"` | Never displayed; triggers encrypted pill |

### Claude Code: `extract_text_from_array` split

Current behavior (line ~693 of `claude_code.rs`): `thinking` and `text` blocks are
concatenated together into a single string.

New behavior: the function returns two values — response text and reasoning text. Thinking
blocks are collected separately and joined with `\n---\n`. The existing `content` field
receives only `text` blocks.

### Codex encrypted sentinel

The value `"[encrypted]"` is a DB-internal sentinel, never rendered to UI. It exists solely
so that `has_reasoning` is true and `reasoning_encrypted` is true for the pill logic.

---

## 2. Schema Migration (v9)

```sql
-- Add reasoning content column to messages
ALTER TABLE messages ADD COLUMN reasoning_content TEXT;

-- Force re-index so existing sessions get reasoning extracted
DELETE FROM file_fingerprints;
```

Schema version bumped from 8 to 9.

No separate `reasoning_encrypted` column — encrypted state is derived from the sentinel
value `reasoning_content = '[encrypted]'`.

No FTS on reasoning content in v1. Transcript search covers response text only.

---

## 3. Data Model Changes

### `MessagePreview`

```rust
pub struct MessagePreview {
    pub session_id: String,
    pub message_index: usize,
    pub role: Role,
    pub content_preview: String,
    pub content_len: usize,
    pub timestamp: DateTime<Utc>,
    pub model: Option<String>,
    pub has_reasoning: bool,       // NEW — reasoning_content IS NOT NULL
    pub reasoning_encrypted: bool, // NEW — reasoning_content = '[encrypted]'
}
```

`has_reasoning` and `reasoning_encrypted` are derived at query time:

```sql
SELECT ...,
       reasoning_content IS NOT NULL AS has_reasoning,
       reasoning_content = '[encrypted]' AS reasoning_encrypted
FROM messages
WHERE session_id = ?1
```

### Lazy-loading function

```rust
pub fn load_message_reasoning_content(
    db_path: &Path,
    session_id: &str,
    message_index: usize,
) -> Result<Option<String>> {
    // Same pattern as load_message_full_content
    // Returns None if reasoning_content is NULL
    // Returns Some("[encrypted]") filtered out by caller — inspector
    // should never be called for encrypted reasoning
}
```

---

## 4. UI: Reasoning Pill in Transcript Row

### Visible pill (has_reasoning && !reasoning_encrypted)

Added to the assistant message header row, after the model/timestamp label:

```
gtk::Button .reasoning-pill .flat .pill
├── gtk::Image "brain-augmented-symbolic" pixel-size: 12
└── gtk::Label "Thinking"
```

Clicking emits `TranscriptRowOutput::InspectReasoning { session_id, message_index }`.

Keyboard: natively focusable `GtkButton`, activated with Enter/Space.

### Encrypted pill (reasoning_encrypted)

```
gtk::Box .reasoning-pill-encrypted
├── gtk::Image "lock-symbolic" pixel-size: 12
└── gtk::Label "Thinking (encrypted)"
```

Non-interactive `gtk::Box` — not clickable, dimmed appearance.

### Display condition

- Only rendered for `role == Assistant`
- Only rendered when `has_reasoning == true`
- No pill on user messages or tool call rows

### CSS

```css
.reasoning-pill {
  padding: 1px 8px;
  border-radius: 99px;
  font-size: 0.8em;
  background-color: alpha(@accent_color, 0.15);
  color: @accent_color;
}

.reasoning-pill-encrypted {
  padding: 1px 8px;
  border-radius: 99px;
  font-size: 0.8em;
  background-color: alpha(@view_fg_color, 0.08);
  color: alpha(@view_fg_color, 0.5);
}
```

---

## 5. UI: Inspector Pane — Reasoning View

### New selection variant

```rust
enum InspectorSelection {
    None,
    ToolCall { session_id, tool_call_id },
    Subagent { session_id, subagent_id },
    Reasoning { session_id, message_index },  // NEW
}
```

### New messages

```rust
// Input
ToolInspectorPaneMsg::SelectReasoning {
    session_id: String,
    message_index: usize,
}

// Command output
ToolInspectorPaneCmd::Reasoning {
    request_id: u64,
    result: Result<Option<String>, String>,
}
```

### New content_stack page: `"reasoning"`

```
gtk::ScrolledWindow
└── gtk::Box .reasoning-inspector (vertical, spacing: 12, padding: 16)
    ├── gtk::Box (horizontal)
    │   ├── gtk::Label "REASONING" .inspector-section-heading
    │   └── gtk::Label "· {model} · {timestamp}" .dim-label
    └── gtk::Box .reasoning-inspector-body
        └── markdown::render(reasoning_content)
```

Reuses the existing `markdown::render` pipeline — no new renderer.

### Loading flow

1. `SelectReasoning` received → set `LoadState::Loading`, show "loading" page
2. `spawn_oneshot_command` calls `load_message_reasoning_content`
3. On result → switch `content_stack` to `"reasoning"` page, render markdown
4. If already showing a tool call or subagent, it is replaced (one content at a time)

### Closing

Escape closes the inspector pane (existing behavior, unchanged).

---

## 6. Message Routing

The pill lives in `TranscriptRow` (factory component). The inspector is a sibling
component managed by `SessionDetail`. Message routing follows the existing tool-call
inspection path:

```
TranscriptRow
  → TranscriptRowOutput::InspectReasoning { session_id, message_index }
    → TranscriptDisplay relays
      → TranscriptDisplayOutput::InspectReasoning { session_id, message_index }
        → SessionDetail receives, calls:
          inspector.emit(ToolInspectorPaneMsg::SelectReasoning { ... })
          + opens/shows inspector split pane if not visible
```

No new components. No new channels. Same routing pattern as `InspectToolCall`.

---

## 7. Testing & Verification

- **Fixtures**: add test fixture sessions containing thinking blocks (Claude Code), `reasoning_content`
  (Mistral Vibe), reasoning parts (OpenCode), and encrypted reasoning (Codex)
- **Parser unit tests**: verify reasoning is extracted separately from content; verify multi-block
  joining with `\n---\n`; verify Codex sentinel
- **DB test**: verify migration v9 adds column; verify `has_reasoning` / `reasoning_encrypted` derivation
- **Manual verification**: `flatpak-builder --run ... sessions-chronicle --sessions-dir tests/fixtures`
  - Confirm pill appears on assistant messages with reasoning
  - Confirm encrypted pill appears dimmed and non-clickable for Codex
  - Confirm clicking pill opens inspector with full reasoning text
  - Confirm no pill on messages without reasoning
  - Confirm dark mode: pill colors remain legible
  - Confirm Escape closes inspector

---

## 8. Files Affected

| File | Change |
|------|--------|
| `src/parsers/claude_code.rs` | Split `extract_text_from_array` into text + reasoning |
| `src/parsers/mistral_vibe.rs` | Extract `reasoning_content` field |
| `src/parsers/opencode/mod.rs` | Capture `reasoning` part instead of skipping |
| `src/parsers/codex.rs` | Detect encrypted reasoning, store sentinel |
| `src/database/schema.rs` | Migration v9: add `reasoning_content` column, clear fingerprints |
| `src/database/mod.rs` | Add `load_message_reasoning_content`; update message preview queries |
| `src/models/message_preview.rs` | Add `has_reasoning`, `reasoning_encrypted` fields |
| `src/ui/transcript_row.rs` | Add reasoning pill to assistant message header; new output variant |
| `src/ui/transcript_display.rs` | Relay `InspectReasoning` output |
| `src/ui/session_detail.rs` | Route `InspectReasoning` to inspector pane |
| `src/ui/tool_inspector_pane.rs` | Add `Reasoning` selection/msg/cmd/page |
| `data/resources/style.css` | Add `.reasoning-pill`, `.reasoning-pill-encrypted` |
| `tests/fixtures/` | Add reasoning-bearing fixture sessions |

---

## 9. Scope Boundaries (v1)

**In scope:**
- Reasoning extraction from all 4 parsers
- Pill indicator on transcript rows
- Inspector pane reasoning view
- Schema migration with re-index

**Out of scope (future):**
- FTS/search over reasoning content
- Global "show all reasoning" toggle
- Inline expander or dual-column view
- Reasoning content in session-level summary beyond existing `reasoning_tokens`
- Token count for reasoning in the pill (e.g. "Thinking (2.4k tokens)")
