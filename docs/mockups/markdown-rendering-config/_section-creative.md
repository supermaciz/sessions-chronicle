# Proposal — Creative: Reading ⟷ Source mode (transcript-wide twin)

**Author**: creative proposal — reframe the problem, don't add two knobs  
**Lens**: borrow the editor mental model (Obsidian *Reading/Source*, GitHub *Preview/Edit*)  

## The reframe

The two axes the brief names — *scope* and *enable/disable* — are both downstream of one observation: **the only time markdown rendering actively hurts is when you want to grab text.** The widget tree (`GtkLabel` per block, `GtkSourceView` for code, `GtkGrid` for tables) is great to read and impossible to select across, because each block is its own selection island.

So instead of a permanent "disable markdown" switch that degrades the reading experience forever, model it the way editors already do: a **mode**. The transcript has a *Reading mode* (today's rendered tree) and a *Source mode* (every message is one plain, monospace, fully-selectable block of its raw text). A single toggle flips the whole transcript between them. "Disable markdown" stops being a crippling preference and becomes "show me the source so I can select it" — a momentary, reversible stance, exactly like hitting *Source* in an editor.

This also dissolves the scope axis as a top-level concern: in **Source mode every message is raw** (user and assistant alike, uniformly), so "should user messages render markdown?" only ever matters in Reading mode — where it becomes a quiet sub-setting, not a co-equal toggle.

## What the user sees

- A flat `GtkToggleButton` labelled `</>` lives in the header's **start gap** (between search and the center `Sessions | Analytics` switcher — the slot the session-summary exploration already identified as free). Tooltip: *"Show raw markdown source"*. `Ctrl+Shift+S` mirrors it.
- **Reading mode** (untoggled, default): exactly today's rendering. Markdown for assistant messages; user messages plain — governed by the scope sub-setting below.
- **Source mode** (toggled): each message row is rebuilt as a single selectable monospace block of its *verbatim* `content` string. One drag selects an entire message; `Ctrl+C` yields canonical markdown you can paste into an editor, an issue, anywhere. Search highlight keeps working because it is now one buffer per message, not N.
- The toggle state is **per-window and remembered** in GSettings, so someone who lives in Source mode never re-flips it, and everyone else never discovers they could.

## Settings

```xml
<key name="transcript-view-mode" type="s">
  <default>"reading"</default>
  <summary>Transcript rendering mode</summary>
  <description>reading = rendered markdown; source = verbatim selectable text.</description>
</key>
<key name="markdown-scope" type="s">
  <default>"assistant"</default>
  <summary>Which roles render markdown in Reading mode</summary>
  <description>assistant = assistant only; all = user and assistant.</description>
</key>
```

Preferences gains an **Appearance** group with two rows:  
- `adw::ComboRow` *Default transcript mode* → Reading / Source (seeds `transcript-view-mode`).  
- `adw::ComboRow` *Render markdown for* → Assistant only / All messages (`markdown-scope`); subtitle notes it only applies in Reading mode.

## Integration

`render_content` (`src/ui/session_detail/transcript/row_rendering.rs:30`) becomes mode-aware:

```rust
match mode {
    ViewMode::Source => append_plain_source(container, content, highlight_query),
    ViewMode::Reading => {
        let render_md = role == Role::Assistant
            || (scope == Scope::All && role == Role::User);
        if render_md { markdown::render_markdown(content, highlight_query) }
        else         { append_plain_source(container, content, highlight_query) }
    }
}
```

`append_plain_source` is the plain-label path that already exists in the `else` branches today — reused, not invented. Flipping the toggle emits a `SessionDetailMsg::SetViewMode`, which re-runs `render_content` over the visible rows (the transcript is a Relm4 `ListView`, so this is a rebind of materialized rows, not a full reload).

## Strengths

- **Selection is a first-class mode, not a degradation.** You never trade away readable rendering permanently to get copy-paste; you flip, grab, flip back.
- **Familiar model.** Reading/Source is a pattern users already know from Obsidian, GitHub, Bear, Typora — discoverable by analogy.
- **Copy fidelity.** Source mode yields the *exact* markdown, which is usually what you want when pasting elsewhere — better than copying rendered text and losing the `**`/fences.
- **Subsumes the scope axis.** One uniform "everything raw" state means scope only matters in one mode, so it can be a calm sub-setting rather than a competing toggle.
- One button, one slot, zero new surfaces.

## Costs

- **Two render paths to keep alive.** Source mode is cheap (one label) but it is a second code path through `render_content` that must stay correct as the renderer evolves.
- **Toggle re-renders visible rows.** Acceptable (ListView only materializes the viewport), but it is a visible reflow on flip.
- **Header slot contention.** The `</>` button competes for the same start-gap slot the summary-button proposal (exploration E) wants. If both ship, they share that cluster.
- **Mode is transcript-wide, not per-message.** If a user wants raw for *one* message while reading the rest rendered, this doesn't do it (a per-message context-menu "Copy as markdown" would — noted as a possible companion, not part of this proposal).
