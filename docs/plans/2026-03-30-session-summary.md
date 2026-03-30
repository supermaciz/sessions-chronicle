# Session Summary Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the current session-detail metadata card with a structured summary header that shows identity, first prompt, activity, tokens, and ending status before the transcript.

**Architecture:** Keep the change local to the existing session-detail component and shared UI formatting helpers. Build the header from existing `Session` fields only, with no schema changes, and prefer small pure helpers for formatting and proportional-width math so the risky UI bits stay testable.

**Tech Stack:** Rust 2024, Relm4 0.10, GTK4/libadwaita, chrono, cargo test, flatpak-builder

---

## Read This First

Before touching code, read these references in this order:

1. `docs/plans/2026-03-30-session-summary-design.md`
2. `src/ui/session_detail.rs:67-247`
3. `src/ui/session_detail.rs:486-682`
4. `src/ui/format.rs:169-223`
5. `src/ui/analytics_view.rs:406-493`

External references already validated for this plan:

- Relm4 book: component macro reference
  `https://relm4.org/book/stable/component_macro/reference.html`
- Context7 library used for GTK4 Rust API checks:
  `/gtk-rs/gtk4-rs`
- GTK accessibility pattern already used in-tree:
  `src/ui/analytics_heatmap.rs:313-318`
  `src/ui/modals/indexing_status.rs:340-346`

Implementation note: do **not** try to make `gtk::Box` wrap children. GTK4 `Box` is not a wrapping layout. For the chip row and token/value tiles, copy the existing `gtk::FlowBox` pattern from `src/ui/analytics_view.rs:406-493`.

Worktree note: execute this plan from the dedicated worktree prepared via `@superpowers:using-git-worktrees`. Use `@superpowers:test-driven-development` for each task and `@superpowers:verification-before-completion` before claiming success.

---

### Task 1: Add formatter helpers for duration and ending status

**Files:**
- Modify: `src/ui/format.rs:169-223`
- Test: `src/ui/format.rs:225-482`

**Step 1: Write the failing tests**

Add tests near the existing `format_count` and ending-status tests.

```rust
#[test]
fn format_session_duration_shows_hours_and_minutes() {
    use chrono::{TimeZone, Utc};

    let start = Utc.with_ymd_and_hms(2026, 3, 30, 10, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2026, 3, 30, 12, 14, 0).unwrap();

    assert_eq!(format_session_duration(start, end), "2h 14m");
}

#[test]
fn format_session_duration_clamps_negative_ranges_to_zero_minutes() {
    use chrono::{TimeZone, Utc};

    let start = Utc.with_ymd_and_hms(2026, 3, 30, 10, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2026, 3, 30, 9, 59, 0).unwrap();

    assert_eq!(format_session_duration(start, end), "0m");
}

#[test]
fn format_ending_label_covers_all_statuses() {
    use crate::models::SessionEndingStatus;

    assert_eq!(format_ending_label(&SessionEndingStatus::Clean), "Ended cleanly");
    assert_eq!(format_ending_label(&SessionEndingStatus::Abrupt), "Ended unexpectedly");
    assert_eq!(format_ending_label(&SessionEndingStatus::Error), "Ended with error");
    assert_eq!(format_ending_label(&SessionEndingStatus::Unknown), "Ending unknown");
}

#[test]
fn format_ending_accessible_label_covers_all_statuses() {
    use crate::models::SessionEndingStatus;

    assert_eq!(
        format_ending_accessible_label(&SessionEndingStatus::Clean),
        "Session ended cleanly"
    );
    assert_eq!(
        format_ending_accessible_label(&SessionEndingStatus::Abrupt),
        "Session ended unexpectedly"
    );
    assert_eq!(
        format_ending_accessible_label(&SessionEndingStatus::Error),
        "Session ended with error"
    );
    assert_eq!(
        format_ending_accessible_label(&SessionEndingStatus::Unknown),
        "Session ending status unknown"
    );
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test format_session_duration -- --nocapture`
Expected: FAIL because `format_session_duration` does not exist yet.

Run: `cargo test format_ending_label_covers_all_statuses -- --exact`
Expected: FAIL because `format_ending_label` does not exist yet.

**Step 3: Write minimal implementation**

Add these helpers above the existing ending helper block so the detail view and tests can share one source of truth.

