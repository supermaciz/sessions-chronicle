# Parser Correctness Audit -- Current Format Drift Fixes

**Issue:** [#106 -- parser correctness audit](https://github.com/supermaciz/sessions-chronicle/issues/106)  
**Date:** 2026-03-31  
**Type:** Design plan  
**Status:** Draft  
**Related:** `docs/SESSION_FORMAT_ANALYSIS.md`, `docs/session-formats/claude-code.md`, `docs/session-formats/codex.md`, `docs/session-formats/opencode.md`, `docs/session-formats/mistral-vibe.md`

## Problem

Recent documentation refreshes for Claude Code, Codex, OpenCode, and Mistral
Vibe exposed five parser correctness regressions. These are not feature gaps; they
are silent compatibility failures against current or recently changed upstream
formats.

The affected behaviors are:

- OpenCode session token totals can be double-counted when both `message.tokens`
  and `step-finish.tokens` are present.
- Claude Code subagent launches are only recognized for the historical `Task`
  tool name, not the current `Agent` name.
- OpenCode source discovery only looks for `opencode.db`, which can silently skip
  non-default channel databases named `opencode-<channel>.db`.
- Mistral Vibe v2.7 changed the persisted system prompt placement, so the parser
  must not rely on any historical ordering assumption.
- Codex skill loading can inject raw `<skill>...</skill>` XML as a synthetic user
  message, which should not be shown as normal transcript prose.

## Scope

This design is intentionally narrow. It covers only the five regressions in issue
`#106`.

**In scope:**

- OpenCode token aggregation correctness
- Claude Code `Task` and `Agent` subagent compatibility
- OpenCode SQLite filename variant discovery
- Mistral Vibe v2.7 system prompt placement compatibility
- Codex injected skill XML transcript handling
- Targeted regression tests and fixture coverage for the above

**Out of scope:**

- parser architecture refactors
- a generalized anti-drift framework
- UI redesign beyond what is needed to avoid raw Codex skill XML display
- implementation planning in this document

---

## Recommended Approach

Apply one localized correction per regression at the layer that best understands
the data:

- parser logic for session-format semantics
- source resolution for on-disk discovery
- transcript rendering only when the issue is purely presentational

This is preferred over introducing a new compatibility abstraction because the
issue is about a small number of concrete, documented regressions. A narrow fix
keeps the parsers easier to reason about and avoids expanding the scope into a
broader refactor.

---

## 1. OpenCode -- Session Token Aggregation

### Decision

Session-level token totals for OpenCode must be aggregated exclusively from
`part.type == "step-finish"`.

`message.tokens` remains useful as observed source metadata, but it is not the
authoritative aggregation point for session totals.

### Rationale

The current OpenCode format can expose token information at multiple layers.
Using `message.tokens` and `step-finish.tokens` together inflates totals. The
issue description identifies `step-finish` as the only correct aggregation point
for session totals.

### Data flow

```text
OpenCode session
-> load messages
-> load parts
-> extract step-finish tokens
-> aggregate parsed session token_usage
```

### Rules

- Aggregate all valid `step-finish.tokens` entries in the session.
- Do not derive session totals from `message.tokens`.
- If no valid `step-finish.tokens` entries exist, prefer no session token total
  over a potentially wrong total.
- Malformed `step-finish` token payloads should be skipped with warning logs
  rather than failing the whole session.

---

## 2. Claude Code -- `Agent` and `Task` Subagent Compatibility

### Decision

Claude Code subagent detection must treat `tool_use.name == "Task"` and
`tool_use.name == "Agent"` as equivalent subagent-launch markers.

### Rationale

The upstream naming changed across versions. Users can have older and newer
sessions side by side. The parser should absorb that version drift directly
instead of depending on downstream UI logic to infer intent.

### Data flow

```text
assistant event
-> message.content[]
-> tool_use block
-> name is Task or Agent
-> create Subagent transcript item
```

### Rules

- Both names produce a `Subagent` record.
- Other tool names continue to produce ordinary `ToolCall` records.
- No fuzzy matching is introduced beyond the two documented aliases.
- Partial but identifiable subagent blocks should degrade gracefully rather than
  disappearing completely.

---

## 3. OpenCode -- SQLite Filename Variant Discovery

### Decision

OpenCode source resolution must discover both the default SQLite filename,
`opencode.db`, and non-default channel variants, `opencode-<channel>.db`.

### Rationale

The documented storage format is no longer single-database-only. Restricting
discovery to `opencode.db` causes silent data loss for users who run non-default
channels.

### Data flow

```text
resolved OpenCode root
-> enumerate documented database filename variants
-> keep existing files
-> pass valid databases to indexing
```

### Rules

- Treat every matching documented database as a valid source.
- Keep database-path discovery separate from session-level deduplication.
- Preserve the existing legacy JSON fallback when no SQLite database is found.
- Avoid hardcoding knowledge of a specific custom channel name.

---

## 4. Mistral Vibe -- System Prompt Placement Compatibility

### Decision

The parser must not rely on any fixed system-message position in
`messages.jsonl`. The system prompt should be treated according to the current
schema: session-level metadata in `meta.json` when present, not an ordinary
transcript message.

### Rationale

The v2.7 documentation corrected system prompt placement. Even if the current
implementation already behaves correctly, the design must explicitly lock in the
rule so compatibility is verified against the real schema rather than against a
historical ordering assumption.

### Data flow

```text
read meta.json
-> read messages.jsonl linearly
-> classify entries by role
-> index only persisted transcript messages that belong in the transcript
```

### Rules

- Do not infer transcript semantics from a message's position in the file.
- Treat `meta.json.system_prompt` as session metadata, not as a normal transcript
  row.
- Audit the existing parser for hidden ordering assumptions.
- If the audit finds no behavioral gap, the compatibility fix can be completed by
  adding explicit regression coverage rather than changing parser logic.

---

## 5. Codex -- Injected Skill XML Handling

### Decision

Codex `response_item` user messages whose payload is an injected
`<skill>...</skill>` block must not be rendered as ordinary transcript text.

The explicit user invocation, such as `$skill-name`, remains the meaningful
transcript event. The injected XML payload is an internal skill-loading artifact.

### Rationale

Showing raw XML in the transcript is noisy and misleading. The payload describes
mechanics of skill loading, not conversational content the user wrote.

### Data flow

```text
Codex rollout JSONL
-> user invocation event_msg kept as transcript message
-> injected response_item skill payload identified
-> payload filtered, neutralized, or converted to metadata
```

### Rules

- Keep explicit `$skill-name` user messages visible.
- Do not display raw `<skill>...</skill>` payloads as normal transcript prose.
- If a skill invocation exists without a following injected payload, preserve the
  visible invocation unchanged.
- Malformed-but-identifiable injected skill payloads should still be suppressed
  rather than displayed raw.

---

## Error Handling And Compatibility Rules

These regressions should follow the project's existing parser safety posture:

- log warnings and continue when individual records are malformed
- accept only variants explicitly documented in current format references
- prefer omission over false data when the source is ambiguous

Applied to this design, that means:

- no OpenCode token total if only ambiguous token sources are available
- no Claude subagent guessing beyond `Task` and `Agent`
- no assumption that a missing OpenCode SQLite default name means no SQLite data
- no positional assumptions for Mistral Vibe system content
- no raw Codex skill XML shown just because it is technically stored as message content

## Testing And Verification Expectations

This design should be implemented with targeted non-regression coverage for each
documented format drift.

### Unit and fixture coverage

- **OpenCode tokens:** a session containing both `message.tokens` and
  `step-finish.tokens`, with the expected total derived only from `step-finish`.
- **Claude Code subagents:** one fixture or test case for `Task`, one for
  `Agent`, both producing visible subagent records.
- **OpenCode DB discovery:** coverage for `opencode.db` and at least one
  `opencode-<channel>.db` variant.
- **Mistral Vibe v2.7:** regression coverage proving transcript extraction does
  not depend on historical system-message ordering.
- **Codex skills:** a rollout containing a `$skill-name` invocation followed by
  an injected `<skill>...</skill>` payload, with the expectation that the XML is
  not shown as a normal transcript message.

### Acceptance criteria

- OpenCode session token totals are no longer inflated by mixed token sources.
- Claude Code sessions from before and after the `Task` -> `Agent` rename both
  expose subagents correctly.
- OpenCode sessions stored in non-default channel databases are discoverable.
- Mistral Vibe v2.7 sessions parse correctly without any ordering dependency on a
  system message in `messages.jsonl`.
- Codex transcripts no longer display raw injected skill XML.

---

## Risks And Trade-Offs

### OpenCode sessions without step-finish tokens

Some sessions may lose previously displayed totals if they only expose
`message.tokens`. This is acceptable because the goal of the fix is correctness,
and a missing total is preferable to an inflated one.

### Localized compatibility logic

The compatibility handling remains distributed across parser and source layers.
This is a deliberate trade-off in favor of narrow, understandable fixes rather
than a larger shared abstraction.

### Codex parser vs renderer boundary

The raw XML suppression could be implemented in the parser or the transcript
rendering path. This design intentionally leaves that exact placement open, as
long as the stored transcript shown to the user does not present the injected XML
as normal conversation text.

## Decision

Implement five targeted correctness fixes with no parser-architecture refactor:

- OpenCode session token aggregation uses only `step-finish.tokens`
- Claude Code accepts both `Task` and `Agent` as subagent launches
- OpenCode source discovery includes `opencode.db` and `opencode-<channel>.db`
- Mistral Vibe compatibility is locked to schema-based parsing, not message order
- Codex injected `<skill>...</skill>` payloads are suppressed or neutralized for transcript display

This keeps the issue tightly scoped to parser correctness while restoring
compatibility with the current documented session formats.
