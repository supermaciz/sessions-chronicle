# Indexing Diagnostics — AdwBanner + Inline Sidebar Status

**Issue:** [#93](https://github.com/supermaciz/sessions-chronicle/issues/93)  
**Date:** 2026-03-24  
**Type:**  Design plan  
**Status:** Implemented [#95](https://github.com/supermaciz/sessions-chronicle/pull/95)  
**Exploration:** `docs/explorations/2026-03-23-indexing-diagnostics-exploration.md` — Proposal C  
**Related:** Issue #94 will add the dedicated diagnostics dialog (Proposal B) in a follow-up PR.

## Problem

Users have no visibility into indexing state. They cannot see which AI assistant sources are detected, which succeeded or failed, or when errors occurred. Silent failures erode trust in a local-first tool.

## Scope

This design implements Proposal C from the exploration: `AdwBanner` for persistent indexing issues, enriched toasts for transient feedback, sidebar status dots for at-a-glance source health, and an enhanced empty state showing detected source paths.

**Explicitly out of scope:**
- "Details" and "Retry" actions on banner or toast (deferred to issue #94 or follow-up work)
- Per-source error log or error messages (deferred to issue #94)
- Session counts in sidebar assistant rows (already present in Projects section)

---

## 1. Backend: Enriched `IndexingWorkerOutput`

### New data structures

In `src/indexing_worker.rs`:

```rust
#[derive(Debug, Clone)]
pub struct IndexingRunResult {
    pub totals: IndexingStats,
    pub per_source: Vec<PerSourceResult>,
}

#[derive(Debug, Clone)]
pub struct PerSourceResult {
    pub assistant: AiAssistant,
    pub display_path: String,
    pub indexed: usize,
    pub skipped: usize,
    pub errors: usize,
    pub status: SourceStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    NotFound,   // no usable source detected for this assistant
    Empty,      // source available but 0 sessions found
    Indexed,    // indexed >= 1, errors == 0
    Degraded,   // indexed >= 1, errors >= 1
    Failed,     // indexed == 0, errors >= 1
}
```

`PerSourceResult` is assistant-level rather than backend-level. `display_path` is a UI-facing resolved location string: a single path for Claude Code/Codex/Mistral Vibe, and the most relevant resolved location for OpenCode (storage root when present, otherwise the SQLite DB path).

### Enriched output

```rust
pub enum IndexingWorkerOutput {
    Completed {
        indexed: usize,
        skipped: usize,
        per_source: Vec<PerSourceResult>,
    },
    Failed,
}
```

`IndexingWorkerOutput` loses its `Copy` derive (because `PerSourceResult` contains owned strings).

### Indexer changes

`IndexingStats` gains an `errors: usize` field. Existing `tracing::warn!` calls in per-session indexing loops (e.g. `index_claude_sessions_internal`, `index_opencode_sessions_internal`, etc.) increment `errors` in addition to logging.

`index_all_incremental` and `index_all_full_reindex` return an `IndexingRunResult` instead of a bare aggregated `IndexingStats`. Each per-assistant call builds its own `PerSourceResult`, then appends it to `per_source` while also updating aggregate totals.

`SourceStatus` is derived from assistant-level source availability, not from a single filesystem path check:

| `source_available` | `indexed` | `errors` | Status |
|---|---|---|---|
| `false` | — | — | `NotFound` |
| `true` | `0` | `0` | `Empty` |
| `true` | `> 0` | `0` | `Indexed` |
| `true` | `> 0` | `> 0` | `Degraded` |
| `true` | `0` | `> 0` | `Failed` |

For Claude Code, Codex, and Mistral Vibe, `source_available` means the resolved path exists.
For OpenCode, `source_available` means **either** the storage root exists **or** the SQLite DB exists. This preserves the current supported case where OpenCode indexes successfully from SQLite even when the storage directory is absent.

### Forwarding to the App

The `forward` closure in `src/app/init.rs` maps the enriched `IndexingWorkerOutput::Completed` to an enriched `AppMsg::IndexingCompleted` carrying `per_source: Vec<PerSourceResult>`.

---

## 2. AdwBanner for Persistent Issues

### Placement

The banner lives inside a dedicated Sessions-page container that becomes the Sessions child of the workspace stack. It is inserted above the `OverlaySplitView` inside that container, so it is visible only in the Sessions workspace and does not bleed into Analytics.

### Widget

A single `adw::Banner` stored as a field on the `App` model for imperative control from handlers.

### Display logic

- **Hidden by default** (`set_revealed: false`).
- **Revealed** when `IndexingCompleted` arrives with at least one source in `Degraded` or `Failed`.
- **Hidden** when the next `IndexingCompleted` arrives with no problems.
- **No manual dismiss** in this phase; `AdwBanner` is used as a passive status surface and clears on the next clean indexing result.
- **No auto-hide** timeout.
- **Not shown** before first indexing completes.

`SourceStatus::NotFound` remains visible in sidebar dots and in the enhanced empty state, but it does **not** reveal the banner in normal operation.

### Message construction

The banner title is built dynamically from `Vec<PerSourceResult>`:

| Situation | Message |
|---|---|
| 1 problematic source | `"1 session source has indexing issues"` |
| N problematic sources | `"N session sources have indexing issues"` |

The banner title describes a **current state**, not a completed event. Detailed counts stay in the toast.

### No action button

No "Details" button in this phase. No custom dismiss button is added either. Actions will be wired to the diagnostics dialog in issue #94 or follow-up work.

---

## 3. Enriched Toasts

### Three cases

| Case | Condition | Message | Timeout |
|---|---|---|---|
| Success | `errors == 0` | `"Index rebuilt — N sessions"` (unchanged) | 3s |
| Partial | `errors > 0` | `"Indexed N sessions with E errors"` | 5s |
| Failure | `IndexingWorkerOutput::Failed` | `"Background indexing failed"` (unchanged) | 3s |

### Toast / banner interaction

- **Success:** toast only, banner hidden.
- **Partial:** toast + banner revealed. Toast is ephemeral (5s), banner persists.
- **Failure:** toast only. The app preserves the last known per-source UI state because the worker produced no fresh `per_source` data.

### Display condition

The completion toast remains gated by `pending_reindex_feedback` for startup indexing (existing behavior preserved). The banner is **always** updated on every `IndexingCompleted`, regardless of `pending_reindex_feedback`.

---

## 4. Sidebar: Status Dots on Assistant Filters

### Structural change

The plain `gtk::CheckButton` widgets in `sidebar.rs` are replaced by horizontal `gtk::Box` containers:

1. `gtk::CheckButton` (with label) — `hexpand: true`
2. `gtk::Image` (status dot) — fixed size, `halign: End`, `valign: Center`

The `hexpand` on the checkbutton pushes all dots to the right edge, keeping them **vertically aligned** across all assistant rows.

### Dot icon

A small `gtk::Image` using a circular symbolic icon (`"record-symbolic"` or `"circle-filled-symbolic"` depending on theme availability) with a CSS class for color.

### Three visual states

| State | CSS class | Color | Condition |
|---|---|---|---|
| OK | `source-status-ok` | green (`@success_color`) | `SourceStatus::Indexed` |
| Degraded | `source-status-degraded` | orange (`@warning_color`) | `SourceStatus::Degraded` or `Failed` |
| Not found | `source-status-not-found` | grey | `SourceStatus::NotFound` or `Empty` |

### CSS

In `data/resources/style.css`:

```css
.source-status-ok { color: @success_color; }
.source-status-degraded { color: @warning_color; }
.source-status-not-found { color: alpha(@view_fg_color, 0.3); }
```

Uses libadwaita/GTK named colors for automatic light/dark theme adaptation.

### Data and messages

The `Sidebar` model gains a field:

```rust
source_statuses: HashMap<AiAssistant, SourceStatus>,
```

A new `SidebarMsg::SourceStatusesUpdated(HashMap<AiAssistant, SourceStatus>)` is sent by the app on each `IndexingCompleted`. The sidebar updates CSS classes on the dot widgets.

`AiAssistant` derives `Hash` in addition to its existing traits so it can be used as the `HashMap` key.

### Initial state

Dots are **not visible** until the first indexing completes. The `HashMap` starts empty, and dots are hidden (`set_visible: false`) when there is no entry for the assistant.

---

## 5. Enhanced Empty State

### Change

When the app is in the genuine global empty state and indexing has completed, the `AdwStatusPage` "No Sessions Yet" (in `session_list.rs`) gains a child widget (`set_child`) listing detected sources and their status.

The child is a non-selectable `gtk::ListBox` with the `boxed-list` style class and one `adw::ActionRow` per assistant:

- **Title:** assistant display name (e.g. "Claude Code")
- **Subtitle:** resolved location in monospace (truncated with ellipsis), or "Source not found" if absent
- **Prefix:** status dot (same icon and CSS classes as the sidebar)

### Display conditions

The child widget is shown only when:
- The session list is empty
- Indexing is not in progress (otherwise "Indexing sessions..." is shown)
- Per-source results are available (at least one indexing has completed)
- The view is in the existing global empty-state branch (`all_tools_selected`, no project filter, no search query)

It is **not** shown for "No sessions match search" or "No sessions match filters".

### Data

`SessionList` receives a new `SessionListMsg::SetSourceResults(Vec<PerSourceResult>)` sent by the app on each `IndexingCompleted`. The model stores these results for building the child widget.

### Empty state text

The existing description `"Your AI coding sessions will appear here"` is replaced by `"No sessions found in checked session sources"` when source results are available.

---

## 6. Data Flow and Orchestration

### Modified `AppMsg`

```rust
IndexingCompleted {
    indexed: usize,
    skipped: usize,
    per_source: Vec<PerSourceResult>,
},
```

### Modified `App` model

New field:

```rust
banner: adw::Banner,
```

### `handle_indexing_completed` operation order

1. Set `self.indexing = false`
2. Build banner state from `per_source` (`Degraded`/`Failed` only) → `banner.set_revealed(has_problems)` + `banner.set_title(msg)`
3. Emit `SidebarMsg::SourceStatusesUpdated` with `HashMap<AiAssistant, SourceStatus>` extracted from `per_source`
4. Emit `SessionListMsg::SetSourceResults(per_source.clone())`
5. Conditional toast (success or partial, gated by `pending_reindex_feedback`)
6. Refresh sidebar projects + emit filters (existing)
7. Analytics refresh (existing)

### No persistence

`per_source` results live in memory only (in the `App` model and propagated to child components). They are recomputed on each indexing run. No database storage.

---

## 7. Tests and Verification

### Unit tests

**`IndexingStats` with errors** — verify that the `errors` counter increments when a session file is malformed (adapt existing test with a corrupt fixture).

**`SourceStatus` derivation** — test the function that derives `SourceStatus` from `(source_available, indexed, errors)`:

| `source_available` | `indexed` | `errors` | Expected |
|---|---|---|---|
| `false` | `0` | `0` | `NotFound` |
| `true` | `0` | `0` | `Empty` |
| `true` | `5` | `0` | `Indexed` |
| `true` | `5` | `2` | `Degraded` |
| `true` | `0` | `3` | `Failed` |

**OpenCode source availability** — test the assistant-level helper so that an existing SQLite DB with a missing storage root is treated as available, not `NotFound`.

**Banner message** — test the banner text construction function from a `Vec<PerSourceResult>` (pure function, easily testable), ensuring the wording reflects persistent state rather than a completed event.

**Banner visibility** — verify that `NotFound` alone does not reveal the banner, while `Degraded` and `Failed` do.

**Empty state gating** — verify that the diagnostics list appears only for the global empty state and not for search/filter empty states.

### Manual verification

- `flatpak-builder --run ... sessions-chronicle --sessions-dir tests/fixtures` → all green dots, no banner
- `flatpak-builder --run ... sessions-chronicle --sessions-dir /tmp/empty` → grey dots, no banner, empty state with resolved paths
- Run with one malformed fixture/source that still yields some indexed sessions → orange dot for the affected assistant, partial toast, persistent banner
- `flatpak-builder --run ... sessions-chronicle` (real data) → verify behavior with actually installed sources

### CI

No CI pipeline changes. New unit tests are covered by `cargo test --all --no-fail-fast`.

---

## References

- [GNOME HIG — Feedback patterns](https://developer.gnome.org/hig/patterns/feedback/)
- [AdwBanner documentation](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/class.Banner.html)
- Exploration: `docs/explorations/2026-03-23-indexing-diagnostics-exploration.md`
- `src/indexing_worker.rs` — current worker output
- `src/database/indexer.rs` — current `IndexingStats` and error handling
- `src/ui/sidebar.rs` — current assistant filter rows
- `src/ui/session_list.rs` — current empty state
- `src/app/handlers/indexing.rs` — current toast logic
