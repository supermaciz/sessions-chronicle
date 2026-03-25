# Indexing Status Dialog Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a dedicated indexing-status dialog that exposes per-assistant indexing results, re-index entry points, and recent error details without adding database persistence.

**Architecture:** Add a lazily created Relm4 `SimpleComponent` modal under `src/ui/modals/` and drive it from app-owned in-memory indexing diagnostics. Phase 1 only reuses existing `PerSourceResult` data for summary and source cards; Phase 2 extends the diagnostics model and indexing pipeline with a capped `IndexingError` list that the dialog renders in a separate section.

**Tech Stack:** Rust 2024, Relm4 0.10, libadwaita 1.8 (`adw`), GTK4, rusqlite, existing fixture-driven test setup

---

## Read This First

- This plan assumes you are already in a dedicated worktree created earlier with the brainstorming workflow. If you are not, stop and create that worktree first.
- Re-read `docs/plans/2026-03-25-indexing-status-dialog-design.md` before Task 1.
- Use `@superpowers:test-driven-development` for every coding task in this plan.
- Use `@superpowers:systematic-debugging` immediately if a GTK test or Relm4 widget assertion fails unexpectedly.
- Use `@superpowers:verification-before-completion` before you claim the feature is done.
- Use `@superpowers:requesting-code-review` after Task 8 verification is clean.

## Context Files Worth Keeping Open

- `src/ui/modals/preferences.rs:24`
- `src/ui/modals/shortcuts.rs:7`
- `src/app/mod.rs:92`
- `src/app/init.rs:55`
- `src/app/handlers/indexing.rs:16`
- `src/app/helpers.rs:119`
- `src/database/indexer.rs:31`
- `src/models/indexing_diagnostics.rs:4`
- `docs/DEVELOPMENT_WORKFLOW.md:61`

---

### Task 1: Create dialog view-state helpers

**Files:**
- Create: `src/ui/modals/indexing_status.rs`
- Modify: `src/ui/modals/mod.rs:1-3`
- Test: `src/ui/modals/indexing_status.rs`

**Step 1: Write the failing unit tests**

Add these tests first in `src/ui/modals/indexing_status.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AiAssistant, PerSourceResult, SourceStatus};

    fn make_result(
        assistant: AiAssistant,
        status: SourceStatus,
        indexed: usize,
        skipped: usize,
        errors: usize,
        display_path: &str,
    ) -> PerSourceResult {
        PerSourceResult {
            assistant,
            display_path: display_path.to_string(),
            indexed,
            skipped,
            errors,
            status,
        }
    }

    #[test]
    fn indexing_status_summary_state_covers_all_phase1_cases() {
        assert_eq!(
            derive_summary_state(&[], false),
            SummaryState::new("Not yet indexed", None, "content-loading-symbolic")
        );
        assert_eq!(
            derive_summary_state(&[], true),
            SummaryState::new("Indexing in progress...", None, "content-loading-symbolic")
        );
        assert_eq!(
            derive_summary_state(
                &[make_result(
                    AiAssistant::ClaudeCode,
                    SourceStatus::Indexed,
                    12,
                    0,
                    0,
                    "/tmp/claude",
                )],
                false,
            ),
            SummaryState::new(
                "12 sessions indexed",
                Some("Completed successfully"),
                "emblem-ok-symbolic",
            )
        );
    }

    #[test]
    fn indexing_status_orders_not_found_sources_last() {
        let rows = build_source_rows(&[
            make_result(AiAssistant::Codex, SourceStatus::NotFound, 0, 0, 0, "/missing/codex"),
            make_result(AiAssistant::OpenCode, SourceStatus::Empty, 0, 0, 0, "/tmp/opencode"),
            make_result(AiAssistant::ClaudeCode, SourceStatus::Indexed, 4, 0, 0, "/tmp/claude"),
        ]);

        assert_eq!(rows[0].assistant, AiAssistant::ClaudeCode);
        assert_eq!(rows[1].assistant, AiAssistant::OpenCode);
        assert_eq!(rows[2].assistant, AiAssistant::Codex);
        assert!(!rows[2].expandable);
        assert!(rows[1].expandable);
    }

    #[test]
    fn indexing_status_pill_text_uses_na_for_missing_source() {
        let row = SourceRowState::from_result(&make_result(
            AiAssistant::MistralVibe,
            SourceStatus::NotFound,
            0,
            0,
            0,
            "/missing/vibe",
        ));

        assert_eq!(row.badge_text, "N/A");
        assert_eq!(row.subtitle_markup, "Source not found");
        assert_eq!(row.badge_css_class, "source-status-not-found");
    }
}
```

Then extend `indexing_status_summary_state_covers_all_phase1_cases` to include the remaining three states from the design doc: partial success, empty sources, and no detected sources.

**Step 2: Run the focused test to verify it fails**

