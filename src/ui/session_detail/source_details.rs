use chrono::{DateTime, Utc};
use gtk::prelude::*;
use relm4::{RelmWidgetExt, adw, gtk};

use crate::models::{Session, SourceKind};

/// A value a Source details page offers for one-tap copying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopyValue {
    pub(crate) label: &'static str,
    pub(crate) value: String,
}

/// A labelled evidence value plus the optional copy action attached to it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EvidenceRow {
    label: &'static str,
    value: String,
    copy: Option<CopyValue>,
    /// Accessible description for the copy control, when `copy` is present.
    copy_description: Option<&'static str>,
}

/// The copy controls offered for a session, ordered for display. Database
/// sources copy their owning database path and native session identity; every
/// other storage shape with a recorded locator copies that locator.
fn source_copy_values(session: &Session) -> Vec<CopyValue> {
    if session.source.kind == Some(SourceKind::DatabaseRecord) {
        let mut values = Vec::new();
        if let Some(path) = session.source.scope.as_ref().filter(|p| !p.is_empty()) {
            values.push(CopyValue {
                label: "Copy path",
                value: path.clone(),
            });
        }
        values.push(CopyValue {
            label: "Copy session ID",
            value: session.id.clone(),
        });
        values
    } else if session.file_path.is_empty() {
        Vec::new()
    } else {
        vec![CopyValue {
            label: "Copy path",
            value: session.file_path.clone(),
        }]
    }
}

/// The retained-content explanation shown at the top of a Source details page.
fn retained_content_text(has_indexed_items: bool) -> &'static str {
    if has_indexed_items {
        "Only indexed content is available"
    } else {
        "No retained transcript"
    }
}

