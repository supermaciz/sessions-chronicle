# Session Resumption + Preferences (Detailed Plan)

## Goals
- Let users choose which terminal emulator is used to resume sessions.
- Follow GNOME HIG using `AdwPreferencesWindow`.
- Provide “Resume in Terminal” entry points in both:
  - Session detail view
  - Session list rows
- Work in Flatpak (host terminal launch).

## UX (GNOME HIG)

### Preferences
- Window: `AdwPreferencesWindow`
- Page: “General”
- Group: “Session Resumption”
- Row: “Terminal” (`AdwComboRow`)
  - Items (always shown):
    - Automatic (recommended)
    - Ptyxis
    - GNOME Terminal
    - Ghostty
    - Foot
    - Alacritty
    - Kitty
  - Detection order for “Automatic”: GNOME first
    1. `ptyxis`
    2. `gnome-terminal`
    3. `ghostty`
    4. `foot`
    5. `alacritty`
    6. `kitty`
  - Behavior:
    - “Automatic” picks the first installed terminal in the list above.
    - Explicit choices force that terminal (even if others exist).
    - If forced terminal is not available, show an error dialog with guidance to change preferences.

### Resume entry points
- Session detail:
  - Add a “Resume” button near the metadata header.
- Session list:
  - Add a suffix icon button per row (terminal icon) to “Resume”.
  - Row activation continues to open detail view.

## GSettings / Schema
- Add key: `resume-terminal` (type `s`, default `auto`)
- Allowed values:
  - `auto`
  - `ptyxis`
  - `gnome-terminal`
  - `ghostty`
  - `foot`
  - `alacritty`
  - `kitty`

Files:
- Schema: `data/io.github.supermaciz.sessionschronicle.gschema.xml.in`
- Access: `gio::Settings::new(APP_ID)` (already used in `src/app.rs`)

## Implementation Plan (Code)

### 1) Preferences window component
- Create a new modal component:
  - `src/ui/modals/preferences.rs`
  - Export in `src/ui/modals/mod.rs`
- Wire existing menu action:
  - `PreferencesAction` already exists in `src/app.rs`
  - Add a stateless action handler to launch the preferences window.

Preferences UI behavior:
- On init:
  - Read `resume-terminal` from GSettings
  - Set ComboRow selection accordingly
- On selection change:
  - Write updated string back to GSettings

### 2) Terminal abstraction
- Create:
  - `src/utils/mod.rs`
  - `src/utils/terminal.rs`

Responsibilities:
- Parse terminal preference from settings string
- Detect installed terminal for `auto` (GNOME-first order)
- Build launch command for chosen terminal (host vs sandbox)
- Build safe “resume command” for a session

### 3) Resume command (Claude Code only for now)
Command template:
- `claude -r {id}`

Security/safety:
- Avoid shell injection; never interpolate `{id}` unquoted in a shell string.
- Prefer passing arguments separately.
- If we need `cd` + keep terminal open, use:
  - `bash -lc 'cd "$1" && claude -r "$2"; exec bash' -- <workdir> <id>`
  - Here `<id>` is passed as an argument, not interpolated.

Workdir:
- Use `session.project_path` if present; otherwise fallback to directory of `session.file_path`.

### 4) Flatpak support (host launching)
- If running under Flatpak:
  - Use `flatpak-spawn --host ...` to launch the terminal on the host.
- Detect Flatpak:
  - Check for `/.flatpak-info` existence OR `FLATPAK_ID` env var.

If not Flatpak:
- Spawn terminal directly.

### 5) UI message plumbing
- Session list (`src/ui/session_list.rs`):
  - Add a suffix “resume” button per row.
  - Emit new output: `SessionListOutput::ResumeRequested(session_id)`.
- App (`src/app.rs`):
  - Handle `ResumeRequested` output:
    - Load session from DB (`load_session`)
    - Determine working directory
    - Spawn terminal + command
    - Show `AdwAlertDialog` on error
- Session detail (`src/ui/session_detail.rs`):
  - Add a “Resume” button.
  - Emit a message to request resumption for current session id.
  - Either:
    - bubble to App, or
    - spawn directly (App-centralized is preferred for consistent dialogs + settings access).

## Errors & Dialogs
- Use `AdwAlertDialog` for:
  - “Terminal not found”
  - “Resume command failed to start”
  - “Session missing project path and file path invalid”
- Include a “Open Preferences” button if possible (nice-to-have).

## Validation
- `cargo fmt --all`
- `cargo test`
- Add unit tests around command-building:
  - “auto resolves to first available”
  - “forced terminal builds expected argv”
  - “flatpak mode wraps command in flatpak-spawn --host”
  - No tests should actually spawn processes.
