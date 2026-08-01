## Proposal — UI Designer (HIG-conformant, minimal change)

**Stance:** Almost none of this should be configurable in-app.  
GNOME Settings ▸ Search already owns on/off and ordering. The app should add **exactly one** setting — the one thing the OS cannot express: **how much transcript text is allowed to appear on screen in the Activities overview.**

### Summary

| | |
|---|---|
| New GSettings keys | **1** — `search-provider-detail` |
| New preference rows | **1** — an `AdwComboRow` |
| New preferences page | none (reuse `General`) |
| New header-bar / sidebar controls | none |
| Restart or re-index required | none |
| Complexity | **Small** |

Everything else that could plausibly be a setting — enable/disable, provider position, per-assistant scope, date window, result count — is either already owned by GNOME Settings, owned by Shell, or fails the "would a user ever change this, and would they understand the consequence?" test. Each rejection is defended below.

---

### 1. Reject the in-app "Enable GNOME search" switch

The Flatpak exports `share/gnome-shell/search-providers/*.ini` with `DefaultDisabled=true`. GNOME Settings ▸ Search therefore already shows Sessions Chronicle in its provider list with a mandatory per-app switch and drag-to-reorder, backed by `org.gnome.desktop.search-providers`. That switch is authoritative: Shell will not call our provider when it is off, regardless of what any app-level key says.

An in-app switch would create a two-source truth table with one genuinely broken quadrant:

| GNOME Settings ▸ Search | In-app switch | Result |
|---|---|---|
| On | On | works |
| Off | Off | no results |
| Off | **On** | **no results — app appears broken, and the app's own UI says it should be working** |
| On | Off | no results — user must remember which of two switches they touched |

Row 3 is the disqualifier. HIG's "be considerate" principle is specifically about not creating states where the interface contradicts observed behaviour. A duplicated toggle guarantees that state.

**What the app owes the user instead is discoverability, not a second switch.** libadwaita's canonical affordance for that is a `PreferencesGroup` *description* — explanatory body text under the group title, no interactive surface, no state to desynchronise:

```text
PreferencesGroup  title="System Search"
                  description="Sessions can be found from the Activities overview.
                               Turn this on in Settings ▸ Search."
```

I deliberately do **not** propose a link row that launches `gnome-control-center`. There is no stable URI scheme for a Settings panel, launching another app's desktop file from inside the sandbox needs a portal round-trip that can silently fail, and a link row that does nothing is worse than a sentence that is always true. Plain text is the honest control here.

---

### 2. The one setting that is justified: result detail

This is where Sessions Chronicle differs from Files, Contacts, or Calculator, and it is the whole reason "la recherche dans GNOME doit être configurable" is a real requirement rather than a preference.

`GetResultMetas` returns `name` and `description`. Both are rendered in the Activities overview — a surface that is on screen during screen-shares, projector sessions, and over a shoulder. The corpus behind them is full AI-assistant transcript bodies: pasted API keys, client names, absolute paths, stack traces, `.env` dumps. A `description` filled with the matched snippet will, sooner or later, put a live credential on a projector.

Note also that `name` and `description` become the **accessible names** for the result — Orca reads them aloud. "It only flashes briefly" is not true for screen-reader users.

So the axis worth exposing is not *whether* search works but *what it is permitted to render*.

#### The row

In `src/ui/modals/preferences.rs`, add a `System Search` group to the existing `General` page, between `Session Resumption` (line 70) and `Advanced` (line 92):

```text
PreferencesGroup  title="System Search"
                  description="Sessions can be found from the Activities overview.
                               Turn this on in Settings ▸ Search."
└── ComboRow      title="Result Details"
                  subtitle="What system search results are allowed to show"
                  model = ["Session and project", "Include matching text"]
                  selected ← bound to "search-provider-detail"
```

#### What each value renders

**`"session"` — "Session and project" (default)**

```text
[icon]  Fix the OAuth token refresh loop in the auth middl…
        Claude Code · sessions-chronicle · 2 days ago
```

`name` = the session's `first_prompt`, truncated to 60 characters on our side.  
`description` = `{AI assistant} · {project name} · {relative time}`.

The rule this default encodes, and the one worth stating in the schema description: **the overview may show what the user themselves typed as intent; it may never show assistant output or pasted material.** `first_prompt` is user-authored and is already the identifying string in the in-app session row, so results stay distinguishable. Matched transcript bodies — where credentials and dumps actually live — never reach the screen.

The project *name* is used, not the absolute path. `/home/alice/clients/acme-bank/…` renders as `acme-bank`.

**`"match"` — "Include matching text"**

```text
[icon]  Fix the OAuth token refresh loop in the auth middl…
        …export ANTHROPIC_API_KEY=sk-ant-api03-4f9c2e… in .env, then…
```

`description` = the matched snippet from the top-ranked matching message, whitespace-collapsed, truncated to ~100 characters. Strictly better for finding things; strictly worse for anything anyone else can see. Opt-in, never default.

#### Wording rationale