```rust
pub fn format_session_duration(start: chrono::DateTime<chrono::Utc>, end: chrono::DateTime<chrono::Utc>) -> String {
    let total_minutes = end.signed_duration_since(start).num_minutes().max(0);
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{total_minutes}m")
    }
}

pub fn format_ending_label(status: &crate::models::SessionEndingStatus) -> &'static str {
    match status {
        crate::models::SessionEndingStatus::Clean => "Ended cleanly",
        crate::models::SessionEndingStatus::Abrupt => "Ended unexpectedly",
        crate::models::SessionEndingStatus::Error => "Ended with error",
        crate::models::SessionEndingStatus::Unknown => "Ending unknown",
    }
}

pub fn format_ending_accessible_label(status: &crate::models::SessionEndingStatus) -> &'static str {
    match status {
        crate::models::SessionEndingStatus::Clean => "Session ended cleanly",
        crate::models::SessionEndingStatus::Abrupt => "Session ended unexpectedly",
        crate::models::SessionEndingStatus::Error => "Session ended with error",
        crate::models::SessionEndingStatus::Unknown => "Session ending status unknown",
    }
}
```

Do not remove `ending_icon_name`, `ending_tooltip`, or `ending_css_class`; the session list still uses them.

**Step 4: Run tests to verify they pass**

Run: `cargo test format_session_duration -- --nocapture`
Expected: PASS.

Run: `cargo test format_ending_ -- --nocapture`
Expected: PASS for the new label/accessibility tests and the existing ending helper tests.

**Step 5: Commit**

```bash
git add src/ui/format.rs
git commit -m "feat: add session summary formatter helpers"
```

---

### Task 2: Add pure activity-bar sizing helpers before touching GTK widgets

**Files:**
- Modify: `src/ui/session_detail.rs:588-682`
- Test: `src/ui/session_detail.rs` (new `#[cfg(test)]` module at end of file)

**Step 1: Write the failing tests**

Add pure unit tests first. Keep them free of GTK widget setup so they stay fast.

```rust
#[test]
fn activity_segment_widths_fill_the_available_width() {
    assert_eq!(activity_segment_widths(14, 9, 3, 260), [140, 90, 30]);
}

#[test]
fn activity_segment_widths_return_zeroes_when_no_activity_exists() {
    assert_eq!(activity_segment_widths(0, 0, 0, 260), [0, 0, 0]);
}

#[test]
fn activity_segment_widths_assign_remainder_to_the_last_visible_segment() {
    assert_eq!(activity_segment_widths(1, 1, 1, 10), [3, 3, 4]);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test activity_segment_widths_ -- --nocapture`
Expected: FAIL because `activity_segment_widths` does not exist yet.

**Step 3: Write minimal implementation**

Add a local helper near `find_item_for_match` so the future UI code can reuse it.

```rust
fn activity_segment_widths(
    edit_count: usize,
    command_count: usize,
    read_count: usize,
    total_width: i32,
) -> [i32; 3] {
    if total_width <= 0 {
        return [0, 0, 0];
    }

    let counts = [edit_count as i32, command_count as i32, read_count as i32];
    let total = counts.iter().sum::<i32>();
    if total == 0 {
        return [0, 0, 0];
    }

    let mut widths = [0, 0, 0];
    let mut used = 0;
    let last_visible = counts.iter().rposition(|count| *count > 0).unwrap();

    for (index, count) in counts.iter().enumerate() {
        if *count == 0 {
            continue;
        }

        let width = if index == last_visible {
            total_width - used
        } else {
            (total_width * *count) / total
        };

        widths[index] = width;
        used += width;
    }

    widths
}
```

Keep this helper private to `session_detail.rs`.

**Step 4: Run tests to verify they pass**

Run: `cargo test activity_segment_widths_ -- --nocapture`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/ui/session_detail.rs
git commit -m "feat: add activity sizing helper for session summary"
```

---

### Task 3: Replace the metadata card with the new summary-header widget tree

**Files:**
- Modify: `src/ui/session_detail.rs:67-247`
- Reference only: `src/ui/analytics_view.rs:406-493`
- Test: `src/ui/session_detail.rs`

**Step 1: Write the failing GTK test**

Add a `#[gtk::test]` that proves the new optional sections start hidden for a session with no `first_prompt` and no `token_usage`.

```rust
#[gtk::test]
fn session_detail_header_hides_optional_sections_when_data_is_missing() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(None, None, 0, 0, 0)),
        search_query: None,
    });

    while gtk::glib::MainContext::default().iteration(false) {}

    let parts = controller.state().get();
    assert!(!parts.widgets.first_prompt_section.is_visible());
    assert!(!parts.widgets.tokens_section.is_visible());
    assert!(parts.widgets.activity_section.is_visible());
}
```

Add a local `build_test_session(...) -> Session` helper in the test module so later GTK tests can reuse it.

