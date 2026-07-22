# Session List Sorting — Design Spec

**Issue:** [#188](https://github.com/supermaciz/sessions-chronicle/issues/188) — Sort options for the session list  
**Date:** 2026-07-21  
**Status:** Approved — implemented in [#190](https://github.com/supermaciz/sessions-chronicle/pull/190), see section 9 for amendments  
**Decision source:** `docs/explorations/2026-07-21-session-list-sorting-exploration.md` — Proposal C (named reading orders), rendered on Proposal A's header-bar pill, labelled but never carrying the `accent` CSS class.

## Summary

The session list order is hardcoded (`ORDER BY last_updated DESC`, or bm25 rank for FTS queries). This feature adds a sort pill in the header bar offering **named reading orders** — fixed `(key, direction)` pairs with self-describing names — instead of a criterion list plus a direction toggle. Relevance remains the implicit order of FTS search and is modelled as **derived state**, never as a storable enum variant.

Corrections to the exploration doc discovered during design:

- The schema is already at **v14** (v13 was consumed by the messages/FTS rework, v14 by the `file_path` index). The sort indexes ship as a **v15 migration**.
- The gschema has no GSettings enum precedent; `resume-terminal` uses a string key with documented values. The `sort-order` key follows that convention.

## 1. Model — `src/models/`

One enum, key and direction fused, `Copy`:

```rust
pub enum SortOrder {
    RecentActivity, // last_updated DESC — default
    OldestFirst,    // start_time ASC
    NewestFirst,    // start_time DESC
    MostMessages,   // message_count DESC
}
```

It owns:

- `label_msgid()` — untranslated message IDs for the popover rows and the pill closed state: "Recent activity", "Oldest first", "Newest first", "Most messages". The UI resolves them through the project's existing `gettext-rs` setup; persisted values and SQL identifiers are never translated.
- `as_setting_str()` / `from_setting_str()` — GSettings serialization: `"recent-activity"`, `"oldest-first"`, `"newest-first"`, `"most-messages"`. Unknown values fall back to the default (`RecentActivity`), so a downgraded or hand-edited setting never breaks startup.
- `Default` — `RecentActivity`.

There is **no `Relevance` variant**. Relevance is not a persistable preference; it is derived from search state (section 5). This makes the two invariants issue #188 flags as "what breaks first" unrepresentable rather than hand-checked:

1. *Never persist relevance* — the enum cannot express it, so the GSettings key cannot contain it.
2. *Relevance is illegal outside FTS search* — non-FTS database paths take a plain `SortOrder` or return at most one direct-ID result, with no relevance value to pass.

## 2. Database layer — `src/database/mod.rs`

- `fn sort_sql_clause(sort: SortOrder) -> &'static str` — a `match` returning constant fragments. Every named order includes a stable `id` tie-breaker so sessions with equal timestamps or message counts never reshuffle between reloads:
  - `RecentActivity` → `ORDER BY last_updated DESC, id DESC`
  - `OldestFirst` → `ORDER BY start_time ASC, id ASC`
  - `NewestFirst` → `ORDER BY start_time DESC, id DESC`
  - `MostMessages` → `ORDER BY message_count DESC, id DESC`
  SQL is built **from the enum, never from strings** — this stays the one place in the file where SQL is concatenated rather than parameterised.
- `load_sessions_for_filter(.., sort: SortOrder)` — replaces the hardcoded `ORDER BY last_updated DESC` (`src/database/mod.rs:494`).
- `search_sessions_with_query(.., sort: Option<SortOrder>)`:
  - `None` → `ORDER BY rank ASC, s.last_updated DESC, s.id DESC` — relevance, with deterministic fallbacks.
  - `Some(order)` → the clause for that order (search-scope column prefix `s.` applied).
- `load_session_by_id_for_filter` (`src/database/mod.rs:305`) is untouched: it returns at most one row per id, ordering is moot.

**HashSet dedup contract.** `search_sessions_with_query` returns one joined row per matching message, then deduplicates session IDs via a `HashSet` while preserving the first occurrence (`src/database/mod.rs:459`). All duplicate rows for an ID map to the same `Session`; no distinct session is discarded. Under relevance, the first occurrence is the session's best-ranked matching message. Under a named order, all duplicates share the same session-level sort keys. Tests therefore pin the observable contract: every session ID appears once, final session order follows the requested reading order, and ties follow the explicit `id` fallback.

## 3. Migration — v15

```sql
DROP INDEX IF EXISTS idx_sessions_top_level_last_updated;
CREATE INDEX IF NOT EXISTS idx_sessions_top_level_last_updated_id
    ON sessions(is_subagent, last_updated DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_top_level_start_time_id
    ON sessions(is_subagent, start_time DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_top_level_message_count_id
    ON sessions(is_subagent, message_count DESC, id DESC);
```

Three indexes cover all four deterministic orders. The v12 top-level activity index is replaced because its implicit rowid tie-breaker does not satisfy `ORDER BY ... id DESC`. SQLite scans indexes backwards, so `(is_subagent, start_time DESC, id DESC)` also serves `OldestFirst` (`start_time ASC, id ASC`). The other v12 filter-specific activity indexes remain unchanged.

Verification: `EXPLAIN QUERY PLAN` per order.

- Unfiltered view: no `USE TEMP B-TREE FOR ORDER BY` for any order.
- Project/assistant/date-filtered views: a temp B-tree is **accepted and documented** — filtered sets are small, and covering every (filter × sort) combination with composite indexes is not worth the write amplification. Same trade-off issue #188 already makes.

## 4. Persistence — GSettings

One key in `data/dev.maciz.sessionschronicle.gschema.xml.in`:

```xml
<key name="sort-order" type="s">
  <default>"recent-activity"</default>
  <summary>Session list sort order</summary>
  <description>Named reading order for the session list. Accepted values: recent-activity, oldest-first, newest-first, most-messages.</description>
</key>
```

String key with documented values, matching the existing `resume-terminal` convention. Read at startup through `SortOrder::from_setting_str` (unknown → default); written on every explicit pick. Relevance is unpersistable by construction (section 1).

## 5. App state & relevance semantics

In `App` (`src/app/mod.rs`):

- `sort_order: SortOrder` — the persisted preference, always a real named order.
- `search_sort_override: Option<SortOrder>` — FTS-search only, never persisted.

Search ordering has three contexts, derived once and shared by the app state and session-loading path so the pill cannot disagree with the query actually executed:

- **No query** — the trimmed query is empty.
- **Direct session-ID lookup** — `parse_session_id_query` recognizes the query; at most one session can be returned, so relevance and user-selected ordering are both operationally moot. The pill shows the persisted named order and does not offer Relevance.
- **FTS query** — a non-empty query that is not a direct session-ID lookup; BM25 relevance exists and can be overridden.

Effective order resolution:

| State | List order | Pill reads / offers |
|---|---|---|
| No active query | `sort_order` | order label (icon-only if default) |
| Direct session-ID lookup | immaterial (at most one result) | persisted order; no Relevance row |
| FTS query, override `None` | bm25 rank | "Relevance" |
| FTS query, override `Some(o)` | `o` | `o`'s label |

Transitions:

- **FTS query becomes active** → `search_sort_override = None` → relevance, automatically. The "Relevance" row appears at the top of the popover.
- **Direct session-ID lookup becomes active** → no relevance state is entered; the persisted order remains visible and no sorting preference is changed.
- **Explicit pick of a named order during FTS search** → `search_sort_override = Some(order)` **and** `sort_order = order` (persisted) — "picked Messages → stays Messages" after the query clears, exactly as issue #188 requires.
- **Explicit pick of "Relevance"** → `search_sort_override = None`. No enum variant needed.
- **FTS query cleared or replaced by a direct session-ID lookup** → back to `sort_order`. Even if Relevance was picked *explicitly*, rank does not exist in the new context.
- **Explicit pick outside FTS search** → `sort_order = order`, persisted; the list reloads when ordering can affect more than one result.

Pinned sessions do not float: `pinned_at` appears in no `ORDER BY` (unchanged; `ProjectFilter::Pinned` is the explicit path).

## 6. UI — `src/ui/sort_pill.rs`

New component modelled on `src/ui/date_pill.rs`: `SimpleComponent` with `Root = gtk::MenuButton` (flat), popover holding a `boxed-list` `gtk::ListBox` in `Browse` selection mode.

**Popover contents** — a flat list of named orders, no direction toggle (that is the point of Proposal C):

```
  Relevance          ← present ONLY while an FTS query is active, top row
  ─────────────
✓ Recent activity
  Oldest first
  Newest first
  Most messages
```

The Relevance row is inserted/removed dynamically — present only for an FTS query, never for a direct session-ID lookup, and never a permanently greyed-out entry. `gtk::SelectionMode::Browse` supplies selected-row state; if the checkmark shown above is rendered literally rather than being illustrative, each row owns an explicit check icon whose visibility follows the effective order.

**Closed state:**

- Default (`RecentActivity`, no FTS query) → icon only (`view-sort-descending-symbolic`, standard Adwaita symbolic), no label. Header stays calm.
- Any other effective order (including Relevance) → icon + label.
- **Never the `accent` CSS class** (amendment from Proposal D): sorting hides nothing; `accent` elsewhere means "data is constrained". The label informs, it must not alarm.
- Tooltip always carries the full, localized description: "Sort by: Oldest first" / "Sort sessions (Ctrl+Shift+O)" when default — same pattern as `tooltip_for_filter`.

**Narrow widths:** keep the adaptive fallback, but place it above the application's existing 710 px minimum width. An `adw::Breakpoint` with an initial condition of `max-width: 760sp` is added to the `adw::ApplicationWindow`; `connect_apply` / `connect_unapply` send a `SortPillInput` toggling a `narrow: bool` that hides the label. The exact threshold is validated against the fully populated header, translations, and increased text scaling; it may move upward if collision appears, but must remain reachable without changing the window's minimum size. The tooltip carries the description at all widths.

**Component interface** (mirrors DatePill):

- `SortPillInput`: `OrderSelected(SortOrder)` and `RelevanceSelected` (row activations inside the popover), `SyncState { sort_order: SortOrder, fts_search_active: bool, override_active: bool }` (app → pill, drives label and row selection), `OpenViaShortcut`, `SetNarrow(bool)`.
- `SortPillOutput`: `OrderPicked(SortOrder)`, `RelevancePicked`.

**Wiring** (`src/app/mod.rs`, `src/app/init.rs`):

- `pack_start` immediately after the DatePill (`src/app/mod.rs:525`) — header-bar left, sibling of the control it belongs with. `pack_end` stays reserved.
- Outputs follow the same path as `DatePillOutput::FilterChanged`: update app state (section 5), persist, reload the session list.
- Shortcut `<Ctrl><Shift>O` → `SortPillInput::OpenViaShortcut`, symmetric with the DatePill's `<Ctrl><Shift>D`.
- The existing `adw::ShortcutsDialog` gains a localized "Sort sessions" entry for `<Ctrl><Shift>O`.

All new user-visible strings use the existing `gettext-rs` setup. This feature does not introduce a new localization mechanism. Enum serialization strings, GSettings accepted values, and SQL fragments remain stable untranslated identifiers.

## 7. Testing & verification

**Unit tests:**

- `SortOrder` ↔ SQL clause mapping (every variant).
- `SortOrder` ↔ setting string round-trip; unknown string falls back to default.
- Effective-order resolution across no-query, direct-ID and FTS contexts, including the explicit-Relevance-then-clear edge case.
- Search dedup contract: unique IDs, requested final order, and deterministic tie handling (section 2).
- v15 migration test: the three replacement indexes exist and the superseded `idx_sessions_top_level_last_updated` index does not after migrating a v14 database (`schema.rs` test helpers already provide `index_exists`).

**GTK tests** (same harness as `date_pill.rs`):

- Popover row selection follows the current order on `OpenViaShortcut`.
- Relevance row present iff an FTS search is active; absent for direct session-ID lookup.
- Label visibility: hidden at default, shown otherwise, hidden again when `SetNarrow(true)`.

**Manual, with `--sessions-dir tests/fixtures`:**

1. Each order visually matches what rows display (dates, message counts).
2. Type a query → pill reads "Relevance"; pick "Most messages"; clear the query → **stays** "Most messages".
3. `EXPLAIN QUERY PLAN` per order after v15: no temp B-tree on the unfiltered view.
4. Narrow the window below `760sp` while staying above the minimum width → label collapses to icon and the tooltip still describes the order. Repeat with increased text scaling and at least one translation with longer labels; move the threshold upward if the populated header collides before the breakpoint applies.

**CI parity:** `cargo fmt --all -- --check && cargo clippy --all -- -D warnings && cargo test --all --no-fail-fast`.

## 8. Scope boundaries

Proposal E (filter-inferred defaults) remains deferred until usage evidence shows that users systematically choose a particular order in a filter context. Proposal B (lenses, including chronology grouping and volume encoding) remains a separate project for which these named orders are the foundation.

## 9. Amendments during implementation

Recorded 2026-07-22, from [#190](https://github.com/supermaciz/sessions-chronicle/pull/190). Sections 1–5 and 8 shipped as designed. What follows is where the implementation departed from this spec, and why. Each entry supersedes the section it names.

### 9.1 The 760sp narrow breakpoint was dropped (supersedes §6 "Narrow widths")

**Spec:** an `adw::Breakpoint` at `max-width: 760sp` on the `adw::ApplicationWindow`, with `connect_apply` / `connect_unapply` sending `SortPillInput::SetNarrow(bool)` to hide the label.

**Shipped:** no breakpoint. The label follows a single rule — visible whenever a non-default order is active, hidden otherwise — at every window width. This is exactly what `DatePill` already does with `self.current_filter.is_active()`, so the two pills sitting side by side in the header now behave identically.

**Why:** the spec treated the new breakpoint as independent of the pre-existing 400sp collapse breakpoint. It is not. libadwaita applies only the **last matching** breakpoint, not the union of all matching ones, so registration order becomes load-bearing as soon as a window has two — and here one condition strictly contains the other. `max-width: 400sp` and `max-width: 760sp` scale together with the text-scaling factor, so any width matching the 400sp condition necessarily also matches the 760sp one. The two are never simultaneously active in the sense of both applying; the later-registered one simply wins and the other's setters never run.

Registering the new breakpoint after the existing one therefore disabled sidebar collapse and the view-switcher bar outright. Because the 400sp condition is only reachable at all above roughly 1.8x text scaling (400sp must exceed the 710 px minimum window width), the regression was invisible at default settings and appeared only in the accessibility path — precisely where the 400sp breakpoint was still doing its only useful work.

Two fixes were attempted before the current one: ordering the breakpoints wide-first and making the 400sp one re-send `SetNarrow` (correct but fragile — the ordering rule is invisible to the next person adding a breakpoint), then deleting the 400sp breakpoint outright (which re-created the regression it was meant to avoid). Aligning on `DatePill` removes the second breakpoint entirely, so no ordering rule is load-bearing and `setup_breakpoints` is byte-identical to its state before this feature.

**Consequences:**

- `SortPillInput::SetNarrow(bool)` does not exist; the interface in §6 is otherwise as specified.
- §7's GTK test "hidden again when `SetNarrow(true)`" became `label_is_shown_only_for_non_default_orders`, which also covers Relevance-during-FTS as a non-default state.
- §7's manual step 4 (narrowing below 760sp, repeated under text scaling and long translations) no longer applies and was not performed.

The underlying question the spec was right to raise — a 400sp condition against a 710 px minimum window width is nearly unreachable — is untouched here and left as a follow-up. It predates this feature.

### 9.2 Row selection carries no explicit check icon (resolves §6's open question)

§6 left this conditional: *"if the checkmark shown above is rendered literally rather than being illustrative, each row owns an explicit check icon"*. The checkmark was read as illustrative. Rows are label-only and the effective order is conveyed solely by `gtk::SelectionMode::Browse`, matching `DatePill`'s preset rows.

This is worth revisiting for both pills at once: a `GtkListBox` in Browse mode announces "list item" to assistive technology, where a menu model with radio items would announce a checked radio button. Deliberately out of scope here — changing it for `SortPill` alone would make the two pills inconsistent again.

### 9.3 `SessionQuery` replaces `parse_session_id_query` (extends §5)

§5 described three search contexts and required they be *"derived once and shared by the app state and session-loading path so the pill cannot disagree with the query actually executed"*, but named no type. The implementation gives that requirement a home: `src/models/session_query.rs` exposes `SessionQuery::{None, DirectId, Fts}` with a single `classify()` entry point. `session_list.rs`'s ad hoc `parse_session_id_query` is deleted, and `App` and the loading path now classify through the same function.

Two related mechanisms follow from the same requirement:

- `SessionListMsg::SetSearchQuery(String)` became `SetSearchState { query, sort }`, so a query change and its resolved order reach the list in one message and cannot be observed apart.
- `App::clear_search_state` encodes "clearing FTS restores the persisted order" once, replacing three copies of that logic.

### 9.4 The query-plan check became an automated test (upgrades §7)

§7 listed `EXPLAIN QUERY PLAN` per order as **manual** step 3. It ships instead as `unfiltered_named_orders_do_not_build_temporary_sort_tables` in `schema.rs`, asserting no `USE TEMP B-TREE FOR ORDER BY` for any of the four unfiltered orders. The filtered-view trade-off documented in §3 is unchanged and remains untested by design.

### 9.5 Minor notes

- The v15 migration is applied to freshly created databases as well, so a new database briefly creates `idx_sessions_top_level_last_updated` (`schema.rs`) before the migration drops it. Harmless, consistent with how earlier migrations in this file behave, but it means the base schema statement is now dead weight.
- The tooltip uses `gettext("Sort by: {}")` with placeholder substitution rather than `format!`, so translators receive the sentence frame rather than a fragment. §6's reference pattern, `tooltip_for_filter`, interpolates with `format!` and is the weaker of the two.
- §6's `pack_start` placement next to the `DatePill` shipped as specified. Note that GNOME convention would put sort controls in `pack_end`; the spec's choice was kept deliberately, not by oversight.
