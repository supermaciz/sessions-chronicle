## Proposal C — The Retractable Cartridge (overlay "Peek")

### Core idea
Proposals A and B keep the summary **in the vertical flow**: a `Revealer` that animates its height and pushes the transcript, plus a persistent bar (~40-46 px). C's bet is the inverse: **lift the summary entirely out of the flow** and place it as a child of the already-present `gtk::Overlay` (`detail_overlay`), exactly like the floating search bar (`add_overlay`).

Direct consequence: the `transcript_scroller` occupies the **full** height, permanently. The summary is no longer a block that shares space, it's a **cartridge** that drops over the transcript on demand, then tucks away into a thin tab anchored to the top edge.  
Zero permanent space tax. Zero reflow of the virtualized `ListView` — an overlay does not re-lay-out its sibling, it covers it.

### Scroll behavior (my choice)
**Auto-present on open, tuck away on first scroll, never auto re-present.**

- When a session opens, the cartridge is **expanded** over the top of the transcript (translation `0`, full opacity), with a light gradient scrim underneath to detach it.
- On the **first** scroll down (crossing a ~32 px threshold on the scroller's `vadjustment`), the cartridge slides up off-screen and shrinks to a **tab** centered on the top edge. Animation via `gtk::Revealer` (`SlideUp`) *inside* the overlay, or CSS translation.
- It **never** re-presents on its own afterward. As in A, deliberate: no yo-yo, no `value-changed` loop. But here the argument is even stronger — since the overlay doesn't touch the scroller's height, there is **structurally no** reflow loop possible. The scroll that drives the overlay cannot, in return, change the scroll metric.

A single subscription: `transcript_scroller.vadjustment().connect_value_changed`, guarded by a `Cell<bool>` to cross the threshold only once.

### Reduced state (what persists)
In the tucked state, only a **tab** remains (~88 × 26 px) anchored to the center of the transcript's top edge:

- a **colored ending-status dot** (green Clean / orange Abrupt / red Error / gray Unknown) — the only glanceable info, reduced to its simplest expression;
- a **handle** + `pan-down-symbolic` chevron inviting a pull.

This is the most **minimal** reduced state of the three proposals: no 40 px bar, no chips. The bet: the "where am I" orientation is already given by the header bar (title = project); the tab only needs to signal *that a summary exists* and *how it ended*. Everything else is one gesture away.

### Re-show (affordance)
Four doors, all to the same `ToggleSummary` message, with no costly permanent surface:

1. **Click on the tab** — direct affordance, where the eye lands at the top of content.
2. **Hover-peek**: hovering the top edge *pre-slides* the cartridge a few pixels (teaser), confirming it's pullable before the click. Optional — a `GtkEventControllerMotion` on the top zone.
3. **Header `ToggleButton`** (icon `info-symbolic`/`view-list-symbolic`), pins the open state: while active, scroll won't tuck it away.
4. **`F9` shortcut** (same register as the inspector), announced in the tooltip.

When the cartridge is recalled, it **drops over** the transcript without pushing it: reading stays exactly where it was, simply hidden for the duration of the consultation. A click in the void / Esc / a new scroll tucks it away again.

### Relm4 / GTK feasibility (concrete widgets, insertion)
Insertion strictly local to `SessionDetail`, and — key point — **#160 is respected without the slightest concession**: the `ListView` stays the direct scrollable child of the `ScrolledWindow`, we move nothing in the flow. We only **add an `add_overlay`** to the existing `detail_overlay`.

```
gtk::Overlay  [detail_overlay]                  // already present
├── child = BreakpointBin → OverlaySplitView → content Box
│            └── transcript_scroller (ScrolledWindow → ListView)   // FULL HEIGHT
├── add_overlay = search-nav-bar                 // already present (top-center)
└── add_overlay = summary_peek  [NEW]            // top, halign Fill / valign Start
      └── gtk::Revealer { SlideDown }
            ├── reveal → cartridge (gtk::Box .summary-card: the current summary_box content)
            └── !reveal → tab (.summary-peek-tab: status dot + chevron)
```

The current `summary_box` (lines 319-614) is **moved as-is** into the overlay cartridge — no rewrite of `project_label`, `chip_row`, `first_prompt_section`, etc. The existing `update_*` functions keep targeting the same named widgets.

State added: `summary_open: bool` (init `true`), `summary_pinned: bool`, `summary_auto_dismissed: Cell<bool>`. Messages: `ToggleSummary`, `ScrollDismissSummary`. As with the inspector, we mirror state to the header bar via a symmetric `SummaryVisibilityChanged(bool)`.

**Collision to manage** (honestly): the `search-nav-bar` is also a top-center overlay. Expanded cartridge + active search overlap. Simple mitigation: anchor the cartridge `valign: Start` full-width and offset the `search-nav-bar` below it when `summary_open && search.query.is_some()` (or hide the tab during a search).

### Trade-offs (honest)
- **Accepted occlusion.** Expanded cartridge = it covers the top ~3-5 transcript rows. That's the price of a full-page transcript: we trade "permanently stolen space" (A/B) for "temporarily borrowed space." Acceptable because the cartridge is transient and invoked, not resident. The scrim clearly signals the overlap.
- **Tab discoverability.** A 26 px tab is more discreet than a 40 px bar: a user might not realize a summary exists. The (always-visible) header `ToggleButton` and hover-peek are there to compensate; to validate in testing.
- **Very info-poor reduced state** (just the ending status). If "project/assistant" orientation is deemed indispensable at all times, C is the wrong proposal — A and B keep it. C bets the header bar suffices.
- **Managing the search-bar collision**: extra state logic, to test. That's the cost of reusing the same Overlay.
- **The strong bet**: it's the only proposal that makes the transcript *truly* full-page and eliminates *structurally* all reflow. If it works, it's the smoothest; if the occasional occlusion bothers, it's the riskiest of the three.
