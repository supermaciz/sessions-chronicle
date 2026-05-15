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
| 3 | Burst expansion / lazy-children state location | `expanded` lives in `TranscriptItemData`; widget child-build state lives in the recycled slot's `Widgets`. Toggle updates the live `Revealer` directly and persists `expanded` via `borrow_mut()` — no `remove`/`insert`, no re-bind |
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
    pub expanded: Cell<bool>,          // pliage commun (message + burst)
    pub highlight_query: Option<String>,
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
`src/ui/transcript_item_data.rs::from_init`.

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
    pub revealer: gtk::Revealer,
    pub children_box: gtk::Box,
    pub children_built_for: Option<usize>, // TranscriptItemData.item_index
    // header button, arrow icon, labels, ...
}
```

`children_built_for` deliberately lives in `Widgets`, not
`TranscriptItemData`. It describes whether this physical `children_box` already
contains GTK child widgets for the currently bound burst. Since GTK recycles
slots, that is not stable row data.

---

## Lifecycle (RelmListItem impl)

### `setup(list_item) -> (Root, Widgets)`

Build the 4-page `gtk::Stack` skeleton described above. No data is consulted —
this runs once per pool slot, before any item is bound.

### `bind(&mut self, widgets, root)`

1. `widgets.stack.set_visible_child_name(...)` based on `self.kind`.
2. Populate the active page from `self.kind`:
   - **Message**: render preview text from `MessageItemInit.preview`. If
     `self.expanded.get()`, kick off full-content load (existing
     `render_content` path from `transcript_row.rs:264-301`) and render with
     `highlight_query`.
   - **ToolCall**: fill name, status badge, preview, duration.
   - **ToolBurst**: fill header label (category counts + error count). If
     `self.expanded.get()`: ensure `children_built_for == Some(self.item_index)`
     by clearing the slot's burst child container when needed and building the
     per-tool-call children for this item, then
     `revealer.set_reveal_child(true)`.
   - **Subagent**: fill title, status.
3. Connect interactive handlers (buttons, inspect actions). Each
   `connect_clicked` returns a `SignalHandlerId`; push it into
   `widgets.connected_handlers`.

### `unbind(&mut self, widgets, root)`

1. Iterate `widgets.connected_handlers` and call `obj.disconnect(id)`. Clear the
   vec.
2. Clear text content of the active page's `TextView` (`buffer.set_text("")`)
   and drop any `sourceview5::View` references in the page (`remove()` from
   their container so their `Buffer` drops).
3. Clear the burst page's `children_box` and set `children_built_for = None`;
   burst children are bound to a physical slot and must not leak across
   recycled items.
4. Leave the `Stack` and per-page skeletons in place — they are reused on the
   next `bind`.

### `teardown(list_item)`

No-op. The default impl suffices; the pool slot is destroyed by GTK.

---

## Event flow: burst expansion toggle

Today the `Revealer` carries widget-level state. Under recycling that state
must also live in the model — but the toggle itself does **not** require a
re-bind.

**Key observation:** a burst header button can only be clicked when its row is
realized and on-screen. The handler therefore has direct access to the live
`Revealer` and can update it immediately. The message to the parent exists only
to persist the new state into the model, so it survives a later recycle.

```
user clicks burst header (row is necessarily realized)
   │
   ├─▶ in the button handler, on the live widget:
   │      revealer.set_reveal_child(new_expanded);
   │      if new_expanded && children_built_for != Some(item_index) {
   │          clear children_box;
   │          build children for item_index;
   │          children_built_for = Some(item_index);
   │      }
   │      → instant feedback, no items-changed emission, no scroll jitter,
   │        no keyboard-focus loss.
   │
   └─▶ sender.input(SessionDetailMsg::ToggleBurst { item_index, expanded })
          │
          ▼
       SessionDetail::update():
          if let Some(item) = self.messages.get(item_index as u32) {
              let mut row = item.borrow_mut();   // TypedListItem::borrow_mut, verified present
              row.expanded.set(expanded);
          }
          → pure persistence write. No remove/insert, no re-bind.
```

When the row is later recycled offscreen and scrolled back in, `bind()` reads
`expanded` from the model and restores the `Revealer` to the correct state. If
the row is expanded, `bind()` also ensures this slot's `children_box` contains
children for the current `item_index`. The model is the source of truth for
*recycled* row expansion; the live widget is authoritative while the row is
realized, and the two are kept in sync by the handler above.

The same pattern handles message expand and any other interactive toggle on a
realized row. No `Rc<RefCell>` shared between widget closures and the data item.

**Relm4 API constraint (verified against relm4 0.10.1):** `TypedListView` exposes
`get`, `remove`, `insert`, `clear`, `extend_from_iter`, and `iter`. Its backing
`store: gio::ListStore` is **private** — there is no public `notify_changed` /
`items_changed` hook to force a re-bind of a single mutated item.
`TypedListItem::borrow_mut()` (`typed_view/mod.rs:76`) **is** public, so the
model can be mutated in place; that mutation alone does not re-fire `bind()`.

This is why the toggle updates the live widget directly instead of mutating the
model and forcing a refresh: for an on-screen row, no refresh primitive is
needed. `remove` + `insert` at the same index *would* re-bind the slot, but it
also destroys and recreates the widget — causing scroll jitter and keyboard-focus
loss — so it is **not** used for the toggle path. It remains available only if a
future requirement needs an off-screen row to change appearance, which the
current design does not have.

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

2. **Signal handler accumulation across recycle cycles.** Core risk of the
   `bind`/`unbind` discipline. Mitigation: `connected_handlers: Vec<...>`
   in `Widgets`, every connect pushes, every `unbind` iterates and
   disconnects. Test: 1000-cycle bind/unbind loop on a burst row, assert the
   vec length returns to 0 after each unbind and that the row remains
   functional.

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
