# Proposal — Mii Beta

**Verdict in one line:** kill the scope axis, and demote the "disable" axis from a global setting to a per-message "Select raw text" toggle. The selection problem is a rendering side effect, not a preference, so it gets fixed where it bites — on the row you're trying to copy.

## Axis 1 — Scope (user AND assistant): cut it

Let's be honest about what a user message *is* in these transcripts. It's a prompt, a pasted stack trace, a pasted diff, a chunk of code, a file path. It is overwhelmingly plain prose or pre-formatted text. Running `render_markdown()` over it does one of two things, both bad:

1. **Nothing visible** — the text has no markdown, so we paid for a `pulldown_cmark` parse and built one `GtkLabel` instead of one `GtkLabel`. Pure tax, zero gain.
2. **Actively wrong** — a pasted shell snippet with `*glob*` or `#comment` or `1. step` gets *reinterpreted* as emphasis, a heading, an ordered list. Now the UI is lying about what the user typed. That's the cardinal sin: the renderer is editorializing the input.

And the cost isn't theoretical. Every user row that today is a single `GtkLabel` becomes a *fragmented widget tree* — `GtkSourceView` in a `ScrolledWindow` for fences, `GtkGrid` for tables, N labels for blocks. In a ListView that recycles rows during fast scroll, you've traded one cheap widget for a tree you rebuild on every bind. The app feels heavier in motion to render markdown nobody asked for, on content that's usually code.

So: **scope is indecision dressed as a feature.** Assistant-only is not a limitation we're working around — it's correct. Assistant output is *authored* markdown; user input is *captured* text. Render the authored thing, show the captured thing verbatim. Don't add a knob to blur that line.

## Axis 2 — Disable markdown: real problem, wrong shape

The motivation is legitimate and it's the strongest thing in this whole exploration: **the multi-widget tree makes whole-message selection impossible.** Each `GtkLabel`, each `GtkSourceView`, each `GtkGrid` is its own selection island. You can drag inside paragraph A, but the drag dies at A's border. Ctrl+A selects one block. There is no "select this whole answer and copy it." For an app whose entire job is *reading and reusing* AI sessions, that's not a missing feature — it's a daily papercut.

But a **global on/off setting in Preferences is the wrong instrument**, for three reasons:

- **It's bimodal for a momentary need.** You don't want raw text *forever*. You want it for *this answer, right now*, so you can grab the code and paste it. A global toggle makes you go to Preferences, flip it, lose your scroll position (the whole ListView re-renders), copy, then go flip it back. That's a settings safari for a two-second task.
- **A global toggle re-renders everything.** Mechanically, flipping a GSettings key has to invalidate every visible row's `render_content()` and re-run it. On a long transcript that's a parse + tree-build storm for content you weren't even looking at.
- **It names the mechanism, not the intent.** "Disable markdown rendering" describes what the code does. The user's actual intent is "let me select and copy this." Name the affordance after the intent: **Select raw text.**

### What it should be: a per-row toggle

A flat `GtkToggleButton` labelled **"Select raw text"**, revealed on row hover/focus, sitting in the assistant row header next to the model label — *where the eye already is when you're about to copy.* Toggling it:

- flips that **one row's** branch in `render_content()` from the markdown path to a raw path, and
- re-runs `render_content()` on **that container only** — `while let Some(child) = container.first_child()` already clears it; we append a single `set_selectable(true)` monospace `GtkLabel` with `set_text(raw)`.

No ListView rebuild. No GSettings round-trip. No parse cost on the other 800 rows. The widget tree for that row collapses from "4 selection islands" to "1 continuous selection" — `Ctrl+A`, drag, copy, all of it works. Untoggle to get the rendered view back. The state lives in the row's Relm4 model field (e.g. `raw: bool`), not in persistent config, because the need is ephemeral.

### Mechanical change to `render_content`

Today line 42 is `if role == Role::Assistant`. It becomes:

```
if role == Role::Assistant && !raw { render_markdown(...) }
else { single selectable GtkLabel, monospace when raw }
```

The raw branch is *already written* — it's the existing `else` plain-label path (lines 57–65), minus the highlight markup. You're wiring an existing branch to a new trigger, not building a renderer.

## GSettings

**None.** This is the load-bearing recommendation. The moment you reach for a schema key you've mis-modeled the need as a preference. It's a transient view mode on a single row. Adding `markdown-render-scope` (string) and `markdown-render-enabled` (bool) would ship two knobs that lie: one says "I change a global behavior" when the behavior is per-message, the other says "scope is configurable" when scope should just be correct.

If — and only if — telemetry someday shows people living in raw mode permanently, *then* add one bool default (`prefer-raw-text`) that sets each new row's initial `raw` field. Until then, don't.

## Strengths

- One affordance, honestly named after intent ("Select raw text"), not mechanism.
- Fixes the actual papercut (whole-message copy) exactly where it hurts, with the content in view.
- Cheapest possible flip: one container re-render, reuses an existing code branch.
- Deletes the scope axis entirely, so user rows stay one cheap label and the renderer stops editorializing pasted code.
- Zero new persistent state, zero schema migration.

## Costs

- New per-row UI: a `GtkRevealer`-wrapped flat `GtkToggleButton` in the assistant row header, plus a `raw: bool` field and a toggle message in the row component.
- `render_content` gains one bool parameter; callers pass the row's `raw` state.
- Highlight interaction: when a search highlight is active and the user flips to raw, you lose in-block highlight markup (raw is plain `set_text`). Acceptable — they toggled raw precisely to escape the rendered structure — but the match count for that row should be suppressed/recomputed so the search counter doesn't lie.
- Accessibility: the toggle needs a clear accessible label ("Show raw text for selection") and `Escape`/focus behavior unchanged; the raw `GtkLabel` is fully selectable so screen-reader and keyboard copy improve, not regress. Verify under large text and high contrast that the monospace raw block still wraps (`WrapMode::WordChar`).

## Verification

- `--sessions-dir tests/fixtures`: open an assistant message with a fenced code block + a table + paragraphs. Confirm drag-select dies at block borders (the bug). Toggle "Select raw text," confirm `Ctrl+A` grabs the whole message and copy yields the raw markdown source.
- Toggle on the longest transcript fixture and confirm scroll position and the other rows don't flicker (proves it's a single-container re-render, not a ListView rebuild).
- Narrow width: the revealer button must not crowd the model label — collapse it into the row's context menu (right-click "Select raw text") as the mouse-honest fallback; the keyboard path stays the toggle on focus.
