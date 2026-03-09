use relm4::{ComponentController, adw};

use crate::indexing_worker::IndexingWorkerInput;
use crate::ui::analytics_view::AnalyticsViewMsg;
use crate::ui::session_list::SessionListMsg;

use super::super::App;
use super::super::helpers::{analytics_indexing_completion_outcome, decide_reindex_action};
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

    pub(crate) fn handle_indexing_completed(&mut self, indexed: usize, skipped: usize) {
        tracing::info!(
            "Background indexing complete: indexed={}, skipped={}",
            indexed,
            skipped
        );
        self.indexing = false;
        self.session_list.emit(SessionListMsg::SetIndexing(false));
        self.session_list.emit(SessionListMsg::Reload);

        let analytics_outcome = analytics_indexing_completion_outcome(self.active_workspace);
        if analytics_outcome.mark_stale {
            self.analytics_view.emit(AnalyticsViewMsg::MarkStale);
        }
        if analytics_outcome.refresh_immediately {
            self.analytics_view.emit(AnalyticsViewMsg::Entered);
        }

        if self.pending_reindex_feedback {
            self.pending_reindex_feedback = false;
            self.toast_overlay.add_toast(
                adw::Toast::builder()
                    .title(format!("Index rebuilt — {} sessions", indexed))
                    .timeout(3)
                    .build(),
            );
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
