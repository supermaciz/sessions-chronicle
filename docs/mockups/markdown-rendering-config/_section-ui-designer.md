## Proposal — UI Designer (HIG-conformant, minimal change)

**Stance:** Put both controls in the existing Preferences dialog. No header-bar toggle, no per-session affordance. This is the conservative GNOME answer, and I argue below why it is also the *correct* one for this specific workflow.

### Summary

Two persistent settings, both on the existing `General` page in a new `Transcript` group:

1. **Render Markdown** — `adw::SwitchRow`, default On. Off = raw selectable text mode.  
2. **Apply To** — `adw::ComboRow` with values `Assistant only` (default) and `Everyone`. Insensitive while Render Markdown is Off.

Changing either setting re-renders all open transcript rows reactively via a GSettings `changed` subscription. No deviation from standard libadwaita preference patterns is required.

### Why Preferences-only, and why I reject a header toggle

The motivating use case is **text selection / copy**. It is tempting to read "easy toggle" as "put a button in the header bar." I reject that for concrete HIG reasons:

- **The session detail page uses the GLOBAL shared `adw::HeaderBar`** (center `AdwViewSwitcher`, end cluster hamburger + inspector + Resume + close). A markdown toggle there is a *mode control scoped to one content type* living on a *global, cross-workspace* bar. It would also be visible (and meaningless) in Analytics unless conditionally hidden — exactly the kind of context-dependent header mutation HIG warns against. The "empty gap in the start cluster" is not an invitation; an empty start cluster is a perfectly normal GNOME layout.
- **Rendering mode is a stable preference, not a per-session decision.** Users who want selectable raw text want it as their default reading mode, not toggled per message. A persisted setting matches the actual frequency of the decision. Per-view toggles are for things you flip many times per session (e.g. wrap on/off in an editor); this is not that.
- **There is already a faster path to the underlying goal for one-offs:** copying a single block. So the header real estate would buy little. If a per-session escape hatch is ever justified, it belongs as a primary menu item in the existing hamburger (`AdwHeaderBar` end), bound to the same GSettings key via a `Gio.Action` — *not* a new visible button. I explicitly scope that out of v1.

This keeps the interaction model coherent: one place to configure how transcripts read, consistent with how `Terminal` and the other knobs already work.

### Exact widgets and wording

In `src/ui/modals/preferences.rs`, add a third group to the existing `General` page (after `Session Resumption`, before or after `Advanced`):

```text
PreferencesGroup  title="Transcript"
├── SwitchRow   title="Render Markdown"
│                subtitle="Off shows raw text you can select and copy as one block"
│                active ← bound to "render-markdown"
└── ComboRow    title="Apply To"
                 subtitle="Which roles get rich formatting"
                 model = ["Assistant only", "Everyone"]
                 selected ← bound to "markdown-scope"
                 sensitive ← (render-markdown == true)
```

Wording rationale:  
- "Render Markdown" not "Enable markdown" — describes the action, sentence-case-on-each-word per GNOME row-title convention.  
- The subtitle names the *benefit* (whole-block copy), which is the real motivation, rather than the implementation ("single GtkLabel").  
- "Apply To" + "Assistant only" / "Everyone" avoids the loaded word "user" colliding with role names; "Everyone" reads naturally for "user and assistant". I deliberately avoid a `SwitchRow` for scope because scope is a 2-value-today-but-conceptually-enumerable choice (tool results could become a third option), and a `ComboRow` extends without a UI redesign.

### GSettings keys

Add to `data/dev.maciz.sessionschronicle.gschema.xml.in`:

```xml
<key name="render-markdown" type="b">
  <default>true</default>
  <summary>Render Markdown in transcript messages</summary>
  <description>When false, message content is shown as raw selectable text in a single label, allowing whole-message selection and copy.</description>
</key>
<key name="markdown-scope" type="s">
  <default>"assistant"</default>
  <summary>Which roles render Markdown</summary>
  <description>Accepted values: assistant (assistant messages only), all (user and assistant messages).</description>
</key>
```

I use a string enum (`"assistant"` / `"all"`) rather than a bool for scope, matching the established `resume-terminal` string-enum pattern in this schema and leaving room for a future `"all-including-tool-results"` value.

### Integration at `render_content`

`src/ui/session_detail/transcript/row_rendering.rs:30` `render_content(container, content, role, highlight_query)` is the single choke point. Today it hardcodes `if role == Role::Assistant`. Replace the role gate with a settings-derived decision:

```text
let render_md = settings.boolean("render-markdown")
    && (settings.string("markdown-scope") == "all" || role == Role::Assistant);

if render_md { /* existing render_markdown branch */ }
else { /* existing plain selectable GtkLabel branch — already exists for the else case */ }
```

