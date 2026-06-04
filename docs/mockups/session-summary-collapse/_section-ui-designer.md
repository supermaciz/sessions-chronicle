## Proposal B — Retractable summary driven by scroll, with a persistent recap bar

### Core idea
Keep the `summary_box` exactly where fix #160 placed it (sibling above the `transcript_scroller`, so the `ListView` stays the direct scrollable child of the `ScrolledWindow`), but wrap it in a `gtk::Revealer`.  
At rest / at the top of the transcript: full summary expanded.  
As soon as the user scrolls down: the `Revealer` collapses onto a **persistent ~40 px recap bar** (project + condensed meta + ending status + "Details ▾" affordance), giving most of the space back to the conversation.  
The summary is never fully removed: there is always a visible anchor point to reopen it.

### Chosen libadwaita pattern (and rejected alternatives)
Chosen pattern: **`gtk::Revealer` driven by the `ScrolledWindow`'s `vadjustment`**, paired with a **header `gtk::ToggleButton`** as an explicit affordance (pin/unpin).  
This is the canonical GNOME pattern for "contextual surface that yields to content while scrolling" (cf. the revealed/hidden-on-scroll header bar behavior), without breaking the structural constraint of #160.

Alternatives evaluated and rejected:

- **`adw::ExpanderRow` / collapsible section** — rejected. Designed for a row in a `boxed-list`/`PreferencesGroup`, not for a rich multi-section header (`title-2` title, FlowBox of pills, activity bar, tokens grid). We'd deform the widget for an unintended use, and `ExpanderRow`'s internal chevron doesn't lend itself to automatic scroll driving.  
- **`adw::Banner`** — rejected. Semantically reserved for transient status/action messages (one line + button). It cannot carry the summary's structured content.  
- **Summary in the `Start` sidebar of an `AdwOverlaySplitView`** — rejected. The view already uses an `OverlaySplitView` (inspector, position `End`). Stacking a second nested split for the summary heavily burdens the hierarchy, complicates responsiveness (two panels collapsing at different breakpoints), and moves vertical meta to a side rail ill-suited to a tokens grid. Over-engineering.  
- **Header toggle + `Revealer`, without scroll collapse** — this is the backbone of this proposal, but alone it leaves the user all the manual burden. Adding `vadjustment` driving directly addresses the raised "auto-collapse on scroll" idea, without taking away any manual control.

Accepted HIG deviation: **automatic** collapse on scroll is a state-driven behavior, rarer than a plain toggle. Rationale: the measured friction is real (the `summary_box` permanently crushes the conversation since #160); the "header hides on scroll" pattern is itself an established GNOME idiom; and the manual affordance (toggle + bar) stays primary, so the automation removes no control.

### Scroll behavior
Driven by the `transcript_scroller`'s `vadjustment` (already accessible in the component):

- `value() <= high_threshold` (e.g. 8 px) → `Revealer.set_reveal_child(true)`: full summary.  
- scroll down past a threshold (e.g. 48 px) → `set_reveal_child(false)`: collapse onto the recap bar.  
- transition: `RevealerTransitionType::SlideUp`, ~200 ms, **disabled if `gtk-enable-animations` is false** (reduced motion).  
- Hysteresis (distinct high/low thresholds) to avoid flicker around the flip point.  
- The manual toggle **pins** the state: if the user forces it open, auto-collapse is suspended until the next return to the top (or a new click). A `summary_pinned: bool` field arbitrates.

### Reduced state (what persists)
A ~40 px recap bar, always present, containing:

- assistant badge + **project name** (`lbl`, ellipsized);  
- condensed meta: `· 128 msg · 42 min ·` (`small dim-label`);  
- **colored ending status chip** (the most "glanceable" info);  
- on the right, the "Details ▾" open affordance (`flat`, accent).

This keeps the session's identity and outcome legible at all times, even with the transcript scrolled.

### Re-show (affordance + a11y + shortcut)
Three redundant paths, all equivalent:

1. **`gtk::ToggleButton` in the detail page header bar** — icon `view-list-symbolic` (or `info-symbolic`), `tooltip` "Show session summary", `active` state reflecting `reveal`. The canonical, discoverable HIG affordance.  
2. **Click on the recap bar** ("Details ▾" zone) — `GtkGestureClick` on the bar, same message.  
3. **Scroll back to the top of the transcript** — automatic re-expand.

Re-show accessibility:

- keyboard shortcut **`F9`** (GNOME "toggle side/contextual panel" convention), registered via a `gtk::ShortcutController` on the component, announced in the tooltip;  
- the `ToggleButton` is in the header bar's focus order; explicit `accessible-label`;  
- the recap bar exposes `role=button` + `accessible-label` "Show session summary" when collapsed;  
- on open, focus doesn't jump (the `Revealer` doesn't grab focus), to avoid disturbing transcript reading.

