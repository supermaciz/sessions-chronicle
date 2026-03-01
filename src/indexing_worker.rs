use std::path::PathBuf;

use relm4::{ComponentSender, Worker};

use crate::database::SessionIndexer;
use crate::database::indexer::TitleCandidate;
use crate::session_sources::SessionSources;
use crate::utils::title_generator::{TitleGenerationConfig, generate_title};

const MAX_TITLE_GENERATIONS_PER_RUN: usize = 25;

pub struct IndexingWorker {
    db_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct IndexingRequest {
    pub sources: SessionSources,
    pub title_generation: TitleGenerationConfig,
}

#[derive(Debug, Clone)]
pub enum IndexingWorkerInput {
    StartIncremental(IndexingRequest),
    StartFullReindex(IndexingRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexingWorkerOutput {
    Completed { indexed: usize, skipped: usize },
    Failed,
}

impl Worker for IndexingWorker {
    type Init = PathBuf;
    type Input = IndexingWorkerInput;
    type Output = IndexingWorkerOutput;

    fn init(init: Self::Init, _sender: ComponentSender<Self>) -> Self {
        Self { db_path: init }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        let result = (|| -> anyhow::Result<crate::database::IndexingOutcome> {
            let mut indexer = SessionIndexer::new(&self.db_path)?;
            let (outcome, title_generation) = match message {
                IndexingWorkerInput::StartIncremental(request) => {
                    let outcome = indexer.index_all_incremental(&request.sources)?;
                    (outcome, request.title_generation)
                }
                IndexingWorkerInput::StartFullReindex(request) => {
                    let outcome = indexer.index_all_full_reindex(&request.sources)?;
                    (outcome, request.title_generation)
                }
            };

            if title_generation.enabled {
                let backlog =
                    indexer.load_title_backlog_candidates(MAX_TITLE_GENERATIONS_PER_RUN)?;
                let selected_ids = select_candidate_ids(
                    &outcome.indexed_session_ids,
                    &backlog,
                    MAX_TITLE_GENERATIONS_PER_RUN,
                );

                for session_id in selected_ids {
                    let candidate = indexer.load_title_candidate(&session_id)?;
                    let Some(candidate) = candidate else {
                        continue;
                    };

                    let context = build_generation_context(&candidate);
                    match generate_title(&context, &title_generation) {
                        Some(title) => {
                            if let Err(err) = indexer.update_session_title(&session_id, &title) {
                                tracing::warn!(
                                    "Failed to update generated title for {}: {}",
                                    session_id,
                                    err
                                );
                            }
                        }
                        None => {
                            tracing::debug!(
                                "Title generation skipped/failed for session {}",
                                session_id
                            );
                        }
                    }
                }
            }

            Ok(outcome)
        })();

        match result {
            Ok(outcome) => {
                let _ = sender.output(IndexingWorkerOutput::Completed {
                    indexed: outcome.stats.indexed,
                    skipped: outcome.stats.skipped,
                });
            }
            Err(err) => {
                tracing::error!("Indexing worker failed: {}", err);
                let _ = sender.output(IndexingWorkerOutput::Failed);
            }
        }
    }
}

fn build_generation_context(candidate: &TitleCandidate) -> String {
    match &candidate.project_path {
        Some(project_path) => format!(
            "Project: {}\nFirst user message: {}",
            project_path, candidate.first_prompt
        ),
        None => candidate.first_prompt.clone(),
    }
}

fn select_candidate_ids(
    indexed_ids: &[String],
    backlog: &[TitleCandidate],
    cap: usize,
) -> Vec<String> {
    let mut selected: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for session_id in indexed_ids {
        if selected.len() >= cap {
            break;
        }
        if seen.insert(session_id.as_str()) {
            selected.push(session_id.clone());
        }
    }

    if selected.len() >= cap {
        return selected;
    }

    for candidate in backlog {
        if selected.len() >= cap {
            break;
        }
        if seen.insert(candidate.session_id.as_str()) {
            selected.push(candidate.session_id.clone());
        }
    }

    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_selection_prioritizes_indexed_ids_then_backlog_until_cap() {
        let indexed = vec!["s-index-1".to_string(), "s-index-2".to_string()];
        let backlog = vec![
            TitleCandidate {
                session_id: "s-index-2".to_string(),
                first_prompt: "Duplicate".to_string(),
                project_path: None,
            },
            TitleCandidate {
                session_id: "s-backlog-1".to_string(),
                first_prompt: "Backlog 1".to_string(),
                project_path: None,
            },
            TitleCandidate {
                session_id: "s-backlog-2".to_string(),
                first_prompt: "Backlog 2".to_string(),
                project_path: None,
            },
        ];

        let selected = select_candidate_ids(&indexed, &backlog, 3);

        assert_eq!(
            selected,
            vec![
                "s-index-1".to_string(),
                "s-index-2".to_string(),
                "s-backlog-1".to_string(),
            ]
        );
    }
}
