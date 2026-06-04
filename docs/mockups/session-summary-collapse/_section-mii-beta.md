## Proposal A — The Collapsible Crest

### Core idea

The `summary_box` doesn't have a styling problem, it has a **conviction** problem. Today it shows seven sections permanently because nobody dared decide which ones actually matter. Fix #160 lifted it out of the scroller, and so what used to be a passing banner became a fixed 300-400 px wall that crushes the conversation — the primary task of this view.

My proposal: turn `summary_box` into a **two-zone** widget, exactly like an `AdwExpanderRow` but hand-built to keep the existing layout:

- a **crest** (~46 px) *always visible*, answering the only question that matters while reading: "where am I?";
- a **body** wrapped in a `GtkRevealer`, holding all the detail (path, ID, first prompt, activity, tokens) and collapsing away.

No new surface. No window. We multiply nothing: we take the widget tree that already exists and cut it in two at the right joint.

### Scroll behavior (my choice + mechanical rationale)

**Automatic collapse on scroll down, with hysteresis, and WITHOUT automatic re-expand.**

When the user scrolls down past a threshold (~48 px), the body `Revealer` sets `reveal_child = false`: slides up, 200 ms, `SlideUp`. The crest stays. Once collapsed, **it never re-expands on its own**: scrolling back to the top does not reopen the body.

Mechanical rationale, because this is where most "sticky that reacts to scroll" designs sabotage themselves:

- **No yo-yo, no reflow loop.** A summary that collapses *and* expands by scroll direction creates a classic trap: collapsing changes the viewport height, which can re-fire a `value-changed`, which re-fires a toggle… On a virtualized `ListView` with markdown rows, each reflow re-measures lines. By making the collapse **one-directional and explicit on the way back up**, we remove the loop by construction.
- **A single subscription.** We listen to a single signal: `vadjustment().connect_value_changed`. No redundant `GtkEventControllerScroll`, no polling timer.
- **Hysteresis**: the down threshold (48 px) ≠ the guard threshold. We only flip state on a clean crossing, never on trackpad micro-jitter. An `auto_collapsed` `Cell<bool>` prevents re-emitting the Relm4 message on every pixel.
- **Collapsing destroys nothing.** `reveal_child = false` hides and animates a height; the child widgets stay alive and bound to their fields. Zero tree rebuild, so zero re-render cost on re-expand. That's exactly why it stays fluid in motion, not just pretty in a screenshot.

It "feels" light because mechanically it is: a height animation on an already-built subtree, triggered once per threshold crossing.

### Reduced state (what persists and why)

The crest keeps ONLY what answers "where am I, and did it end well":

- **Project name** (`project_label`, shrunk to `title-4`) — the anchor.
- **Assistant chip** (icon + name) — Claude Code / Codex / OpenCode / Mistral Vibe: never confuse two sessions from two assistants.
- **Message count chip** — the scale of the conversation.
- **Ending status chip** (`ending_status_chip`, already colored Clean/Abrupt/Error/Unknown) — the highest-value piece of info in the whole box and the one you lose by scrolling. It stays.

What collapses: `path_label`, `session_id_row`, `first_prompt_section`, `activity_section`, `tokens_section`. These are **reference** data ("I want the numbers"), not **orientation** data ("where am I"). You don't consult them while reading; you go fetch them. So they're allowed to disappear behind a gesture.

Naming note, because it's my job to flinch: the crest chips must tell the truth. "128 msg" must count the same thing the transcript actually contains, and the ending status already comes from an indexed field (`session.ending_status`) — no AI-generated data, no flattering rounding. Good. Keep it.

### Re-show (affordance)

Three doors to the same action, all honest, no extra surface:

