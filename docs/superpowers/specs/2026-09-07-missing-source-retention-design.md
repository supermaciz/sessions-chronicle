# Missing-source retention — design

**Date:** 2026-09-07  
**Status:** Approved design  
**Scope:** Claude Code, OpenCode JSON and SQLite, Codex, Mistral Vibe, and Kimi Code

## Goal

When a successfully indexed session source is no longer present, retain its
local record and indexed transcript instead of deleting it. The session stays
visible and readable, but the product clearly communicates that its source is
missing and prevents terminal resume. This design implements proposal A from
the [exploration](../../explorations/2026-09-06-source-deleted-exploration.md):
marked rows only. It deliberately adds neither an availability filter nor a
sidebar destination.

## Non-goals

- Restoring original transcript files, bundles, or database records.
- Treating malformed or incomplete sources as confirmed disappearance.
- An audit timeline of every disappearance and return (proposal C).
- A dedicated missing-source navigation item (proposal B).
- Making session identity globally unique across roots. `sessions` is keyed on
  `id` alone and `SESSION_UPSERT_SQL` resolves `ON CONFLICT(id) DO UPDATE`, so
  two sources sharing a native ID in different roots already cannot coexist:
  the last indexed one wins. That predates this change and is unaffected by it.
  Fixing it means re-keying `sessions` and every reference to it
  (`parent_session_id`, `subagents.child_session_id`, `transcript_items`,
  `messages`, deep links, shell search) — a separate migration unrelated to
  #195. This design owes the narrower guarantee stated under **Scope
  representation**: a collision must never produce a *false absence*.

## Current removal paths

The work is not uniform across assistants. Only three code paths delete a
session row when its source disappears:

| Path | Assistant | Behavior today |
| --- | --- | --- |
| `prune_stale_opencode_sessions` | OpenCode (JSON and SQLite) | Deletes every indexed OpenCode session whose ID is absent from the run's indexed set. |
| `prune_stale_kimi_bundles`, `prune_kimi_no_user_bundle` | Kimi Code | Deletes sessions of bundles absent from the workspace scan. |
| `prune_deleted_vibe_children` | Mistral Vibe | Deletes child subtrees under `<session>/agents/` using a bare `Path::exists()` check. |

Everything else survives disappearance already, without detection or marking:

- `prune_orphan_fingerprints` removes rows from `file_fingerprints` **only**;
  it never touches `sessions`.
- Claude Code, Codex, and top-level Mistral Vibe session directories therefore
  have **no** disappearance-driven removal at all. Their stale rows linger
  silently, which is exactly the symptom reported in #195.

So this change **replaces** removal for OpenCode, Kimi Code, and Vibe children,
and **adds** net-new detection for Claude Code, Codex, and top-level Vibe
sessions: after a complete walk, compare the previously indexed sources of that
scope against the identities the walk discovered.

`prune_session_after_parse_skip` is a different concern and stays deletion. A
source that is present but no longer eligible (the v17 migration pruned Claude
Code transcripts whose only assistant events were synthetic) is not a missing
source. It must never fire against a retained missing-source row, and the same
audit applies to Kimi bundle replacement and Vibe subtree removal.

## Data model and reconciliation

The `sessions` data carries a shared source state:

| Field | Meaning |
| --- | --- |
| `source_missing` | Confirmed absence of the current session source; defaults to false. |
| `source_missing_detected_at` | First confirmed absence in the current absence period; repeated scans do not change it. |
| `source_last_seen_at` | Time of the last successful presence observation, independent of conversation activity. |
| `source_kind` | Storage shape of the source: transcript file, session directory, session bundle, or database record. Selects the details layout and the copy actions. |
| `source_mtime_ns`, `source_size` | Last observed file metadata for transcript sources; null for database records and for bundles where no single file is authoritative. |
| `source_scope` | The originating database path for OpenCode SQLite sessions; null for file and directory sources, whose scope is derived (see below). |

Schema version 18 adds these columns to `sessions` with `ALTER TABLE ADD
COLUMN`, following the existing `column_exists` guard so the migration is
re-runnable. It **must not** clear `file_fingerprints`: nothing here changes how
a source parses, and a fingerprint clear would force a full reindex for no gain.
The migration initializes existing sessions as not missing. It must not infer
absence until an adapter completes a successful enumeration of the matching
source scope.

