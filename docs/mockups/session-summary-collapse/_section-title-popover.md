## Proposal E — The Summary Button (header-anchored disclosure)

> **Revised after review of the real header bar.** The detail page shares the **global** `adw::HeaderBar`, whose **center title slot is permanently occupied by the `AdwViewSwitcher` (Sessions | Analytics)**, and whose end side already carries the hamburger menu, the inspector toggle, **Resume** and the window close button. The earlier version of this proposal wrongly put an `AdwWindowTitle` menu in the center — that collides head-on with the view switcher. This revision drops that idea and anchors the disclosure to a **dedicated button in the header's *start* area**, where the screenshot shows ample empty space (between the search icon and the switcher).

### Core idea

Every other proposal treats the summary as a *block of content* that must be parked somewhere — folded in the flow (A, B), floated over the transcript (C), or moved to the side rail (D). This one treats it as a **header affordance**: the header bar already exists, it is global, and it already speaks in *buttons* (search, hamburger, inspector toggle). So we add **one more button** — a flat `GtkMenuButton` in the start cluster, reading `● sessions-chronicle ▾` (status-colored dot + project name + chevron) — and hang the **full** summary off it as a `GtkPopover`.

The vertical flow then holds only the transcript, full height, permanently. There is no banner, no bar, no `Revealer`, no overlay cartridge — the "reduced state" is **a single header button** plus the colored dot that encodes the most glanceable fact (ending status).

### Why the start area (and not the title)

The header, in the screenshot, is laid out:

```
[‹]  [📌]  [🔍]            ⟨ Sessions | Analytics ⟩            [☰] [�&#9707;] [ Resume ] [✕]
  start cluster                  center: view switcher                  end cluster
```

- The **center** is the `AdwViewSwitcher` — top-level navigation, shared by the whole window. **Untouchable.** This is exactly what the previous draft got wrong.
- The **end** cluster is already dense (hamburger, inspector toggle, Resume, close). Adding there crowds the primary actions.
- The **start** cluster has a large empty gap between `🔍` and the switcher. A flat menu button sits there naturally, next to the back arrow that already scopes "you are inside a session."

