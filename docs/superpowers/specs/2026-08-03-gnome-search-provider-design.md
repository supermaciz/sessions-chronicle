# GNOME Shell search provider — Design

**Date**: 2026-08-03  
**Status**: Verified — ready for an implementation plan.  
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

The setting gates the **matched body snippet**, not the session title: `first_prompt`
remains the result `name` in both modes and can itself contain sensitive user text. That
always-visible title is an explicit exposure boundary of this design, not a promise that
the excerpt-off mode renders no transcript-derived text.

## Decisions this spec locks in

| Question | Decision |
|---|---|
| Provider process | Separate binary, no GTK |
| Code sharing | A GTK-free `sessions-chronicle-core` crate shared through a Cargo workspace |
| Configuration | One key: `search-provider-show-excerpts` (`b`, default `false`) |
| Does the setting gate matching? | No — rendering only |
| Is `first_prompt` gated? | No — it remains the result title in both modes |
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

### Component boundaries

The design has three Rust crates. The repository root remains the application package.

```
Cargo.toml                    # [workspace] + the sessions-chronicle package
crates/core/                  # sessions-chronicle-core — no GTK
crates/search-provider/       # sessions-chronicle-search-provider
src/                          # the application, unchanged in shape
```

`sessions-chronicle-core` owns the domain model, parsing, session-source resolution,
database filename/path construction from a supplied base directory, and all SQLite
access. It must not depend on GTK, Adwaita, Relm4, GIO, or GLib. Each binary obtains the
platform base directory with GLib and gives it to core. The exact module moves and
compatibility re-exports belong in the implementation plan; the design constraint is
that both binaries use one query and path policy without pulling a display stack into
the provider.

`sessions-chronicle-search-provider` depends on:

- `sessions-chronicle-core` — the query path and database-path resolution.
- `zbus` 5 — the D-Bus interface, using `#[interface]` and an async connection built
  with `zbus::connection::Builder`.
- `gio`/`glib` — **only** for short-lived `gio::Settings` reads and
  `glib::user_data_dir()`.

Pulling in glib is not a GTK dependency and does not open a display connection. It buys
the same GSettings reader and the same data-directory resolution the application uses,
rather than a second implementation of either. Plain `gio::Settings` reads work without
a running main loop, and this provider only ever reads.

The zbus interface object does **not** contain a `gio::Settings`: zbus's `Interface`
trait requires `Send + Sync`, while gtk-rs correctly marks `gio::Settings` as neither.
The database worker creates, reads, and drops a `gio::Settings` on its own thread for
each metadata request, returning only the boolean to async code. No GObject crosses a
thread boundary.

### D-Bus contract

The provider implements version 2 exactly as published by GNOME:

| Method | D-Bus signature |
|---|---|
| `GetInitialResultSet` | `(as) → (as)` |
| `GetSubsearchResultSet` | `(as, as) → (as)` |
| `GetResultMetas` | `(as) → (aa{sv})` |
| `ActivateResult` | `(s, as, u) → ()` |
| `LaunchSearch` | `(as, u) → ()` |

All five methods are asynchronously dispatched. In zbus 5, the wire types map to
`Vec<String>`, `u32`, and result dictionaries whose values are `zvariant::OwnedValue`.
The interface is served at the generated object path before the generated well-known
bus name is requested.

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

Three new generated artifacts, all per-profile. The search-provider basename follows
Flatpak's required `$FLATPAK_ID-search-provider.ini` pattern.

| Generated file | Installed to | Key contents |
|---|---|---|
| Search-provider keyfile | `datadir/gnome-shell/search-providers/@APP_ID@-search-provider.ini` | `DesktopId=@APP_ID@.desktop`, `BusName=@APP_ID@.SearchProvider`, `ObjectPath`, `Version=2`, `DefaultDisabled=true` |
| `data/dev.maciz.sessionschronicle.SearchProvider.service.in` | `datadir/dbus-1/services/@APP_ID@.SearchProvider.service` | `Name=@APP_ID@.SearchProvider`, `Exec=@BINDIR@/sessions-chronicle-search-provider` — **no arguments** |
| Provider executable | `bindir/sessions-chronicle-search-provider` | GTK-free D-Bus service |

