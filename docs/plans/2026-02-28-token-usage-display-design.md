# Token Usage Display in SessionDetail — Design

**Date:** 2026-02-28
**Issue:** [#43](https://github.com/supermaciz/sessions-chronicle/issues/43)
**Status:** Design approved

---

## Goal

Show per-session token usage in the `SessionDetail` header card when data is available.
Degrade gracefully to hidden when no token data exists for a session.

---

## Architecture Overview

The feature spans four layers:

1. **Parsers** — extract token data during indexing
2. **Model** — `TokenUsage` struct + `Session.token_usage`
3. **Database** — 5 new nullable columns in `sessions`, schema migration v3
4. **UI** — new compact row in the `SessionDetail` metadata card

---

## Section 1 — Data Model

### New struct: `TokenUsage` (`src/models/token_usage.rs`)

```rust
#[derive(Debug, Clone)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: Option<i64>,   // Claude Code, Codex, OpenCode
    pub cache_write_tokens: Option<i64>,  // Claude Code, OpenCode
    pub reasoning_tokens: Option<i64>,    // Codex only
}
```

### Updated `Session` struct (`src/models/session.rs`)

Add:

```rust
pub token_usage: Option<TokenUsage>,
```

`None` means no token data available for this session (hidden in UI).

Export `TokenUsage` from `src/models/mod.rs`.

---

## Section 2 — Database Schema (migration v3)

Add 5 nullable `INTEGER` columns to the `sessions` table via `ALTER TABLE ADD COLUMN`.
This mirrors the v1 migration pattern (same idempotent approach).

```sql
ALTER TABLE sessions ADD COLUMN input_tokens INTEGER;
ALTER TABLE sessions ADD COLUMN output_tokens INTEGER;
ALTER TABLE sessions ADD COLUMN cache_read_tokens INTEGER;
ALTER TABLE sessions ADD COLUMN cache_write_tokens INTEGER;
ALTER TABLE sessions ADD COLUMN reasoning_tokens INTEGER;
PRAGMA user_version = 3;
```

`NULL` in all 5 columns = no token data (maps to `session.token_usage = None`).
A session with `input_tokens IS NOT NULL` is considered to have token data.

### Schema migration (`src/database/schema.rs`)

Add `apply_v3_migration()` and call it from `initialize_database()` after the v2 check.
The migration is idempotent: "duplicate column name" errors from `ALTER TABLE` are silently ignored.

### Read path (`src/database/mod.rs`)

Update `session_from_row()` to read the 5 new columns (indices 10–14) and construct
`TokenUsage` when `input_tokens IS NOT NULL`.

Update all SELECT queries that return session rows to include the 5 new columns:
- `load_sessions`
- `search_sessions_with_query`
- `load_session`

### Write path (`src/database/indexer.rs`)

Update `insert_parsed_session()` to include the 5 token columns in `INSERT OR REPLACE INTO sessions`.

---

## Section 3 — Parser Changes

Each parser computes session-level token totals during its parse pass and stores them in
`ParsedSession`. The `ParsedSession` struct gains `token_usage: Option<TokenUsage>`.

### Claude Code (`src/parsers/claude_code.rs`)

**Source:** `message.usage` on `type == "assistant"` events.

**Fields:**
- `input_tokens` ← sum of `message.usage.input_tokens`
- `output_tokens` ← sum of `message.usage.output_tokens`
- `cache_read_tokens` ← sum of `message.usage.cache_read_input_tokens`
- `cache_write_tokens` ← sum of `message.usage.cache_creation_input_tokens`
- `reasoning_tokens` ← `None` (not in Claude Code format)

**Deduplication:** Claude Code logs are append-only and can contain multiple assistant events
for the same underlying request. Deduplicate by the compound key `(requestId, message.id)`,
keeping only the last seen `usage` record per key before summing.

**Result:** `Some(TokenUsage)` if at least one `usage` block was found; `None` otherwise.

### Codex (`src/parsers/codex.rs`)

**Source:** `event_msg` entries with `payload.type == "token_count"`.

**Strategy:** `total_token_usage` is a running session total emitted at each turn.
Take the **last non-null** `info.total_token_usage` seen in the file.

**Fields:**
- `input_tokens` ← `info.total_token_usage.input_tokens`
- `output_tokens` ← `info.total_token_usage.output_tokens`
- `cache_read_tokens` ← `info.total_token_usage.cached_input_tokens`
- `cache_write_tokens` ← `None` (not in Codex format)
- `reasoning_tokens` ← `info.total_token_usage.reasoning_output_tokens`

**Edge case:** `info: null` → treat as unknown, not zero. Skip that event.

**Result:** `Some(TokenUsage)` if any non-null `total_token_usage` was seen; `None` otherwise.

### OpenCode (`src/parsers/opencode/`)

**Source:** `message.data.tokens` on assistant messages (JSON blob in the `data` column).
Prefer message-level tokens. Do **not** additionally accumulate `part.type == "step-finish"` tokens
to avoid double-counting.

**Fields (from `message.data.tokens`):**
- `input_tokens` ← sum of `tokens.input` (or `tokens.prompt` in legacy)
- `output_tokens` ← sum of `tokens.output` (or `tokens.completion` in legacy)
- `cache_read_tokens` ← sum of `tokens.cache.read` (when present)
- `cache_write_tokens` ← sum of `tokens.cache.write` (when present)
- `reasoning_tokens` ← `None`

Both the JSON backend and the SQLite backend parse the same `message.data` blob,
so token extraction logic lives in shared message parsing code.

**Result:** `Some(TokenUsage)` if at least one message had token data; `None` otherwise.

### Mistral Vibe (`src/parsers/mistral_vibe.rs`)

**Source:** `meta.json.stats` (session-level only; `messages.jsonl` has no per-message tokens).

**Fields:**
- `input_tokens` ← `stats.session_prompt_tokens`
- `output_tokens` ← `stats.session_completion_tokens`
- `cache_read_tokens` ← `None` (not in Vibe format)
- `cache_write_tokens` ← `None`
- `reasoning_tokens` ← `None`

**Edge case:** `stats: null` or field missing → `None`.

**Result:** `Some(TokenUsage)` if both prompt and completion fields are present; `None` otherwise.

---

## Section 4 — UI (SessionDetail header card)

### Placement

Add a new `gtk::Box` row to the existing metadata card in `src/ui/session_detail.rs`,
below the tool/message-count/time row and above the Session ID row.
Controlled by `set_visible` based on whether `session.token_usage.is_some()`.

### Display format

**Minimum (input + output only):**
```
Tokens: 12 345 input · 678 output
```

**With cache:**
```
Tokens: 12 345 input · 678 output · 9 012 cache read · 234 cache write
```

**With reasoning (Codex):**
```
Tokens: 12 345 input · 678 output · 456 reasoning · 9 012 cache read
```

Numbers are formatted with thousands separators (locale-aware via a helper).

### Widget structure

```
gtk::Box [horizontal, spacing=6, halign=Start]
  gtk::Label "Tokens:"        [dim-label]
  #[name="token_usage_label"]
  gtk::Label "<formatted>"    [dim-label]
```

The label text is built in `post_view()` from `session.token_usage`.

### Hiding when unavailable

```rust
#[watch]
set_visible: model.session.as_ref()
    .and_then(|s| s.token_usage.as_ref())
    .is_some(),
```

No "N/A" placeholder — the entire row is hidden.

---

## Section 5 — Error Handling & Edge Cases

| Case | Behavior |
|------|----------|
| No token data in file | `token_usage = None`, row hidden |
| Partial token data (e.g. only input) | Store what is available; display partial |
| `info: null` in Codex | Skip that `token_count` event |
| OpenCode step-finish tokens present | Ignored; message-level preferred |
| Claude duplicate assistant events | Deduplicate by `(requestId, message.id)` before summing |
| Very large token numbers | `i64` is sufficient (max ~9.2 × 10¹⁸) |

---

## Section 6 — Testing

### Unit tests

- `TokenUsage` construction and field access
- Parser tests for each tool using existing fixtures:
  - Claude Code: verify deduplication and sum correctness
  - Codex: verify last-non-null `total_token_usage` is used
  - OpenCode: verify message-level accumulation, no double-counting
  - Mistral Vibe: verify `stats` extraction and null handling

### Integration tests (`tests/`)

- After indexing fixture sessions, verify `sessions.input_tokens` is populated where expected
- Verify `sessions.input_tokens IS NULL` for sessions without token data

### Schema migration test

- Verify fresh DB initializes at v3
- Verify v2 → v3 migration adds the 5 new columns without data loss

### Fixture coverage

Existing fixtures may not contain token usage fields.
Add or extend fixtures with token data for at least Claude Code and Codex.

---

## Non-Goals

- No per-message token breakdown (future feature)
- No cost estimation (future feature)
- No sorting/filtering sessions by token count (future feature)
- No display in the session list row (only `SessionDetail` header)

---

## Files Changed

| File | Change |
|------|--------|
| `src/models/token_usage.rs` | New file: `TokenUsage` struct |
| `src/models/mod.rs` | Export `TokenUsage`; add `token_usage` to `Session` |
| `src/models/session.rs` | Add `token_usage: Option<TokenUsage>` |
| `src/database/schema.rs` | Add `apply_v3_migration()`, update `initialize_database()` |
| `src/database/mod.rs` | Update `session_from_row()` and all SELECT queries |
| `src/database/indexer.rs` | Update `insert_parsed_session()` |
| `src/parsers/mod.rs` | Add `token_usage: Option<TokenUsage>` to `ParsedSession` |
| `src/parsers/claude_code.rs` | Extract and deduplicate `message.usage` |
| `src/parsers/codex.rs` | Extract last-non-null `total_token_usage` |
| `src/parsers/opencode/` | Extract message-level `data.tokens` |
| `src/parsers/mistral_vibe.rs` | Extract `meta.json.stats` |
| `src/ui/session_detail.rs` | Add `token_usage_label` widget + `post_view()` logic |
| `tests/` | New/extended integration tests |
| `tests/fixtures/` | Extend fixtures with token data |

---

**Last Updated:** 2026-02-28