Run: `cargo test indexing_status_summary_state_covers_all_phase1_cases -- --exact`

Expected: FAIL with a compile error because `src/ui/modals/indexing_status.rs`, `SummaryState`, and `derive_summary_state` do not exist yet.

**Step 3: Write the minimal implementation**

Create `src/ui/modals/indexing_status.rs` with data-only helpers first, not the full widget tree yet:

```rust
use crate::models::{AiAssistant, PerSourceResult, SourceStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SummaryState {
    title: String,
    subtitle: Option<String>,
    icon_name: &'static str,
}

impl SummaryState {
    fn new(title: &str, subtitle: Option<&str>, icon_name: &'static str) -> Self {
        Self {
            title: title.to_string(),
            subtitle: subtitle.map(str::to_string),
            icon_name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceRowState {
    assistant: AiAssistant,
    display_path: String,
    subtitle_markup: String,
    badge_text: String,
    badge_css_class: &'static str,
    expandable: bool,
    indexed: usize,
    skipped: usize,
    errors: usize,
}

impl SourceRowState {
    fn from_result(result: &PerSourceResult) -> Self {
        let subtitle_markup = if matches!(result.status, SourceStatus::NotFound) {
            "Source not found".to_string()
        } else {
            format!("<tt>{}</tt>", glib::markup_escape_text(&result.display_path))
        };

        let badge_text = if matches!(result.status, SourceStatus::NotFound) {
            "N/A".to_string()
        } else {
            (result.indexed + result.skipped).to_string()
        };

        let badge_css_class = match result.status {
            SourceStatus::Indexed => "source-status-ok",
            SourceStatus::Degraded | SourceStatus::Failed => "source-status-degraded",
            SourceStatus::NotFound | SourceStatus::Empty => "source-status-not-found",
        };

        Self {
            assistant: result.assistant,
            display_path: result.display_path.clone(),
            subtitle_markup,
            badge_text,
            badge_css_class,
            expandable: !matches!(result.status, SourceStatus::NotFound),
            indexed: result.indexed,
            skipped: result.skipped,
            errors: result.errors,
        }
    }
}

fn derive_summary_state(results: &[PerSourceResult], indexing: bool) -> SummaryState {
    if indexing {
        return SummaryState::new("Indexing in progress...", None, "content-loading-symbolic");
    }

    if results.is_empty() {
        return SummaryState::new("Not yet indexed", None, "content-loading-symbolic");
    }

    let indexed_total: usize = results.iter().map(|r| r.indexed).sum();
    let skipped_total: usize = results.iter().map(|r| r.skipped).sum();
    let errors_total: usize = results.iter().map(|r| r.errors).sum();
    let no_detected_sources = results
        .iter()
        .all(|r| matches!(r.status, SourceStatus::NotFound));
    let empty_sources = results.iter().any(|r| matches!(r.status, SourceStatus::Empty))
        && indexed_total == 0
        && skipped_total == 0
        && !no_detected_sources;

    if no_detected_sources {
        return SummaryState::new(
            "No sessions found",
            Some("No session sources detected"),
            "dialog-warning-symbolic",
        );
    }

    if empty_sources {
        return SummaryState::new(
            "No sessions found",
            Some("Session sources detected, but no sessions were found"),
            "dialog-warning-symbolic",
        );
    }

    if errors_total > 0 {
        return SummaryState::new(
            &format!("{indexed_total} sessions indexed"),
            Some(&format!("Completed with {errors_total} errors")),
            "dialog-warning-symbolic",
        );
    }

    SummaryState::new(
        &format!("{indexed_total} sessions indexed"),
        Some("Completed successfully"),
        "emblem-ok-symbolic",
    )
}

fn build_source_rows(results: &[PerSourceResult]) -> Vec<SourceRowState> {
    let mut rows: Vec<_> = results.iter().map(SourceRowState::from_result).collect();
    rows.sort_by_key(|row| {
        let missing_rank = matches!(
            row.badge_css_class,
            "source-status-not-found"
        ) && row.badge_text == "N/A";
        let assistant_rank = AiAssistant::ALL
            .iter()
            .position(|assistant| *assistant == row.assistant)
            .unwrap_or(usize::MAX);
        (missing_rank, assistant_rank)
    });
    rows
}
```

Also update `src/ui/modals/mod.rs`:

```rust
pub mod about;
pub mod indexing_status;
pub mod preferences;
pub mod shortcuts;
```

**Step 4: Run the focused tests to verify they pass**

Run: `cargo test indexing_status_ -- --nocapture`

Expected: PASS for the new `indexing_status_*` tests in `src/ui/modals/indexing_status.rs`.

**Step 5: Format the code**

Run: `cargo fmt --all`

Expected: command exits successfully with no diff from rustfmt afterward.

**Step 6: Commit**