`DesktopId` points at the main application desktop file even though the provider owns a
separate bus name. `DefaultDisabled=true` states the privacy decision in source;
Flatpak also marks exported providers disabled, independently of this key.

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

The packaging contract requires the keyfile's `BusName` to equal the service file's
`Name`, the service `Exec` to name the provider executable, and `DesktopId` to name the
installed application desktop file. Packaging verification covers those invariants.

### Development override

The provider accepts `--database <path>` for direct invocation only. It never appears in
the service file's `Exec`, so Shell can never activate a fixture-backed provider. With
no argument the provider resolves the default database exactly as the app does:
`glib::user_data_dir() / APP_ID / "sessions.db"`.

Note that `--sessions-dir` writes to `sessions-override.db`
(`select_db_filename`, `src/session_sources.rs:125-127`), so a fixture database is
reached with `sessions-chronicle --print-db-path --sessions-dir tests/fixtures`.

### Lifecycle

The provider remains alive after D-Bus activation and exits when its bus connection is
lost or the desktop session ends. An inactivity exit would be incorrect while Shell is
still displaying results: a later `GetResultMetas` call must retain the generation-to-
query association used for matched excerpts.

---

## Query path

### Building the match expression

Shell hands over `terms: Vec<String>`, which is raw user keystrokes. The application
currently passes such text straight to `messages_fts MATCH ?` — `SessionQuery::classify`
(`src/models/session_query.rs`) only separates empty, `id:`-prefixed, and full-text
queries — so a stray quote or a bare `AND` is already an FTS5 syntax error in the in-app
search. Per keystroke, in a Shell surface, that is not acceptable: it is exactly the
"malformed input → fails quietly" acceptance criterion.

The provider therefore builds its own expression and does **not** reuse the app's path.
It extracts runs of Unicode alphanumeric characters from every Shell term, rather than
deleting punctuation and accidentally joining the two sides of it. Each resulting token
is quoted, the tokens are joined with `AND` as required by SearchProvider2, and `*` is
placed **after** the closing quote of the final token for prefix matching (`"term"*`).

If no token survives, or the normalized tokens contain fewer than **3 characters** in
total, the provider returns an empty result set before asking core to open SQLite. The
normalizer also accepts at most 32 tokens and 256 normalized characters; oversized input
returns no results instead of creating an unbounded FTS expression.

The character floor, input bounds, and 20-result limit are values, not preferences.
Exposing them would ask the user to tune a query planner.

### Ranked session lookup

FTS5 ranking is calculated per matching message, then materialized before sessions are
grouped. This boundary is required: SQLite rejects an aggregate such as
`MIN(bm25(messages_fts))` directly in the FTS scan with `unable to use function bm25
in the requested context`.

```sql
WITH ranked_messages AS MATERIALIZED (
    SELECT s.id AS session_id,
           s.last_updated,
           messages_fts.rank AS message_rank
    FROM messages_fts
    JOIN messages m ON m.id = messages_fts.rowid
    JOIN sessions s ON s.id = m.session_id
    WHERE messages_fts MATCH ?1
      AND s.is_subagent = 0
)
SELECT session_id,
       MIN(message_rank) AS session_rank,
       MAX(last_updated) AS session_last_updated
FROM ranked_messages
GROUP BY session_id
ORDER BY session_rank ASC, session_last_updated DESC, session_id ASC
LIMIT 20
```

Three narrow columns instead of the 22 that `search_sessions_with_query`
(`src/database/mod.rs:453`) selects, deduplication pushed into SQLite **before** the
limit rather than into a Rust `HashSet` after materialising every matching message row,
and no `Session` struct constructed at all.

`AND s.is_subagent = 0` is defence in depth rather than the only guard — the receiving
`open-session` handler rejects subagent rows — and that is precisely why it must stay.
An unfiltered subagent hit would look like an ordinary overview result and resolve to a
*Session not found* toast on click: a result that opens onto an error.