### Relm4 / GTK feasibility (concrete widgets, insertion)
Insertion **strictly local to the `SessionDetail` component** (`src/ui/session_detail.rs`) — no moving the `ListView` out of the `ScrolledWindow`, so #160 is preserved.

In the existing `set_content = &gtk::Box` vertical, we encapsulate the current `summary_box`:

```
gtk::Box (content, vertical)            // unchanged
├── gtk::Revealer  [#name = summary_revealer]
│     set_transition_type: SlideUp
│     #[watch] set_reveal_child: model.summary_revealed
│     └── gtk::Box [#name = summary_box]   // current content, unchanged
├── gtk::Revealer  [#name = recap_revealer]   // recap bar
│     #[watch] set_reveal_child: !model.summary_revealed
│     └── gtk::Box .summary-recap-bar (horizontal)
│           badge · project_recap_label · meta_recap_label · ending_status_chip(logical clone) · "Details ▾"
└── gtk::ScrolledWindow [#name = transcript_scroller]   // unchanged
      └── ListView                                       // direct child, unchanged
```

Model state added to `struct SessionDetail`:

- `summary_revealed: bool` (init `true`)  
- `summary_pinned: bool` (init `false`)

Messages added to `SessionDetailMsg`:

- `ToggleSummary` (manual toggle → pin/unpin)  
- `ScrollPositionChanged(f64)` (emitted by the `vadjustment` handler)

Scroll wiring: in `init`, after `view_output!()`, fetch `widgets.transcript_scroller.vadjustment()` and connect `connect_value_changed` → `sender.input(SessionDetailMsg::ScrollPositionChanged(adj.value()))`. The handler applies hysteresis and updates `summary_revealed` (unless `summary_pinned`).

Header toggle: the detail page is today an `adw::NavigationPage` whose child is the `gtk::Box` root of `SessionDetail`, **without an internal `AdwToolbarView`/`AdwHeaderBar`** (`src/app/init.rs:165`). Two options:

- **(recommended, most local)** introduce an `adw::ToolbarView` as the component root with an `adw::HeaderBar` carrying the `ToggleButton` — everything stays in `SessionDetail`, zero cross-component wiring. Cost: wrap the current root.  
- (alternative) put the toggle in the app-level header bar and route via a `SessionDetailMsg` — more coupling between `app/` and the component; avoid.

Reduced-motion animations: read `gtk::Settings::gtk_enable_animations()` and force `RevealerTransitionType::None` if disabled.

### HIG compliance / accessibility
- Progressive disclosure: the rich detail yields to the primary content (the transcript) — "Make it simple" principle.  
- Reduced effort: zero clicks in the nominal case (auto-collapse), explicit control available (toggle + shortcut).  
- Context preservation: the recap bar avoids disorientation ("am I still in the right session?").  
- Keyboard: `F9` + standard header focus; no focus loss on open.  
- Screen reader: `accessible-label` on toggle and collapsed bar; the `Revealer` introduces no focus trap.  
- Large text: the recap bar uses `ellipsize`/`dim-label`, height dictated by content (the 40 px is indicative, not a rigid `height-request`).  
- High contrast: reuses existing `card`, `pill`, `dim-label`, status chip → automatically follows the theme; no hardcoded color.  
- Reduced motion: transition disabled if animations off.

### Trade-offs
Pros:

- Respects #160 without compromise (ListView stays direct child).  
- Surgical change: we wrap the existing `summary_box`, we don't rewrite it.  
- Exactly answers the "auto-collapse on scroll" idea while keeping a discoverable manual control.  
- 100% composed of standard libadwaita/GTK widgets and already-present CSS classes.

Cons / risks:

- The `vadjustment` + hysteresis + pinning driving is **state logic** to test carefully (flicker, short transcripts).  
- On a transcript too short to scroll, auto-collapse never triggers: the manual toggle is then the only lever (acceptable).  
- Introducing an internal `AdwToolbarView`/`HeaderBar` slightly changes the component root (to verify against the app-level header bar to avoid a double bar).  
- Small duplication overhead: the recap bar must reflect project/meta/status already computed for the `summary_box` (reuse the same values, no recompute).

**Estimated complexity: medium** (trivial encapsulation + scroll/pinning logic to test).
