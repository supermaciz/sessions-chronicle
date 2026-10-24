# Mistral Vibe Unified Session Support

- Date: 2026-10-23
- Status: Implemented — PR open; manual release validation pending
- Scope: Full functional parity with existing Mistral Vibe sessions
- Revision: Implementation status and deviations recorded on 2026-10-24
- Implementation: [PR #211](https://github.com/supermaciz/sessions-chronicle/pull/211), branch `vibe-v3`; code baseline `9bf2b16`

## Intent and agreed decisions

Users upgrading Mistral Vibe must be able to browse, search, inspect, and resume their new sessions in Sessions Chronicle while retaining access to legacy sessions. Full parity is a release condition, rather than a follow-up milestone.

The user selected a native Rust reader based on Vibe's persisted public history projection: load its snapshot and referenced chunks, then apply journal updates. Supplement session metadata from the runtime state and `meta.json`. Reuse Sessions Chronicle's domain models and presentation.

The following policies define the intended behavior. The implementation status and deviations section records the delivered behavior and outstanding gaps without weakening these requirements.

## Implementation status and deviations

### Delivered implementation and validation

The native Rust reader, journal replay, domain conversion, saved-result resolution, incremental indexer integration, schema v21, child linkage and resume eligibility are implemented in PR #211. Legacy Vibe sessions remain supported alongside Unified stores. The implementation accepts store minor 9 only; it does not add a Python runtime dependency or execute Vibe to read sessions.

The implementation is under review, not merged or declared fully release-validated. On code baseline `9bf2b16`, the recorded verification is:

- `cargo fmt --all -- --check` and `cargo clippy --all -- -D warnings`: passed.
- Full headless repository suite: **1,724 passed, 0 failed, 1 ignored**. The ignored case is the opt-in local-store reader check, not an unreported test failure.
- Final-code isolated Flatpak build: passed, including **4/4** packaging metadata checks.
- Independent whole-field journal-digest audit: **16/16** complete synthetic records; deterministic fixture regeneration and matching Python/Rust replay counts for all seven synthetic stores.
- Whole-change review followed by a scoped review of the final corrections: all identified Critical/Important findings addressed. The two deferred Minor findings are recorded below.

These results establish the tested code baseline; they do not substitute for the manual release gates below or prove compatibility with untested future storage revisions. See the [implementation plan](../plans/2026-10-23-mistral-vibe-unified.md) and [format reference](../../session-formats/mistral-vibe.md).

### Functional deviations and known limits

| Spec requirement | Implemented behavior / deviation | Impact and follow-up |
|---|---|---|
| Failed reads retain the previous indexed snapshot and user-owned state. | Incremental and non-clearing per-source updates preserve prior rows, contents, signatures and favorites on read failure. The existing global `index_all_full_reindex` still calls `clear_all_sessions` before rebuilding; global rebuild atomicity was not changed. | **Preservation scope is narrower than an unconditional reading of this spec.** An explicit global rebuild can lose the previous snapshot and favorites even if a replacement store fails to parse. A transactional global rebuild is separate follow-up work. |
| Reject deeper subagent nesting and cycles. | Known deeper ancestry is rejected, including either discovery order and cleanup of previously indexed children once their parent is revealed as a subagent. A structurally valid orphan can still index while its parent is absent; the reader cannot infer missing ancestry safely. | A forged orphan can appear temporarily without a navigable parent link. Later verified ancestry triggers rejection/removal. Requiring a readable parent before any child indexing would conflict with child-before-parent support; that stricter policy is not implemented. |
| Reject traversal and escaping symlinks. | Static session-down symlinks, including `CURRENT`, are rejected; symlinked source ancestors remain supported. Saved results use no-follow metadata validation, then compare the opened handle's device/inode before reading from that handle. This is not an `openat`/`O_NOFOLLOW` path-resolution guarantee. | Ordinary different-inode substitutions are rejected, but a same-inode symlink/hardlink ABA can evade the literal no-symlink policy. This remains nonblocking hardening debt, not a claim that all adversarial filesystem races are closed. |
| Drop empty reasoning content. | A blank-only summary can remain as `summary_text = Some(...)` and create an otherwise empty reasoning attachment. | Deferred Minor presentation defect; filter blank summary parts before creating an attachment. Readable neighboring content is not lost. |
| Meaningful end-to-end filter coverage. | Integration tests verify overall Vibe filter counts and R302 membership, while a separate database count verifies indexed Unified roots. They do not individually assert all four Unified roots in the public filter result. | Deferred Minor test gap, not evidence of a known filter failure. Add explicit result-set membership assertions for all expected roots and excluded children. |

The remaining limitations above are explicit implementation gaps or bounded policies, not retroactive changes to the design requirements. The global rebuild behavior predates this reader; it is listed because the preservation wording otherwise overstates the guarantee delivered by this feature.

### Equivalent implementation choices, not functional gaps

- Child availability is not serialized into `UnifiedSignature`. Instead, reciprocal SQL linkage runs when either participant is published, and deletion cleanup clears links. A newly readable child can therefore link to an incrementally skipped parent without forcing a parent transcript reparse.
- Existing analytics retain their top-level-only semantics for all AI assistants. All seven Unified fixture sessions are indexed, but only their four root/fork sessions contribute to top-level session totals; child tokens are not added to parent totals.
- Minor 9 rejects `pending` effects and unknown state-bearing variants. The mapping mentioned later for a separately supported schema is conditional; no such schema was added. Unsupported stores are diagnosed rather than partially accepted.
- The final journal implementation omits the **entire** `record_sha256` field when hashing, as required below and by pinned upstream. A shared reader/generator slicing defect found during implementation review was corrected, the fixtures regenerated, and an independently anchored writer vector added. This is a resolved defect, not an accepted deviation from the spec.

### Outstanding manual release gates

- [ ] Inspect fixture R302 in the running UI and capture screenshots covering message order, reasoning, tool call statuses, full saved output and navigation to CHILD_A/CHILD_B.
- [ ] Check the target Vibe CLI's `--resume` support and actually reopen a real Unified root from the app; confirm the running UI disables resume for a Unified child. Automated eligibility tests do not validate external CLI behavior.

Neither manual gate has been performed or waived. No live Vibe command or desktop-window inspection was used during implementation. Until these gates are completed, the full functional-parity release condition remains pending.

## Goals and boundaries

Support messages, full-text search, ordered transcript items, tool call inputs/results/statuses, visible reasoning, model metadata, project association, timestamps, token totals, subagent inspection, and root-session resume. Existing filters, analytics, favorites, deep links, and GNOME search must work through the existing indexed domain data.

Keep legacy `meta.json` + `messages.jsonl` sessions supported alongside Unified sessions. Read source data without modifying it. Preserve existing favorites and session identity during reindexing.

Do not add Vibe-specific screens or controls, execute Vibe to obtain transcripts, add a Python runtime, import Vibe's own pin/archive settings, or implement Vibe's recovery engine. New capabilities such as scheduling, interactive approvals, and multimedia rendering are outside scope. Existing textual content and readable tool results remain required even when an entry also carries unsupported media.

Failures include silently dropping transcript content, fabricating unavailable data, duplicate turns caused by replay, broken child links, and discarding a previously indexed conversation because its source is temporarily inconsistent.

## Evidence and compatibility target

The implementation baseline is upstream commit `63d0e8f17e2c1f0d9e1d6f4399ab2dc10fbdf477`, documented locally as Vibe v3.0.2. It uses `mistral.vibe.unified-session-store/v1`, minor 9. These are storage revisions, not a promise of compatibility with every future Vibe 3.x release.

Primary sources inspected:

- [Storage, projection replay, chunk loading, and version rules](https://github.com/mistralai/mistral-vibe/blob/63d0e8f17e2c1f0d9e1d6f4399ab2dc10fbdf477/harness/runtimes/python/python/mistralai_vibe_local_harness/vibe/_storage.py).
- [Public session schema and token usage](https://github.com/mistralai/mistral-vibe/blob/63d0e8f17e2c1f0d9e1d6f4399ab2dc10fbdf477/harness/runtimes/python/python/mistralai_vibe_local_harness/session_protocol.py).
- [Messages, reasoning, effects, and subagent links](https://github.com/mistralai/mistral-vibe/blob/63d0e8f17e2c1f0d9e1d6f4399ab2dc10fbdf477/harness/runtimes/python/python/mistralai_vibe_local_harness/vibe/_projection.py).
- [CLI session resume](https://github.com/mistralai/mistral-vibe/blob/63d0e8f17e2c1f0d9e1d6f4399ab2dc10fbdf477/README.md).
- [Legacy migration and resume identity](https://github.com/mistralai/mistral-vibe/blob/63d0e8f17e2c1f0d9e1d6f4399ab2dc10fbdf477/harness/runtimes/python/python/mistralai_vibe_local_harness/vibe/_host.py).
- [Subagent identity and lifecycle](https://github.com/mistralai/mistral-vibe/blob/63d0e8f17e2c1f0d9e1d6f4399ab2dc10fbdf477/harness/runtimes/python/python/mistralai_vibe_local_harness/vibe/_subagents/_models.py), [deterministic child IDs](https://github.com/mistralai/mistral-vibe/blob/63d0e8f17e2c1f0d9e1d6f4399ab2dc10fbdf477/harness/runtimes/python/python/mistralai_vibe_local_harness/vibe/_subagents/_operations.py).
- [Large-result persistence](https://github.com/mistralai/mistral-vibe/blob/63d0e8f17e2c1f0d9e1d6f4399ab2dc10fbdf477/harness/core/src/core/features/large_output/mod.rs) and [host/sandbox output copies](https://github.com/mistralai/mistral-vibe/blob/63d0e8f17e2c1f0d9e1d6f4399ab2dc10fbdf477/harness/runtimes/python/python/mistralai_vibe_local_harness/vibe/_saved_outputs.py).

The public projection includes session token totals, visible reasoning entries, and subagent effects with explicit `childSessionId`. The current loader reconstructs the projection by applying the journal to the published generation. An interop export is derived on demand and is unavailable for recoverable generations; a standalone export file is therefore unsuitable as the primary source.

Support stores written by Vibe 3.0.0, 3.0.1, and the pinned 3.0.2 baseline. Inspection of `_storage.py` at tags [v3.0.0](https://github.com/mistralai/mistral-vibe/blob/v3.0.0/harness/runtimes/python/python/mistralai_vibe_local_harness/vibe/_storage.py) and [v3.0.1](https://github.com/mistralai/mistral-vibe/blob/v3.0.1/harness/runtimes/python/python/mistralai_vibe_local_harness/vibe/_storage.py) confirms that both also set `STORE_FORMAT_MINOR = 9`. There is no evidence that supporting those releases requires accepting a different minor.

Minor 9 is mandatory, with fixtures covering the three release shapes, full projection replacements, and deltas. Lower minors are accepted only when their required structures and semantics are implemented and covered by fixtures; there is no blanket acceptance based solely on a smaller number. Newer minors and unsupported required schema versions produce an explicit diagnostic. Pre-3.0 experimental Unified revisions are outside the mandatory release scope. The release documents the exact fixture-backed revision range.

The local format reference has been corrected: upstream comments assign minor 8 to pooled capability catalogs and pruning settled action bodies, and minor 9 to atomic relocation and system-instruction baselines. The earlier description of minor 9 as introducing pooled chunks was inaccurate.

## Approaches considered

| Approach | Benefits | Trade-offs |
|---|---|---|
| Persisted public projection plus journal updates — selected | Closest to what Vibe displays; already structures reasoning, effects, and child links | Requires faithful snapshot/chunk loading and ordered projection replay; runtime metadata still needed |
| Checkpoints plus internal execution events | Access to core execution data | Reimplements substantially more engine logic; greater risk of duplicate or different transcript content |

The selected approach fits a read-only conversation browser and keeps engine recovery outside the application.

## Discovery and component boundaries

Keep the existing source root resolution, `VIBE_HOME` handling, Flatpak permissions, and `--sessions-dir` fixture convention. This work does not expand configuration discovery beyond current source resolution.

Enumerate legacy session directories and `<source-root>/unified/<session-id>/` together. Discover Unified candidates by their `CURRENT` marker; ignore catalog/cache files, `scratchpad`, and staging directories. A Unified child may have no `meta.json`, so that file is not an eligibility requirement. A candidate with missing or invalid required files is a failed/deferred candidate, not an empty session.

Keep the legacy reader separate. Introduce a Unified reader under `crates/core/src/parsers/` that returns `ParsedSession`, with internal responsibilities for store loading, projection replay, and model conversion. Integrate discovery and freshness checks in `crates/core/src/database/indexer.rs`. Exact file subdivision is left to the implementer.

Session identity is the persisted session ID; `file_path` remains its session directory. Reject conflicting identities between the directory, pointer, manifest, and loaded state. Never merge two different source directories claiming the same ID without a diagnostic; preserve an existing indexed record until the conflict is resolved.

### Legacy migration and imported histories

Upstream `_host.py::_import_legacy` creates a new Unified session ID and records the original legacy ID in `import_provenance.source` with `backend: "legacy"`. `_resolve_legacy_import_id` uses that provenance when resuming an old ID. These are two persisted sessions with distinct identities, not an ID collision.

Author's scope choice: index both stores under their own IDs, retaining the historical legacy record and the continued Unified record. Their shared imported prefix may appear in both search results, just as copied/forked conversations can. Do not deduplicate by text, hide a readable legacy source, move its favorite, or silently redirect an existing deep link. Resuming the legacy ID may open the migrated Unified session according to Vibe's own resolver. No new lineage UI or alias system is required for this release.

Guard genuine same-ID/different-directory collisions before the SQL upsert, which currently replaces `file_path` on ID conflict. Diagnose those cases and keep the prior record. Add migration coverage for a legacy source imported under a new ID and subsequently continued; neither indexed transcript may contain duplicate turns introduced by our replay.

## Reading a coherent store

1. Read and validate `CURRENT`, the supported format revision, and the referenced generation manifest.
2. Load the referenced projection document and its ordered chunk references. Load the runtime envelope fields needed for metadata and identity; the pooled capability catalog is unnecessary. Follow the manifest rather than guessing filenames or selecting the newest directory.
3. Sample the selected journal identity/size/mtime before reading. Stream it with `BufReader` and newline-aware iteration, beginning after the snapshot sequence. Check sequence and hash-chain semantics. Ignore an unterminated final fragment, even after a crash, and publish the complete validated prefix. A newline-terminated malformed record, including the last complete record, is corruption. Never rewrite or truncate the source.
4. Apply public projection changes in journal order. Handle full projection replacement, `append_entry`, `replace_entry`, `remove_entry`, `set_history_entries`, and `set_envelope`. Include `session_relocated` records and their metadata. Historical model changes come from projection checkpoints, not an invented runtime journal event.
5. Recheck `CURRENT` before committing. Retry once if the selected generation changed or a publication race made a referenced file disappear; defer if instability persists. Journal append growth alone does not invalidate a validated prefix. Commit the journal signature sampled before reading, so a concurrent append triggers a later pass. A journal replacement or shrink invalidates the read; handle it like a generation race.

### Integrity rules

Use SHA-256 of stored bytes, never of reserialized JSON. For manifest, referenced documents, and chunks, remove exactly the required final newline before hashing, matching `_read_document_body`; chunks are not hashed with that newline. A missing required document newline is invalid. Validate the manifest hash from `CURRENT` and the digests of every document/chunk consumed.

For a journal record, remove its terminating newline, locate the last byte sequence `,"record_sha256":"`, and remove that field including its 64 hex digits and closing quote before hashing the remaining bytes, matching `_journal_record_digest`. Check the parsed digest, sequence, and previous-record digest; the first segment record chains from `null` as upstream specifies.

Separate canonical-form validation from digest verification: upstream `_read_document_bytes` also checks canonical JSON for `CURRENT` and the manifest. The reader must retain this validation where upstream requires it, using a conforming implementation or an independently validated equivalent. Ordinary Rust JSON serialization is insufficient evidence of RFC 8785 compliance. Referenced documents/chunks need no parse-and-recanonicalize step to verify their hashes. Include byte-level vectors with Unicode keys, numeric values, and newline handling.

Preserve ordered entry lists. Entry IDs are not necessarily unique: match upstream replacement/removal semantics, and never deduplicate the complete history by ID. `set_envelope` replaces metadata without erasing history; `set_history_entries` replaces history without concatenating old entries. Replaying a session must be idempotent.

Read only files referenced by the selected store state and the metadata needed for linkage. Do not load the whole journal into memory. Holding the reconstructed projection and `ParsedSession` in memory is acceptable, as with other parsers; avoid retaining historical snapshots or duplicate chunk payloads. File references must remain within the source session directory, with explicit shared-root handling only if demonstrated by the supported upstream schema. Reject traversal and escaping symlinks.

Unknown additive fields can be ignored. At the supported revision the journal record union is closed: validate all 14 known record kinds and their common integrity envelope. Apply `projection_advanced`, `projection_delta`, and `session_relocated`; other known kinds can be ignored for transcript reconstruction after identifying them. An unknown record kind, projection operation, required entry type, or state-bearing variant must fail that candidate explicitly rather than yield a successful partial transcript. Do not classify an unknown record as irrelevant merely because it lacks a recognized projection field.

## Mapping to existing domain data

| Capability | Unified mapping and behavior |
|---|---|
| Messages and search | Convert nonempty textual user/assistant `message` entries in source order to `Message` and `TranscriptItem`; preserve the displayed text for FTS. Prefer a valid `userDisplayContent` over injected user content, using the upstream display schema; otherwise use `content`. `turn_steer` remains user input in its original position. Apply existing system-message filtering. |
| Tool calls | Convert tool `effect` entries using structured detail/input and state/output fields. Preserve the qualified tool name for display. Extend classification with fixture-backed built-in group/name mappings so read/edit/command counts remain correct; never strip arbitrary connector namespaces based only on a familiar suffix. Prefer readable output text; serialize structured output when no readable text exists. Do not emit a second activity for the same effect. |
| Tool status | Map `running`/`completed` to existing equivalents and `failed`/`cancelled`/`skipped` to `Error`, preserving reason/error/output text. `pending`, if present in a separately supported schema, maps to `Pending`; it is not emitted by the pinned projection. Unsupported status variants are diagnosed. No skipped or cancelled operation appears successful. |
| Reasoning | Convert visible text/summary to existing reasoning attachments using the next related assistant message/tool/subagent item, consistent with current conventions. Keep timestamps and respect the discarded-content policy below. Exact upstream sentinel `text: "[redacted]"` maps to `has_encrypted_content = true`, `visible_text = None`; preserve any accompanying readable summary. Do not display the sentinel as recovered reasoning. |
| Model | Normalize the session model using existing helpers. Process `checkpoint` entries with `kind: "model_change"` and `details.model` in history order; apply the new model only to subsequent assistant messages. Messages before the first change remain unknown. If history has no model change, the current session model is the legacy-compatible fallback. User messages have no model. |
| Project and time | Prefer effective runtime metadata for current project and identity; use matching `meta.json` as supplementary metadata. Empty `cwd` means unknown. Use projection `createdAt`/`updatedAt` in milliseconds for entry/session times. Missing optional values remain unknown; required identity/time failures are diagnosed. |
| Tokens | Read cumulative session `tokenUsage`, not `contextUsage`. Map input/output and cached input to existing counters; leave unavailable cache-write/reasoning counters unset. Do not sum cumulative snapshots or add child usage to parent totals. |
| Ending state and activity | Extend indexer insertion to honor an explicitly derived Unified ending status; it currently overwrites parser status using the last tool call. Latest-turn failure maps to `Error`, interruption or persisted running/blocked state to `Abrupt`, and a completed latest turn to `Clean`; use the existing tool-call heuristic when no authoritative turn state is available. Preserve existing behavior for other parsers. |
| Resume | Retain root/fork session ID and working directory for the existing terminal command. Validate `vibe --resume <id>` against the target CLI. Disable resume for Unified subagent records: upstream requires them to be resumed through their parent. Extend `Session::can_resume` with source-format-aware capability data or an equivalent scoped check, without disabling legacy Vibe child resume by assumption. |

### Entry policies and rejected content

Author's scope choice: omit assistant messages and reasoning explicitly marked `outcome.type: "discarded"` from the visible transcript and FTS. This prevents a rejected or superseded candidate from reading as an accepted answer without adding new presentation states. This is an intentional treatment of uncommitted content; it must not remove committed neighboring messages or tool effects. Drop empty textual entries. Omit provisional `in_progress` assistant text/reasoning until commitment, while still indexing the remaining valid entries; never turn it into a completed answer. Tool effects in progress remain visible with their actual status.

All known public history entry types are handled explicitly:

| Entry | Treatment |
|---|---|
| `message`, `reasoning`, `effect` | Convert as above; distinguish subagent effects from ordinary tool effects. |
| `checkpoint / model_change` | Update subsequent model attribution; no synthetic conversation message. |
| `checkpoint / agent_change` | Recognize and omit profile-change bookkeeping, which legacy transcript presentation does not expose. |
| `checkpoint / compaction` | Recognize lifecycle metadata and omit the bookkeeping row. The pinned public entry carries summary length, not summary text; do not fabricate a compaction summary from it. Existing visible history remains intact. |
| `checkpoint / recovered` | Recognize the source's repaired-history marker and log its recorded repair details as a warning. Index the history actually retained by Vibe; do not reconstruct discarded source history or treat source recovery as reader corruption. No new recovery UI is required. |
| `notice` | Recognize hook notices and omit this operational bookkeeping from conversation messages/FTS; associated user text, effects, and failures are still mapped normally. |

An unsupported checkpoint kind or malformed required entry shape follows the explicit unsupported/corruption policy, rather than accidentally failing every session with a known bookkeeping entry.

### Large tool results

The pinned core can replace a large tool result with a receipt containing a truncated preview and the path to its saved JSON. The original serialized result is saved under `tool-results/<encoded-tool-name>-<encoded-call-id>.json`; sandbox copies may have a different absolute path. A readable preview alone does not satisfy full-result parity when the original saved result is available locally.

Resolve receipts to the source session's local saved result using the pinned filename encoding and the originating call identity, or an equally verified local reference. Never follow arbitrary absolute paths embedded in result text or fetch remote resources. Read and render textual content from the saved result, keeping error status and receipt context. Validate paths and JSON; include needed saved-result presence/size/mtime in freshness inputs. A missing or malformed advertised local result produces a recoverable diagnostic and preserves the previous indexed version; do not silently claim a full successful import of a truncated preview. The implementation plan must prove the receipt-to-local-file association on host and sandbox fixtures before claiming parity.

An empty-but-valid session with no user messages follows the existing skip policy. Reasoning that has no subsequent related visible transcript item follows the legacy policy: omit the orphan attachment with a debug diagnostic, without inventing an assistant message. This is an existing presentation limitation, not a new standalone reasoning feature. Unsupported nontext media does not invalidate otherwise readable text, but omitted media must follow existing transcript conventions rather than become fabricated content.

## Subagents

Index sibling child stores under `unified/` using their runtime identity and projection metadata, including children without `meta.json`. Distinguish subagent identity from fork/reset lineage; `parent_session_id` alone is insufficient proof.

Use subagent effects and explicit `childSessionId` for parent navigation. This field is populated by result annotations and may be null while spawning or after a failure. As a fallback before annotation, use the pinned deterministic `child_session_id(parent_id, action_id)` rule only when the spawn action identity is available and the loaded child's runtime identity confirms that exact parent and `subagent_spawn_key`; never match by agent name or directory order. Without sufficient evidence leave the effect unlinked. Populate agent name/type, prompt, and result summary from persisted effect/child information where available. Use existing `Subagent` transcript items for spawn effects. Keep other agent operations as ordinary tool calls unless the existing domain has a faithful representation.

Build the set of successfully indexed child IDs before publishing navigable parent links. A missing, failed, or not-yet-written child must not create a dangling link; the parent effect remains visible. Retry linkage when the child becomes readable. The pinned schema allows one level of subagents (`depth: 1`), owned by their root; reject deeper nesting and cycles. Legacy nested-child support is retained independently. A Unified fork is an independent top-level session (`is_subagent = false`); its source lineage must not populate the domain's subagent-parent field. Refresh affected parents when a child appears or disappears. Deletion cleanup follows explicit indexed relationships rather than legacy directory nesting.

Do not probe or acquire Vibe's session leases/lock files to infer liveness. A persisted running/blocked projection can represent either active work or a crashed writer; preserve its recorded effect states and classify its ending as `Abrupt`, without fabricating cancellation. Recognize `storage_lifetime: "ephemeral"` as nonpersistent and do not index it. For `lifecycle.type: "deleting_tree"`, hide/remove that confirmed deleting root and its owned children atomically; do not leave navigable links to them or try to finish Vibe's deletion. A malformed/unreadable lifecycle is a read failure, not proof of deletion.

## Incremental indexing and failure behavior

A Unified session freshness signature includes `CURRENT` content, selected journal identity/size/mtime sampled before reading, optional `meta.json` changes, required saved-result file metadata, and child availability that affects navigation. Do not fingerprint the directory mtime alone or rely on Vibe's catalog cache. Immutable generation/chunk data is validated when read; silent external modification without freshness changes is outside incremental guarantees and is caught by a full reindex.

Update session rows, transcript data, FTS, and the successful freshness signature atomically. Failed/deferred reads must not mark the source successfully indexed. Preserve user favorites and other existing user-owned state through replacement.

If a required file disappears during publication or a generation changes repeatedly while reading, retain the previous database snapshot and retry on a later pass. An incomplete final journal fragment alone is not a failure: publish the valid prefix as specified above, including for sessions left after a crash. A malformed complete record or hash/sequence failure is an explicit corruption diagnostic; do not skip it and publish a misleading suffix. Unaffected sessions continue indexing.

Integrate unsupported-format/corruption diagnostics with existing indexing status surfaces. There is no existing cross-pass retry accounting to rely on: add a small in-memory per-source-path deferral counter for publication races, owned by an explicit per-database indexing context that the indexing worker retains across its per-request indexer recreation — never by a process global; independently created contexts never share counters. After the single immediate retry, defer the first failed pass quietly; report an indexing diagnostic on the second consecutive failed pass and subsequent failures. Only strictly consecutive unstable outcomes accumulate: any other observed outcome or diagnostic for the path — a successful index or current-signature skip, a same-ID collision, a non-unstable parser/diagnostic result such as corruption, an unsupported format, or a missing `CURRENT` in an indexed directory — or a confirmed deletion breaks the sequence, so the following unstable outcome is deferred quietly as a new first occurrence. Reset the counter on any such observed outcome or confirmed deletion, and clear a context's counters only after its `clear_all_sessions` transaction commits; a failed transaction leaves them untouched. Restarting the application resets these transient counters. Unsupported versions, corruption, and missing required saved outputs are diagnosed immediately.

Confirm physical deletion only after a successful enumeration of an accessible source root shows that a previously indexed session directory is absent. An unreadable/missing source root, absent `CURRENT` inside an existing directory, or transient missing dependency does not prove deletion. An absent pointer in a surviving directory is diagnosed as an invalid/incomplete store. The explicit readable `deleting_tree` lifecycle above is also authoritative deletion evidence. Clear removed child links in the same update.

Use existing fingerprint infrastructure where sufficient; a scoped persisted Unified signature/schema migration is allowed if necessary. Invalidate affected Vibe freshness records when parser semantics change without deleting user data or forcing unrelated assistants to reindex. The exact schema design and location of the specified deferral counters are implementation decisions.

## Acceptance and validation

Release requires fixture-backed parity for the target revision and legacy nonregression. Use sanitized, source-faithful fixtures with valid hashes, never unsanitized private logs. Derive expected projection results from the pinned upstream behavior or captured public state independently of the Rust implementation.

Cover:

- Legacy and Unified sessions under the same source root; fixture overrides; ignored scratch/catalog/staging data; source-faithful release shapes for 3.0.0, 3.0.1, and 3.0.2.
- Legacy migration under a new ID followed by Unified continuation, preservation of both source records/favorites/deep links, and rejection of unrelated same-ID directory conflicts.
- Inline and chunked history, a published generation followed by journal updates, generation rollover, and replay without duplicate content.
- Every supported projection operation, duplicate entry IDs, history replacement/removal, and relocation metadata.
- Text/display-content precedence, steering messages, empty entries, committed/discarded/provisional candidates, tool inputs/results and all supported statuses, qualified built-in tool classification and analytics, visible/redacted reasoning, model-change checkpoints, known checkpoint/notice entries, timestamps, token totals without double counting, and database full-text search.
- Large-result receipts with full saved JSON on host and sandbox, safe receipt resolution, missing/restored outputs, and freshness changes limited to saved-result files.
- Child stores without metadata files, explicit and verified deterministic linkage, pending/missing/corrupt children, roots/forks with their own depth-one children, invalid deeper nesting/cycles, child resume disabled, deletion lifecycle, and parent refresh after child availability changes. Legacy nested children remain covered by legacy tests.
- Valid prefixes with unterminated tails, continuously growing journals, changing pointers, missing referenced files, raw-byte digests and canonical-envelope validation, malformed complete records, unknown required operations, unsupported revisions, failure-counter reset/reporting, and preservation of the previous indexed data on failure.
- Incremental no-op passes, journal-only changes, metadata-only changes, favorite preservation, source deletion, and one bad session alongside readable sessions.
- Manual headless-compatible fixture inspection of existing transcript rendering and an explicit root-session resume check with the target Vibe version. No production UI redesign is expected.

Run repository checks during implementation: `cargo fmt --all -- --check`, `cargo clippy --all -- -D warnings`, and `dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --all --no-fail-fast`. Packaging changes, if needed, additionally require Flatpak build verification. The original design phase changed documentation only; implementation verification and remaining release gates are recorded above.

## Deliverables and implementation discretion

Deliver the Rust store reader/converter, discovery and incremental indexing integration, sanitized fixtures and meaningful parser/indexer tests, and refreshed format/development documentation. Reuse existing UI and diagnostics.

The implementer chooses internal module organization, cache structures, signature persistence, and helper extraction. Before coding, the implementation plan must pin exact serialized fields for effect/reasoning conversion, user display content, child action/spawn identity, receipt-to-local-result resolution, qualified built-in name mappings, and byte-level integrity checks using the referenced sources. Account explicitly for indexer ending-status precedence and source-format-aware resume eligibility. Any discovered absence of data required for parity is a design blocker to raise explicitly, not permission to silently reduce scope.

## Design review

The initial builder review missed substantive issues subsequently identified by an independent Claude Code review. This revision corrects journal-prefix semantics, byte-level digests, known entry handling, model attribution, qualified tool classification, ending-status insertion, resume eligibility, source lifecycles, and deferral diagnostics.

Additional upstream inspection confirms that legacy migration allocates a new ID, Unified children have depth one, and all three target 3.0.x releases write minor 9. The review's concern that 3.0.0/3.0.1 might require other minors is therefore not confirmed. Its advice to remove all canonical-form validation is too broad: `_read_document_bytes` explicitly performs that validation for pointer/manifest documents, separately from raw-byte digest checks. Chunk hashes also exclude their required final newline.

Author's product choices in the design revision were to retain legacy and migrated Unified stores as separate sessions, omit discarded assistant candidates from transcript/search, and keep operational checkpoint/notice bookkeeping out of conversation messages. These choices are implemented in PR #211. Full local saved-result recovery remains required; host and sandbox fixture coverage was added during implementation. This record does not assert completion of the outstanding manual release gates.

A subsequent independent consistency review of the design revision found no important internal contradictions or unresolved product decisions blocking planning. That was a design consistency check, not implementation verification. Later implementation-review results and the remaining deviations are recorded in the implementation status section above.
