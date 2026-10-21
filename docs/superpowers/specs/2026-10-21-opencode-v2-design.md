# OpenCode V2 support

Status: reviewed draft pending user approval. No implementation authorized yet.

## Intent and agreed scope

Restore OpenCode support after its primary storage moved to `session_v2` and
`session_message`. Success means feature parity with the actual V1 implementation,
plus searchable system messages, skill injections, and compaction summaries.

User-approved decisions:

- Preserve V1 SQLite and legacy JSON support; prefer V2, then V1, then JSON for
  the same session ID, without duplicate sessions.
- Use a dedicated V2 reader feeding the application’s shared domain model.
- Preserve exchanges, reasoning, tool calls/results/statuses, subagent navigation,
  titles, timestamps, models, tokens, and search.
- Introduce typed context entries for system messages, injected skills, and
  compaction summaries; these are not user or assistant messages.
- Extend the existing transcript rather than redesigning the UI.
- Context entries are collapsed initially (visual option A), with their full
  text available on expansion. Search includes them by default and identifies
  their kind; navigating to a match expands the targeted entry.

The technical policies below are proposed builder decisions, not additional
requirements explicitly supplied by the user.

## Evidence and limitations

Source-format baseline: `docs/SESSION_FORMAT_ANALYSIS.md` and
`docs/session-formats/opencode.md`, especially the 2026-10-14 V2 investigation.
That investigation reports upstream source inspection and a local v2.0.29 DB.
This design does not claim a fresh upstream or personal-database validation.

Repository inspection confirms:

- `crates/core/src/parsers/opencode/sqlite_backend.rs` discovers only `session`
  and reads `message` / `part`; the connection is read-only.
- `crates/core/src/parsers/opencode/mod.rs` exposes a V1-shaped backend contract
  and sorts messages by timestamp and ID. It already handles reasoning, despite
  the older parser-behavior section of the format reference.
- `crates/core/src/models/message.rs` has user, assistant, tool-call, and
  tool-result roles. Context categories should not be added as speaker roles.
- `crates/core/src/models/transcript_item.rs` represents message, tool-call, and
  subagent entries. UI equivalents live under
  `src/ui/session_detail/transcript/`.
- `crates/core/src/database/indexer.rs` handles SQLite-first indexing, JSON
  fallback, source fingerprints, and indexed-ID deduplication. Source selection
  must work even when an unchanged SQLite source is skipped incrementally.
- `crates/core/src/database/schema.rs` owns versioned local-cache migrations and
  the message full-text index.

Before implementation, inspect source-derived V2 fixtures to settle exact shapes
of skill injection payloads, compaction summaries, and tool-result content blocks.
The provided references describe those fields but are not a complete schema for
every nested value. Do not invent nested fields or infer skill identity from prose.

## Architecture and alternatives

### Selected: dedicated V2 reader and shared normalized output

Keep the V1/JSON reader contract for existing formats. Add a V2 reader which
reads native messages and produces the same normalized parsed-session output,
extended with context entries. Share normalization helpers where their semantics
really match: model names, timestamps, token conversion, text projections, and
domain-object assembly. Do not duplicate the whole V1 parser.

The indexer selects an explicit source variant carrying the DB path and schema
generation. The V2 reader need not implement the V1 `load_parts` abstraction.
SQLite connections and source probing may be shared independently of parsing.

### Rejected: adapt V2 into synthetic V1 parts

This minimizes initial plumbing but embeds legacy assumptions about roles,
separate parts, tool state, and timestamp ordering. Context messages and V2
sequence ordering would require exceptions throughout the old parser.

### Rejected: store context as ordinary assistant messages

This reuses rendering immediately but misattributes system content, affects
message counts/title selection, and loses meaningful search labels. A typed
context item is a smaller durable abstraction than broadening speaker roles.

## Discovery, compatibility, and incremental indexing

- Probe table availability instead of parsing the OpenCode version string.
  Support V2-only, V1-only, mixed SQLite databases, and JSON-only installations.
- Preserve existing path/channel discovery and fixture overrides. No new
  hardcoded home paths or mandatory schema columns unrelated to this feature.
- Select one representation per session ID globally across discovered sources:
  V2 > V1 > JSON. Within one generation, retain a deterministic configured-source
  order. Do not merge a frozen V1 transcript with its V2 counterpart.