`GetSubsearchResultSet` searches only within the sessions named by
`previous_results`. SearchProvider2 defines a subsearch as a refinement that may return
fewer previous results, but not new ones. The bounded previous set therefore matters for
contract correctness even if a full re-query would be fast.

### Result identity and matched excerpts

`GetResultMetas` receives identifiers only; it does not receive the search terms. The
provider therefore returns opaque, generation-scoped result identifiers. Each identifier
contains a safely encoded session ID plus a generation token, while a small bounded map
associates the generation with its normalized match expression. This prevents
overlapping metadata requests from using whichever query happened to arrive last.
`GetSubsearchResultSet` creates a new generation from the session IDs decoded from
`previous_results`, and `ActivateResult` decodes the session ID before invoking the
application. If the process is unexpectedly restarted, activation still works and
metadata falls back to the excerpt-off form because the optional expression cache is
gone.

For the handful of rows Shell actually draws, `snippet(messages_fts, …, '…', 32)` is
evaluated against the stored expression only when `show_excerpts` is true. When excerpts
are off, the metadata query does not join or invoke FTS5 at all.

### Cancellation

A dedicated database thread owns a single connection. It publishes the connection's
thread-safe rusqlite `InterruptHandle` to the D-Bus side; a newly arriving query or the
250 ms deadline interrupts the in-flight statement, which surfaces `SQLITE_INTERRUPT`
and returns empty. This is what makes "the last keystroke wins" true rather than
"keystrokes queue" and makes the stated budget enforceable.

Concurrent reads need no new mechanism: `SessionIndexer::new` sets `journal_mode=WAL`
(`src/database/indexer.rs:284`) and `open_connection` sets a five-second busy timeout
(`src/database/mod.rs:28-30`).

Budget: a hard 250 ms deadline per query.

---

## Rendering

| Field | Value |
|---|---|
| `id` | the opaque provider result identifier supplied to `GetResultMetas` |
| `name` | `first_prompt`, whitespace collapsed, truncated to 60 characters on a `char` boundary |
| `description`, excerpts off | `Claude Code · sessions-chronicle · 3 days ago` |
| `description`, excerpts on | the matched snippet, whitespace collapsed, truncated to 100 characters |
| `gicon` | the application icon name |

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

SearchProvider2 still accepts textual `gicon`, but its installed introspection XML marks
that field deprecated in favour of `icon`, a serialized `GIcon`. This design uses
`gicon` deliberately: the value is a single themed icon name and zbus can encode it as
an ordinary D-Bus string without translating GLib's nested `GVariant` serialization.
Compatibility with the targeted GNOME Shell version is verified in the packaged build;
if Shell removes `gicon`, the provider moves to serialized `icon` without changing the
user-visible design.

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

The provider reads the key **on every `GetResultMetas` call**, using a short-lived
`gio::Settings` confined to the database worker thread. That is what makes flipping the
switch take effect without restarting the provider, makes a cold provider start read the
persisted value rather than the schema default, and satisfies the `Send + Sync` bound on
the zbus interface.

### Preferences

A new *System Search* group appears on the existing General preferences page, below
*Session Resumption*:

- An `AdwSwitchRow`, title **"Show matching text in results"**, subtitle *"Transcript
  excerpts appear in the Activities overview."*. Its `active` property is bound
  bidirectionally to the boolean GSettings key.
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

Resolves the opaque result identifier to its exact `sessions.id`, then delegates to the
existing application contract. The call target is the application App ID at the standard
App-ID-derived object path, on `org.freedesktop.Application.ActivateAction` with the
signature `(sava{sv})`:

- action name: `open-session`;
- parameter array: one variant containing the session ID string;
- platform data: `desktop-startup-id` set to `_TIME<timestamp>`, using the user-event
  timestamp supplied by Shell.

The call is awaited before returning from the provider method. Cold start, warm present,
and all three failure toasts then come from #197.

### `LaunchSearch`

Ships in v1 and carries the query. The provider joins the terms with a single space and
invokes the same `(sava{sv})` method with action name `search-sessions`, one string
variant in the parameter array, and the same `_TIME<timestamp>` platform data.

