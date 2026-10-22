# OpenCode V2 support

Status: implemented by [PR #210](https://github.com/supermaciz/sessions-chronicle/pull/210).

## Intent and agreed scope

Restore OpenCode support after its primary storage moved to `session_v2` and
`session_message`. Success means feature parity with the actual V1 implementation,
plus visible system messages, skill injections, and compaction summaries.

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
  text available on expansion. Context entries are excluded from search in
  this delivery (revised 2026-10-21: avoids repeated system prompts and skill
  bodies flooding results, and removes the search plumbing).

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
message counts/title selection, and pushes context text into message search. A typed
context item is a smaller durable abstraction than broadening speaker roles.

## Discovery, compatibility, and incremental indexing

- Probe table availability instead of parsing the OpenCode version string.
  Support V2-only, V1-only, mixed SQLite databases, and JSON-only installations.
- Preserve existing path/channel discovery and fixture overrides. No new
  hardcoded home paths or mandatory schema columns unrelated to this feature.
- Select one representation per session ID globally across discovered sources:
  V2 > V1 > JSON. Within one generation, retain a deterministic configured-source
  order. Do not merge a frozen V1 transcript with its V2 counterpart.
- Distinguish absence, ineligibility, and failure for each session ID:
  - Absence: no V2 row exists. Use V1/JSON normally.
  - Ineligibility: the session was read but is excluded by policy, such as
    having no user message. Keep the current behavior: it is not indexed and
    its cached representation may be pruned.
  - Failure: a selected session cannot be read, parsed, or persisted. Report the error
    and add its ID to the retained set, so stale-session pruning keeps the
    last successfully indexed representation. Do not fall back to V1 or JSON
    for that ID in the same run.
- Revised 2026-10-22 (review points 1 and 2): commit a SQLite source's captured
  fingerprint (DB and WAL state) atomically with its enumerated representations,
  proven policy exclusions, and explicit per-representation `retry_required`
  flags, only after cleanup succeeds. A failed selected session retains its
  last good cache and sets its retry flag; it is never marked ineligible.
  The next incremental run retries flagged winners even without source writes,
  while successfully indexed siblings of an unchanged source are skipped.
  Success or proven policy exclusion clears the representation's retry flag.
  An inventory/fingerprint commit failure rolls back all that metadata, so the
  previous fingerprint or pending retry continues to force a safe retry.
  Opening, probing, or enumeration failure still prevents a source metadata
  commit and retains its cached owners.
- Capture the DB and WAL fingerprint values before opening the read snapshot,
  and commit those captured values, not a fresh stat taken after processing.
  Writes made by OpenCode during indexing then still differ from the stored
  fingerprint and are picked up by the next incremental run. The captured
  state includes whether the WAL exists: its creation or disappearance also
  triggers a re-read.
- A database that fails to open, probe, or enumerate retains every cached
  session it owns. Pruning decisions are made per source, never from one
  global "SQLite enumerated" flag: a successfully enumerated database must not
  prune sessions owned by another database that was skipped or unreadable.
- Not being discovered is not proof of deletion: current discovery returns an
  empty list when the directory cannot be read and ignores entry and `is_file`
  errors (`session_sources.rs`). A cached owner database that is not
  discovered loses its sessions only when its absence is confirmed, for
  example a direct check of its cached path reporting "not found". In every
  case, a session can still move to another discovered source that owns the
  same ID. Any discovery or access error (an unreadable directory, permission
  denied, or a failed metadata check) is not proof of deletion and retains
  the affected sessions.
- Source ownership comes from the cache. `sessions.file_path` already records
  the owning DB path for SQLite sessions and the file path for JSON sessions.
  Also persist the indexed representation's generation (V2, V1, or JSON);
  the DB path alone cannot distinguish generations in a mixed database.
  Populate this metadata during the mandatory one-time reindex.
  At the start of an OpenCode run, preload the cached IDs owned by each SQLite
  source that is skipped as unchanged or is unreadable, and treat them as
  retained candidates with their source and generation, not unconditionally
  finalized IDs. Resolve candidates using the global generation priority and
  configured-source order, including candidates discovered in other databases.
  A lower-priority candidate for a preloaded ID is skipped even when its own
  file changed. A higher-priority candidate may replace the cached owner only
  after successful parsing and atomic persistence; failure retains the previous
  cache and records a retry for the selected representation. Within a database
  that is actually read, generation probing, V2/V1 enumeration, and parsing
  share one read-only connection and one consistent snapshot. V2 is enumerated
  first and V1 rows for V2-present IDs are ignored. Dropping either reader must
  not end the snapshot while another reader still uses it.
- Ownership moves to a lower-priority source only when the owning database
  was successfully enumerated and no longer contains that ID. A failed or
  skipped owner never yields ownership to a lower-priority source, but does not
  block promotion to a higher-priority representation. In particular, an
  unchanged cached V1 owner must not prevent a newly discovered V2 candidate
  in another database from becoming the owner.
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

Persist context in a dedicated local-cache table, written in the same
transaction as the other session rows. It has no full-text index, and context
text must never be written to `messages` / `messages_fts`; existing search
queries stay unchanged.

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

Context entries participate in transcript ordering only. Global search,
in-transcript search (match counts, next/previous navigation, highlighting),
and the GNOME search provider ignore context text: a session matches only
through its user and assistant message content, as today. In-transcript search
navigation skips context rows. Expansion can remain transient for the current
detail view; no persistent setting is needed.

This exclusion applies to typed context entries, not to every occurrence of
the same text. Tool calls, including skill tool calls, keep their existing
search behavior: they are stored in their own table and are not full-text
indexed today. This delivery does not change that.

## Robustness

Skip malformed message rows/content items with useful diagnostics and continue
when possible. A malformed item must not discard later valid content. Session-wide
read failures preserve the previous cache. Unknown message/content types are
forward-compatible skips. Diagnostics identify source/session/item without dumping
entire prompt or result bodies.

Failure modes to avoid: stale V1 replacing V2, duplicated tool results or subagent
rows, chronology reordered by timestamps, double-counted tokens, injected text
presented as human speech, context-only sessions appearing as conversations, and
context text leaking into search results or snippets.

## Acceptance and verification

Use synthetic, source-derived fixtures without personal transcript content.

1. V2-only discovery and full indexing; V1-only and JSON regressions pass.
2. Mixed DB and JSON overlap yields one V2-backed session; test the same behavior
   on an incremental run with unchanged SQLite and modified JSON. With two
   channel databases, an incremental run that reads only one of them keeps
   the sessions of the other. An unreadable database keeps its sessions.
   A V2 parse failure keeps the previous cached session, does not advance the
   fingerprint, and is retried on the next incremental run; an ineligible
   session is still pruned.
   A cached V1 session in an unchanged database is promoted when V2 is discovered
   in another database, regardless of discovery order. A failed promotion keeps
   the previous cache and is retried. A cache-persistence failure after another
   session in the same source succeeded still leaves the source fingerprint
   unchanged and preserves the failed session's previous cached representation.
   A write to the source DB/WAL during indexing is detected on the next
   incremental run, and so are WAL creation and removal. Removing a channel
   database prunes its sessions; making it unreadable retains them, and so
   does an inaccessible discovery directory.
3. Native `seq` and content-array order win over conflicting timestamps/IDs.
4. Nullable/blank titles fall back to genuine user text; context cannot become
   the first prompt, inflate exchange counts, or create an otherwise empty session.
5. Tool lifecycle covers streaming strings, running, completed multi-block text,
   error with partial content, unknown states, missing timestamps, and updates to
   an existing message. Verify inspector payloads and displayed result text.
6. Reasoning, model metadata, and tokens match V1 feature semantics, without
   adding session rollups to assistant usage.
7. Subagent tests cover task+completion, completion alone, multiple resumed calls,
   missing children, and deterministic parent navigation without duplicates.
8. System, skill, and compaction context round-trip through persistence; a term
   present only in context matches no session in global, in-transcript, or GNOME
   search. Test structured duplicate injections and unknown/malformed records.
9. UI tests verify initially collapsed rows, keyboard expansion, Markdown, and
   that in-transcript match counts and navigation are unchanged by context rows.
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

## Implementation record — 2026-10-22

Implemented through `1e75ca4` on `opencode-v2`, submitted in
[PR #210](https://github.com/supermaciz/sessions-chronicle/pull/210).
The design sections above are preserved as the original specification; this
section records implementation choices, extensions, and remaining gaps.

### Deviations and clarifications

- **Unchanged sources use persisted inventories, not only cached owners.**
  Retaining only cached session IDs loses reservations for excluded IDs and
  omits lower-priority representations never selected before. The coordinator
  therefore stores every enumerated SQLite representation with its fingerprint
  in `opencode_source_sessions`. Unchanged sources participate in arbitration
  without opening SQLite; readers open lazily for fallback or promotion parses.
- **Ineligibility is proven per representation, not inferred from cache absence.**
  `opencode_source_sessions.ineligible` records actual `NoUserMessages` outcomes
  for a source path, generation, and session ID. A never-selected representation
  is parsed when it first wins, even if no session is cached. Known exclusions
  continue to reserve priority without reopening unchanged sources. Inventory,
  exclusions, and fingerprint commit atomically after successful processing and
  cleanup; failed parses remain retryable without another source write.
- **Confirmed deletion permits lower-priority fallback.** The original sentence
  requiring successful enumeration is qualified by direct confirmation that the
  owner is missing. Access errors still retain ownership. A fresh JSON fingerprint
  skips parsing only if the cache already holds that same JSON representation.
- **Generation belongs to the coordinator's source candidate.** The unused
  `generation` field was removed from the legacy `SessionSource::SqliteRow`;
  the V1 backend contract retains its DB path without duplicating that metadata.
- **Native subagent variants extend the specified `task` format.** Both `task`
  and `subagent` names produce subagent rows. Child links accept
  `state.metadata.sessionId` and `sessionID` (the former takes precedence when
  both are strings); `state.input.agent` supplies the agent name. A targeted
  read-only investigation confirmed the native variant in a user-reported
  session; regression fixtures use synthetic content, not personal transcripts.
  Completion correlation occurs while streaming rows rather than in a final pass.
- **Context does not consume pending reasoning.** Reasoning carries across
  context rows to the next message, tool-call, or subagent item, keeping its
  inspection affordance visible. A missing context entry uses the existing
  unknown-item fallback rather than an expander that cannot load.
- **Metadata failures are isolated.** Failure to load cached owners preserves
  the cache; unreadable fingerprint/inventory metadata forces a re-read. Cleanup
  precedes fingerprint commits, and finalization errors do not abort indexing
  of other AI assistants. Read snapshots are released before cache cleanup.

### Delivered structure and migrations

The native reader and projection live in `parsers/opencode/{v2,v2_projection}.rs`;
source arbitration and coordination live in `database/opencode_sources.rs` and
`database/indexer/opencode.rs`. Context has dedicated model/storage modules and
a typed GTK transcript row with lazy body loading, stale-response guards, and
search exclusion.

| Cache version | Delivered change |
| --- | --- |
| 19 | Typed context storage and transcript references; OpenCode owner generations and DB/WAL fingerprints |
| 20 | Source inventories persisted alongside fingerprints |
| 21 | Proven per-representation ineligibility; existing inventory defaults to unproven without deleting cached sessions |

OpenCode reader generation **5** forces a one-time refresh of earlier inventories,
independently of upstream writes. Cache schema version, reader generation, and
source representation generation (`v2`, `v1`, `json`) are distinct concepts.

### Verification and outstanding evidence

- Synthetic fixture provenance is pinned to upstream commit
  `0c6be8694894dc96b87c5a7b773d52e57ddaa1b2` in
  `tests/fixtures/opencode_v2/README.md`. Both format references were updated.
- Parser, coordinator, migration, GTK, public-path integration, and GNOME search
  tests cover the delivered behavior, including fallback from an unchanged,
  never-selected source and retries without another source write.
- Formatting, Clippy, and the full headless suite passed for `1e75ca4` before
  PR creation. No dependency or packaging configuration changes were made.
- **Screenshots are not delivered:** the maintainer explicitly chose to capture
  them separately. This is a waiver of the screenshot deliverable, not a claim
  of exhaustive live-UI verification.
- The coordinator test named
  `opencode_snapshot_fingerprint_detects_concurrent_write` checks WAL changes
  between runs, not a synchronized write during finalization. Reader snapshot
  consistency is tested separately.
- The UI loads the transcript in one query; the planned "across pages" coverage
  exercises interleaved context and message ordering rather than a literal UI
  page boundary.