- Distinguish absence from failure. If no V2 row exists, use V1/JSON normally.
  If a selected V2 session fails to parse, report the error and retain the last
  successfully indexed representation; do not silently replace it with stale V1.
  Unreadable databases must not trigger destructive cleanup of their sessions.
- Establish source ownership even for unchanged/skipped databases, so an older
  JSON entry cannot overwrite an already indexed V2 session.
- Preserve WAL-aware change detection. A changed embedded result must refresh
  the session even if message count and creation timestamps are unchanged.
- Force a one-time OpenCode reindex when deploying this reader; unchanged file
  fingerprints must not preserve the V1-only cache. Track reader generation in
  cache metadata or equivalent invalidation state.
- Replace one session’s dependent rows atomically. Failure leaves the previous
  indexed session intact. Clear all dependent context/search rows on replacement
  or deletion, using existing cleanup conventions.
- Read a consistent SQLite snapshot without modifying the source database;
  stream message rows rather than materializing the whole source DB. The existing
  normalized session result may still occupy session-sized memory.

## V2 transcript projection

Order native messages by `seq`, with deterministic tie-breaking by ID for damaged
input. Preserve `content[]` order within each assistant message. Timestamps are
display metadata, not a replacement for V2 ordering. Generate stable identities
from native message ID and content position where no native content ID exists.

| Source | Normalized behavior |
| --- | --- |
| `user.text` | User message; eligible for first-prompt fallback |
| Assistant `text` | Assistant message with per-message model |
| Assistant `reasoning` | Existing reasoning attachments and inspection behavior |
| Assistant `tool` | Existing tool-call or subagent representation |
| `system.text` | Context entry: system |
| Standalone `skill` | Context entry: skill, retaining structured name and text |
| Structured injected skill content on user messages | Separate skill context when supported by verified payload shape; preserve user prompt |
| Completed `compaction` summary | Context entry: compaction summary |
| Subagent completion `synthetic` | Structural linkage/result enrichment, not a duplicate context row |
| Other `synthetic`, switches, `idle`, standalone `shell` | Tolerate and skip in this delivery |

Incomplete/failed compactions without a completed summary do not create summary
entries. Do not reconstruct compaction `recent` history or fork ancestry. Read
the session’s own stored transcript. Do not add attachment fetching or binary
rendering; preserve existing textual projection and inspection conventions.

Session title: nonblank explicit title, otherwise first genuine user text. Context
must not supply a title or make a context-only session eligible for indexing.
Retain the V1 no-user-message eligibility policy for this delivery. Context does
not count as a user/assistant exchange or contribute token usage.

Use session timestamps for session dates and native message timestamps for rows.
Read assistant `model.id` through existing normalization; do not retroactively
assign a session-level model to every message. Aggregate assistant usage once;
do not add session rollup tokens on top. Preserve absent values as unknown rather
than manufacturing zero usage.

## Tool calls and subagents

- V2 tool name is `name`; its call identity is the content item `id`.
- Map `streaming` to the existing pending status, then `running`, `completed`,
  and `error` to their existing equivalents. Unknown states remain unknown.
- Preserve raw streaming input strings; do not require parsed JSON prematurely.
- Derive result text from supported textual blocks in `state.content`, preserving
  order; retain structured payload for existing inspectors as appropriate.
  Error states preserve both error text and any partial output.
- Use V2 content-level times for durations when valid; missing/invalid ranges
  remain unknown, not negative durations.
- `name == "task"` produces a subagent row, not a duplicate ordinary tool call.
  `state.metadata.sessionId` is the primary child link. `input.task_id` is a
  requested resume target, not proof that a child was successfully launched.
- A synthetic completion with `metadata.source == "subagent"` and `childID`
  enriches the matching task row. Do not collapse multiple task invocations that
  resume the same child. Match unambiguously by child and sequence; if multiple
  candidates remain, keep their independent results rather than guessing.
- With no corresponding task row, a completion may provide one stable standalone
  subagent row keyed by its native message ID. Avoid repeating the completion
  payload as both a context message and a subagent result.
- Child `parent_id` controls parent-child session classification. A missing child
  transcript leaves the parent row visible without a broken navigation action.
- Skill tool calls remain tool calls. Dedicated injected skill content becomes
  context. Suppress duplicated bodies only where structured provenance proves
  they are the same injection; never deduplicate unrelated invocations by name
  or text similarity alone.

## Shared context model and persistence

