# Claude Code Teammate Support — Design Spec

**Date:** 2026-07-27  
**Status:** Implemented by [PR #194](https://github.com/supermaciz/sessions-chronicle/pull/194) — see [Implementation](#implementation) for what shipped and where it diverged  
**Format notes:** [`docs/session-formats/claude-code.md`](../../session-formats/claude-code.md) (updated by commit `393a589`)

## Goal

Restore parent→child subagent navigation for Claude Code sessions produced by
v2.1.216+, and adopt the new `ai-title` event as the session label.

Claude Code changed how subagents are spawned: since the changelog entry for
v2.1.198 they run in the background as "teammates", and the token that
Sessions Chronicle relies on to link a parent transcript to its nested child
transcript no longer exists. On every session recorded with a recent Claude
Code, the subagent rows in the inspector have no child to open.

## What broke

Linkage is a join on `agent_id`, populated from two independent sides.

| Side | Source | Legacy value | v2.1.216+ value |
|---|---|---|---|
| Child | file stem `agent-<id>.jsonl` (`src/parsers/claude_code.rs:38`) | `a41c0fb07beb52ed6` | `aimpl-task1-d4584135445167d0` |
| Parent | `agentId:` token scraped from `tool_result` text (`src/parsers/claude_code.rs:64`) | `a41c0fb07beb52ed6` | *absent* |

The child side still works. Verified against local session
`66ae4ab6-e5ea-40f4-8e8f-fb80fd307472` (v2.1.220): every event in a child
transcript carries `"agentId": "aimpl-task1-d4584135445167d0"`, identical to
its file stem.

The parent side is the whole of the regression. `extract_agent_id_from_result_text`
only recognises `agentId:`, which no longer appears. What the parent transcript
does carry, for the same spawn:

| Location | Value |
|---|---|
| `tool_use` `input.name` | `impl-task1` |
| `toolUseResult.name` | `impl-task1` |
| `toolUseResult.agent_id` | `impl-task1@session-66ae4ab6` |
| `tool_result` text, `name:` line | `impl-task1` |
| sidecar `.meta.json`, `name` | `impl-task1` |

The 16-hex suffix in the child filename appears in **no** parent-side field and
in **no** sidecar field. `agent_id` (`<name>@session-<shortid>`) appears in no
child-side field. **`name` is the only value common to both sides**, so the join
can no longer be a string equality between stored ids.

## Non-goals

- Teammate metadata (`model`, `subagent_type`, `status`, `team_name`, `color`)
  is not surfaced in the UI. The sidecar and `toolUseResult` carry it; exposing
  it is a separate scope.
- `mode`, `file-history-delta`, `effort`, `requestId` and the snake_case
  `session_id` duplicate need no work. The parser dispatches only on
  `type in {user, assistant}`, so these fall through and are skipped without
  error.
- No new `Session.title` field. See *ai-title* below.

## Architecture

Three units, each changeable without touching the others.

### 1. Extraction — `src/parsers/claude_code.rs`

`Subagent` gains `agent_name: Option<String>` alongside the existing
`agent_id`. The parser reports whichever the transcript contains, possibly
neither. It knows nothing about the filesystem or the schema.

Two write sites:

- **At `tool_use`** (`src/parsers/claude_code.rs:578`): capture `input.name`.
- **At `tool_result`** (`src/parsers/claude_code.rs:386`): keep the existing
  `extract_agent_id_from_result_text` call for the legacy form; additionally
  fill `agent_name` from `event["toolUseResult"]["name"]` if it is still empty.
  `record_tool_result` already receives the full event (`src/parsers/claude_code.rs:345`),
  so this reads a structured field rather than scraping text.

Capturing at `tool_use` time is what makes linkage work on interrupted
sessions, where the spawn was recorded but the result never arrived.

The `tool_result` text is **not** parsed for `name:`. The structured
`toolUseResult` and `input.name` cover every observed case, and the text block
is explicitly marked internal metadata by Claude Code itself.

### 2. Resolution — `src/database/indexer.rs`

One pure function, testable in isolation, used by both indexing directions:

```rust
/// Splits a nested transcript's agent id into (full id, optional teammate name).
///
/// `"aimpl-task1-d4584135445167d0"` -> `("aimpl-task1-d4584135445167d0", Some("impl-task1"))`
/// `"a41c0fb07beb52ed6"`            -> `("a41c0fb07beb52ed6", None)`
fn child_agent_key(agent_id: &str) -> (&str, Option<&str>)
```

Rule: strip the leading `a`, then strip a trailing `-` followed by exactly 16
ASCII hex digits. If what remains is empty, there is no name — that is the
legacy form. Discrimination is mechanical, not heuristic: `a41c0fb07beb52ed6`
is 17 characters, so `a` + 16 hex leaves nothing.

A name that itself ends in `-<16 hex>` would be truncated. No such name has
been observed; teammate names are short slugs supplied at spawn time. The
failure mode is a missing link, not a wrong one.

### 3. Persistence — `src/database/schema.rs`

Migration **v16** (`CURRENT_DB_VERSION` is currently 15, `src/database/schema.rs:5`):

> **16** – `subagents` gains a nullable `agent_name` for Claude Code teammate
> linkage (v2.1.216+ dropped the `agentId:` token); clears `file_fingerprints`
> so parents are re-parsed and the new column is populated.

Clearing `file_fingerprints` is required, not cosmetic: teammate sessions
already indexed hold `subagents` rows with a null `agent_id`, and adding a
column does not repair them. This follows v5, v10, v11, v13 and v14; v12 and
v15 skip the clear because they only add indexes.

Also adds `idx_subagents_agent_name ON subagents(session_id, agent_name)`,
mirroring the existing `idx_subagents_agent`.

## Data flow

Indexing guarantees no ordering between a parent and its children, so both
directions must work. This is already the shape of
`link_claude_subagents_tx` (`src/database/indexer.rs:978`) and it is preserved.

### Child indexed, parent already in the database

The suffix of `session.id` goes through `child_agent_key`.

For the **legacy form** (no name), the existing exact-match statement is kept
unchanged — `agent_id` is unique by construction:

```sql
UPDATE subagents SET child_session_id = ?1
 WHERE session_id = ?2 AND agent_id = ?3
```

For the **teammate form**, a bare `UPDATE ... WHERE agent_name = ?` would
violate the uniqueness rule below in two ways: two homonymous parent rows would
both be linked to this child, and two homonymous children would each overwrite
the other's link. So resolution is explicit:

1. `SELECT id FROM subagents WHERE session_id = ?1 AND agent_name = ?2` —
   abort if this returns anything other than exactly one row (duplicate parent
   rows).
2. `SELECT id FROM sessions WHERE parent_session_id = ?1` — apply
   `child_agent_key` to each and abort if more than one child resolves to this
   name (duplicate children).
3. `UPDATE subagents SET child_session_id = ?1 WHERE id = ?2` on the single
   resolved row.

### Parent indexed, children already in the database

The parent cannot construct a child id — it does not know the hash. It
enumerates instead:

```sql
SELECT id FROM sessions WHERE parent_session_id = ?1
```

then applies `child_agent_key` to each id in Rust and pairs by name, linking
only names that resolve to exactly one child *and* to exactly one parent
`subagents` row. This replaces the id construction at
`src/database/indexer.rs:1014` and the `SELECT EXISTS` that followed it.

Enumeration is chosen over a `LIKE 'claude-subagent::<parent>::a<name>-%'`
prefix query for a concrete reason: `input.name` comes from a session file,
which `AGENTS.md` requires be treated as untrusted, and a `%` or `_` inside a
name would match unrelated children. Enumeration removes the need for `LIKE`
escaping and makes both directions share the same pure function. The volume is
negligible — the real `66ae4ab6` session has 13 children.

Legacy subagents keep the existing construction path, since for them the parent
does know the full child id.

## Error handling

**Duplicate names.** Nothing prevents two spawns named `reviewer` in one
session, yielding `areviewer-<hash1>` and `areviewer-<hash2>`, which the name
alone cannot tell apart. **Link only when the name resolves to exactly one
child session and exactly one parent `subagents` row**; otherwise leave the row
unlinked and emit `tracing::warn!`. Both indexing directions enforce this, so
the outcome does not depend on which side is indexed first. An unlinked
subagent is exactly today's behaviour, so this is not a regression; a
mislinked one would open the wrong transcript. Chronological pairing was
considered and rejected — it recovers these cases at the cost of a silent
wrong link.

**Neither key present.** Skip, as today.

**Legacy sessions.** `extract_agent_id_from_result_text` is kept unchanged,
validation warning included.

**Malformed child filenames.** Unchanged: a file under `subagents/` whose stem
yields no valid agent id is already rejected (`src/parsers/claude_code.rs:648`).

## ai-title

`ai-title` carries the AI-generated session title and, in the 20 most recent
local sessions, has replaced `summary` entirely.

It feeds the existing `first_prompt` field, following the precedent already set
by OpenCode (`src/parsers/opencode/mod.rs:294`):

```rust
let first_prompt = match &self.ai_title {
    Some(title) if !title.trim().is_empty() => Some(title.clone()),
    _ => first_prompt::extract_first_prompt(&self.messages),
};
```

`first_prompt` is in practice a session label rather than a literal first
prompt — OpenCode already stores a generated title there, and both
`src/ui/session_row.rs:165` and `src/ui/session_detail/session_summary.rs:372`
render it as such. Reusing it means no schema change, no UI change, and search
coverage for free.

A session may emit several `ai-title` events as the title is regenerated; the
last non-blank one wins. This requires adding `ai-title` to the parser's event
dispatch, which currently handles only `user` and `assistant`.

Introducing a dedicated `Session.title` was considered and deferred: it would
require a schema migration, reworking `session_row` and `session_summary`,
moving OpenCode's `metadata.title`, and indexing a new FTS column — all outside
a Claude Code support update.

## Testing

**Fixture.** A new `tests/fixtures/claude_teammate_linkage/` mirroring the
shape of `tests/fixtures/claude_subagent_linkage/` (3 + 2 lines, hand-written).
It must contain a parent with a `tool_use` carrying `input.name`, its
`tool_result` user event carrying a structured `toolUseResult`, and a child at
`<parent>/subagents/agent-a<name>-<16 hex>.jsonl`. Hand-written rather than
copied from a real session: real transcripts carry absolute paths and project
content, and the existing fixture is already minimal.

**Unit — `child_agent_key`.** Teammate form; legacy form; a name containing
dashes (`rereview-task3-r1`, observed); a stem that is only `a`; a suffix of 15
or 17 hex digits; a non-hex suffix.

**Unit — parser.** `agent_name` populated from `input.name`; populated from
`toolUseResult.name` when `input.name` is absent; legacy `agentId:` still
populates `agent_id` and leaves `agent_name` null; `ai-title` overrides the
extracted first prompt; blank `ai-title` falls back.

**Integration — linkage.** Extend the pattern of
`tests/claude_subagent_linkage.rs` to cover both indexing orders (child first,
parent first) on the teammate fixture, plus a duplicate-name case asserting
that neither row is linked.

**Regression.** `tests/claude_subagent_linkage.rs` must pass unmodified. That
is the guarantee legacy sessions still link.

**Manual.** Run against real data and open a subagent from a v2.1.220 session:

```
flatpak-builder --run flatpak_app build-aux/dev.maciz.sessionschronicle.Devel.json sessions-chronicle
```

## Definition of done

- `cargo fmt --all -- --check` passes.
- `cargo clippy --all -- -D warnings` passes.
- `cargo test --all --no-fail-fast` passes.
- Parent→child navigation verified manually on a real v2.1.216+ session.
- `docs/session-formats/claude-code.md` known-gap entry removed and the
  implemented linkage described.

---

## Implementation

Shipped in [PR #194](https://github.com/supermaciz/sessions-chronicle/pull/194),
10 commits on `feat/claude-teammate-linkage` from `96a57f1`:

| Commit | Scope |
|---|---|
| `1059671` | `subagents.agent_name` column, migration v16 |
| `fb6cc24` | Parser captures the teammate name |
| `2a122c6` | Pure agent-id splitter |
| `2bdde6b` | Linker rewritten for both indexing orders |
| `4fede08` | Fixtures and integration tests |
| `e145cc9` | `ai-title` as the session label |
| `18c1bba` | Format notes updated |
| `6bb0ca0` `c0bfe48` `5313205` | Post-review fix wave |

Everything above the fix wave matches this spec. What follows is where the
shipped code diverges from it, and why.

### 1. The ambiguity guard had to retract, not just refuse

**This is the spec's own defect, not an implementation slip.** *Error handling*
above says an ambiguous name must leave the row unlinked, and the *Data flow*
resolution steps enforce that — but only for a link not yet made. They never
say what to do about a link already recorded.

That gap is reachable. Two same-named children indexed in one pass: child B is
processed first, finds one sibling (itself), and links. Child A is processed
next, finds two siblings, correctly refuses to link *itself* — and leaves B's
link standing. The parent's single subagent row ends up pointing at an
arbitrary one of the two children, which is exactly the mislink this design
set out to prevent. It occurs in real use when a parent is indexed in one pass
and two same-named teammate transcripts in a later incremental one.

`link_teammate_child_tx` therefore retracts on detection, scoped to the parent
and the name, before returning:

```sql
UPDATE subagents SET child_session_id = NULL
 WHERE session_id = ?1 AND agent_name = ?2
```

The warning distinguishes the two cases using the affected-row count, so a
retraction is only reported when one happened.

No symmetric hole exists in the parent-indexed direction: re-indexing a parent
runs `delete_session_contents_tx`, which deletes its `subagents` rows before
reinserting them, so those rows are always recreated with a null
`child_session_id`.

### 2. `child_agent_key` became `teammate_name`

```text
spec:     fn child_agent_key(agent_id: &str) -> (&str, Option<&str>)
shipped:  fn teammate_name(agent_id: &str)   -> Option<&str>
```

Every return path returned its own argument as `.0`, so that half carried no
information — one call site ignored it and the other bound it to a new name
for the value it already had.

The extraction logic is unchanged: same `strip_prefix('a')`, same
`rsplit_once('-')`, same exactly-16-ASCII-hex check, same empty-name rejection.

### 3. `ai-title` is normalized, not merely trimmed

The *ai-title* section's sketch only trimmed. `extract_first_prompt` runs every
fallback label through `normalize_prompt`, which collapses whitespace and
truncates at `FIRST_PROMPT_MAX_CHARS`; the sketch bypassed it, so an
arbitrarily long or newline-containing title from an untrusted session file
would have reached a GTK label verbatim. Since `ai-title` is present on most
recent sessions, that widened an existing narrow exposure to nearly all Claude
Code sessions. Shipped as `Some(crate::parsers::normalize_prompt(title))`.

### 4. A third fixture, for the guard the test plan missed

*Testing* above specifies a duplicate-name fixture, but its ambiguity is on the
parent side — two subagent rows named `reviewer`. Such a case returns early on
`parent_rows.len() != 1` and never reaches the sibling check, so **the
child-side guard had no coverage at all**. That is how defect 1 survived
review.

`tests/fixtures/claude_teammate_child_duplicate/` fills the gap: one parent
declaring exactly one teammate named `solo`, plus an unambiguous `helper`
sibling, and two child transcripts `agent-asolo-aaaa…` / `agent-asolo-bbbb…`.
Reaching `link_teammate_child_tx` at all requires indexing the parent alone
first and then the children alone, since a mixed directory pass routes through
the parent loop's separate guard instead. `tests/claude_teammate_linkage.rs`
holds 7 tests, covering all three orderings and asserting the retraction leaves
`helper` untouched.

### 5. An undocumented transactional invariant

The sibling-count guard is correct only because `link_claude_subagents_tx` runs
*after* `upsert_session_row_tx` in the same transaction, so the child being
indexed is already in `sessions` and `siblings.len() > 1` means precisely "a
duplicate exists". Reversing the two calls produces mislinks rather than
missing rows. The spec never stated this. There is now a comment at the call
site, and hoisting the call fails four tests with a real mislink assertion.

### 6. "Search coverage for free" was wrong

The *ai-title* section claims reusing `first_prompt` brings search coverage.
It does not: search runs against `messages_fts` only, and `first_prompt` is in
no FTS index and no `WHERE` clause. The reuse is still right for the reasons
that actually hold — no migration, no UI change, and consistency with the
OpenCode precedent — but a model-generated title may name no message, so a
label visible in the list can now be unfindable by search. Tracked as debt
alongside the "FIRST PROMPT" heading, both belonging with a future
`Session.title` field.

### 7. Verification method

*Definition of done* asks for manual GUI verification. That was done, and it
passed. It was preceded by a stronger check the spec did not ask for: indexing
the real session directory (84 sessions) into a temporary database and counting
links — **52/52 subagent rows linked, 13/13 for a 13-teammate session**, against
zero before the branch.

Tests were also required to fail before being accepted. Breaking only
`input.name` fails 1 of 4, since the `toolUseResult` fallback rescues the rest —
itself evidence that the fallback path is covered. Breaking both routes fails
all 4.

The test command changed during this work, for reasons unrelated to the
feature: the suite's ~175 `#[gtk::test]` cases open real windows and steal
keyboard focus, and `xvfb-run` alone is insufficient on Wayland because GTK 4
prefers the live compositor over the Xvfb `DISPLAY`. See `AGENTS.md`.