Run:

```bash
git add src/ui/modals/indexing_status.rs src/ui/modals/mod.rs
git commit -m "test: scaffold indexing status dialog helpers"
```

---

### Task 2: Build the Phase 1 dialog shell

**Files:**
- Modify: `src/ui/modals/indexing_status.rs`
- Test: `src/ui/modals/indexing_status.rs`

**Step 1: Write the failing GTK component tests**

Add `#[gtk::test]` coverage for the actual dialog component:

```rust
#[gtk::test]
fn indexing_status_dialog_hides_sources_before_first_index() {
    let controller = IndexingStatusDialog::builder().launch(());
    let parts = controller.state().get();

    assert!(!parts.widgets.sources_group.is_visible());
    assert_eq!(parts.widgets.summary_row.title(), "Not yet indexed");
}

#[gtk::test]
fn indexing_status_dialog_disables_reindex_button_while_indexing() {
    let controller = IndexingStatusDialog::builder().launch(());
    controller.emit(IndexingStatusMsg::Update {
        per_source: vec![],
        indexing: true,
    });

    let parts = controller.state().get();
    assert!(!parts.widgets.reindex_button.is_sensitive());
    assert!(parts.widgets.progress_bar.is_visible());
}

#[gtk::test]
fn indexing_status_dialog_keeps_empty_sources_expandable() {
    let controller = IndexingStatusDialog::builder().launch(());
    controller.emit(IndexingStatusMsg::Update {
        per_source: vec![PerSourceResult {
            assistant: AiAssistant::OpenCode,
            display_path: "/tmp/opencode".into(),
            indexed: 0,
            skipped: 0,
            errors: 0,
            status: SourceStatus::Empty,
        }],
        indexing: false,
    });

    let parts = controller.state().get();
    assert!(parts.model.source_rows[0].expandable);
}
```

**Step 2: Run the focused test to verify it fails**

Run: `cargo test indexing_status_dialog_hides_sources_before_first_index -- --exact`

Expected: FAIL because `IndexingStatusDialog`, `IndexingStatusMsg`, and named widgets do not exist yet.

**Step 3: Write the minimal implementation**

Turn `src/ui/modals/indexing_status.rs` into a real component. Follow the existing modal pattern from `src/ui/modals/preferences.rs:39`, but use `adw::Dialog` as the root.

Add these core pieces:

```rust
use adw::prelude::*;
use gtk::prelude::{BoxExt, ButtonExt, WidgetExt};
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};

pub struct IndexingStatusDialog {
    source_rows: Vec<SourceRowState>,
    indexing: bool,
}

#[derive(Debug, Clone)]
pub enum IndexingStatusMsg {
    Update {
        per_source: Vec<PerSourceResult>,
        indexing: bool,
    },
    ReindexRequested,
}

#[derive(Debug, Clone)]
pub enum IndexingStatusOutput {
    Reindex,
}

#[relm4::component(pub)]
impl SimpleComponent for IndexingStatusDialog {
    type Init = ();
    type Input = IndexingStatusMsg;
    type Output = IndexingStatusOutput;
    type Root = adw::Dialog;

    view! {
        root = adw::Dialog {
            set_title: Some("Indexing Status"),
            set_content_width: 480,
            set_content_height: 520,
            set_follows_content_size: true,

            #[wrap(Some)]
            set_child = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    pack_end = #[name = "reindex_button"] gtk::Button {
                        add_css_class: "suggested-action",
                        connect_clicked => IndexingStatusMsg::ReindexRequested,
                    }
                },

                #[wrap(Some)]
                set_content = &gtk::ScrolledWindow {
                    #[wrap(Some)]
                    set_child = &adw::Clamp {
                        set_maximum_size: 440,
                        #[wrap(Some)]
                        set_child = &gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 24,

                            #[name = "summary_group"]
                            adw::PreferencesGroup {
                                #[name = "summary_row"]
                                add = &adw::ActionRow {}
                            },

                            #[name = "progress_bar"]
                            gtk::ProgressBar {
                                set_pulse_step: 0.2,
                            },

                            #[name = "sources_group"]
                            adw::PreferencesGroup {
                                set_title: "Sources",
                            }
                        }
                    }
                }
            }
        }
    }
}
```

Then add helper methods on the model to:

- recompute `summary_state` and `source_rows` from `Update`
- set `summary_row` title/subtitle/icon
- show/hide `progress_bar`
- rebuild `sources_group` on every update using one `adw::ExpanderRow` per assistant
- add `Source Path`, `Sessions Indexed`, `Skipped`, and `Parse Errors` child rows
- disable the re-index button and swap its label to `Indexing...` while indexing is true

Use the existing clipboard pattern from `src/ui/modals/preferences.rs:106-123` for the per-source copy button.

**Step 4: Run the focused tests to verify they pass**