So the summary button lives in the start area, contextual to the detail page, and disappears when you leave it (it's added/removed with the detail page, like a `NavigationPage`-scoped header widget).

### Scroll behavior (my choice + rationale)

**None — and not even a side surface to maintain.**

The summary is never in the scrollable column and never floats over it; it is disclosed *on demand* from the header button and dismissed *on demand*. No `vadjustment` subscription, no hysteresis, no pin flag, no anti-yo-yo guard, no reflow of the virtualized `ListView`, and **no occlusion** — a `GtkPopover` is modal-light and auto-dismisses, so it never sits on content the user is reading the way C's expanded cartridge does. Open, read, click away, gone; the transcript was full-page the entire time.

The shared scroll machine of A/B/C is not just deleted (as in D) — there is no surface state to keep in sync at all. The popover's only state is "open / closed," owned by the `MenuButton`.

### Reduced state (what persists)

A single header button, always visible while you're in the session:

- **Status-colored dot** (Clean / Abrupt / **Error** / Unknown) as the button's leading icon — the highest-value glanceable fact, the one A/B fought hardest to preserve, here permanent and cheap. In the screenshot's session this dot is **red** ("Ended with error").
- **Project name** (`sessions-chronicle`) as the button label — the orientation anchor, ellipsized if the start gap is tight.
- **Chevron** signalling "there's more here."

Honest scope: this is **less** than the previous (wrong) draft claimed. Because the center title is taken, the header **cannot** also show the `Claude Code · 87 msg · 1h13m` subtitle inline — that condensed meta moves *into* the popover (or, if product insists on it always-visible, it can ride a second line of the button only on wide windows). So E's permanent footprint is realistically **dot + project name**, not a full title+subtitle. Still zero vertical pixels, but a more modest anchor than I first described.

### Re-show (affordance)

1. **Click the header button** — the primary affordance, in the start cluster, advertised by the chevron and the colored dot.
2. **`F9` / `win.toggle-summary`** — keyboard parity with the inspector toggle's register.
3. Opening the popover does **not** move transcript focus; closing returns focus to the button. No focus trap (`GtkPopover` manages and restores its own focus ring).

### Relm4 / GTK feasibility (concrete widgets, insertion)

Strictly local to the detail page, and **#160 is untouched** — we remove the `summary_box` sibling from the main column and re-host its content inside a popover anchored to a header button. The `ListView` stays the direct child of the `ScrolledWindow`.

Crucially, **E does *not* introduce a header** (the earlier draft's claim that it must wrap the page in a new `AdwToolbarView` was both wrong and unnecessary). The header already exists and is global. We only need to **add a start widget to it for the lifetime of the detail page** — the same mechanism by which the back arrow and search already appear when you drill in.

```
adw::HeaderBar  [EXISTING, global — shared with the view switcher]
├── start: [ back ‹ ] [ pin ] [ search ]
│          + gtk::MenuButton { "flat" }            // NEW, detail-scoped
│              ├── child = gtk::Box (horizontal)
│              │     ├── status_dot   (drawn circle, #[watch] color ← ending_status)
│              │     ├── gtk::Label { #[watch] label: project, ellipsize: End }
│              │     └── gtk::Image { pan-down-symbolic }
│              └── popover = gtk::Popover  [#name = summary_popover]
│                    └── gtk::Box (vertical)  // the MOVED summary_box content:
│                          path · session_id · meta chips · first_prompt · activity · tokens
├── center: adw::ViewSwitcher { Sessions | Analytics }   // UNTOUCHED
└── end:    [ ☰ ] [ inspector toggle ] [ Resume ] [ ✕ ]   // UNTOUCHED

content (detail page) = gtk::Box (vertical)
   └── transcript_scroller (ScrolledWindow → ListView)    // FULL HEIGHT
```

The `summary_box` subtree (lines 319-614) moves verbatim into the popover; the existing `update_*` functions keep targeting the same named widgets, only the parent changes. `project_label` + `ending_status` additionally feed the button label + dot (one extra `set_label` / dot-color set, the kind of trivial duplication A already accepts for its crest chips).

State: `summary_popover_open: bool` (driven by the `MenuButton`). No scroll state. The dot color reuses the existing `ending_status` field and the chip's color logic — no new data, no AI content.

### Trade-offs (honest)

- **Smaller permanent anchor than first claimed.** With the center slot owned by the view switcher, the header can carry only `dot + project` for the detail page; assistant + msg-count + duration live in the popover, not always-on. If product wants that meta permanently visible, **D** (side panel) or **B** (recap bar) are better homes — E deliberately trades ambient meta for a full-page transcript.
- **Start-area spacing.** The button shares the start cluster with back/pin/search; on a narrow window the project label ellipsizes or the button collapses to dot-only (`●▾`). Acceptable, but it is a finite budget to respect.
- **Popover height budget.** A `GtkPopover` is meant for compact content; the full summary (activity bar + tokens grid + first prompt) must fit a ~360-420 px-wide, height-bounded popover, scrolling internally if it overflows. Fine for *reference* data fetched deliberately, but the summary is then no longer all-visible-at-once the way the banner was. If every field must be simultaneously visible, **D**'s side rail is the better home.
- **The summary becomes "consult," not "ambient."** Beyond the dot + project, the rich detail is one click away. That's the point (give the conversation the screen), but it is a real behavior change from today's always-on banner — worth validating with users who currently scan tokens/activity at a glance.
- **Discoverability.** A header menu-button is a familiar pattern, and the dot + chevron advertise it; still, a returning user who expects the banner at the top must learn the summary now lives behind a header button. A one-time hint on first view could help.
- **Cleanest mechanics of the five** — no scroll logic, no reflow, no occlusion, and *no new header* — at the cost of the smallest live summary footprint (a single button). It is the maximal expression of "the conversation is the page; everything else is disclosure," now corrected to fit the header the app actually has.
