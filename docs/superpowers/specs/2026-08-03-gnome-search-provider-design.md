# GNOME Shell search provider — Design

**Date**: 2026-08-03  
**Status**: Approved — ready for an implementation plan.  
**Issue**: [#189 — feat: expose session search in GNOME Activities](https://github.com/supermaciz/sessions-chronicle/issues/189)  
**Implements**: [`docs/explorations/2026-08-01-gnome-search-configurability-exploration.md`](../../explorations/2026-08-01-gnome-search-configurability-exploration.md) — its [Recommendation](../../explorations/2026-08-01-gnome-search-configurability-exploration.md#recommendation) section.  
**Builds on**: [`2026-08-01-deep-link-session-design.md`](2026-08-01-deep-link-session-design.md) — the `open-session` action contract shipped in [PR #200](https://github.com/supermaciz/sessions-chronicle/pull/200).

---

## Summary

Ship an `org.gnome.Shell.SearchProvider2` implementation so indexed sessions appear
in the GNOME Activities overview, as a **separate GTK-free binary** reading the same
SQLite database, and expose exactly **one** preference — whether transcript excerpts
are allowed to render outside the app window.

The corpus behind this provider is full AI-assistant transcript bodies. `GetResultMetas`
returns a `description` string that Shell renders in the overview — the most
screen-shared surface on the desktop, and one Orca reads aloud. That is why exposure is
configurable and why the restrained value is the default.

## Decisions this spec locks in

| Question | Decision |
|---|---|
| Provider process | Separate binary, no GTK |
| Code sharing | Cargo workspace with a GTK-free `sessions-chronicle-core` crate |
| Configuration | One key: `search-provider-show-excerpts` (`b`, default `false`) |
| Does the setting gate matching? | No — rendering only |
| In-app on/off switch | None. Settings ▸ Search owns on/off |
| `LaunchSearch` | Ships in v1, carries the query via a new `search-sessions` action |
| Result icon | The application icon, for every result |
| Per-project exclusion | Deferred |

## Non-goals

- Recent or suggested sessions for an empty query.
- Provider-specific query syntax or filters.
- Secondary result actions (resume, reveal in files).
- Per-project exclusion from search. Both the UI Designer and Creative proposals
  independently placed it on the project sidebar row rather than in Preferences;
  that agreement is banked, the feature is not built here. Its storage question
  (`as` GSettings key versus a `projects.exclude_from_shell_search` column) is
  deferred with it.
- Creative's live preview of a mock Shell result row.
- Per-assistant result icons. See [Result icon](#result-icon).

---

## Architecture

### Workspace layout

Three crates. The repository root stays the application package.

```
Cargo.toml                    # [workspace] + the sessions-chronicle package
crates/core/                  # sessions-chronicle-core — no GTK
crates/search-provider/       # sessions-chronicle-search-provider
src/                          # the application, unchanged in shape
```

`sessions-chronicle-core` receives six modules moved verbatim from `src/`:
`database/`, `models/`, `parsers/`, `session_sources.rs`, `project_resolver.rs`,
`utils/`. None of them imports `gtk`, `glib`, `gio`, `relm4`, or `adw` today, so the
extraction is mechanical rather than a rewrite.

The application crate replaces its `mod database;` (and siblings) declarations with
`use sessions_chronicle_core::…`, and `src/lib.rs` re-exports the core modules under
their existing names so that the ~175 tests under `tests/` keep compiling unchanged.

`sessions-chronicle-search-provider` depends on:

- `sessions-chronicle-core` — the query path and database-path resolution.
- `zbus` 5 — the D-Bus interface, for its `#[interface]` macro and async dispatch.
- `gio`/`glib` — **only** for `gio::Settings` and `glib::user_data_dir()`.

Pulling in glib is not a GTK dependency and does not open a display connection. It buys
the same GSettings reader and the same data-directory resolution the application uses,
rather than a second implementation of either. Plain `gio::Settings` reads work without
a running main loop, and this provider only ever reads.

### Why not in the application process

`#197` made in-process hosting technically possible by giving the app a bus name, and
simultaneously disqualified it: the root window is declared `set_visible: true`
(`src/app/mod.rs:259`), and a D-Bus activation *is* a launch. Typing three letters in
the overview would map an unrequested window on top of Activities, mid-keystroke.
Making that lazy requires hold/release counting and turning Sessions Chronicle into a
background service, which #197 lists as an explicit non-goal. The app also starts an
incremental index on startup (`src/app/mod.rs:621`).

The warm-process argument does not survive either: Shell queries providers whether or
not the app is running, so the same keystroke would answer silently when warm and pop a
window when cold — the same input producing two different desktops.

### Why the query lives in core, not in the provider

`crates/core/src/database/shell_search.rs` owns both SQL functions. The provider crate
writes no `rusqlite`. This keeps all SQL in one module and, more importantly, makes the
per-keystroke query testable through the existing fixture-database harness instead of
only through D-Bus.

---

## Packaging

Three new generated files, all per-profile. The `.ini` basename is prefixed with the
App ID so Flatpak exports it.

| Generated file | Installed to | Key contents |
|---|---|---|
| `data/dev.maciz.sessionschronicle.search-provider.ini.in` | `datadir/gnome-shell/search-providers/@APP_ID@.search-provider.ini` | `DesktopId=@APP_ID@.desktop`, `BusName=@APP_ID@.SearchProvider`, `ObjectPath`, `Version=2`, `DefaultDisabled=true` |
| `data/dev.maciz.sessionschronicle.SearchProvider.service.in` | `datadir/dbus-1/services/@APP_ID@.SearchProvider.service` | `Name=@APP_ID@.SearchProvider`, `Exec=@BINDIR@/sessions-chronicle-search-provider` — **no arguments** |
| — | `bindir/sessions-chronicle-search-provider` | second `custom_target` in `src/meson.build` |

This matches every search provider installed on the reference machine (Alpaca, Bazaar,
Devtoolbox, Icon Library, Cartridges): all five set `DefaultDisabled=true` and point
`DesktopId` at the main application desktop file even when the provider owns a separate
bus name.

**No `finish-args` change.** Flatpak auto-grants `--own=$APP_ID` and `--own=$APP_ID.*`,
so `@APP_ID@.SearchProvider` needs no permission. None of the five reference providers
declares an `own-name`.

**The bus name must be generated per profile.** Hardcoding
`dev.maciz.sessionschronicle.SearchProvider` is a sub-name of the stable App ID but a
*sibling* of `dev.maciz.sessionschronicle.Devel` — not auto-granted for the development
profile, and colliding with the stable build's provider.

**The object path is derived at runtime** from `APP_ID` (dots to slashes, prefixed with
`/`, suffixed with `/SearchProvider`), so no new meson variable is needed. For the
development profile that yields
`/dev/maciz/sessionschronicle/Devel/SearchProvider`.

`build-aux/validate-activation-metadata.py` is extended to validate the provider trio —
the `.ini`'s `BusName` matches the service file's `Name`, the service file's `Exec`
points at the installed provider binary, and the `DesktopId` names the installed desktop
file. The equivalent check already exists for the desktop/service pair.

### Development override

The provider accepts `--database <path>` for direct invocation only. It never appears in
the service file's `Exec`, so Shell can never activate a fixture-backed provider. With
no argument the provider resolves the default database exactly as the app does:
`glib::user_data_dir() / APP_ID / "sessions.db"`.

Note that `--sessions-dir` writes to `sessions-override.db`
(`select_db_filename`, `src/session_sources.rs:125-127`), so a fixture database is
reached with `sessions-chronicle --print-db-path --sessions-dir tests/fixtures`.

### Lifecycle

The provider exits with status 0 after roughly 30 seconds without a method call. D-Bus
activation restarts it on demand.

---

## Query path

### Building the match expression

Shell hands over `terms: Vec<String>`, which is raw user keystrokes. The application
currently passes such text straight to `messages_fts MATCH ?` — `SessionQuery::classify`
(`src/models/session_query.rs`) only separates empty, `id:`-prefixed, and full-text
queries — so a stray quote or a bare `AND` is already an FTS5 syntax error in the in-app
search. Per keystroke, in a Shell surface, that is not acceptable: it is exactly the
"malformed input → fails quietly" acceptance criterion.

The provider therefore builds its own expression and does **not** reuse the app's path:

1. Reduce each term to its alphanumeric and `_` characters.
2. Drop terms that are empty after reduction.
3. Quote each surviving term as `"term"` and join with `AND`.
4. Append `*` to the last term for prefix matching — the user is still typing.
5. If nothing survives, or the joined raw terms total fewer than **3 characters**,
   return an empty result set **without opening SQLite**.

The 3-character floor and the 20-result limit are values, not preferences. Exposing them
would ask the user to tune a query planner.

### `search_session_ids_for_shell(db, match_expr, limit) -> Vec<String>`

```sql
SELECT s.id, MIN(bm25(messages_fts)) AS rank
FROM messages_fts
JOIN messages m ON m.id = messages_fts.rowid
JOIN sessions s ON s.id = m.session_id
WHERE messages_fts MATCH ?1
  AND s.is_subagent = 0
GROUP BY s.id
ORDER BY rank ASC, s.last_updated DESC
LIMIT 20
```

Two columns instead of the 22 that `search_sessions_with_query`
(`src/database/mod.rs:453`) selects, deduplication pushed into SQLite **before** the
limit rather than into a Rust `HashSet` after materialising every matching message row,
and no `Session` struct constructed at all.

`AND s.is_subagent = 0` is defence in depth rather than the only guard — the receiving
`open-session` handler rejects subagent rows — and that is precisely why it must stay.
An unfiltered subagent hit would look like an ordinary overview result and resolve to a
*Session not found* toast on click: a result that opens onto an error.

`GetSubsearchResultSet` ignores the previous result set and re-runs
`GetInitialResultSet`. Filtering a 20-row bounded set gains nothing.

### `result_metas_for_shell(db, ids, match_expr, show_excerpts) -> Vec<ShellResultMeta>`

Called for the handful of rows Shell actually draws. `snippet(messages_fts, …, '…', 32)`
is evaluated only when `show_excerpts` is true, so excerpt cost is paid about 5 times
rather than 20.

### Cancellation

A dedicated database thread owns a single connection and its rusqlite `InterruptHandle`.
A newly arriving query interrupts the in-flight one, which surfaces `SQLITE_INTERRUPT`
and returns empty. This is what makes "the last keystroke wins" true rather than
"keystrokes queue".

Concurrent reads need no new mechanism: `SessionIndexer::new` sets `journal_mode=WAL`
(`src/database/indexer.rs:284`) and `open_connection` sets a five-second busy timeout
(`src/database/mod.rs:28-30`).

Budget: 250 ms per query.

---

## Rendering

| Field | Value |
|---|---|
| `id` | `sessions.id` |
| `name` | `first_prompt`, whitespace collapsed, truncated to 60 characters on a `char` boundary |
| `description`, excerpts off | `Claude Code · sessions-chronicle · 3 days ago` |
| `description`, excerpts on | the matched snippet, whitespace collapsed, truncated to 100 characters |
| `gicon` | the application icon |

Truncation happens on our side, not Shell's, because Orca reads the untruncated string.
Collapsing whitespace is what keeps a 4 KB pasted block or a multi-line stack trace from
producing an oversized overview row.

### `name` fallback

`Session::first_prompt` is `Option<String>` (`src/models/session.rs:54`). Measured
against the reference index: **0 of 1112** top-level sessions lack one, so this is a
rare path — but with excerpts off, `name` is the only thing separating two results, so
it still needs a defined value.

The fallback is `Untitled Claude Code session` (the assistant's display name). It is
never a file path and never a session ID. The description keeps its normal form, so the
row still carries project and time.

### Result icon

Every result carries the application icon. The five assistant symbolic icons are
installed as themed icons (`data/icons/meson.build`), but Flatpak only exports files
whose basename is prefixed with the App ID — verified: the host's
`exports/share/icons/hicolor/symbolic/apps/` contains
`dev.maciz.sessionschronicle-symbolic.svg` and no `claude-code-symbolic.svg`. The Shell
process therefore cannot resolve them, and they would render blank.

Making them exportable means renaming all five with an App-ID prefix, which makes the
icon name profile-dependent and forces `AiAssistant::icon_name()` to stop returning
`&'static str`, touching every in-app `from_icon_name` call site. That is out of scope
here, and the payoff is small: with excerpts off the description already spells the
assistant out in words.

---

## Configuration

### GSettings

```xml
<key name="search-provider-show-excerpts" type="b">
  <default>false</default>
  <summary>Show matching transcript text in system search results</summary>
  <description>
    When enabled, GNOME Activities search results include an excerpt of the matching
    transcript text. When disabled, results show only the session's first prompt,
    assistant, project, and time.
  </description>
</key>
```

Type `b` deliberately breaks from the `resume-terminal` / `sort-order` string
convention. Those are enums with an unknown-value case to fail closed on; this is a
boolean and has none.

The provider reads the key **on every `GetResultMetas` call**. That is what makes
flipping the switch take effect without restarting the provider, and what makes a cold
provider start read the persisted value rather than the schema default.

### Preferences

A new *System Search* group on the existing General page
(`src/ui/modals/preferences.rs`), below *Session Resumption*:

- An `AdwSwitchRow`, title **"Show matching text in results"**, subtitle *"Transcript
  excerpts appear in the Activities overview."*, bound with `settings.bind()`.
- A non-activatable `AdwActionRow` pointing at Settings ▸ Search for the on/off control.

A `SwitchRow` rather than a `ComboRow`: two rendering-only values is a boolean, and a
two-item `ComboRow` is a switch that makes you click twice. The naming carries the
framing that a switch would otherwise get wrong — *"Show matching text in results"* makes
*off* obviously the restrained state.

A signpost row rather than a link row: there is no stable URI for a Settings panel, and
a link that silently fails inside the sandbox is worse than a sentence that is always
true.

### What is not configurable, and why

- **An in-app enable/disable switch.** The OS owns on/off. When Settings ▸ Search has
  the provider off, Shell never opens the D-Bus connection at all, so an in-app switch
  in its *On* position could only ever produce an empty result set. Three of the four
  states in the two-switch truth table render identically.
- **Ranking, result count, provider ordering.** Owned by bm25, by Shell, and by
  Settings ▸ Search respectively.
- **Per-assistant, per-project, or date scoping.** These filters work in the session
  list because the sidebar shows you *why* results are missing. The overview has no such
  affordance.
- **Anything that gates matching.** A setting whose safe direction is also the
  weaker-recall direction forces the user to price their own paranoia against their own
  memory, in a surface that shows them neither. Rendering-only removes that trade: the
  safe default costs zero recall.

### The accepted cost of the default

With excerpts off, `description` is always `assistant · project · relative time`, so two
sessions in the same project on the same day are indistinguishable in the overview — the
discriminating information is exactly the text we chose not to render. This is the right
call, and it makes `name` load-bearing.

---

## Opening a result

### `ActivateResult`

Delegates to the existing contract: `org.freedesktop.Application.ActivateAction` with
`open-session` and the exact `sessions.id`. Cold start, warm present, and all three
failure toasts come for free from #197.

### `LaunchSearch`

Ships in v1 and carries the query. The provider joins the terms with a single space and
invokes `ActivateAction("search-sessions", ["<query>"])`.

Parameter type `s`, not `as`: the provider must join the terms to build its own `MATCH`
expression regardless, so joining app-side would duplicate the same decision in two
places.

### `search-sessions` — application side

A new stateless `GApplication` action registered next to `open-session` in
`src/main.rs`, forwarding an `AppMsg::ExternalSearch(String)`.

`handle_external_search` is the **mirror** of `handle_external_session_open`
(`src/app/handlers/sessions.rs:173`), and deliberately shares no code with it. The two
handlers have opposite contracts over the same widgets: `open-session` *clears* search
mode, the query, transcript highlights, and the search-only sort override;
`search-sessions` must *set* them. Sharing a code path would have them fight over the
search entry.

The handler:

1. Presents the window.
2. Switches to `Workspace::Sessions`. This is mandatory, not cosmetic —
   `workspace_allows_search` (`src/app/helpers.rs:57`) hides the search UI on Analytics,
   so without this the action would be a silent no-op from the Analytics workspace.
3. Pops back from the detail page if one is pushed.
4. Sets `search_visible = true` and `sync_search_bar`.
5. Sets `search_query` and drives the existing search pipeline.

A carried query that matches nothing produces the ordinary empty-search state, not a
toast: the search bar shows the term, so the absence of results is already explained
on screen.

---

## Error handling

**The provider's three query methods — `GetInitialResultSet`, `GetSubsearchResultSet`,
`GetResultMetas` — never return a D-Bus error.** Missing database,
SQLite failure, interrupted query, terms emptied by sanitisation — every case returns an
empty array, with a `tracing` record. This is what #189's "fails quietly" criterion
binds.

That criterion does **not** bind the application. Once a result is activated the
interaction has left Shell, and #197 deliberately reports an unresolvable ID with a
toast rather than a silent no-op:

| Condition | App behaviour (unchanged from #197) |
|---|---|
| Unknown, empty, or subagent ID | *Session not found* |
| Database absent | *Sessions are not indexed yet* |
| SQLite error | *Could not open session* |

All three preserve the user's current view.

---

## Testing

### Core, against the fixture database

- Deduplication happens before `LIMIT`, not after.
- `is_subagent = 0` rows only.
- bm25 ordering, with `last_updated` as tie-breaker.
- The 3-character floor returns empty **without opening SQLite**.
- Term sanitisation: a bare quote, a bare `AND`, emoji, a 4 KB pasted block.
- Truncation lands on `char` boundaries for multi-byte input.
- `snippet()` is not evaluated when `show_excerpts` is false.
- A session with no `first_prompt` yields a non-empty, human-readable `name`.

### Provider, on a private bus

`dbus-run-session` launches the binary with `--database` pointed at the fixture database
and drives the interface directly:

- `GetInitialResultSet(['ak'])` → empty array, and no SQLite query in the log.
- `GetInitialResultSet(['aki'])` → non-empty.
- `GetResultMetas` with excerpts off and on → descriptions differ.

Deterministic, scriptable in CI, no compositor required. This is the query half of the
verification plan, and it must not go through the overview: Shell activates the provider
with no arguments and therefore resolves the *default* database, so a fixture-backed
check driven through Activities would return nothing for every query and the
2-character case would "pass" for entirely the wrong reason.

### Application

A `#[gtk::test]` for `search-sessions`, modelled on the existing `open-session` tests in
`src/app/handlers/sessions.rs`.

### Build

`validate-activation-metadata.py` extended to the provider trio.

### Manual, default index only

A fixture instance is `NON_UNIQUE`, owns no bus name, and cannot receive `open-session`,
so activation testing uses the ordinary default-index instance.

- Packaged Flatpak build: the app appears in Settings ▸ Search, **disabled by default**,
  and nothing in Preferences claims search is active while it is off.
- Activate a result whose session has been deleted or re-indexed away: the app shows
  *Session not found* while the provider itself does not error, log a D-Bus fault, or
  block the overview.
- Hold a key down against the largest index: no stutter, no growing memory, last query
  wins.
- Flip the switch while the provider process is alive: the next keystroke reflects it
  without a restart.
- Provider cold start with the GUI closed reads the persisted value.
- Two sessions in the same project on the same day, excerpts off: confirm their `name`
  strings differ enough to tell them apart. This is the accepted cost of the safe
  default and the one case where it bites.
- A term present only in a message body, never in a first prompt: present in both
  modes, with the description differing.

---

## Sources

- [Writing a Search Provider — GNOME Developer Documentation](https://developer.gnome.org/documentation/tutorials/search-provider.html)
- [Requirements & Conventions — Flatpak documentation](https://docs.flatpak.org/en/latest/conventions.html)
