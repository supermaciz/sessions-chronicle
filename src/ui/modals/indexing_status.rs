use crate::models::{AiAssistant, PerSourceResult, SourceStatus};
use adw::prelude::{
    ActionRowExt, AdwDialogExt, ExpanderRowExt, PreferencesGroupExt, PreferencesRowExt,
};
use gtk::prelude::{BoxExt, ButtonExt, DisplayExt, WidgetExt};
use relm4::gtk::glib;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};

pub struct IndexingStatusDialog {
    summary_state: SummaryState,
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

pub struct IndexingStatusWidgets {
    pub summary_row: adw::ActionRow,
    pub summary_icon: gtk::Image,
    pub progress_bar: gtk::ProgressBar,
    pub sources_group: adw::PreferencesGroup,
    pub reindex_button: gtk::Button,
    source_rows: Vec<adw::ExpanderRow>,
}

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
            format!(
                "<tt>{}</tt>",
                glib::markup_escape_text(&result.display_path)
            )
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

impl SimpleComponent for IndexingStatusDialog {
    type Init = ();
    type Widgets = IndexingStatusWidgets;
    type Input = IndexingStatusMsg;
    type Output = IndexingStatusOutput;
    type Root = adw::Dialog;

    fn init_root() -> Self::Root {
        adw::Dialog::builder().build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        root.set_title("Indexing Status");
        root.set_content_width(480);
        root.set_content_height(520);
        root.set_follows_content_size(true);

        let toolbar_view = adw::ToolbarView::new();
        let header_bar = adw::HeaderBar::new();
        let reindex_button = gtk::Button::builder()
            .label("Re-index")
            .css_classes(["suggested-action"])
            .build();

        let input_sender = sender.input_sender().clone();
        reindex_button.connect_clicked(move |_| {
            input_sender.send(IndexingStatusMsg::ReindexRequested).ok();
        });

        header_bar.pack_end(&reindex_button);
        toolbar_view.add_top_bar(&header_bar);

        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_hscrollbar_policy(gtk::PolicyType::Never);

        let clamp = adw::Clamp::new();
        clamp.set_maximum_size(440);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        content.set_margin_start(16);
        content.set_margin_end(16);

        let summary_group = adw::PreferencesGroup::new();
        let summary_row = adw::ActionRow::builder().activatable(false).build();
        let summary_icon = gtk::Image::new();
        summary_row.add_prefix(&summary_icon);
        summary_group.add(&summary_row);

        let progress_bar = gtk::ProgressBar::builder().pulse_step(0.2).build();

        let sources_group = adw::PreferencesGroup::builder().title("Sources").build();

        content.append(&summary_group);
        content.append(&progress_bar);
        content.append(&sources_group);

        clamp.set_child(Some(&content));
        scrolled.set_child(Some(&clamp));
        toolbar_view.set_content(Some(&scrolled));
        root.set_child(Some(&toolbar_view));

        let model = Self {
            summary_state: derive_summary_state(&[], false),
            source_rows: Vec::new(),
            indexing: false,
        };

        let mut widgets = IndexingStatusWidgets {
            summary_row,
            summary_icon,
            progress_bar,
            sources_group,
            reindex_button,
            source_rows: Vec::new(),
        };

        model.sync_widgets(&mut widgets);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            IndexingStatusMsg::Update {
                per_source,
                indexing,
            } => {
                self.indexing = indexing;
                self.summary_state = derive_summary_state(&per_source, indexing);
                self.source_rows = build_source_rows(&per_source);
            }
            IndexingStatusMsg::ReindexRequested => {
                sender.output(IndexingStatusOutput::Reindex).ok();
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        self.sync_widgets(widgets);
    }
}

impl IndexingStatusDialog {
    fn sync_widgets(&self, widgets: &mut IndexingStatusWidgets) {
        widgets.summary_row.set_title(&self.summary_state.title);
        widgets
            .summary_row
            .set_subtitle(self.summary_state.subtitle.as_deref().unwrap_or(""));
        widgets
            .summary_icon
            .set_icon_name(Some(self.summary_state.icon_name));

        widgets.progress_bar.set_visible(self.indexing);
        if self.indexing {
            widgets.progress_bar.pulse();
        }

        widgets.reindex_button.set_sensitive(!self.indexing);
        widgets.reindex_button.set_label(if self.indexing {
            "Indexing..."
        } else {
            "Re-index"
        });

        widgets
            .sources_group
            .set_visible(!self.source_rows.is_empty());
        self.rebuild_source_rows(widgets);
    }

    fn rebuild_source_rows(&self, widgets: &mut IndexingStatusWidgets) {
        for existing_row in widgets.source_rows.drain(..) {
            widgets.sources_group.remove(&existing_row);
        }

        for row in &self.source_rows {
            let source_row = adw::ExpanderRow::builder()
                .title(row.assistant.display_name())
                .subtitle(&row.subtitle_markup)
                .build();
            source_row.set_enable_expansion(row.expandable);

            let pill = gtk::Label::new(Some(&row.badge_text));
            pill.add_css_class("source-count-pill");
            pill.add_css_class(row.badge_css_class);
            source_row.add_suffix(&pill);

            let source_path_row = adw::ActionRow::builder()
                .title("Source Path")
                .subtitle(&row.subtitle_markup)
                .subtitle_lines(1)
                .activatable(false)
                .build();

            let display_path = row.display_path.clone();
            let copy_button = gtk::Button::builder()
                .icon_name("edit-copy-symbolic")
                .tooltip_text("Copy path to clipboard")
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            copy_button.connect_clicked(move |button| {
                let clipboard = button.display().clipboard();
                clipboard.set_text(&display_path);
            });
            source_path_row.add_suffix(&copy_button);
            source_row.add_row(&source_path_row);

            source_row.add_row(&Self::stat_row("Sessions Indexed", row.indexed));

            if row.skipped > 0 {
                source_row.add_row(&Self::stat_row("Skipped", row.skipped));
            }

            if row.errors > 0 {
                let parse_errors_row = adw::ActionRow::builder()
                    .title("Parse Errors")
                    .activatable(false)
                    .build();
                let parse_errors_label = gtk::Label::new(Some(&row.errors.to_string()));
                parse_errors_label.add_css_class("warning");
                parse_errors_label.add_css_class("numeric");
                parse_errors_row.add_suffix(&parse_errors_label);
                source_row.add_row(&parse_errors_row);
            }

            widgets.sources_group.add(&source_row);
            widgets.source_rows.push(source_row);
        }
    }

