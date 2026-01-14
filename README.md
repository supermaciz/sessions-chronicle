# Sessions Chronicle

A GNOME app for browsing, searching, and resuming AI coding sessions.

## What does it contain?

- Parse Claude Code session files (JSONL format)
- SQLite database with full-text search (FTS5)
- Basic UI with sidebar and session list
- Local data only (no telemetry)

### Planned Features
- Search across all sessions
- Session detail view with conversation history
- Resume sessions in terminal
- Support for OpenCode and Codex formats
- Real-time session monitoring

## Building the project

Make sure you have `flatpak` and `flatpak-builder` installed. Then run:

```bash
flatpak install --user org.gnome.Sdk//49 org.gnome.Platform//49 org.freedesktop.Sdk.Extension.rust-stable//25.08 org.freedesktop.Sdk.Extension.llvm21//25.08
flatpak-builder --user flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json --force-clean
```

## Running the project

```bash
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle
```

## Inspiration

This project was inspired by [agent-sessions](https://github.com/jazzyalex/agent-sessions).

## License

See LICENSE file.
