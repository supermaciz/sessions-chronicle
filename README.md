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
- Sidebar filters by project and assistant with keyword search
- Markdown rendering (code blocks, tables, task lists, blockquotes)
- Expand inline tool calls and drill down into subagents; consecutive calls are grouped and collapsible
- Pin favorite sessions
- Per-session token breakdowns in detail view
- Assistant status dots, issue banner, empty-state diagnostics, and detailed indexing status dialog
- Launch sessions directly from the app in your terminal
- Supports 4 AI assistants — Claude Code, OpenCode, Codex, Mistral Vibe
- View most-used assistants, most-active days, and more


## Installation

1. Install the self-hosted Flatpak remote:

```bash
flatpak install --user https://sessions-chronicle.maciz.dev/flatpak/dev.maciz.sessionschronicle.flatpakref
```

2. Launch **Sessions Chronicle** from your app menu, or run:

```bash
flatpak run dev.maciz.sessionschronicle
```

If you prefer a standalone bundle, download the latest `.flatpak` file from the [Releases page](https://github.com/supermaciz/sessions-chronicle/releases) and install it with `flatpak install ./sessions-chronicle-<version>.flatpak`.


## Screenshots

**Browse and search your sessions**  
<img src="docs/screenshots/session_list.png" alt="Session List" width="800"/>

**Read conversations with full markdown rendering and tool call inspection**  
<img src="docs/screenshots/session_detail.png" alt="Session Detail" width="800"/>

**Analytics**  
<img src="docs/screenshots/analytics.png" alt="Analytics Dashboard" width="800"/>


## Inspiration

Inspired by [agent-sessions](https://github.com/jazzyalex/agent-sessions).


## License

Licensed under MIT. See [LICENSE](LICENSE).
