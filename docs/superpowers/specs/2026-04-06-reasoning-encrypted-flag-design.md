# Reasoning Encrypted Presence Design

**Status:** Implemented [#117](https://github.com/supermaciz/sessions-chronicle/pull/117)

## Problem

The current reasoning attachment model stores opaque encrypted payloads in
`reasoning_attachments.encrypted_content`. That data is not rendered directly,
is not useful for search or inspection, and should not be persisted.

We still need to preserve one product behavior: transcript rows must show a
non-interactive `Thinking (encrypted)` pill when a session source reported
encrypted-only reasoning with no visible text or summary.

This branch has not shipped. The user will delete the existing development
database, so this branch should rewrite the existing v9 migration instead of
adding a new migration.

## Scope

This change applies to:

- the shared reasoning attachment model
- the v9 reasoning attachment schema on this branch
- Codex and OpenCode reasoning parsers
- Claude Code reasoning parsing for `thinking` blocks with empty `thinking` and
  a `signature`
- transcript-row and inspector behavior that distinguishes visible reasoning
  from encrypted-only reasoning

This change does not add any decryption, raw encrypted display, or persistence
of encrypted payloads.

## Decision

Replace stored encrypted payloads with a boolean presence flag.

### Data model

`ReasoningAttachment` will keep:

- `visible_text: Option<String>`
- `summary_text: Option<String>`
- `has_encrypted_content: bool`
- `source_model: Option<String>`
- `source_timestamp: Option<DateTime<Utc>>`

`encrypted_content: Option<String>` is removed entirely.

### Meaning

- `has_encrypted_content == true` means the source transcript indicated an
  encrypted reasoning payload existed.
- The application must never persist the opaque encrypted payload itself.
- `encrypted_only` remains a derived state meaning:
  - `has_encrypted_content == true`
  - `visible_text.is_none()`
  - `summary_text.is_none()`

## Source-specific parsing

### Claude Code

For `content[].type == "thinking"` blocks:

- non-empty `thinking` contributes to `visible_text`
- empty `thinking` is ignored for visible reasoning
- presence of a non-empty `signature` sets `has_encrypted_content = true`
- the `signature` value itself is never stored

Attachment rules stay the same: reasoning attaches to the next visible
transcript item emitted from the same assistant event, or becomes an orphan if
no visible item follows.

### OpenCode

For `part.type == "reasoning"`:

- visible reasoning still comes from `text`
- encrypted presence comes from
  `metadata.openai.reasoningEncryptedContent`
- that metadata only sets `has_encrypted_content = true`
- the encrypted string itself is discarded

### Codex

For `response_item.type == "reasoning"`:

- visible reasoning still comes from `content`
- summary reasoning still comes from `summary[]`
- encrypted presence comes from `encrypted_content`
- the encrypted string itself is discarded

## Database

The v9 migration in `src/database/schema.rs` is rewritten in place.

### `reasoning_attachments` table

Use:

```sql
CREATE TABLE IF NOT EXISTS reasoning_attachments (
    session_id TEXT NOT NULL,
    transcript_item_index INTEGER NOT NULL,
    visible_text TEXT,
    summary_text TEXT,
    has_encrypted_content INTEGER NOT NULL DEFAULT 0,
    source_model TEXT,
    source_timestamp INTEGER,
    PRIMARY KEY (session_id, transcript_item_index)
)
```

Do not create or reference an `encrypted_content` column anywhere on this
branch.

Because the development database will be deleted, no branch-local data
migration is required for already-created v9 databases.

## Derived preview behavior

Preview logic continues to expose:

- `has_reasoning`
- `has_visible_reasoning`
- `encrypted_only`

Derivation changes to:

- `has_reasoning = visible_text.is_some() || summary_text.is_some() || has_encrypted_content`
- `has_visible_reasoning = visible_text.is_some() || summary_text.is_some()`
- `encrypted_only = has_encrypted_content && visible_text.is_none() && summary_text.is_none()`

## UI behavior

Transcript rows keep the current presentation model:

- visible text or summary present: clickable `Thinking`
- encrypted-only: dimmed non-clickable `Thinking (encrypted)`
- mixed visible + encrypted: clickable `Thinking` only

The inspector must continue to show visible and summary reasoning only. It must
not attempt to show any encrypted payload because none is stored.

## Files expected to change

- `src/models/reasoning.rs`
- `src/database/schema.rs`
- `src/database/mod.rs`
- `src/parsers/claude_code.rs`
- `src/parsers/codex.rs`
- `src/parsers/opencode/mod.rs`
- any reasoning-related tests in parser, database, and UI modules

## Verification

Required automated checks:

- model tests cover `encrypted_only` using `has_encrypted_content`
- parser tests prove:
  - Claude `signature` sets encrypted presence without stored payload
  - Codex encrypted reasoning yields encrypted-only preview without payload
  - OpenCode encrypted reasoning yields encrypted-only preview without payload
- schema tests prove fresh DB initialization creates the boolean column and not
  the old text column
- DB round-trip tests prove `has_encrypted_content` persists and reloads
- transcript row tests still cover visible and encrypted-only pill states

Required repo verification before completion:

- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`

## Non-goals

- preserving encrypted blobs from earlier local development databases
- rendering raw encrypted data anywhere in the UI
- adding a new schema version solely for this branch-local cleanup