Run: `cargo test indexing_status_dialog_ -- --nocapture`

Expected: PASS for the new GTK dialog tests.

**Step 5: Format the code**

Run: `cargo fmt --all`

Expected: rustfmt completes successfully.

**Step 6: Commit**

Run:

```bash
git add src/ui/modals/indexing_status.rs
git commit -m "feat: add indexing status dialog shell"
```

---

### Task 3: Wire Phase 1 app state and lazy dialog creation

**Files:**
- Modify: `src/app/mod.rs:92-183`
- Modify: `src/app/mod.rs:376-408`
- Modify: `src/app/mod.rs:494-503`
- Modify: `src/app/init.rs:16-20`
- Modify: `src/app/init.rs:55-133`
- Modify: `src/app/handlers/indexing.rs:16-95`
- Test: `src/app/mod.rs`

**Step 1: Write the failing tests**

Add one pure behavior test and one GTK wiring test in `src/app/mod.rs`:

```rust
#[test]
fn indexing_status_dialog_state_starts_empty() {
    let last_per_source: Vec<crate::models::PerSourceResult> = vec![];
    assert!(last_per_source.is_empty());
}

#[gtk::test]
fn indexing_status_dialog_is_created_lazily() {
    let schema_available = gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup(crate::config::APP_ID, true))
        .is_some();
    if !schema_available {
        return;
    }

    let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));

    {
        let parts = controller.state().get();
        assert!(parts.model.last_per_source.is_empty());
        assert!(parts.model.indexing_status_dialog.is_none());
    }

    controller.emit(AppMsg::ShowIndexingStatus);

    let parts = controller.state().get();
    assert!(parts.model.indexing_status_dialog.is_some());
}
```

**Step 2: Run the focused test to verify it fails**

Run: `cargo test indexing_status_dialog_is_created_lazily -- --exact`

Expected: FAIL because `AppMsg::ShowIndexingStatus`, `last_per_source`, and `indexing_status_dialog` do not exist yet.

**Step 3: Write the minimal implementation**

Update `src/app/mod.rs`:

```rust
use crate::ui::modals::{
    indexing_status::{IndexingStatusDialog, IndexingStatusMsg, IndexingStatusOutput},
    preferences::PreferencesDialog,
};

pub(super) struct App {
    // existing fields...
    last_per_source: Vec<crate::models::PerSourceResult>,
    indexing_status_dialog: Option<Controller<IndexingStatusDialog>>,
}

pub(super) enum AppMsg {
    // existing variants...
    ShowIndexingStatus,
    IndexingCompleted {
        indexed: usize,
        skipped: usize,
        per_source: Vec<crate::models::PerSourceResult>,
    },
}
```

Initialize the new fields in `App::init`:

```rust
last_per_source: Vec::new(),
indexing_status_dialog: None,
```

Handle the new message in `update`:

```rust
AppMsg::ShowIndexingStatus => {
    if self.indexing_status_dialog.is_none() {
        let dialog = IndexingStatusDialog::builder()
            .launch(())
            .forward(sender.input_sender(), |output| match output {
                IndexingStatusOutput::Reindex => AppMsg::ReindexRequested,
            });
        self.indexing_status_dialog = Some(dialog);
    }

    if let Some(dialog) = self.indexing_status_dialog.as_ref() {
        dialog.emit(IndexingStatusMsg::Update {
            per_source: self.last_per_source.clone(),
            indexing: self.indexing,
        });
        dialog.widget().present(Some(&main_application().windows()[0]));
    }
}
```

Update `handle_indexing_completed` in `src/app/handlers/indexing.rs` so the app stores and forwards the latest diagnostics:

```rust
self.last_per_source = per_source.clone();

if let Some(dialog) = self.indexing_status_dialog.as_ref() {
    dialog.emit(IndexingStatusMsg::Update {
        per_source: self.last_per_source.clone(),
        indexing: false,
    });
}
```

Update `handle_reindex_requested` so an open dialog receives `indexing: true` immediately after the worker starts.

Do not create the dialog in `init_child_components`; keep creation lazy exactly as the design requires.

**Step 4: Run the focused tests to verify they pass**

Run: `cargo test indexing_status_dialog_is_created_lazily -- --exact`

Expected: PASS.

**Step 5: Format the code**

Run: `cargo fmt --all`

Expected: rustfmt exits successfully.

**Step 6: Commit**

Run:

```bash
git add src/app/mod.rs src/app/handlers/indexing.rs
git commit -m "feat: lazily create indexing status dialog"
```

---

### Task 4: Add entry points, banner details button, shortcuts, and badge CSS

**Files:**
- Modify: `src/app/mod.rs:176-200`
- Modify: `src/app/init.rs:382-469`
- Modify: `src/app/handlers/indexing.rs:52-95`
- Modify: `src/app/helpers.rs:119-138`
- Modify: `src/ui/modals/shortcuts.rs:28-40`
- Modify: `data/resources/style.css:359-367`
- Test: `src/app/helpers.rs`

