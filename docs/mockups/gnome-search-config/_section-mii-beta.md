# Proposal — Mii Beta

> **Correction note (2026-08-02).** This is the proposal as written on 2026-08-01. Its *argument* stands unchanged, but several codebase facts it cites were invalidated when [PR #200](https://github.com/supermaciz/sessions-chronicle/pull/200) shipped the deep-link prerequisite (#197). Corrected points are marked inline below; the authoritative current facts live in [`../../explorations/2026-08-01-gnome-search-configurability-exploration.md`](../../explorations/2026-08-01-gnome-search-configurability-exploration.md). The reasoning is preserved rather than rewritten so the record of what was argued, and on what basis, stays intact.

**Verdict in one line:** there is exactly **one** honest knob here, and it is not "on/off". The on/off switch already exists, it belongs to GNOME Settings ▸ Search, and shipping a second one in our Preferences is how you build a setting whose two states are indistinguishable. What we own is not *whether* the shell searches us — it is *how much of the transcript corpus leaves the app and lands on screen in the Activities overlay*. One `AdwComboRow`, one GSettings key, three values, each pair distinguishable by something the user can actually see.

## The test every proposed knob has to pass

A setting is real only if **its two states differ in an outcome the user can observe and would deliberately choose between.** Everything below is judged against that, and most of the obvious candidates fail it.

## Knob 1 — "Enable GNOME search" (bool): **delete it**

This is the one that looks most obviously necessary and is most obviously wrong.

Flatpak exports `share/gnome-shell/search-providers/*.ini` marked disabled by default. That is not a gap we're filling — it means **every user already has a mandatory, OS-owned, per-app toggle** in GNOME Settings ▸ Search, backed by `org.gnome.desktop.search-providers`, complete with drag-to-reorder against Files, Calculator and Characters. That toggle is authoritative in a way ours can never be: flipping it off removes us from the provider list, so the Shell never opens the D-Bus connection at all.

Now ask what our switch could mechanically do. It cannot un-register the provider — that list lives in the host's dconf, outside our sandbox. The best it can do is make `GetInitialResultSet` return an empty array. So:

| Our switch | GNOME's switch | What the user sees in Activities |
|---|---|---|
| On | On | Results |
| On | Off | **Nothing** |
| Off | On | **Nothing** |
| Off | Off | **Nothing** |

Three of four states render identically. The user flips our switch to On, sees nothing, and files a bug — or worse, concludes the feature is broken and stops trusting the app. **That is the definition of a setting that lies.** And the "Off/On" case burns a D-Bus round trip and a process spawn per keystroke to answer "no", which is the most expensive way to do nothing I can think of.

**What ships instead:** a non-interactive `AdwActionRow` that *points at* the real switch.

```
GNOME Search
Turn results on or off in Settings ▸ Search
```

No suffix widget. `activatable(false)`. It is a signpost, not a control. And no, we cannot make it a live status indicator either: the manifest (`build-aux/dev.maciz.sessionschronicle.json:11`) has no dconf hole, so `org.gnome.desktop.search-providers` is not readable from inside the sandbox. A row that guessed at that state would be the second lie stacked on the first. Don't guess — just say where the switch is.

## Knob 2 — Per-assistant / per-project scoping in shell search: **delete it**

Tempting, because those filter axes already exist in-app (`tool`, project, date). They fail the test differently: **the state is invisible at the point of use.** In the session list, an active assistant filter is visible as sidebar toggles right next to the results; you can see why Codex sessions are missing. In the Activities overview there is no sidebar, no chip, no affordance — just a result list that silently omits half your corpus because of a checkbox you set four months ago in a dialog. "Why doesn't my session show up in GNOME search" becomes an unanswerable support question.

**One corpus, one truth: if it's in the index, the shell can find it.** That is a rule the user can hold in their head. Any per-axis filtering here is indecision wearing a `SwitchRow`.

Same reasoning kills **date-range scoping** ("only search the last 90 days"). Nobody wants a search that quietly can't find things.

## Knob 3 — Minimum query length, result limit: **delete them, they're constants**

These are real *values* but they are not *preferences*. They're a correctness-and-cost floor, and there is no user for whom "match on 1 character" is the right answer.

- **Minimum 3 characters**, hard-coded. Below that, return `[]` immediately without touching SQLite. A 2-char FTS prefix query against full transcript bodies matches a substantial fraction of the corpus; you'd be sorting tens of thousands of rows by bm25 to display five.
- **`LIMIT 20`** on the ID query, hard-coded. The Shell renders ~5 in the collapsed grid and asks for metas only for what it draws.

Exposing either as a knob asks the user to tune a query planner. That's not configuration, that's outsourcing our homework.

## Knob 4 — What `ActivateResult` does (open vs. resume): **delete it**

`ActivateResult` opens the session in the app. Full stop. "Resume in terminal" spawns a process and attaches to a live AI assistant session; putting that one Enter keypress away from a fuzzy-matched result in the Activities overview, with no confirmation and no visible target, is a way to resume the wrong session by muscle memory. This isn't configurable, it's just correct.

## Knob 5 — Transcript exposure: **this one is real. Ship it.**

Here is what the corpus actually is: **full AI conversation transcript bodies.** Pasted API keys. Client names. Absolute paths with usernames in them. Stack traces naming internal services. Now recall that `GetResultMetas` returns a `description` string that the Shell renders as the result subtitle — in the Activities overview, which is the single most screen-shared, most shoulder-surfed surface in the entire desktop. You hit Super, type three letters, and the overlay paints a line of your last session's transcript across a projector.

An excerpt is also *the whole point of full-text search*. Without it, you can't tell two sessions in the same project apart. Both positions are correct, they genuinely conflict, and there is no default that's right for everyone. **That is what a preference is for.**

But it's one axis, not two. Do not ship a "match bodies" bool and a "show excerpts" bool — that's a 2×2 matrix with two nonsense cells, solving one problem with two surfaces. Collapse it into a single ordered scale of increasing exposure:

### `AdwComboRow` — "Activities search"

Subtitle: *What GNOME Shell searches and shows for this app*

| Option string | Matches against | `description` field shows |
|---|---|---|
| Session titles only | `sessions.first_prompt`, `project_path`, assistant name (`LIKE`, no FTS) | `Claude Code · sessions-chronicle · 3 days ago` |
| **Full transcripts** *(default)* | `messages_fts MATCH` | `Claude Code · sessions-chronicle · 3 days ago` |
| Full transcripts with excerpt | `messages_fts MATCH` | `…rotated the AWS_SECRET_ACCESS_KEY after…` |

Each step up is observably different: query `AKIA` finds nothing under *titles only* and finds the session under *full transcripts*; the third option changes what's printed on screen without changing what matches. Three values, three distinct outcomes, one monotonic axis. Nothing here is decorative.

**Default is the middle value.** Full search power, zero transcript text rendered outside our window. The leak is opt-in, and the option string says exactly what it does — "with excerpt" — rather than something metaphorical like "detailed results". Rendering transcript body text in the shell overlay should be a thing you chose, not a thing you discover during a demo.

## GSettings

**Exactly one new key.**

```xml
<key name="search-provider-exposure" type="s">
  <default>"transcripts"</default>
  <summary>How much session content GNOME Shell search can reach</summary>
  <description>
    Controls what the GNOME Shell search provider matches against and what it
    displays as a result description. Accepted values: titles (match session
    titles and project paths only), transcripts (match full transcript text,
    show session metadata), transcripts-excerpt (match full transcript text,
    show the matching excerpt as the result description).
  </description>
</key>
```

Type `s`, not an enum type, matching the existing convention set by `resume-terminal` and `sort-order` in `data/dev.maciz.sessionschronicle.gschema.xml.in`. Unknown values fall back to `"transcripts"`.

Key name notes: it says *exposure*, because that is the axis — how far session content travels. It does not say "enabled", because it does not control that. It does not say "mode", because "mode" tells you nothing.

## How the provider process reads it

**Which process?** Not the app. If the main binary owns the bus name, the first keystroke of every login session spawns a full GTK 4 + libadwaita + Relm4 init — window-less, ~60 MB, hundreds of milliseconds — to answer a three-letter query. That's a fog machine with a D-Bus interface.

Ship a **separate binary, `sessions-chronicle-search-provider`**: no GTK, `zbus` (or bare `gio::DBusConnection`) + `rusqlite` opening the same DB read-only, WAL mode, idle-exit after ~30 s. Single-digit MB RSS, millisecond startup, D-Bus-activated via its own `.service` file.

**Config path:** it links `gio::Settings::new(APP_ID)` against the same schema and reads `search-provider-exposure` per `GetInitialResultSet` call. GSettings/dconf is already a shared, change-notifying store — there is **no IPC, no config file, no duplicated state**. Flip the combo row, the next keystroke in the overview behaves differently. One source of truth, and it's the one the app already uses (`src/ui/modals/preferences.rs:55`).

**The provider never writes.** No indexing, no schema migration, no `PRAGMA` that takes a write lock. Typing in the overview must not be able to trigger a reindex. If the DB is missing or stale, return `[]` — a shell search provider is the worst possible place to discover that your index needs rebuilding.

## Mechanical cost per keystroke

The Shell calls on every keystroke. The existing `search_sessions_with_query` (`src/database/mod.rs:453`) is **the wrong query to reuse**: it selects 22 columns, joins `messages_fts → messages → sessions`, materialises *every matching message row*, and dedupes to sessions in Rust with a `HashSet` afterwards. For a broad prefix query that's tens of thousands of rows constructed into `Session` structs to display five. In the session list that's amortised over a deliberate search; on a per-keystroke path it's a disaster.

New sibling function, `search_session_ids_for_shell`, next to it:

- returns `Vec<String>` of session IDs only — no `Session` construction, no `first_prompt`, no token counts;
- `GROUP BY s.id`, `MIN(bm25(messages_fts))` as rank, `ORDER BY rank ASC LIMIT 20` — dedupe happens **in SQLite before the limit**, which is the actual fix;
- keeps `AND s.is_subagent = 0` (a subagent transcript surfacing standalone in Activities with no parent context is a result you can't act on);
- prefix operator `*` appended to the final token only, and FTS special characters escaped — transcripts are full of `"`, `*`, `:` and `^`;
- applies **none** of the assistant/project/date clauses, per Knob 2.

`GetResultMetas` is then called for only the IDs the Shell draws (~5). That's where `first_prompt` and the metadata line are fetched, and — in excerpt mode only — where FTS5 `snippet()` runs. **Excerpt cost is paid 5 times, not 20.** That split is why exposure can be one setting rather than a performance trade.

Budget: **250 ms wall clock** for `GetInitialResultSet`. The handler must be async and must **cancel the in-flight query** when the next keystroke arrives — Shell fires `GetSubsearchResultSet` against the previous result set while the previous call may still be running. A `rusqlite` interrupt handle on the connection, or a one-slot task that drops the stale future. Get this wrong and the overview stutters while typing, which is a design failure even though nothing is visually wrong.

## Result rendering in the overview

- **`id`**: the session ID. Already stable and already the app's primary key — no synthetic ID scheme needed.
- **`icon`**: the per-assistant symbolic icon where we ship one, app icon otherwise. Free visual disambiguation between five assistants in a list with no filter chips.
- **`name`**: `first_prompt`, single line, ellipsized. It's the session's real identity and it's what the session row already shows.
- **`description`**: governed by the one key above.
- **`LaunchSearch`**: opens the app with the query pre-filled in the existing search entry.

## Integration points

- `data/dev.maciz.sessionschronicle.gschema.xml.in` — one key (see above).
- `src/ui/modals/preferences.rs:92` — new `adw::PreferencesGroup` titled **"GNOME Search"**, inserted before the existing "Advanced" group, containing the informational `AdwActionRow` and the `AdwComboRow`. Wire it exactly like the terminal `ComboRow` at `src/ui/modals/preferences.rs:74–86`: build a `gio::ListStore<StringObject>`, select from the current key, write back in `connect_selected_notify`.
- `src/database/mod.rs:453` — new sibling `search_session_ids_for_shell` + a metas-fetch function; do not touch `search_sessions_with_query`.
- New crate binary + `data/dev.maciz.sessionschronicle.SearchProvider.service` + `data/dev.maciz.sessionschronicle.search-provider.ini`, plus `meson.build` install rules.
- ~~`build-aux/dev.maciz.sessionschronicle.json:11` — add `--own-name=dev.maciz.sessionschronicle.SearchProvider` to `finish-args`.~~ **Corrected 2026-08-02:** no `finish-args` change is needed. Flatpak grants `--own=$APP_ID` and `--own=$APP_ID.*`, so a sub-name needs no permission. The name must however be generated per profile as `@APP_ID@.SearchProvider` — the hardcoded string above is a sub-name of the stable app ID but a *sibling* of `dev.maciz.sessionschronicle.Devel`, so it would not be granted for development builds and would collide with the stable provider.
- ~~`data/dev.maciz.sessionschronicle.desktop.in.in` — `DBusActivatable=true`.~~ **Shipped in #197**, along with the application's own per-profile D-Bus service file. No longer part of #189's delta.
- ~~`src/main.rs:35` — `--session <id>` and `--search <query>` clap args, plus app-side deep-linking in `app.rs`.~~ **Corrected 2026-08-02:** deep-linking is a private stateless GApplication action `open-session` (parameter `s`), not a CLI argument — a public `--session` and a URI scheme were explicitly rejected as non-goals. `LaunchSearch`'s counterpart will likewise be a sibling action, not `--search`. (`app.rs` no longer exists either; the module was split into `src/app/`.)

## Risks

- ~~**Deep-linking does not exist.**~~ **Retired 2026-08-02 — this risk was realised as a prerequisite and closed.** #197 / [PR #200](https://github.com/supermaciz/sessions-chronicle/pull/200) shipped exactly the missing piece: `ActivateResult` now calls `org.freedesktop.Application.ActivateAction("open-session", ["<id>"])`, and the app handles cold start, warm present, and three non-destructive failure toasts. The "dead in the hand" outcome this risk warned about did not happen. Two obligations it leaves on #189: the provider must return only top-level IDs (`s.is_subagent = 0`), since a subagent ID resolves to a *Session not found* toast; and `--sessions-dir` instances are `NON_UNIQUE` and cannot receive the action, so they are unusable for activation testing.
- **Stale index while the app is closed.** The provider reads whatever the last indexing run left. Acceptable and correct; the alternative (indexing from the provider) is a write storm triggered by typing.
- **Excerpt mode and screen sharing.** Mitigated by defaulting away from it, not eliminated. Consider a one-line hint under the combo row when excerpt is selected: *"Matching transcript text will appear in the Activities overview."* Present tense, no scare language, no dialog.
- **Assistant symbolic icons** must exist as installed themed icons the Shell process can load — icons resolved only from our GResource bundle will render blank in the overview. Fall back to the app icon if that isn't in place.

## Complexity

**Configuration surface: Small.** One GSettings key, one combo row, one static info row, one read in the provider process. Roughly a day.

**The rest of #189: Large** — separate binary, D-Bus activation, deep-linking, packaging, and a bounded async query path. Scoping this exploration to "what's configurable" is right, but the configuration is not what makes this issue expensive, and the summary table should not imply otherwise.

## Verification

- ~~`--sessions-dir tests/fixtures`, index, then from the overview: type 2 characters → **no results, no SQLite query**~~ — **invalid, corrected 2026-08-02.** The assertion is right; the harness is not. Shell activates the provider with no arguments, so it resolves the *default* database, while `--sessions-dir` writes to `sessions-override.db` (`src/session_sources.rs:125-127`) — the 2-character case would pass for the wrong reason, and every query would return nothing. Drive the provider directly instead (`gdbus call … GetInitialResultSet "['ak']"`) against a fixture database passed by an override used only for direct invocation, never in the service file's `Exec`. Overview-driven and activation checks use the default index.
- Query a string present only in a message body, never in a first prompt (e.g. a token from a fixture tool call). Under *Session titles only* → absent. Under *Full transcripts* → present with a metadata description. Under *…with excerpt* → present with the surrounding text. **If any two options render identically for some query, the setting failed the test and one value should be deleted.**
- Hold a key down in the overview to fire rapid subsearches on the largest fixture: no stutter, no growing memory, and the last query's results win (proves cancellation, not just queueing).
- Flip the combo row with the overview open, reopen and retype — behaviour changes without restarting the provider (proves the GSettings read is per-call, not cached at startup).
- Turn the provider off in GNOME Settings ▸ Search: our Preferences group must not change appearance or claim anything. It's a signpost; signposts don't blink.
- Large text and high contrast: the `AdwComboRow` option strings are long ("Full transcripts with excerpt") — confirm they ellipsize rather than push the group wider at narrow dialog widths.

---

# Amendment (2026-08-01) — Knob 5 withdrawn in part

Everything above is the proposal as first written. It was challenged on an internal contradiction and the challenge was conceded. The original argument is kept intact rather than rewritten, because the failure mode it demonstrates is the useful part of this document.

## The contradiction

**Knob 2** deletes per-assistant, per-project and date scoping because the state is invisible at the point of use — "no sidebar, no chip, no affordance" — and closes with *"nobody wants a search that quietly can't find things."*

**Knob 5** then ships **Session titles only** as the dial's first position, which stops matching against `messages_fts` and falls back to `LIKE` on `first_prompt` / `project_path` / assistant name. Query `AKIA`: nothing, no explanation on screen, because of a combo row set months ago. That is the defect Knob 2 disqualifies.

## Conceded, and worse than charged

**The dial was not one axis.** It was sold as monotonically increasing exposure — the only framing that justifies three values in one `AdwComboRow` rather than two separate knobs. It isn't true. Position 1→2 changes *what matches*; position 2→3 changes *what renders*. Two mechanisms, two failure modes, welded into one widget because a scale needs three points. That is surface multiplication hiding inside a single control.

The tell is in the original justification: *titles only* was defended as the option for people who find full-text results **noisy**. Noise, not exposure. It was a recall knob wearing the exposure knob's clothes, and since the default was already `"transcripts"`, it wasn't even the safe position. It existed to give the scale a bottom end.

## The principled defence, recorded so it is rejected knowingly

GNOME Files' search provider matches filenames, not contents, and nobody files bugs — because "search matches names" is a total, one-sentence rule the user can hold, and the name it matched is the name the result displays. Under *titles only*, `name` is `first_prompt`, so the rule would be "the shell matches what it shows you." That is self-consistent in a way a stale assistant filter never is: a stale filter excludes on an axis orthogonal to the query and invisible in the result; *titles only* excludes by *where in the document* the match lives, uniformly, for every query, forever. A rule rather than an exception.

**It dies on contact with this corpus.** Files has no body corpus — filename matching is not a reduced mode there, it is the entire product. We have `messages_fts`, already built and already paid for, and the whole reason anyone wants #189 is to find a session by something they remember from *inside* it. A mode that switches off the only capability distinguishing this provider from the `Keywords=` line already in `data/dev.maciz.sessionschronicle.desktop.in.in` is not a taste option. It is a downgrade with a label.

## Why rendering-only is the stronger constraint

Not taste — mechanics, and this is the transferable part: **a setting whose safe direction is also the weaker-recall direction forces the user to price their own paranoia against their own memory, in a surface that shows them neither.** Rendering-only removes the trade outright. The safe default costs zero recall, so it needs no argument and no user reasoning.

Gating matching is honest only where the matched field *is* the result identity — Files, Calculator, Characters. Not here.

## What replaces the dial

**Two rendering-only values is a boolean, and it should look like one.** `AdwSwitchRow`, not `AdwComboRow` — a two-item combo is a switch that makes you click twice. Named after the mechanism it controls, which is the `description` string returned by `GetResultMetas`:

```xml
<key name="search-provider-show-excerpts" type="b">
  <default>false</default>
  <summary>Show matching transcript text in GNOME Shell search results</summary>
</key>
```

Row title **"Show matching text in results"**  
Subtitle *"Transcript excerpts appear in the Activities overview."*

`search-provider-exposure` is withdrawn — "exposure" was the honest word for a dial that no longer exists, and against a boolean it is vaguer than the thing it names. Type `b` breaks from the `resume-terminal` / `sort-order` string convention cited earlier, correctly: those are enums, this is not.

## What is lost, and one cost the safe default really carries

Nothing on the exposure axis. One real concern leaves with *titles only* — that a 3-character query against full transcript bodies floods the overview with weak matches. That was never a settings problem: the 3-char floor, bm25 ordering and `LIMIT 20` are the instruments, and if they prove insufficient the fix is a bm25 score cutoff inside the provider, not a dialog asking the user to compensate for our ranking.

**The safe default is not free, and the doc should say so.** With excerpts off, `description` is always `Claude Code · sessions-chronicle · 3 days ago`. Two sessions in the same project on the same day are indistinguishable in the overview — the discriminating information is exactly the text we chose not to render. Still the right call, but it makes `name` load-bearing: `first_prompt`, ellipsized, is the only thing separating results, so it must never fall back to a filename or a session ID.

Note also that deferring `snippet()` to `GetResultMetas` now buys nothing *at the default* — it only pays off for users who opt in. Keep it regardless: it is free, and it is the difference between excerpt mode costing 5 snippets and 20.

## Unchanged

The informational `AdwActionRow` signpost instead of a second on/off; no scoping axes; the 3-character floor and `LIMIT 20` as constants rather than knobs; `ActivateResult` opens rather than resumes; the separate no-GTK provider binary; `search_session_ids_for_shell` with dedupe pushed into SQLite before the limit; query cancellation on the next keystroke; and deep-linking as the prerequisite that decides whether this ships alive or ships as a screenshot.
