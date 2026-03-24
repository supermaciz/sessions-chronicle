use relm4::{ComponentController, adw};

use crate::indexing_worker::IndexingWorkerInput;
use crate::models::PerSourceResult;
use crate::ui::analytics_view::AnalyticsViewMsg;
use crate::ui::session_list::SessionListMsg;
use crate::ui::sidebar::SidebarMsg;

use super::super::App;
use super::super::helpers::{
    analytics_indexing_completion_outcome, banner_title, completion_toast_title,
    decide_reindex_action,
};
use super::super::types::ReindexAction;

impl App {
    pub(crate) fn handle_reindex_requested(&mut self) {
        match decide_reindex_action(self.indexing) {
            ReindexAction::AlreadyRunning => {
                self.toast_overlay.add_toast(
                    adw::Toast::builder()
                        .title("Indexing already in progress.")
                        .timeout(3)
                        .build(),
                );
            }
            ReindexAction::StartFull => {
                tracing::info!("Reindex requested — scheduling full background reindex");
                self.indexing = true;
                self.pending_reindex_feedback = true;
                self.session_list.emit(SessionListMsg::SetIndexing(true));
                self.indexing_worker
                    .emit(IndexingWorkerInput::StartFullReindex(self.sources.clone()));
            }
        }
    }

    pub(crate) fn handle_indexing_completed(
        &mut self,
        indexed: usize,
        _skipped: usize,
        per_source: Vec<PerSourceResult>,
    ) {
        tracing::info!(
            "Background indexing complete: indexed={}, skipped={}",
            indexed,
            _skipped
        );
        self.indexing = false;
        self.session_list.emit(SessionListMsg::SetIndexing(false));

        // Update banner state from per_source (Degraded/Failed only).
        let errors: usize = per_source.iter().map(|r| r.errors).sum();
        match banner_title(&per_source) {
            Some(title) => {
                self.banner.set_title(&title);
                self.banner.set_revealed(true);
            }
            None => {
                self.banner.set_revealed(false);
            }
        }

        // Push source status to sidebar dots.
        let source_results = per_source
            .iter()
            .map(|r| (r.assistant, r.clone()))
            .collect();
        self.sidebar
            .emit(SidebarMsg::SourceStatusesUpdated(source_results));

        self.session_list
            .emit(SessionListMsg::SetSourceResults(per_source.clone()));

        self.refresh_sidebar_projects();
        self.emit_session_list_filters();

        let analytics_outcome = analytics_indexing_completion_outcome(self.active_workspace);
        if analytics_outcome.mark_stale {
            self.analytics_view.emit(AnalyticsViewMsg::MarkStale);
        }
        if analytics_outcome.refresh_immediately {
            self.analytics_view.emit(AnalyticsViewMsg::Entered);
        }

        if self.pending_reindex_feedback {
            self.pending_reindex_feedback = false;
            let title = completion_toast_title(indexed, errors);
            let timeout = if errors > 0 { 5 } else { 3 };
            self.toast_overlay
                .add_toast(adw::Toast::builder().title(title).timeout(timeout).build());
        }
    }

    pub(crate) fn handle_indexing_failed(&mut self) {
        tracing::error!("Background indexing failed");
        self.indexing = false;
        self.session_list.emit(SessionListMsg::SetIndexing(false));

        let title = if self.pending_reindex_feedback {
            self.pending_reindex_feedback = false;
            "Failed to reset index"
        } else {
            "Background indexing failed"
        };

        self.toast_overlay
            .add_toast(adw::Toast::builder().title(title).timeout(3).build());
    }
}
