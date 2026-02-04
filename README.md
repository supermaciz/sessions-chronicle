<img src="data/icons/io.github.supermaciz.sessionschronicle.svg" alt="App Icon" width="80" height="80" align="left"/>

# Sessions Chronicle
<br clear="left"/>

**A GNOME app for browsing, searching, and resuming AI coding sessions.**


## Features

- Parse Claude Code session files (JSONL format)
- SQLite database with full-text search (FTS5)
- Browse and search sessions with filters
- Session detail view with conversation history
- Local data only (no telemetry)
- Resume sessions in terminal
- Support for multiple AI tools: Claude Code, OpenCode, Codex

## Screenshots

**Session List View**  
<img src="docs/screenshots/session_list.png" alt="Session List" width="800"/>

**Session Detail View**  
<img src="docs/screenshots/session_detail.png" alt="Session Detail" width="800"/>

## Prerequisites

- `flatpak`
- `flatpak-builder`

Install via your package manager (e.g., `apt install flatpak flatpak-builder`).

## Building the project

Install the required SDKs, then build:

```bash
flatpak install --user org.gnome.Sdk//49 org.gnome.Platform//49 org.freedesktop.Sdk.Extension.rust-stable//25.08 org.freedesktop.Sdk.Extension.llvm21//25.08
flatpak-builder --user flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json --force-clean
```

## Running the project

```bash
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle
```

## Testing

```bash
cargo test
```

## Inspiration

This project was inspired by [agent-sessions](https://github.com/jazzyalex/agent-sessions).

## License

See LICENSE file.
