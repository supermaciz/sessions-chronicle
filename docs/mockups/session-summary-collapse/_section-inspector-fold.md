## Proposal D — Fold the summary into the existing inspector (surface consolidation)

### Core idea

A, B and C all answer the same way: *keep the summary near the transcript and invent a mechanism to make it yield* — a `Revealer` height animation (A, B) or an overlay cartridge (C), all driven by the scroller's `vadjustment`, all carrying hysteresis, a pin flag and an anti-yo-yo guard. They differ in geometry; they share a machine.

This proposal questions the premise. The view **already owns a contextual side surface**: the inspector, an `adw::OverlaySplitView` (position `End`, collapses at 860 sp, toggled by `inspector_toggle` + `F9`). And the `summary_box` is, conceptually, *the same class of object as the inspector*: both are **metadata about the session**, not part of the conversation. We have two competing "things-about-this-session" surfaces fighting for the screen. The cheapest, most honest fix is not a fourth mechanism — it's to **stop having two surfaces**.

My proposal: **move the `summary_box` content into the inspector panel** as its top section (or behind a small `AdwViewSwitcher`: *Summary | Inspector*). The vertical flow then holds **only the transcript**, full height. No `Revealer`, no `vadjustment` subscription, no hysteresis, no reflow — the entire scroll machine that A/B/C share is deleted by construction, because the summary is no longer in the column the user scrolls.

### Scroll behavior (my choice + rationale)

**None. Deliberately none.**

The summary lives in the side rail, perpendicular to the scroll axis. Scrolling the transcript cannot touch it, and it cannot touch the scroll metric. There is therefore nothing to debounce, no threshold to cross, no `Cell<bool>` to arm, no flicker on short transcripts, no "what happens when you scroll back to the top" question to litigate. The single hardest part of A/B/C — getting the scroll-driven state machine to feel right and not loop — simply does not exist here.

This is the whole bet: the smoothest scroll behavior is the absence of one.

### Reduced state (what persists)

In the **main column**: nothing from the summary. The transcript is full-height. Optionally, the app header's `AdwWindowTitle` carries `title = project` + `subtitle = ending status`, so identity survives even with the panel closed — but that's a one-line bonus, not a bar.

In the **inspector panel** (when open): the *full* summary, every section, at full fidelity — because a side rail has the vertical room the top banner never did. Path, ID, first prompt, activity and the tokens grid finally get to breathe in a column sized for them, instead of being crushed into a 300 px horizontal banner.

So the reduced state isn't a *shrunk* summary; it's a *relocated* one. Nothing is lost — it moves to a place where it costs the conversation zero permanent pixels and reads better when consulted.

### Re-show (affordance)

Here is the elegance: **the affordances already exist and need no new wiring.**

1. **The inspector `ToggleButton`** in the header — already present (`inspector_toggle`), already bound, already discoverable. It now reveals the summary too.
2. **`F9`** — already mapped to `toggle-inspector`. Keyboard-first re-show, for free.
3. **Default-open on first view** (optional): when a session opens, the inspector starts revealed so the summary greets the user, then a click / `F9` / `Esc` tucks it away. This preserves today's "summary is the first thing you see" behavior without taxing every subsequent scroll.

If we keep a *Summary | Inspector* `AdwViewSwitcher` at the top of the panel, the toggle opens to whichever page was last viewed; the switcher arbitrates which metadata you want.

### Relm4 / GTK feasibility (concrete widgets, insertion)

Strictly local to `SessionDetail` (`src/ui/session_detail.rs`), and **#160 is untouched** — we never move the `ListView`; we *remove* a sibling from the main `gtk::Box` and re-host it in the panel that already exists.

```
adw::OverlaySplitView  [already present]
├── content  = gtk::Box (vertical)
│     └── transcript_scroller (ScrolledWindow → ListView)   // FULL HEIGHT, only child now
└── sidebar  = adw::ToolbarView   [already the inspector host]
      └── content = gtk::Box (vertical)
            ├── adw::ViewSwitcher  { Summary | Inspector }   // NEW, optional
            └── adw::ViewStack
                  ├── page "summary"  → summary_box  (MOVED here, unchanged)
                  └── page "inspector" → (current inspector content)
```

The move is mechanical: the `summary_box` subtree (lines 319-614) — `project_label`, `chip_row`, `first_prompt_section`, `activity_section`, `tokens_section` — is re-hosted verbatim as a `ViewStack` page. The existing `update_*` functions keep targeting the same named widgets; only the parent changes. The chip `FlowBox` and tokens grid actually *prefer* the narrower panel (they were built to wrap).

State: we reuse `inspector_open` (rename to `panel_open` if desired). One new field if we add the switcher: `panel_page: SummaryOrInspector`. No new scroll state, no `Cell`, no hysteresis.

No header gymnastics either: the inspector toggle already lives in the header bar via the established `inspector_toggle` pattern — the very thing A and B have to *introduce*. Here it is already there.

### Trade-offs (honest)

- **Summary and inspector now share one panel.** If a user genuinely wants the summary *and* the inspector visible at the same instant, the `ViewSwitcher` makes them pick. Mitigation: if both are short, stack them in one scrollable panel instead of a switcher. But the honest position is that they're the same *kind* of thing and rarely needed simultaneously — that's the premise of the proposal.
- **Below 860 sp the panel overlays the transcript** (it's an `OverlaySplitView`), so on a narrow window the open summary occludes — same accepted occlusion as C, but on the side and only when the user opted in. On a wide window it splits cleanly, no occlusion.
- **Horizontal real estate, not vertical.** An open panel narrows the transcript instead of shortening it. For a reading column that's usually the better trade (line length stays comfortable), but it is a different tax to be aware of, paid only while open.
- **The conceptual bet.** This only works if you accept that "session summary" and "session inspector" are one family. If product wants them to read as distinct features, D is the wrong proposal — A/B/C keep them separate. I'd argue the opposite is the feature: collapsing two metadata surfaces into one is a *simplification* the screen has been missing since #160 split them apart.
- **Discoverability of the moved summary.** A returning user who expects the banner at the top must learn it now lives behind the inspector toggle. The optional default-open-on-first-view and the header subtitle both soften this; a one-time hint could too.