**Step 1: Write the failing tests**

Add this pure test in `src/app/helpers.rs`:

```rust
#[test]
fn indexing_status_banner_button_label_matches_issue_presence() {
    let degraded = make_result(AiAssistant::ClaudeCode, SourceStatus::Degraded);
    let ok = make_result(AiAssistant::ClaudeCode, SourceStatus::Indexed);

    assert_eq!(banner_button_label(&[degraded]), Some("Details"));
    assert_eq!(banner_button_label(&[ok]), None);
    assert_eq!(banner_button_label(&[]), None);
}
```

**Step 2: Run the focused test to verify it fails**

Run: `cargo test indexing_status_banner_button_label_matches_issue_presence -- --exact`

Expected: FAIL because `banner_button_label` does not exist yet.

**Step 3: Write the minimal implementation**

In `src/app/helpers.rs`, add:

```rust
pub(super) fn banner_button_label(results: &[PerSourceResult]) -> Option<&'static str> {
    if banner_title(results).is_some() {
        Some("Details")
    } else {
        None
    }
}
```

Then wire the three entry points.

In `src/app/mod.rs`, add the action and menu item above Preferences:

```rust
relm4::new_stateless_action!(IndexingStatusAction, WindowActionGroup, "indexing-status");

menu! {
    primary_menu: {
        section! {
            "_Indexing Status..." => IndexingStatusAction,
            "_Preferences" => PreferencesAction,
            "_Keyboard" => ShortcutsAction,
            "_About Sessions Chronicle" => AboutAction,
        }
    }
}
```

In `src/app/init.rs`, register the action and accelerator:

```rust
let indexing_status_action = {
    let sender = sender.clone();
    RelmAction::<IndexingStatusAction>::new_stateless(move |_| {
        sender.input(AppMsg::ShowIndexingStatus);
    })
};

app.set_accelerators_for_action::<IndexingStatusAction>(&["<Control><Shift>i"]);
actions.add_action(indexing_status_action);
```

Also connect the persistent banner button once during app setup:

```rust
let banner_sender = sender.input_sender().clone();
model.banner.connect_button_clicked(move |_| {
    banner_sender.send(AppMsg::ShowIndexingStatus).ok();
});
```

In `src/app/handlers/indexing.rs`, set the label only when issues exist:

```rust
self.banner.set_button_label(banner_button_label(&per_source));
```

In `src/ui/modals/shortcuts.rs`, add:

```rust
general.add(adw::ShortcutsItem::new(
    &gettext("Indexing Status"),
    "<Control><Shift>i",
));
```

In `data/resources/style.css`, append the badge styling right after the existing source dot styles:

```css
.source-count-pill {
  border-radius: 99px;
  padding: 2px 8px;
  font-size: 0.85em;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
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

Make sure the dialog badge label adds both `source-count-pill` and the per-status class.

**Step 4: Run the focused tests to verify they pass**

Run: `cargo test indexing_status_banner_button_label_matches_issue_presence -- --exact`

Expected: PASS.

**Step 5: Run the Phase 1 regression bucket**

Run: `cargo test indexing_status_ && cargo test indexing_diagnostics_`

Expected: PASS for dialog tests plus the existing indexing diagnostics tests in `src/app/helpers.rs`, `src/app/mod.rs`, and `src/database/indexer.rs`.

**Step 6: Commit**

Run:

```bash
git add src/app/mod.rs src/app/init.rs src/app/handlers/indexing.rs src/app/helpers.rs src/ui/modals/shortcuts.rs data/resources/style.css
git commit -m "feat: add indexing status entry points"
```

---

### Task 5: Extend diagnostics models and collect detailed indexing errors

**Files:**
- Modify: `src/models/indexing_diagnostics.rs:4-27`
- Modify: `src/models/mod.rs:12-13`
- Modify: `src/database/indexer.rs:8-9`
- Modify: `src/database/indexer.rs:114-158`
- Modify: `src/database/indexer.rs:180-332`
- Modify: `src/database/indexer.rs:417-487`
- Modify: `src/database/indexer.rs:776-887`
- Test: `src/database/indexer.rs:1708-1777`

**Step 1: Write the failing tests**

Add these tests in `src/database/indexer.rs`:

```rust
#[test]
fn indexing_diagnostics_error_details_are_capped_at_fifty() {
    let temp = tempfile::tempdir().unwrap();
    let claude_root = temp.path().join("claude_sessions").join("project-a");
    std::fs::create_dir_all(&claude_root).unwrap();

    for i in 0..60 {
        std::fs::write(claude_root.join(format!("bad-{i}.jsonl")), b"not-json\n").unwrap();
    }

    let temp_db = tempfile::NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let sources = SessionSources::resolve(Some(temp.path()));

    let result = indexer.index_all_incremental(&sources).unwrap();

    assert_eq!(result.errors_detail.len(), 50);
    assert!(result.errors_detail.iter().all(|error| error.assistant == AiAssistant::ClaudeCode));
}