1. **Click on the crest / chevron** — the crest is a flat `GtkButton` (`add_css_class: "flat"`); the `pan-down`/`pan-end-symbolic` chevron indicates state. The obvious mouse affordance, placed *where the eye already is* (top of content).
2. **Header `ToggleButton`** — exactly the existing `inspector_toggle` pattern (`app/mod.rs`, `pack_start`), bound to `model.summary_open`, icon `view-list-symbolic` or `info-symbolic`. The keyboard-consistent, always-reachable path, even when the crest is off-screen.
3. **Shortcut** — a `win.toggle-summary` action (keyboard-first), in the same register as `toggle-inspector` (F9). Fully drivable from the keyboard.

Explicit re-expand sets `reveal_child = true` AND re-arms `auto_collapsed = false`, so the "auto-collapse on scroll" cycle starts over cleanly.

### Relm4 / GTK feasibility (concrete widgets, insertion into the current hierarchy)

Everything happens in `src/ui/session_detail.rs`, inside the `set_content = &gtk::Box` (lines 315-630). We touch neither the `OverlaySplitView`, the `detail_overlay`, the `ToastOverlay`, nor the floating search bar.

`summary_box` refactor:

```
summary_box: gtk::Box (vertical)              // unchanged as container
├── summary_crest: gtk::Button { "flat" }     // NEW: clickable crest
│   └── gtk::Box (horizontal)
│       ├── project_label  (re-parented, title-4)
│       ├── assistant / msg / ending_status chips (re-parented from chip_row)
│       └── gtk::Image { pan-down-symbolic }   // chevron, #[watch] on summary_open
├── gtk::Separator
└── summary_revealer: gtk::Revealer { SlideUp, 200ms, #[watch] set_reveal_child: model.summary_open }
    └── gtk::Box (vertical)                     // the old content: path, id,
        └── (first_prompt / activity / tokens, separators)  // first prompt, activity, tokens
```

State added to the model:

- `summary_open: bool` (default `true`),
- `summary_auto_collapsed: Cell<bool>`.

Messages: `SessionDetailMsg::ToggleSummary` (crest click / header toggle / action) and `SessionDetailMsg::SummaryAutoCollapse`. The scroll hook: in `init`/`post_view`, `transcript_scroller.vadjustment().connect_value_changed` which, on threshold crossing and if `!summary_auto_collapsed`, sends `SummaryAutoCollapse` then arms the `Cell`. Since `inspector_open` is already mirrored to `app/mod.rs` via `SessionDetailOutput::InspectorVisibilityChanged`, we add a symmetric `SummaryVisibilityChanged(bool)` to drive the active state of the header `ToggleButton` — the pattern already exists, we copy it.

No main-thread work: no I/O, no parsing. Just a property toggle and a GTK animation.

### Trade-offs (honest)

- **Re-parenting named widgets** (`project_label`, the chips) between the crest and the body takes rigor: either move them once at build time, or — cleaner — **duplicate** the 3 crest chips as separate widgets fed by the same fields. Duplicating 3 labels costs three extra `set_label` in `update_*`; that's the price of not playing re-parenting ping-pong. I recommend duplication: more readable, and the crest can diverge (smaller chips) without breaking the body.
- **One-directional collapse = a deliberate choice.** Someone will want it to re-expand when scrolling back to the top. I refuse: that's precisely what brings back the yo-yo and the reflow. The explicit re-show (3 affordances) is the right compromise. If we cave later, it'll need a strict anti-loop guard (only re-expand if `vadjustment.value == 0` AND stayed at 0 for a tick).
- **The crest adds ~46 permanent px** vs a full hide. Deliberate: a full hide that leaves no anchor is the invisible-state indicator I fight against — the user acts on a state whose context they've lost. 46 px that always say "project X, Claude Code, ended cleanly" beat 0 px of amnesia.
- **`GtkRevealer` animates height**, so the content below it (the transcript) re-lays-out during the 200 ms. On a virtualized `ListView` that's bounded and short; but it's the only window where things "move." Acceptable, and it's the reason for the one-directional collapse: we pay that animation once per read, not on every scroll oscillation.
