# Kimi Code Support Design

**Date:** 2026-07-29

**Status:** Approved design for [issue #167](https://github.com/supermaciz/sessions-chronicle/issues/167)

## Problem

Sessions Chronicle supports Claude Code, OpenCode, Codex, and Mistral Vibe,
but it does not discover or index Kimi Code sessions. Kimi Code uses a
composite directory format rather than one transcript per top-level session:

```text
$KIMI_CODE_HOME/
├── session_index.jsonl
├── workspaces.json
└── sessions/<workDirKey>/<sessionId>/
    ├── state.json
    └── agents/
        ├── main/wire.jsonl
        └── <subagentId>/wire.jsonl
```

`state.json` contains session and agent metadata, while every agent has an
append-only event journal. Correct support therefore requires coordinated
discovery, parsing, incremental change detection, subagent linkage, and
cleanup across multiple files.

The current format is documented in
[`docs/session-formats/kimi-code.md`](../../session-formats/kimi-code.md) and
compared with the other assistants in
[`docs/SESSION_FORMAT_ANALYSIS.md`](../../SESSION_FORMAT_ANALYSIS.md).

## Goals

- Discover current Kimi Code sessions under `$KIMI_CODE_HOME`, defaulting to
  `~/.kimi-code`.
- Index user and assistant messages, reasoning, tool calls and results, model
  metadata, and token usage.
- Index Kimi subagent journals as navigable child sessions, including nested
  subagents when the metadata provides a valid parent chain.
- Reindex when any file in a Kimi session bundle changes, appears, or
  disappears.
- Remove stale top-level and child sessions safely.
- Integrate Kimi Code into existing filters, analytics, diagnostics, resume,
  documentation, application metadata, and website surfaces.
- Reuse the existing database schema and transcript UI.

## Non-Goals

- Supporting the legacy Python CLI layout under `~/.kimi`.
- Adding a generic parser trait or refactoring all session-source
  implementations.
- Adding a Kimi-specific database table or transcript UI.
- Treating `independent` agents without a parent as subagents.
- Deduplicating, grouping, or displaying lineage between sessions related by
  `state.json.forkedFrom`. Each fork remains an independently indexed session.
- Implementing the unshipped cross-assistant skill-visibility design. Kimi
  skill records remain documented structural signals, not new transcript row
  types in this issue.
- Requesting broad Flatpak filesystem permissions for a custom
  `$KIMI_CODE_HOME` outside the user's home directory.

## Chosen Approach

Implement a Kimi-specific bundle parser. One Kimi session directory is the
unit of parsing and persistence:

```text
Kimi session directory
    -> state metadata + agent journals
    -> KimiParsedBundle
    -> main ParsedSession + zero or more child ParsedSession values
    -> atomic database replacement
```

This keeps Kimi format semantics inside one parser module while allowing the
indexer to remain responsible for discovery, change detection, persistence,
and pruning. It is smaller and safer than introducing a generic composite
source abstraction, and more cohesive than splitting Kimi semantics between a
single-journal parser and the database indexer.

Kimi-specific indexer behavior lives in `src/database/indexer/kimi.rs`,
declared as a private `kimi` submodule from `src/database/indexer.rs`. The
submodule owns bundle discovery, composite fingerprint checks, transactional
replacement, and Kimi-scoped pruning. Generic insertion helpers and indexing
result types remain in `src/database/indexer.rs`. This boundary prevents the
already large generic indexer file from absorbing source-specific bundle logic
without refactoring the existing assistants.

## Source Identity And Paths

Add `AiAssistant::KimiCode` with these mappings:

| Property | Value |
|----------|-------|
| Storage key | `kimi_code` |
| Display name | `Kimi Code` |
| Icon name | `kimi-code-symbolic` |
| Default home | `$KIMI_CODE_HOME`, otherwise `$HOME/.kimi-code` |

`AiAssistant::KimiCode.session_dir()` returns the Kimi home root rather than
the nested `sessions/` directory. This is required because
`session_index.jsonl` and `workspaces.json` are siblings of `sessions/` and can
provide project-path fallbacks.

`SessionSources` gains a `kimi_home` path. In `--sessions-dir` override mode,
the canonical fixture subdirectory is `kimi_home/`; if it is absent, the
existing fallback-to-root behavior applies.

The default Flatpak home permission covers the standard location. A custom
Kimi home is supported when it is visible inside the sandbox. Locations
outside the home remain inaccessible unless the user grants access separately;
the application does not add a broad host permission for this feature.

## Discovery

The indexer scans:

```text
<kimi_home>/sessions/wd_*/session_*/
```

A candidate is parseable when both of these paths exist:

- `state.json`
- `agents/main/wire.jsonl`

`session_index.jsonl` and `workspaces.json` are optional discovery aids and
project-path fallbacks. They are not authoritative because they can be absent,
stale, or partially written. A valid session directory found by scanning must
still be indexed when either top-level index is unavailable.

The scanner does not traverse the rest of `<kimi_home>`. Paths such as
`user-history/`, `migration-report.json`, credentials, logs, configuration,
and every other sibling of `sessions/` are ignored. Inside a discovered session,
the parser likewise ignores `plans/`, `tasks/`, `cron/`, logs, and other files
outside `state.json` and the declared agent journals. The only files read
outside `sessions/` are the two targeted metadata sources
`session_index.jsonl` and `workspaces.json`. The legacy migration marker under
`~/.kimi/.migrated-to-kimi-code` is outside the current Kimi home and is also
ignored.

Project path precedence is:

1. `state.json.cwd`, defined by the current upstream metadata contract
2. `state.json.workDir`, observed in current-format local sessions and retained
   as a compatibility alias
3. matching `session_index.jsonl.workDir`
4. matching workspace entry in `workspaces.json`
5. no project path

Malformed top-level index lines are skipped independently. They never prevent
directory scanning.

## Parser Boundary

Add `src/parsers/kimi_code.rs` with an entry point conceptually equivalent to:

```rust
pub fn parse_session_dir(&self, session_dir: &Path) -> Result<KimiParsedBundle>
```

`KimiParsedBundle` contains:

- the normalized main `ParsedSession`
- normalized child `ParsedSession` values
- the dependency paths used for incremental fingerprints
- the set of normalized session IDs that belong to the bundle

The exact Rust shape is an implementation detail, but the boundary must keep
`state.json` interpretation, agent identity construction, and journal parsing
inside the Kimi parser. The indexer must not interpret Kimi wire records.

JSONL journals are streamed with `BufReader` and line iteration. The parser
does not load complete journals into memory.

## Session Identity And Metadata

### Main session

The main session keeps the upstream session ID. Prefer `state.json.id` when it
is present and consistent; otherwise use the `session_<uuid>` directory name.

### Child sessions

Every supported child agent receives a globally namespaced synthetic ID:

```text
kimi-subagent::<main-session-id>::<agent-id>
```

This avoids collisions with main sessions and with equal agent IDs in other
Kimi sessions.

For an agent with `parentAgentId == "main"`, `parent_session_id` is the main
session ID. For a nested child, it is the synthetic ID of the declared parent
agent. A child is emitted only when the declared parent chain can be resolved
without a cycle.

Agents whose metadata type is `independent`, agents without a parent, cyclic
relationships, and references to unknown parents are not assigned an invented
parent. Their journals are skipped with a debug or warning diagnostic rather
than being surfaced as misleading subagents.

### Session fields

| Sessions Chronicle field | Kimi source |
|---------------------------|-------------|
| `tool` | `AiAssistant::KimiCode` |
| `project_path` | precedence from Discovery |
| `start_time` | `state.createdAt`, fallback earliest valid wire time |
| `last_updated` | maximum of `state.updatedAt` and valid wire times |
| `file_path` | main session directory for main; agent journal for child |
| `first_prompt` | meaningful `state.title`, fallback first real user prompt |
| `is_subagent` | `false` for main, `true` for supported children |
| `parent_session_id` | resolved parent ID for children |

Current Kimi metadata has appeared with both ISO-8601 and epoch-millisecond
timestamps across observed and upstream schemas. Timestamp parsing must accept
both representations and reject invalid values locally without panicking.

The `archived` metadata flag does not exclude a session. Sessions Chronicle is
a history browser, so archived Kimi sessions remain indexable while their
directories exist.

`state.json.forkedFrom` records lineage between independently evolving Kimi
sessions. Each fork is indexed under its own upstream session ID as a normal
top-level session. The field is not mapped to `parent_session_id`, which is
reserved for subagent navigation, and #167 does not deduplicate fork content or
add fork-specific UI.

The placeholder title `"New Session"` is not meaningful and does not override
the first real user prompt. A non-empty custom or automatically frozen Kimi
title does.

## Transcript Normalization

### User messages

`turn.prompt` with `origin.kind == "user"` is the canonical user-message
carrier. Its text and supported media placeholders are normalized into one
message at the wire record timestamp.

`context.append_message` duplicates prompts in normal journals. User-carrier
selection is journal-wide and deterministic: if the journal contains at least
one real-user `turn.prompt`, all user-origin `context.append_message` records
are ignored; otherwise user-origin `context.append_message` records are the
fallback carrier. The parser never attempts text-similarity matching and never
emits both carriers in one agent journal.

Records whose origin is `skill_activation`, `injection`, or `system_trigger`
do not become ordinary user messages, do not validate an otherwise empty
session, and do not supply the fallback title. This prevents injected context
from appearing as human input.

A parsed session must contain at least one real user message. Otherwise it
returns the source-specific `NoUserMessages` condition so the indexer can skip
or prune it without reporting a fatal indexing failure.

### Assistant messages

For `context.append_loop_event` records:

- `content.part` with `part.type == "text"` emits assistant text at its exact
  transcript position.
- Empty text parts are ignored.
- Unsupported media content is ignored unless the existing message model can
  represent a safe local placeholder without fetching remote content.

Every non-empty text part remains a separate message, including adjacent text
parts. This preserves journal order and avoids an implicit merge rule that
could change model or reasoning attribution.

### Reasoning

`content.part` with `part.type == "think"` accumulates in the shared
`PendingReasoning` helper. Pending reasoning attaches to the next visible
assistant message, tool call, or subagent transcript item. Orphan reasoning at
the end of a journal is dropped with a debug diagnostic, matching existing
parser behavior.

### Event timestamps and ordering

Wire `time` is epoch milliseconds. Transcript items preserve journal order;
timestamps are metadata and do not reorder records. Missing event timestamps
fall back to the session start time plus a stable sequence offset so indexes
remain deterministic.

Unknown record and loop-event types are ignored. Operational records that do
not produce transcript content remain available to the parser only when needed
for model or usage attribution.

## Tool Calls

On `tool.call`:

- create a pending `ToolCall`
- use a namespaced local row ID
- preserve the raw `toolCallId` in `parser_call_id`
- preserve `args` as JSON in `input_json`
- record the call time when present

On `tool.result`:

- correlate strictly by `toolCallId`
- normalize `result.output` from either a string or content-part array
- set `Completed` unless `result.isError` is true
- store error output in `error_text` for failed calls
- compute end time and duration when both timestamps are available

Unmatched results are ignored with a debug diagnostic. Calls without results
remain pending, allowing the existing ending-status derivation to classify an
interrupted session as abrupt.

## Models

`llm.request` is the source of per-step model metadata. Model attribution uses:

1. explicit turn/step metadata when it identifies a loop step
2. the next `step.begin` in journal order as a conservative fallback

The normalized model value prefers `modelAlias`, then `model`, and passes
through the existing `normalize_model` helper. It applies to assistant messages
and reasoning generated by that step. User messages have no model.

`config.update.modelAlias` is a fallback active-model hint, not a replacement
for a matching `llm.request`.

## Token Usage

Kimi exposes equivalent usage in two carriers, so they must not be added
together blindly.

Session aggregate precedence is:

1. sum every `usage.record` value with `usageScope == "turn"` exactly once
2. if no turn usage exists, sum `step.end.usage` values
3. otherwise leave usage absent

Mapping to `TokenUsage` is:

| Kimi field | Sessions Chronicle field |
|------------|--------------------------|
| `inputOther` | `input_tokens` |
| `output` | `output_tokens` |
| `inputCacheRead` | `cache_read_tokens` |
| `inputCacheCreation` | `cache_write_tokens` |
| none | `reasoning_tokens = None` |

Cache values are separate from `inputOther` and are not added into
`input_tokens`.

## Subagent Representation And Linking

`state.json.agents` is the canonical child-session graph. A parent `Agent`
tool call supplies the parent transcript position and delegation details.

For each `Agent` call, child matching uses this precedence:

1. an explicit child agent ID in structured call arguments or result data
2. an unambiguous child ID in the textual result
3. chronological one-to-one pairing between the remaining calls and children
   of the same parent

The chronological fallback is allowed only when the number of remaining
`Agent` calls equals the number of remaining children and every child journal
starts no earlier than its paired call. Calls and children are sorted by wire
time with their stable raw IDs as tie-breakers, then zipped. If counts differ,
a required timestamp is missing, or the time ordering is inconsistent, all
remaining pairs for that parent are ambiguous. The parser must not match by
prompt similarity, title, or agent profile alone.

When a child is matched, the parent `Agent` call becomes a `Subagent`
transcript item rather than a duplicate generic tool-call item. The row stores:

- a stable ID namespaced by parent session and raw call ID
- child `agent_id`
- delegated prompt extracted from call arguments when available
- terminal result summary when available
- `child_session_id` set to the synthetic child session ID
- `parser_ref` set from the raw call ID or stable local ID

When matching is ambiguous, preserve the `Agent` call as a generic tool call.
The child session may still be indexed as hidden, but no incorrect parent-side
navigation link is created.

Nested agents use the same rules recursively. Parsing rejects cyclic metadata
graphs and does not recurse without a depth bound derived from the finite
`agents` map.

## Incremental Indexing

A Kimi session cannot use a single-file fingerprint. The dependency set for a
bundle includes:

- `state.json`
- `agents/`
- every supported agent directory
- every supported `wire.jsonl`

Directory fingerprints detect immediate child creation and deletion; file
fingerprints detect journal appends and metadata changes.

The existing `file_fingerprints(file_path, mtime_ns, size)` table is sufficient.
Kimi-specific indexer helpers must:

1. enumerate the current dependency paths
2. compare every current path with its stored fingerprint
3. query stored fingerprint paths under the session-directory prefix
4. trigger reindexing if a current path changed or a previously stored path is
   no longer present
5. replace the bundle and its dependency fingerprints only after successful
   parsing

No schema migration or synthetic hash file is needed.

The main session, child sessions, transcript contents, subagent links, and
fingerprints are replaced in one database transaction. A parsing or insertion
failure leaves the previously indexed bundle intact.

After a successful replacement, child sessions formerly owned by the main Kimi
session but absent from the new bundle are deleted. Cascades remove their
messages, tool calls, subagents, transcript items, and reasoning.

A source-level pruning pass compares discovered top-level Kimi directories with
indexed Kimi main sessions under the active source root. Missing directories
are deleted with their descendants. Pruning is scoped by assistant and source
root so fixture runs and custom roots do not delete unrelated data.

## Error Handling

- Missing Kimi home or sessions directory: successful no-op.
- Missing required files in a candidate: skip the candidate.
- Invalid `state.json`: fail only that bundle and report it through existing
  indexing diagnostics.
- Unreadable main journal: fail only that bundle.
- Missing or unreadable journal for a declared supported child: fail that
  bundle and preserve its previously indexed version. This avoids replacing a
  complete bundle with a transiently partial one.
- Syntactically malformed JSONL line: warn, skip the line, and continue.
- I/O failure while reading a journal: fail the bundle and preserve its
  previously indexed version.
- Unknown record, content-part, or metadata field: ignore it.
- Missing real user message: skip/prune as a non-session, not a fatal error.
- Broken parent graph: skip affected child agents and continue the bundle.
- Integer or timestamp overflow: reject the affected value, never panic.

Session files are untrusted input. Errors must include the source path but must
not log full prompt, tool arguments, tool results, credentials, or other
potentially sensitive content.

## Database And UI Impact

No database schema change is required. Existing tables already represent:

- main and child `Session` rows
- messages and ordered transcript items
- tool calls and results
- subagent rows and child links
- reasoning attachments
- model attribution
- token usage and activity counts

Existing transcript and inspector widgets render all normalized Kimi content.
No Kimi-specific row component or CSS is added.

Add Kimi Code to source filters, default active assistants, source labels,
analytics display names, and indexing diagnostics. Prefer existing
`AiAssistant::ALL`-driven behavior where available. In explicitly exhaustive
UI state, add the minimal Kimi case rather than refactoring the full filter
model as part of this issue.

## Resume Behavior

Current official Kimi documentation defines:

```text
kimi --session <id>
```

Main Kimi sessions use this command from the canonical project workdir. The
stored upstream session ID is passed as the argument.

Synthetic child-session IDs are internal to Sessions Chronicle and must never
be passed to Kimi Code. Resume is disabled or hidden for Kimi child sessions.

## Product And Documentation Surfaces

Update supported-assistant references in:

- `README.md`
- `docs/DEVELOPMENT_WORKFLOW.md`
- `docs/SESSION_FORMAT_ANALYSIS.md`
- `docs/session-formats/kimi-code.md`
- AppStream metadata
- desktop search keywords
- website supported-assistant copy and guide

Copy the existing application icon to the website assistant-icon directory.
The docs must state that current `~/.kimi-code` sessions are supported and the
legacy `~/.kimi` parser remains out of scope.

## Test Fixtures

Add an anonymized Kimi home under:

```text
tests/fixtures/kimi_home/
```

Fixture coverage must include:

- a basic main session with user and assistant text
- duplicate `turn.prompt` and `context.append_message` carriers
- adjacent and interleaved text/reasoning parts
- successful, failed, unmatched, and interrupted tool calls
- multiple LLM steps and a model switch
- matching `usage.record` and `step.end.usage` data
- a malformed JSONL line followed by valid records
- a session with no real user prompt
- direct, sibling, and nested subagents
- an ambiguous `Agent` call-to-child case
- a child added after the first indexing pass
- a child journal removed after indexing
- a title-only `state.json` change
- a journal-only append
- one state fixture using upstream `cwd` and another using the observed
  compatibility field `workDir`
- two sessions linked by `forkedFrom`, both retained as top-level sessions
- operation without `session_index.jsonl` or `workspaces.json`

Fixtures must contain no real credentials, user names, private paths, or
proprietary prompt/tool output.

## Automated Tests

### Parser tests

- Preserve transcript order across messages, reasoning, tools, and subagents.
- Emit each human prompt once despite duplicate carriers.
- Exclude injected origins from human-message validation and title fallback.
- Attach reasoning to the next visible item.
- Correlate calls and results strictly by `toolCallId`.
- Preserve pending calls and mark explicit errors.
- Attribute models to the correct steps.
- Prefer turn usage and avoid token double counting.
- Parse ISO-8601 and epoch-millisecond metadata timestamps.
- Prefer upstream `cwd` over compatibility `workDir` and cover both fields.
- Skip malformed lines and unknown events without losing later valid data.
- Reject a session without a real user message.
- Build stable main and synthetic child identities.
- Preserve nested parent relationships and reject cycles.
- Avoid child linkage when matching is ambiguous.

### Indexer tests

- Discover sessions by directory scan with and without top-level indexes.
- Use top-level index data only as metadata fallback.
- Persist a complete bundle atomically.
- Skip unchanged bundles.
- Reindex after state, journal, agent-directory, or dependency-set changes.
- Remove deleted child sessions and links.
- Remove deleted top-level sessions only within the active Kimi source.
- Link parent and child content regardless of enumeration order inside a
  bundle.
- Keep synthetic IDs collision-free across two main sessions.
- Preserve the previous indexed bundle after a failed replacement.

### Integration and UI tests

- Resolve default `$KIMI_CODE_HOME` and fixture override paths.
- Resolve `tests/fixtures/kimi_home/` as the Kimi home in override mode.
- Round-trip the `kimi_code` storage value.
- Include Kimi Code in the fifth source filter and default selection.
- Show the Kimi icon and display name.
- Include Kimi in source analytics and diagnostics.
- Build `kimi --session "$2"` for a main session.
- Never offer resume for a child synthetic ID.
- Verify parent-to-child navigation using fixture data.

## Verification

Run CI-parity checks:

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo \
  cargo test --all --no-fail-fast
```

Run the application against fixtures through the Flatpak build:

```bash
flatpak-builder --user flatpak_app \
  build-aux/dev.maciz.sessionschronicle.Devel.json --force-clean
flatpak-builder --run flatpak_app \
  build-aux/dev.maciz.sessionschronicle.Devel.json \
  sessions-chronicle --sessions-dir tests/fixtures
```

Manual verification with local Kimi data confirms:

- top-level sessions appear under the Kimi filter
- messages, reasoning, tools, models, and token metrics render normally
- linked subagents open their child transcript
- child sessions do not appear in top-level lists
- main-session resume opens the expected Kimi session
- no local session content is copied into repository fixtures

## Success Criteria

Issue #167 is complete when current-format Kimi Code sessions behave as a
first-class supported source throughout Sessions Chronicle, including
incremental indexing, nested subagents, normalized transcript data, filtering,
analytics, diagnostics, main-session resume, public documentation, and website
representation, with no schema migration and no regression to existing
assistants.