Introduce a context entity with session identity, stable source identity, subtype
(`System`, `Skill`, `CompactionSummary`), optional structured name, text, timestamp,
and transcript position. Add a `Context` transcript-item variant and corresponding
reference; exactly one domain reference remains populated per transcript item.

Persist context in a dedicated local-cache table, with a full-text index maintained
transactionally using the repository’s existing FTS conventions. Extend normalized
search results with context subtype and a stable transcript target. Reuse common
query and snippet facilities rather than treating context as a fake message.

Provide a versioned cache migration. Preserve favorites and user-owned settings;
invalidate only derived data as needed. Other AI assistants remain behaviorally
unchanged and need not emit context entries in this delivery.

## UI and search contract

Option A: one compact, keyboard-operable expander in the existing transcript.
Labels distinguish System, Skill (with name when available), and Compaction
summary. Reuse existing typography, Markdown rendering, selection, and lazy
content-loading patterns. Long bodies must not all load just to list the transcript.

The prototype is at `.superpowers/brainstorm/224788-1792543607/content/context-options.html`
in the design workspace; it is illustrative, not a pixel-perfect GTK specification.
Its essential approved behavior is fully described here and does not depend on
that local artifact remaining available.

Global and in-transcript search include full context text by default, even when
collapsed. Search snippets identify the context kind; activating a hit loads,
expands, scrolls to, and highlights its exact transcript entry. Context participates
in transcript ordering, match counts, and next/previous match navigation. Expansion
can remain transient for the current detail view; no persistent setting is needed.

The system search provider inherits session eligibility through the shared search
path, while retaining its existing excerpt/privacy preferences. Do not expose
additional context text in GNOME results when transcript excerpts are disabled.

## Robustness

Skip malformed message rows/content items with useful diagnostics and continue
when possible. A malformed item must not discard later valid content. Session-wide
read failures preserve the previous cache. Unknown message/content types are
forward-compatible skips. Diagnostics identify source/session/item without dumping
entire prompt or result bodies.

Failure modes to avoid: stale V1 replacing V2, duplicated tool results or subagent
rows, chronology reordered by timestamps, double-counted tokens, injected text
presented as human speech, context-only sessions appearing as conversations, and
search hits which cannot be reached in the transcript.

## Acceptance and verification

Use synthetic, source-derived fixtures without personal transcript content.

1. V2-only discovery and full indexing; V1-only and JSON regressions pass.
2. Mixed DB and JSON overlap yields one V2-backed session; test the same behavior
   on an incremental run with unchanged SQLite and modified JSON.
3. Native `seq` and content-array order win over conflicting timestamps/IDs.
4. Nullable/blank titles fall back to genuine user text; context cannot become
   the first prompt, inflate exchange counts, or create an otherwise empty session.
5. Tool lifecycle covers streaming strings, running, completed multi-block text,
   error with partial content, unknown states, missing timestamps, and updates to
   an existing message. Verify inspector payloads and search text.
6. Reasoning, model metadata, and tokens match V1 feature semantics, without
   adding session rollups to assistant usage.
7. Subagent tests cover task+completion, completion alone, multiple resumed calls,
   missing children, and deterministic parent navigation without duplicates.
8. System, skill, and compaction context round-trip through persistence and search;
   test structured duplicate injections and unknown/malformed records.
9. UI tests verify initially collapsed rows, keyboard expansion, Markdown, search
   match counts, targeted expansion/highlighting, and navigation through context.
10. Migration reindexes an unchanged existing source and preserves favorites;
    repeat indexing does not accumulate dependent rows. WAL-only updates refresh
    mutable content; failed reads do not erase the last good cache.
11. A large synthetic transcript exercises lazy body loading and bounded source
    reading. Capture updated screenshots with fixture-driven UI runs.

Implementation checks: `cargo fmt --all -- --check`,
`cargo clippy --all -- -D warnings`, and the test suite exclusively headless:
`dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --all --no-fail-fast`.
Run Flatpak build verification if packaging/build configuration changes.

Update both format references to reflect delivered support and correct stale
reasoning statements. The implementation plan must cover the entire data path,
not just parser fixtures.

## Left to implementation planning

Exact module/type names, migration version number, helper extraction boundaries,
SQL batching/index details, and test file organization are builder choices.
Verify nested payload contracts against pinned upstream sources or sanitized
fixtures before encoding them. If that reveals a product-level ambiguity rather
than a schema detail, bring it back for a decision.
