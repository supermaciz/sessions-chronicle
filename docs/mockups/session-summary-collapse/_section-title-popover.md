## Proposal E — The Title Popover (header-anchored disclosure)

### Core idea

Every other proposal treats the summary as a *block of content* that must be parked somewhere — folded in the flow (A, B), floated over the transcript (C), or moved to the side rail (D). This one treats it as **identity**, and asks where identity belongs in a GNOME window. The answer is canonical: the **header bar title**.

Make the detail page's `adw::HeaderBar` carry an `AdwWindowTitle` — `title = project name`, `subtitle = Claude Code · 128 msg · 42 min` — with a **colored ending-status dot** beside the title and a small dropdown chevron. The title becomes a `GtkMenuButton`. Clicking it drops a `GtkPopover` holding the **full** summary: path, ID, first prompt, activity, the tokens grid. The popover is transient; click-away or `Esc` dismisses it.

The vertical flow holds only the transcript, full height, permanently. There is no banner, no bar, no Revealer, no overlay cartridge — the "reduced state" is literally **the header bar that the window already has**.

### Scroll behavior (my choice + rationale)

**None — and unlike D, not even a side surface to maintain.**

The summary is never in the scrollable column and never floats over it; it is disclosed *on demand* from the title and dismissed *on demand*. There is no `vadjustment` subscription, no hysteresis, no pin flag, no anti-yo-yo guard, no reflow of the virtualized `ListView`, and — crucially — **no occlusion**: a popover is modal-light and auto-dismisses, so it never sits on top of content the user is trying to read the way C's expanded cartridge does. You open it, you read, you click away, it's gone, the transcript was full-page the entire time.

The shared scroll machine of A/B/C is not just deleted (as in D) — there is no surface state to keep in sync at all. The popover's only state is "open / closed," owned by the `MenuButton`.

### Reduced state (what persists)

The header bar, always visible, is the reduced state:

- **Colored status dot** (Clean / Abrupt / Error / Unknown) immediately left of the title — the highest-value glanceable fact, and the one A/B fought hardest to preserve, here permanent and free.
- **Project name** as the window title — the orientation anchor.
- **Subtitle** `Claude Code · 128 msg · 42 min` — assistant + scale, condensed, `dim`.
- **Dropdown chevron** signalling "there's more here."

This is richer than C's bare tab (it keeps project + assistant + meta + status) and costs the conversation **zero** vertical pixels, because the header bar is space the window already spends. It's the best permanent-info-to-tax ratio of all five proposals.

### Re-show (affordance)

1. **Click the title** — the GNOME-native "title menu" gesture (the eye is already at the top-center; the chevron advertises it). This is the primary affordance and it's where users *expect* document/session metadata to live.
2. **Dedicated `MenuButton`** — the title itself *is* the button (`AdwWindowTitle` inside a flat `GtkMenuButton`), so no separate icon is strictly needed; an optional `info-symbolic` button is the explicit fallback.
3. **`F9`** — a `win.toggle-summary` action that pops the popover, consistent with the inspector's keyboard register.

Opening the popover does **not** move transcript focus; closing returns focus to the title button. No focus trap (a `GtkPopover` manages its own focus ring and restores on close).

### Relm4 / GTK feasibility (concrete widgets, insertion)

Strictly local to `SessionDetail`, and **#160 is untouched** — we remove the `summary_box` sibling from the main column and re-host its content inside a popover anchored to the header. The `ListView` stays the direct child of the `ScrolledWindow`.

The detail page is today an `adw::NavigationPage` whose root is a `gtk::Box` **without an internal header** (`src/app/init.rs:165`). This proposal *introduces an `adw::ToolbarView` + `adw::HeaderBar`* — but note A and B must introduce a local toolbar **anyway** just to host their toggle button. Here the header isn't overhead to bolt a control onto; the header **is** the design. It earns its place.

```
adw::ToolbarView                         [NEW root — but A/B need this too]
├── top = adw::HeaderBar
│     └── title-widget = gtk::MenuButton { "flat", "title-menu" }
│           ├── child = gtk::Box (horizontal)
│           │     ├── status_dot  (Image / drawn circle, #[watch] color)
│           │     ├── adw::WindowTitle { #[watch] title: project,
│           │     │                      #[watch] subtitle: "Claude Code · 128 msg · 42 min" }
│           │     └── gtk::Image { pan-down-symbolic }
│           └── popover = gtk::Popover  [#name = summary_popover]
│                 └── gtk::Box (vertical)  // the MOVED summary_box content:
│                       path · session_id · first_prompt · activity · tokens
└── content = gtk::Box (vertical)
      └── transcript_scroller (ScrolledWindow → ListView)   // FULL HEIGHT
```

The `summary_box` subtree (lines 319-614) moves verbatim into the popover; `update_*` keeps targeting the same named widgets, only the parent changes. The `project_label`/status feed the `WindowTitle` + dot instead of standalone labels (one extra `set_title`/`set_subtitle`, the kind of trivial duplication A already accepts for its crest chips).

State: `summary_popover_open: bool` (driven by the `MenuButton`). No scroll state. The status dot color reuses the existing `ending_status` field and the chip's color logic — no new data, no AI content.

### Trade-offs (honest)

- **Popover height budget.** A `GtkPopover` is meant for compact content; the full summary (activity bar + tokens grid + 3-line first prompt) must fit a ~360-420 px-wide, height-bounded popover. If it overflows, the popover scrolls internally — acceptable for *reference* data you fetch deliberately, but it does mean the summary is no longer all-visible-at-once the way the banner was. If product insists every field be simultaneously visible without an internal scroll, D's side rail is the better home.
- **The summary becomes "consult," not "ambient."** Beyond the subtitle line, the rich detail is one click away rather than in your face. That's the point (give the conversation the screen), but it's a real behavior change from today's always-on banner — to validate with users who currently scan tokens/activity at a glance.
- **Introduces a header on the detail page.** A structural change to the page root (`ToolbarView` wrap), with the usual care to avoid a double header bar against the app-level chrome. Same risk B flags — but here the header is load-bearing, not incidental.
- **Discoverability of the title-menu gesture.** Clickable titles are an established but not universally-known pattern; the chevron and the optional `info-symbolic` fallback exist to advertise it. A one-time hint on first view could help.
- **No occlusion, no reflow, no scroll logic — the cleanest mechanics of the five**, at the cost of the smallest live summary footprint (a subtitle). It's the maximal expression of "the conversation is the page; everything else is disclosure."
