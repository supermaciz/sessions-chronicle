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

### Data flow

```text
Parser  -->  ParsedSession.token_usage  -->  indexer writes 5 DB columns
                                                    |
UI  <--  Session.token_usage  <--  session_from_row() reads 5 DB columns
```

Parsers populate `ParsedSession.token_usage`. The indexer reads it from there
(not from `parsed.session`). `Session.token_usage` is only populated when loading
back from the database.

---

## Section 1 - Data Model

### New struct: `TokenUsage` (`src/models/token_usage.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
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

`PartialEq` and `Eq` are derived for use in test assertions (`assert_eq!`).

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
- Use named column access (`row.get::<_, Option<i64>>("input_tokens")`) instead of
  positional indices to eliminate index drift issues entirely.

### Write path (`src/database/indexer.rs`)

- Update `insert_parsed_session()` to include token columns in
  `INSERT OR REPLACE INTO sessions`.
- Token data comes from `parsed.token_usage` (not `parsed.session.token_usage`).
  The `Session` struct only gains `token_usage` when loaded back from the database.

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
Pick the snapshot with the **highest global total** (`input + output + reasoning`)
across the file. Use all fields from that single winning snapshot — do not mix fields
from different snapshots.

**Fields:**

- `input_tokens` <- `total_token_usage.input_tokens`
- `output_tokens` <- `total_token_usage.output_tokens`
- `cache_read_tokens` <- `total_token_usage.cached_input_tokens`
- `cache_write_tokens` <- `None`
- `reasoning_tokens` <- `total_token_usage.reasoning_output_tokens`

**Edge cases:**

- `info: null` => unknown snapshot, skip
- out-of-order or duplicated snapshots => highest global total still wins

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

**Label** shows only the total:

```text
Tokens: 13 479
```

**Tooltip** on hover shows the full breakdown:

```text
12 345 input · 678 output · 456 reasoning
Cache: 9 012 read · 234 write
```

This keeps the header card compact while making the full breakdown discoverable.
The tooltip is built dynamically: reasoning line only when present, cache line only when
at least one cache field is non-null.

`total = input + output + reasoning(if any)`

### Widget structure

```text
gtk::Box [horizontal, spacing=6, halign=Start]
  gtk::Label "Tokens:" [dim-label]
  #[name="token_usage_label"]
  gtk::Label "<formatted total>" [dim-label, has-tooltip=true, tooltip-text=<breakdown>]
```

### Formatting helpers (`src/ui/format.rs`)

Use a simple pure-Rust helper with **thin space** (U+2009) as thousands separator.
This follows the SI/GNOME convention, avoids a new dependency (`num-format` and its
`localeconv` C call are overkill for integer-only formatting), and is fully
deterministic across platforms.

```rust
/// Format an integer with thin-space thousands grouping: 12 345 678
pub fn format_token_count(n: i64) -> String { ... }

/// Build the total label text from TokenUsage: "13 479"
pub fn format_token_total(usage: &TokenUsage) -> String { ... }

/// Build the tooltip breakdown text from TokenUsage
pub fn format_token_tooltip(usage: &TokenUsage) -> String { ... }
```

All three are pure functions, unit-testable without a GTK harness.

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
- `format_token_count()` grouping: `0`, `999`, `1000`, `1234567`, negative
- `format_token_total()` output string
- `format_token_tooltip()` permutations:
  - input/output only (no reasoning line, no cache line)
  - with reasoning
  - with cache
  - with reasoning + cache
- parser tests per tool:
  - Claude: dedupe + aggregate correctness
  - Codex: highest-global-total snapshot selection, not per-field max
  - OpenCode: message-level aggregation + step-finish fallback + no double-counting
  - Mistral Vibe: `stats` extraction + null handling

### Integration tests (`tests/`)

- After indexing fixtures, verify token columns in `sessions` are populated for sessions
  with data and remain `NULL` otherwise.
- Verify `load_session()` maps DB rows to `Session.token_usage` correctly.

### End-to-end test

- For at least one tool (Claude Code): fixture file -> parse -> index -> load_session ->
  assert `Session.token_usage` matches expected `TokenUsage` values.
  This catches regressions across the full pipeline.

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
3. **Number formatting:** pure-Rust helper with thin space (U+2009) as thousands separator.
   No `num-format` dependency — SI/GNOME convention, deterministic, zero platform variance.
4. **Display strategy:** total in label, full breakdown in tooltip (keeps header compact)
5. **Codex max:** select the single snapshot with the highest global total, use all its
   fields as-is (never mix fields from different snapshots)
6. **Data flow:** parsers write to `ParsedSession.token_usage`; `Session.token_usage` is
   only populated from the database read path
7. **Column access:** use named columns (`row.get("input_tokens")`) instead of positional
   indices to prevent index drift

---

## Section 8 - Implementation Order

1. **Model** — `token_usage.rs`, update `session.rs`, `models/mod.rs`
2. **ParsedSession** — add `token_usage` field to `parsers/mod.rs`, propagate `None` default
3. **Parsers** — implement extraction in each parser (Claude, Codex, OpenCode, Mistral Vibe)
4. **Database** — schema v3 migration, update `session_from_row()` + SELECT queries, update indexer
5. **Fixtures + tests** — extend fixtures with token data, add parser unit tests, DB migration tests, end-to-end test
6. **UI** — `format.rs` helpers + formatter tests, `session_detail.rs` token row with tooltip

Each step compiles and passes `cargo test` before moving to the next.

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
| `src/models/token_usage.rs` | New file: `TokenUsage` struct (Debug, Clone, PartialEq, Eq) + total helper |
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
| `src/ui/format.rs` | Add `format_token_count`, `format_token_total`, `format_token_tooltip` |
| `src/ui/session_detail.rs` | Add token row (label + tooltip) in metadata header |
| `tests/` | Add parser, DB migration, and mapping tests |
| `tests/fixtures/` | Extend fixtures with token coverage |

---

**Last Updated:** 2026-02-28
