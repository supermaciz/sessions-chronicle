# Open Session by ID — Design Spec

**Date:** 2026-08-01  
**Status:** Approved design for [issue #197](https://github.com/supermaciz/sessions-chronicle/issues/197)  
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
- Implementing or packaging the search provider from #189.
- Changing the database schema, session IDs, indexing policy, or source path resolution.

## Ground facts

- `src/main.rs` currently obtains Relm4's default `adw::Application` through `main_application()` without assigning `APP_ID`. GApplication uniqueness and D-Bus export are therefore disabled.
- `RelmApp::run` launches the root component from GApplication's `startup` signal. GApplication emits `startup` before the `activate`, `open`, `command-line`, or action entry point that caused a cold start.
- A GApplication with an application ID owns that well-known name on the session bus and routes application actions to the primary instance.
- The existing `AppMsg::SessionSelected` path already loads a session by exact ID and drives `SessionDetail`, but its missing-session branch opens an empty detail page and provides no user feedback. External activation needs a non-destructive failure path.
- The search entry is shared by session-list filtering and transcript highlighting. Carrying an unrelated in-app query into an externally opened result would show stale context.
- `sessions.id` is a `TEXT PRIMARY KEY`; `load_session` binds it as a SQLite parameter. The row ID is stable across ordinary reindexing.
- `SessionIndexer::new` enables WAL, and every connection created through `open_connection` has a five-second busy timeout. A cold-start read can therefore coexist with incremental indexing without a new concurrency mechanism in #197.
- Meson already derives `APP_ID` as `dev.maciz.sessionschronicle` for stable builds and `dev.maciz.sessionschronicle.Devel` for development builds.

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
| SQLite failure | Preserve state, show a generic failure toast, log details |
| Active search on success | Clear search mode, query, transcript highlights, and search-only sort override |
| Existing filters | Preserve assistant, project, date, pinned, and sort settings |
| Existing parent context | Clear it before showing the externally selected top-level session |
| `LaunchSearch` | Deferred to #189 or a follow-up |
| `--sessions-dir` in a remote invocation | Reject explicitly with stderr and a non-zero status |
| Packaging | D-Bus service file plus `DBusActivatable=true` |
| Additional D-Bus permission | None expected for the application's own App ID; verify in Flatpak |

## Alternatives considered

### Public command line

`sessions-chronicle --session <id>` would be convenient for scripts and manual testing. It would also make the command and the ID a compatibility promise, require command-line forwarding to the primary instance, and force definitions for Flatpak invocation and option combinations. None of that improves the #189 result-activation path, so it is deferred until a concrete third-party use case exists.

### URI through `org.freedesktop.Application.Open`

A `sessions-chronicle://session/<id>` URI would work with `gio open` and clickable links. It would require a URI grammar, escaping rules, desktop handler registration, and long-term compatibility. More importantly, the ID only names a row in one local index, so the URI would look shareable without being portable across machines, profiles, or deleted sessions. The provider can activate an application action directly, so #197 does not introduce a URI.

### Custom D-Bus interface

A dedicated `OpenSession(s)` method would duplicate functionality already supplied by GApplication's exported action group and add custom introspection and registration code. The standard application action has the required cold-start and primary-instance semantics.

## Architecture

### GApplication setup

`src/main.rs` will construct the `adw::Application` with the generated `APP_ID` before giving it to `RelmApp::from_app`. Default GApplication uniqueness remains enabled. Stable and development builds use different IDs and may run side by side.

The application will add `gio::ApplicationFlags::HANDLES_COMMAND_LINE`. This flag is not the deep-link transport. It exists only so a remote invocation that contains `--sessions-dir` can receive an explicit error and exit status instead of silently discarding its override payload.

The application's normal `activate` handler presents its sole registered window, using `active_window()` when available and otherwise the first application window. This handles desktop launches and ordinary second invocations without depending on Relm4's `set_visible`-only activation behavior.

The existing local Clap pass remains responsible for:

- resolving the first instance's `sessions_dir` payload;
- handling `--print-db-path` locally and exiting before GApplication starts;
- separating options that GApplication/GTK must still receive.

`sessions-dir` will also be registered as a GApplication main option so its presence is available through `ApplicationCommandLine::options_dict()` in the primary process. The argument vector passed to GApplication will include the locally parsed override when present.

The command-line handler follows this table:

| Invocation | Result |
|---|---|
| Primary, no `--sessions-dir` | Activate and present the new application |
| Primary, with `--sessions-dir` | The startup payload fixes the sources; activate and present |
| Remote, no `--sessions-dir` | Activate and present the existing primary instance; return success |
| Remote, with `--sessions-dir` | Print an explanatory error through `ApplicationCommandLine`, return non-zero, do not present or mutate the primary instance |

The first instance fixes its sources and database path for its entire lifetime. The application never switches source roots at runtime.

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

The handler performs an exact `load_session(&db_path, &id)` lookup and resolves one of three outcomes:

- `Found(session)` when the row exists and `session.is_subagent` is false;
- `Unavailable` when the ID is empty, absent, or belongs to a subagent;
- `Failed(error)` when SQLite cannot complete the lookup.

On `Found`, the handler:

1. dismisses the session-summary popover;
2. closes search mode and clears the shared query, transcript highlights, and search-only sort override through the existing search-state path;
3. switches the top-level workspace to Sessions;
4. clears `parent_session`, because the external request has no parent-navigation context;
5. sets the active session and sends `SessionDetailMsg::SetSession` with `search_query: None`;
6. pushes the registered detail page only if it is not already visible.

Assistant, project, pinned, and date filters are list state and remain unchanged. The persisted sort order also remains unchanged; only a temporary FTS relevance override is cleared with the query. Navigating back therefore returns to the user's prior filtered list with no active text search.

Opening the same ID again reloads it and resends `SetSession`, so metadata changed by indexing is refreshed.

On `Unavailable`, the handler adds a translated `Session not found` toast. On `Failed`, it logs the underlying error and adds a translated `Could not open session` toast. Both outcomes preserve the active workspace, active session, navigation stack, parent context, filters, search state, inspector state, and summary popover state. Window presentation has already happened so the feedback is visible.

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
| Any state | SQLite lookup failure | Existing state remains; `Could not open session` toast |
| Primary instance active | Second invocation without override | Existing window focuses |
| Primary instance active | Second invocation with `--sessions-dir` | Calling process fails explicitly; existing app is untouched |

The two new toast strings are translatable, and their source file is added to `po/POTFILES.in`.

## Testing

### Pure tests

- Command-line decision table: primary/remote crossed with `sessions-dir` absent/present.
- External target classification: top-level row accepted, subagent row unavailable, missing row unavailable, SQLite error distinct.
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
- the GApplication action accepts one string and reaches the root component through the broker after startup.

Toast creation may be tested through a small outcome/effect boundary if `AdwToastOverlay` does not expose its queue. Tests must not depend on private widget internals solely to inspect toast text.

### Generated metadata tests

- `desktop-file-validate` passes with `DBusActivatable=true`.
- Stable Meson output generates matching stable desktop and service names.
- Development Meson output generates matching `.Devel` desktop and service names.
- The service `Exec` path uses the configured install bindir and includes only `--gapplication-service`.

### Flatpak acceptance checks

Install the development Flatpak, then verify:

1. With the application closed and an ID from its default index, activating `open-session` starts one window directly on that session.
2. Repeating the action while it runs keeps one process and one window, presents that window, and replaces the detail.
3. An unknown ID preserves the visible page and shows the not-found toast.
4. A subagent ID is rejected the same way.
5. Stable and `.Devel` installations can run concurrently, and each action reaches the matching App ID.
6. While a fixture-backed development instance is running, a second invocation with `--sessions-dir` returns non-zero and explains that the active instance fixes the source directory.
7. A second invocation without `--sessions-dir` presents the fixture-backed instance.
8. The exported Flatpak metadata contains the desktop and D-Bus service files, and activation works without an additional bus permission.

The fixture override is not persisted. A cold D-Bus activation therefore uses the normal default index; warm action tests can use a fixture-backed instance.

## Verification before PR

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --all --no-fail-fast
flatpak-builder --user flatpak_app build-aux/dev.maciz.sessionschronicle.Devel.json --force-clean
```

The Flatpak must also be installed for the cold host-to-sandbox D-Bus checks; a build alone does not prove service activation.

## Success criteria

- A provider result opens the exact indexed top-level session from cold and warm application states.
- Warm activation focuses the existing window and creates no second process or window.
- Invalid, stale, and subagent IDs never replace or clear the user's current view.
- Search state is cleared only after a target has been validated successfully.
- Stable and development profiles own distinct application bus names.
- The behavior works in the installed Flatpak without extra D-Bus permissions.
- #189 can depend on this protocol without requiring a URI or public CLI.

## References

- [Gio.Application](https://docs.gtk.org/gio/class.Application.html)
- [Gio.ApplicationFlags](https://docs.gtk.org/gio/flags.ApplicationFlags.html)
- [Using GtkApplication](https://developer.gnome.org/documentation/tutorials/application.html)
- [Desktop Entry Specification — D-Bus Activation](https://specifications.freedesktop.org/desktop-entry-spec/latest/dbus.html)
- [Flatpak Requirements & Conventions](https://docs.flatpak.org/en/latest/conventions.html)
- Relm4 0.11.0 source: `RelmApp::run`, `main_application`, and `MessageBroker`