/// Renders an optional observation time, leaving values that were never
/// observed (or cannot be represented) as `Unknown`.
fn format_observed_time(instant: Option<DateTime<Utc>>) -> String {
    instant
        .map(|when| when.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Renders a byte count. A recorded zero is `0`, not `Unknown`.
fn format_bytes(size: Option<i64>) -> String {
    size.map(|bytes| bytes.to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Renders a nanosecond modification timestamp with a checked conversion.
/// Out-of-range or otherwise unrepresentable values render as `Unknown`.
fn format_mtime_ns(mtime_ns: Option<i64>) -> String {
    let Some(mtime_ns) = mtime_ns else {
        return "Unknown".to_string();
    };
    let seconds = mtime_ns.div_euclid(1_000_000_000);
    let subsec_nanos = mtime_ns.rem_euclid(1_000_000_000) as u32;
    match DateTime::from_timestamp(seconds, subsec_nanos) {
        Some(when) => format_observed_time(Some(when)),
        None => "Unknown".to_string(),
    }
}

/// Human-readable label for a recorded storage kind.
fn source_kind_label(kind: Option<SourceKind>) -> &'static str {
    match kind {
        Some(SourceKind::TranscriptFile) => "Transcript file",
        Some(SourceKind::SessionDirectory) => "Session directory",
        Some(SourceKind::SessionBundle) => "Session bundle",
        Some(SourceKind::DatabaseRecord) => "Database record",
        None => "Unknown",
    }
}

/// Builds the labelled evidence rows shown on a Source details page.
fn source_evidence_rows(session: &Session) -> Vec<EvidenceRow> {
    let mut rows = Vec::new();
    rows.push(EvidenceRow {
        label: "Absence detected",
        value: format_observed_time(session.source.missing_detected_at),
        copy: None,
        copy_description: None,
    });
    rows.push(EvidenceRow {
        label: "Last seen",
        value: format_observed_time(session.source.last_seen_at),
        copy: None,
        copy_description: None,
    });
    rows.push(EvidenceRow {
        label: "Storage",
        value: source_kind_label(session.source.kind).to_string(),
        copy: None,
        copy_description: None,
    });

    let is_database = session.source.kind == Some(SourceKind::DatabaseRecord);
    let copies = source_copy_values(session);
    if is_database {
        if let Some(path) = session.source.scope.as_ref().filter(|p| !p.is_empty()) {
            rows.push(EvidenceRow {
                label: "Database path",
                value: path.clone(),
                copy: copies.iter().find(|c| c.value == *path).cloned(),
                copy_description: Some("Copy database path"),
            });
        }
        rows.push(EvidenceRow {
            label: "Session ID",
            value: session.id.clone(),
            copy: copies.iter().find(|c| c.value == session.id).cloned(),
            copy_description: Some("Copy session ID"),
        });
    } else if !session.file_path.is_empty() {
        rows.push(EvidenceRow {
            label: "Path",
            value: session.file_path.clone(),
            copy: copies.first().cloned(),
            copy_description: Some("Copy source path"),
        });
    }

    // Database records never carry a session-level file size or modification
    // time; only file-shaped sources may.
    if !is_database {
        rows.push(EvidenceRow {
            label: "Size",
            value: format_bytes(session.source.size),
            copy: None,
            copy_description: None,
        });
        rows.push(EvidenceRow {
            label: "Last modified",
            value: format_mtime_ns(session.source.mtime_ns),
            copy: None,
            copy_description: None,
        });
    }

    rows
}

/// Builds the scrollable evidence content shared by Source details pages.
pub(crate) fn source_details_content(
    session: &Session,
    has_indexed_items: bool,
) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_vexpand(true);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_all(16);
    content.set_vexpand(true);

    let explanation = gtk::Label::new(Some(retained_content_text(has_indexed_items)));
    explanation.add_css_class("title-2");
    explanation.set_halign(gtk::Align::Start);
    explanation.set_wrap(true);
    explanation.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    content.append(&explanation);

    for row in source_evidence_rows(session) {
        content.append(&evidence_row_widget(&row));
    }

    scroller.set_child(Some(&content));
    scroller
}

fn evidence_row_widget(row: &EvidenceRow) -> gtk::Box {
    let boxed = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    boxed.set_halign(gtk::Align::Fill);

    let label = gtk::Label::new(Some(row.label));
    label.add_css_class("dim-label");
    label.set_halign(gtk::Align::Start);
    label.set_valign(gtk::Align::Center);
    boxed.append(&label);

    let value = gtk::Label::new(Some(&row.value));
    value.set_halign(gtk::Align::Start);
    value.set_valign(gtk::Align::Center);
    value.set_selectable(true);
    value.set_wrap(true);
    value.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    value.set_hexpand(true);
    boxed.append(&value);

    if let (Some(copy), Some(description)) = (&row.copy, row.copy_description) {
        let copy_value = copy.value.clone();
        let button = gtk::Button::builder().label(copy.label).build();
        button.update_property(&[gtk::accessible::Property::Description(description)]);
        button.connect_clicked(move |button| {
            button.clipboard().set_text(&copy_value);
        });
        button.set_valign(gtk::Align::Center);
        boxed.append(&button);
    }

    boxed
}

/// Builds the `Source details` navigation page for a retained session.
pub(crate) fn build_source_details_page(
    session: &Session,
    has_indexed_items: bool,
) -> adw::NavigationPage {
    let content = source_details_content(session, has_indexed_items);
    adw::NavigationPage::builder()
        .title("Source details")
        .tag("source-details")
        .child(&content)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn test_session() -> Session {
        use crate::models::SessionEndingStatus;
        Session {
            id: "session_abc".to_string(),
            tool: crate::models::AiAssistant::OpenCode,
            project_path: Some("/tmp/project".to_string()),
            project_id: None,
            start_time: Utc::now(),
            message_count: 1,
            file_path: "/tmp/opencode/session.jsonl".to_string(),
            last_updated: Utc::now(),
            pinned_at: None,
            first_prompt: Some("Prompt".to_string()),
            parent_session_id: None,
            is_subagent: false,
            token_usage: None,
            edit_count: 0,
            read_count: 0,
            command_count: 0,
            ending_status: SessionEndingStatus::Unknown,
            source: Default::default(),
        }
    }

    #[test]
    fn retained_content_text_matches_exact_copy() {
        assert_eq!(
            retained_content_text(true),
            "Only indexed content is available"
        );
        assert_eq!(retained_content_text(false), "No retained transcript");
    }

    #[test]
    fn transcript_source_copies_its_file_path() {
        let mut session = test_session();
        session.source.kind = Some(SourceKind::TranscriptFile);
        let values = source_copy_values(&session);
        assert_eq!(
            values,
            vec![CopyValue {
                label: "Copy path",
                value: "/tmp/opencode/session.jsonl".to_string(),
            }]
        );
    }

    #[test]
    fn database_source_copies_database_path_and_native_id() {
        let mut session = test_session();
        session.source.kind = Some(SourceKind::DatabaseRecord);
        session.source.scope = Some("/tmp/opencode.db".into());
        let values = source_copy_values(&session);
        assert_eq!(values.len(), 2);
        assert_eq!(
            values[0],
            CopyValue {
                label: "Copy path",
                value: "/tmp/opencode.db".into(),
            }
        );
        assert_eq!(
            values[1],
            CopyValue {
                label: "Copy session ID",
                value: session.id.clone(),
            }
        );
    }

    #[test]
    fn directory_and_bundle_sources_copy_their_locator() {
        for kind in [SourceKind::SessionDirectory, SourceKind::SessionBundle] {
            let mut session = test_session();
            session.source.kind = Some(kind);
            let values = source_copy_values(&session);
            assert_eq!(
                values,
                vec![CopyValue {
                    label: "Copy path",
                    value: "/tmp/opencode/session.jsonl".to_string(),
                }]
            );
        }
    }

    #[test]
    fn empty_locator_yields_no_copy_values() {
        let mut session = test_session();
        session.file_path.clear();
        session.source.kind = Some(SourceKind::TranscriptFile);
        assert!(source_copy_values(&session).is_empty());
    }

    #[test]
    fn null_evidence_renders_unknown() {
        assert_eq!(format_observed_time(None), "Unknown");
        assert_eq!(format_bytes(None), "Unknown");
        assert_eq!(format_mtime_ns(None), "Unknown");
        assert_eq!(source_kind_label(None), "Unknown");
    }

    #[test]
    fn zero_byte_evidence_renders_zero_not_unknown() {
        assert_eq!(format_bytes(Some(0)), "0");
    }

    #[test]
    fn database_rows_never_expose_size_or_mtime() {
        let mut session = test_session();
        session.source.kind = Some(SourceKind::DatabaseRecord);
        session.source.scope = Some("/tmp/opencode.db".into());
        session.source.size = Some(2048);
        session.source.mtime_ns = Some(1_700_000_000_000_000_000);
        let labels: Vec<&str> = source_evidence_rows(&session)
            .iter()
            .map(|row| row.label)
            .collect();
        assert!(labels.contains(&"Database path"));
        assert!(labels.contains(&"Session ID"));
        assert!(!labels.contains(&"Size"));
        assert!(!labels.contains(&"Last modified"));
    }

    #[test]
    fn mtime_ns_renders_checked_datetime() {
        // 1_700_000_000_000_000_000 ns == 2023-11-14 22:13:20 UTC.
        assert_eq!(
            format_mtime_ns(Some(1_700_000_000_000_000_000)),
            "2023-11-14 22:13:20 UTC"
        );
    }

    #[test]
    fn markup_characters_in_paths_stay_plain() {
        let mut session = test_session();
        session.source.kind = Some(SourceKind::TranscriptFile);
        session.file_path = "/tmp/A&B<demo>/session.jsonl".to_string();
        let values = source_copy_values(&session);
        assert_eq!(values[0].value, "/tmp/A&B<demo>/session.jsonl");
    }

    #[test]
    fn rows_expose_recorded_observation_times() {
        let mut session = test_session();
        let detected = Utc.with_ymd_and_hms(2026, 9, 7, 10, 30, 0).unwrap();
        session.source.kind = Some(SourceKind::TranscriptFile);
        session.source.missing_detected_at = Some(detected);
        let rows = source_evidence_rows(&session);
        assert!(rows.iter().any(|row| {
            row.label == "Absence detected" && row.value == "2026-09-07 10:30:00 UTC"
        }));
    }

    fn find_copy_button(root: &gtk::Widget, label: &str) -> Option<gtk::Button> {
        use gtk::prelude::*;
        if let Ok(button) = root.clone().downcast::<gtk::Button>()
            && button.label().as_deref() == Some(label)
        {
            return Some(button);
        }
        let mut child = root.first_child();
        while let Some(child_widget) = child {
            if let Some(found) = find_copy_button(&child_widget, label) {
                return Some(found);
            }
            child = child_widget.next_sibling();
        }
        None
    }

    /// Pumps the main context until `condition` holds or `timeout` elapses.
    /// Nonblocking iterations keep the deadline effective when no event arrives.
    fn pump_until(timeout: std::time::Duration, condition: impl Fn() -> bool) -> bool {
        let context = gtk::glib::MainContext::default();
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            context.iteration(false);
            if condition() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        condition()
    }

    #[gtk::test]
    fn copy_buttons_place_the_exact_full_string_on_the_clipboard() {
        use gtk::glib::prelude::ObjectExt;
        use gtk::prelude::*;

        let mut session = test_session();
        session.source.kind = Some(SourceKind::DatabaseRecord);
        session.source.scope = Some("/tmp/database&A&B.db".into());
        let content = source_details_content(&session, false);
        let content_widget = content.clone().upcast::<gtk::Widget>();
        let window = gtk::Window::new();
        window.set_child(Some(&content));
        window.present();

        pump_until(std::time::Duration::from_millis(800), || false);

        let path_button = find_copy_button(&content_widget, "Copy path").expect("path copy button");
        path_button.emit_by_name::<()>("clicked", &[]);
        let copied = std::rc::Rc::new(std::cell::RefCell::new(None::<String>));
        {
            let copied = copied.clone();
            let clipboard = path_button.clipboard();
            let cancellable: Option<&gtk::gio::Cancellable> = None;
            clipboard.read_text_async(cancellable, move |result| {
                let text: Result<Option<gtk::glib::GString>, gtk::glib::Error> = result;
                *copied.borrow_mut() = text.ok().flatten().map(|s| s.to_string());
            });
        }
        let copied_ref = copied.clone();
        assert!(
            pump_until(std::time::Duration::from_millis(800), || {
                copied_ref.borrow().is_some()
            }),
            "clipboard path read did not return text before the deadline"
        );
        assert_eq!(
            copied.borrow().as_deref(),
            Some("/tmp/database&A&B.db"),
            "copy must place the exact full path, never an ellipsized label"
        );

        let id_button =
            find_copy_button(&content_widget, "Copy session ID").expect("session id copy button");
        id_button.emit_by_name::<()>("clicked", &[]);
        let copied_id = std::rc::Rc::new(std::cell::RefCell::new(None::<String>));
        {
            let copied_id = copied_id.clone();
            let clipboard = id_button.clipboard();
            let cancellable: Option<&gtk::gio::Cancellable> = None;
            clipboard.read_text_async(cancellable, move |result| {
                let text: Result<Option<gtk::glib::GString>, gtk::glib::Error> = result;
                *copied_id.borrow_mut() = text.ok().flatten().map(|s| s.to_string());
            });
        }
        let copied_id_ref = copied_id.clone();
        assert!(pump_until(std::time::Duration::from_millis(800), || {
            copied_id_ref.borrow().is_some()
        }));
        assert_eq!(copied_id.borrow().as_deref(), Some(session.id.as_str()));

        window.destroy();
    }
}
