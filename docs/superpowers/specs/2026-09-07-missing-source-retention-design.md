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

## Data model and reconciliation

The `sessions` data carries a shared source state:

| Field | Meaning |
| --- | --- |
| `source_missing` | Confirmed absence of the current session source; defaults to false. |
| `source_missing_detected_at` | First confirmed absence in the current absence period; repeated scans do not change it. |
| `source_last_seen_at` | Time of the last successful presence observation, independent of conversation activity. |
| scoped source identity and evidence | Assistant, storage kind, owning root/database scope, native identity, locator, and storage-appropriate last-known metadata. |

The migration initializes existing sessions as not missing. It must not infer
absence until an adapter completes a successful enumeration of the matching
source scope.

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

The source identity is scoped, so the same native ID in fixtures and real
data cannot alias. A return only clears `source_missing` after the matching
identity has been successfully reindexed. Moves within a completed scope are
matched before absence is recorded. A different identity at the same path
cannot replace the retained session.

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

- prepends `Source missing` to the existing subtitle chain;
- includes a 16px `action-unavailable-symbolic` suffix before the pin icon,
  with tooltip `Source missing — showing retained content`;
- marks the icon as presentational and adds the `source_missing` CSS class;
- leaves the title, row activation, pinning, and normal sort position intact;
- uses text, icon, and disabled Resume as redundant state channels rather
  than relying on color.

There is no second subtitle line, strikethrough, dimmed title, substituted AI
assistant icon, toolbar filter, list banner, grouping, count, or sidebar row.
`SessionRow` is rebuilt when its data changes, so the conditional state must
not rely on a factory update that it does not receive.

### Session detail

Open missing sessions show a persistent banner:

> Source missing — showing retained content

The banner action opens **Source details**. That surface shows absence
detection, last presence, locator, and last-known evidence. It adapts by
storage type: transcript and bundle sources offer **Copy path**; database
sources offer database path plus **Copy session ID**. Unknown values remain
unknown. It never claims an exact deletion time or cause. With incomplete or
empty retained data it says respectively **Only indexed content is available**
or **No retained transcript**.

Resume remains visible but insensitive and has an accessible explanation. At
narrow widths, source details are a page with explicit back navigation.

## Implementation boundaries

- `crates/core/src/database/`: schema migration, scoped observations,
  reconciliation, removal-path replacement, session queries, shell search,
  and teammate ambiguity checks.
- `crates/core/src/models/`: source state and source-evidence representation.
- `src/ui/`: session row marker, CSS, retained-content banner, and
  storage-aware details surface.
- `src/app/handlers/resume.rs`: action-time resume guard.
- Integration and GTK tests: source transitions and the rendered experience.

## Verification

For every supported AI assistant and both OpenCode backends, tests index a
fixture, remove its authoritative source, and rescan. They verify retained
content and evidence, visible default-list row, repeated-scan timestamp
stability, reappearance, resume denial, and teammate ambiguity exclusion.

Additional tests cover identical IDs in distinct roots, partial and failed
enumerations, unreadable returns, path replacement by another identity,
bundle-component loss, child independence, pins, analytics, internal and
shell search, and source-specific copy actions. GTK coverage scrolls mixed
available/missing rows to prove recycled rows only expose the missing CSS
class, suffix icon, and resume state for missing sessions. It also verifies
markup escaping, keyboard navigation, screen-reader semantics, large text,
dark mode, and high contrast.

The completed change passes `cargo fmt --all -- --check`,
`cargo clippy --all -- -D warnings`, and the headless CI-parity test suite.
