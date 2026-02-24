# Model Tracking — Design Document

**Date**: 2026-02-24
**Phase**: 8
**Scope**: Parsers + DB schema only (UI in a separate PR)

---

## Goal

Track which LLM model generated each assistant message across all 4 supported tools (Claude Code, OpenCode, Codex, Mistral Vibe). Store the raw model slug per-message for future display, filtering, and analytics.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Storage granularity | Per-message | Captures mid-session model switches accurately |
| Model format | Raw slug (e.g. `claude-opus-4-6`) | Simple, no information loss, normalize later if needed |
| Which messages | Assistant only | Model identifies which LLM generated the response |
| Mistral Vibe handling | Propagate session-level `active_model` to all messages | Best available data; no per-message model in Vibe format |
| FTS5 strategy | Recreate with `model UNINDEXED` column (v2 migration) | Cleaner than separate table, avoids JOINs |
| Session-level model | None (derive via query) | Pure normalization; no denormalization on sessions table |

## Database Schema Changes

### v2 Migration

Drop and recreate the `messages` FTS5 table with a new `model` column:

```sql
DROP TABLE IF EXISTS messages;

CREATE VIRTUAL TABLE messages USING fts5(
    session_id UNINDEXED,
    message_index UNINDEXED,
    role UNINDEXED,
    content,
    timestamp UNINDEXED,
    model UNINDEXED
);
```

Schema version bumps from `1` to `2`, triggering a full reindex on first launch.

### Message Struct

Add `model: Option<String>` to the `Message` struct:

```rust
pub struct Message {
    pub session_id: String,
    pub index: usize,
    pub role: Role,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub model: Option<String>,  // NEW: raw model slug, None for user messages
}
```

### Query Helpers (for future UI)

```sql
-- Models used in a session
SELECT DISTINCT model FROM messages WHERE session_id = ? AND model IS NOT NULL;

-- All known models (for sidebar filter)
SELECT DISTINCT model FROM messages WHERE model IS NOT NULL;
```

## Parser Changes

### Claude Code (`src/parsers/claude_code.rs`)

- Read `message.model` from assistant JSONL events
- Already present in source data, currently ignored
- User events → `model: None`
- Assistant events → `model: Some("claude-opus-4-6")` etc.
- Sentinel `<synthetic>` model slugs should be treated as `None`

### Codex (`src/parsers/codex.rs`)

- Extract from `turn_context.payload.model` (per-turn)
- Apply to the assistant response message for that turn
- User messages → `model: None`

### OpenCode (`src/parsers/opencode/`)

- **SQLite backend**: Decode `message.data` JSON blob → extract `modelID` (or `model.modelID`)
- **JSON fallback**: Same extraction from `msg_*.json` files
- Assistant messages only; user messages → `model: None`

### Mistral Vibe (`src/parsers/mistral_vibe.rs`)

- Read `config.active_model` from `meta.json`
- Propagate to all assistant messages in the session
- If `config` or `active_model` is absent → `model: None`

## Indexer Changes

- Update `INSERT INTO messages` statement to include the `model` parameter
- Accept `model: Option<String>` from parsed messages

## Testing

- **Unit tests per parser**: Verify model extraction from existing fixtures
  - Assistant messages have expected model slug
  - User messages have `None`
- **DB tests**: Verify v2 migration schema, model data roundtrips through insert/query
- **Update fixtures if needed**: Ensure fixture files contain model data
- **Full suite**: `cargo test --all --no-fail-fast`

## Task Breakdown

| Task ID | Subject | Blocked By |
|---------|---------|------------|
| #7 | Implement v2 schema migration with model column | — |
| #8 | Update parsers to extract model information | #7 |
| #9 | Add tests for model extraction and persistence | #7, #8 |

## Out of Scope

- UI display (badges, tooltips, per-message indicators) — separate PR
- Sidebar model filter — separate PR
- Analytics/charts — future feature
- Model normalization (provider/family/version splitting) — future if needed