**Presence is observed by enumeration, not by parsing.** In an incremental run
`should_reindex` skips unchanged files, so a session that was discovered and
skipped is present. Deriving presence from a successful parse would mark every
unchanged session missing on the first incremental scan. Discovery updates
`source_last_seen_at`; parsing updates content.

Storage adapters report discovered identities, successful observations, and
whether their scope was completely enumerated. A common reconciliation step
then compares only previously observed identities in that completed scope.
It records absence transactionally before any fingerprint or stale-session
cleanup, preserving sessions, messages, transcript items, child links, pins,
and search data.

I/O errors, a denied permission, an inaccessible root, a disabled source, a
missing OpenCode database, a partial scan, or parsing failure do not prove
absence. They retain the previous state and report indexing diagnostics. A
surviving bundle with a required component missing is likewise an
incomplete-source diagnostic, not a missing session.

### Evidence snapshot

Everything Source details displays lives on `sessions` and is written during
indexing, while the source is still present. **`file_fingerprints` cannot be
the evidence store**: it holds only `(file_path, mtime_ns, size)`, and
`prune_orphan_fingerprints` deletes the row precisely when the file disappears
— the moment the evidence becomes needed. Reading evidence from it would leave
every missing session with unknown metadata.

| Displayed value | Source |
| --- | --- |
| Locator (path, directory, bundle, or database) | Existing `sessions.file_path`, unchanged. |
| Native session identity | Existing `sessions.id`. |
| Last known size and modification time | `source_size` and `source_mtime_ns`, copied from the fingerprint reading at index time rather than read back later. |
| Storage shape and its copy actions | `source_kind`. |
| Owning database | `source_scope`, for OpenCode SQLite only. |

A value that was never observed stays null and renders as unknown; the surface
never fabricates one. A shared database's file size and modification time are
never written as a session-level fingerprint, which is why `source_mtime_ns`
and `source_size` stay null for database records.

### Scope representation

Scope needs no new column for file and directory sources. They are scoped by
`sessions.file_path` prefix, served by the existing `idx_sessions_file_path`
index through the half-open range that `indexed_paths_under` already uses.
OpenCode SQLite sessions are scoped by `source_scope`, their originating
database path.

Fixture and real data cannot alias through the database: `select_db_filename`
already selects a separate database file in `--sessions-dir` mode. The aliasing
that scope must actually prevent is within one database — Codex resolves
several roots per run, OpenCode resolves several databases, and two different
`--sessions-dir` values share the same override database. A completed
enumeration of one root may only mark sources belonging to that root.

This also bounds the identity collision listed under non-goals. When two roots
hold the same native ID, one row survives with the winner's `file_path`. A
complete enumeration of the losing root must not mark that row missing, because
its locator does not fall under that root's prefix. The collision costs a row;
it must never cost a false absence.

A return only clears `source_missing` after the matching identity has been
successfully reindexed. Moves within a completed scope are matched before
absence is recorded. A different identity at the same path cannot replace the
retained session.

## Per-source evidence

| Source | Confirmed absence condition |
| --- | --- |
| Claude Code and Codex | A previously observed transcript is absent from a complete scan of its configured root. |
| OpenCode JSON | Its session identity is absent from successful session enumeration; missing message/part files alone are diagnostic. |
| OpenCode SQLite | Its record is absent from a successful, consistent database enumeration; database unavailability affects no contained sessions. |
| Mistral Vibe | Its session directory is absent from a complete parent-scope scan; missing `meta.json` or `messages.jsonl` in a surviving directory is diagnostic. |
| Kimi Code | Its session bundle is absent from a complete workspace scan; missing required files in a surviving bundle are diagnostic. |

Children are reconciled independently when stored independently. When an owned
bundle is absent, its owned sessions become missing but logical parent links
do not cascade absence into independently stored, present children.

## Product behavior

Missing sessions remain in ordinary list results, internal search, GNOME Shell
search, pins, navigation counts, and analytics, in their normal sort order.
Their retained indexed content remains readable through list rows, deep links,
and child-session links. Search-provider result text remains unchanged; the
state appears in the persistent in-app detail surface after activation.

Machine decisions exclude a missing source: resume is unavailable, and a
missing teammate candidate cannot make a present candidate ambiguous. The
resume handler enforces this at action time, including if a menu was opened
before the source state changed. Detection never updates `last_updated`.

### Session list

A missing row:

- prepends `Source missing` to the existing subtitle chain, reusing the
  existing `markup_escape_text` path in `session_subtitle`;