**Step 2: Run test to verify it fails**

Run: `cargo test session_detail_header_hides_optional_sections_when_data_is_missing -- --exact`
Expected: FAIL because the named widgets do not exist yet.

**Step 3: Write minimal implementation**

Replace the current metadata card in the `view!` macro with explicit named section containers and separators. Use `gtk::FlowBox` for wrapping chip/tile rows.

Use this structure as the target skeleton:

```rust
#[name = "scroll_child"]
gtk::Box {
    set_orientation: gtk::Orientation::Vertical,
    set_spacing: 12,
    set_margin_all: 16,

    #[name = "project_label"]
    gtk::Label {
        add_css_class: "title-2",
        set_halign: gtk::Align::Start,
        set_wrap: true,
        set_wrap_mode: gtk::pango::WrapMode::WordChar,
    },

    #[name = "path_label"]
    gtk::Label {
        add_css_class: "dim-label",
        set_halign: gtk::Align::Start,
        set_wrap: true,
        set_wrap_mode: gtk::pango::WrapMode::WordChar,
        set_selectable: true,
    },

    #[name = "session_id_row"]
    gtk::Box {
        set_orientation: gtk::Orientation::Horizontal,
        set_spacing: 6,

        gtk::Label {
            set_label: "Session ID:",
            add_css_class: "dim-label",
        },

        #[name = "session_id_label"]
        gtk::Label {
            add_css_class: "monospace",
            set_selectable: true,
            set_wrap: true,
            set_wrap_mode: gtk::pango::WrapMode::WordChar,
        },
    },

    #[name = "chip_row"]
    gtk::FlowBox {
        set_selection_mode: gtk::SelectionMode::None,
        set_row_spacing: 8,
        set_column_spacing: 8,
        set_max_children_per_line: 4,
        set_min_children_per_line: 1,

        append = &gtk::Box {
            set_orientation: gtk::Orientation::Horizontal,
            set_spacing: 6,
            add_css_class: "pill",

            #[name = "tool_icon"]
            gtk::Image {
                set_pixel_size: 16,
            },

            #[name = "tool_label"]
            gtk::Label {},
        },

        append = &gtk::Label {
            #[name = "duration_chip"]
            add_css_class: "pill",
        },

        append = &gtk::Label {
            #[name = "message_count_chip"]
            add_css_class: "pill",
        },

        append = &gtk::Label {
            #[name = "ending_status_chip"]
            add_css_class: "pill",
        },
    },

    #[name = "first_prompt_separator"]
    gtk::Separator {},

    #[name = "first_prompt_section"]
    gtk::Box {
        set_orientation: gtk::Orientation::Vertical,
        set_spacing: 4,

        gtk::Label {
            set_label: "FIRST PROMPT",
            add_css_class: "section-heading",
            set_halign: gtk::Align::Start,
        },

        #[name = "first_prompt_label"]
        gtk::Label {
            set_halign: gtk::Align::Start,
            set_xalign: 0.0,
            set_wrap: true,
            set_wrap_mode: gtk::pango::WrapMode::WordChar,
            set_lines: 3,
            set_ellipsize: gtk::pango::EllipsizeMode::End,
            set_max_width_chars: 80,
        },
    },

    #[name = "activity_separator"]
    gtk::Separator {},

    #[name = "activity_section"]
    gtk::Box {
        set_orientation: gtk::Orientation::Vertical,
        set_spacing: 8,
    },

    #[name = "tokens_separator"]
    gtk::Separator {},

    #[name = "tokens_section"]
    gtk::Box {
        set_orientation: gtk::Orientation::Vertical,
        set_spacing: 8,
    },

    #[name = "transcript_separator"]
    gtk::Separator {},

    #[local_ref]
    messages_box -> gtk::Box {
        set_orientation: gtk::Orientation::Vertical,
        set_spacing: 8,
    },
}
```

Inside `activity_section`, add named children for `activity_bar`, `conversation_only_label`, and a wrapping legend row. Inside `tokens_section`, add a `gtk::FlowBox` named `tokens_grid` plus four named value labels and four named pair boxes so cache/reasoning can hide independently.

**Step 4: Run test to verify it passes**

