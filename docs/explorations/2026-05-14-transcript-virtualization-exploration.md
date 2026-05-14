# Transcript Virtualization for SessionDetail — Design Exploration

**Issue:** [#134](https://github.com/supermaciz/sessions-chronicle/issues/134)  
**Related:** [#132](https://github.com/supermaciz/sessions-chronicle/issues/132) (batch-size tuning), [#146](https://github.com/supermaciz/sessions-chronicle/issues/146) (frame drop investigation)  
**Date:** 2026-05-14  
**Status:** Decision made — **Proposal A (TypedListView migration)** selected

---

## Problem Statement

`SessionDetail` renders transcript rows into a `gtk::ListBox` via `FactoryVecDeque`
(`src/ui/session_detail.rs:50`). `gtk::ListBox` does not virtualize: every mounted row
participates in every Layout pass. The post-#146 investigation measured
`update_to_layout_us` spans of **1.3–1.4 seconds** on the frame following each batch drop,
with `total_row_build_duration_ms = 1–3 ms` — confirming that GTK Layout cost, not Rust
work, dominates.

Two mitigations exist today:

1. **Pagination**: `Load more` button, 75 initial rows then 100 per page
   (`session_detail.rs:31-32`).
2. **Incremental rendering**: 1 row pushed to the factory per tick, 100 ms watchdog
   (`session_detail.rs:36-39`).

Both are workarounds. They spread Layout cost across more frames but cannot reduce the
total Layout work for a session a user has scrolled through. Long sessions still hit
the wall — just later.

The structural ceiling is the same regardless of batching: Layout cost grows linearly
with mounted rows. The only way through is mounting fewer rows at any given time.

### UX consequences observed today

- First frame after `Load more`: visible scroll stutter on sessions with many tool calls.
- Search jump to a far-off match: the row must be mounted first (re-pagination), then
  scrolled to — feels laggy.
- Memory: every code block in the visible page instantiates a `sourceview5::View` +
  `Buffer`. A long session keeps dozens of these alive simultaneously.
- The `Load more` button itself is a friction point — users expect transcripts to
  behave like chat UIs (scroll = more content).

---

## Background: what the codebase looks like today

This section documents the current structure that any virtualization approach must
preserve or replace.

### ListBox usage

- `messages: FactoryVecDeque<TranscriptRow>` (`session_detail.rs:50`)
- Built via `FactoryVecDeque::builder().launch_default()` (`session_detail.rs:844-865`)
- Root widget: `gtk::Box` from the factory, inserted as `messages_box` in the view
  (`session_detail.rs:728-731`)
- Rows pushed via `guard.push_back(item)` one-by-one in render batches
  (`session_detail.rs:1874`)

### Search highlight state

- Stored **in the model**: `MessageItemInit.highlight_query: Option<String>`
  (`transcript_row.rs:58`), passed through `TranscriptRow.highlight_query`
  (`transcript_row.rs:224`)
- Applied during `render_content()` (`transcript_row.rs:264-301`) by calling
  `markdown::render_markdown_to_textview(content, highlight_query)` or
  `highlight::highlight_text()`
- **Already model-resident** — no widget-level state to migrate.

### Row expansion state

- **Message rows**: `TranscriptRow.expanded: bool` (`transcript_row.rs:226`),
  stored in the factory model. Survives row updates by design.
- **Tool burst rows**: expansion state lives **in the Revealer widget**
  (`transcript_row.rs:1229`), with a `children_built: Rc<Cell<bool>>` flag
  (`transcript_row.rs:1223`) for lazy population. **Not in the model.**
- `default_expanded: bool` exists in `ToolBurstItemInit` (`transcript_row.rs:114`)
  but is initial-only; user-driven toggle is not persisted.

### Tool burst grouping

`group_transcript_rows()` (`transcript_display.rs:15-31`) emits
`DisplayTranscriptItem::ToolBurst { rows: Vec<TranscriptItemRow> }` whenever 2+
consecutive tool calls appear. **Each burst is one factory row**, not multiple. The
burst row internally contains a header `gtk::Button` + `gtk::Revealer` whose child is
a `gtk::Box` of tool call widgets (`transcript_row.rs:1103-1272`).

**Implication:** the migration does not need section headers or grouped rows.
A burst is already a single row from the list's perspective.

### Widget choices

No `Adw.ExpanderRow`, no `Adw.ActionRow`, no `ListBox`-specific separators are used.
Only standard widgets: `gtk::Box`, `gtk::Label`, `gtk::Button`, `gtk::Revealer`,
`gtk::TextView`, `gtk::ScrolledWindow`, `sourceview5::View`. **No Adwaita list-box
semantics to migrate.**

### Scroll-to-row for search

`session_detail.rs:1433-1485, 2588-2603`. The current path:

1. `match_positions: Vec<MatchPosition>` holds `(item_index, char_offset)`
   pairs (`session_detail.rs:64`).
2. On jump: `scroll_to_item: Cell<Option<ScrollTarget>>` is set with a `display_index`
   (factory position) and optional `child_index` (tool call inside a burst).
3. Post-render idle callback fetches the row widget via
   `messages_widget.observe_children().item(target.display_index as u32)`
   (`session_detail.rs:1439`) — this is the **only** ListBox-specific API used.
4. Walks the widget tree to find the target child and uses
   `compute_point()` + `vadjustment.set_value()` to scroll it 1/3 into view
   (`session_detail.rs:2588-2603`).

### Row identity

- `transcript_item_index: i64` — stable database identifier
  (`transcript_row.rs:56`)
- `item_index: usize` — factory position (`transcript_row.rs:55`)
- `display_targets_by_item_index: BTreeMap<i64, ScrollTarget>` maps DB id →
  factory display position (`session_detail.rs:65`)

The dual-key design (stable DB id + display position) is already in place. A virtualized
list view does not need to change this; it just changes how `display_index` resolves to
a widget.

---

## Proposal A — Migrate to `relm4::typed_view::list::TypedListView`

**Principle:** replace `gtk::ListBox` + `FactoryVecDeque` with the supported GTK4
virtualization primitive (`gtk::ListView` + `SignalListItemFactory` + `gio::ListStore`),
exposed by Relm4 0.10 as the `TypedListView<T, SelectionModel>` typed wrapper.

### How it works

- `gio::ListStore` holds **data items** (cheap, no widgets).
- `SignalListItemFactory` fires `setup` once per physical widget pool slot, then
  `bind`/`unbind` every time a data item attaches/detaches as the user scrolls.
- Only rows in the visible viewport (plus a small overscan) are realized at any time.
- `RelmListItem` trait gives a typed `setup` / `bind` / `unbind` / `teardown` lifecycle.
  ([docs](https://docs.rs/relm4/0.10.0/relm4/typed_view/list/index.html))

### Widget hierarchy

```
gtk::ScrolledWindow
└── gtk::ListView (.transcript-list-view)
    └── SignalListItemFactory
        └── (per visible row) gtk::Box .message-row (or .tool-burst-row)
            └── … existing per-kind widget tree …
```

### Migration plan

1. Define `TranscriptItemData: RelmListItem` — a `gio::Object`-wrapped struct holding
   `transcript_item_index: i64`, `display_index: usize`, `kind: DisplayKind`,
   `expanded: Cell<bool>`, `highlight_query: Option<String>`, plus enough payload to
   rebuild the widget tree (lazy DB load for full content stays as-is).
2. `setup()` builds the widget skeleton (Box + role label + content container).
3. `bind()` populates content via existing `render_content()` and `populate_tool_burst_children()`.
4. `unbind()` clears child widgets, drops `sourceview5::View` references, disconnects signal handlers
   set in bind.
5. Bulk-load all rows via `gio::ListStore::splice()` — one `items-changed` emission,
   no incremental render needed.
6. Replace `observe_children().item(idx)` (`session_detail.rs:1439`) with
   `typed_view.view.scroll_to(pos, ListScrollFlags::NONE, None)`. Burst-child scrolling
   stays as today (the burst row exists once realized).

### What goes away

- `Load more` button + `loading_next_page` / `has_more_messages` / `loading_first_page` state.
- `PendingRenderBatch` + per-tick render scheduling (`session_detail.rs:1860-1920`).
- `INITIAL_PAGE_SIZE` / `NEXT_PAGE_SIZE` / `RENDER_BATCH_SIZE` / `RENDER_BATCH_WATCHDOG_MS`
  constants (`session_detail.rs:31-39`).
- All `LoadMore` message handling.

This is **net code reduction**, not just movement.

### Trade-offs

| Aspect | Verdict |
|--------|--------|
| Layout cost | Constant w.r.t. transcript size (only visible rows) ✅ |
| Memory | Bounded — `sourceview5::View` count tied to viewport ✅ |
| Scroll fluidity | Native GTK4 — no `Load more` interruptions ✅ |
| Search jump | `ListView::scroll_to(pos)` works without realizing intermediate rows ✅ |
| Code complexity | Net reduction (pagination + batch machinery deleted) ✅ |
| Implementation risk | Medium — recycling discipline + measurement of bind() cost ⚠ |
| Scrollbar precision | Imprecise on highly variable row heights (one-line user vs 200-line assistant w/ code) ⚠ |
| Fast-scroll hitching | Possible — each new row builds a TextView + GtkSourceView on `bind` ⚠ |

---

## Proposal B — Manual windowing over `gtk::ListBox`

**Principle:** keep `gtk::ListBox` + `FactoryVecDeque`, but mount/unmount rows as the
scroll position changes. Maintain a sliding window of, say, the visible viewport ±
N rows.

### How it works

- Subscribe to the `ScrolledWindow`'s `vadjustment::value-changed`.
- Compute the current first/last visible factory index from cumulative row heights
  (must be cached — `compute_point()` is not cheap on every scroll tick).
- `guard.remove(idx)` rows outside the window; `guard.insert(idx, item)` rows that
  enter it.

### Trade-offs

| Aspect | Verdict |
|--------|--------|
| Layout cost | Same as Proposal A in steady state ✅ |
| Native idioms | Re-invents what GTK4 already does — works against the framework ❌ |
| Row height accounting | Manual — must cache, must handle expansion height changes ❌ |
| Scroll jitter | Mount/unmount during scroll causes layout reshuffles, visible jumps ❌ |
| Scrollbar | Wrong, unless we compute a synthetic adjustment from cached heights ❌ |
| Implementation risk | High — many edge cases around fast scroll, search jumps, dynamic heights |
| Code complexity | Net increase — adds windowing layer on top of existing factory |

There is no real upside over Proposal A. Proposal B exists in the design space only
because `TypedListView` is relatively new and worth de-risking against. After studying
its API, no Sessions Chronicle requirement justifies hand-rolling windowing.

---

## Proposal C — Tune pagination only (#132), no virtualization

**Principle:** continue along #132 — pick batch sizes empirically until median
`update_to_layout_us` per batch is under ~100 ms.

### Why this fails as a long-term answer

- Reduces the **per-batch** Layout cost but does not change the **steady-state** cost
  once a user has paged through enough rows. A session with 1500 transcript items still
  ends up with 1500 mounted rows after the user clicks `Load more` enough times.
- Does not address the search-jump UX — far matches still force pagination to mount the
  target row first.
- Does not address memory bloat from accumulated `sourceview5::View` instances.
- Keeps the `Load more` button — a UX dead end for a chat-like interface.

This proposal stays on the table only if the migration risk in Proposal A turns out
to be unacceptably high. The hard-point analysis below shows it isn't.

### Real-world variant: Newelle's scroll-triggered pagination

The [Newelle](https://github.com/qwersyk/Newelle) chat client (Python + GTK4) uses
`Gtk.ListBox` with **scroll-triggered** batched loading: 10 messages on open, additional
batches loaded when the user scrolls within ~10% of the top or bottom. Messages are
**never unmounted**.

This is a UX refinement of Proposal C — it removes the explicit `Load more` button —
but it does **not** change the structural picture:

- Still `gtk::ListBox`, no widget recycling.
- Append-only mounting; total mounted rows still grows monotonically with browsing.
- Steady-state Layout cost is identical to Proposal C once the user has scrolled
  through the session.

Newelle feels more fluid than Sessions Chronicle on small-to-medium sessions because
it starts with 10 rows and the chat-style trigger is invisible to the user. On the
Sessions Chronicle workload (longer sessions, denser per-message content including
GtkSourceView code blocks), the same approach would still hit the Layout wall — just
later, and without a visible `Loading…` indicator to set expectations.

Scroll-triggered loading is a useful UX pattern, but it belongs **on top of** the
virtualization in Proposal A, not as a substitute. After Proposal A lands, the
question of "what triggers loading" becomes moot — virtualization removes the need
for any explicit trigger.

---

## Hard-point analysis

This section walks through each hard point from the issue, scored for Proposal A.

### Search highlight state under widget recycling

**Status: already solved by the current design.**

Highlight is passed via `MessageItemInit.highlight_query` and applied inside
`render_content()` — the highlight is **a function of the data item, not the widget**.
In a `TypedListView`, `bind()` calls `render_content()` with the data item's stored
query and re-applies highlight to the recycled widget tree. No state survives on the
widget that would conflict with recycling.

When the user clears search, the model is updated in place (no row replacement
needed) and `notify_changed(idx)` re-fires `bind()`. Implementation cost: trivial.

### Row expansion state under recycling

**Status: requires moving burst expansion into the data model.**

- **Message expansion**: already model-resident (`TranscriptRow.expanded`). Direct port.
- **Burst expansion**: currently lives on the `gtk::Revealer` widget. Must move to
  `TranscriptItemData.expanded` (or a parallel field for bursts), with `bind()` reading
  the value and calling `revealer.set_reveal_child(value)` accordingly. The
  `children_built: Cell<bool>` lazy-population flag also moves to the data model so it
  survives recycling.

This change is **strictly better** than the current code: the burst's open/closed
state currently resets when the row scrolls offscreen and the user scrolls back —
even though today's `ListBox` happens not to unmount, the behavior is fragile and
the migration formalizes the right design.

Implementation cost: small, ~20 lines.

### Grouped tool call rows

**Status: not a hard point.**

A burst is already one row in the factory (`transcript_display.rs:15-31`). The
internal structure (header `Button` + `Revealer` + child `Box`) is preserved as the
row's widget tree under `TypedListView`. No `GtkSectionModel` or header-factory work
is needed.

Sub-child scrolling (jump to a specific tool call inside a burst) keeps working
because the burst row is mounted by the time `scroll_to()` resolves to that
position — at which point the existing Revealer traversal in
`session_detail.rs:1433-1485` operates on the live widget tree just as today.

### `AdwExpanderRow` and `ListBox` separators

**Status: not a hard point.**

The codebase uses neither. Manual `gtk::Separator` widgets in the SessionDetail view
header (`session_detail.rs:518, 544, 725`) sit outside the transcript list and are
unaffected.

### Scroll-to-row on search navigation

**Status: one well-bounded change.**

The single `ListBox`-specific call is
`messages_widget.observe_children().item(display_index)` (`session_detail.rs:1439`).
Replacement: `typed_view.view.scroll_to(display_index as u32, ListScrollFlags::NONE, None)`.
[`gtk::ListView::scroll_to`](https://gtk-rs.org/gtk4-rs/stable/latest/docs/gtk4/struct.ListView.html)
is GTK 4.12+; the project's minimum GTK version is satisfied (Flatpak runtime GNOME 47+).

After the row is realized (next layout pass), the existing `compute_point()` +
`vadjustment.set_value()` logic positions it 1/3 down the viewport — that step is
identical to today.

**Caveat — variable-height scrollbar drift**: with rows whose heights span 40 px
(one-line user message) to several thousand px (assistant message with multiple code
blocks), `ListView`'s scrollbar thumb is an estimate that settles as rows are realized.
A `scroll_to(far_index)` followed by user-initiated scroll-up to scan intermediate
rows may briefly see the scrollbar reposition. Acceptable for a transcript viewer;
documented as a known limitation.

### Bonus hard point: cost of `bind()` on fast scroll

Not in the issue, but the migration's most realistic risk. Each `bind()` for an
assistant message currently triggers:

- `pulldown-cmark` parse of full markdown
- TextView + TextBuffer creation
- One or more `sourceview5::View` + `sourceview5::Buffer` instantiations for code blocks
- Tool burst children population (lazy, controlled by the `children_built` flag)

If the user flicks the scroll wheel through a dense session, `bind()` may fire 20+
times per second. The cost is currently absorbed by `Load more` because the work happens
once at row creation; with virtualization it potentially runs on every scroll-in.

**Mitigation strategy (deferred, not blocking the migration):**

1. Measure `bind()` p50/p99 latency after the initial migration. If under one frame
   budget (~16 ms), no action needed.
2. If hitching is observed, pool `sourceview5::View` instances across rows (one of the
   levers identified in earlier perf work on the markdown renderer).
3. As a last resort, render a lightweight `gtk::Label` placeholder during fast scroll
   and swap in the rich widget tree after a short idle delay.

The migration unblocks these levers; none are required for v1.

---

## Trade-off summary

| Dimension | Proposal A (TypedListView) | Proposal B (Manual windowing) | Proposal C (Pagination only) |
|-----------|----------------------------|-------------------------------|------------------------------|
| Steady-state Layout cost | O(visible rows) | O(visible rows) | O(mounted rows) — grows |
| Memory | Bounded | Bounded | Grows with pagination |
| `Load more` UX | Removed | Removed | Retained (negative) |
| Code delta | Net reduction | Net increase | Status quo |
| Framework alignment | Native GTK4 path | Reinvents virtualization | Status quo |
| Implementation risk | Medium | High | Low (no work) |
| Solves the structural problem | ✅ | ✅ | ❌ |

---

## Decision: **Proceed with Proposal A**

Migrate `SessionDetail`'s transcript list from `gtk::ListBox` + `FactoryVecDeque` to
`relm4::typed_view::list::TypedListView`. The hard points are all manageable:

- Search highlight is already model-resident.
- Burst expansion state moves into the data model — a small, strictly-better change.
- Tool bursts remain single rows.
- No Adwaita list-box semantics to migrate.
- Scroll-to-row reduces to one API substitution.

The migration also lets us delete the `Load more` button, the pagination state
machinery, and the per-tick incremental render scheduling. The net effect is
**less code, lower steady-state cost, and a fluid scroll UX** matching user expectation
for chat-style interfaces.

### Follow-up implementation issue

Create a new issue titled **"Migrate SessionDetail transcript to TypedListView"**
that references this exploration and includes in its scope:

1. Define `TranscriptItemData: RelmListItem` with model-resident expansion + lazy-load state.
2. Implement `setup` / `bind` / `unbind` for the three row kinds (message, tool burst, single tool call).
3. Replace `observe_children().item(idx)` with `ListView::scroll_to`.
4. Remove `Load more` button, pagination state, and incremental render scheduling.
5. Measure `bind()` p50/p99 on a representative large session; record results in the PR.
6. Manual regression pass: search jump, burst expansion, scroll-to-match-in-burst-child,
   highlight clear/restore.

Out of scope of that issue (separate follow-ups if needed): `sourceview5::View` pooling,
placeholder-during-fast-scroll, sidebar virtualization.

### What would reopen this decision

- Measured `bind()` p99 > 50 ms after a good-faith implementation, with no obvious
  optimization path. Unlikely given the current per-row build time (1–3 ms total).
- A blocker in Relm4 0.10's `TypedListView` API discovered during implementation
  (e.g., a leak in widget recycling, a regression with custom `gio::Object` payloads).
  No such issue is documented today.
