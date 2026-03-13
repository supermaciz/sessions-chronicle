# Session Title Command Cleanup — Design

**Status:** Implemented [#78](https://github.com/supermaciz/sessions-chronicle/pull/78)

Strip Claude Code command tags from session titles, replacing raw
`<command-name>`, `<command-message>`, and `<command-args>` markup with clean
`/command args` text.

---

## Problem

Claude Code encodes slash commands as XML-like tags in session JSONL data.
When the first user message is a command invocation, `extract_first_prompt()`
currently stores the raw markup as the session title. Result:

```
<command-message>brainstorming</command-message>
<command-name>/brainstorming</command-name>
```

instead of `/brainstorming`.

This affects roughly 111 real sessions across 28 unique commands and makes the
session list harder to scan.

## Scope

- **In scope:** Claude Code session title cleanup for `Session::first_prompt`.
- **Out of scope:** transcript detail rendering, new UI widgets, database schema
  changes, search schema changes, non-Claude parsers.

## Context

`extract_first_prompt()` only considers `Role::User` messages. Claude Code may
also emit command metadata in other event types, but those do not participate in
this title extraction flow and are out of scope for this cleanup.

The relevant flow today is:

```text
extract_first_prompt(messages)
  -> first user message content
  -> normalize_prompt(content)
  -> collapse whitespace
  -> truncate to 200 chars
```

## Recommended Approach

Add a `strip_command_tags()` helper in `src/parsers/mod.rs` and call it from
`normalize_prompt()` before whitespace collapsing.

The parser layer is the right place because:

- clean data flows into DB storage and all UI views automatically;
- the original raw transcript content remains available in indexed messages;
- existing sessions are fixed by re-indexing, with no migration;
- `src/ui/session_row.rs` stays focused on display concerns such as Pango
  escaping.

## Supported Tag Shape

The cleanup only targets fully matched Claude Code command blocks in user
message content.

| Tag | Meaning |
|-----|---------|
| `<command-name>` | Slash command with `/` prefix, for example `/brainstorming` |
| `<command-message>` | Command name without the `/` prefix |
| `<command-args>` | Optional arguments, possibly empty or multiline |

Observed examples:

```text
<command-message>brainstorming</command-message>
<command-name>/brainstorming</command-name>
```

```text
<command-name>/model</command-name>
<command-message>model</command-message>
<command-args></command-args>
```

## Confidence Rule

Cleanup is applied only when the parser can identify a single complete
`<command-name>...</command-name>` block without ambiguity.

If the content is malformed or ambiguous, the existing `normalize_prompt()`
behavior remains unchanged. Title cleanup is display-oriented and must never
silently guess.

## Algorithm

1. **Fast path:** if the content does not contain `<command-name>`, return it
   unchanged.
2. Search for complete command-tag blocks and their spans.
3. Require exactly one complete `<command-name>...</command-name>` block.
   - If none are found, return unchanged.
   - If multiple are found, treat the input as ambiguous and return unchanged.
4. Extract the exact command name from that block.
5. Extract args from a complete `<command-args>...</command-args>` block only if
   one is present and its trimmed content is non-empty.
6. Remove the fully matched blocks already consumed by parsing:
   - `<command-name>...</command-name>`
   - `<command-message>...</command-message>`
   - `<command-args>...</command-args>`
7. Inspect the remaining text.
   - If unmatched command-tag fragments still remain, treat the input as
     malformed and return unchanged.
   - Otherwise trim it, collapse internal whitespace, and treat any non-empty
     content as trailing user text.
8. Rebuild the clean title from structured parts:
   - command only -> `/brainstorming`
   - command + args -> `/learn-rust PATH B`
   - command + trailing text -> `/review — fix the auth bug`
   - command + args + trailing text -> `/review #36 — fix the auth bug`

This avoids the main failure mode in the original draft: command text from
`<command-message>` or `<command-args>` being counted twice when deriving the
residual text.

## Integration Point

```text
normalize_prompt(content)
  -> strip_command_tags(content)   // new
  -> collapse whitespace           // existing
  -> truncate at 200 chars         // existing
```

`strip_command_tags()` must run before whitespace collapsing. Once whitespace is
flattened, reliable tag-boundary detection becomes harder and malformed inputs
are more difficult to distinguish from valid ones.

## Data Flow

1. The Claude Code parser keeps raw `message.content` unchanged.
2. `extract_first_prompt()` selects the first non-empty `Role::User` message.
3. `normalize_prompt()` tries structured command cleanup first.
4. The cleaned title is stored in `Session::first_prompt`.
5. The database stores the cleaned title.
6. The UI displays the cleaned title, and `session_row.rs` still performs Pango
   escaping.

## Parsing Strategy

Use simple regex-based extraction for fully matched known blocks, then rebuild
the title deterministically from:

- the exact `<command-name>` value;
- optional `<command-args>` content;
- untouched residual text outside consumed blocks.

No XML parser is needed because the supported structure is rigid and limited,
but the implementation should track full-match spans rather than merely removing
opening and closing tags.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Canonical command source | `<command-name>` | It already contains the slash-prefixed CLI form |
| Qualified command names | Preserve exact value | `/superpowers-extended-cc:brainstorming` is truthful and deterministic |
| Ambiguous multiple command blocks | Fallback unchanged | Safer than guessing which command is canonical |
| Trailing text separator | ` — ` | Keeps command/args readable while preserving extra user intent |

## No Changes To

- database schema;
- `src/ui/session_row.rs` behavior beyond existing title escaping;
- OpenCode, Codex, or Mistral Vibe title extraction;
- transcript detail content.

## Examples

| Raw `first_prompt` | Clean title |
|-----|------|
| `<command-message>brainstorming</command-message> <command-name>/brainstorming</command-name>` | `/brainstorming` |
| `<command-name>/learn-rust</command-name> <command-message>learn-rust</command-message> <command-args>PATH B</command-args>` | `/learn-rust PATH B` |
| `<command-message>review</command-message> <command-name>/review</command-name> <command-args>#36</command-args>` | `/review #36` |
| `<command-name>/model</command-name> <command-message>model</command-message> <command-args></command-args>` | `/model` |
| `<command-message>review</command-message> <command-name>/review</command-name> fix the auth bug` | `/review — fix the auth bug` |
| `<command-name>/superpowers-extended-cc:brainstorming</command-name>` | `/superpowers-extended-cc:brainstorming` |
| `<command-name>/review</command-name> <command-name>/model</command-name>` | unchanged fallback |

## Failure Handling

If cleanup cannot be applied with high confidence:

- indexing continues normally;
- `first_prompt` falls back to the existing normalized raw text;
- no session disappears from the list;
- no transcript data is discarded or rewritten.

## Test Plan

Add unit tests in `src/parsers/mod.rs` for:

1. **Command-only message** -> `/brainstorming`
2. **Command with args** -> `/learn-rust PATH B`
3. **Command with empty args** -> `/model`
4. **Command with trailing user text** -> `/review — fix the auth bug`
5. **Whitespace variation** -> leading spaces, indentation, and multiline args
   still parse correctly
6. **No command tags** -> message passes through unchanged
7. **Partial `<command-name>` tag** -> unchanged fallback
8. **Partial `<command-args>` tag** -> unchanged fallback
9. **Multiple complete `<command-name>` blocks** -> unchanged fallback
10. **Long qualified command name** -> exact value preserved

Update the existing UI test in `src/ui/session_row.rs` so it verifies markup
escaping for an already normalized title, for example `/review & fix`, instead
of depending on raw command-tag markup reaching the row layer.