- includes a 16px `action-unavailable-symbolic` suffix placed first among the
  suffixes, ahead of the pin, the ending-status icon, and the chevron, with
  tooltip `Source missing — showing retained content`;
- marks the icon as presentational and adds the `source_missing` CSS class;
- leaves the title, row activation, pinning, and normal sort position intact;
- uses text, icon, and disabled Resume as redundant state channels rather
  than relying on color.

A row that is missing, pinned, and carries a non-clean ending status shows four
suffixes. That density is accepted; none of the existing suffixes is dropped or
displaced.

There is no second subtitle line, strikethrough, dimmed title, substituted AI
assistant icon, toolbar filter, list banner, grouping, count, or sidebar row.
`SessionRow` is rebuilt when its data changes, so the conditional state must
not rely on a factory update that it does not receive.

The row's `resume` action is disabled with `set_enabled(false)`. A
`gio::MenuItem` carries no tooltip, so the context menu conveys the state only
by being insensitive; the explanation lives in the detail surface.

### Session detail

Open missing sessions show a persistent banner:

> Source missing — showing retained content

`adw::Banner` is not yet used anywhere in `src/ui/`; the banner is new and
belongs in `session_detail.rs`, above the transcript.

The banner action opens **Source details**, a new `adw::NavigationPage` pushed
onto the detail's navigation view rather than a modal `adw::Dialog`. A modal
dialog would need its own back affordance bolted on at narrow widths;
`NavigationPage` gives back navigation natively at every width, and the
detail's existing navigation already owns the header bar that carries it.

That surface shows absence detection, last presence, locator,
and last-known evidence. It adapts by storage type: transcript and bundle
sources offer **Copy path**; database sources offer database path plus **Copy
session ID**. Unknown values remain unknown. It never claims an exact deletion
time or cause. With incomplete or empty retained data it says respectively
**Only indexed content is available** or **No retained transcript**.

Resume remains visible but insensitive and has an accessible explanation.

## Implementation boundaries

- `crates/core/src/database/schema.rs`: the v18 migration, without a
  fingerprint clear.
- `crates/core/src/database/indexer.rs` and `indexer/kimi.rs`: scoped
  observations and reconciliation; replace the OpenCode, Kimi, and Vibe-child
  removal paths; add detection for Claude Code, Codex, and top-level Vibe;
  leave `prune_session_after_parse_skip` a removal path.
- `crates/core/src/database/mod.rs`: session queries and teammate ambiguity
  checks.
- `crates/core/src/models/session.rs`: source state, source evidence, and
  `Session::can_resume`, which gains the missing-source condition alongside the
  existing Kimi child rule.
- `src/ui/session_row.rs` and `data/resources/style.css`: subtitle segment,
  suffix icon, and the `source_missing` class.
- `src/ui/session_detail.rs`: retained-content banner and the storage-aware
  source-details navigation page.
- `src/app/handlers/resume.rs`: action-time resume guard, refusing through the
  existing error-dialog path rather than launching a terminal.
- `crates/core/src/database/shell_search.rs`: **no change**. Neither
  `search_session_ids` nor `load_metadata` filters on availability, and the
  rendered description is unchanged, so missing sessions are already included.
  Adding a `source_missing` column to those queries would be dead weight. A
  regression test pins the inclusion.
- Integration and GTK tests: source transitions and the rendered experience.

## Verification

For every supported AI assistant and both OpenCode backends, tests index a
fixture, remove its authoritative source, and rescan. They verify retained
content and evidence, visible default-list row, repeated-scan timestamp
stability, reappearance, resume denial, and teammate ambiguity exclusion.

A dedicated test covers the incremental case: index a fixture, rescan
incrementally without touching any file, and assert that no session becomes
missing even though every file was skipped by `should_reindex`.

Additional tests cover a native ID colliding across two roots — the surviving
row keeps the winner's locator and a complete scan of the losing root leaves it
available — multiple Codex roots
and OpenCode databases in one run, partial and failed enumerations, unreadable
returns, path replacement by another identity, bundle-component loss, child
independence, parse-skip removal leaving retained rows untouched, pins,
analytics, internal and shell search, and source-specific copy actions. GTK
coverage scrolls mixed available/missing rows to prove recycled rows only
expose the missing CSS class, suffix icon, and resume state for missing
sessions. It also verifies markup escaping, keyboard navigation, screen-reader
semantics, large text, dark mode, and high contrast.

The completed change passes `cargo fmt --all -- --check`,
`cargo clippy --all -- -D warnings`, and the headless CI-parity test suite.
