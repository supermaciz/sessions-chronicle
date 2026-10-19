# Claude Code Recent Format Update — Design

**Date:** 2026-10-19  
**Status:** Approved design, pending implementation plan  
**Related docs:** [claude-code.md](../../session-formats/claude-code.md), [SESSION_FORMAT_ANALYSIS.md](../../SESSION_FORMAT_ANALYSIS.md)

## Problem

The 2026-10-18 Claude Code format watch pass (v2.1.264–v2.1.302) concluded that
no parser change was *required*, but left four follow-ups open:

1. **Parsing cost.** `ClaudeCodeParser::parse` deserializes every JSONL line
   into a full `serde_json::Value` before dispatching on `type`. Since v2.1.300
   the `api-request`, `api-request-shape`, and `api-request-blob` lines account
   for 35–69 % of transcript bytes and are discarded right after being parsed.
   The cost has never been measured.
2. **No fixture coverage.** No fixture contains the v2.1.226+ / v2.1.300+ event
   types, so the "skipped without error" claim in the docs is not protected by
   any test — in particular the `api-request-blob` trap, whose
   `message.role: user|assistant` payloads look like conversation messages.
3. **Non-human user turns leak into the first prompt.**
   `first_prompt::extract_first_prompt` already ignores `<command-…>`,
   `<local-command-…>`, `<system-reminder>`, and `<command-output>` wrappers,
   but the parser ignores `isMeta` and `origin.kind` / `turnOrigin`. A
   `<task-notification>` turn or an injected skill body
   (`isMeta: true`, "Base directory for this skill: …") can therefore become
   the session's fallback title.
4. **Stale docs.** The model slug table predates the Claude 5 family, and the
   `SESSION_FORMAT_ANALYSIS.md` "Last Updated" line still reads 2026-10-14.

## Goals

- Skip full deserialization of event types the parser does not consume,
  without changing any indexing result, and measure the effect.
- Exclude non-human user turns from first-prompt / title selection.
- Add a synthetic v2.1.300+ fixture and an integration test that lock in the
  current skip behavior and the two changes above.
- Bring the Claude Code docs up to date.

## Non-Goals

- No change to the transcript: non-human turns stay visible and searchable.
- No change to `message_count` or `has_user_message`.
- No new message role, flag, database column, or UI change.
- No consumption of new event types (`cost-state`, `agent-name`,
  `api-request*`, …).
- No raw-text (substring) pre-filter; see "Alternatives considered".

## Design

### 1. Event-header pre-filter

Add a private header struct in `crates/core/src/parsers/claude_code.rs`:

```rust
#[derive(Deserialize)]
struct EventHeader<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(rename = "sessionId", borrow)]
    session_id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    cwd: Option<Cow<'a, str>>,
}
```

serde skips all other keys without allocating. For each non-blank line the
event loop:

1. Deserializes `EventHeader`.
2. Feeds `session_id` and `cwd` into the existing first-wins capture
   (`maybe_capture_session_id` / `maybe_capture_cwd`, adapted to take
   `Option<&str>`). Capture still runs on **every** line, so a session whose
   first `sessionId` or `cwd` sits on a `permission-mode`,
   `file-history-snapshot`, or `attachment` line behaves exactly as today.
3. Fully deserializes the line into `serde_json::Value` only when `kind` is
   `user`, `assistant`, or `ai-title`, then runs the existing dispatch. Any
   other `kind` (or a missing one) is skipped.

**Fallback.** If the header fails to deserialize (invalid JSON, a non-object
line, a non-string `type` / `sessionId` / `cwd`, …), the loop falls back to the
current path: full `Value` parse, existing capture from the `Value`, existing
dispatch, and the existing `tracing::warn!` + `had_parse_errors = true` when
that parse also fails. This keeps behavior identical for atypical but valid
lines and introduces no new parse errors.

**Trade-off.** Kept lines (`user` / `assistant`) are scanned twice — once
cheaply for the header, once fully. This may slightly slow transcripts with no
`api-request*` lines; the measurement below decides.

### 2. Parse-timing measurement

Add `crates/core/tests/claude_parse_timing.rs` containing one `#[ignore]`
test:

- Reads a transcript path from `CLAUDE_PARSE_TIMING_FILE`; returns early (and
  prints a note) when unset, so no personal data is committed and CI is
  unaffected.
- Calls `ClaudeCodeParser::parse` N times (default 20, overridable via
  `CLAUDE_PARSE_TIMING_RUNS`) and prints the median and min wall time plus the
  file size and line count.

Run with:

```sh
CLAUDE_PARSE_TIMING_FILE=<path> \
  cargo test --release -p sessions-chronicle-core --test claude_parse_timing \
  -- --ignored --nocapture
```

Procedure: run it on `main` (baseline) and after the pre-filter, on two local
transcripts — one v2.1.301+ with many `api-request*` lines and one written
before v2.1.300.

**Decision rule.** Keep the pre-filter if it is clearly faster on the
v2.1.300+ transcript **and** slows the pre-v2.1.300 transcript by no more than
5 %. Otherwise stop and report the numbers before deciding. In every case the
numbers go into `docs/session-formats/claude-code.md`.

### 3. Non-human user turns

A user turn is **non-human** when any of these holds on the event:

- `isMeta == true`
- `origin.kind == "task-notification"`
- `turnOrigin == "task_notification"`

