# Indexing Status Dialog — Dedicated AdwDialog

**Issue:** [#94](https://github.com/supermaciz/sessions-chronicle/issues/94)  
**Date:** 2026-03-25  
**Status:** Implemented [#96](https://github.com/supermaciz/sessions-chronicle/pull/96)  
**Type:** Design plan  
**Exploration:** `docs/explorations/2026-03-23-indexing-diagnostics-exploration.md` — Proposal B  
**Prerequisite:** Proposal C implemented in [#95](https://github.com/supermaciz/sessions-chronicle/pull/95) (`docs/explorations/2026-03-24-indexing-diagnostics-design.md`)

## Problem

Proposal C (PR #95) solved proactive notification: `AdwBanner` for persistent warnings, enriched toasts, sidebar status dots, and enhanced empty state. But users still have no persistent inspection surface to understand *what* went wrong, *where*, and to take corrective action. The banner says "there are issues" but cannot show details.

## Scope

This design implements Proposal B from the exploration: a dedicated `AdwDialog` for indexing diagnostics, accessible from the hamburger menu, keyboard shortcut, and the banner "Details" button.

**Two phases:**

- **Phase 1:** Dialog shell, summary, source cards, entry points, "Re-index All". Zero backend changes — reuses `PerSourceResult` data from Proposal C.
- **Phase 2:** Error log section with individual error messages. Requires new `IndexingError` struct and collection in the indexer.

**Explicitly out of scope:**

- Per-source re-index (requires `StartSourceReindex` worker variant)
- Error history persistence (all data is in-memory, recomputed each run)
- Clickable file paths in error log (future: open in file manager)

---

## 1. Shell & Layout

### Widget

`AdwDialog` with `content_width: 480`, `content_height: 520`, `follows_content_size: true`. Title: "Indexing Status".

### Header bar

A single "Re-index All" button with `.suggested-action`. During indexing: the button label changes to "Indexing..." and the button is set insensitive. The button content is replaced with a small composite child (`gtk::Box` or equivalent) containing a `GtkSpinner` and label, rather than relying on `adw::ButtonContent` for a live spinner. No custom close button — `AdwDialog` provides one natively.

### Content structure

`AdwToolbarView` > `GtkScrolledWindow` > `AdwClamp(maximum_size: 440)` > `GtkBox(vertical, spacing: 24)`.

All sections live in this single scrollable column. Pattern reference: GNOME Disks SMART dialog.

---

## 2. Summary Section

An `AdwPreferencesGroup` (no title) containing one `AdwActionRow`:

- **Title:** session count or status text
- **Subtitle:** completion status or `None`
- **Prefix:** symbolic icon

During indexing, a `GtkProgressBar` in pulse mode is added below the group as a separate widget.

### States

| State | Title | Subtitle | Prefix icon |
|---|---|---|---|
| Before first indexing | "Not yet indexed" | `None` | `content-loading-symbolic` |
| Indexing in progress | "Indexing in progress..." | `None` | `content-loading-symbolic` |
| Success | "N sessions indexed" | "Completed successfully" | `emblem-ok-symbolic` |
| Partial | "N sessions indexed" | "Completed with E errors" | `dialog-warning-symbolic` |
| Empty sources | "No sessions found" | "Session sources detected, but no sessions were found" | `dialog-warning-symbolic` |
| No detected sources | "No sessions found" | "No session sources detected" | `dialog-warning-symbolic` |

State derivation rules:

- **No detected sources** applies only when every `PerSourceResult` is `NotFound`.
- **Empty sources** applies when at least one source is available (`Empty`) but no sessions were indexed or skipped.
- `Empty` and `NotFound` intentionally remain distinct in the UI: both use grey styling, but `Empty` keeps the resolved path visible while `NotFound` uses the fallback text "Source not found".

---

## 3. Source Cards

### Container

`AdwPreferencesGroup` with title "Sources".

### One `AdwExpanderRow` per AI assistant

- **Title:** assistant display name (e.g. "Claude Code")
- **Subtitle:** `display_path` in monospace via Pango markup (`<tt>...</tt>`), `subtitle_lines: 1`, ellipsized. If `NotFound`: "Source not found"
- **Suffix:** `GtkLabel` styled as a pill badge with session count. Color by status:
  - `Indexed` → green (`source-status-ok`)
  - `Degraded` / `Failed` → orange (`source-status-degraded`)
  - `NotFound` / `Empty` → grey (`source-status-not-found`)
- **`enable_expansion`:** `false` when `NotFound`; `true` for `Empty` so the user can still inspect and copy the resolved path

### Expanded children

Each child is an `AdwActionRow`:

| Row | Title | Suffix |
|---|---|---|
| Source Path | "Source Path" | monospace label with full path + copy button (`edit-copy-symbolic`). Copy uses `gdk::Display::clipboard().set_text()` and shows an `AdwToast` "Path copied to clipboard" (3s). |
| Sessions Indexed | "Sessions Indexed" | count label |
| Skipped | "Skipped" | count label (row hidden if 0) |
| Parse Errors | "Parse Errors" | count label, `@warning_color` if > 0 (row hidden if 0) |

### Before first indexing

When `last_per_source` is empty (dialog opened before first indexing completes), the Sources group is hidden entirely. Only the summary row ("Not yet indexed") is visible. Source cards appear once the first `IndexingCompleted` delivers data.

### Ordering

Fixed order matching the sidebar: Claude Code, OpenCode, Codex, Mistral Vibe. Sources with `NotFound` status appear last, grouped at the bottom.

`Empty` sources stay in the normal assistant order. They are available sources with zero sessions, not missing sources.

### No per-source re-index

Deferred. The backend only supports `StartFullReindex`. The header bar "Re-index All" is sufficient for Phase 1.

---

## 4. Entry Points

Three entry points, all triggering `AppMsg::ShowIndexingStatus`:

### Hamburger menu

New item "Indexing Status..." in the existing section, above "Preferences".

```rust
new_stateless_action!(IndexingStatusAction, WindowActionGroup, "indexing-status");
```

Accelerator: `Ctrl+Shift+I`.

### Banner "Details" button

When the banner is revealed in `handle_indexing_completed`:

```rust
self.banner.set_button_label(Some("Details"));
```

When hidden:

```rust
self.banner.set_button_label(None);
```

The `banner.connect_button_clicked` signal is connected once during init to send `AppMsg::ShowIndexingStatus`.

### Keyboard shortcut

`Ctrl+Shift+I` registered via `app.set_accelerators_for_action::<IndexingStatusAction>`. Added to the `ShortcutsDialog` under the "General" section.

### No toast "Details" button

The toast is ephemeral (5s) and appears simultaneously with the banner. Having "Details" on both is redundant. The banner is the persistent entry point.

---

## 5. Component Lifecycle & Data Flow

### Lazy creation

The dialog controller is `Option<Controller<IndexingStatusDialog>>` on the `App` model. Created on first `ShowIndexingStatus`, reused after. Unlike `PreferencesDialog` (which is created eagerly at init), lazy creation is preferred here because the dialog is rarely opened and should not add to startup cost.

### Stored state on App

New field:

```rust
last_per_source: Vec<PerSourceResult>,
```

Updated on every `IndexingCompleted`. Feeds the dialog on open and during live updates.

This state is also the source of truth for distinguishing "not yet indexed" (`last_per_source.is_empty()`) from "indexed and empty" (non-empty vec whose statuses are `Empty`/`NotFound` only).

### Dialog messages

```rust
pub enum IndexingStatusMsg {
    /// Data update from App
    Update {
        per_source: Vec<PerSourceResult>,
        indexing: bool,
    },
    /// User clicked "Re-index All"
    ReindexRequested,
}

pub enum IndexingStatusOutput {
    /// Bubbled to App to trigger re-index
    Reindex,
}
```

### Flow

1. **Open:** `AppMsg::ShowIndexingStatus` → create controller if absent → send `IndexingStatusMsg::Update` with `last_per_source` + `self.indexing` → `dialog.present()`
2. **Indexing starts:** app startup or `handle_reindex_requested` sets `self.indexing = true` → if dialog exists, send `Update { indexing: true, ... }`
3. **Indexing completes:** `handle_indexing_completed` → store `last_per_source` → if dialog exists, send `Update { per_source, indexing: false }`
4. **Re-index:** dialog emits `IndexingStatusOutput::Reindex` → App receives → triggers `indexing_worker.emit(StartFullReindex)`

### No persistence

Zero database storage. All data lives in memory, recomputed on each indexing run.

---

## 6. Phase 2: Error Log

### Backend: new structures

In `src/models/indexing_diagnostics.rs`:

```rust
#[derive(Debug, Clone)]
pub struct IndexingError {
    pub assistant: AiAssistant,
    pub location: Option<String>, // basename, relative path, or source-level location
    pub message: String,        // parse/IO error description
}
```

`IndexingError` belongs in `models`, alongside `PerSourceResult`, because it is worker output and app/UI state rather than worker-local implementation detail.

`IndexingWorkerOutput::Completed` gains a field:

```rust
errors_detail: Vec<IndexingError>,  // capped at 50 entries
```

### Indexer changes

Existing `tracing::warn!` calls in per-session indexing loops (`index_claude_sessions_internal`, etc.) additionally collect errors into a `Vec<IndexingError>`, truncated to 50. This vec propagates via `IndexingWorkerOutput::Completed` up to the App model.

The collection logic should handle both:

- **file/session-scoped errors** (`location: Some("rollout-123.jsonl".into())`)
- **source-level errors** such as SQLite enumeration/open failures (`location: Some(db_path.display().to_string())` or `None` when no stable location is available)

### UI: "Recent Errors" section

`AdwPreferencesGroup` with title "Recent Errors". Visible only when `errors_detail` is non-empty.

- Up to 10 `AdwActionRow` entries:
  - **Title:** `location` basename when present (`session_abc123.jsonl`), otherwise assistant display name
  - **Subtitle:** error message, `subtitle_lines: 2`, ellipsized
  - **Prefix:** `dialog-warning-symbolic` with `.warning` color
- If more than 10 errors: final non-activatable row with title "and N more errors" in dim text

### Enriched message

`IndexingStatusMsg::Update` gains `errors_detail: Vec<IndexingError>`. The App stores `last_errors_detail` alongside `last_per_source`.

### Phase boundary

- **Phase 1:** dialog without error section, `IndexingError` does not exist yet
- **Phase 2:** add `IndexingError`, collect in indexer, add UI section

---

## 7. CSS

In `data/resources/style.css`, for source card pill badges:

```css
.source-count-pill {
    border-radius: 99px;
    padding: 2px 8px;
    font-size: 0.85em;
    font-weight: 600;
}
.source-count-pill.source-status-ok {
    background-color: alpha(@success_color, 0.15);
    color: @success_color;
}
.source-count-pill.source-status-degraded {
    background-color: alpha(@warning_color, 0.15);
    color: @warning_color;
}
.source-count-pill.source-status-not-found {
    background-color: alpha(@view_fg_color, 0.08);
    color: alpha(@view_fg_color, 0.5);
}
```

Uses libadwaita semantic colors. Adapts automatically to light/dark/high-contrast themes. No hardcoded hex values.

No custom icons — all symbolic icons are standard GTK: `emblem-ok-symbolic`, `dialog-warning-symbolic`, `content-loading-symbolic`, `edit-copy-symbolic`.

---

## 8. Accessibility

- **Summary icon:** `tooltip_text` ("Status: OK", "Status: errors detected", "Status: indexing")
- **Source pills:** `accessible_label` ("182 sessions, status OK" / "38 sessions, errors present" / "not available")
- **Error rows (Phase 2):** `accessible_description` with full error message (subtitle may be visually truncated)
- **Re-index All button:** `accessible_label: "Re-index all session sources"`
- **Copy button:** `tooltip_text: "Copy path to clipboard"`
- **Focus chain:** Tab traverses summary → each expander → Re-index All. Escape closes the dialog (native `AdwDialog` behavior).

---

## 9. File Locations

### New files

- `src/ui/modals/indexing_status.rs` — the dialog `SimpleComponent`

### Modified files

- `src/ui/modals/mod.rs` — add `pub mod indexing_status;`
- `src/app/mod.rs` — add `AppMsg::ShowIndexingStatus`, `IndexingStatusAction`, menu entry, `last_per_source` field, dialog controller field
- `src/app/init.rs` — register action, wire banner `button-clicked`, set accelerator
- `src/app/handlers/indexing.rs` — store `last_per_source`, set banner button label, forward data to dialog if open
- `src/ui/modals/shortcuts.rs` — add `Ctrl+Shift+I` entry
- `data/resources/style.css` — pill badge classes

### Phase 2 additional changes

- `src/models/indexing_diagnostics.rs` — add `IndexingError` struct
- `src/indexing_worker.rs` — add `errors_detail` field to `IndexingWorkerOutput::Completed`
- `src/database/indexer.rs` — collect errors into `Vec<IndexingError>`
- `src/app/mod.rs` — add `last_errors_detail` field, enrich `AppMsg::IndexingCompleted`

---

## 10. Tests & Verification

### Unit tests

- **Summary state derivation:** pure function returning `(title, subtitle, icon_name)` from `(Vec<PerSourceResult>, bool)`. Test all 6 states from the table in Section 2.
- **Banner "Details" label:** verify `banner.button_label()` is `Some("Details")` when issues present, `None` when OK.
- **Source card ordering:** verify `NotFound` sources sort last.
- **Pill text:** verify count label displays "N" for Indexed/Degraded/Failed/Empty, "N/A" for NotFound.
- **Empty vs NotFound rendering:** verify `Empty` rows remain expandable with visible path, while `NotFound` rows are not expandable and show "Source not found".
- **Phase 2 — Error cap:** verify `errors_detail` is truncated to 50 entries in the indexer.
- **Phase 2 — Error row count:** verify dialog shows max 10 rows + "and N more" on overflow.

### Manual verification

| Scenario | Command | Expected |
|---|---|---|
| Sources OK | `--sessions-dir tests/fixtures` | All green sources, summary "N sessions indexed", no errors section |
| Empty sources | `--sessions-dir /tmp/empty` | Grey sources with badge `0`, summary "No sessions found", rows expandable so paths can be inspected |
| No detected sources | without `--sessions-dir`, on a machine with no supported assistants installed | Grey sources, summary "No sessions found", `NotFound` rows non-expandable |
| Real data | without `--sessions-dir` | Sources detected per installed assistants |
| Banner → Details | Trigger indexing error | Banner visible, click "Details" opens dialog |
| Re-index All | Click button in dialog | Spinner, then live data update |
| Ctrl+Shift+I | Shortcut | Dialog opens from any workspace |
| Narrow window | Reduce width | Dialog adaptive (bottom sheet behavior) |

### CI

No pipeline changes. New unit tests covered by `cargo test --all --no-fail-fast`.

---

## References

- [GNOME HIG — Dialogs](https://developer.gnome.org/hig/patterns/feedback/dialogs.html)
- [AdwDialog documentation](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/class.Dialog.html)
- Exploration: `docs/explorations/2026-03-23-indexing-diagnostics-exploration.md` — Proposal B
- Proposal C design: `docs/explorations/2026-03-24-indexing-diagnostics-design.md`
- `src/indexing_worker.rs` — current worker output and `PerSourceResult`
- `src/app/handlers/indexing.rs` — current banner and toast logic
- `src/ui/modals/preferences.rs` — modal pattern reference
