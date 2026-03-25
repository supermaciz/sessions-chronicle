use crate::models::{AiAssistant, PerSourceResult, SourceStatus};
use relm4::gtk::glib;

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
}