- **"Result Details"** rather than "Privacy" or "Snippet mode" — GNOME row titles name the thing being configured, not the anxiety motivating it. The subtitle carries the consequence.
- **"What system search results are allowed to show"** — "allowed to" is the load-bearing phrase; it tells the user this is a ceiling, not a formatting preference.
- **"Session and project" / "Include matching text"** — describes output, not mechanism. No jargon (`description`, `metas`, `bm25`) leaks into user-facing strings.
- **"System Search"**, not "GNOME Search" — matches how the Settings panel names itself, and stays accurate if the same provider is ever consumed elsewhere.

#### Why a `ComboRow` and not a `SwitchRow`

A switch would have to be titled something like "Show matching text", making the safe state the *off* state. GNOME switches read as "turn on the feature"; framing the privacy-preserving default as "off" invites users to flip it without reading. A `ComboRow` presents two named outcomes with the safe one selected, and leaves room for a third value later without a UI redesign.

I considered and rejected a third value, `"minimal"` (`name` = "Claude Code session", no prompt text at all). At that point results are mutually indistinguishable and the feature no longer does its job; a user who wants zero transcript text in the overview wants the provider *off*, and that control already exists in Settings ▸ Search. Adding a near-off value in-app re-imports the duplicate-toggle problem from §1 through the side door.

---

### 3. Rejected settings, with reasons

**Provider position / ordering.** Owned by `org.gnome.desktop.search-providers` via drag in Settings ▸ Search. Nothing to add.

**Number of results.** Shell decides how many rows fit and truncates. Not ours to expose. The provider should return a bounded set (see §5) — that is an implementation bound, not a preference.

**Coupling to the in-app filter state (assistant / project / date).** Firmly reject. The provider process can run while the GUI is not running, so there is no live UI state to read. Even persisted, "GNOME search returns nothing because I left a project filter on last Tuesday" is a silent failure the user cannot diagnose from the overview. System search must behave the same on every invocation.

**Per-assistant scope** (`as` list of AI assistants to expose). Rejected: which assistant produced a transcript is not a privacy boundary, and the in-app assistant filter already covers the browsing use case.

**Recency window** (`search-scope-days`). Rejected, though it is the most tempting one. Date is not a privacy axis — a credential pasted eight months ago is exactly as sensitive as one pasted yesterday — so it does not serve the actual concern. And a default other than "all" produces silent empty results for older material with no on-screen explanation. Query cost is better bounded by `LIMIT` + bm25 than by a user-facing knob.

**Per-project exclusion** (`as` key `search-excluded-projects`). The one rejection I hold open. "This client repo must never surface in the overview" is a genuine boundary that Settings ▸ Search cannot express, because its toggle is all-or-nothing for the app. But it should **not** be a Preferences list of checkboxes — that is a settings-screen re-implementation of something the user already has in front of them. If it ships, it belongs as a context-menu item on the project sidebar row ("Exclude from system search"), direct manipulation on the object it applies to, with the excluded set persisted in an `as` key. **Explicitly out of scope for v1:** it is only meaningful once the provider has been adopted, and v1's job is to be safe by default rather than exhaustively tunable.

---

### 4. GSettings key

Add to `data/dev.maciz.sessionschronicle.gschema.xml.in`, after `sort-order` (line 25):

```xml
<key name="search-provider-detail" type="s">
  <default>"session"</default>
  <summary>Detail level for system search results</summary>
  <description>Controls how much transcript content the GNOME Shell search provider may
  place in the Activities overview. Accepted values: session (session prompt, AI assistant,
  project name and relative time only), match (additionally shows the matched transcript
  snippet). The default deliberately excludes assistant output and pasted content, which
  may contain credentials or client data.</description>
</key>
```

Type `s` with named values, consistent with the existing `resume-terminal` and `sort-order` keys. Unknown values must fall back to `"session"` — fail closed, never fail open.

---

### 5. How the provider process reads the setting

**The provider may not share a process with the GUI.** Shell D-Bus-activates it, so it can start cold while the window is closed, and it can outlive several open/close cycles of the GUI. The design must survive both shapes:

- **In-process**, if the provider is hosted by the main app via `DBusActivatable=true` + `--own-name` in the manifest.
- **Out-of-process**, if it is a separate entry point (for example `sessions-chronicle --gnome-search-provider`).

`gio::Settings::new(APP_ID)` works identically in both cases — the GSettings backend is per-user, not per-process. Inside Flatpak with the current `finish-args` (no `--talk-name=ca.desrt.dconf`), GIO uses the keyfile backend at `~/.var/app/dev.maciz.sessionschronicle/config/glib-2.0/settings/keyfile`. Every instance of the same app id — GUI and D-Bus-activated provider alike — maps the same file, and `GKeyfileSettingsBackend` watches it with a `GFileMonitor`, so cross-process change notification works without adding dconf to the sandbox. **No manifest change is required for the setting itself.**

**Read the key inside `GetResultMetas`, not once at provider startup.** `gio::Settings` caches internally and refreshes on backend change notification, so a per-call `settings.string("search-provider-detail")` is a cheap in-memory read, not a file hit — and it means a long-lived provider process cannot serve stale detail levels. Do not cache the value in a struct field; there is no invalidation path worth writing when the API already provides one.

