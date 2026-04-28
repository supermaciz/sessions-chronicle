# Model Tracking - Design Document

**Date:** 2026-02-24  
**Status:** Implemented [#39](https://github.com/supermaciz/sessions-chronicle/pull/39)  
**Phase:** 8  
**Scope:** Parsers + DB schema only (UI in a separate PR)

---

## Goal

Track which LLM model generated each assistant message across all 4 supported tools (Claude Code, OpenCode, Codex, Mistral Vibe). Store the raw model slug per message for future display, filtering, and analytics.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Storage granularity | Per-message | Captures mid-session model switches accurately |
| Model format | Raw slug (for example `claude-opus-4-6`) | No information loss; normalization can come later |
| Which messages get model | Assistant messages only | Model identifies generator of assistant output |
| Mistral Vibe handling | Propagate session-level model to assistant messages | Best available source, no reliable per-message model |
| FTS5 strategy | Recreate `messages` FTS5 table with `model UNINDEXED` (v2 migration) | FTS virtual tables cannot be altered to add columns directly |
| Session-level model | No denormalized column on `sessions` | Derive with SQL from message-level data |
| Normalization | Shared helper across parsers | Consistent semantics and testability |

## Data Contract

### Message struct

Add `model: Option<String>` to `Message`:

```rust
pub struct Message {
    pub session_id: String,
    pub index: usize,
    pub role: Role,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub model: Option<String>, // raw model slug, only set on assistant messages
}
```

### Model normalization helper

Introduce one shared helper used by all parsers:

- Input: optional raw JSON value/string from source format.
- Output: `Option<String>`.
- Rules:
1. Non-string -> `None` (log debug).
2. Trim whitespace.
3. Empty string -> `None`.
4. Sentinel `<synthetic>` -> `None`.
5. Otherwise preserve raw slug as-is (no case rewrite, no splitting).

## Database Schema Changes

### v2 migration requirements

Migration from schema version `1` to `2` must be atomic:

```sql
BEGIN IMMEDIATE;
DROP TABLE IF EXISTS messages;
CREATE VIRTUAL TABLE messages USING fts5(
    session_id UNINDEXED,
    message_index UNINDEXED,
    role UNINDEXED,
    content,
    timestamp UNINDEXED,
    model UNINDEXED
);
COMMIT;
PRAGMA user_version = 2;
```

Implementation notes:

- Run DDL inside a transaction; on any failure, rollback and keep previous schema.
- Set `PRAGMA user_version = 2` **after** the transaction commits successfully.
  `PRAGMA user_version` is not transactional in SQLite (takes effect immediately),
  so placing it inside the transaction would mark version 2 even if `COMMIT` fails.
  Setting it after `COMMIT` means a crash between commit and pragma leaves version
  at 1, which simply re-runs the idempotent migration on next launch.
- Existing indexed messages are discarded by design and rebuilt by normal startup indexing.

### Query helpers (for later UI/filter work)

```sql
-- Models used in one session
SELECT DISTINCT model
FROM messages
WHERE session_id = ?1
  AND model IS NOT NULL
  AND trim(model) <> ''
ORDER BY model COLLATE NOCASE;

-- All known models
SELECT DISTINCT model
FROM messages
WHERE model IS NOT NULL
  AND trim(model) <> ''
ORDER BY model COLLATE NOCASE;
```

## Parser Changes

### Claude Code (`src/parsers/claude_code.rs`)

- Source: `assistant` event -> `message.model`.
- Apply only when emitting assistant message rows.
- User messages always keep `model: None`.
- Pass extracted value through shared normalization helper.

### Codex (`src/parsers/codex.rs`)

Source and correlation must be explicit:

- Source event: `turn_context` with `payload.model`.
- Handle both known envelopes for `turn_context` (do not assume `event_msg` only):
1. top-level `type == "turn_context"` with `payload.model`
2. wrapped `type == "event_msg"` and `payload.type == "turn_context"` with `payload.model`
- Keep parser state `current_turn_model: Option<String>`.
- On each `turn_context`, update `current_turn_model` using normalization helper.
- On `event_msg.payload.type == "agent_message"`, assign `model = current_turn_model.clone()`.
- On user messages, always `None`.

Edge semantics:

- If no prior `turn_context`, assistant message model is `None`.
- If multiple `turn_context` events occur before an assistant message, last one wins.
- If a `turn_context` has missing/invalid/empty model, it resets state to `None`.

### OpenCode (`src/parsers/opencode/`)

Backend contract update is required:

- Extend `MessageMetadata` with `model: Option<String>`.
- Parse model from message-level `data` JSON in both backends:
1. `data.modelID`
2. fallback `data.model.modelID`
- Normalize with shared helper.

Mapping to emitted `Message`:

- Assistant text messages inherit `message.model`.
- User text messages force `model: None` even if source contains a model.

### Mistral Vibe (`src/parsers/mistral_vibe.rs`)

- Read session-level model from `meta.json` path `config.active_model`.
- Normalize once at parse start.
- Apply to all emitted assistant messages.
- User messages always `None`.
- If `config.active_model` absent/invalid/empty, assistant messages get `None`.

## Indexer Changes

- Update `INSERT INTO messages` to include `model`.
- Bind `msg.model` in insert params.
- Keep all current delete/replace behavior per session unchanged.

## Testing

### Parser unit tests

Add/extend tests per parser:

- Assistant message gets expected slug.
- User message keeps `None`.
- Missing/invalid model field yields `None`.
- Sentinel `<synthetic>` yields `None`.
- Tool-specific edge cases:
1. Codex model switch between turns.
2. OpenCode `modelID` and nested `model.modelID`.
3. Mistral Vibe `config.active_model` present vs absent.

### DB schema tests

- Fresh DB initializes with `user_version = 2`.
- v1 -> v2 migration recreates `messages` with `model` column.
- Message insert/query roundtrip preserves `model`.
- Distinct-model helper queries return deterministic sorted values.
- Update existing raw `INSERT INTO messages` test statements to include the new `model` column (`NULL` when not relevant), including current coverage in `tests/load_session.rs`, `tests/message_preview.rs`, and `tests/search_sessions.rs`.

### Integration tests with fixtures

Add/update fixtures with real model fields:

- `tests/fixtures/claude_sessions/` with assistant `message.model`.
- `tests/fixtures/codex_sessions/` with `turn_context.payload.model` in both supported envelopes (top-level `turn_context` and wrapped `event_msg` form).
- `tests/fixtures/opencode_storage/` JSON + SQLite paths containing `modelID`.
- `tests/fixtures/vibe_sessions/` with `meta.config.active_model`.

Run full validation:

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
cargo test --all --no-fail-fast
```

## Task Breakdown

| Task ID | Subject | Blocked By |
|---------|---------|------------|
| #7 | Implement schema v2 migration (`messages.model`) with atomic migration flow | - |
| #8 | Add `Message.model` and wire DB insert/read paths | #7 |
| #9 | Implement parser extraction + shared model normalization helper | #8 |
| #10 | Add/refresh fixtures and parser + DB tests | #7, #8, #9 |

## Out of Scope

- UI display (badges, tooltips, per-message indicators) - separate PR
- Sidebar model filter UI - separate PR
- Analytics/charts - future feature
- Deep model normalization (provider/family/version decomposition) - future feature
