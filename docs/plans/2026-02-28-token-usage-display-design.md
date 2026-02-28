# Token Usage Display in SessionDetail - Design

**Date:** 2026-02-28
**Issue:** [#43](https://github.com/supermaciz/sessions-chronicle/issues/43)
**Status:** Design approved (decisions captured)

---

## Goal

Show per-session token usage in the `SessionDetail` header card when data is available.
When available, show:

- a total token count
- a best-effort breakdown (`input`, `output`, optional `reasoning`)
- cache metrics separately when present (`cache read`, `cache write`)

Hide the entire row when no token data exists for a session.

---

## Architecture Overview

The feature spans five layers:

1. **Parsers** - extract and aggregate session-level token data during indexing
2. **Parser output model** - add `token_usage` to `ParsedSession`
3. **Domain model** - `TokenUsage` struct + `Session.token_usage`
4. **Database** - 5 new nullable columns in `sessions` table (schema migration v3)
5. **UI** - compact token row in `SessionDetail` metadata card

---

## Section 1 - Data Model

### New struct: `TokenUsage` (`src/models/token_usage.rs`)

```rust
#[derive(Debug, Clone)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>, // Codex and OpenCode when present
}

impl TokenUsage {
    pub fn display_total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.reasoning_tokens.unwrap_or(0)
    }
}
```

Notes:

- `input_tokens` + `output_tokens` are required for v1 storage/display.
- `cache_*` and `reasoning_tokens` are optional.
- Cache metrics are displayed separately and are **not** added to `display_total_tokens()`
  because cache semantics differ across providers and can overlap with input accounting.

### Updated `Session` struct (`src/models/session.rs`)

Add:

```rust
pub token_usage: Option<TokenUsage>,
```

`None` means no token data is available for this session (row hidden in UI).

### Module exports (`src/models/mod.rs`)

- Add `pub mod token_usage;`
- Re-export with `pub use token_usage::TokenUsage;`

---

## Section 2 - Database Schema (migration v3)

Add 5 nullable `INTEGER` columns to `sessions` via `ALTER TABLE ADD COLUMN`:

```sql
ALTER TABLE sessions ADD COLUMN input_tokens INTEGER;
ALTER TABLE sessions ADD COLUMN output_tokens INTEGER;
ALTER TABLE sessions ADD COLUMN cache_read_tokens INTEGER;
ALTER TABLE sessions ADD COLUMN cache_write_tokens INTEGER;
ALTER TABLE sessions ADD COLUMN reasoning_tokens INTEGER;
PRAGMA user_version = 3;
```

### Presence rules

- All 5 columns `NULL` => `session.token_usage = None`
- `input_tokens` and `output_tokens` both non-null => `Some(TokenUsage)`
- Inconsistent core data (`input` xor `output`) => treat as unavailable (`None`) and log warning

### Schema migration (`src/database/schema.rs`)

- Add `apply_v3_migration()` and call it after v2 in `initialize_database()`.
- Keep migration idempotent by ignoring `duplicate column name` errors.
- Update schema tests to assert fresh DB initializes at version 3.

### Read path (`src/database/mod.rs`)

- Update `session_from_row()` to read the 5 token columns.
- Construct `TokenUsage` using the presence rules above.
- Update all session SELECT queries to include token columns:
  - `load_sessions`
  - `search_sessions_with_query`
  - `load_session`
- Prefer column index constants in `session_from_row()` to avoid accidental index drift.

### Write path (`src/database/indexer.rs`)

- Update `insert_parsed_session()` to include token columns in
  `INSERT OR REPLACE INTO sessions`.

---

## Section 3 - Parser Changes

`ParsedSession` gains:

```rust
pub token_usage: Option<TokenUsage>
```

Each parser computes a session-level aggregate and fills `ParsedSession.token_usage`.

### Claude Code (`src/parsers/claude_code.rs`)

**Source:** `message.usage` on `type == "assistant"` events.

**Fields:**

- `input_tokens` <- sum of `usage.input_tokens`
- `output_tokens` <- sum of `usage.output_tokens`
- `cache_read_tokens` <- sum of `usage.cache_read_input_tokens`
- `cache_write_tokens` <- sum of `usage.cache_creation_input_tokens`
- `reasoning_tokens` <- `None`

**Deduplication rule:**

- Dedupe by `(requestId, message.id)` when both identifiers exist.
- For duplicates, keep the entry with the highest observed usage total for that key
  (equivalent to "last/max" in append-only logs).
- If either identifier is missing, do not dedupe that event.

**Result:** `Some(TokenUsage)` if at least one valid usage block was found.

### Codex (`src/parsers/codex.rs`)

**Source:** `event_msg` entries with `payload.type == "token_count"`.

**Strategy:** `info.total_token_usage` is a running session snapshot.
Use the **maximum observed** `total_token_usage` across the file (not "last non-null").

**Fields:**

- `input_tokens` <- `total_token_usage.input_tokens`
- `output_tokens` <- `total_token_usage.output_tokens`
- `cache_read_tokens` <- `total_token_usage.cached_input_tokens`
- `cache_write_tokens` <- `None`
- `reasoning_tokens` <- `total_token_usage.reasoning_output_tokens`

**Edge cases:**

- `info: null` => unknown snapshot, skip
- out-of-order or duplicated snapshots => max snapshot still wins

**Result:** `Some(TokenUsage)` if any non-null `total_token_usage` was seen.

### OpenCode (`src/parsers/opencode/`)

**Source priority:**

1. Assistant message tokens (`message.data.tokens`) - preferred
2. `part.type == "step-finish".tokens` - fallback only when no message-level tokens exist

Do not aggregate both sources in the same session to avoid double-counting.

**Fields:**

- `input_tokens` <- sum of `tokens.input` (fallback `tokens.prompt`)
- `output_tokens` <- sum of `tokens.output` (fallback `tokens.completion`)
- `cache_read_tokens` <- sum of `tokens.cache.read` when present
- `cache_write_tokens` <- sum of `tokens.cache.write` when present
- `reasoning_tokens` <- sum of `tokens.reasoning` when present

Implementation note:

- Keep extraction shared in `src/parsers/opencode/mod.rs` so SQLite and JSON backends
  reuse the same token parsing rules.

**Result:** `Some(TokenUsage)` if the selected source yields usable token data.

### Mistral Vibe (`src/parsers/mistral_vibe.rs`)

**Source:** `meta.json.stats` (session-level only).

**Fields:**

- `input_tokens` <- `stats.session_prompt_tokens`
- `output_tokens` <- `stats.session_completion_tokens`
- `cache_read_tokens` <- `None`
- `cache_write_tokens` <- `None`
- `reasoning_tokens` <- `None`

**Edge case:** `stats: null` or missing required fields => `None`.

**Result:** `Some(TokenUsage)` only when prompt + completion totals are both present.

---

## Section 4 - UI (SessionDetail header card)

### Placement

Add a new `gtk::Box` row in `src/ui/session_detail.rs`, below the
tool/message-count/time row and above Session ID.

Visibility is controlled by `session.token_usage.is_some()`.

### Display format

`total = input + output + reasoning(if any)`

Examples:

**Minimum:**

```text
Tokens: 13 023 total - 12 345 input - 678 output
```

**With reasoning:**

```text
Tokens: 13 479 total - 12 345 input - 678 output - 456 reasoning
```

**With cache metrics:**

```text
Tokens: 13 479 total - 12 345 input - 678 output - 456 reasoning - 9 012 cache read - 234 cache write
```

### Widget structure

```text
gtk::Box [horizontal, spacing=6, halign=Start]
  gtk::Label "Tokens:" [dim-label]
  #[name="token_usage_label"]
  gtk::Label "<formatted value>" [dim-label]
```

### Formatting helpers

- Follow GNOME localization guidance: use locale-provided numeric separators.
- Implement token count formatting in `src/ui/format.rs` with system locale behavior
  (for example via `num-format` `SystemLocale`, with deterministic fallback).
- Add a pure function to build the token usage label text from `TokenUsage`
  (unit-testable without GTK harness).

### Hiding when unavailable

```rust
#[watch]
set_visible: model.session.as_ref()
    .and_then(|s| s.token_usage.as_ref())
    .is_some(),
```

No "N/A" placeholder; hide the full row.

---

## Section 5 - Error Handling and Edge Cases

| Case | Behavior |
|------|----------|
| No token data in source file | `token_usage = None`, row hidden |
| Codex `info: null` | Skip snapshot (unknown, not zero) |
| Codex out-of-order snapshots | Use max observed `total_token_usage` |
| OpenCode message tokens and step-finish tokens both present | Use message-level only |
| OpenCode has only step-finish tokens | Use step-finish fallback |
| Claude duplicate assistant events | Dedupe by `(requestId, message.id)` |
| Inconsistent DB row (`input` xor `output`) | Log warning and treat as unavailable |
| Very large numbers | `i64` capacity is sufficient |

---

## Section 6 - Testing

### Unit tests

- `TokenUsage::display_total_tokens()`
- token label formatter permutations:
  - input/output only
  - with reasoning
  - with cache
  - with reasoning + cache
  - locale-aware grouping fallback behavior
- parser tests per tool:
  - Claude: dedupe + aggregate correctness
  - Codex: **max observed** snapshot selection
  - OpenCode: message-level aggregation + step-finish fallback + no double-counting
  - Mistral Vibe: `stats` extraction + null handling

### Integration tests (`tests/`)

- After indexing fixtures, verify token columns in `sessions` are populated for sessions
  with data and remain `NULL` otherwise.
- Verify `load_session()` maps DB rows to `Session.token_usage` correctly.

### Schema migration tests

- Fresh DB initializes at v3.
- v2 -> v3 migration adds all 5 columns without data loss.
- Re-running migration remains idempotent.

### Fixture coverage

Existing fixtures may not include all token shapes.
Extend fixtures to cover at least:

- Claude usage with duplicate assistant events
- Codex `token_count` with non-monotonic order and `info: null`
- OpenCode message-level tokens and step-finish-only fallback
- Mistral Vibe `stats` present and `stats: null`

---

## Section 7 - Decision Log

1. **Core fields:** strict v1 (`input_tokens` + `output_tokens` required)
2. **Total semantics:** `total = input + output + reasoning` (cache excluded)
3. **Number formatting:** GNOME-style locale-aware separators (system locale)

### References for decision 3

- GNOME localization guidance: use locale-provided values
  - https://developer.gnome.org/documentation/guidelines/localization/practices.html
- GNOME localization archive note (explicit numeric separator guidance)
  - https://wiki.gnome.org/TranslationProject(2f)DevGuidelines(2f)Use(20)locale(2d)provided(20)values.html
- Rust implementation option for system locale formatting
  - https://docs.rs/num-format

---

## Non-Goals

- No per-message token breakdown (future feature)
- No cost estimation (future feature)
- No sorting/filtering by token count (future feature)
- No token display in session list rows (SessionDetail only)

---

## Files Changed

| File | Change |
|------|--------|
| `src/models/token_usage.rs` | New file: `TokenUsage` struct + total helper |
| `src/models/session.rs` | Add `token_usage: Option<TokenUsage>` |
| `src/models/mod.rs` | Export `TokenUsage` module/type |
| `src/parsers/mod.rs` | Add `token_usage: Option<TokenUsage>` to `ParsedSession` |
| `src/parsers/claude_code.rs` | Extract usage + dedupe strategy |
| `src/parsers/codex.rs` | Extract token snapshots; pick max observed total |
| `src/parsers/opencode/mod.rs` | Shared token extraction + source priority logic |
| `src/parsers/opencode/json_backend.rs` | Surface message token metadata for shared extractor |
| `src/parsers/opencode/sqlite_backend.rs` | Surface message token metadata for shared extractor |
| `src/parsers/mistral_vibe.rs` | Extract `meta.json.stats` token totals |
| `src/database/schema.rs` | Add `apply_v3_migration()`, bump schema version |
| `src/database/mod.rs` | Read token columns in session queries/mapping |
| `src/database/indexer.rs` | Persist token columns in session upsert |
| `src/ui/format.rs` | Add token number formatting helper |
| `src/ui/session_detail.rs` | Add token row + label formatting in metadata header |
| `tests/` | Add parser, DB migration, and mapping tests |
| `tests/fixtures/` | Extend fixtures with token coverage |

---

**Last Updated:** 2026-02-28
