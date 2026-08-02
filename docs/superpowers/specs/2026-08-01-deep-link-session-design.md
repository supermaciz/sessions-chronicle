# Open Session by ID — Design Spec

**Date:** 2026-08-01  
**Status:** Implemented by [PR #200](https://github.com/supermaciz/sessions-chronicle/pull/200)  
**Issue:** [#197 — deep-linking: open a session by ID from outside the app](https://github.com/supermaciz/sessions-chronicle/issues/197)  
**Blocks:** [#189 — expose session search in GNOME Activities](https://github.com/supermaciz/sessions-chronicle/issues/189)  
**Related exploration:** [`docs/explorations/2026-08-01-gnome-search-configurability-exploration.md`](../../explorations/2026-08-01-gnome-search-configurability-exploration.md)

## Goal

Allow a component shipped with Sessions Chronicle to ask the desktop application to open one indexed top-level session by `sessions.id`.

The request must work when the application is closed or already running. It must present one existing window, navigate directly to the requested session, and never create a second application instance or window.

This is the minimum deep-link contract required by the GNOME Activities search provider in #189. It is an internal protocol between components released together, not a public integration API.

## Non-goals

- A public `--session` command-line option.
- A `sessions-chronicle:` URI scheme or MIME handler.
- A stable API for third-party scripts or applications.
- Pre-filling the in-app search entry for `LaunchSearch`.
- Opening subagent sessions by ID.
- Resuming a session in a terminal.
- Turning the application into a lazy background D-Bus service that starts without a window.
- Implementing or packaging the search provider from #189.
- Changing the database schema, session IDs, indexing policy, or source path resolution.

## Ground facts

- `src/main.rs:88` currently obtains Relm4's default `adw::Application` through `main_application()` without assigning `APP_ID`. GApplication uniqueness and D-Bus export are therefore disabled.
- `RelmApp::run` launches the root component from GApplication's `startup` signal (`relm4-0.11.0/src/app.rs:162`). GApplication emits `startup` before the `activate`, `open`, `command-line`, or action entry point that caused a cold start.
- `RelmApp::from_app` calls `set_main_application` (`relm4-0.11.0/src/app.rs:48`), so the `main_application()` call inside the root `view!` (`src/app/mod.rs:254`) resolves to the same instance. Setting the ID on the existing global application is equivalent and shorter.
- `RelmApp` exposes `allow_multiple_instances(bool)`, which toggles `gio::ApplicationFlags::NON_UNIQUE` on the underlying application (`relm4-0.11.0/src/app.rs:93`).
- GLib adds the `--gapplication-service` option in `g_application_real_local_command_line` whenever the flags contain neither `IS_SERVICE` nor `IS_LAUNCHER`. It does **not** depend on `HANDLES_COMMAND_LINE`, so D-Bus service activation needs no extra application flag.
- The local Clap parser currently absorbs unknown options into `gtk_options` through `trailing_var_arg` and `allow_hyphen_values`, and forwards them to GApplication. Verified: `sessions-chronicle --gapplication-service --print-db-path` reaches GTK rather than erroring — but everything after the first unknown option is swallowed, so `--print-db-path` is no longer handled locally in that ordering. The same defect would prevent a later `--sessions-dir` from selecting `NON_UNIQUE`.
- The root window is declared with `set_visible: true` in the `view!` macro (`src/app/mod.rs:255`) while `main.rs` uses `visible_on_activate(false)`. A cold start therefore always maps a window, including one triggered by D-Bus activation. Sessions Chronicle is not a lazy background service.
- A GApplication with an application ID owns that well-known name on the session bus and routes application actions to the primary instance. An application carrying `NON_UNIQUE` does not own the name and is not reachable through it.
- The existing `AppMsg::SessionSelected` path already loads a session by exact ID and drives `SessionDetail` (`src/app/handlers/sessions.rs:42`), but its missing-session branch opens an empty detail page and provides no user feedback (`src/app/handlers/sessions.rs:52-63`). External activation needs a non-destructive failure path.
- The search entry is shared by session-list filtering and transcript highlighting. Carrying an unrelated in-app query into an externally opened result would show stale context.
- `sessions.id` is a `TEXT PRIMARY KEY`; `load_session` binds it as a SQLite parameter. The row ID is stable across ordinary reindexing.
- `load_session` returns `Ok(None)` when the database file does not exist (`src/database/mod.rs:939-941`). A missing index is therefore indistinguishable from a missing row unless the handler checks the path itself.
- `SessionIndexer::new` enables WAL, and every connection created through `open_connection` has a five-second busy timeout. A cold-start read can therefore coexist with incremental indexing without a new concurrency mechanism in #197.
- `--sessions-dir` selects a separate database file (`sessions-override.db`) so fixture runs never contaminate the default index (`docs/DEVELOPMENT_WORKFLOW.md:106`). The documented dev loop and the checked-in IDE run configurations rely on a fixture instance running next to an ordinary instance.
- Meson already derives `APP_ID` as `dev.maciz.sessionschronicle` for stable builds and `dev.maciz.sessionschronicle.Devel` for development builds.
- `po/POTFILES.in` still lists `src/app.rs`, which no longer exists since the app module was split into `src/app/`. No string under `src/app/` is currently extracted.

## Decisions

| Topic | Decision |
|---|---|
| Transport | Private stateless GApplication action `open-session` |
| Parameter | One GVariant string (`s`) containing the exact session ID |
| Public API | None; action name and ID format may change atomically with the provider |
| Supported target | Indexed top-level session (`is_subagent = false`) |
| Cold start | Build the Relm4 component in `startup`, then handle the action |
| Warm start | Present the existing window and replace/open its detail page |
| Unknown ID | Preserve all current UI state and show a toast |
| Subagent ID | Treat as unavailable; preserve state and show the same toast |
| Index not built yet | Preserve state, show a distinct “index not ready” toast |
| SQLite failure | Preserve state, show a generic failure toast, log details |
| Active search on success | Clear search mode, query, transcript highlights, and search-only sort override |
| Existing filters | Preserve assistant, project, date, pinned, and sort settings |
| Existing parent context | Clear it before showing the externally selected top-level session |
| `LaunchSearch` | Deferred to #189 or a follow-up |
| `--sessions-dir` | Set `NON_UNIQUE`; the fixture instance runs independently and is not deep-linkable |
| Application flags | Default flags only; no `HANDLES_COMMAND_LINE` |
| Packaging | D-Bus service file plus `DBusActivatable=true` |
| Additional D-Bus permission | None expected for the application's own App ID; verify in Flatpak |

## Alternatives considered

### Public command line

`sessions-chronicle --session <id>` would be convenient for scripts and manual testing. It would also make the command and the ID a compatibility promise, require command-line forwarding to the primary instance, and force definitions for Flatpak invocation and option combinations. None of that improves the #189 result-activation path, so it is deferred until a concrete third-party use case exists.

### URI through `org.freedesktop.Application.Open`

A `sessions-chronicle://session/<id>` URI would work with `gio open` and clickable links. It would require a URI grammar, escaping rules, desktop handler registration, and long-term compatibility. More importantly, the ID only names a row in one local index, so the URI would look shareable without being portable across machines, profiles, or deleted sessions. The provider can activate an application action directly, so #197 does not introduce a URI.

### Rejecting `--sessions-dir` in a remote invocation

An earlier version of this design kept a single unique application for every invocation and made a second launch carrying `--sessions-dir` fail explicitly, so its override payload could not be silently discarded. That required `HANDLES_COMMAND_LINE`, a `command-line` handler, a primary/remote decision table, and an explicit `activate` call — because with that flag GApplication no longer activates on its own.

It also broke the documented development loop. Today the application has no ID and no uniqueness, so a fixture instance backed by `sessions-override.db` runs beside an ordinary instance; `docs/DEVELOPMENT_WORKFLOW.md` and the checked-in IDE run configurations assume it. Adopting an App ID without further precaution would turn that everyday command into a hard error.

`NON_UNIQUE` is both simpler and more correct. A fixture instance does not own the bus name, so it cannot receive `open-session` — which is the right semantics, since an ID drawn from the fixture index means nothing to a provider reading the default index. The override payload is never discarded because the invocation is never remote. The flag, the handler, the decision table, and their tests all disappear.

### Custom D-Bus interface

A dedicated `OpenSession(s)` method would duplicate functionality already supplied by GApplication's exported action group and add custom introspection and registration code. The standard application action has the required cold-start and primary-instance semantics.

## Architecture

### GApplication setup

`src/main.rs` keeps the existing `main_application()` handle and assigns the generated `APP_ID` to it before `RelmApp::from_app`. Because `from_app` re-registers that same object as Relm4's main application, the `main_application()` call inside the root `view!` continues to resolve to it; no second application object is ever created. Stable and development builds use different IDs and may run side by side.

No application flag is added. `HANDLES_COMMAND_LINE` is deliberately not used: `--gapplication-service` does not require it, and setting it would suppress automatic `activate` emission for no benefit.

When the locally parsed arguments contain `--sessions-dir`, `main.rs` sets `gio::ApplicationFlags::NON_UNIQUE` (through `RelmApp::allow_multiple_instances(true)`) before running. That instance does not own the bus name, does not merge into a running instance, and cannot receive `open-session`. This preserves the documented fixture workflow exactly as it behaves today and keeps the fixture index out of the deep-link surface.

Without `--sessions-dir` the application is unique. A second launch is a remote invocation, GApplication activates the primary instance, and the local process exits.

The application's own `activate` handler presents its sole registered window, using `active_window()` when available and otherwise the first application window. This handles desktop launches and ordinary second invocations without depending on Relm4's `set_visible`-only activation behavior, which is disabled here by `visible_on_activate(false)`.

The existing local Clap pass remains responsible for:

- resolving the instance's `sessions_dir` payload and deciding the uniqueness flag;
- handling `--print-db-path` locally and exiting before GApplication starts;
- separating options that GApplication/GTK must still receive, including `--gapplication-service`.

The current `trailing_var_arg` catch-all is replaced by an order-independent separation pass. Before the `--` delimiter, it extracts the exact local forms `--sessions-dir DIR`, `--sessions-dir=DIR`, and `--print-db-path`; every other argument remains in its original order and is forwarded to GApplication. Everything after `--` is forwarded unchanged. Clap then validates the extracted local arguments, retaining diagnostics for a missing directory value and duplicate local options.

This guarantees that a local option appearing after a GTK/GApplication option still affects startup. In particular, `sessions-chronicle --display=:1 --sessions-dir tests/fixtures` sets `NON_UNIQUE`, while `sessions-chronicle --gapplication-service --print-db-path` handles `--print-db-path` locally instead of starting GApplication. The separator must not maintain a hand-written list of GTK options; unknown non-local arguments are opaque pass-through values.

An instance fixes its sources and database path for its entire lifetime. The application never switches source roots at runtime.

`gtk::init()` and `sourceview5::init()` currently run before GApplication registration, so an ordinary second launch pays for a display connection and GtkSourceView before discovering it is remote. This is pre-existing, cheap, and out of scope for #197; it is recorded here so the cost is a known choice rather than an oversight.

### Application action and broker

A process-wide `MessageBroker<AppMsg>` is attached to the root component with `RelmApp::with_broker`. Before `run()`, `main.rs` registers a stateless `gio::SimpleAction` named `open-session` with parameter type `s` on the application.

The action callback has two responsibilities, in this order:

1. Present the sole registered application window immediately, with the same active-window/first-window fallback.
2. Send `AppMsg::OpenExternalSession(id)` through the broker.

Presentation stays in the GIO callback because GtkApplication applies startup notification and Wayland activation-token platform data around the incoming action. Delaying `present()` until the Relm4 message is processed could lose that activation context and allow the compositor to deny focus.

The callback does not open SQLite, inspect navigation widgets, or decide whether the ID is valid.

The broker is initialized by Relm4's `startup` handler before an action is emitted. No pending-action field or early-message fallback is required.

### App navigation handler

`AppMsg::OpenExternalSession(String)` has a dedicated handler rather than reusing `handle_session_selected` unchanged. It must validate the target before mutating navigation state.

The handler performs an exact `load_session(&db_path, &id)` lookup and resolves one of four outcomes:

- `Found(session)` when the row exists and `session.is_subagent` is false;
- `IndexMissing` when the database file does not exist yet;
- `Unavailable` when the ID is empty, absent, or belongs to a subagent;
- `Failed(error)` when SQLite cannot complete the lookup.

`IndexMissing` is checked before the lookup, because `load_session` collapses a missing database file into `Ok(None)` and would otherwise report a freshly installed application as a missing session.

On `Found`, the handler:

1. dismisses the session-summary popover;
2. closes search mode and clears the shared query, transcript highlights, and search-only sort override through the existing search-state path;
3. switches the top-level workspace to Sessions;
4. clears `parent_session`, because the external request has no parent-navigation context;
5. sets the active session and sends `SessionDetailMsg::SetSession` with `search_query: None`;
6. pushes the registered detail page only if it is not already visible.

Assistant, project, pinned, and date filters are list state and remain unchanged. The persisted sort order also remains unchanged; only a temporary FTS relevance override is cleared with the query. Navigating back therefore returns to the user's prior filtered list with no active text search.

Opening the same ID again reloads it and resends `SetSession`, so metadata changed by indexing is refreshed.

On `Unavailable`, the handler adds a translated `Session not found` toast. On `IndexMissing`, it adds a translated `Sessions are not indexed yet` toast, so a first run activated over D-Bus does not claim the session is gone. On `Failed`, it logs the underlying error and adds a translated `Could not open session` toast. All three outcomes preserve the active workspace, active session, navigation stack, parent context, filters, search state, inspector state, and summary popover state. Window presentation has already happened so the feedback is visible.

No retry is scheduled after indexing. A provider result came from the same existing index; an arbitrary stale ID is allowed to fail. SQLite's current WAL and busy timeout remain the only lock-contention policy.

## Internal protocol

The future provider invokes the standard `org.freedesktop.Application.ActivateAction` method on the application App ID:

- action name: `open-session`;
- parameter array: one variant containing a string;
- platform data: the activation data supplied by GNOME Shell/GIO.

The provider must only return top-level IDs, but the application still checks the loaded row. Input is treated as untrusted even though both components ship together. The lookup stays parameterized, and malformed or unexpected strings cannot become SQL.

The action will be discoverable through the session bus and `gapplication list-actions`. Discoverability does not make it a supported third-party API. The name, parameter shape, and underlying ID may change in a release that updates both provider and application.

## Packaging

The generated desktop file adds:

```ini
DBusActivatable=true
```

`Exec=sessions-chronicle` remains as the fallback launch command. There is no `%U`, `MimeType`, or URI handler.

Meson generates and installs one D-Bus service file named after the profile-specific App ID under `share/dbus-1/services`:

```ini
[D-BUS Service]
Name=@APP_ID@
Exec=@BINDIR@/sessions-chronicle --gapplication-service
```

`--gapplication-service` is available under the application's default flags, so the service file needs no counterpart in the application's flag set. Note that service activation still maps a window immediately, because the root window is declared visible in the `view!` macro. Sessions Chronicle is not a lazy background service and #197 does not make it one; a D-Bus activation is a launch.

The filename, `Name`, desktop filename, GApplication ID, and Flatpak ID must match for both profiles:

| Profile | App ID and service basename |
|---|---|
| Stable | `dev.maciz.sessionschronicle` |
| Development | `dev.maciz.sessionschronicle.Devel` |

Flatpak permits an application to own its own App ID on the session bus. #197 therefore adds no main-application `--own-name` or `--talk-name` permission. The Flatpak verification must confirm that the desktop and service files are exported and that host-side activation reaches the sandbox before this assumption is considered proven.

## User-visible behavior

| Starting state | Request | Result |
|---|---|---|
| Application closed | Valid top-level ID | One window starts and opens the session detail |
| Application open on session list | Valid top-level ID | Existing window focuses and pushes detail |
| Application open on another detail | Valid top-level ID | Existing detail is replaced without another window |
| Application open on Analytics | Valid top-level ID | Existing window focuses, switches to Sessions, and opens detail |
| Any state | Unknown/empty/subagent ID | Existing state remains; `Session not found` toast |
| Any state | Database not created yet | Existing state remains; `Sessions are not indexed yet` toast |
| Any state | SQLite lookup failure | Existing state remains; `Could not open session` toast |
| Instance running | Second invocation without override | Existing window focuses |
| Instance running | Second invocation with `--sessions-dir` | A separate fixture-backed instance starts, as it does today; it owns no bus name |
| Fixture instance running | `open-session` request | Reaches the unique default-index instance, never the fixture instance |

The three new toast strings are translatable. `po/POTFILES.in` currently lists `src/app.rs`, a path that disappeared when the app module was split into `src/app/`, so no string under that module is extracted today. #197 repairs that entry and adds the files holding the new strings.

## Testing

### Pure tests

- Uniqueness decision: `--sessions-dir` absent yields default flags, present yields `NON_UNIQUE`.
- Local argument separation: `--gapplication-service` survives the separation pass and is forwarded to GApplication instead of aborting startup. This is the D-Bus activation path, so it is tested rather than assumed.
- Order independence: `--sessions-dir` after a GTK option still yields `NON_UNIQUE`; `--print-db-path` after `--gapplication-service` still exits locally; both `--sessions-dir=DIR` and the split form are accepted; arguments after `--` are forwarded unchanged.
- External target classification: top-level row accepted, subagent row unavailable, missing row unavailable, missing database distinct from missing row, SQLite error distinct from both.
- Successful external navigation requests search clearing without changing persisted sort or list filters.

### GTK tests under Xvfb

Using `tests/fixtures`:

- a valid top-level ID opens detail and sets the expected active session;
- a second valid ID replaces an existing detail without another page push;
- a request from Analytics switches to Sessions;
- a successful request clears an active query and search-only sort override;
- a successful request clears an existing parent context;
- an unknown ID preserves all model navigation and search state;
- a known subagent ID has the same non-destructive result as an unknown ID;
- a request against a database path that does not exist is classified as index-missing, not session-missing;
- the GApplication action accepts one string and reaches the root component through the broker after startup.

Toast creation may be tested through a small outcome/effect boundary if `AdwToastOverlay` does not expose its queue. Tests must not depend on private widget internals solely to inspect toast text.

### Generated metadata tests

- `desktop-file-validate` passes with `DBusActivatable=true`.
- Stable Meson output generates matching stable desktop and service names.
- Development Meson output generates matching `.Devel` desktop and service names.
- The service `Exec` path uses the configured install bindir and includes only `--gapplication-service`.
- `po/POTFILES.in` lists only existing paths, and the files carrying the new toast strings are among them.

### Flatpak acceptance checks

Install the development Flatpak, then verify:

1. With the application closed and an ID from its default index, activating `open-session` starts one window directly on that session.
2. Repeating the action while it runs keeps one process and one window, presents that window, and replaces the detail.
3. An unknown ID preserves the visible page and shows the not-found toast.
4. A subagent ID is rejected the same way.
5. Stable and `.Devel` installations can run concurrently, and each action reaches the matching App ID.
6. A fixture-backed instance started with `--sessions-dir` runs alongside an ordinary instance, exactly as it does today, and each keeps its own database file.
7. While a fixture instance is running, an `open-session` request reaches the ordinary default-index instance and leaves the fixture instance untouched.
8. A second invocation without `--sessions-dir` presents the running default-index instance instead of starting a second process.
9. The exported Flatpak metadata contains the desktop and D-Bus service files, and activation works without an additional bus permission.

Deep-link checks always use the default index, because a fixture instance is `NON_UNIQUE` and owns no bus name. Fixture instances remain the tool for ordinary UI work, not for activation testing.

## Verification before PR

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --all --no-fail-fast
flatpak-builder --user flatpak_app build-aux/dev.maciz.sessionschronicle.Devel.json --force-clean
```

The Flatpak must also be installed for the cold host-to-sandbox D-Bus checks; a build alone does not prove service activation.

## Implementation differences

The implementation preserves the protocol, fail-fast lookup behavior, user-visible messages, and navigation guarantees described above. The following details changed while implementing and verifying the design:

| Area | Planned design | Final implementation | Rationale and impact |
|---|---|---|---|
| Index availability | Distinguish a missing index from a missing row by checking whether the database exists before lookup. | `App::init` captures `index_available` before `SessionIndexer::new` can create the database, and a successful indexing pass sets it to `true`. | The name describes whether an index can be queried, not whether it reflects the latest files on disk. Existing indexed results still open immediately while incremental indexing runs; no retry or deferred opening was added. |
| Lookup outcomes | Classify lookup as `Found`, `IndexMissing`, `Unavailable`, or `Failed`, then map failures to user feedback. | `lookup_external_session` returns `Result<Session, ExternalOpenFailure>`, with the SQLite error retained by `Failed(anyhow::Error)`. | This removes duplicate outcome enums and the boxed success payload without changing classification, logging, or toast text. Unknown, empty, and subagent IDs remain `Unavailable`. |
| Application-action test | Activate `open-session` through the application action map and verify broker delivery. | The GTK unit test looks up the typed `SimpleAction` and activates it directly. Installed-Flatpak acceptance checks exercise the real host-to-sandbox GApplication route. | The test application is not registered on the session bus, so `GApplication::activate_action` rejects activation there. Direct action activation still tests the callback and broker boundary; cold and warm D-Bus routing are verified at the installed-process level. |
| Popover preservation test | Assert that unavailable targets preserve the visible summary popover together with model navigation and search state. | The headless regression asserts the model, search, inspector, detail, parent, and navigation-stack state; popover preservation was verified during installed-Flatpak acceptance instead. | Under Xvfb, iterating the main context unmaps the popover because the test GApplication has not emitted normal startup, even for a no-op handler. Keeping that assertion would test the harness artifact rather than the failure path. |

## Success criteria

- A provider result opens the exact indexed top-level session from cold and warm application states.
- Warm activation focuses the existing window and creates no second process or window.
- Invalid, stale, and subagent IDs never replace or clear the user's current view.
- An application whose index does not exist yet says so instead of reporting a missing session.
- Search state is cleared only after a target has been validated successfully.
- Stable and development profiles own distinct application bus names.
- The `--sessions-dir` development workflow keeps working unchanged, including a fixture instance running beside an ordinary one.
- The behavior works in the installed Flatpak without extra D-Bus permissions.
- #189 can depend on this protocol without requiring a URI or public CLI.

## References

- [Gio.Application](https://docs.gtk.org/gio/class.Application.html)
- [Gio.ApplicationFlags](https://docs.gtk.org/gio/flags.ApplicationFlags.html)
- [Using GtkApplication](https://developer.gnome.org/documentation/tutorials/application.html)
- [Desktop Entry Specification — D-Bus Activation](https://specifications.freedesktop.org/desktop-entry-spec/latest/dbus.html)
- [Flatpak Requirements & Conventions](https://docs.flatpak.org/en/latest/conventions.html)
- Relm4 0.11.0 source: `RelmApp::run`, `RelmApp::from_app`, `RelmApp::allow_multiple_instances`, `main_application`, and `MessageBroker`
- GLib source: `g_application_real_local_command_line` in `gio/gapplication.c`, for the conditions under which `--gapplication-service` is registered
- [`docs/DEVELOPMENT_WORKFLOW.md`](../../DEVELOPMENT_WORKFLOW.md) — the `--sessions-dir` fixture workflow this design must preserve