**On change:** nothing to invalidate, no re-index, no restart, no D-Bus signal. Shell caches result metas only for the duration of one search interaction, so the next keystroke after the user closes Preferences already reflects the new value.

**`GetInitialResultSet` / `GetSubsearchResultSet` do not read the key at all.** They return ids only; the detail level affects rendering, never matching. Search quality is identical in both modes — the setting costs recall nothing, which is what makes the safe default defensible.

---

### 6. Integration points

| File | Line | Change |
|---|---|---|
| `data/dev.maciz.sessionschronicle.gschema.xml.in` | after 25 | add `search-provider-detail` |
| `src/ui/modals/preferences.rs` | 67–92 | new `System Search` group with description + `ComboRow`, added between `resumption_group` and `advanced_group` |
| `src/database/mod.rs` | 452 `search_sessions_with_query` | reuse as-is for matching; provider path needs a `LIMIT`-bounded variant and, for `"match"`, a snippet column (`snippet(messages_fts, …)`) |
| `data/dev.maciz.sessionschronicle.desktop.in.in` | — | `DBusActivatable=true` (provider work, not config work) |
| `build-aux/dev.maciz.sessionschronicle.json` | `finish-args` | `--own-name=…` (provider work, not config work) |

The `ComboRow` follows the existing `resume-terminal` pattern in `preferences.rs:74-86` exactly: a `gio::ListStore<StringObject>`, `selected` computed from the current string value, `connect_selected_notify` writing back. No new abstraction.

**No new CSS.** No new style classes. `AdwPreferencesGroup` description and `AdwComboRow` are stock.

---

### 7. Accessibility

- **Keyboard path:** the row joins the existing `General` page tab order after the `Terminal` combo and before `Database Location`. <kbd>Tab</kbd> to reach, <kbd>Space</kbd>/<kbd>Enter</kbd> to open the popover, <kbd>↑</kbd>/<kbd>↓</kbd> to choose, <kbd>Enter</kbd> to commit, <kbd>Esc</kbd> to dismiss without changing. All stock `AdwComboRow` behaviour — nothing to implement.
- **Focus order:** Terminal → **Result Details** → Database Location → Copy → Reset. Placing `System Search` before `Advanced` keeps the destructive Reset last, which is correct.
- **Escape:** dismisses the popover first, then the `AdwPreferencesDialog`. Unchanged.
- **Screen reader:** the `AdwPreferencesGroup` description is announced with the group, so the "Turn this on in Settings ▸ Search" instruction reaches Orca users without an extra label. The row's title and subtitle are sufficient accessible naming — no explicit `set_accessible_label` needed.
- **The result strings are themselves an a11y surface.** `name` and `description` are read aloud by Orca in the overview. Truncate on our side rather than relying on Shell's visual ellipsis: a 4 KB stack trace that is visually clipped is still 4 KB of speech. Cap `name` at 60 and `description` at 100 characters, and collapse newlines and runs of whitespace before returning.
- **Large text:** a two-line `ComboRow` with a short value string ("Session and project") reflows without truncation at 200% text scale. "Include matching text" is the longer of the two and still fits; both were chosen short for this reason.
- **High contrast:** stock widgets only, no custom colours, nothing to verify beyond the default theme.
- **Reduced motion:** not applicable — no animation introduced.

---

### 8. Adaptive behaviour

- **Wide:** `AdwPreferencesDialog` centres the page at its natural width; the group sits below `Session Resumption` with no layout change.
- **Narrow:** `AdwComboRow` collapses the value under the title as usual. The group description wraps to two or three lines; it is plain body text with no fixed width. The Activities overview rendering is Shell's responsibility and is unaffected by our layout.

---

### 9. Verification

Run with fixtures: `--sessions-dir tests/fixtures`.

1. **Default is safe on a fresh profile.** `gsettings reset dev.maciz.sessionschronicle search-provider-detail`, search for a term that matches deep in a fixture transcript, confirm no transcript body appears in any result — only prompt, assistant, project, time.
2. **Cross-process change takes effect.** Start the provider (or trigger it from the overview), change the value in Preferences while it is running, search again, confirm the new detail level applies with no restart. This is the assertion that the per-call read exists.
3. **Provider cold-start with the GUI closed** reads the persisted value, not the schema default.
4. **Unknown value falls back closed:** `gsettings set … search-provider-detail 'bogus'` must render as `"session"`, not crash and not render matches.
5. **Truncation edge cases:** a fixture whose first prompt is a single 4 KB pasted block; a fixture whose matched message is a multi-line stack trace. Neither may produce a multi-line or oversized overview row in either mode.
6. **A session with an empty `first_prompt`** must still produce an identifiable `name` — fall back to the project name plus assistant rather than an empty string.
7. **GNOME Settings ▸ Search off** — confirm the app is listed and disabled by default on first install, and that nothing in Preferences implies search is active while it is off.

---

**Complexity: Small.** One schema key, one `ComboRow` built from an existing pattern, one group description, zero CSS, zero new components, zero manifest changes for the configuration itself. The cost in this feature lives entirely in the search provider; the configuration surface is deliberately not where the work is.