Parameter type `s`, not `as`: the provider must join the terms to build its own `MATCH`
expression regardless, so joining app-side would duplicate the same decision in two
places.

### `search-sessions` application behavior

A new stateless `GApplication` action accepts one string parameter. Its user-visible
contract is the inverse of opening a particular result: it presents the existing window,
opens the Sessions workspace and search surface, and submits the carried query through
the existing search pipeline.

`handle_external_search` is the **mirror** of `handle_external_session_open`
(`src/app/handlers/sessions.rs:173`), and deliberately shares no code with it. The two
handlers have opposite contracts over the same widgets: `open-session` *clears* search
mode, the query, transcript highlights, and the search-only sort override;
`search-sessions` must *set* them. Sharing a code path would have them fight over the
search entry.

The resulting state is the Sessions list with detail navigation dismissed, the search
surface visible, and the carried query active. Switching workspace is mandatory because
Analytics does not expose session search. The precise Relm4 messages and widget-update
order belong in the implementation plan.

A carried query that matches nothing produces the ordinary empty-search state, not a
toast: the search bar shows the term, so the absence of results is already explained
on screen.

---

## Error handling

**The provider's three query methods — `GetInitialResultSet`, `GetSubsearchResultSet`,
`GetResultMetas` — never return an application-level D-Bus error.** Missing database,
SQLite failure, interrupted query, and terms emptied by sanitisation are recorded with
`tracing`, not exposed to Shell. Initial and subsearch failures return an empty result
array.

`GetResultMetas` preserves the interface invariant of one dictionary per requested
identifier, in the same order. If an identifier cannot be resolved or metadata cannot be
loaded, that entry uses the same opaque `id`, the application `gicon`, the generic name
*Session unavailable*, and no description. If only the generation's match expression was
lost, the normal excerpt-off metadata is returned. This is quieter and more contract-
correct than returning an array whose length does not match the identifiers.

Activation methods likewise complete normally after logging an application-activation
failure; a temporarily unavailable GUI must not turn into a Shell search-provider fault.

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

## Acceptance and verification

### Core, against the fixture database

- The materialized per-message rank is valid SQLite FTS5 and deduplication happens
  before `LIMIT`, not after.
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
- The result metadata `id` exactly matches its opaque input identifier, and one
  dictionary is returned per requested identifier.
- Overlapping generations retain their own snippet expression.
- A subsearch never introduces an identifier from outside `previous_results`.

Deterministic, scriptable in CI, no compositor required. This is the query half of the
verification plan, and it must not go through the overview: Shell activates the provider
with no arguments and therefore resolves the *default* database, so a fixture-backed
check driven through Activities would return nothing for every query and the
2-character case would "pass" for entirely the wrong reason.

### Application

Headless GTK coverage verifies the `search-sessions` behavior contract: an existing
window is reused, Analytics switches to Sessions, detail navigation is dismissed, and
the carried query is visible and active.

### Build

Packaging validation covers the provider keyfile, D-Bus service, executable, desktop
file, per-profile names, and Flatpak export naming as one consistent activation chain.

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
- [`org.gnome.Shell.SearchProvider2` introspection XML](https://gitlab.gnome.org/GNOME/gnome-shell/-/blob/main/data/dbus-interfaces/org.gnome.ShellSearchProvider2.xml)
- [Requirements & Conventions — Flatpak documentation](https://docs.flatpak.org/en/latest/conventions.html)
- [D-Bus Activation — Desktop Entry Specification](https://specifications.freedesktop.org/desktop-entry/latest/dbus.html)
- [SQLite FTS5 Extension](https://www.sqlite.org/fts5.html)
- [zbus service documentation](https://github.com/z-galaxy/zbus/blob/main/book/src/service.md)
- [zbus `Interface` trait](https://docs.rs/zbus/5/zbus/object_server/trait.Interface.html)
- [gtk-rs `gio::Settings`](https://gtk-rs.org/gtk-rs-core/stable/latest/docs/gio/struct.Settings.html)
- [rusqlite `InterruptHandle`](https://docs.rs/rusqlite/0.40.0/rusqlite/struct.InterruptHandle.html)
