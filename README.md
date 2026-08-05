<img src="data/icons/dev.maciz.sessionschronicle.svg" alt="App Icon" width="80" height="80" align="left"/>

# Sessions Chronicle
<br clear="left"/>

[![CI](https://github.com/supermaciz/sessions-chronicle/actions/workflows/ci.yml/badge.svg)](https://github.com/supermaciz/sessions-chronicle/actions/workflows/ci.yml) [![codecov](https://codecov.io/gh/supermaciz/sessions-chronicle/graph/badge.svg)](https://codecov.io/gh/supermaciz/sessions-chronicle)

**Browse, search, and resume your AI assistant sessions**

Sessions Chronicle indexes all your local AI assistant sessions into a searchable database,
so you can find any conversation, inspect tool calls, diagnose source indexing issues,
and pick up where you left off.

→ Read the [User Guide](https://sessions-chronicle.maciz.dev/guide) to get started.


## Features

- Full-text search across all conversations (SQLite FTS5)
- Search your sessions straight from the GNOME Activities overview
- Sidebar filters by project and assistant with keyword search
- Filter the session list by date (Today, Yesterday, or a custom range)
- Markdown rendering (code blocks, tables, task lists, blockquotes)
- Expand inline tool calls and drill down into subagents; consecutive calls are grouped and collapsible
- Pin favorite sessions
- Per-session token breakdowns in detail view
- Assistant status dots, issue banner, empty-state diagnostics, and detailed indexing status dialog
- Launch sessions directly from the app in your terminal
- Supports 5 AI assistants — Claude Code, OpenCode, Codex, Mistral Vibe, and Kimi Code
- View most-used assistants, most-active days, and more

The GNOME search provider is off by default: enable Sessions Chronicle in
**Settings ▸ Search**, then type at least three characters in the Activities
overview. Results show the session's first prompt; matching transcript excerpts
stay hidden unless you turn them on in **Preferences ▸ System Search**. See the
[User Guide](https://sessions-chronicle.maciz.dev/guide#gnome-search) for details.

Kimi Code support covers current sessions under `$KIMI_CODE_HOME` (default
`~/.kimi-code`) when that location is visible in the Flatpak sandbox. Legacy
sessions under `~/.kimi` are not parsed.


## Installation

1. Install the self-hosted Flatpak remote:

```bash
flatpak install --user https://sessions-chronicle.maciz.dev/flatpak/dev.maciz.sessionschronicle.flatpakref
```

2. Launch **Sessions Chronicle** from your app menu, or run:

```bash
flatpak run dev.maciz.sessionschronicle
```

The Flatpak remote is the recommended install path: it is signed and updates automatically with `flatpak update`.


## Build from source (native)

Contributors and advanced users can build a native Linux binary with Meson instead of Flatpak. This produces a regular GTK / Libadwaita application that links against your system libraries, so you need the development packages for them.

**Build dependencies**

- Rust toolchain (`cargo`) — Rust 2024 edition
- Meson `>= 0.59` and Ninja
- GLib / GIO `>= 2.84`
- GTK 4 `>= 4.18`
- GtkSourceView 5 `>= 5.0`
- Libadwaita `>= 1.8`

These minimums are set by the Rust bindings (the `gnome_48` GTK feature requires GTK 4.18 / GLib 2.84, and Libadwaita 1.8 APIs are used), and match the GNOME 49 runtime the Flatpak build targets — not the lower floors declared in `meson.build`.

On Fedora, the system packages are roughly:

```bash
sudo dnf install cargo meson ninja-build \
  glib2-devel gtk4-devel gtksourceview5-devel libadwaita-devel
```

On Debian/Ubuntu, the equivalents are `cargo meson ninja-build libglib2.0-dev libgtk-4-dev libgtksourceview-5-dev libadwaita-1-dev`.

**Configure, compile, and install**

```bash
meson setup builddir --prefix="$HOME/.local"
meson compile -C builddir
meson install -C builddir
```

Then run the installed build:

```bash
"$HOME/.local/bin/sessions-chronicle"
```

For the faster day-to-day development loop (development profile, incremental rebuilds) see [`docs/DEVELOPMENT_WORKFLOW.md`](docs/DEVELOPMENT_WORKFLOW.md).


## Screenshot

<img src="website/src/assets/screenshots/session_list.png" alt="Session List" width="800"/>

See the website for more screenshots and project details: https://sessions-chronicle.maciz.dev/


## Inspiration

Inspired by [agent-sessions](https://github.com/jazzyalex/agent-sessions).


## License

Licensed under MIT. See [LICENSE](LICENSE).