#[test]
fn indexing_diagnostics_collects_source_level_opencode_errors() {
    let temp = tempfile::tempdir().unwrap();
    let storage_root = temp.path().join("missing-storage");
    let sqlite_path = temp.path().join("opencode.db");
    std::fs::write(&sqlite_path, b"not-a-real-db").unwrap();

    let sources = SessionSources {
        opencode_storage_root: storage_root.clone(),
        opencode_db_path: Some(sqlite_path.clone()),
        ..SessionSources::resolve(Some(temp.path()))
    };

    let temp_db = tempfile::NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let result = indexer.index_all_incremental(&sources).unwrap();

    assert!(result
        .errors_detail
        .iter()
        .any(|error| error.assistant == AiAssistant::OpenCode
            && error.location.as_deref().is_some_and(|path| path.contains("opencode.db"))));
}
```

**Step 2: Run the focused test to verify it fails**

Run: `cargo test indexing_diagnostics_error_details_are_capped_at_fifty -- --exact`

Expected: FAIL because `IndexingRunResult.errors_detail` and `IndexingError` do not exist yet.

**Step 3: Write the minimal implementation**

In `src/models/indexing_diagnostics.rs`, add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexingError {
    pub assistant: AiAssistant,
    pub location: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct IndexingRunResult {
    pub totals: IndexingStats,
    pub per_source: Vec<PerSourceResult>,
    pub errors_detail: Vec<IndexingError>,
}
```

Export it from `src/models/mod.rs`:

```rust
pub use indexing_diagnostics::{IndexingError, IndexingRunResult, PerSourceResult, SourceStatus};
```

In `src/database/indexer.rs`, add a small capped collector helper near `build_per_source_result`:

```rust
const MAX_INDEXING_ERRORS: usize = 50;

fn push_indexing_error(
    errors_detail: &mut Vec<IndexingError>,
    assistant: AiAssistant,
    location: Option<String>,
    message: impl Into<String>,
) {
    if errors_detail.len() >= MAX_INDEXING_ERRORS {
        return;
    }

    errors_detail.push(IndexingError {
        assistant,
        location,
        message: message.into(),
    });
}
```

Thread `errors_detail: Vec<IndexingError>` through each per-source indexing function by mutable reference. At every existing `tracing::warn!` site that corresponds to a real failure, keep the log and also call `push_indexing_error(...)`.

Required locations:

- Claude JSONL parse/index failures in `index_claude_sessions_internal`
- OpenCode SQLite open/list/insert/parse failures in `index_opencode_sessions_internal`
- OpenCode JSON parse/index failures in `index_opencode_sessions_internal`
- Codex indexing failures in `index_codex_sessions_internal`
- Mistral Vibe parse/index failures in `index_vibe_sessions_internal`

When `index_all_incremental` and `index_all_full_reindex` return, include the collected vector:

```rust
Ok(IndexingRunResult {
    totals,
    per_source,
    errors_detail,
})
```

**Step 4: Run the focused tests to verify they pass**

Run: `cargo test indexing_diagnostics_error_details_are_capped_at_fifty -- --exact && cargo test indexing_diagnostics_collects_source_level_opencode_errors -- --exact`

Expected: PASS.

**Step 5: Format the code**

Run: `cargo fmt --all`

Expected: rustfmt exits successfully.

**Step 6: Commit**

Run:

```bash
git add src/models/indexing_diagnostics.rs src/models/mod.rs src/database/indexer.rs
git commit -m "feat: collect detailed indexing errors"
```

---

### Task 6: Plumb detailed errors through worker and app state

**Files:**
- Modify: `src/indexing_worker.rs:19-57`
- Modify: `src/app/mod.rs:136-174`
- Modify: `src/app/mod.rs:376-408`
- Modify: `src/app/init.rs:102-115`
- Modify: `src/app/handlers/indexing.rs:17-95`
- Modify: `src/ui/modals/indexing_status.rs`
- Test: `src/app/mod.rs`

**Step 1: Write the failing tests**

Add this GTK test in `src/app/mod.rs`:

```rust
#[gtk::test]
fn indexing_completed_stores_error_details_for_dialog() {
    let schema_available = gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup(crate::config::APP_ID, true))
        .is_some();
    if !schema_available {
        return;
    }

    let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));

    controller.emit(AppMsg::IndexingCompleted {
        indexed: 3,
        skipped: 0,
        per_source: vec![],
        errors_detail: vec![crate::models::IndexingError {
            assistant: AiAssistant::ClaudeCode,
            location: Some("broken.jsonl".into()),
            message: "invalid JSON".into(),
        }],
    });

    let parts = controller.state().get();
    assert_eq!(parts.model.last_errors_detail.len(), 1);
}
```

