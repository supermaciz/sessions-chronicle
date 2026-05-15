# SessionDetail Transcript: TypedListView Migration — Design

**Issue:** [#134](https://github.com/supermaciz/sessions-chronicle/issues/134)
**Source exploration:** [`docs/explorations/2026-05-14-transcript-virtualization-exploration.md`](../../explorations/2026-05-14-transcript-virtualization-exploration.md)
**Date:** 2026-05-15
**Status:** Design ready; implementation sequencing belongs in a separate plan/PR

---

## Purpose

Migrate `SessionDetail`'s transcript list from `gtk::ListBox` + `FactoryVecDeque` to
`relm4::typed_view::list::TypedListView`. The exploration doc above made the
**why** call (Proposal A); this spec makes the **how** decisions.

Out of scope of *deciding what to build* — the exploration already settled that.
In scope here: data model shape, widget recycling discipline, event flow,
refresh strategy, risk mitigation, and Definition of Done.

---

## Decisions Summary (rationale in body)

| # | Topic | Decision |
|---|-------|----------|
| 1 | Row polymorphism under one `TypedListView<T>` | Single `TranscriptItemData` struct with `kind` enum; per-slot widget is a `gtk::Stack` with 4 pre-built pages, `bind()` switches visible child |
| 2 | Full-content cache for expanded rows | None for v1 — re-fetch DB + re-parse on every `bind()`. Measure, add cache later if p99 > frame budget |
| 3 | Burst expansion / lazy-children state location | `expanded` is a `relm4::binding::BoolBinding` in `TranscriptItemData`, write-only-bound to the burst `Revealer.reveal-child` — no `ToggleBurst` message, no re-bind. Slot-local `children_built_for: Rc<Cell<Option<usize>>>` lives in `Widgets`. Children are built only when expanded: in `bind()` for rows that arrive expanded, in a `notify::reveal-child` handler for runtime toggles |
| 4a | Initial load mode | `extend_from_iter(all_rows)` in one call; no incremental render |
| 4b | Initial scroll position | Top (no change vs. today) |
| 5 | Scroll-to-match | `ListView::scroll_to(pos, NONE, None)` + existing idle callback for `compute_point` + 1/3-viewport adjustment + burst sub-child walk |
| 6 | Selection model | `gtk::NoSelection` — strict equivalent of today's `gtk::ListBox` with `selection_mode = None`. Keyboard nav at row level is a separate follow-up |
| 7 | Implementation sequencing | Separate plan/PR concern; this document records constraints and acceptance criteria only |

---

## Data model

### `TranscriptItemData`

Stored in the `gio::ListStore` backing the `TypedListView`. `relm4`'s
`TypedListView` wraps each `T` in a `gio::Object` automatically — no manual
`BoxedAnyObject` plumbing is required by user code.

```rust
#[derive(Debug, Clone)]
pub struct TranscriptItemData {
    pub item_index: usize,             // display position (factory-equivalent)
    pub transcript_item_index: i64,    // stable DB id
    pub kind: TranscriptItemKind,      // variant-specific payload
    pub expanded: relm4::binding::BoolBinding, // expansion state, recycle-stable
    pub highlight_query: Option<String>,
    pub sender: relm4::Sender<SessionDetailMsg>, // channel for row-internal actions
}

#[derive(Debug, Clone)]
pub enum TranscriptItemKind {
    Message(MessageItemInit),
    ToolCall(ToolCallItemInit),
    ToolBurst(ToolBurstItemInit),
    Subagent(SubagentItemInit),
}
```

The existing `*ItemInit` structs from `src/ui/transcript_row.rs:54-124` are
reused after making each variant payload `Clone` where needed. The conversion
from `TranscriptItemInit` (today's enum) to `TranscriptItemData` lives in
`src/ui/transcript_item_data.rs::from_init(init, sender)`.

**Why the data item carries a `Sender`.** `RelmListItem::setup()` and `bind()`
receive **no** `Sender` — the trait gives a recycled row no channel of its own.
Row-internal controls (tool-call / subagent "inspect" buttons, the message
expander, the burst header) must still reach `SessionDetail`. The only handle a
handler connected in `bind()` can reach is `&mut self`, so the channel travels
on the data item: `from_init` is given a clone of `SessionDetail`'s input
`Sender` and stores it. Every interactive handler captures `self.sender.clone()`
at `bind()` time. (`relm4::Sender` is `Clone`; if it is not `Debug`, drop the
`derive(Debug)` on `TranscriptItemData` or wrap accordingly — verify at
implementation time.)

**`expanded` is a `relm4::binding::BoolBinding`, not a `Cell<bool>`.** A
`BoolBinding` is a `Clone` `glib::Object` carrying a single `bool` "value"
property (verified in `relm4-0.10.1/src/binding/bindings.rs:86`). Two reasons:

- It lives in the data item, so expansion state survives widget recycling.
- For tool bursts, it is bound to the burst `Revealer`'s `reveal-child`
  property (`relm4` provides `ConnectBinding for gtk::Revealer`,
  `src/binding/widgets.rs:44`). Mutating the binding moves the revealer with no
  message round-trip.

Because `BoolBinding` is `Clone` as a refcounted `glib::Object`, cloning a
`TranscriptItemData` (the highlight refresh does this) shares the *same*
binding object — the expansion state is correctly carried over, not copied.

For **message** rows the field is used as a plain bool store (`.get()` /
`.set()`); message expansion swaps rendered content rather than toggling a
single GTK property, so it is not bound to a widget — see the event-flow
section.

**Why one struct with `kind` rather than four `TypedListView`s:** `TypedListView<T>`
is parameterized by a single `T`. Four lists would break chronological order
and require composite scrolling — unworkable. The single-type + Stack approach
is the documented pattern.

### Widget skeleton per slot

`setup()` builds once per pool slot (typically ~30-50 slots in practice; GTK
sizes the pool to viewport + overscan):

```
gtk::Box .transcript-row-slot
└── gtk::Stack .transcript-row-stack (transition-type = StackTransitionType::None)
    ├── child "message"    → gtk::Box (role label + content container)
    ├── child "tool_call"  → gtk::Box (tool name + status badge + preview)
    ├── child "tool_burst" → gtk::Box (header gtk::Button + gtk::Revealer{gtk::Box})
    └── child "subagent"   → gtk::Box (title + status)
```

Memory impact: ~4× the empty-skeleton cost per slot. Empty skeletons are a
handful of KiB each; this is negligible against `sourceview5::View` instances,
which only exist on the visible page and are released at `unbind()`.

### `Widgets` companion struct

Holds typed references to per-page sub-widgets **and** the `Vec<SignalHandlerId>`
of every signal connected by `bind()`. `unbind()` iterates and disconnects to
prevent handler accumulation across recycle cycles.

```rust
pub struct Widgets {
    pub stack: gtk::Stack,
    pub message_page: MessagePageWidgets,
    pub tool_call_page: ToolCallPageWidgets,
    pub tool_burst_page: ToolBurstPageWidgets,
    pub subagent_page: SubagentPageWidgets,
    pub connected_handlers: Vec<(glib::Object, glib::SignalHandlerId)>,
}
```

`ToolBurstPageWidgets` owns the physical child-build state for the recycled
slot:

```rust
pub struct ToolBurstPageWidgets {
    pub header_button: gtk::Button,
    pub revealer: gtk::Revealer,
    pub children_box: gtk::Box,
    /// glib::Binding from the bound item's `expanded` BoolBinding to
    /// `revealer.reveal-child`. `None` between unbind and the next bind.
    pub reveal_binding: Option<glib::Binding>,
    /// `item_index` whose children currently populate `children_box`,
    /// or `None` if empty. `Rc<Cell>` because the `notify::reveal-child`
    /// handler (connected in `bind()`) also reads and updates it.
    pub children_built_for: Rc<Cell<Option<usize>>>,
}
```

Two fields deliberately live in `Widgets`, not `TranscriptItemData`:

- `children_built_for` — whether *this physical* `children_box` holds widgets
  for a given burst. Slots are recycled, so this is not stable row data. A
  model-resident `children_built` flag would wrongly report "built" for a burst
  whose children were built into a *different* slot. It is an `Rc<Cell<…>>`
  because both `bind`/`unbind` (`&mut Widgets`) and the `notify::reveal-child`
  closure must read and write it.
- `reveal_binding` — the `glib::Binding` handle (see `bind` / `unbind`).

There is no separate `BurstLazyState` bundle. The `notify::reveal-child` handler
is connected in `bind()` (it needs `self.sender` and the current `tool_calls`,
neither available in `setup()`), so it simply captures everything it needs —
`children_box`, `revealer`, a clone of `children_built_for`, a clone of
`self.sender`, the burst's `Rc<[ToolCallItemInit]>`, `self.item_index`, and
`self.highlight_query` — by value at connection time.

---

## Lifecycle (RelmListItem impl)

### `setup(list_item) -> (Root, Widgets)`

Build the 4-page `gtk::Stack` skeleton described above and the empty
`children_built_for: Rc<Cell<Option<usize>>>`. No item data is consulted, and
**no signal handlers are connected** — `setup()` receives no `Sender`, so any
handler needing to reach `SessionDetail` cannot be wired here. This runs once
per pool slot, before any item is bound.

### `bind(&mut self, widgets, root)`

1. `widgets.stack.set_visible_child_name(...)` based on `self.kind`.
2. Populate the active page from `self.kind`:
   - **Message**: render preview text from `MessageItemInit.preview`. If
     `self.expanded.get()`, kick off full-content load (existing
     `render_content` path from `transcript_row.rs:264-301`) and render with
     `highlight_query`.
   - **ToolCall**: fill name, status badge, preview, duration.
   - **ToolBurst**: fill header label (category counts + error count), then run
     the **burst bind sequence** below.
   - **Subagent**: fill title, status.
3. Connect interactive handlers (inspect actions, the burst header button, the
   burst `notify::reveal-child` handler). Each captures `self.sender.clone()`
   and any other item-specific data it needs. Each `connect_*` returns a
   `SignalHandlerId`; push `(object, id)` into `widgets.connected_handlers` so
   `unbind()` disconnects it.

**Burst bind sequence** (ordering matters — see notes):

   a. Connect the `notify::reveal-child` handler on `widgets.revealer`. It
      captures: `children_box`, `revealer`, a clone of `children_built_for`, a
      clone of `self.sender`, the burst's `Rc<[ToolCallItemInit]>`,
      `self.item_index`, and `self.highlight_query`. Its body is the **burst
      child build** below. Push its id into `connected_handlers`.
   b. **Lazy build, gated on expansion.** If `self.expanded.get()` is `true`,
      run the burst child build now (so a row that *arrives* expanded shows its
      children immediately). If `false`, build nothing — a collapsed burst
      realizes **zero** child widgets. This is the actual laziness.
   c. Establish the reveal binding **write-only**, capturing the handle:
      ```rust
      let handle = self.expanded
          .bind_property("value", &widgets.revealer, "reveal-child")
          .sync_create()        // revealer immediately matches `expanded`
          .build();             // glib::Binding
      widgets.reveal_binding = Some(handle);
      ```
      `RelmObjectExt::add_binding` / `ConnectBinding::bind` are **not** used:
      they discard the returned `glib::Binding`, leaving no handle to unbind on
      recycle. The binding is write-only (`expanded → revealer`); see the
      event-flow section for why bidirectional is unnecessary.

**Burst child build** (shared by step b and the `notify::reveal-child`
handler): if `children_built_for.get() != Some(item_index)`, clear
`children_box`, build the per-tool-call children for `item_index` (wiring their
inspect buttons to the captured `sender`), then
`children_built_for.set(Some(item_index))`. If it already equals `item_index`,
do nothing — this is what prevents a duplicate build when the same burst is
expanded twice without an intervening recycle.

**Why not rely on `sync_create` → `notify` for the bind-time build:** a GObject
`notify::reveal-child` fires only when the value *changes*. A recycled slot
whose `Revealer` was already `reveal-child = true` (previous item expanded),
rebound to another expanded burst, gets `sync_create` writing `true → true` —
no change, no `notify`. Step b therefore builds explicitly, gated on
`self.expanded.get()`, rather than depending on the signal. The handler from
step a covers the *other* case: a realized, collapsed burst the user expands at
runtime (no `bind()` involved).

### `unbind(&mut self, widgets, root)`

1. Iterate `widgets.connected_handlers`, call `obj.disconnect(id)`, clear the
   vec.
2. `widgets.reveal_binding.take()` and call `.unbind()` on it. Without this a
   recycled slot accumulates bindings to previous items' `expanded` objects.
3. Clear text content of the active page's `TextView` (`buffer.set_text("")`)
   and drop any `sourceview5::View` references in the page (`remove()` from
   their container so their `Buffer` drops).
4. Clear the burst page's `children_box` and set
   `children_built_for.set(None)`; burst children are tied to a physical slot
   and must not leak across recycled items.
5. Leave the `Stack` and per-page skeletons in place — they are reused on the
   next `bind`.

All handlers — including `notify::reveal-child` — are connected in `bind()` and
disconnected here in step 1; none is connected in `setup()`.

### `teardown(list_item)`

No-op. The default impl suffices; the pool slot is destroyed by GTK.

---

## Event flow: burst expansion toggle

Burst expansion needs **no** `SessionDetailMsg` and **no** store re-bind. The
`expanded` `BoolBinding` is the shared source of truth: it lives in the data
item (recycle-stable) and `bind()` write-only-binds it to the slot's
`Revealer.reveal-child`.

Two slot-local signal handlers cooperate. **Both are connected in `bind()` and
disconnected in `unbind()`** (tracked in `connected_handlers`):

```
user clicks burst header (row is necessarily realized)
   │
   ▼
header_button "clicked" handler — captures a clone of `expanded` (BoolBinding
is a Clone glib::Object):
   expanded.set(!expanded.get());
   │
   │  write-only binding propagates value → revealer.reveal-child
   ▼
revealer "notify::reveal-child" handler — captures children_box, revealer,
the Rc<Cell> children_built_for, the sender, the Rc<[ToolCallItemInit]>,
item_index, highlight_query:
   if revealer.reveals_child() {
       // burst child build (shared with bind step b)
       if children_built_for.get() != Some(item_index) {
           clear children_box;
           build children for item_index, wiring inspect buttons to sender;
           children_built_for.set(Some(item_index));
       }
   }
```

- The button handler only flips the binding — instant, no `items-changed`, no
  scroll jitter, no keyboard-focus loss.
- The notify handler performs lazy child population for the **runtime** case: a
  realized, collapsed burst the user expands. The *arrives-expanded* case is
  handled explicitly by `bind()` step b (see the burst bind sequence and its
  note on why `sync_create` → `notify` cannot be relied on).
- On collapse (`reveal-child` → false) the notify handler does nothing; built
  children stay in `children_box` until `unbind()` or a different burst rebinds
  into the slot.

**Why write-only, not bidirectional:** the `Revealer` is non-interactive — it
never changes `reveal-child` on its own. Every change originates from a write
to the `expanded` binding. Bidirectional sync would buy nothing and add a
feedback path. **Invariant:** no code calls `revealer.set_reveal_child()`
directly; the `BoolBinding` is the only write surface. The search-jump path
that needs a burst expanded (to scroll to a tool-call sub-child) therefore does
`item.borrow().expanded.set(true)`, not a direct revealer call.

**Handler connection points:** both handlers capture item-specific data — the
button handler the current item's `expanded` binding, the notify handler the
current `sender` / `tool_calls` / `item_index`. `RelmListItem::setup()` receives
no `Sender` and runs before any item exists, so neither can be wired there. Both
are connected in `bind()`, recorded in `connected_handlers`, and disconnected in
`unbind()`.

**Relm4 API note (verified against relm4 0.10.1):** `TypedListView`'s backing
`store: gio::ListStore` is private — there is no public `notify_changed` /
`items_changed` hook for a single item, and `RelmObjectExt::add_binding`
discards its `glib::Binding`. The design above sidesteps both: the binding
*is* the state channel, and its handle is captured via `bind_property().build()`
directly (see the burst bind sequence).

### Message expansion is different

Message rows have no `Revealer`: expanding a message swaps *rendered content*
(preview ↔ full markdown, the latter lazy-loaded from the DB). A property
binding cannot express that. Message expansion therefore keeps a handler +
parent-message flow:

- The message expander button sends `SessionDetailMsg::ToggleMessageExpand
  { item_index }`.
- `SessionDetail::update` flips that item's `expanded` binding via
  `get(idx).borrow().expanded.set(...)`, and — if expanding — starts the async
  full-content load (the existing `render_content` / command path).
- The realized row is updated directly by the load completion; `expanded`
  persists the state for later recycles, where `bind()` re-reads it.

`expanded` being a `BoolBinding` for message rows too is harmless: it is simply
used as a `.get()`/`.set()` bool store, not bound to any widget property.

---

## Initial load

`start_first_page_load` (today: 75 rows incremental) is simplified:

1. Worker thread: load the entire session transcript through a new
   non-paginated helper (`load_all_transcript_items`) or an explicit full-range
   mode on the existing `load_transcript_items(db_path, session_id, limit,
   offset, preview_len)` query.
2. UI thread: convert to `Vec<TranscriptItemData>` via `from_init`, then
   `typed_view.extend_from_iter(items)`.
3. `ListView` realizes only viewport rows on the next layout pass.
4. No explicit `scroll_to` — natural top position.

**Deleted as a consequence:**

- Constants `INITIAL_PAGE_SIZE`, `NEXT_PAGE_SIZE`, `RENDER_BATCH_SIZE`,
  `RENDER_BATCH_WATCHDOG_MS` (`session_detail.rs:31-39`).
- Fields `loading_first_page`, `loading_next_page`, `has_more_messages`,
  `pending_render_batch`.
- Messages `LoadMore` and its handlers.
- The `Load more` button + its row factory placeholder.
- The per-tick batch render scheduler and its watchdog timer
  (`session_detail.rs:1860-1920`).

**Preserved:**

- `match_positions: Vec<MatchPosition>` (`session_detail.rs:64`).
- `display_targets_by_item_index: BTreeMap<i64, ScrollTarget>` (`session_detail.rs:65`).
- `scroll_to_item: Cell<Option<ScrollTarget>>` (`session_detail.rs:70`).
- All search infrastructure (match counting, highlight application via the
  model).

---

## Scroll-to-match

The current path (`session_detail.rs:1433-1485, 2588-2603`) is preserved
end-to-end except for the row-lookup primitive.

**Before:**
```rust
let row = messages_widget.observe_children().item(target.display_index as u32);
```

**After:**
```rust
typed_view.view.scroll_to(
    target.display_index as u32,
    gtk::ListScrollFlags::NONE,
    None, // ScrollInfo
);
// existing idle_add_local_once → walk widget tree → compute_point
// → vadjustment.set_value(target_y - viewport_h / 3.0)
```

`gtk::ListView::scroll_to` is GTK 4.12+; the project's Flatpak runtime
(GNOME 49) provides it. Documented in
[`gtk4::ListView`](https://gtk-rs.org/gtk4-rs/stable/latest/docs/gtk4/struct.ListView.html).

**Fallback for race:** if the idle callback's widget-tree walk yields `None`
(row not yet realized after `scroll_to`), retry once via
`add_tick_callback`. After a second miss, log debug and abort the
fine-positioning step — `scroll_to` already brought the row into the
viewport, which is acceptable degradation.

**Sub-child scrolling** (jumping to a specific tool call inside a burst):
unchanged. By the time the idle callback runs, the burst row is realized;
the existing `Revealer` traversal in `session_detail.rs:1433-1485` operates
on the live widget tree exactly as today.

**Variable-height scrollbar drift:** documented limitation, acknowledged in
the exploration. No mitigation in this spec.

---

## Search highlight clear / restore

A query change affects **every** row, including off-screen ones — so unlike the
burst toggle this cannot be handled purely on live widgets. The model must be
updated for all items; on-screen rows must additionally be re-bound.

**Step 1 — update the model in place (all items, cheap):**

```rust
for item in typed_view.iter() {
    item.borrow_mut().highlight_query = new_query.clone();
}
```

`TypedListItem::borrow_mut()` mutates the wrapped value without reallocating the
`gio::Object` wrappers. Off-screen rows need nothing more — `bind()` reads the
updated `highlight_query` when they scroll in.

**Step 2 — refresh the on-screen rows.** `TypedListView` exposes no single-item
re-bind hook (its `store` is private). The available public primitive that
forces a re-bind is `clear()` + `extend_from_iter()`. Used naively this is a
**regression**: `clear()` empties the store, which collapses the
`ScrolledWindow`'s vertical adjustment and resets the scroll position to the
top. If the highlight refreshes while the user is scrolled into the transcript,
the list jumps to the top.

The fix is to save and restore the scroll position around the refresh:

```rust
let vadj = scrolled_window.vadjustment();
let saved_value = vadj.value();

let refreshed: Vec<_> = typed_view.iter().map(|i| i.borrow().clone()).collect();
typed_view.clear();
typed_view.extend_from_iter(refreshed);

// Restore after the next layout pass, once the adjustment upper has grown back.
glib::idle_add_local_once(clone!(@strong vadj => move || {
    vadj.set_value(saved_value.min(vadj.upper() - vadj.page_size()));
}));
```

Step 1's in-place mutation is still done first so the cloned items in step 2
already carry the new query.

**Cost and caveat:** step 2 rebuilds all `n` `gio::Object` wrappers. This is
acceptable for an explicit search submit / clear. If highlight is refreshed on
every keystroke, the refresh must be debounced (a separate concern from this
migration) — or replaced with a surgical `remove` + `insert` over only the
visible index range, which avoids the full rebuild and the scroll-restore dance
but requires computing that range from the `ListView`. Surgical refresh is the
documented optimization path; the save/restore approach above is the v1 baseline.

---

## Implementation constraints

This document is not the implementation plan. A follow-up plan or PR
description should decide commit boundaries, but the implementation must satisfy
these constraints:

- Introduce `TranscriptItemData` and its `RelmListItem` implementation before
  wiring it into `SessionDetail`, so the recycling lifecycle can be tested in
  isolation.
- Keep the legacy `FactoryComponent` row code until the `TypedListView` path is
  wired, manually verified, and measured.
- Reuse existing content-rendering helpers where practical; if helper extraction
  would make the legacy row harder to read, prefer a small duplicated adapter
  over broad churn.
- Replace `messages: FactoryVecDeque<TranscriptRow>` with
  `messages: TypedListView<TranscriptItemData, gtk::NoSelection>`, and use the
  public `typed_view.view` field as the `gtk::ListView` widget.
- Remove pagination and render-batch machinery only after full-session loading
  and virtualized rendering are working together.
- Update tests around behavior, not pagination internals: row count, search
  jump, highlight updates, burst persistence, signal cleanup, and measurement
  instrumentation.

---

## Risks and mitigations

1. **`bind()` cost on fast scroll.** Identified in the exploration § bonus
   hard point. Mitigation: measure first with a `bind_duration_us` trace. If p99 > 16 ms,
   leverage `sourceview5::View` pooling, `Arc<str>` content cache in the
   data item, or fast-scroll placeholders. None blocks v1.

2. **Signal handler *and* property-binding accumulation across recycle
   cycles.** Core risk of the `bind`/`unbind` discipline. Two leaks to prevent:
   - Signal handlers: `connected_handlers: Vec<...>` in `Widgets`, every
     per-bind connect pushes, every `unbind` disconnects and clears.
   - The reveal `glib::Binding`: stored in `reveal_binding`, `unbind` calls
     `.unbind()` and takes it. A leaked bidirectional/write-only binding would
     leave a recycled slot's `Revealer` driven by a previous item's `expanded`.

   Test: 1000-cycle bind/unbind loop on a burst row; assert `connected_handlers`
   returns to empty and `reveal_binding` to `None` after each `unbind`, and that
   the row stays functional. All handlers — button click and
   `notify::reveal-child` — are connected in `bind()` and so are covered by
   `connected_handlers`; `setup()` connects none.

3. **Variable-height scrollbar drift.** Known, documented, accepted.

4. **`ListView::scroll_to` not yet realized when idle callback runs.**
   Mitigation: single-retry via `add_tick_callback`, silent abort
   thereafter. The user still ends up with the row in view from `scroll_to`
   itself; only the 1/3-viewport refinement is degraded.

5. **Burst state persistence regressions.** Test plan: open, scroll out
   (>1 viewport), scroll back, assert open. Same for closed, plus slot reuse:
   children are rebuilt for the current burst when a recycled slot is reused,
   and are not duplicated on repeated binds of the same item into the same slot.

---

## Measurements to publish in the PR description

- `bind_duration_us` p50 / p99 per variant
  (Message / ToolCall / ToolBurst / Subagent), captured on the longest
  fixture under `tests/fixtures/`.
- `update_to_layout_us` p50 / p99 on the frame following a full-viewport
  scroll — compared to the 1.3–1.4 s baseline from #146.
- Approximate RSS before and after full scroll on a long session, captured
  via `ps -o rss= -p $(pgrep sessions-chronicle)` snapshots.

---

## Out of scope (separate follow-ups if needed)

- `sourceview5::View` pooling across rows.
- Lightweight placeholder during fast-scroll.
- Per-session "last viewed position" memory.
- Sidebar (session list) virtualization.
- Keyboard `↑/↓` row navigation via `gtk::SingleSelection`.
- Full-content cache (`Arc<str>`) on expanded `TranscriptItemData`.

---

## Definition of Done

- `cargo fmt --all -- --check` passes.
- `cargo clippy --all -- -D warnings` passes.
- `cargo test --all --no-fail-fast` passes.
- Flatpak build verified:
  `flatpak-builder --user flatpak_app build-aux/dev.maciz.sessionschronicle.Devel.json --force-clean`.
- Before/after screenshots on a long fixture.
- `bind_duration_us` and `update_to_layout_us` numbers in the PR description.
- Manual regression pass:
  - Search jump to a far match (message and sub-child of burst).
  - Burst expand → scroll out (>1 viewport) → scroll back → still expanded.
  - Burst collapse → same trip → still collapsed.
  - Highlight on then off, content re-renders correctly.
  - Open a 1000+ item session, scroll continuously top-to-bottom, no
    visible stutter beyond the per-row `bind` cost budget.