    fn stat_row(title: &str, value: usize) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(title)
            .activatable(false)
            .build();
        let value_label = gtk::Label::new(Some(&value.to_string()));
        value_label.add_css_class("numeric");
        row.add_suffix(&value_label);
        row
    }
}

fn derive_summary_state(results: &[PerSourceResult], indexing: bool) -> SummaryState {
    if indexing {
        return SummaryState::new("Indexing in progress...", None, "content-loading-symbolic");
    }

    if results.is_empty() {
        return SummaryState::new("Not yet indexed", None, "content-loading-symbolic");
    }

    let indexed_total: usize = results.iter().map(|result| result.indexed).sum();
    let skipped_total: usize = results.iter().map(|result| result.skipped).sum();
    let errors_total: usize = results.iter().map(|result| result.errors).sum();
    let no_detected_sources = results
        .iter()
        .all(|result| matches!(result.status, SourceStatus::NotFound));
    let empty_sources = results
        .iter()
        .any(|result| matches!(result.status, SourceStatus::Empty))
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
        let missing_rank = row.badge_text == "N/A";
        let assistant_rank = AiAssistant::ALL
            .iter()
            .position(|assistant| *assistant == row.assistant)
            .unwrap_or(usize::MAX);

        (missing_rank, assistant_rank)
    });

    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use adw::prelude::PreferencesRowExt;
    use gtk::prelude::WidgetExt;
    use relm4::{Component, ComponentController};
    use std::time::Duration;

    fn pump_main_context(condition: impl Fn() -> bool) {
        let context = gtk::glib::MainContext::default();
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        while std::time::Instant::now() < deadline {
            if condition() {
                return;
            }

            if !context.iteration(false) {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

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

        assert_eq!(
            derive_summary_state(
                &[make_result(
                    AiAssistant::ClaudeCode,
                    SourceStatus::Degraded,
                    8,
                    0,
                    2,
                    "/tmp/claude",
                )],
                false,
            ),
            SummaryState::new(
                "8 sessions indexed",
                Some("Completed with 2 errors"),
                "dialog-warning-symbolic",
            )
        );

        assert_eq!(
            derive_summary_state(
                &[make_result(
                    AiAssistant::OpenCode,
                    SourceStatus::Empty,
                    0,
                    0,
                    0,
                    "/tmp/opencode",
                )],
                false,
            ),
            SummaryState::new(
                "No sessions found",
                Some("Session sources detected, but no sessions were found"),
                "dialog-warning-symbolic",
            )
        );

        assert_eq!(
            derive_summary_state(
                &[make_result(
                    AiAssistant::Codex,
                    SourceStatus::NotFound,
                    0,
                    0,
                    0,
                    "/missing/codex",
                )],
                false,
            ),
            SummaryState::new(
                "No sessions found",
                Some("No session sources detected"),
                "dialog-warning-symbolic",
            )
        );
    }

    #[test]
    fn indexing_status_orders_not_found_sources_last() {
        let rows = build_source_rows(&[
            make_result(
                AiAssistant::Codex,
                SourceStatus::NotFound,
                0,
                0,
                0,
                "/missing/codex",
            ),
            make_result(
                AiAssistant::OpenCode,
                SourceStatus::Empty,
                0,
                0,
                0,
                "/tmp/opencode",
            ),
            make_result(
                AiAssistant::ClaudeCode,
                SourceStatus::Indexed,
                4,
                0,
                0,
                "/tmp/claude",
            ),
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

    #[gtk::test]
    fn indexing_status_dialog_hides_sources_before_first_index() {
        let controller = IndexingStatusDialog::builder().launch(());
        let parts = controller.state().get();

        assert!(!parts.widgets.sources_group.is_visible());
        assert_eq!(parts.widgets.summary_row.title(), "Not yet indexed");
    }

    #[gtk::test]
    fn indexing_status_dialog_disables_reindex_while_indexing_progress_visible() {
        let controller = IndexingStatusDialog::builder().launch(());
        controller.emit(IndexingStatusMsg::Update {
            per_source: vec![],
            indexing: true,
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            !parts.widgets.reindex_button.is_sensitive()
        });

        let parts = controller.state().get();
        assert!(!parts.widgets.reindex_button.is_sensitive());
        assert!(parts.widgets.progress_bar.is_visible());
    }

    #[gtk::test]
    fn indexing_status_dialog_empty_source_remains_expandable() {
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

        pump_main_context(|| {
            let parts = controller.state().get();
            !parts.model.source_rows.is_empty()
        });

        let parts = controller.state().get();
        assert!(parts.model.source_rows[0].expandable);
    }
}