No text-based `<task-notification>` detection: the structured fields were
present in every observed case.

`ParseState` records the indices (into `messages`) of user messages emitted
from non-human events — a `HashSet<usize>` or a `Vec<bool>` parallel to
`messages`, whichever reads better in context. `emit_user_message_from_event`
takes the event, so it can classify there. `Message` itself is unchanged.

In `finish()`, the first-prompt fallback (used only when there is no
`ai-title`) calls `first_prompt::extract_first_prompt` on the messages minus
the non-human indices. If that yields `None` — every user turn was non-human,
e.g. a subagent transcript whose only prompt is an `isMeta` skill payload —
it falls back to `extract_first_prompt` over all messages, i.e. today's
behavior, so the session never loses a title it has today.

`has_user_message` is deliberately unchanged: a local subagent transcript
(`…/subagents/agent-a4dc5520e322acc51.jsonl`) opens with an `isMeta` user turn,
and excluding such turns would risk rejecting real transcripts with
`NoUserMessages`.

### 4. Fixture and integration test

New directory `tests/fixtures/claude_recent_events/` with one synthetic
`<uuid>.jsonl`. It is separate from `claude_sessions/` because several tests
(e.g. `tests/project_detection.rs`) index that directory and count sessions.
All UUIDs, hashes, and account/organization IDs are invented.

Line order mirrors a real v2.1.301 session:

| Line(s) | Purpose |
|---------|---------|
| `permission-mode` (the first line, carries `sessionId`) | session-id capture through the pre-filter |
| `mode`, `atis-latch`, `bridge-session`, `agent-name` | skipped without error |
| `attachment` (`queued_command`, `deferred_tools_record`) | skipped without error |
| `api-request-shape`, `api-request` | skipped (the heavy lines) |
| `api-request-blob` with `role: "user"`, then `role: "assistant"`, each with a unique token | must not become messages |
| `user` with `isMeta: true` (skill body), then `user` with `origin.kind: "task-notification"` | must not become the first prompt |
| `user` (human), `assistant` with `requestRef`, `effort`, `requestedModel`, `message.usage` | real messages |
| `cost-state` | skipped |

The fixture has no `ai-title`, so the first-prompt fallback is exercised end
to end.

New `tests/claude_recent_events.rs`, modeled on
`tests/claude_teammate_linkage.rs`, indexes the directory and asserts:

- exactly one session, with the fixture's `sessionId`;
- `message_count` equals the number of text-bearing `user` / `assistant`
  lines (would be wrong if blobs were counted);
- `first_prompt` is the human turn's text, not the skill body or the
  notification;
- searching the token present only in an `api-request-blob` returns nothing;
- searching a token present only in the task-notification turn returns the
  session (non-human turns stay searchable).

Update `tests/fixtures/README.md` to list the new directory.

### 5. Documentation

- `docs/session-formats/claude-code.md`
  - *Parser Behavior*: describe the pre-filter (fully parsed types,
    header-level `sessionId` / `cwd` capture, fallback) and replace "Not yet
    measured" with the measured numbers.
  - *Title extraction*: document the non-human exclusion and its fallback.
  - *Supported Claude Code Model Slugs*: replace the pre-Claude 5 table. Check
    the official model configuration page during implementation; if reachable,
    use its slugs and cite it, otherwise list the 2026-10-18 locally observed
    slugs marked "observed locally, not verified upstream". Replace the
    Sonnet 4.6 / Opus 4.6 reference links the same way.
- `docs/SESSION_FORMAT_ANALYSIS.md`: set "Last Updated" to the delivery date,
  add a status line for this pass, replace "extra full-line deserialization
  cost is unmeasured" in the API-request Key Finding with the result, and
  mention the new fixture under *Next Steps* → edge cases.

## Testing

- Unit tests in `claude_code.rs`, in the style of the existing `ai-title`
  tests:
  - `sessionId` present only on a leading `permission-mode` line is captured;
  - a session starting with a task-notification turn gets the human prompt as
    first prompt;
  - a session starting with an `isMeta` skill body gets the human prompt;
  - a session whose only user turns are non-human keeps today's first prompt.
- Integration test `tests/claude_recent_events.rs` (above).
- The whole existing suite must pass unchanged — the pre-filter is
  behavior-preserving.
- Definition of Done commands from `AGENTS.md` (fmt, clippy, headless tests).

## Implementation Order

1. Timing test only; record the baseline on `main`.
2. Pre-filter plus its unit test; re-measure and apply the decision rule.
3. Non-human turn exclusion plus its unit tests.
4. Fixture plus integration test (validates 2 and 3 together).
5. Documentation.

Each step is a separate commit; commits are made only with the user's
go-ahead.

## Alternatives Considered

- **Raw substring pre-filter** (skip lines containing `"type":"api-request`):
  fastest, but relies on Claude Code's serialization (compact JSON, top-level
  key) and a nested object containing that key would silently drop a real
  message. Rejected as unsafe for untrusted input.
- **Removing non-human turns from the transcript**: cleaner transcript, but
  subagent results and skill bodies would no longer be searchable. Rejected.
- **Distinct role or dimmed rendering for non-human turns**: most faithful,
  but touches the model, possibly the SQLite schema, and the UI. Deferred.
- **Excluding non-human turns from `has_user_message`**: risks pruning real
  subagent transcripts that open with an `isMeta` prompt. Rejected.
