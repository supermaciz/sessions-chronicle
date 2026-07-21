# Session List Sorting — Design Spec

**Issue:** [#188](https://github.com/supermaciz/sessions-chronicle/issues/188) — Sort options for the session list  
**Date:** 2026-07-21  
**Status:** Approved  
**Decision source:** `docs/explorations/2026-07-21-session-list-sorting-exploration.md` — Proposal C (named reading orders), rendered on Proposal A's header-bar pill, labelled but never carrying the `accent` CSS class.

## Summary

The session list order is hardcoded (`ORDER BY last_updated DESC`, or bm25 rank in search mode). This feature adds a sort pill in the header bar offering **named reading orders** — fixed `(key, direction)` pairs with self-describing names — instead of a criterion list plus a direction toggle. Relevance remains the implicit order of search mode and is modelled as **derived state**, never as a storable enum variant.

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

- `label()` — UI strings for the popover rows and the pill closed state: "Recent activity", "Oldest first", "Newest first", "Most messages".
- `as_setting_str()` / `from_setting_str()` — GSettings serialization: `"recent-activity"`, `"oldest-first"`, `"newest-first"`, `"most-messages"`. Unknown values fall back to the default (`RecentActivity`), so a downgraded or hand-edited setting never breaks startup.
- `Default` — `RecentActivity`.

There is **no `Relevance` variant**. Relevance is not a persistable preference; it is derived from search state (section 5). This makes the two invariants issue #188 flags as "what breaks first" unrepresentable rather than hand-checked:

1. *Never persist relevance* — the enum cannot express it, so the GSettings key cannot contain it.
2. *Relevance is illegal outside search* — outside search the DB layer takes a plain `SortOrder`, which has no relevance to pass.

## 2. Database layer — `src/database/mod.rs`

- `fn sort_sql_clause(sort: SortOrder) -> &'static str` — a `match` returning constant fragments (`"ORDER BY last_updated DESC"`, `"ORDER BY start_time ASC"`, `"ORDER BY start_time DESC"`, `"ORDER BY message_count DESC"`). SQL is built **from the enum, never from strings** — this stays the one place in the file where SQL is concatenated rather than parameterised.
- `load_sessions_for_filter(.., sort: SortOrder)` — replaces the hardcoded `ORDER BY last_updated DESC` (`src/database/mod.rs:494`).
- `search_sessions_with_query(.., sort: Option<SortOrder>)`:
  - `None` → `ORDER BY rank ASC, s.last_updated DESC` (current behaviour, `src/database/mod.rs:446`) — relevance.
  - `Some(order)` → the clause for that order (search-scope column prefix `s.` applied).
- `load_session_by_id_for_filter` (`src/database/mod.rs:305`) is untouched: it returns at most one row per id, ordering is moot.

**HashSet dedup trap.** `search_sessions_with_query` deduplicates via a `HashSet` keeping the *first* row seen (`src/database/mod.rs:459`). Changing `ORDER BY` silently changes which row survives. A test pins this behaviour explicitly (assert which session survives under two different orders), so any future change to the surviving-row rule is a conscious decision, not an accident.

## 3. Migration — v15

```sql
CREATE INDEX IF NOT EXISTS idx_sessions_subagent_start_time
    ON sessions(is_subagent, start_time DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_subagent_message_count
    ON sessions(is_subagent, message_count DESC);
```

Two indexes cover all four orders: SQLite scans indexes backwards, so `(is_subagent, start_time DESC)` also serves `OldestFirst` (ASC). `RecentActivity` keeps using the v12 `(is_subagent, last_updated DESC)` index.

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
- `search_sort_override: Option<SortOrder>` — search-mode only, never persisted.

Effective order resolution:

| State | List order | Pill reads |
|---|---|---|
| No active query | `sort_order` | order label (icon-only if default) |
| Query active, override `None` | bm25 rank | "Relevance" |
| Query active, override `Some(o)` | `o` | `o`'s label |

Transitions:

- **Query becomes active** → `search_sort_override = None` → relevance, automatically. The "Relevance" row appears at the top of the popover.
- **Explicit pick of a named order during search** → `search_sort_override = Some(order)` **and** `sort_order = order` (persisted) — "picked Messages → stays Messages" after the query clears, exactly as issue #188 requires.
- **Explicit pick of "Relevance"** → `search_sort_override = None`. No enum variant needed.
- **Query cleared** → back to `sort_order`. Edge case resolved beyond the issue's text: even if Relevance was picked *explicitly*, clearing the query falls back to `sort_order` — rank does not exist without a query. This generalizes the issue's implicit-only rule without contradicting it.
- **Explicit pick outside search** → `sort_order = order`, persisted, list reloads.

Pinned sessions do not float: `pinned_at` appears in no `ORDER BY` (unchanged; `ProjectFilter::Pinned` is the explicit path).

## 6. UI — `src/ui/sort_pill.rs`

New component modelled on `src/ui/date_pill.rs`: `SimpleComponent` with `Root = gtk::MenuButton` (flat), popover holding a `boxed-list` `gtk::ListBox` in `Browse` selection mode.

**Popover contents** — a flat list of named orders, no direction toggle (that is the point of Proposal C):

```
  Relevance          ← present ONLY while a query is active, top row
  ─────────────
✓ Recent activity
  Oldest first
  Newest first
  Most messages
```

The Relevance row is inserted/removed dynamically — never a permanently greyed-out entry.

**Closed state:**

- Default (`RecentActivity`, no query) → icon only (`view-sort-descending-symbolic`, standard Adwaita symbolic), no label. Header stays calm.
- Any other effective order (including Relevance) → icon + label.
- **Never the `accent` CSS class** (amendment from Proposal D): sorting hides nothing; `accent` elsewhere means "data is constrained". The label informs, it must not alarm.
- Tooltip always carries the full description: "Sort by: Oldest first" / "Sort sessions (Ctrl+Shift+O)" when default — same pattern as `tooltip_for_filter`.

**Narrow widths:** an `adw::Breakpoint` (condition `max-width: 550sp`) added to the `adw::ApplicationWindow`; `connect_apply` / `connect_unapply` send a `SortPillInput` toggling a `narrow: bool` that hides the label (icon-only fallback). The tooltip carries the description at all widths.

**Component interface** (mirrors DatePill):

- `SortPillInput`: `OrderSelected(SortOrder)` and `RelevanceSelected` (row activations inside the popover), `SyncState { sort_order: SortOrder, search_active: bool, override_active: bool }` (app → pill, drives label and row selection), `OpenViaShortcut`, `SetNarrow(bool)`.
- `SortPillOutput`: `OrderPicked(SortOrder)`, `RelevancePicked`.

**Wiring** (`src/app/mod.rs`, `src/app/init.rs`):

- `pack_start` immediately after the DatePill (`src/app/mod.rs:525`) — header-bar left, sibling of the control it belongs with. `pack_end` stays reserved.
- Outputs follow the same path as `DatePillOutput::FilterChanged`: update app state (section 5), persist, reload the session list.
- Shortcut `<Ctrl><Shift>O` → `SortPillInput::OpenViaShortcut`, symmetric with the DatePill's `<Ctrl><Shift>D`.

## 7. Testing & verification

**Unit tests:**

- `SortOrder` ↔ SQL clause mapping (every variant).
- `SortOrder` ↔ setting string round-trip; unknown string falls back to default.
- Effective-order resolution: the five transitions of section 5, including the explicit-Relevance-then-clear edge case.
- HashSet dedup pinning test (section 2).
- v15 migration test: indexes exist after migrating a v14 database (schema.rs test helpers already exist: `index_exists`).

**GTK tests** (same harness as `date_pill.rs`):

- Popover row selection follows the current order on `OpenViaShortcut`.
- Relevance row present iff search is active.
- Label visibility: hidden at default, shown otherwise, hidden again when `SetNarrow(true)`.

**Manual, with `--sessions-dir tests/fixtures`:**

1. Each order visually matches what rows display (dates, message counts).
2. Type a query → pill reads "Relevance"; pick "Most messages"; clear the query → **stays** "Most messages".
3. `EXPLAIN QUERY PLAN` per order after v15: no temp B-tree on the unfiltered view.
4. Narrow the window below the breakpoint → label collapses to icon, tooltip still describes the order.

**CI parity:** `cargo fmt --all -- --check && cargo clippy --all -- -D warnings && cargo test --all --no-fail-fast`.

## 8. Implementation sequencing

1. **Data layer** — `SortOrder` in `src/models/`, `sort_sql_clause()`, parameterised `ORDER BY` sites, v15 migration, unit tests (incl. dedup pin).
2. **Sort pill** — `src/ui/sort_pill.rs`, header wiring, breakpoint, shortcut, GTK tests.
3. **Persistence** — gschema key, load-at-startup, write-on-pick.

Deferred by the exploration's decision, unchanged here: Proposal E (filter-inferred defaults) waits for usage evidence; Proposal B (lenses) is a separate project for which C's named orders are the foundation.
