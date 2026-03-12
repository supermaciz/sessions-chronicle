# Session Title Command Cleanup — Design

Strip XML command tags from Claude Code session titles in the session list,
replacing raw `<command-name>`, `<command-message>`, and `<command-args>` markup
with clean `/command args` text.

---

## Problem

Claude Code encodes slash commands as XML-like tags in session JSONL data.
When a command is the first user message, `extract_first_prompt()` stores the
raw tags as the session title.  Result:

```
<command-message>brainstorming</command-message>
<command-name>/brainstorming</command-name>
```

instead of `/brainstorming`.  This affects **~111 real sessions** across 28
unique commands.

## Scope

- **In scope:** session list title cleanup for Claude Code sessions.
- **Out of scope:** transcript detail view, new UI widgets, command row design,
  database schema changes.

## Approach

Add a `strip_command_tags()` function in `src/parsers/mod.rs`, called by
`normalize_prompt()` before whitespace collapsing.  The parser layer is the
right place because:

- Clean data flows to DB, search, and all UI views automatically.
- Raw JSONL is always available if the original content is ever needed.
- Existing sessions get clean titles on next re-index (no migration).

## Tag Format

Tags appear in user messages (`type: "user"`) and system events
(`type: "system"`, `subtype: "local_command"`).  The format is rigid:

| Tag | Content |
|-----|---------|
| `<command-name>` | Slash command with `/` prefix (e.g. `/brainstorming`) |
| `<command-message>` | Command name without `/` prefix |
| `<command-args>` | Arguments (may be empty or multiline) |

Two layout patterns exist in the wild:

```
<command-message>brainstorming</command-message>
<command-name>/brainstorming</command-name>
```

```
<command-name>/model</command-name>
              <command-message>model</command-message>
              <command-args></command-args>
```

## Algorithm

1. **Fast path:** if content does not contain `<command-name>`, return as-is.
2. Extract the command name from `<command-name>...</command-name>`.
3. Extract args from `<command-args>...</command-args>` if present and
   non-empty.
4. Strip all known command tags (opening and closing) from the content.
5. Collect any remaining non-whitespace text as "trailing user text".
6. Build the clean title:
   - Command only: `/brainstorming`
   - Command + args: `/learn-rust PATH B`
   - Command + trailing text: `/review — fix the auth bug`
   - Command + args + trailing text: `/review #36 — fix the auth bug`

## Integration Point

```
normalize_prompt(content)
  → strip_command_tags(content)   // NEW
  → whitespace collapsing         // existing
  → truncate at 200 chars         // existing
```

`strip_command_tags()` runs first because whitespace collapsing would merge tag
boundaries and make regex matching unreliable.

### Data Flow

1. JSONL parser extracts raw `message.content` (unchanged).
2. `extract_first_prompt()` calls `normalize_prompt()`.
3. `normalize_prompt()` calls `strip_command_tags()` first.
4. Clean title stored in `Session::first_prompt`.
5. DB stores clean title.
6. UI displays clean title (Pango escaping still applies).

### No Changes To

- Database schema
- UI code (`session_row.rs`)
- Other parsers (OpenCode, Codex, Mistral Vibe)

## Parsing Approach

Simple regex — no full XML parser needed.  The tag format is rigid and
predictable.  A single regex like
`<(command-name|command-message|command-args)>(.*?)</\1>` captures all three
tags in one pass (with `(?s)` for multiline args).

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Which tag for the name | `<command-name>` | Has the `/` prefix, recognizable CLI convention |
| Long qualified names | Full name as-is | `/superpowers-extended-cc:brainstorming` — truthful, truncation handles length |
| Command + trailing text | `/command args — text` | Shows both command and user intent |

## Examples

| Raw `first_prompt` | Clean title |
|-----|------|
| `<command-message>brainstorming</command-message> <command-name>/brainstorming</command-name>` | `/brainstorming` |
| `<command-name>/learn-rust</command-name> <command-message>learn-rust</command-message> <command-args>PATH B</command-args>` | `/learn-rust PATH B` |
| `<command-message>review</command-message> <command-name>/review</command-name> <command-args>#36</command-args>` | `/review #36` |
| `<command-name>/model</command-name> <command-message>model</command-message> <command-args></command-args>` | `/model` |
| `<command-message>review</command-message> <command-name>/review</command-name> fix the auth bug` | `/review — fix the auth bug` |

## Test Plan

Unit tests in `src/parsers/mod.rs`:

1. **Command-only message** → `/brainstorming`
2. **Command with args** → `/learn-rust PATH B`
3. **Command with empty args** → `/model` (no trailing space)
4. **Command with trailing user text** → `/review — fix the auth bug`
5. **Varying whitespace** — leading spaces, mixed indentation → parses correctly
6. **No command tags** — normal message passes through unchanged
7. **Partial tags** — `<command-name>` with no closing tag → passes through unchanged
8. **Long qualified names** — `/superpowers-extended-cc:brainstorming` preserved

**Existing test update:** `session_title_escapes_markup_special_chars` in
`session_row.rs` uses `<command-message>review</command-message> & fix` as
input.  After the fix, `first_prompt` would already be clean, so this test
should be updated to reflect that titles no longer contain raw tags.