The plain-text branches already build a fully selectable `GtkLabel` (with optional highlight markup), so the "Off" / "not in scope" path reuses existing code verbatim — no new rendering code, only a changed condition. Read the two keys via `gio::Settings::new(APP_ID)` once at the top of `render_content` (cheap; GSettings caches). To avoid constructing a `Settings` on every row, prefer threading the two resolved booleans in from the transcript component that already owns a `Settings` handle, or memoize a `thread_local!` `Settings`.

### Reactivity

Changing a preference must re-render open transcripts, or the setting feels broken.

- The transcript list component subscribes to `settings.connect_changed(Some("render-markdown"), ...)` and `connect_changed(Some("markdown-scope"), ...)`.  
- On change it sends an existing/new `SessionDetailMsg` (e.g. `RerenderTranscript`) that walks visible rows and calls `render_content` again with current content + `highlight_query`. `render_content` already clears the container (`while let Some(child) = container.first_child()`), so re-rendering is idempotent.  
- Highlight state and match counts are recomputed by the same call, so search highlighting survives a mode switch.

### Accessibility checklist

- **Keyboard:** both rows are standard libadwaita rows, fully focusable and operable (Space toggles the switch, Enter/arrow opens and selects in the combo). No custom focus handling needed.  
- **Focus order:** rows follow DOM order within the group; the group sits in normal page tab order after `Session Resumption`.  
- **Insensitive state:** when Render Markdown is Off, `Apply To` is set `sensitive=false` — libadwaita dims it and removes it from tab order automatically, and screen readers announce it disabled. This prevents the confusing state of choosing a scope that does nothing.  
- **Screen reader:** row titles/subtitles provide accessible names automatically; no manual `accessible_label` needed. The switch announces its on/off state.  
- **Raw text mode and SR:** the "Off" single `GtkLabel` is *better* for screen readers and selection than the fragmented widget tree — one contiguous accessible text node.  
- **High contrast / large text:** all named/semantic styling, standard rows; no custom CSS, so HC and large-text scale natively.  
- **Reduced motion:** the switch/combo use system-standard transitions; nothing added.

### Adaptive behaviour

- The Preferences dialog (`adw::PreferencesDialog`) is already adaptive; rows reflow and the dialog goes bottom-sheet on narrow widths with no extra work.  
- No header-bar control means **no narrow-width crowding** of the shared header — a real benefit of this approach over a toggle button, since that cluster is already busy (hamburger + inspector + Resume + close).  
- The transcript itself is unaffected structurally; "Off" mode actually improves narrow-width readability since a single wrapping label has no nested grids/tables forcing horizontal space.

### Strengths

- Zero new rendering code; reuses the existing plain-`GtkLabel` branch and the existing Preferences component pattern.  
- No mutation of the shared global header bar; interaction model stays coherent across Sessions/Analytics.  
- Scope as a string enum is future-proof (tool results as a third option).  
- "Off" mode directly solves the selection-island problem and is simultaneously the more accessible rendering.  
- Fully keyboard- and SR-accessible with no custom a11y work.

### Costs

- **Files to touch:** `data/dev.maciz.sessionschronicle.gschema.xml.in` (2 keys), `src/ui/modals/preferences.rs` (one group, two rows, two `connect_*` handlers + sensitivity binding), `src/ui/session_detail/transcript/row_rendering.rs` (change the gate), and the owning transcript/session-detail component (settings subscription + a re-render message). Possibly a `thread_local!` Settings helper.  
- **New CSS:** none.  
- **Reused style classes:** none custom needed; standard `adw::SwitchRow` / `adw::ComboRow` styling.  
- **Estimated complexity:** **Small.** The only non-trivial piece is wiring reactive re-render of open rows; everything else is boilerplate matching the existing `resume-terminal` combo.

### Verification

- Run with fixtures: `flatpak-builder --run ... sessions-chronicle --sessions-dir tests/fixtures`.  
- Toggle **Render Markdown** off with a transcript open → message collapses to a single label; drag-select spans the entire message including code-fence lines; copy yields raw markdown source.  
- Toggle back on → markdown tree returns; search highlight (open a search first) still highlights after the toggle.  
- Set **Apply To = Everyone** → user messages now render markdown (verify with a fixture user message containing a list/code block, e.g. Claude Code and Codex fixtures).  
- Edge cases: empty message content (no crash, empty label); message with only a code block (Off mode shows fenced text selectably); `Apply To` correctly dimmed and ignored while Off; very long transcript re-renders without jank on toggle (confirms re-render is bounded to visible/loaded rows).
