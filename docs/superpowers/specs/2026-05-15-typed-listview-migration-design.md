# SessionDetail Transcript: TypedListView Migration — Design

**Issue:** [#134](https://github.com/supermaciz/sessions-chronicle/issues/134)
**Source exploration:** [`docs/explorations/2026-05-14-transcript-virtualization-exploration.md`](../../explorations/2026-05-14-transcript-virtualization-exploration.md)
**Date:** 2026-05-15
**Status:** Ready for implementation planning

---

## Purpose

Migrate `SessionDetail`'s transcript list from `gtk::ListBox` + `FactoryVecDeque` to
`relm4::typed_view::list::TypedListView`. The exploration doc above made the
**why** call (Proposal A); this spec makes the **how** decisions and structures the
work so it can be reviewed commit by commit.

Out of scope of *deciding what to build* — the exploration already settled that.
In scope here: data model shape, widget recycling discipline, event flow,
migration sequencing, risk mitigation, and Definition of Done.

---

## Decisions Summary (rationale in body)

| # | Topic | Decision |
|---|-------|----------|
| 1 | Row polymorphism under one `TypedListView<T>` | Single `TranscriptItemData` struct with `kind` enum; per-slot widget is a `gtk::Stack` with 4 pre-built pages, `bind()` switches visible child |
| 2 | Full-content cache for expanded rows | None for v1 — re-fetch DB + re-parse on every `bind()`. Measure, add cache later if p99 > frame budget |
| 3 | Burst expansion / lazy-children state location | In `TranscriptItemData`; user toggle goes through a Relm4 message, parent updates the store, `items_changed` re-fires `bind()` |
| 4a | Initial load mode | `extend_from_iter(all_rows)` in one call; no incremental render |
| 4b | Initial scroll position | Top (no change vs. today) |
| 5 | Scroll-to-match | `ListView::scroll_to(pos, NONE, None)` + existing idle callback for `compute_point` + 1/3-viewport adjustment + burst sub-child walk |
| 6 | Selection model | `gtk::NoSelection` — strict equivalent of today's `gtk::ListBox` with `selection_mode = None`. Keyboard nav at row level is a separate follow-up |
| 7 | Migration sequencing | One PR, 6 atomic commits |

---

## Data model

### `TranscriptItemData`

Stored in the `gio::ListStore` backing the `TypedListView`. `relm4`'s
`TypedListView` wraps each `T` in a `gio::Object` automatically — no manual
`BoxedAnyObject` plumbing is required by user code.

```rust
pub struct TranscriptItemData {
    pub item_index: usize,             // display position (factory-equivalent)
    pub transcript_item_index: i64,    // stable DB id
    pub kind: TranscriptItemKind,      // variant-specific payload
    pub expanded: Cell<bool>,          // pliage commun (message + burst)
    pub children_built: Cell<bool>,    // lazy-population flag (burst only)
    pub highlight_query: Option<String>,
}

pub enum TranscriptItemKind {
    Message(MessageItemInit),
    ToolCall(ToolCallItemInit),
    ToolBurst(ToolBurstItemInit),
    Subagent(SubagentItemInit),
}
```

The existing `*ItemInit` structs from `src/ui/transcript_row.rs:54-124` are
reused as-is. The conversion from `TranscriptItemInit` (today's enum) to
`TranscriptItemData` lives in `src/ui/transcript_item_data.rs::from_init`.

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
   if let Some(item) = self.messages.get(item_index) {
       let expanded = !item.borrow().expanded.get();
       item.borrow().expanded.set(expanded);
       self.messages.notify_changed(item_index);
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

**Verification at implementation time:** confirm `TypedListView::get(pos)`
returns a `TypedListItem<T>` whose `borrow_mut()` accessor exists; if not,
adapt to `find` + `notify_changed`. Either way the pattern holds.

---

## Initial load

`start_first_page_load` (today: 75 rows incremental) is simplified:

1. Worker thread: `load_session_messages(db_path, session_id)` returns
   `Vec<TranscriptItemInit>` for the entire session (current code already
   supports this; the 75-row cutoff is at the UI layer).
2. UI thread: convert to `Vec<TranscriptItemData>` via `from_init`, then
   `typed_view.extend_from_iter(items)`. Single `items-changed` emission.
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
typed_view.view().scroll_to(
    target.display_index as u32,
    gtk::ListScrollFlags::NONE,
    None, // ScrollInfo
);
// existing idle_add_local_once → walk widget tree → compute_point
// → vadjustment.set_value(target_y - viewport_h / 3.0)
```

`gtk::ListView::scroll_to` is GTK 4.12+; the project's Flatpak runtime
(GNOME 48) provides it. Documented in
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

Today: rows are replaced via the factory guard. Tomorrow:

```rust
let n = typed_view.len();
for i in 0..n {
    let item = typed_view.get(i).unwrap();
    item.borrow_mut().highlight_query = new_query.clone();
}
// Trigger a re-bind for all currently realized rows. The exact API call
// (whether to drive items_changed via the backing ListStore directly, or
// to use TypedListView's notify_changed loop) is verified at implementation
// time against relm4 0.10.1; both options exist in the API surface.
```

This re-fires `bind()` for visible rows only — bounded by viewport size.

---

## Migration plan: 6 atomic commits in one PR

PR title: **"Migrate SessionDetail transcript to TypedListView"**.
Each commit must pass `cargo fmt --all -- --check && cargo clippy --all -- -D warnings && cargo test --all --no-fail-fast`.

**Commit 1 — Introduce `TranscriptItemData`**
- New file `src/ui/transcript_item_data.rs`.
- Defines `TranscriptItemData`, `TranscriptItemKind`, and `from_init(TranscriptItemInit) -> TranscriptItemData`.
- Unit tests covering each `kind` variant conversion.
- No changes to `session_detail.rs` or rendering. Code is dead-shipped.

**Commit 2 — Implement `RelmListItem` for `TranscriptItemData`**
- In a new module `src/ui/transcript_row_view.rs` (kept separate from the
  legacy `transcript_row.rs` `FactoryComponent` until commit 6).
- Defines `Widgets` (with `connected_handlers: Vec<...>`), implements
  `setup` / `bind` / `unbind` / `teardown`.
- Reuses existing helpers from `transcript_row.rs` for content rendering
  (`render_content`, `populate_tool_burst_children`) — these are extracted as
  pub(crate) where needed, no logic duplication.
- Tests: snapshot of `setup` structure (stack present, 4 children, correct
  CSS classes), and per-variant `bind` smoke test via `gtk::test`.
- Not yet wired into `SessionDetail`.

**Commit 3 — Wire `TypedListView` into `SessionDetail`; remove pagination and batch machinery**
- Replace `messages: FactoryVecDeque<TranscriptRow>` with
  `messages: TypedListView<TranscriptItemData, gtk::NoSelection>`.
- Replace the `Box` from the factory with `typed_view.view()` as `messages_box`.
- Simplify `start_first_page_load` to load the full session in one DB pass +
  `extend_from_iter`.
- Delete: pagination constants and fields, `Load more` button, `LoadMore`
  message handlers, `PendingRenderBatch`, batch tick scheduler, watchdog.
- Integration tests adjusted: no more pagination assertions; assertions on
  total row count via `typed_view.len()`.

**Commit 4 — Migrate scroll-to-match**
- Swap `observe_children().item(idx)` for
  `typed_view.view().scroll_to(...)`.
- Preserve the idle callback, 1/3-viewport `compute_point` step, and burst
  sub-child walk.
- Add the single-retry `add_tick_callback` fallback described above.
- Integration test: search jump on a long fixture, assert the matched row's
  bounding box lands within `[viewport_h/4, viewport_h/2]` of the viewport top.

**Commit 5 — Migrate burst toggle to the Relm4 message flow**
- Burst button handler in `bind()` sends `ToggleBurst { item_index }` via the
  `Sender` stashed in `Widgets`.
- `SessionDetail::update` handles it by mutating the store item and calling
  `items_changed(idx, 1, 1)`.
- Remove all widget-level `Revealer` state plumbing for bursts.
- Tests: toggle a burst, scroll it out then back in, assert the state
  persists; same for `children_built` (children are not rebuilt on second
  bind).

**Commit 6 — Cleanup and measurement**
- Delete the legacy `FactoryComponent` impl in `transcript_row.rs` (now
  unused). Keep any shared helpers used by `transcript_row_view.rs`.
- Add a `bind_duration_us` trace span per variant (matches the existing
  `update_to_layout_us` style from #146).
- Update `docs/PROJECT_STATUS.md` if the roadmap referenced the `Load more`
  button or pagination as current behavior.

---

## Risks and mitigations

1. **`bind()` cost on fast scroll.** Identified in the exploration § bonus
   hard point. Mitigation: measure first (commit 6 trace). If p99 > 16 ms,
   leverage `sourceview5::View` pooling, `Arc<str>` content cache in the
   data item, or fast-scroll placeholders. None blocks v1.

2. **Signal handler accumulation across recycle cycles.** Hardware of the
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
