use std::path::PathBuf;

use relm4::{ComponentSender, Worker};

use crate::database::SessionIndexer;
use crate::models::{IndexingError, PerSourceResult};
use crate::session_sources::SessionSources;

pub struct IndexingWorker {
    db_path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum IndexingWorkerInput {
    StartIncremental(SessionSources),
    StartFullReindex(SessionSources),
}

#[derive(Debug, Clone)]
pub enum IndexingWorkerOutput {
    Completed {
        indexed: usize,
        skipped: usize,
        per_source: Vec<PerSourceResult>,
        errors_detail: Vec<IndexingError>,
    },
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
        let result = (|| -> anyhow::Result<crate::models::IndexingRunResult> {
            let mut indexer = SessionIndexer::new(&self.db_path)?;
            match message {
                IndexingWorkerInput::StartIncremental(sources) => {
                    indexer.index_all_incremental(&sources)
                }
                IndexingWorkerInput::StartFullReindex(sources) => {
                    indexer.index_all_full_reindex(&sources)
                }
            }
        })();

        match result {
            Ok(run_result) => {
                let _ = sender.output(IndexingWorkerOutput::Completed {
                    indexed: run_result.totals.indexed,
                    skipped: run_result.totals.skipped,
                    per_source: run_result.per_source,
                    errors_detail: run_result.errors_detail,
                });
            }
            Err(err) => {
                tracing::error!("Indexing worker failed: {}", err);
                let _ = sender.output(IndexingWorkerOutput::Failed);
            }
        }
    }
}
