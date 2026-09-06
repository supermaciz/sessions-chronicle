# Missing sources: preserve the record and make it discoverable

**Issue:** [#195 — Deleted Claude Code transcripts leave stale session rows in the database](https://github.com/supermaciz/sessions-chronicle/issues/195)  
**Date:** 2026-09-06  
**Status:** Exploration — proposal A recommended; UI choice pending review  
**Scope:** Claude Code, OpenCode, Codex, Mistral Vibe, and Kimi Code  
**Direction:** Preserve indexed sessions with `source_deleted` or an equivalent state instead of automatically deleting them.

## Problem and scope

The issue originally proposes deleting sessions whose Claude Code transcripts disappeared. The [comment by grimdalltech](https://github.com/supermaciz/sessions-chronicle/issues/195#issuecomment-5354802262) proposes preserving the evidence instead: a missing-source state, the last known fingerprint, and observation timestamps. This exploration adopts that direction and supersedes the issue's original cascade-deletion acceptance criteria.

**This is a shared capability for every supported AI assistant, not a Claude Code-only fix.** A source can be a transcript file, a session directory containing several files, or a session record in a shared database. The application should expose one consistent state while each storage adapter determines whether the corresponding source still exists.

A session can lose its source and remain useful in the local index. Users need to understand what is missing, read retained content, and deliberately find these sessions. Missing sessions must also stop distorting ordinary results and active-session linkage.

**Shared mockup scenario:** successful source scans find three previously indexed sessions missing: one from Claude Code, one from OpenCode, and one from Kimi Code. Two belong to `sessions-chronicle`. The user wants to reread a parser investigation. All titles, paths, and dates are illustrative. Mockups show the same UI across storage formats; file-specific metadata in the shared detail is one example.

### Existing implementation

The paths referenced in the issue have changed:

- [`crates/core/src/database/indexer.rs`](../../crates/core/src/database/indexer.rs) contains `prune_orphan_fingerprints` and OpenCode-specific stale-session pruning. Preserve source evidence **before** fingerprint cleanup; replace disappearance-driven hard deletion with the shared retention policy.
- [`crates/core/src/database/indexer/kimi.rs`](../../crates/core/src/database/indexer/kimi.rs) also prunes stale Kimi bundles. Its disappearance path needs the same policy, including retained child sessions.
- [`crates/core/src/database/schema.rs`](../../crates/core/src/database/schema.rs) stores file fingerprints based on `mtime_ns` and `size`, not content hashes.
- [`src/ui/session_detail.rs`](../../src/ui/session_detail.rs) loads transcript items from the database with `load_all_transcript_items`. A missing source does not inherently prevent reading already indexed content. This is useful retained content, not a promised complete backup.
- The existing [`session_list.rs`](../../src/ui/session_list.rs), [`session_row.rs`](../../src/ui/session_row.rs), and [`session_summary.rs`](../../src/ui/session_detail/session_summary.rs) provide the main UI surfaces. The sidebar already filters by AI assistant, project, and pin; the header offers date and sorting controls.

## Shared behavior across all three proposals

### Describe observations accurately

Use **“Source missing”** in the UI. `source_deleted` is acceptable internally, though `source_missing` better describes the observation. “Deleted on…” would claim knowledge of an action and timestamp that the indexer does not have. A source may have moved, or a database record may have disappeared while its database remains present.

Source details show **“Absence detected”**, **“Last seen”**, the source location, and the last retained source metadata. Full dates include the timezone; unavailable values display “Unknown”. Do not derive last-seen time from conversation activity or file modification time. The cause and exact deletion time remain unknown. This local record is not a tamper-proof audit log or evidence of malicious intent.

### Preserve without counting as active

| Surface or operation | Proposed behavior |
| --- | --- |
| Default internal list and search | Exclude confirmed missing sessions; provide an explicit mode to include them. |
| GNOME search | Exclude missing sessions from results, excerpts, and counts. Activating an old result still opens retained detail. |
| Pins | Preserve the pin; visibility and counts follow the availability filter. |
| Navigation counts | Count sessions in the displayed scope; distinguish available and missing when both are included. |
| Teammate resolution | Exclude missing candidates from ambiguity checks. Preserve existing historical links and label missing destinations. |
| Analytics | Exclude missing sessions initially; label the scope “Sessions without confirmed missing sources”. Inclusive historical analysis is a separate product choice. |
| Transcript reading and search | Use indexed content without attempting to reload a missing source. |
| Resume in terminal | Disable in the row, menu, and detail; enforce the restriction in the action handler too. |
| Deep links, child-session links, already open detail | Open or retain the detail with its source state instead of a generic error. |

An unsuccessful scan preserves the previous availability state and surfaces an indexing diagnostic. “Available” is the last known state, not a guarantee of current reachability. Sessions with no successful observation since migration remain included in ordinary browsing, with “Last seen: Unknown”; do not invent evidence of presence or absence.

Detection must not update conversation `last_updated`: disappearance is not new conversation activity. The Date filter continues to use session dates, except in proposal C's explicitly labeled observation timeline.

### Shared retained-content detail

![Shared detail: persistent missing-source banner, retained transcript, and source observations](../mockups/source-deleted/shared-retained-detail.svg)

A persistent banner reads **“Source missing — showing retained content”**, with **“Source details”** as its action. It applies to the open session and remains visible while the source is missing. The [GNOME HIG banner guidance](https://developer.gnome.org/hig/patterns/feedback/banners.html) recommends this pattern for persistent states, with a short title and optional action.

The source-details surface adapts its fields to storage:

| Source kind | Details and copy actions |
| --- | --- |
| Transcript file | Last known path, retained size and modification time; **“Copy path”**. |
| Session bundle | Bundle location and last known required-file metadata; **“Copy path”** copies the bundle path. Identify a missing component when known. |
| Database session | Database location and session ID; **“Copy path”** copies the database location, **“Copy session ID”** copies the record identity. Explain **“Session record missing”** even if the database exists. |

Never represent a shared database's file size or modification time as an individual session fingerprint. File paths may be visually shortened, but accessible and copied values are complete. Pinning and copying retained content remain available. Resume has a visible explanation also associated with the control for assistive technology.

When retained content is incomplete, show **“Only indexed content is available”**. With no retained transcript, show **“No retained transcript”** alongside source observations and metadata, rather than an empty conversation. At narrow widths, source details become a dialog page with explicit back navigation; the banner can wrap.

## Proposal A — Availability filter, GNOME conventions

**Intent:** Find an old conversation through the familiar list and search.

![Proposal A: Sources menu set to All indexed, with a Source missing session row](../mockups/source-deleted/a-gnome-filter.svg)

Add **“Sources: Available”** above the list. Its menu offers three exclusive scopes: **“Available”**, **“All indexed”**, and **“Missing”**. Use radio choices rather than a binary switch, consistent with the distinction in the [GNOME HIG switch guidance](https://developer.gnome.org/hig/patterns/controls/switches.html).

“Available” is the default. Preserve the selection while navigating between list and detail and while searching; reset it on the next launch. Keep the control visible even with no missing sessions, so its location stays predictable. Combine it with project, AI assistant, pins, date, and search without changing the selected sort.

In “All indexed”, missing rows receive a **“Source missing”** status line. Keep titles at normal opacity and avoid strikethrough. A summary distinguishes **“24 available · 3 missing”**, respecting the other active filters. Keep the status on rows in “Missing” too, including their accessible names.

Under the default filter, a quiet **“3 missing sources”** count appears beside the control when matching missing sessions exist; activating it selects “Missing”. When every matching session is missing, the empty state says **“No available sessions”** and offers **“Show missing sources”**. An empty missing-only scope says **“No missing sources”**. Counts always refer to top-level sessions, not files or database containers.

**Adaptive behavior:** Keep the control below the header to avoid competing with date and sort. Move the count summary onto another line at narrow widths. Keep the textual status visible and use theme colors and native controls rather than a red error treatment.

**Strengths:** Limited UI cost, one search flow, and continuity with the current list.  
**Trade-offs:** “All Sessions” in the sidebar means all projects, while “All indexed” means both availability states; this distinction needs clear labeling. Status lines increase affected row heights and require checking recycled-row state.

## Proposal B — A dedicated “Missing Sources” destination

**Intent:** Provide a stable place to inspect retained records across AI assistants.

![Proposal B: Missing Sources sidebar destination listing Claude Code, OpenCode, and Kimi Code sessions](../mockups/source-deleted/b-missing-sources.svg)

Add **“Missing Sources”** with a count near “Pinned”. It opens a dedicated list of missing top-level sessions. The title identifies the destination; the subtitle states its scope. Show observation timestamps per row, rather than implying that all AI assistants were successfully checked at the same time. Sort by most recently detected absence, independently of conversation activity.

Entering this destination resets search and date, but retains project and AI assistant filters and identifies that scope in the subtitle. Project filters remain usable there. Returning to ordinary sessions restores their previous navigation state. The sidebar count follows project and AI assistant filters, not the destination's local search.

Each row names the AI assistant, displays **“detected…”**, and opens the shared detail. **“Indexing Status”** opens existing diagnostics to investigate source errors or request reindexing. There is no Restore action: the application may not possess the original file, bundle, or database record.

**Adaptive behavior:** The entry remains accessible in the collapsible sidebar and the main view retains its title. The empty page explains that no missing sources match the filters, without suggesting cleanup.

**Strengths:** Highly discoverable, clear separation from ordinary browsing, and a persistent count.  
**Trade-offs:** Adds a destination to a recently simplified sidebar and needs separate search/navigation state. Finding an absent conversation requires leaving ordinary search. Despite the destination's name, counts represent sessions, never a mixture of files, bundles, and database records.

## Proposal C — Source Chronicle, a creative alternative

**Intent:** Understand how a project's locally retained history has changed.

![Proposal C: project source timeline with absence observations and actions to read retained content](../mockups/source-deleted/c-source-chronicle.svg)

Above the list, **“Sessions”** and **“Source Chronicle”** offer two views of the selected project. The chronicle groups observations by detection day, using a restrained timeline and one card per event. Each card identifies the session, AI assistant, observed state, and interval between last presence and detected absence. **“Read retained content”** and **“Source details”** lead to the shared surfaces.

“Sessions” excludes missing sessions. A **“2 missing sources”** count opens the chronicle. Under “All Sessions”, the chronicle covers all projects and includes project labels on cards. Search filters events by session title or retained content; date controls explicitly read **“Observation date”** in this mode.

This proposal requires an append-only observation history: absence and subsequent reappearance are separate events. A boolean alone is insufficient. Repeated scans must not create duplicate cards without a state transition. Do not synthesize events for periods before migration. Events from different AI assistants use the same structure, while their source details remain storage-aware.

**Adaptive behavior:** Cards form a vertical list at narrow widths and actions wrap. The timeline spine is decorative; dates, order, and accessible names convey all information without it. An empty chronicle says **“No source changes recorded”**, which does not imply that no earlier deletion occurred.

**Strengths:** Makes preserved evidence tangible and separates conversation dates from observation dates.  
**Trade-offs:** Additional storage, event pagination, and navigation; excessive if rereading a conversation is the main use case. A chronological presentation must not imply audit integrity guarantees.

## Generalized source reconciliation

### One shared policy, storage-specific evidence

Separate **source discovery** from **successful parsing**. A discovered but malformed session is not a deleted session. Each adapter should report its source scope, discovered session identities or locators, successful observations, and which parts of the scope were completely enumerated. These are proposed responsibilities, not existing APIs.

The shared reconciler compares previously observed sources only against the matching completed scope. It records confirmed absence and preserves content. The UI consumes the resulting session state without branching on the AI assistant.

| AI assistant / storage | Source identity and evidence | Proposed absence rule |
| --- | --- | --- |
| Claude Code | Session identity within its configured root; JSONL transcript locator and file metadata. | Previously observed transcript absent from a complete scan of its owning scope. A parse failure is not absence. |
| Codex | Session identity within its configured root; JSONL rollout locator and file metadata. | Same file-source rule. Match rediscovered identities before marking old locators missing, so a move within the scanned scope does not create a false deletion. |
| OpenCode — JSON | Session identity within its storage root; session metadata and associated message/part storage. | Session identity absent from successful session enumeration. Missing message/part data with a surviving session is an incomplete-source diagnostic, not whole-session deletion. |
| OpenCode — SQLite | Database scope plus session record ID; last successful observation and any supported record-level revision metadata. | Record absent from a successful, consistent enumeration of that database. An inaccessible, missing, or unreadable database makes the scope unavailable; it does not confirm deletion of every contained session. |
| Mistral Vibe | Session directory identity with `meta.json` and `messages.jsonl`; child sources under `agents/`. | Previously observed session directory absent from a complete parent-scope scan. A surviving directory with missing required components produces an incomplete-source diagnostic. |
| Kimi Code | Session bundle identity; `state.json`, `agents/main/wire.jsonl`, and resolved child dependencies. | Previously observed bundle absent from a complete workspace scan. Missing required files in a surviving bundle are incomplete-source diagnostics, not whole-bundle deletion. |

The bundle rules deliberately distinguish **a missing session** from **a damaged or partially written source**. Incomplete-source diagnostics preserve content and availability history; they do not silently enter “Missing”. This keeps a shared boolean meaningful without adding unrelated UI states to #195.

A child source can disappear independently. Reconcile independently stored children using their own verified source scope. When an entire owned bundle disappears, mark the sessions whose sources belong to that bundle missing while retaining their relationships and content. Do not cascade absence through logical parent links to independently stored sources that still exist. Top-level list counts exclude child sessions; child links still expose their own source state.

OpenCode and Kimi already have disappearance-driven pruning paths. Replacing those paths is part of this cross-assistant change, not a later extension. Audit child replacement and parser-skip cleanup too: distinguish intentional eligibility changes from missing source data, and never let generic content replacement erase an already retained missing-source record.

### Persistence and transitions

| Proposed data | Purpose |
| --- | --- |
| `source_missing` boolean, default false | Shared confirmed-absence marker; equivalent to the suggested `source_deleted`. |
| `source_missing_detected_at`, nullable | First detection in the current absence period; unchanged by repeated scans. |
| `source_last_seen_at`, nullable | Last actual presence observation, independent of conversation activity. |
| Source identity and locator | AI assistant, storage kind, owning root/database scope, and native session identity; path alone is insufficient. |
| Last known evidence snapshot | File metadata for files, dependency metadata for bundles, record identity and supported revision metadata for database sessions. Unknown values remain null. |
| Last absence and return observations | Preserve the latest completed absence period for A/B; C stores every transition as an event. |

Source identity must be scoped: the same native ID in fixtures and real data must not alias. Check existing schema uniqueness and source-to-session mapping before choosing the migration. If necessary, introduce a source-instance mapping rather than overloading the conversation ID. Do not reuse a shared database fingerprint as a session-level fingerprint or require a fabricated hash where none exists.

Migration must not mark existing sessions missing immediately. Reconcile only after complete, successful enumeration of the relevant scope. An inaccessible root, permission denial, I/O error, disabled source, or partial scan does not prove deletion. Preserve the previous state and report diagnostics; a global `Path::exists()` check is insufficient. If a subtree fails, do not infer absence within it.

Fixture scans must never modify real-source observations, and vice versa. Snapshot evidence and mark absence transactionally before cleaning the indexing cache. Retain messages, transcript items, child sessions, and search data. Ordinary reindexing must preserve these records; explicit removal belongs to a separate retention policy.

When the same scoped identity reappears, successfully reindex it before making a missing session active again. An unreadable return preserves the previous missing state with a diagnostic. A different identity at the same path must not overwrite the old record. Resolve same-identity moves within a completed scope before recording absence; moves across scopes must not be automatically merged without identity evidence.

### Implementation surfaces

- Schema, indexer, storage adapters, and shared queries in `crates/core/src/database/`: scoped reconciliation, retention, fingerprints, availability predicates, and teammate ambiguity checks.
- Session models in `crates/core/src/models/`: carry source state and filter selection independently of UI presentation.
- List, row, detail, and summary in `src/ui/`: textual state, storage-aware source details, and retained-content reading.
- Handlers in `src/app/handlers/`, especially `resume.rs`: enforce source state even if it changed after a menu opened.
- System search in `crates/core/src/database/shell_search.rs` and analytics: apply their explicit active-session scope.

## Implementation acceptance checks

1. For **each supported AI assistant and both OpenCode storage modes**, index a fixture, remove the session's authoritative source, and rescan: retain content and evidence, mark absence, and decrement active counts. A repeated scan must preserve the first detection time.
2. For SQLite, remove one session record while keeping the database and other records: only that session becomes missing. Database unavailability must not mark all sessions missing.
3. For Mistral Vibe and Kimi Code, distinguish whole-bundle removal from missing required components. Cover independently missing children and surviving independent child sources.
4. Alternate real and fixture roots; test identical native IDs in separate scopes, inaccessible roots, partial scans, permission errors, and malformed but discovered sources. No false absence or cross-scope mutation.
5. Reintroduce the same identity, move it within a scope, return unreadable data, and replace a path with another identity. Reactivate only after a successful matching reindex; preserve old evidence where appropriate.
6. Check internal search, GNOME search, pins, pagination, analytics, and navigation counts. A missing teammate must not make a present candidate ambiguous.
7. Open missing sessions through the list, deep links, and child links. Cover complete, partial, and absent retained transcripts, plus disappearance during reading. Verify storage-specific copy actions and disabled resume.
8. Check keyboard navigation, screen readers, large text, dark mode, and high contrast; never encode state through color alone. Follow the [GNOME HIG accessibility guidance](https://developer.gnome.org/hig/guidelines/accessibility.html).
9. For C, verify no duplicate events and preservation of multiple absence/return cycles across different AI assistants.

These checks belong to implementation. This change delivers an exploration and static SVGs, not screenshots of running widgets.

## Comparison and recommended decision

| Criterion | A — GNOME filter | B — Dedicated destination | C — Creative chronicle |
| --- | --- | --- | --- |
| Find a conversation | Direct, same search | Switch destination | Through an event |
| Discover missing sources | Contextual count | Persistent entry | Count and project history |
| Fit with existing UI | Strong | Moderate | Moderate to low |
| Multiple disappearance/return cycles | Latest observation | Latest observation | Explicit history |
| Relative UI cost | Low to medium | Medium | High |

**Recommendation: implement A with the shared detail and generalized reconciliation across all five AI assistants.** The storage adapter work is common to all three proposals; A keeps the presentation focused while making retained records discoverable through existing browsing paths. Its contextual count prevents preservation from becoming invisible.

B is a strong alternative if users inspect missing sources frequently. C deserves a separate UI effort if source-change history becomes a central use case. The data decision is to retain absence evidence for every supported source; the recommended presentation is A, pending review. Neither automatic record deletion nor restoration of original source data is required.