Run: `cargo test session_detail_header_hides_optional_sections_when_data_is_missing -- --exact`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/ui/session_detail.rs
git commit -m "feat: add session detail summary header layout"
```

---

### Task 4: Bind session data into the new header sections

**Files:**
- Modify: `src/ui/session_detail.rs:486-547`
- Modify: `src/ui/session_detail.rs:588-682`
- Modify: `src/ui/format.rs:169-223`
- Test: `src/ui/session_detail.rs`

**Step 1: Write the failing GTK tests**

Add focused tests for the populated state and the conversation-only fallback.

```rust
#[gtk::test]
fn session_detail_header_populates_identity_prompt_and_tokens() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(
            Some("Ship the summary header"),
            Some(crate::models::TokenUsage {
                input_tokens: 1200,
                output_tokens: 300,
                cache_read_tokens: Some(400),
                cache_write_tokens: None,
                reasoning_tokens: Some(50),
            }),
            4,
            2,
            1,
        )),
        search_query: None,
    });

    while gtk::glib::MainContext::default().iteration(false) {}

    let parts = controller.state().get();
    assert_eq!(parts.widgets.project_label.label(), "project");
    assert_eq!(parts.widgets.duration_chip.label(), "2h 14m");
    assert_eq!(parts.widgets.message_count_chip.label(), "42 messages");
    assert_eq!(parts.widgets.ending_status_chip.label(), "Ended cleanly");
    assert!(parts.widgets.first_prompt_section.is_visible());
    assert!(parts.widgets.tokens_section.is_visible());
}

#[gtk::test]
fn session_detail_header_uses_conversation_only_when_activity_counts_are_zero() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_test_session(Some("Only chat"), None, 0, 0, 0)),
        search_query: None,
    });

    while gtk::glib::MainContext::default().iteration(false) {}

    let parts = controller.state().get();
    assert!(parts.widgets.conversation_only_label.is_visible());
    assert!(!parts.widgets.activity_bar.is_visible());
}
```

Make `build_test_session` deterministic. Set `project_path` to `/tmp/project`, `message_count` to `42`, `start_time` to `2026-03-30T10:00:00Z`, and `last_updated` to `2026-03-30T12:14:00Z` so the duration assertion stays stable.

**Step 2: Run tests to verify they fail**

Run: `cargo test session_detail_header_populates_identity_prompt_and_tokens -- --exact`
Expected: FAIL because the new widgets are not populated yet.

Run: `cargo test session_detail_header_uses_conversation_only_when_activity_counts_are_zero -- --exact`
Expected: FAIL because the activity fallback logic does not exist yet.

**Step 3: Write minimal implementation**

Keep `post_view()` readable by moving repeated section logic into tiny private helpers if needed, for example `update_first_prompt_section`, `update_activity_section`, and `update_tokens_section`.

Core binding logic should look like this:

```rust
widgets.project_label.set_label(project_name);
widgets.path_label.set_label(path);
widgets.session_id_label.set_label(&session.id);

widgets.tool_icon.set_icon_name(Some(session.tool.icon_name()));
widgets.tool_label.set_label(session.tool.display_name());
widgets.duration_chip.set_label(&crate::ui::format::format_session_duration(
    session.start_time,
    session.last_updated,
));
widgets.message_count_chip.set_label(&crate::ui::format::format_count(
    session.message_count,
    "message",
    "messages",
));
widgets.ending_status_chip.set_label(crate::ui::format::format_ending_label(&session.ending_status));
widgets.ending_status_chip.set_css_classes(&[
    "pill",
    crate::ui::format::ending_css_class(&session.ending_status),
]);
widgets.ending_status_chip.update_property(&[
    gtk::accessible::Property::Label(
        crate::ui::format::format_ending_accessible_label(&session.ending_status),
    ),
]);
```

For activity, do all of the following:

```rust
let widths = activity_segment_widths(
    session.edit_count,
    session.command_count,
    session.read_count,
    widgets.activity_bar.allocated_width(),
);

widgets.edit_segment.set_size_request(widths[0], 8);
widgets.command_segment.set_size_request(widths[1], 8);
widgets.read_segment.set_size_request(widths[2], 8);