**Step 2: Run the focused test to verify it fails**

Run: `cargo test indexing_completed_stores_error_details_for_dialog -- --exact`

Expected: FAIL because `errors_detail` and `last_errors_detail` are not part of the app message/state yet.

**Step 3: Write the minimal implementation**

Update `src/indexing_worker.rs`:

```rust
use crate::models::{IndexingError, PerSourceResult};

pub enum IndexingWorkerOutput {
    Completed {
        indexed: usize,
        skipped: usize,
        per_source: Vec<PerSourceResult>,
        errors_detail: Vec<IndexingError>,
    },
    Failed,
}
```

Forward the new field from `run_result.errors_detail`.

Update `src/app/mod.rs`:

```rust
last_errors_detail: Vec<crate::models::IndexingError>,

IndexingCompleted {
    indexed: usize,
    skipped: usize,
    per_source: Vec<crate::models::PerSourceResult>,
    errors_detail: Vec<crate::models::IndexingError>,
},
```

Initialize `last_errors_detail: Vec::new()` in `App::init`.

Update `src/app/init.rs` so the worker forwarder passes the new field into `AppMsg::IndexingCompleted`.

Update `src/app/handlers/indexing.rs` so `handle_indexing_completed` stores and forwards the new data:

```rust
self.last_errors_detail = errors_detail.clone();

if let Some(dialog) = self.indexing_status_dialog.as_ref() {
    dialog.emit(IndexingStatusMsg::Update {
        per_source: self.last_per_source.clone(),
        indexing: false,
        errors_detail: self.last_errors_detail.clone(),
    });
}
```

Also update `AppMsg::ShowIndexingStatus` and `handle_reindex_requested` to include `errors_detail: self.last_errors_detail.clone()` in every dialog `Update` message.

Expand the dialog input message in `src/ui/modals/indexing_status.rs`:

```rust
Update {
    per_source: Vec<PerSourceResult>,
    indexing: bool,
    errors_detail: Vec<IndexingError>,
}
```

**Step 4: Run the focused tests to verify they pass**

Run: `cargo test indexing_completed_stores_error_details_for_dialog -- --exact`

Expected: PASS.

**Step 5: Run the updated regression bucket**

Run: `cargo test indexing_status_ && cargo test indexing_diagnostics_`

Expected: PASS.

**Step 6: Commit**

Run:

```bash
git add src/indexing_worker.rs src/app/mod.rs src/app/init.rs src/app/handlers/indexing.rs src/ui/modals/indexing_status.rs
git commit -m "feat: forward indexing error details to dialog"
```

---

### Task 7: Render the Phase 2 recent-errors section

**Files:**
- Modify: `src/ui/modals/indexing_status.rs`
- Test: `src/ui/modals/indexing_status.rs`

**Step 1: Write the failing tests**

Add these tests in `src/ui/modals/indexing_status.rs`:

```rust
#[test]
fn indexing_status_recent_errors_limits_visible_rows() {
    let errors: Vec<crate::models::IndexingError> = (0..12)
        .map(|i| crate::models::IndexingError {
            assistant: AiAssistant::ClaudeCode,
            location: Some(format!("/tmp/session-{i}.jsonl")),
            message: format!("failure {i}"),
        })
        .collect();

    let section = build_recent_error_rows(&errors);

    assert_eq!(section.rows.len(), 10);
    assert_eq!(section.overflow_label.as_deref(), Some("and 2 more errors"));
}

#[test]
fn indexing_status_recent_errors_use_file_basename_as_title() {
    let section = build_recent_error_rows(&[crate::models::IndexingError {
        assistant: AiAssistant::OpenCode,
        location: Some("/tmp/nested/session_abc123.jsonl".into()),
        message: "bad payload".into(),
    }]);

    assert_eq!(section.rows[0].title, "session_abc123.jsonl");
}

#[gtk::test]
fn indexing_status_dialog_hides_recent_errors_group_when_empty() {
    let controller = IndexingStatusDialog::builder().launch(());
    controller.emit(IndexingStatusMsg::Update {
        per_source: vec![],
        indexing: false,
        errors_detail: vec![],
    });

    let parts = controller.state().get();
    assert!(!parts.widgets.recent_errors_group.is_visible());
}
```

**Step 2: Run the focused test to verify it fails**

Run: `cargo test indexing_status_recent_errors_limits_visible_rows -- --exact`

Expected: FAIL because `build_recent_error_rows` and the Recent Errors UI do not exist yet.

**Step 3: Write the minimal implementation**

