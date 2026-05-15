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
| 3 | Burst expansion / lazy-children state location | In `TranscriptItemData`; user toggle goes through a Relm4 message, parent replaces the item at the same index so GTK rebinds the visible slot |
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
    pub children_built: Cell<bool>,    // lazy-population flag (burst only)
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
     `self.expanded.get()`: `revealer.set_reveal_child(true)` and, if
     `!self.children_built.get()`, build the per-tool-call children inside the
     revealer's box and set the flag.
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
3. Leave the `Stack` and per-page skeletons in place — they are reused on the
   next `bind`.

### `teardown(list_item)`

No-op. The default impl suffices; the pool slot is destroyed by GTK.

---

## Event flow: burst expansion toggle

Today the `Revealer` carries widget-level state. Under recycling that state
must live in the model.

```
user clicks burst header
   │
   ▼
button handler (connected in bind()):
   sender.input(SessionDetailMsg::ToggleBurst { item_index })
   │
   ▼
SessionDetail::update():
   if let Some(item) = self.messages.get(item_index as u32) {
       let mut next = item.borrow().clone();
       next.expanded.set(!next.expanded.get());
       self.messages.remove(item_index as u32);
       self.messages.insert(item_index as u32, next);
   }
   │
   ▼
TypedListView re-fires bind() for that slot
   │
   ▼
bind() reads expanded; revealer.set_reveal_child(expanded);
if expanded && !children_built: build children, set flag.
```

The same flow handles message expand and any other interactive toggle. No
`Rc<RefCell>` shared between widget closures and the data item — all
state lives in the store, all mutation goes through messages.

**Relm4 API constraint:** `TypedListView` in relm4 0.10.1 exposes `get`,
`remove`, `insert`, `clear`, and `extend_from_iter`, but it does not expose a
public `notify_changed` / `items_changed` hook for a single mutated item. For
single-row state changes, replacing the item at the same index is the explicit
refresh mechanism. If this causes unacceptable scroll-position jitter in
practice, the implementation should switch the row state to Relm4 bindings or a
custom GTK model with notifiable properties; that is a design change, not an
assumption hidden in the implementation.

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

Today: rows are replaced via the factory guard. Tomorrow, `TypedListView` does
not provide an in-place public notification API for mutated items, so query
updates replace the store contents while preserving per-row UI state:

```rust
let mut next_items = Vec::with_capacity(typed_view.len() as usize);
for item in typed_view.iter() {
    let mut next = item.borrow().clone();
    next.highlight_query = new_query.clone();
    next_items.push(next);
}
typed_view.clear();
typed_view.extend_from_iter(next_items);
```

This is O(n) in data items but still only realizes viewport rows. It avoids
depending on private `gio::ListStore` internals and keeps the spec aligned with
Relm4 0.10.1.

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
   (>1 viewport), scroll back, assert open. Same for closed, plus the
   children-built flag (children should not duplicate on second bind).

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