widgets.activity_bar.update_property(&[
    gtk::accessible::Property::Label(&format!(
        "Activity: {}, {}, {}",
        crate::ui::format::format_count(session.edit_count, "edit", "edits"),
        crate::ui::format::format_count(session.command_count, "command", "commands"),
        crate::ui::format::format_count(session.read_count, "read", "reads"),
    )),
]);
```

For tokens, populate each value label with `format_token_count(...)`, hide the cache/reasoning pair boxes when their values are `None`, and set the section tooltip with `token_semantics_help_tooltip()`.

For the first prompt, trim the string and treat blank content as missing.

Do not remove or change transcript loading logic.

**Step 4: Run tests to verify they pass**

Run: `cargo test session_detail_header_ -- --nocapture`
Expected: PASS for the new GTK header tests.

Run: `cargo test format_ending_ -- --nocapture`
Expected: PASS so the detail view still matches the shared format helpers.

**Step 5: Commit**

```bash
git add src/ui/session_detail.rs src/ui/format.rs
git commit -m "feat: populate session detail summary sections"
```

---

### Task 5: Add CSS for the flush summary header and remove card-specific styling from the detail view

**Files:**
- Modify: `data/resources/style.css:51-57`
- Modify: `data/resources/style.css:174-186`
- Modify: `data/resources/style.css` (append new summary classes nearby)
- Test: `src/ui/session_detail.rs`

**Step 1: Write the failing GTK style smoke test**

Add a small GTK test that checks the ending chip and activity bar pick up the expected classes after `SetSession`.

```rust
#[gtk::test]
fn session_detail_header_applies_status_and_activity_css_classes() {
    let temp_db = tempfile::NamedTempFile::new().expect("temp db");
    let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

    controller.emit(SessionDetailMsg::SetSession {
        session: Box::new(build_error_session_for_css_test()),
        search_query: None,
    });

    while gtk::glib::MainContext::default().iteration(false) {}

    let parts = controller.state().get();
    assert!(parts.widgets.ending_status_chip.has_css_class("pill"));
    assert!(parts.widgets.ending_status_chip.has_css_class("ending-failed"));
    assert!(parts.widgets.activity_bar.has_css_class("activity-bar"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test session_detail_header_applies_status_and_activity_css_classes -- --exact`
Expected: FAIL until the new classes are assigned consistently.

**Step 3: Write minimal implementation**

Update `data/resources/style.css` with the exact classes from the design doc, keeping the existing `.ending-interrupted` and `.ending-failed` classes intact.

```css
.section-heading {
  font-size: 0.75rem;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  opacity: 0.55;
}

.pill {
  padding: 2px 10px;
  border-radius: 99px;
  background-color: alpha(@card_bg_color, 0.6);
}

.ending-clean {
  color: #26a269;
}

.activity-bar {
  min-height: 8px;
  border-radius: 4px;
  overflow: hidden;
}

.activity-edits { background-color: #e66100; }
.activity-commands { background-color: #26a269; }
.activity-reads { background-color: #3584e4; }

.token-value {
  font-size: 1.1rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
```

Important: do **not** delete the global `.card` class, because transcript/message styling still uses card-like visuals elsewhere. Just stop using `.card` in `session_detail.rs`.

**Step 4: Run test to verify it passes**

Run: `cargo test session_detail_header_applies_status_and_activity_css_classes -- --exact`
Expected: PASS.

Run: `cargo fmt --all -- --check`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/ui/session_detail.rs data/resources/style.css
git commit -m "feat: style the session summary header"
```

---

### Task 6: Run full verification, exercise fixture data, and refresh the screenshot

**Files:**
- Verify: `src/ui/session_detail.rs`
- Verify: `src/ui/format.rs`
- Verify: `data/resources/style.css`
- Update: `docs/screenshots/session_detail.png`

**Step 1: Run the focused test suites first**

Run: `cargo test session_detail_header_ -- --nocapture`
Expected: PASS.

Run: `cargo test format_session_duration -- --nocapture`
Expected: PASS.

Run: `cargo test activity_segment_widths_ -- --nocapture`
Expected: PASS.

**Step 2: Run repository-level verification**

Run: `cargo fmt --all -- --check && cargo clippy --all -- -D warnings && cargo test --all --no-fail-fast`
Expected: PASS for all three commands.

**Step 3: Run the app with fixture data for manual QA**

Run: `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures`
Expected: The app launches with fixture-backed sessions.

Manual checks in the running app:

- Open one session with `first_prompt` and token data.
- Open one session with zero edit/command/read counts and confirm `Conversation only`.
- Open sessions with clean, abrupt, error, and unknown ending status.
- Narrow the window below ~600px and confirm the chip row and token tiles wrap cleanly.
- Verify the first prompt hides completely when blank.
- Verify the transcript still starts immediately after the summary header and separators.
- Verify the summary header stays visually under roughly 300px tall for ordinary sessions.
- Verify semantic colors remain readable in dark mode and high contrast.

**Step 4: Refresh the screenshot**

Overwrite `docs/screenshots/session_detail.png` with a new capture of the updated summary header using the fixture-backed app.

**Step 5: Commit**

```bash
git add docs/screenshots/session_detail.png src/ui/session_detail.rs src/ui/format.rs data/resources/style.css
git commit -m "feat: add structured session summary header"
```