Add pure error-row helpers first:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
struct ErrorRowState {
    title: String,
    subtitle: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ErrorSectionState {
    rows: Vec<ErrorRowState>,
    overflow_label: Option<String>,
}

fn build_recent_error_rows(errors: &[IndexingError]) -> ErrorSectionState {
    let rows: Vec<_> = errors
        .iter()
        .take(10)
        .map(|error| ErrorRowState {
            title: error
                .location
                .as_deref()
                .and_then(|path| std::path::Path::new(path).file_name())
                .and_then(|name| name.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| error.assistant.display_name().to_string()),
            subtitle: error.message.clone(),
        })
        .collect();

    let overflow = errors.len().saturating_sub(rows.len());

    ErrorSectionState {
        rows,
        overflow_label: (overflow > 0).then(|| format!("and {overflow} more errors")),
    }
}
```

Then add a named `recent_errors_group` widget to the dialog `view!` tree and rebuild it during `Update`.

Required UI details:

- group title: `Recent Errors`
- hidden when `errors_detail.is_empty()`
- each row is an `adw::ActionRow` with `dialog-warning-symbolic` prefix
- subtitle lines limited to 2
- overflow row is non-activatable and dimmed
- set `accessible_description` to the full error message even when the subtitle is ellipsized

**Step 4: Run the focused tests to verify they pass**

Run: `cargo test indexing_status_recent_errors_ -- --nocapture`

Expected: PASS for the new recent-errors tests.

**Step 5: Format the code**

Run: `cargo fmt --all`

Expected: rustfmt exits successfully.

**Step 6: Commit**

Run:

```bash
git add src/ui/modals/indexing_status.rs
git commit -m "feat: show recent indexing errors"
```

---

### Task 8: Run full verification and prepare handoff

**Files:**
- Verify only: `docs/plans/2026-03-25-indexing-status-dialog-design.md`
- Verify only: `src/ui/modals/indexing_status.rs`
- Verify only: `src/app/mod.rs`
- Verify only: `src/app/init.rs`
- Verify only: `src/app/handlers/indexing.rs`
- Verify only: `src/database/indexer.rs`
- Verify only: `data/resources/style.css`

**Step 1: Run the focused regression bucket**

Run:

```bash
cargo test indexing_status_ && cargo test indexing_diagnostics_
```

Expected: PASS.

**Step 2: Run CI-parity checks**

Run:

```bash
cargo fmt --all -- --check && cargo clippy --all -- -D warnings && cargo test --all --no-fail-fast
```

Expected: PASS for all three commands.

**Step 3: Run the manual fixture scenario**

Run:

```bash
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures
```

Expected:

- dialog opens from hamburger menu and `Ctrl+Shift+I`
- summary shows indexed session count
- sources render in Claude/OpenCode/Codex/Mistral order
- missing sources, if any, appear last
- no Recent Errors section for clean fixture data

**Step 4: Run the empty-source and banner-entry scenarios**

Run:

```bash
mkdir -p /tmp/sessions-chronicle-empty && flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir /tmp/sessions-chronicle-empty
```

Expected:

- summary shows `No sessions found`
- empty sources remain expandable so the resolved path can be copied
- if you inject a malformed fixture or point to bad source data, the banner shows `Details` and clicking it opens the dialog

**Step 5: Capture UI evidence**

Capture screenshots for:

- clean dialog state
- partial/error dialog state with banner visible
- narrow-width adaptive dialog state

Expected: screenshots are ready for the PR description.

**Step 6: Final review and commit only if needed**

Run `@superpowers:requesting-code-review`.

If review or manual verification required code changes, make them, rerun Step 1 and Step 2, then commit with:

```bash
git add src/ui/modals/indexing_status.rs src/app/mod.rs src/app/init.rs src/app/handlers/indexing.rs src/database/indexer.rs src/models/indexing_diagnostics.rs src/models/mod.rs src/indexing_worker.rs src/ui/modals/shortcuts.rs data/resources/style.css
git commit -m "feat: add indexing status dialog"
```

If no files changed after verification, do **not** create an empty commit.

---

## References

- Design source: `docs/plans/2026-03-25-indexing-status-dialog-design.md`
- Existing modal reference: `src/ui/modals/preferences.rs:39`
- Existing shortcuts reference: `src/ui/modals/shortcuts.rs:28`
- Existing indexing completion flow: `src/app/handlers/indexing.rs:38`
- Existing diagnostics models: `src/models/indexing_diagnostics.rs:4`
- Existing diagnostics tests: `src/database/indexer.rs:1708`
- Workflow commands: `docs/DEVELOPMENT_WORKFLOW.md:65`
- Relm4 crate docs: `https://docs.rs/crate/relm4/0.10.0`
- Relm4 book: `https://raw.githubusercontent.com/Relm4/book/refs/heads/main/src/SUMMARY.md`
