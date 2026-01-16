# Session Resumption + Preferences (Detailed Plan)

## Goals
- Let users choose which terminal emulator is used to resume sessions.
- Follow GNOME HIG using `AdwPreferencesDialog` (newer API, replaces `AdwPreferencesWindow`).
- Provide "Resume in Terminal" entry points in both:
  - Session detail view
  - Session list rows
- Work in Flatpak (host terminal launch).

## UX (GNOME HIG)

### Preferences
- Dialog: `AdwPreferencesDialog`
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

### Recommended Implementation Order
1. `src/utils/terminal.rs` - Self-contained, testable first
2. GSettings schema - Add `resume-terminal` key
3. `src/ui/modals/preferences.rs` - Wire to `PreferencesAction`
4. `src/ui/session_list.rs` - Add resume button + `ResumeRequested` output
5. `src/ui/session_detail.rs` - Add resume button + output to parent
6. `src/app.rs` - Handle `ResumeSession`, spawn terminal, error dialogs

### 1) Preferences dialog component
- Create a new modal component:
  - `src/ui/modals/preferences.rs`
  - Export in `src/ui/modals/mod.rs`
- Wire existing menu action:
  - `PreferencesAction` already exists in `src/app.rs`
  - Add a stateless action handler to launch the preferences dialog.

Relm4 pattern (follow `src/ui/modals/about.rs`):
```rust
impl SimpleComponent for PreferencesDialog {
    type Init = ();
    type Root = adw::PreferencesDialog;
    type Widgets = PreferencesWidgets;  // Custom struct to hold ComboRow reference
    type Input = PreferencesMsg;
    type Output = ();  // No output needed back to parent

    fn init_root() -> Self::Root {
        adw::PreferencesDialog::builder().build()
    }

    fn init(...) {
        // Build UI: PreferencesPage > PreferencesGroup > ComboRow
        // Read GSettings, set ComboRow selection
        // Connect notify::selected signal
        widgets.present(Some(&relm4::main_application().windows()[0]));
    }
}
```

Preferences UI behavior:
- On init:
  - Read `resume-terminal` from GSettings
  - Set ComboRow selection accordingly
- On selection change (use `connect_notify_selected`):
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

#### Session list (`src/ui/session_list.rs`)
- Add a suffix "resume" button per row using `row.add_suffix()`:
  ```rust
  let resume_btn = gtk::Button::from_icon_name("utilities-terminal-symbolic");
  resume_btn.add_css_class("flat");
  row.add_suffix(&resume_btn);
  ```
- Add new output variant:
  ```rust
  pub enum SessionListOutput {
      SessionSelected(String),
      ResumeRequested(String),  // NEW
  }
  ```

#### App (`src/app.rs`)
- Update `forward()` to handle new output:
  ```rust
  SessionList::builder()
      .launch(db_path.clone())
      .forward(sender.input_sender(), |msg| match msg {
          SessionListOutput::SessionSelected(id) => AppMsg::SessionSelected(id),
          SessionListOutput::ResumeRequested(id) => AppMsg::ResumeSession(id),  // NEW
      });
  ```
- Add new message variant and handler:
  ```rust
  pub enum AppMsg {
      // ... existing variants
      ResumeSession(String),  // NEW
  }
  ```
- In `update()`, handle `ResumeSession`:
  - Load session from DB (`load_session`)
  - Determine working directory
  - Spawn terminal + command via `utils::terminal`
  - Show `AdwAlertDialog` on error

#### Session detail (`src/ui/session_detail.rs`)
- Add a "Resume" button in metadata_box.
- Add output type (currently uses `detach()` with no output):
  ```rust
  pub enum SessionDetailOutput {
      ResumeRequested(String),
  }
  ```
- Change from `detach()` to `forward()` in `app.rs`:
  ```rust
  // Before:
  let session_detail = SessionDetail::builder().launch(db_path.clone()).detach();

  // After:
  let session_detail = SessionDetail::builder()
      .launch(db_path.clone())
      .forward(sender.input_sender(), |msg| match msg {
          SessionDetailOutput::ResumeRequested(id) => AppMsg::ResumeSession(id),
      });
  ```

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
